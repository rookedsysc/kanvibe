//! rollout 이벤트 하나를 화면 메시지로 옮긴다. 사용자 입력, 개발자 지침, 추론, 도구 호출과 결과, AI 답을 역할로 가른다.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use crate::javascript::{JS_WHITESPACE_CLASS, js_string, js_trim};
use crate::message_attachments::extract_message_attachments;
use crate::message_files::summarize_codex_custom_tool_call;
use crate::message_text::extract_plain_text;
use crate::record_fields::{first_present, first_present_or_null, string_field};
use crate::types::{AiMessageAttachment, AiMessageFile, AiMessageRole};

/// 이벤트에서 뽑은 메시지 하나. 미리보기를 만들기 전의 원문이다
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CodexEventMessage {
    pub role: AiMessageRole,
    pub text: String,
    pub attachments: Vec<AiMessageAttachment>,
    pub tool_name: Option<String>,
    pub files: Vec<AiMessageFile>,
}

impl CodexEventMessage {
    fn new(role: AiMessageRole, text: String) -> Self {
        Self {
            role,
            text,
            attachments: Vec::new(),
            tool_name: None,
            files: Vec::new(),
        }
    }
}

static IMAGE_FRAME_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^(?:<image(?:[{JS_WHITESPACE_CLASS}][^>]*)?>|</image>)$"
    ))
    .expect("이미지 틀 패턴")
});

static IMAGE_MARKER_PREFIX_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^(?:\[Image #[0-9]+\][{JS_WHITESPACE_CLASS}]*)+"))
        .expect("이미지 번호 패턴")
});

const TOOL_CALL_TYPES: [&str; 3] = ["function_call", "tool_call", "custom_tool_call"];

static EMPTY_PAYLOAD: LazyLock<Value> = LazyLock::new(|| Value::Object(Map::new()));

/// `event.payload ?? {}`. 이벤트마다 payload를 복제하지 않도록 참조를 돌려준다
fn event_payload(event: &Value) -> &Value {
    match event.get("payload") {
        Some(payload) if !payload.is_null() => payload,
        _ => &EMPTY_PAYLOAD,
    }
}

/// 이벤트 하나의 메시지. 도구 결과에는 같은 call_id로 먼저 기록된 호출의 도구 이름을 붙인다
pub(crate) fn extract_codex_event_messages(
    event: &Value,
    source_ref: &str,
    tool_names: &mut HashMap<String, String>,
) -> Vec<CodexEventMessage> {
    match event.get("type").and_then(Value::as_str) {
        Some("response_item") => {
            extract_response_item(event_payload(event), source_ref, tool_names)
        }
        Some("event_msg") => extract_agent_message(event_payload(event)),
        _ => Vec::new(),
    }
}

fn extract_response_item(
    payload: &Value,
    source_ref: &str,
    tool_names: &mut HashMap<String, String>,
) -> Vec<CodexEventMessage> {
    let call_id = string_field(payload, "call_id");
    let payload_type = payload
        .get("type")
        .map_or_else(|| "undefined".to_string(), js_string);
    if let (Some(call_id), Some(name)) = (call_id, string_field(payload, "name"))
        && TOOL_CALL_TYPES.contains(&payload_type.as_str())
    {
        tool_names.insert(call_id.to_string(), name.to_string());
    }
    let recorded_tool_name = call_id.and_then(|call_id| tool_names.get(call_id));
    extract_codex_payload_messages(payload, source_ref)
        .into_iter()
        .map(|mut message| {
            if message.role == AiMessageRole::Tool && message.tool_name.is_none() {
                message.tool_name = recorded_tool_name.cloned();
            }
            message
        })
        .collect()
}

fn extract_agent_message(payload: &Value) -> Vec<CodexEventMessage> {
    if string_field(payload, "type") != Some("agent_message") {
        return Vec::new();
    }
    let text = match payload.get("message") {
        Some(Value::String(message)) => message.clone(),
        _ => extract_plain_text(first_present(payload, &["message"]).unwrap_or(payload)),
    };
    if text.is_empty() {
        return Vec::new();
    }
    vec![CodexEventMessage::new(AiMessageRole::Assistant, text)]
}

pub(crate) fn should_skip_codex_message(role: AiMessageRole, text: &str) -> bool {
    let trimmed = js_trim(text);
    match role {
        AiMessageRole::User => ["<environment_context>", "<skill>", "<system-reminder>"]
            .iter()
            .any(|prefix| trimmed.starts_with(prefix)),
        AiMessageRole::Developer => [
            "<permissions instructions>",
            "<apps_instructions>",
            "<skills_instructions>",
            "<collaboration_mode>",
        ]
        .iter()
        .any(|marker| trimmed.contains(marker)),
        _ => false,
    }
}

