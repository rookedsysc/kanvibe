//! 입력창(TS `PaneChatComposer`·`usePaneChatComposer`). 초안은 대화마다 따로 두고, 보낸 글은 CLI가 받아들인 뒤에만 지운다.
//! 받아들이기 전에 새로 적은 글과 새 첨부는 남긴다. 같은 글·첨부로 다시 보내면 같은 요청 식별값을 써서 두 번 넣지 않는다.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::base::input::{InputEvent, InputState, TextareaState};
use gpui_kit::{
    App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable, Pixels, Point, SharedString,
    Subscription, Task, Window,
};
use kanvibe_ai::pane_chat::snapshot::PaneChatInputState;
use kanvibe_domain::i18n::Translator;

use super::actions::SendOutcome;
use super::client::OutgoingMessage;
use super::drafts::{Draft, DraftError, DraftOwner, DraftRequest, Drafts, draft_key};
use super::session::{ChatSession, new_request_id};
use super::suggestions::{Completion, SuggestionState, completion_at};
use super::view::PaneChatView;

pub const COMPOSER_MIN_HEIGHT: f32 = 64.0;
pub const COMPOSER_RESIZE_STEP: f32 = 24.0;
const COMPOSER_MAX_HEIGHT_RATIO: f32 = 0.6;

