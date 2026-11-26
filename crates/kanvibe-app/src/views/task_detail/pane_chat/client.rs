//! TS `actions/paneChat.ts`의 함수들. 인자를 TS 순서의 JSON으로 싸서 [`PaneChatBackend`]에 넘기고 결과를 타입으로 푼다.
//! 서비스 호출은 막히는 일이라 늘 백그라운드 실행기에서 기다린다.

use std::collections::BTreeMap;
use std::sync::Arc;

use gpui_kit::{App, AppContext, Task};
use kanvibe_ai::AiSessionProvider;
use kanvibe_ai::pane_chat::model_settings::{PaneChatModelSettingChange, PaneChatModelSettings};
use kanvibe_ai::pane_chat::snapshot::{PaneChatLayout, PaneChatSnapshot, PaneChatTranscript};
use kanvibe_domain::pane_chat::{PaneChatPromptAnswer, PaneChatSuggestion};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::backend::{
    PaneChatBackend, PaneChatCallError, PaneChatConnectionWatch, PaneChatRequest,
};

pub type PaneChatResult<T> = Result<T, PaneChatCallError>;

/// TS `PaneChatActionResult`
#[derive(Debug, Clone, Copy, Deserialize)]
struct ActionResult {
    success: bool,
}

/// TS `PaneChatSuggestions`. 어느 CLI 대상의 결과인지 함께 온다
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatSuggestionList {
    pub target_token: String,
    pub items: Vec<PaneChatSuggestion>,
    #[serde(rename = "truncated")]
    pub is_truncated: bool,
}

/// TS `PaneChatCodexThreadReveal["outcome"]`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RevealOutcome {
    Revealed,
    InputNotEmpty,
    UnsupportedVersion,
    ThreadNotFound,
}

#[derive(Deserialize)]
struct RevealResult {
    outcome: RevealOutcome,
}

#[derive(Deserialize)]
struct StagedAttachment {
    id: String,
}

/// 입력창 첨부로 올릴 파일(TS `PaneChatAttachmentInput`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentUpload {
    pub name: String,
    pub mime_type: String,
    pub data_url: String,
}

#[derive(Clone)]
pub struct PaneChatClient {
    backend: Arc<dyn PaneChatBackend>,
}

impl PaneChatClient {
    pub fn new(backend: Arc<dyn PaneChatBackend>) -> Self {
        Self { backend }
    }

