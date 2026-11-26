//! pane 하나의 채팅 화면(TS `PaneChatView`). 그 pane의 CLI 상태를 주기적으로 읽어 대화·선택 화면·입력창을 그리고,
//! 동작은 확인된 같은 CLI 대상(대상 토큰)에만 보낸다. 숨겨진 동안에는 읽지 않고, 다시 보이면 대상을 새로 확인한 뒤에만 입력을 받는다.

use std::collections::HashSet;

use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, ScrollHandle, Task, Window,
};
use kanvibe_ai::pane_chat::snapshot::PaneChatSnapshot;
use kanvibe_domain::i18n::Translator;

use super::client::{PaneChatClient, PaneChatResult, RevealOutcome};
use super::composer::ComposerState;
use super::model_picker::ModelPickerState;
use super::preferences::PaneChatPreferences;
use super::prompt_card::PromptCardState;
use super::session::ChatSession;
use super::transcript::TranscriptPages;
use crate::components::text_context_menu::OpenTextMenu;
use crate::qa::perf_markers::{ScreenPaintMarks, mark_screen_phase};

/// 성능 표식의 화면 이름
pub(super) const PERF_SCREEN: &str = "pane_chat";

/// 화면을 만들 때 넘기는 값
pub struct PaneChatViewProps {
    pub client: PaneChatClient,
    pub task_id: String,
    /// 없으면 그 작업의 활성 pane을 따른다
    pub pane_id: Option<String>,
    pub task_title: Option<String>,
    pub translator: Translator,
    pub preferences: Entity<PaneChatPreferences>,
    /// 층이 자동으로 열기 전에 방금 확인한 이 pane의 상태. 있으면 첫 화면부터 그 대화를 보이고, 곧바로 다시 읽어 바꾼다
    pub initial_snapshot: Option<PaneChatSnapshot>,
}

/// 부모(pane 채팅 층)에 알리는 사용자 요청
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneChatViewEvent {
    /// "터미널로 전환"
    OpenTerminal,
    /// "히스토리"
    OpenHistory,
}

pub struct PaneChatView {
    pub(super) client: PaneChatClient,
    pub(super) translator: Translator,
    pub(super) preferences: Entity<PaneChatPreferences>,
    pub(super) task_title: Option<String>,
    pub(super) session: ChatSession,
    pub(super) transcript: TranscriptPages,
    pub(super) is_visible: bool,
    pub(super) is_window_active: bool,
    pub(super) focus_handle: FocusHandle,
    pub(super) reveal_outcome: Option<RevealOutcome>,
    /// 대화를 위로 올려 최신 메시지를 따라가지 않는 중
    pub(super) is_detached: bool,
    /// 휠을 굴린 뒤 다음 그리기에서 따라가기 여부를 다시 정한다
    pub(super) needs_detach_check: bool,
    pub(super) transcript_scroll: ScrollHandle,
    /// "현재 CLI 화면" 펼침
    pub(super) is_screen_open: bool,
    pub(super) open_disclosures: HashSet<String>,
    /// 작업 중 표시의 경과 초. 작업이 끝나면 지우고 다음 작업에서 0부터 센다
    pub(super) working_seconds: Option<u64>,
    /// 대상이 바뀌면 열린 팝오버·결과 안내를 닫는 기준(TS `targetKey`)
    target_key: String,
    pub(super) composer: ComposerState,
    pub(super) prompt_card: PromptCardState,
    pub(super) model_picker: Option<ModelPickerState>,
    /// 채팅 칸 안에서 우클릭으로 연 글자 메뉴
    pub(super) text_menu: Option<OpenTextMenu>,
    pub(super) text_menu_focus: FocusHandle,
    is_reading: bool,
    /// 읽는 중에 연결 신호가 왔다. 지금 읽기가 끝나면 주기를 기다리지 않고 한 번 더 읽는다
    needs_signal_read: bool,
    /// 성능 표식(`--perf-markers`)이 기다리는 첫 그리기
    pub(super) paint_marks: ScreenPaintMarks,
    /// 첫 그리기에 최신 메시지만 넣는 중. 열거나 다시 보이거나 대화가 바뀌면 켜고, 메시지를 그린 다음 프레임에 끈다
    pub(super) is_first_paint_limited: bool,
    /// 대화 대상이 바뀐 뒤 보낼 수 있는 화면이 그려지기를 기다리는 중(성능 표식)
    pub(super) is_retarget_mark_pending: bool,
    _read: Option<Task<()>>,
    _poll_timer: Option<Task<()>>,
    _working_timer: Option<Task<()>>,
    _connection_signals: Option<Task<()>>,
    pub(super) _action: Option<Task<()>>,
    pub(super) _older: Option<Task<()>>,
    _activation: Option<gpui_kit::Subscription>,
    _preferences: gpui_kit::Subscription,
}

impl EventEmitter<PaneChatViewEvent> for PaneChatView {}