pub struct ComposerState {
    pub textarea: Entity<TextareaState>,
    pub skill_search: Entity<InputState>,
    pub drafts: Drafts,
    /// 지금 입력창에 보이는 초안
    pub draft_key: String,
    /// 커서 위치(바이트)
    pub caret: usize,
    pub suggestions: SuggestionState,
    pub input_height: f32,
    pub model_trigger_focus: FocusHandle,
    pub separator_focus: FocusHandle,
    pub(super) model_trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// 대화 화면 높이. 입력창은 그 60%까지 키울 수 있다
    pub(super) section_height: Rc<Cell<f32>>,
    /// 테두리를 끌기 시작한 마우스 높이와 그때의 입력창 높이
    pub(super) resize_drag: Option<(f32, f32)>,
    /// 마지막으로 본 CLI 대상. 바뀌면 메뉴 상태를 처음으로 되돌린다
    last_target: Option<String>,
    pub(super) next_local_id: u64,
    /// 입력란에 마지막으로 건 쓰기 막음(TS `disabled={!canEdit}`). 같으면 다시 걸지 않는다
    is_textarea_disabled: Option<bool>,
    pub(super) _query: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl ComposerState {
    pub fn new(
        translator: &Translator,
        window: &mut Window,
        cx: &mut Context<PaneChatView>,
    ) -> Self {
        let placeholder = SharedString::from(translator.translate("paneChat.placeholder"));
        let textarea = cx.new(|cx| {
            // 우클릭 메뉴는 채팅 칸이 연다(`text_menu`). 입력란 기본 메뉴는 macOS에서 두 번째 메뉴를 띄운다
            TextareaState::new(window, cx)
                .placeholder(placeholder)
                .submit_on_enter(true)
                .context_menu(false)
        });
        let search_placeholder = SharedString::from(translator.translate("paneChat.searchSkills"));
        let skill_search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(search_placeholder)
                .context_menu(false)
        });
        let subscriptions = subscribe_inputs(&textarea, &skill_search, window, cx);
        Self {
            textarea,
            skill_search,
            drafts: Drafts::default(),
            draft_key: String::new(),
            caret: 0,
            suggestions: SuggestionState::default(),
            input_height: COMPOSER_MIN_HEIGHT,
            model_trigger_focus: cx.focus_handle(),
            separator_focus: cx.focus_handle(),
            model_trigger_bounds: Rc::new(Cell::new(None)),
            section_height: Rc::new(Cell::new(0.0)),
            resize_drag: None,
            last_target: None,
            next_local_id: 0,
            is_textarea_disabled: None,
            _query: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn draft(&self) -> Draft {
        self.drafts
            .get(&self.draft_key)
            .cloned()
            .unwrap_or_default()
    }

    /// 입력창에 글을 쓸 수 있는지(TS `canEdit`). 대상이 확인된 뒤에만 받는다
    pub fn can_edit(session: &ChatSession) -> bool {
        session.is_ready()
            && session
                .snapshot()
                .is_some_and(|snapshot| snapshot.target_token.is_some())
    }

    pub fn sync_enabled(&mut self, session: &ChatSession, cx: &mut App) {
        let is_disabled = !Self::can_edit(session);
        if self.is_textarea_disabled == Some(is_disabled) {
            return;
        }
        self.is_textarea_disabled = Some(is_disabled);
        // TS의 disabled 입력란은 자리 표시 글자를 흐리게 하지 않는다. GPUI의 쓰기 막음은 글자를 반투명하게 그리므로
        // 읽기 전용으로 막아 같은 색을 지킨다
        self.textarea
            .update(cx, |state, cx| state.set_readonly(is_disabled, cx));
    }

    /// 입력란에 글을 쓸 수 있는지(TS 입력란의 `disabled`가 아닌지)
    pub fn is_editable(&self) -> bool {
        self.is_textarea_disabled == Some(false)
    }

    pub fn can_take_focus(&self, session: &ChatSession, is_visible: bool) -> bool {
        is_visible && Self::can_edit(session)
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        let handle = self.textarea.focus_handle(cx);
        window.focus(&handle, cx);
    }

    /// 마우스가 모델 버튼 위인지. 팝오버 바깥 누르기에서 그 버튼은 뺀다
    pub fn is_model_trigger_at(&self, position: Point<Pixels>) -> bool {
        self.model_trigger_bounds
            .get()
            .is_some_and(|bounds| bounds.contains(&position))
    }

    /// 입력창을 키울 수 있는 최대 높이(대화 화면의 60%, 최소 높이보다 작지 않게)
    pub fn maximum_height(&self, window: &Window) -> f32 {
        let section = self.section_height.get();
        let pane_height = match section > 0.0 {
            true => section,
            false => f32::from(window.viewport_size().height),
        };
        COMPOSER_MIN_HEIGHT.max((pane_height * COMPOSER_MAX_HEIGHT_RATIO).floor())
    }
}

/// 입력란의 글 바뀜·Enter·커서 이동과 스킬 검색란의 글·Enter를 화면 동작으로 잇는다
fn subscribe_inputs(
    textarea: &Entity<TextareaState>,
    skill_search: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<PaneChatView>,
) -> Vec<Subscription> {
    vec![
        cx.subscribe_in(textarea, window, |view, _, event, window, cx| match event {
            InputEvent::Change => view.handle_text_change(window, cx),
            InputEvent::PressEnter { shift: false, .. } => view.handle_enter(window, cx),
            _ => {}
        }),
        cx.subscribe_in(
            skill_search,
            window,
            |view, _, event, window, cx| match event {
                InputEvent::Change => view.handle_skill_query_change(window, cx),
                InputEvent::PressEnter { .. } => {
                    view.handle_completion_key(CompletionKey::Enter, window, cx);
                }
                _ => {}
            },
        ),
        cx.observe_in(textarea, window, |view, _, window, cx| {
            view.handle_caret_move(window, cx)
        }),
    ]
}

/// 메뉴가 받는 키
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKey {
    Up,
    Down,
    Tab,
    Enter,
    Escape,
}

/// 지금 메뉴 상태(TS `completion`·`completionKey`·`catalogKey`)
pub struct CompletionContext {
    pub completion: Option<Completion>,
    pub completion_key: Option<String>,
    pub catalog_key: Option<String>,
    pub is_shown: bool,
}

impl PaneChatView {
    fn target_token(&self) -> Option<String> {
        self.session
            .snapshot()
            .and_then(|snapshot| snapshot.target_token.clone())
    }

    /// 입력이 CLI에 바로 들어갈 수 있는지(TS `inputReady`). API 대화는 작업 중에도 받지만 슬래시 명령은 빈 입력란에서만 넣는다
    pub fn is_input_ready(&self) -> bool {
        let Some(snapshot) = self.session.snapshot() else {
            return false;
        };
        let can_send_while_busy = snapshot
            .composer
            .as_ref()
            .and_then(|composer| composer.can_send_while_busy)
            == Some(true);
        let is_command = self.composer.draft().text.trim_start().starts_with('/');
        snapshot.input_state == PaneChatInputState::Ready || (can_send_while_busy && !is_command)
    }

