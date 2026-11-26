//! Claude Code JSONL 이벤트 한 줄을 역할, 사용자가 보낸 글, 화면에 보일 메시지들로 읽는다.

use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

use crate::javascript::{JS_WHITESPACE_CLASS, js_trim, utf16_length};
use crate::message_attachments::extract_message_attachments;
use crate::message_files::summarize_claude_tool_use;
use crate::message_text::extract_plain_text;
use crate::types::{AiMessageAttachment, AiMessageFile, AiMessageRole};

/// 이벤트의 `message.content`. 없으면 `null`로 본다
pub(super) fn message_content(event: &Value) -> &Value {
    event
        .get("message")
        .and_then(|message| message.get("content"))
        .unwrap_or(&Value::Null)
}

pub(super) fn message_role(event: &Value) -> Option<&str> {
    event
        .get("message")
        .and_then(|message| message.get("role"))
        .and_then(Value::as_str)
}

pub(super) fn string_field<'a>(event: &'a Value, key: &str) -> Option<&'a str> {
    event.get(key).and_then(Value::as_str)
}

fn is_true(event: &Value, key: &str) -> bool {
    event.get(key) == Some(&Value::Bool(true))
}

static IMAGE_MARKERS_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^(?:\[Image #[0-9]+\][{JS_WHITESPACE_CLASS}]*)+"))
        .expect("이미지 표시 패턴")
});

static PASTED_CONTENT_OPENING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<pasted_content id="([^"]*)">"#).expect("붙여넣기 여는 표시 패턴")
});

const SHELL_COMMAND_OPENING: &str = "<bash-input>";
const SHELL_COMMAND_CLOSING: &str = "</bash-input>";

/// Claude가 입력란에서 접어 보여 준 붙여넣기를 기록할 때 두르는 표시를 걷는다. 여닫는 표시에 같은 id가 붙은 것만 걷는다.
/// JS 원본은 역참조(`<\/pasted_content id="\1">`)로 닫는 표시를 찾지만 Rust regex에는 역참조가 없어 직접 찾는다
fn unwrap_pasted_contents(text: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    let mut search_from = 0;
    while let Some(opening) = PASTED_CONTENT_OPENING.captures_at(text, search_from) {
        let whole = opening.get(0).expect("전체 일치");
        let closing = format!("</pasted_content id=\"{}\">", &opening[1]);
        let body_start = whole.end() + usize::from(text[whole.end()..].starts_with('\n'));
        let Some(closing_offset) = text[body_start..].find(&closing) else {
            // 닫는 표시가 없으면 JS처럼 한 글자 뒤에서 다시 찾는다
            search_from = whole.start() + 1;
            continue;
        };
        let body = &text[body_start..body_start + closing_offset];
        output.push_str(&text[cursor..whole.start()]);
        output.push_str(body.strip_suffix('\n').unwrap_or(body));
        cursor = body_start + closing_offset + closing.len();
        search_from = cursor;
    }
    output.push_str(&text[cursor..]);
    output
}

/// Claude가 `!`로 시작한 셸 명령을 명령만 따로 남긴 모양이면 사용자가 친 `!` 명령으로 되돌린다
fn restore_shell_command(text: String) -> String {
    let is_shell_record = text.len() >= SHELL_COMMAND_OPENING.len() + SHELL_COMMAND_CLOSING.len()
        && text.starts_with(SHELL_COMMAND_OPENING)
        && text.ends_with(SHELL_COMMAND_CLOSING);
    if !is_shell_record {
        return text;
    }
    let command = &text[SHELL_COMMAND_OPENING.len()..text.len() - SHELL_COMMAND_CLOSING.len()];
    format!("!{command}")
}

/// Claude가 사용자 글에 두른 표시를 걷어, 사용자가 보낸 글로 되돌린다
fn restore_claude_sent_text(recorded_text: &str) -> String {
    js_trim(&restore_shell_command(unwrap_pasted_contents(
        recorded_text,
    )))
    .to_string()
}

/// JS `value?.length`가 참인지. 배열과 문자열만 길이를 가진다
fn has_length(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::String(text)) => utf16_length(text) > 0,
        _ => false,
    }
}

pub(super) fn claude_message_text(event: &Value) -> String {
    let text = extract_plain_text(message_content(event));
    let without_image_markers = if has_length(event.get("imagePasteIds")) {
        IMAGE_MARKERS_PATTERN.replace(&text, "").into_owned()
    } else {
        text
    };
    if message_role(event) == Some("user") {
        restore_claude_sent_text(&without_image_markers)
    } else {
        without_image_markers
    }
}

fn has_content_part_type(content: &Value, part_type: &str) -> bool {
    content.as_array().is_some_and(|parts| {
        parts
            .iter()
            .any(|part| part.get("type").and_then(Value::as_str) == Some(part_type))
    })
}

