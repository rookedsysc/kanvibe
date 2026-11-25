//! 확인된 CLI 대상에 보내는 동작(TS `usePaneChat`의 `run`·`send`·`answerPrompt`·`revealCodexThread`와 `PaneChatView`의 버튼).
//! 한 번에 하나만 보내고, 보내는 동안 대상이 바뀌면 그 결과의 오류는 새 대상 화면에 보이지 않는다.

use gpui_kit::{Context, Window};
use kanvibe_ai::pane_chat::snapshot::{PaneChatPrompt, get_pane_chat_prompt_dialog_key};
use kanvibe_domain::pane_chat::PaneChatPromptAnswer;

use super::client::{OutgoingMessage, PaneChatResult};
use super::model_picker::ModelPickerState;
use super::session::PromptError;
use super::view::PaneChatView;

/// 보낸 결과. 받아들였는지와 전달됐을 수도 있는지
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendOutcome {
    pub is_accepted: bool,
}

impl PaneChatView {
    /// 동작이 끝난 뒤 공통 정리. 같은 세대면 곧바로 다시 읽는다
    pub(super) fn finish_action(
        &mut self,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session.finish_action();
        if self.session.generation == generation {
            self.refresh(window, cx);
        }
        self.composer.sync_enabled(&self.session, cx);
        self.sync_working_timer(cx);
        cx.notify();
    }