    fn invoke<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &'static str,
        arguments: Vec<Value>,
        cx: &App,
    ) -> Task<PaneChatResult<T>> {
        let call = self.backend.call(PaneChatRequest { target, arguments });
        cx.background_spawn(async move {
            let value = call.await?;
            serde_json::from_value(value).map_err(|error| PaneChatCallError::new(error.to_string()))
        })
    }

    /// 결과가 `{ success }`인 호출. 받아들였으면 `true`다
    fn act(
        &self,
        target: &'static str,
        arguments: Vec<Value>,
        cx: &App,
    ) -> Task<PaneChatResult<bool>> {
        let task = self.invoke::<ActionResult>(target, arguments, cx);
        cx.background_spawn(async move { task.await.map(|result| result.success) })
    }

    /// 그 Task의 연결 신호. 받은 쪽이 신호마다 한 번씩 다시 확인한다. 신호를 낼 수 없는 서비스면 `None`이다
    pub fn watch_connection(&self, task_id: &str) -> Option<ConnectionSignals> {
        let (sender, receiver) = futures::channel::mpsc::unbounded();
        let watch = self.backend.watch_connection(
            task_id,
            Arc::new(move || {
                let _ = sender.unbounded_send(());
            }),
        )?;
        Some(ConnectionSignals {
            receiver,
            _watch: watch,
        })
    }

    pub fn get_layout(
        &self,
        task_id: &str,
        tab_id: Option<&str>,
        cx: &App,
    ) -> Task<PaneChatResult<PaneChatLayout>> {
        self.invoke(
            "paneChat.getPaneChatLayout",
            vec![json!(task_id), json!(tab_id)],
            cx,
        )
    }

    pub fn get_agent_provider(
        &self,
        task_id: &str,
        pane_id: &str,
        cx: &App,
    ) -> Task<PaneChatResult<Option<AiSessionProvider>>> {
        self.invoke(
            "paneChat.getPaneChatAgentProvider",
            vec![json!(task_id), json!(pane_id)],
            cx,
        )
    }

    pub fn get_snapshot(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        cx: &App,
    ) -> Task<PaneChatResult<PaneChatSnapshot>> {
        self.invoke(
            "paneChat.getPaneChatSnapshot",
            vec![json!(task_id), json!(pane_id)],
            cx,
        )
    }

    pub fn focus_terminal(
        &self,
        task_id: &str,
        pane_id: &str,
        cx: &App,
    ) -> Task<PaneChatResult<bool>> {
        self.act(
            "paneChat.focusPaneChatTerminal",
            vec![json!(task_id), json!(pane_id)],
            cx,
        )
    }

    pub fn get_modes(
        &self,
        task_id: &str,
        cx: &App,
    ) -> Task<PaneChatResult<BTreeMap<String, bool>>> {
        self.invoke("paneChat.getPaneChatModes", vec![json!(task_id)], cx)
    }

    pub fn set_mode(
        &self,
        task_id: &str,
        pane_id: &str,
        is_chat: bool,
        cx: &App,
    ) -> Task<PaneChatResult<()>> {
        self.invoke(
            "paneChat.setPaneChatMode",
            vec![json!(task_id), json!(pane_id), json!(is_chat)],
            cx,
        )
    }

    pub fn get_transcript_page(
        &self,
        task_id: &str,
        token: &str,
        cursor: &str,
        cx: &App,
    ) -> Task<PaneChatResult<PaneChatTranscript>> {
        self.invoke(
            "paneChat.getPaneChatTranscriptPage",
            vec![json!(task_id), json!(token), json!(cursor)],
            cx,
        )
    }

    pub fn send_prompt(
        &self,
        task_id: &str,
        token: &str,
        message: &OutgoingMessage,
        cx: &App,
    ) -> Task<PaneChatResult<bool>> {
        let arguments = vec![
            json!(task_id),
            json!(token),
            json!(message.text),
            json!(message.attachment_ids),
            json!(message.request_id),
        ];
        self.act("paneChat.sendPaneChatPrompt", arguments, cx)
    }

    pub fn interrupt(&self, task_id: &str, token: &str, cx: &App) -> Task<PaneChatResult<bool>> {
        self.act(
            "paneChat.interruptPaneChat",
            vec![json!(task_id), json!(token)],
            cx,
        )
    }

    pub fn open_model_picker(
        &self,
        task_id: &str,
        token: &str,
        cx: &App,
    ) -> Task<PaneChatResult<bool>> {
        self.act(
            "paneChat.openPaneChatModelPicker",
            vec![json!(task_id), json!(token)],
            cx,
        )
    }

    pub fn reveal_codex_thread(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        cx: &App,
    ) -> Task<PaneChatResult<RevealOutcome>> {
        let task = self.invoke::<RevealResult>(
            "paneChat.revealPaneChatCodexThread",
            vec![json!(task_id), json!(pane_id)],
            cx,
        );
        cx.background_spawn(async move { task.await.map(|result| result.outcome) })
    }

    pub fn answer_prompt(
        &self,
        task_id: &str,
        token: &str,
        prompt_id: &str,
        answer: &PaneChatPromptAnswer,
        cx: &App,
    ) -> Task<PaneChatResult<bool>> {
        let arguments = vec![
            json!(task_id),
            json!(token),
            json!(prompt_id),
            json!(answer),
        ];
        self.act("paneChat.answerPaneChatPrompt", arguments, cx)
    }

    /// 올린 첨부의 식별값을 돌려준다
    pub fn stage_attachment(
        &self,
        task_id: &str,
        token: &str,
        upload: &AttachmentUpload,
        cx: &App,
    ) -> Task<PaneChatResult<String>> {
        let input = json!({ "name": upload.name, "mimeType": upload.mime_type, "dataUrl": upload.data_url });
        let task = self.invoke::<StagedAttachment>(
            "paneChat.stagePaneChatAttachment",
            vec![json!(task_id), json!(token), input],
            cx,
        );
        cx.background_spawn(async move { task.await.map(|staged| staged.id) })
    }

    pub fn remove_attachment(
        &self,
        task_id: &str,
        token: &str,
        attachment_id: &str,
        cx: &App,
    ) -> Task<PaneChatResult<bool>> {
        self.act(
            "paneChat.removePaneChatAttachment",
            vec![json!(task_id), json!(token), json!(attachment_id)],
            cx,
        )
    }

    /// `limit`이 없으면 TS처럼 인자를 아예 넘기지 않는다
    pub fn get_suggestions(
        &self,
        task_id: &str,
        token: &str,
        query: &SuggestionQuery,
        cx: &App,
    ) -> Task<PaneChatResult<PaneChatSuggestionList>> {
        let mut arguments = vec![
            json!(task_id),
            json!(token),
            json!(query.kind.as_str()),
            json!(query.text),
        ];
        arguments.extend(query.limit.map(|limit| json!(limit)));
        self.invoke("paneChat.getPaneChatSuggestions", arguments, cx)
    }

    pub fn get_model_settings(
        &self,
        task_id: &str,
        token: &str,
        cx: &App,
    ) -> Task<PaneChatResult<PaneChatModelSettings>> {
        self.invoke(
            "paneChat.getPaneChatModelSettings",
            vec![json!(task_id), json!(token)],
            cx,
        )
    }

    pub fn change_model_setting(
        &self,
        task_id: &str,
        token: &str,
        change: &PaneChatModelSettingChange,
        cx: &App,
    ) -> Task<PaneChatResult<bool>> {
        self.act(
            "paneChat.changePaneChatModelSetting",
            vec![json!(task_id), json!(token), json!(change)],
            cx,
        )
    }

    pub fn get_app_setting(&self, key: &str, cx: &App) -> Task<PaneChatResult<Option<String>>> {
        self.invoke("appSettings.getAppSetting", vec![json!(key)], cx)
    }

    pub fn set_app_setting(&self, key: &str, value: &str, cx: &App) -> Task<PaneChatResult<()>> {
        self.invoke(
            "appSettings.setAppSetting",
            vec![json!(key), json!(value)],
            cx,
        )
    }
}

/// 연결 신호를 받는 쪽. 구독을 함께 들고 있어 이 값을 버리면 신호도 멈춘다
pub struct ConnectionSignals {
    receiver: futures::channel::mpsc::UnboundedReceiver<()>,
    _watch: PaneChatConnectionWatch,
}

impl ConnectionSignals {
    /// 다음 신호. 구독이 끝나면 `None`이다. 값 전체를 빌려 쓰므로 이 값을 옮긴 곳이 구독도 함께 들고 있다
    pub async fn next_signal(&mut self) -> Option<()> {
        futures::StreamExt::next(&mut self.receiver).await
    }
}

/// 보낼 메시지(TS `sendPaneChatPrompt`의 글·첨부·요청 식별값)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingMessage {
    pub text: String,
    pub attachment_ids: Vec<String>,
    pub request_id: String,
}

/// 입력창 메뉴가 찾는 종류(TS `PaneChatSuggestionKind`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuggestionKind {
    Skill,
    Command,
    File,
}

impl SuggestionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Command => "command",
            Self::File => "file",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestionQuery {
    pub kind: SuggestionKind,
    pub text: String,
    pub limit: Option<usize>,
}