pub(super) fn resolve_claude_role(event: &Value) -> AiMessageRole {
    if has_content_part_type(message_content(event), "tool_result") {
        return AiMessageRole::Tool;
    }
    let role = message_role(event);
    if role == Some("system") || string_field(event, "type") == Some("system") {
        return AiMessageRole::System;
    }
    // isMeta가 붙은 사용자 줄은 사용자가 친 글이 아니라 CLI가 끼워 넣은 문맥(명령 안내문, 스킬 본문)이다
    match role {
        Some("user") if is_true(event, "isMeta") => AiMessageRole::System,
        Some("user") => AiMessageRole::User,
        Some("assistant") => AiMessageRole::Assistant,
        _ if string_field(event, "type") == Some("progress") => AiMessageRole::Tool,
        _ => AiMessageRole::Unknown,
    }
}

/// Claude가 사람이 붙인 이미지 옆에 남기는 출처 줄. 대화가 아니라서 목록·상세·현재 작업 어디에도 싣지 않는다
pub(super) fn is_claude_image_companion(event: &Value) -> bool {
    is_true(event, "isMeta")
        && is_true(event, "turnCompanion")
        && extract_plain_text(message_content(event)).starts_with("[Image: source: ")
}

/// Claude Code가 대화 파일에 함께 남기는 API 요청 로그. 요청에 실어 보낸 메시지의 사본이라 대화의 한 줄이 아니다
pub(super) fn is_claude_api_request_log(event: &Value) -> bool {
    string_field(event, "type") == Some("api-request-blob")
}

/// 기록 한 줄에서 나온 화면 메시지 하나
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ClaudeEventMessage {
    pub role: AiMessageRole,
    pub text: String,
    pub attachments: Vec<AiMessageAttachment>,
    pub tool_name: Option<String>,
    pub files: Vec<AiMessageFile>,
}

impl ClaudeEventMessage {
    fn plain(role: AiMessageRole, text: String, attachments: Vec<AiMessageAttachment>) -> Self {
        Self {
            role,
            text,
            attachments,
            tool_name: None,
            files: Vec::new(),
        }
    }
}

/// 기록 한 줄을 화면에 보일 메시지들로 나눈다. 줄 전체의 글은 메시지 하나가 되고, assistant 줄의 `thinking`과
/// `tool_use` 조각은 각각 따로 메시지가 된다. 조각이 적힌 순서가 곧 일어난 순서라서 글 메시지는 첫 `text` 조각 자리에 둔다
pub(super) fn extract_claude_event_messages(
    event: &Value,
    source_ref: &str,
) -> Vec<ClaudeEventMessage> {
    let content = message_content(event);
    let entry_message = ClaudeEventMessage::plain(
        resolve_claude_role(event),
        claude_message_text(event),
        extract_message_attachments(content, source_ref),
    );
    let Some(parts) = content
        .as_array()
        .filter(|_| message_role(event) == Some("assistant"))
    else {
        return vec![entry_message];
    };

    let mut messages = Vec::new();
    let mut pending_entry = Some(entry_message);
    for part in parts {
        if part.get("type").and_then(Value::as_str) == Some("text") {
            messages.extend(pending_entry.take());
        }
        messages.extend(thinking_message(part));
        if let Some(tool_call) = summarize_claude_tool_use(part) {
            messages.push(ClaudeEventMessage {
                role: AiMessageRole::Tool,
                text: tool_call.summary,
                attachments: Vec::new(),
                tool_name: Some(tool_call.tool_name),
                files: tool_call.files,
            });
        }
    }
    messages.extend(pending_entry);
    messages
}

fn thinking_message(part: &Value) -> Option<ClaudeEventMessage> {
    if part.get("type").and_then(Value::as_str) != Some("thinking") {
        return None;
    }
    let thinking = part.get("thinking").and_then(Value::as_str)?;
    (!js_trim(thinking).is_empty()).then(|| {
        ClaudeEventMessage::plain(AiMessageRole::Reasoning, thinking.to_string(), Vec::new())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_content_needs_matching_ids_and_drops_one_newline_each_side() {
        assert_eq!(
            unwrap_pasted_contents(
                "a <pasted_content id=\"x\">\nbody\n</pasted_content id=\"x\"> b"
            ),
            "a body b"
        );
        let mismatched = "<pasted_content id=\"1\">\ntext\n</pasted_content id=\"2\">";
        assert_eq!(unwrap_pasted_contents(mismatched), mismatched);
        assert_eq!(
            unwrap_pasted_contents(
                "<pasted_content id=\"1\"><pasted_content id=\"2\">in</pasted_content id=\"2\">"
            ),
            "<pasted_content id=\"1\">in"
        );
    }

    #[test]
    fn shell_records_become_bang_commands() {
        assert_eq!(
            restore_claude_sent_text("<bash-input>ls</bash-input>"),
            "!ls"
        );
        assert_eq!(restore_claude_sent_text("<bash-input>"), "<bash-input>");
    }
}