impl Focusable for PaneChatView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl PaneChatView {
    pub fn new(props: PaneChatViewProps, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = ComposerState::new(&props.translator, window, cx);
        let activation = cx.observe_window_activation(window, |view, window, cx| {
            view.is_window_active = window.is_window_active();
            view.restart_reading(window, cx);
        });
        let preferences = cx.observe(&props.preferences, |_, _, cx| cx.notify());
        // 화면이 닫히면 보내는 중이 아닌 초안의 올린 첨부를 서비스에서 지운다
        cx.on_release(|view, cx| view.discard_abandoned_drafts(cx))
            .detach();
        let target_key = format!("{}:unavailable", props.task_id);
        let mut view = Self {
            client: props.client,
            translator: props.translator,
            preferences: props.preferences,
            task_title: props.task_title,
            session: ChatSession::new(props.task_id, props.pane_id),
            transcript: TranscriptPages::default(),
            is_visible: true,
            is_window_active: window.is_window_active(),
            focus_handle: cx.focus_handle(),
            reveal_outcome: None,
            is_detached: false,
            needs_detach_check: false,
            transcript_scroll: ScrollHandle::new(),
            is_screen_open: false,
            open_disclosures: HashSet::new(),
            working_seconds: None,
            target_key,
            composer,
            prompt_card: PromptCardState::default(),
            model_picker: None,
            text_menu: None,
            text_menu_focus: cx.focus_handle(),
            is_reading: false,
            needs_signal_read: false,
            paint_marks: ScreenPaintMarks::default(),
            is_first_paint_limited: true,
            is_retarget_mark_pending: false,
            _read: None,
            _poll_timer: None,
            _working_timer: None,
            _connection_signals: None,
            _action: None,
            _older: None,
            _activation: Some(activation),
            _preferences: preferences,
        };
        view.paint_marks.arm(PERF_SCREEN, "open");
        if let Some(snapshot) = props.initial_snapshot {
            view.apply_snapshot(snapshot, window, cx);
            view.composer.sync_enabled(&view.session, cx);
        }
        view.listen_for_connection_signals(window, cx);
        view.restart_reading(window, cx);
        view
    }