    /// 대상 토큰이 필요한 단순 동작(중단·CLI 모델 선택기 열기). 받아들였으면 `on_accepted`를 부른다
    fn run_simple_action(
        &mut self,
        start: impl FnOnce(&Self, &str, &mut Context<Self>) -> gpui_kit::Task<PaneChatResult<bool>>,
        on_accepted: fn(&mut Self, &mut Context<Self>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_visible {
            return;
        }
        let Some(token) = self.session.begin_action() else {
            return;
        };
        self.session.action_error = None;
        let generation = self.session.generation;
        let request = start(self, &token, cx);
        self._action = Some(cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            let _ = this.update_in(cx, |view, window, cx| {
                let is_same = view.session.is_same_target(generation, &token);
                match result {
                    Ok(true) if is_same => on_accepted(view, cx),
                    Err(error) if is_same => view.session.action_error = Some(error.message),
                    _ => {}
                }
                view.finish_action(generation, window, cx);
            });
        }));
        cx.notify();
    }

    /// "중단"과 작업 중 전송 자리의 응답 중단
    pub fn interrupt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run_simple_action(
            |view, token, cx| view.client.interrupt(&view.session.task_id, token, cx),
            |_, _| {},
            window,
            cx,
        );
    }

    /// 모델 버튼. 모델 목록을 알거나 API로 바꿀 수 있으면 채팅 안 팝오버를 열고, 아니면 CLI의 모델 선택기를 열고 터미널로 바꾼다
    pub fn open_model_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = self.session.snapshot() else {
            return;
        };
        let has_popover = snapshot.model_settings.is_some()
            || snapshot.model_picker.is_api_supported == Some(true);
        if has_popover {
            self.toggle_model_popover(window, cx);
            return;
        }
        self.run_simple_action(
            |view, token, cx| {
                view.client
                    .open_model_picker(&view.session.task_id, token, cx)
            },
            |view, cx| view.open_terminal(cx),
            window,
            cx,
        );
    }

    fn toggle_model_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model_picker.is_some() {
            self.model_picker = None;
            cx.notify();
            return;
        }
        let Some(snapshot) = self.session.snapshot() else {
            return;
        };
        let Some(token) = snapshot.target_token.clone() else {
            return;
        };
        let initial = snapshot.model_settings.clone();
        let picker = ModelPickerState::new(token, initial, cx);
        self.model_picker = Some(picker);
        self.load_model_settings(window, cx);
        cx.notify();
    }

    /// 메시지를 보낸다(TS `send`). 대화 글은 기록되기 전까지 말풍선으로 보이고, 슬래시 명령은 말풍선을 만들지 않는다
    pub(super) fn send_message(
        &mut self,
        message: OutgoingMessage,
        on_settled: impl FnOnce(&mut Self, SendOutcome, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_visible {
            return false;
        }
        let now = self.now_iso(cx);
        self.session
            .add_pending(&message.request_id, &message.text, now);
        let Some(token) = self.session.begin_action() else {
            self.session
                .settle_pending(&message.request_id, false, false);
            return false;
        };
        self.session.action_error = None;
        let generation = self.session.generation;
        let request = self
            .client
            .send_prompt(&self.session.task_id, &token, &message, cx);
        let request_id = message.request_id.clone();
        let abandoned = AbandonedAttachments {
            client: self.client.clone(),
            task_id: self.session.task_id.clone(),
            token: token.clone(),
            attachment_ids: message.attachment_ids.clone(),
        };
        // 결과를 알 때까지 첨부를 지우지 않도록, 화면이 닫혀도 전송 결과는 끝까지 기다린다
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            let is_open = this
                .update_in(cx, |view, window, cx| {
                    let outcome = view.settle_send(generation, &token, &request_id, result);
                    view.finish_action(generation, window, cx);
                    on_settled(view, outcome, window, cx);
                })
                .is_ok();
            if !is_open {
                abandoned.discard(cx).await;
            }
        })
        .detach();
        self.sync_working_timer(cx);
        cx.notify();
        true
    }

    /// 전송 결과를 말풍선에 남긴다. 같은 대상일 때만 거절 이유를 보이고, 전달됐을 수도 있는 글은 말풍선을 남긴다
    fn settle_send(
        &mut self,
        generation: u64,
        token: &str,
        request_id: &str,
        result: PaneChatResult<bool>,
    ) -> SendOutcome {
        let is_accepted = matches!(result, Ok(true));
        let mut may_have_been_delivered = false;
        if let Err(error) = result {
            may_have_been_delivered = error.is_delivery_uncertain;
            if self.session.is_same_target(generation, token) {
                self.session.action_error = Some(error.message);
            }
        }
        self.session
            .settle_pending(request_id, is_accepted, may_have_been_delivered);
        SendOutcome { is_accepted }
    }

    fn now_iso(&self, cx: &Context<Self>) -> String {
        let clock = cx
            .try_global::<crate::services::AppServices>()
            .map(|services| services.clock)
            .unwrap_or_else(crate::qa::AppClock::system);
        kanvibe_ai::javascript::iso_string_from_ms(clock.now_ms() as f64).unwrap_or_default()
    }

    /// "대화 ID 표시". 대상 토큰이 아직 없으므로 서비스가 pane 기준으로 다시 확인한다.
    /// 붙여넣기·Enter 전 확인(버전, 빈 입력란, 같은 pane·프로세스)은 모두 서비스의 승인된 예외 경로가 한다
    pub fn reveal_codex_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_visible || !self.session.begin_reveal() {
            return;
        }
        let generation = self.session.generation;
        let request = self.client.reveal_codex_thread(
            &self.session.task_id,
            self.session.pane_id.as_deref(),
            cx,
        );
        self._action = Some(cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            let _ = this.update_in(cx, |view, window, cx| {
                match result {
                    Ok(outcome) => view.reveal_outcome = Some(outcome),
                    Err(error) if view.session.generation == generation => {
                        view.reveal_outcome = None;
                        view.session.action_error = Some(error.message);
                    }
                    Err(_) => view.reveal_outcome = None,
                }
                view.finish_action(generation, window, cx);
            });
        }));
        cx.notify();
    }

    /// 선택 화면에 답을 보낸다. 보내는 동안 누른 다중 선택의 다른 선택지는 기억했다가 같은 화면에 그대로 있으면 차례로 보낸다
    pub fn answer_prompt(
        &mut self,
        prompt: PaneChatPrompt,
        answer: PaneChatPromptAnswer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.session.is_busy {
            if let PaneChatPromptAnswer::Option { number } = answer {
                self.session.queue_prompt_option(&prompt, u64::from(number));
            }
            return;
        }
        let option_number = match answer {
            PaneChatPromptAnswer::Option { number } => Some(u64::from(number)),
            _ => None,
        };
        if !self.is_visible {
            return;
        }
        let Some(token) = self.session.begin_action() else {
            return;
        };
        self.session.prompt_error = None;
        self.session.set_sending_prompt_option(option_number);
        let dialog_key = get_pane_chat_prompt_dialog_key(&prompt);
        let generation = self.session.generation;
        let request =
            self.client
                .answer_prompt(&self.session.task_id, &token, &prompt.id, &answer, cx);
        self._action = Some(cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.finish_answer(generation, token, dialog_key, result, window, cx)
            });
        }));
        cx.notify();
    }

    /// 답을 보낸 뒤 바뀐 화면을 받아 온 다음에야 바쁨을 풀어, 낡은 카드의 답이 나가지 않게 한다
    fn finish_answer(
        &mut self,
        generation: u64,
        token: String,
        dialog_key: String,
        result: PaneChatResult<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = &result
            && self.session.is_same_target(generation, &token)
        {
            self.session.prompt_error = Some(PromptError {
                dialog_key,
                message: error.message.clone(),
            });
        }
        let is_accepted = matches!(result, Ok(true));
        let read =
            self.client
                .get_snapshot(&self.session.task_id, self.session.pane_id.as_deref(), cx);
        self._action = Some(cx.spawn_in(window, async move |this, cx| {
            let snapshot = read.await;
            let _ = this.update_in(cx, |view, window, cx| {
                if view.session.generation == generation
                    && let Ok(snapshot) = snapshot
                {
                    view.apply_snapshot_after_answer(snapshot, window, cx);
                }
                view.continue_queued_answers(generation, is_accepted, window, cx);
            });
        }));
    }

    fn continue_queued_answers(
        &mut self,
        generation: u64,
        is_accepted: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session.set_sending_prompt_option(None);
        let is_same_generation = self.session.generation == generation;
        let accepted = is_accepted && is_same_generation;
        let next = self.session.next_queued_prompt_option(accepted);
        self.finish_action(generation, window, cx);
        if let Some((shown, number)) = next {
            let number = u8::try_from(number).unwrap_or(u8::MAX);
            self.answer_prompt(shown, PaneChatPromptAnswer::Option { number }, window, cx);
        }
    }

    /// "이전 메시지 더 보기"
    pub fn load_older_messages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = self.session.snapshot().cloned() else {
            return;
        };
        let (Some(token), Some(cursor)) = (
            snapshot.target_token.clone(),
            self.transcript.next_cursor(Some(&snapshot)),
        ) else {
            return;
        };
        let Some(key) = self.transcript.begin_loading(&snapshot) else {
            return;
        };
        let request = self
            .client
            .get_transcript_page(&snapshot.task_id, &token, &cursor, cx);
        self._older = Some(cx.spawn_in(window, async move |this, cx| {
            let result = request.await.map_err(|_| ());
            let _ = this.update_in(cx, |view, _, cx| {
                view.transcript
                    .finish_loading(&key, view.session.snapshot(), result);
                cx.notify();
            });
        }));
        cx.notify();
    }
}

/// 화면이 닫힌 뒤 결과가 온 전송의 첨부. 그 초안은 더 쓰일 곳이 없어 서비스에서 지운다
struct AbandonedAttachments {
    client: super::client::PaneChatClient,
    task_id: String,
    token: String,
    attachment_ids: Vec<String>,
}

impl AbandonedAttachments {
    /// 창이 닫혔어도 앱 문맥으로 지운다
    async fn discard(self, app: &gpui_kit::AsyncApp) {
        for attachment_id in &self.attachment_ids {
            let removal = app.update(|cx| {
                self.client
                    .remove_attachment(&self.task_id, &self.token, attachment_id, cx)
            });
            let _ = removal.await;
        }
    }
}