fn extract_codex_payload_messages(payload: &Value, source_ref: &str) -> Vec<CodexEventMessage> {
    match payload.get("type").and_then(Value::as_str) {
        Some("message") => extract_role_message(payload, source_ref),
        Some("reasoning") => {
            let source = first_present(payload, &["summary", "content", "text"]).unwrap_or(payload);
            non_empty_message(AiMessageRole::Reasoning, extract_plain_text(source))
        }
        Some("function_call" | "tool_call") => vec![function_call_message(payload)],
        Some("custom_tool_call") => {
            let tool_call = summarize_codex_custom_tool_call(payload);
            let mut message = CodexEventMessage::new(AiMessageRole::Tool, tool_call.summary);
            message.tool_name = Some(tool_call.tool_name);
            message.files = tool_call.files;
            vec![message]
        }
        Some("function_call_output" | "custom_tool_call_output" | "tool_result") => {
            let source =
                first_present(payload, &["output", "result", "content"]).unwrap_or(payload);
            non_empty_message(AiMessageRole::Tool, extract_plain_text(source))
        }
        _ => non_empty_message(AiMessageRole::Unknown, extract_plain_text(payload)),
    }
}

fn non_empty_message(role: AiMessageRole, text: String) -> Vec<CodexEventMessage> {
    if text.is_empty() {
        return Vec::new();
    }
    vec![CodexEventMessage::new(role, text)]
}

fn function_call_message(payload: &Value) -> CodexEventMessage {
    let name = string_field(payload, "name")
        .or_else(|| string_field(payload, "call_id"))
        .unwrap_or("tool")
        .to_string();
    let argument_text = extract_plain_text(first_present_or_null(
        payload,
        &["arguments", "args", "input"],
    ));
    let text = if argument_text.is_empty() {
        format!("{name} called")
    } else {
        format!("{name}: {argument_text}")
    };
    let mut message = CodexEventMessage::new(AiMessageRole::Tool, text);
    message.tool_name = Some(name);
    message
}

/// 사용자·AI 메시지. 이미지가 있으면 Codex가 이미지를 감싸는 `<image>` 틀 줄과 앞의 `[Image #N]` 표시를 걷어 낸다
fn extract_role_message(payload: &Value, source_ref: &str) -> Vec<CodexEventMessage> {
    let role = resolve_codex_message_role(payload.get("role"));
    let content = payload.get("content").unwrap_or(&Value::Null);
    let attachments = extract_message_attachments(content, source_ref);
    let has_attachments = !attachments.is_empty();
    let raw_text = match content {
        Value::Array(parts) if has_attachments => extract_plain_text(&Value::Array(
            parts
                .iter()
                .filter(|part| !is_image_frame(part))
                .cloned()
                .collect(),
        )),
        _ => extract_plain_text(content),
    };
    let text = if has_attachments {
        IMAGE_MARKER_PREFIX_PATTERN
            .replace(&raw_text, "")
            .into_owned()
    } else {
        raw_text
    };
    if text.is_empty() && !has_attachments {
        return Vec::new();
    }
    let mut message = CodexEventMessage::new(role, text);
    message.attachments = attachments;
    vec![message]
}

fn is_image_frame(part: &Value) -> bool {
    string_field(part, "text").is_some_and(|text| IMAGE_FRAME_PATTERN.is_match(js_trim(text)))
}

fn resolve_codex_message_role(role: Option<&Value>) -> AiMessageRole {
    match role.and_then(Value::as_str) {
        Some("user") => AiMessageRole::User,
        Some("assistant") => AiMessageRole::Assistant,
        Some("system") => AiMessageRole::System,
        Some("developer") => AiMessageRole::Developer,
        Some("tool") => AiMessageRole::Tool,
        _ => AiMessageRole::Unknown,
    }
}

/// 같은 역할·시각·글·첨부의 메시지를 한 번만 싣기 위한 열쇠
pub(crate) fn create_codex_message_key(
    role: AiMessageRole,
    timestamp: &Value,
    text: &str,
    attachments: &[AiMessageAttachment],
) -> String {
    let timestamp_text = if timestamp.is_null() {
        String::new()
    } else {
        js_string(timestamp)
    };
    let attachment_ids = attachments
        .iter()
        .map(|attachment| attachment.id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let role_name = serde_json::to_value(role)
        .ok()
        .map(|value| js_string(&value))
        .unwrap_or_default();
    format!("{role_name}\u{0}{timestamp_text}\u{0}{text}\u{0}{attachment_ids}")
}

/// `event.timestamp`. 없으면 null로 본다
pub(crate) fn event_timestamp(event: &Value) -> &Value {
    event.get("timestamp").unwrap_or(&Value::Null)
}
