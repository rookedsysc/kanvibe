//! 기록 본문을 화면용 글로 바꾼다. 터미널 잡음 줄을 지우되 코드 블록 안은 그대로 두고, 목록용 미리보기는 한 줄로 접어 자른다.

use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

use crate::javascript::{
    JS_WHITESPACE_CLASS, is_present_truthy, iso_string_from_ms, js_trim, js_trim_end,
    parse_date_ms, utf16_length, utf16_prefix,
};
use crate::types::{AggregatedAiMessage, AggregatedAiSession, AiMessageAttachment, AiMessageRole};

pub const MAX_PREVIEW_TEXT_LENGTH: usize = 240;
const SESSION_TITLE_MAX_LENGTH: usize = 80;

static TRANSPORT_NOISE_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"^\[(?:remote-ssh|터미널|terminal)\]".to_string(),
        "^sshHost:".to_string(),
        "^command:".to_string(),
        "^sshArgs:".to_string(),
        "^error:".to_string(),
        format!(r"^at[{JS_WHITESPACE_CLASS}]+[^\n\r\x{{2028}}\x{{2029}}]*:[0-9]+:[0-9]+\)?$"),
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("잡음 줄 패턴"))
    .collect()
});

static CODE_FENCE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^[{JS_WHITESPACE_CLASS}]*(`{{3,}}|~{{3,}})([^\n\r\x{{2028}}\x{{2029}}]*)$"
    ))
    .expect("코드 울타리 패턴")
});

static PASTED_PLACEHOLDER_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[Pasted ~[0-9]+ lines\]").expect("붙여넣기 표시 패턴"));

static TRAILING_SPACES_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)[ \t]+$").expect("줄 끝 공백 패턴"));

static WHITESPACE_RUN_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!("[{JS_WHITESPACE_CLASS}]+")).expect("공백 묶음 패턴"));

fn is_transport_noise_line(trimmed_line: &str) -> bool {
    TRANSPORT_NOISE_PATTERNS
        .iter()
        .any(|pattern| pattern.is_match(trimmed_line))
}

/// 닫는 울타리는 여는 울타리와 같은 글자로 그 이상 길어야 하고, 뒤에 언어 이름 같은 글이 붙지 않는다
fn is_closing_fence(marker: &str, trailing_text: &str, open_fence: &str) -> bool {
    marker.chars().next() == open_fence.chars().next()
        && marker.len() >= open_fence.len()
        && js_trim(trailing_text).is_empty()
}

fn remove_transport_noise(line: &str) -> Vec<String> {
    PASTED_PLACEHOLDER_PATTERN
        .replace_all(line, "\n")
        .split('\n')
        .filter(|segment| !is_transport_noise_line(js_trim(segment)))
        .map(str::to_string)
        .collect()
}

/// JS `value.split(/\r?\n/)`
pub fn split_lines(value: &str) -> impl Iterator<Item = &str> {
    value
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

/// 터미널 잡음 줄을 지우되 코드 블록 안은 건드리지 않는다. 코드 블록에는 YAML의 `command:`나 컴파일러의 `error:`처럼
/// 잡음과 같은 모양의 줄이 본문으로 들어 있고, 닫는 울타리가 없으면 글 끝까지를 코드 블록으로 본다
pub fn preserve_message_text(value: &str) -> String {
    let mut open_fence: Option<String> = None;
    let mut kept_lines: Vec<String> = Vec::new();

    for line in split_lines(value) {
        let fence = CODE_FENCE_PATTERN.captures(line);
        if let Some(open) = &open_fence {
            kept_lines.push(line.to_string());
            if fence.is_some_and(|found| is_closing_fence(&found[1], &found[2], open)) {
                open_fence = None;
            }
            continue;
        }
        if let Some(found) = fence {
            open_fence = Some(found[1].to_string());
            kept_lines.push(line.to_string());
            continue;
        }
        kept_lines.extend(remove_transport_noise(line));
    }

    let joined = kept_lines.join("\n");
    js_trim(&TRAILING_SPACES_PATTERN.replace_all(&joined, "")).to_string()
}

/// 잡음을 지운 글을 한 줄로 접는다
pub fn normalize_text(value: &str) -> String {
    let preserved = preserve_message_text(value);
    let sanitized = split_lines(&preserved)
        .map(js_trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    js_trim(&WHITESPACE_RUN_PATTERN.replace_all(&sanitized, " ")).to_string()
}

/// 한 줄로 접은 글을 UTF-16 길이 `max_length`까지 자르고 `...`을 붙인다
pub fn truncate_text_to(value: &str, max_length: usize) -> String {
    let normalized = normalize_text(value);
    if utf16_length(&normalized) <= max_length {
        return normalized;
    }
    let kept = utf16_prefix(&normalized, max_length.saturating_sub(1));
    format!("{}...", js_trim_end(kept))
}

pub fn truncate_text(value: &str) -> String {
    truncate_text_to(value, MAX_PREVIEW_TEXT_LENGTH)
}

struct TruncatedText {
    full_text: String,
    preview_text: String,
    is_truncated: bool,
}

fn create_truncated_text(value: &str) -> TruncatedText {
    let full_text = preserve_message_text(value);
    let preview_text = truncate_text(&full_text);
    let is_truncated = utf16_length(&preview_text) < utf16_length(&normalize_text(&full_text));
    TruncatedText {
        full_text,
        preview_text,
        is_truncated,
    }
}

/// TS `toIsoString`. 해석되는 문자열은 ISO로 바꾸고 못 하면 원문을 두며, 숫자는 밀리초·초 단위를 가려 바꾼다
pub fn to_iso_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(match parse_date_ms(text) {
            Some(milliseconds) => iso_string_from_ms(milliseconds)?,
            None => text.clone(),
        }),
        Value::Number(number) => {
            let number = number.as_f64()?;
            if number > 1_000_000_000_000.0 {
                iso_string_from_ms(number)
            } else if number > 1_000_000_000.0 {
                iso_string_from_ms(number * 1000.0)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// 문자열 시각 하나를 [`to_iso_string`]으로 바꾼다
pub fn to_iso_string_from_str(value: Option<&str>) -> Option<String> {
    to_iso_string(&Value::String(value?.to_string()))
}

const DIRECT_TEXT_KEYS: [&str; 8] = [
    "text",
    "input_text",
    "output_text",
    "display",
    "description",
    "subject",
    "output",
    "result",
];

/// 문자열, 배열, `text`·`content`·`message` 같은 필드가 있는 객체에서 사람이 읽을 글을 꺼낸다
pub fn extract_plain_text(value: &Value) -> String {
    match value {
        Value::String(text) => preserve_message_text(text),
        Value::Array(items) => {
            let joined = items
                .iter()
                .map(extract_plain_text)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            preserve_message_text(&joined)
        }
        Value::Object(record) => {
            if let Some(text) = DIRECT_TEXT_KEYS
                .iter()
                .find_map(|key| record.get(*key).and_then(Value::as_str))
            {
                return preserve_message_text(text);
            }
            for key in ["content", "message"] {
                if is_present_truthy(record.get(key)) {
                    let nested = extract_plain_text(&record[key]);
                    if !nested.is_empty() {
                        return nested;
                    }
                }
            }
            match record.get("title").and_then(Value::as_str) {
                Some(title) if !title.is_empty() => preserve_message_text(title),
                _ => String::new(),
            }
        }
        _ => String::new(),
    }
}

/// 미리보기와 전문을 함께 담은 메시지. 글도 첨부도 없으면 `None`이다
pub fn make_preview_message(
    role: AiMessageRole,
    timestamp: &Value,
    text: &str,
    attachments: Vec<AiMessageAttachment>,
) -> Option<AggregatedAiMessage> {
    let truncated = create_truncated_text(text);
    if truncated.full_text.is_empty() && attachments.is_empty() {
        return None;
    }
    Some(AggregatedAiMessage {
        role,
        timestamp: to_iso_string(timestamp),
        text: truncated.preview_text,
        full_text: truncated.full_text,
        is_truncated: truncated.is_truncated,
        attachments: (!attachments.is_empty()).then_some(attachments),
        tool_name: None,
        files: None,
    })
}

/// 목록에 내보내기 전에 첫 프롬프트와 제목을 미리보기 길이로 자른다
pub fn finalize_session(mut session: AggregatedAiSession) -> AggregatedAiSession {
    session.first_user_prompt = session
        .first_user_prompt
        .filter(|prompt| !prompt.is_empty())
        .map(|prompt| truncate_text(&prompt));
    session.title = session
        .title
        .filter(|title| !title.is_empty())
        .map(|title| truncate_text_to(&title, SESSION_TITLE_MAX_LENGTH));
    session
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // parity: TS-aiSessions-shared-4c29d3
    #[test]
    fn normalize_removes_pasted_placeholders_and_transport_noise() {
        let result = normalize_text(
            "Fix hook state\n[Pasted ~33 lines]\n[remote-ssh] command failed {\nsshHost: 'roky-home'\nerror: 'server exited unexpectedly'\n    at createSessionWithoutWorktree (/tmp/worktree.js:1:1)\nmain branch tmux session failed",
        );
        assert_eq!(result, "Fix hook state main branch tmux session failed");
    }

    const NOISE_LINES: [&str; 3] = [
        "error: build failed",
        "command: pnpm test",
        "    at foo (file.ts:1:2)",
    ];

    fn lines(parts: &[&str]) -> String {
        parts.join("\n")
    }

    // parity: TS-aiSessions-shared-ce4446
    #[test]
    fn keeps_noise_lines_inside_code_blocks() {
        let source = lines(
            &[
                &["빌드 결과입니다", "```yaml"],
                &NOISE_LINES[..],
                &["```", "끝"],
            ]
            .concat(),
        );
        assert_eq!(preserve_message_text(&source), source);
    }

    // parity: TS-aiSessions-shared-17b12f
    #[test]
    fn removes_noise_lines_outside_code_blocks() {
        let source = lines(&[&["빌드 결과입니다"], &NOISE_LINES[..], &["끝"]].concat());
        assert_eq!(preserve_message_text(&source), "빌드 결과입니다\n끝");
    }

    // parity: TS-aiSessions-shared-fe906d
    #[test]
    fn unclosed_code_block_runs_to_the_end() {
        let source = lines(&[&["error: 블록 밖", "```"], &NOISE_LINES[..]].concat());
        assert_eq!(
            preserve_message_text(&source),
            lines(&[&["```"], &NOISE_LINES[..]].concat())
        );
    }

    // parity: TS-aiSessions-shared-0f931e
    #[test]
    fn tilde_fences_behave_like_backtick_fences() {
        let block = [&["~~~"], &NOISE_LINES[..], &["~~~"]].concat();
        let source = lines(&[&block[..], &["error: 블록 밖"]].concat());
        assert_eq!(preserve_message_text(&source), lines(&block));
    }

    // parity: TS-aiSessions-shared-736f76
    #[test]
    fn shorter_fence_does_not_close_block() {
        let outer_block = [
            "````md",
            "```yaml",
            "command: a",
            "```",
            "error: 바깥 블록 안",
            "````",
        ];
        let source = lines(&[&outer_block[..], &["error: 블록 밖"]].concat());
        assert_eq!(preserve_message_text(&source), lines(&outer_block));
    }

    // parity: TS-aiSessions-shared-190cb1
    #[test]
    fn fence_with_language_does_not_close_block() {
        let block = ["```", "code", "```js", "error: 아직 블록 안", "```"];
        let source = lines(&[&block[..], &["error: 블록 밖"]].concat());
        assert_eq!(preserve_message_text(&source), lines(&block));
    }

    // parity: TS-aiSessions-shared-9029c0
    #[test]
    fn longer_same_fence_closes_and_other_marker_does_not() {
        let block = ["```", "~~~~", "error: 블록 안", "`````"];
        let source = lines(&[&block[..], &["error: 블록 밖"]].concat());
        assert_eq!(preserve_message_text(&source), lines(&block));
    }

    // parity: TS-aiSessions-shared-548487
    #[test]
    fn keeps_markdown_line_breaks_for_detail_and_flat_preview() {
        let source = json!({
            "text": "## Markdown summary\n\n- **Strong** list item\n- Safe [link](https://example.com)\n\n```ts\nconst rendered = true;\n```\n\n```mermaid\ngraph TD\n  A[AI history loaded] --> B[Markdown rendered]\n  B --> C[Mermaid SVG rendered]\n```"
        });
        let full_text = extract_plain_text(&source);
        let preview = make_preview_message(
            AiMessageRole::Assistant,
            &json!("2026-01-01T00:00:00.000Z"),
            &full_text,
            Vec::new(),
        )
        .unwrap();

        assert!(full_text.contains("## Markdown summary\n"));
        assert!(full_text.contains("```mermaid\ngraph TD"));
        assert!(
            preview
                .text
                .contains("## Markdown summary - **Strong** list item")
        );
        assert_eq!(preview.full_text, full_text);
    }

    #[test]
    fn truncate_counts_utf16_units_and_trims_before_ellipsis() {
        let long = format!("{} {}", "a".repeat(238), "bbbb");
        assert_eq!(truncate_text(&long), format!("{}...", "a".repeat(238)));
        assert_eq!(truncate_text("short"), "short");
    }

    #[test]
    fn to_iso_string_handles_seconds_milliseconds_and_unparsable_text() {
        assert_eq!(
            to_iso_string(&json!(1_700_000_000)).as_deref(),
            Some("2023-11-14T22:13:20.000Z")
        );
        assert_eq!(
            to_iso_string(&json!(1_700_000_000_123_i64)).as_deref(),
            Some("2023-11-14T22:13:20.123Z")
        );
        assert_eq!(
            to_iso_string(&json!("yesterday")).as_deref(),
            Some("yesterday")
        );
        assert_eq!(to_iso_string(&json!(12)), None);
        assert_eq!(to_iso_string(&Value::Null), None);
    }
}