    /// 보내기·모델·중단을 막는 상태(TS `disabled`)
    pub fn is_composer_disabled(&self) -> bool {
        !self.session.is_ready()
            || !self
                .session
                .snapshot()
                .is_some_and(|snapshot| snapshot.can_send)
            || self.session.has_read_error
            || self.session.is_busy
    }

    /// 스킬 검색 메뉴면 검색어로, 아니면 커서 앞 낱말로 찾은 자동 완성
    fn current_completion(&self, text: &str, caret: usize) -> Option<Completion> {
        let suggestions = &self.composer.suggestions;
        match suggestions.is_skill_picker_open {
            true => Some(Completion {
                kind: super::client::SuggestionKind::Skill,
                query: suggestions.skill_query.clone(),
                start: caret,
                end: caret,
            }),
            false => completion_at(text, caret),
        }
    }

    /// 지금 CLI가 그 종류의 자동 완성을 받는지(파일 언급 또는 스킬·명령)
    fn is_completion_supported(&self, completion: &Completion) -> bool {
        let capabilities = self
            .session
            .snapshot()
            .and_then(|snapshot| snapshot.composer.as_ref());
        match completion.kind {
            super::client::SuggestionKind::File => {
                capabilities.is_some_and(|c| c.can_mention_files)
            }
            _ => capabilities.is_some_and(|c| c.can_use_skills),
        }
    }

    pub(super) fn completion_context(&self) -> CompletionContext {
        let draft = self.composer.draft();
        let suggestions = &self.composer.suggestions;
        let caret = self.composer.caret.min(draft.text.len());
        let completion = self.current_completion(&draft.text, caret);
        let target = self.target_token();
        let can_open = self.is_visible && ComposerState::can_edit(&self.session);
        let completion_key = completion
            .as_ref()
            .filter(|completion| can_open && self.is_completion_supported(completion))
            .map(|completion| {
                format!(
                    "{:?}|{target:?}|{:?}|{caret}|{}|{:?}|{:?}",
                    self.session.task_id,
                    draft.text,
                    suggestions.is_skill_picker_open,
                    completion.kind,
                    completion.query
                )
            });
        let is_shown = completion_key.is_some() && completion_key != suggestions.dismissed_key;
        let catalog_key = completion
            .as_ref()
            .filter(|completion| is_shown && completion.kind != super::client::SuggestionKind::File)
            .map(|completion| {
                format!(
                    "{:?}|{target:?}|{:?}",
                    self.session.task_id, completion.kind
                )
            });
        CompletionContext {
            completion,
            completion_key,
            catalog_key,
            is_shown,
        }
    }

