//! 화면·모바일로 나가는 pane 채팅 상태. TS `src/desktop/shared/paneChat.ts`에서 AI 세션 타입(메시지, 모델 설정,
//! 선택 화면)에 기대는 부분이라 M1의 `kanvibe_domain::pane_chat` 대신 여기에 둔다. JSON 이름은 TS와 같다.

use kanvibe_domain::task::SessionType;
use serde::{Deserialize, Serialize};

use super::claude_prompt::PaneChatPromptScreen;
use super::model_settings::PaneChatModelSettings;
use super::timeline::PaneChatTimelineRow;
use crate::javascript::quote_json_string;
use crate::providers::AiSessionProvider;
use crate::types::AggregatedAiMessage;

/// CLI가 답을 기다리며 띄운 선택 화면. 채팅이 같은 내용을 카드로 보여 주고, 고른 답을 키 입력으로 전달한다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneChatPrompt {
    #[serde(flatten)]
    pub screen: PaneChatPromptScreen,
    /// 같은 선택 화면인지 가리는 값. 답을 보낼 때 그대로 돌려준다
    pub id: String,
}

/// 체크 상태가 바뀌어도 같은 선택 화면으로 묶는 값.
/// 답을 보낼 때의 `id`는 체크가 바뀌면 달라지므로, 화면에 떠 있는 카드와 그 카드에 남길 안내는 이 값으로 이어 붙인다
pub fn get_pane_chat_prompt_dialog_key(prompt: &PaneChatPrompt) -> String {
    let kind = serde_json::to_value(prompt.screen.kind).expect("선택 화면 종류 직렬화");
    format!(
        "[{},{},{}]",
        quote_json_string(kind.as_str().unwrap_or_default()),
        quote_json_string(&prompt.screen.title),
        quote_json_string(&prompt.screen.detail)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneChatBounds {
    pub left: i64,
    pub top: i64,
    pub width: i64,
    pub height: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatLayoutPane {
    pub id: String,
    pub tab_id: String,
    /// 셀 단위 좌표이며, `None`은 분할 없는 plain PTY 전체 영역이다
    pub bounds: Option<PaneChatBounds>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatLayout {
    pub active_pane_id: Option<String>,
    pub panes: Vec<PaneChatLayoutPane>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneChatInputState {
    Ready,
    Occupied,
    Unknown,
}

/// 에이전트가 응답을 만들거나 도구를 실행하는 중인지. 화면으로 확인하지 못하는 CLI나 surface는 unknown이다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneChatAgentActivity {
    Working,
    Idle,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneChatTranscriptStatus {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatTranscript {
    pub status: PaneChatTranscriptStatus,
    pub reason: Option<String>,
    pub session_id: Option<String>,
    pub messages: Vec<AggregatedAiMessage>,
    pub next_cursor: Option<String>,
    /// 대화에 매긴 순번이 유효한 범위. 순번을 매기지 않는 조회에는 없다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<String>,
    /// 가장 최근 메시지의 순번. messages는 최신순이라 index 0이 이 순번이다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneChatModelPickerMode {
    #[serde(rename = "native-cli")]
    NativeCli,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneChatModelPicker {
    #[serde(rename = "supported")]
    pub is_supported: bool,
    pub mode: PaneChatModelPickerMode,
    #[serde(
        default,
        rename = "supportsApi",
        skip_serializing_if = "Option::is_none"
    )]
    pub is_api_supported: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatComposerCapabilities {
    #[serde(rename = "attachments")]
    pub can_attach: bool,
    #[serde(rename = "skills")]
    pub can_use_skills: bool,
    #[serde(rename = "fileMentions")]
    pub can_mention_files: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_send_while_busy: Option<bool>,
    pub max_attachment_bytes: u64,
    pub max_attachments: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatSnapshot {
    pub task_id: String,
    pub pane_id: Option<String>,
    pub session_type: Option<SessionType>,
    pub provider: Option<AiSessionProvider>,
    pub target_token: Option<String>,
    /// host·작업·pane·대화 범위의 draft 키. 전송 권한인 target_token과 수명이 다르다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_key: Option<String>,
    pub can_send: bool,
    pub input_state: PaneChatInputState,
    pub agent_activity: PaneChatAgentActivity,
    pub screen: String,
    pub reason: Option<String>,
    pub transcript: PaneChatTranscript,
    pub model_picker: PaneChatModelPicker,
    /// 현재 모델·effort와 고를 수 있는 선택지. 기록에서 읽지 못했거나 지원하지 않는 CLI면 없다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_settings: Option<PaneChatModelSettings>,
    /// CLI가 답을 기다리며 띄운 선택 화면. 화면 배치를 확인한 CLI에서 대화가 연결돼 있을 때만 있다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PaneChatPrompt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composer: Option<PaneChatComposerCapabilities>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneChatSubscriptionDetail {
    /// 대화와 입력 상태까지 받는다
    Chat,
    /// 에이전트가 돌고 있는지만 받는다
    Agent,
}

/// 모바일이 push로 받으려는 pane 하나
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatSubscriptionTarget {
    pub task_id: String,
    pub pane_id: String,
    pub detail: PaneChatSubscriptionDetail,
    /// CLI 화면이 실제로 보이는 동안에만 켠다
    #[serde(rename = "wantsScreen")]
    pub should_include_screen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneChatFreshness {
    Current,
    /// 구독자가 스냅샷을 다시 받아야 한다
    Behind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatSubscriptionFreshness {
    pub task_id: String,
    pub pane_id: String,
    pub freshness: PaneChatFreshness,
}

/// push로 보내는 대화의 바깥 상태(transcript 요약)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatPushedTranscript {
    pub status: PaneChatTranscriptStatus,
    pub reason: Option<String>,
    pub session_id: Option<String>,
    pub next_cursor: Option<String>,
}

/// 대화 밖의 상태. 순번이 없고 마지막에 받은 값이 이긴다. 스냅샷의 일부 필드만 싣는다(TS `Partial<...>`).
/// 바깥 `Option`은 필드가 실렸는지, 안쪽 `Option`은 그 값이 null인지다
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatPushedState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_type: Option<Option<SessionType>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Option<AiSessionProvider>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_token: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_send: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_state: Option<PaneChatInputState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_activity: Option<PaneChatAgentActivity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<PaneChatPushedTranscript>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_picker: Option<PaneChatModelPicker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_settings: Option<PaneChatModelSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PaneChatPrompt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composer: Option<PaneChatComposerCapabilities>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum PaneChatPushEvent {
    #[serde(rename = "pane-chat-rows")]
    Rows {
        task_id: String,
        pane_id: String,
        epoch: String,
        rows: Vec<PaneChatTimelineRow>,
    },
    /// 기록이 다시 쓰였거나 다른 세션으로 바뀌었다. 줄은 싣지 않고 구독자가 스냅샷을 다시 받는다
    #[serde(rename = "pane-chat-replaced")]
    Replaced {
        task_id: String,
        pane_id: String,
        epoch: String,
    },
    #[serde(rename = "pane-chat-state")]
    State {
        task_id: String,
        pane_id: String,
        state: Box<PaneChatPushedState>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane_chat::claude_prompt::PaneChatPromptKind;
    use serde_json::json;

    fn prompt(is_checked: bool) -> PaneChatPrompt {
        PaneChatPrompt {
            screen: PaneChatPromptScreen {
                kind: PaneChatPromptKind::Question,
                title: "Pick \"one\"".to_string(),
                detail: "line\nnext".to_string(),
                steps_line: None,
                options: Vec::new(),
                highlighted_number: 1,
                is_multi_select: is_checked,
                is_text_entry_active: false,
                has_hidden_options: false,
            },
            id: format!("prompt-{is_checked}"),
        }
    }

    #[test]
    fn dialog_key_ignores_answer_id_and_matches_json_stringify() {
        assert_eq!(
            get_pane_chat_prompt_dialog_key(&prompt(false)),
            r#"["question","Pick \"one\"","line\nnext"]"#
        );
        assert_eq!(
            get_pane_chat_prompt_dialog_key(&prompt(true)),
            get_pane_chat_prompt_dialog_key(&prompt(false))
        );
    }

    #[test]
    fn prompt_and_push_events_use_ts_json_names() {
        let value = serde_json::to_value(prompt(false)).unwrap();
        assert_eq!(value["id"], "prompt-false");
        assert_eq!(value["stepsLine"], serde_json::Value::Null);
        let event = PaneChatPushEvent::State {
            task_id: "task".to_string(),
            pane_id: "pane".to_string(),
            state: Box::new(PaneChatPushedState {
                pane_id: Some(None),
                can_send: Some(true),
                ..PaneChatPushedState::default()
            }),
        };
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            json!({"type": "pane-chat-state", "taskId": "task", "paneId": "pane", "state": {"paneId": null, "canSend": true}})
        );
    }
}
