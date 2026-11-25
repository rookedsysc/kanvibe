//! 화면이 사용자에게 보이는 상태를 읽는 자리. 테스트와 QA가 그린 결과와 같은 판정 함수를 거쳐 읽는다.

use gpui_kit::{App, Context, Focusable, Window};

use super::attachments::PickedFile;
use super::drafts::{AttachmentError, DraftAttachment, DraftError};
use super::view::PaneChatView;

impl PaneChatView {
    /// 입력창에 지금 보이는 초안 글
    pub fn draft_text(&self) -> String {
        self.composer.draft().text
    }

    /// 입력란에 글을 쓸 수 있는지(TS 입력란 `disabled`의 반대)
    pub fn is_input_editable(&self) -> bool {
        self.composer.is_editable()
    }

    /// 에이전트가 처리하는 중으로 보는지(작업 중이거나 보낸 글이 아직 기록되지 않음)
    pub fn is_agent_working(&self) -> bool {
        self.session.is_agent_working()
    }

    /// 입력창을 키울 수 있는 최대 높이(대화 화면의 60%)
    pub fn composer_max_height(&self, window: &Window) -> f32 {
        self.composer.maximum_height(window)
    }

    /// 입력창에 포커스가 있는지
    pub fn is_input_focused(&self, window: &Window, cx: &App) -> bool {
        self.composer.textarea.focus_handle(cx).is_focused(window)
    }

    /// 입력창 높이(논리 px)
    pub fn composer_height(&self) -> f32 {
        self.composer.input_height
    }

    pub fn attachments(&self) -> Vec<DraftAttachment> {
        self.composer.draft().attachments
    }

    /// 첨부 한 개 아래에 보이는 안내 글
    pub fn attachment_error_text(&self, attachment: &DraftAttachment) -> Option<String> {
        attachment.error.as_ref().map(|error| match error {
            AttachmentError::TooLarge => self.t("attachmentTooLarge"),
            AttachmentError::TargetChanged => self.t("attachmentTargetChanged"),
            AttachmentError::Service(message) => message.clone(),
        })
    }

    /// 입력창 아래 안내: 서비스가 거절한 이유나 초안 안내, CLI가 입력을 받지 못하는 이유
    pub fn composer_notes(&self) -> Vec<String> {
        let draft = self.composer.draft();
        let mut notes = Vec::new();
        let error = self.session.action_error.clone().or_else(|| {
            draft.error.map(|error| match error {
                DraftError::SendFailed => self.t("sendError"),
                DraftError::AttachmentLimit => self.t("attachmentLimit"),
            })
        });
        notes.extend(error);
        let snapshot = self.session.snapshot();
        if snapshot.is_some_and(|snapshot| snapshot.can_send) && !self.is_input_ready() {
            let key = match snapshot.map(|snapshot| snapshot.input_state) {
                Some(kanvibe_ai::pane_chat::snapshot::PaneChatInputState::Occupied) => {
                    "inputOccupied"
                }
                _ => "inputWaiting",
            };
            notes.push(self.t(key));
        }
        notes
    }

    /// 선택 화면이 떠서 입력창을 가린 상태인지
    pub fn is_composer_hidden(&self) -> bool {
        self.session
            .snapshot()
            .is_some_and(|snapshot| snapshot.prompt.is_some())
    }

    pub fn active_suggestion_index(&self) -> usize {
        self.composer.suggestions.active_index
    }

    pub fn is_skill_picker_open(&self) -> bool {
        self.composer.suggestions.is_skill_picker_open
    }

    /// 스킬 검색 입력란에 글을 넣는다(사용자가 친 것처럼)
    pub fn set_skill_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = query.to_string();
        self.composer
            .skill_search
            .update(cx, |state, cx| state.set_value(text.clone(), window, cx));
        self.composer.suggestions.skill_query = text;
        self.sync_suggestions(window, cx);
        cx.notify();
    }

    /// 입력창에 파일을 붙여넣거나 끌어 놓은 것처럼 더한다
    pub fn attach_files(
        &mut self,
        files: Vec<PickedFile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.add_files(files, window, cx);
    }

    /// "지침 표시" 체크 상자를 누른다. 설정을 읽기 전과 저장하는 동안에는 바꾸지 않는다
    pub fn toggle_show_instructions(&mut self, cx: &mut Context<Self>) {
        let preferences = self.preferences.read(cx);
        if !preferences.is_loaded || preferences.is_saving {
            return;
        }
        let should_show = !preferences.should_show_instructions;
        self.preferences.update(cx, |preferences, cx| {
            preferences.set_show_instructions(should_show, cx)
        });
    }

    /// "지침 표시"를 누를 수 있는지와 체크 상태
    pub fn show_instructions_state(&self, cx: &App) -> (bool, bool) {
        let preferences = self.preferences.read(cx);
        (
            preferences.is_loaded && !preferences.is_saving,
            preferences.should_show_instructions,
        )
    }
}