    /// 새 스냅샷의 대화로 초안을 맞춘다. 같은 대화에서 대상 토큰만 바뀌면 올려 둔 첨부를 다시 올리게 하고 옛 첨부는 버린다
    pub(super) fn sync_composer_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let snapshot = self.session.snapshot().cloned();
        let key = draft_key(&self.session.task_id, snapshot.as_ref());
        if let Some(token) = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.target_token.clone())
        {
            let owner = DraftOwner {
                task_id: self.session.task_id.clone(),
                target_token: token,
            };
            if let Some((previous, abandoned)) = self.composer.drafts.renew_owner(&key, owner) {
                self.discard_staged(&previous, abandoned, cx);
            }
        }
        let target = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.target_token.clone());
        if target != self.composer.last_target {
            self.composer.last_target = target;
            self.composer.suggestions = SuggestionState::default();
            self.composer.caret = 0;
        }
        if key != self.composer.draft_key {
            self.composer.draft_key = key;
            let text = self.composer.draft().text;
            self.composer
                .textarea
                .update(cx, |state, cx| state.set_value(text, window, cx));
        }
        self.sync_suggestions(window, cx);
    }

    fn handle_text_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (value, cursor) = {
            let state = self.composer.textarea.read(cx);
            (state.value().to_string(), state.cursor())
        };
        let key = self.composer.draft_key.clone();
        let draft = self.composer.drafts.entry(&key);
        if draft.text == value {
            return;
        }
        draft.text = value;
        draft.revision += 1;
        draft.error = None;
        self.composer.caret = cursor;
        self.composer.suggestions.is_skill_picker_open = false;
        self.sync_suggestions(window, cx);
        cx.notify();
    }

    fn handle_caret_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.composer.textarea.read(cx).cursor();
        if cursor == self.composer.caret {
            return;
        }
        self.composer.caret = cursor;
        self.sync_suggestions(window, cx);
        cx.notify();
    }

    fn handle_skill_query_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.composer.skill_search.read(cx).value().to_string();
        if query == self.composer.suggestions.skill_query {
            return;
        }
        self.composer.suggestions.skill_query = query;
        self.sync_suggestions(window, cx);
        cx.notify();
    }

    /// Enter. 메뉴가 열려 있으면 메뉴가 먼저 받는다
    pub fn handle_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.handle_completion_key(CompletionKey::Enter, window, cx) {
            return;
        }
        self.submit(window, cx);
    }

    /// 입력창에 사용자가 쓴 것처럼 글을 바꾼다(제안 고르기·테스트)
    pub fn set_composer_text(
        &mut self,
        text: &str,
        caret: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.to_string();
        self.composer.textarea.update(cx, |state, cx| {
            state.set_value(text.clone(), window, cx);
            state.set_selected_range(caret..caret, cx);
        });
        let key = self.composer.draft_key.clone();
        let draft = self.composer.drafts.entry(&key);
        if draft.text != text {
            draft.text = text;
            draft.revision += 1;
            draft.error = None;
        }
        self.composer.caret = caret;
        self.composer.suggestions.is_skill_picker_open = false;
        self.sync_suggestions(window, cx);
        cx.notify();
    }

    /// 보내기(TS `submit`). 보낼 수 없는 상태거나 메뉴가 고르기를 기다리면 아무것도 하지 않는다
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_composer_disabled() || !self.is_input_ready() {
            return;
        }
        let context = self.completion_context();
        self.composer.suggestions.is_skill_picker_open = false;
        self.composer.suggestions.dismissed_key = context.completion_key;
        let key = self.composer.draft_key.clone();
        let captured = self.composer.draft();
        if captured.is_sending || !captured.has_content() || captured.has_unsettled_attachment() {
            return;
        }
        let message = self.mark_draft_sending(&key, &captured);
        let is_started = self.send_message(
            message,
            move |view, outcome, window, cx| {
                view.finish_submit(&key, &captured, outcome, window, cx)
            },
            window,
            cx,
        );
        if !is_started {
            let key = self.composer.draft_key.clone();
            let captured = self.composer.draft();
            self.finish_submit(
                &key,
                &captured,
                SendOutcome { is_accepted: false },
                window,
                cx,
            );
        }
        cx.notify();
    }

    /// 초안을 보내는 중으로 표시하고 보낼 메시지를 만든다. 글·첨부가 그대로면 지난 요청 식별값을 다시 쓴다
    fn mark_draft_sending(&mut self, key: &str, captured: &Draft) -> OutgoingMessage {
        let fingerprint = captured.fingerprint();
        let request = match captured.request.clone() {
            Some(request) if request.fingerprint == fingerprint => request,
            _ => DraftRequest {
                fingerprint,
                id: new_request_id(),
            },
        };
        let draft = self.composer.drafts.entry(key);
        draft.is_sending = true;
        draft.error = None;
        draft.request = Some(request.clone());
        OutgoingMessage {
            text: captured.text.clone(),
            attachment_ids: captured
                .attachments
                .iter()
                .filter_map(|item| item.staged_id.clone())
                .collect(),
            request_id: request.id,
        }
    }

    /// 받아들였으면 보낸 뒤 새로 쓴 글이 없을 때만 글을 지우고, 보낸 첨부만 뺀다. 아니면 전부 남기고 안내한다
    fn finish_submit(
        &mut self,
        key: &str,
        captured: &Draft,
        outcome: SendOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft = self.composer.drafts.entry(key);
        draft.is_sending = false;
        if !outcome.is_accepted {
            draft.error = Some(DraftError::SendFailed);
            cx.notify();
            return;
        }
        draft.error = None;
        draft.request = None;
        if draft.revision == captured.revision {
            draft.text.clear();
        }
        draft.attachments.retain(|item| {
            !captured
                .attachments
                .iter()
                .any(|sent| sent.local_id == item.local_id)
        });
        let text = draft.text.clone();
        if key == self.composer.draft_key {
            self.composer
                .textarea
                .update(cx, |state, cx| state.set_value(text, window, cx));
            self.composer.caret = self.composer.draft().text.len();
        }
        cx.notify();
    }
}