    /// CLI가 새 대화 ID를 알리면(hook, 세션 기록 파일) 다음 조회 주기를 기다리지 않고 신호마다 한 번 다시 읽는다
    fn listen_for_connection_signals(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut signals) = self.client.watch_connection(&self.session.task_id) else {
            return;
        };
        self._connection_signals = Some(cx.spawn_in(window, async move |this, cx| {
            while signals.next_signal().await.is_some() {
                let handled = this.update_in(cx, |view, window, cx| {
                    view.handle_connection_signal(window, cx)
                });
                if handled.is_err() {
                    break;
                }
            }
        }));
    }

    /// 보이지 않거나 창이 비활성이면 넘긴다. 다시 보이거나 창이 활성이 되면 어차피 새로 읽는다.
    /// 읽는 중이면 그 결과가 신호 전의 상태일 수 있으므로, 끝난 뒤 한 번만 더 읽는다
    fn handle_connection_signal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_visible || !self.is_window_active {
            return;
        }
        mark_screen_phase(PERF_SCREEN, "signal");
        match self.is_reading {
            true => self.needs_signal_read = true,
            false => self.refresh(window, cx),
        }
    }

    pub fn task_id(&self) -> &str {
        &self.session.task_id
    }

    pub fn pane_id(&self) -> Option<&str> {
        self.session.pane_id.as_deref()
    }

    pub fn snapshot(&self) -> Option<&PaneChatSnapshot> {
        self.session.snapshot()
    }

    pub fn is_visible(&self) -> bool {
        self.is_visible
    }

    /// 탭이 숨거나 다시 보일 때. 숨긴 동안에는 읽지 않고 동작 대상도 잊는다
    pub fn set_visible(&mut self, is_visible: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_visible == is_visible {
            return;
        }
        self.is_visible = is_visible;
        if is_visible {
            self.paint_marks.arm(PERF_SCREEN, "shown");
            // 읽던 위치로 돌아가는 대화는 위쪽 메시지가 보일 수 있으므로 전부 그린다
            self.is_first_paint_limited = !self.is_detached;
        }
        self.restart_reading(window, cx);
        cx.notify();
    }

    /// 보이기·창 활성 상태가 바뀌면 진행 중인 읽기를 버리고 새 세대로 다시 읽는다
    fn restart_reading(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._read = None;
        self._poll_timer = None;
        self.is_reading = false;
        self.needs_signal_read = false;
        if !self.is_visible {
            self.session.hide();
            self.composer.sync_enabled(&self.session, cx);
            return;
        }
        self.session.show();
        self.refresh(window, cx);
    }

    /// 지금 pane 상태를 읽는다. 이미 읽는 중이면 그 결과를 기다린다(TS `refresh`)
    pub fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_visible || self.is_reading {
            return;
        }
        self._poll_timer = None;
        self.is_reading = true;
        mark_screen_phase(PERF_SCREEN, "read_start");
        let generation = self.session.generation;
        let read =
            self.client
                .get_snapshot(&self.session.task_id, self.session.pane_id.as_deref(), cx);
        self._read = Some(cx.spawn_in(window, async move |this, cx| {
            let result = read.await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.finish_read(generation, result, window, cx)
            });
        }));
    }

    fn finish_read(
        &mut self,
        generation: u64,
        result: PaneChatResult<PaneChatSnapshot>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if generation != self.session.generation {
            return;
        }
        self.is_reading = false;
        mark_screen_phase(PERF_SCREEN, "read_done");
        match result {
            Ok(snapshot) => self.apply_snapshot(snapshot, window, cx),
            Err(_) => self.session.apply_read_failure(),
        }
        self.composer.sync_enabled(&self.session, cx);
        self.sync_working_timer(cx);
        match std::mem::take(&mut self.needs_signal_read) {
            true => self.refresh(window, cx),
            false => self.schedule_poll(window, cx),
        }
        cx.notify();
    }

    fn apply_snapshot(
        &mut self,
        snapshot: PaneChatSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.transcript.absorb_latest(&snapshot);
        let next_target_key = snapshot
            .target_token
            .clone()
            .unwrap_or_else(|| format!("{}:unavailable", self.session.task_id));
        self.session.apply_snapshot(snapshot);
        if next_target_key != self.target_key {
            self.is_retarget_mark_pending = ScreenPaintMarks::is_enabled();
            self.target_key = next_target_key;
            self.reset_for_new_target();
        }
        let placeholder = self.translator.translate("paneChat.promptTextPlaceholder");
        self.prompt_card
            .sync(self.session.snapshot(), placeholder.into(), window, cx);
        self.sync_composer_target(window, cx);
    }

    /// 답을 보낸 뒤 다시 읽은 화면. 주기 조회와 같은 자리로 넣는다
    pub(super) fn apply_snapshot_after_answer(
        &mut self,
        snapshot: PaneChatSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_snapshot(snapshot, window, cx);
        cx.notify();
    }

    /// 다른 CLI 대상으로 바뀌면 따라가기·팝오버·결과 안내를 처음 상태로 둔다
    fn reset_for_new_target(&mut self) {
        self.is_detached = false;
        self.is_first_paint_limited = true;
        self.model_picker = None;
        self.reveal_outcome = None;
        self.transcript_scroll.scroll_to_bottom();
    }

    fn schedule_poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_visible || !self.is_window_active {
            return;
        }
        let delay = self.session.poll_delay();
        let generation = self.session.generation;
        self._poll_timer = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update_in(cx, |view, window, cx| {
                if view.session.generation == generation {
                    view.refresh(window, cx);
                }
            });
        }));
    }

    /// 작업 중 표시가 처음 뜰 때 경과 초를 0부터 세고, 사라지면 멈춘다
    pub(super) fn sync_working_timer(&mut self, cx: &mut Context<Self>) {
        let is_shown = self.session.is_agent_working()
            && self
                .session
                .snapshot()
                .is_some_and(|snapshot| snapshot.can_send);
        match (is_shown, self.working_seconds.is_some()) {
            (true, false) => {
                self.working_seconds = Some(0);
                self._working_timer = Some(cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor()
                            .timer(std::time::Duration::from_secs(1))
                            .await;
                        let is_alive = this.update(cx, |view, cx| {
                            if let Some(seconds) = view.working_seconds.as_mut() {
                                *seconds += 1;
                            }
                            cx.notify();
                        });
                        if is_alive.is_err() {
                            break;
                        }
                    }
                }));
            }
            (false, true) => {
                self.working_seconds = None;
                self._working_timer = None;
            }
            _ => {}
        }
    }

    /// 대화 영역에 들어갈 입력 포커스. 입력창을 쓸 수 있으면 입력창, 아니면 화면 자체다
    pub fn focus_input(&self, window: &mut Window, cx: &mut App) {
        match self.composer.can_take_focus(&self.session, self.is_visible) {
            true => self.composer.focus(window, cx),
            false => window.focus(&self.focus_handle, cx),
        }
    }

    /// "터미널로 전환"
    pub fn open_terminal(&mut self, cx: &mut Context<Self>) {
        cx.emit(PaneChatViewEvent::OpenTerminal);
    }

    /// "히스토리"
    pub fn open_history(&mut self, cx: &mut Context<Self>) {
        cx.emit(PaneChatViewEvent::OpenHistory);
    }

    /// "현재 CLI 화면"을 펼치거나 접는다
    pub fn toggle_screen(&mut self, cx: &mut Context<Self>) {
        self.is_screen_open = !self.is_screen_open;
        cx.notify();
    }

    pub fn toggle_disclosure(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.open_disclosures.remove(key) {
            self.open_disclosures.insert(key.to_string());
        }
        cx.notify();
    }
}
