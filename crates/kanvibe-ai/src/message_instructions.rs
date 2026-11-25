//! 사용자가 쓴 글이 아닌 자동 주입 문맥(지침, 슬래시 명령 기록, 작업 알림)을 가린다.

use regex::Regex;
use std::sync::LazyLock;

use crate::javascript::{JS_WHITESPACE_CLASS, js_trim};
use crate::types::{AggregatedAiMessage, AiMessageRole};

static AGENTS_INSTRUCTIONS_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^(?s:# AGENTS\.md instructions for [^\n]+\n.*<INSTRUCTIONS>.*</INSTRUCTIONS>[{JS_WHITESPACE_CLASS}]*(?:<environment_context>.*</environment_context>)?)$"
    ))
    .expect("AGENTS 지침 패턴")
});

/// JS 원본은 역참조(`<\/\1>`)로 같은 태그를 닫는다. Rust regex에는 역참조가 없어 태그마다 갈래를 펼쳤다
static SLASH_COMMAND_RECORD_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    let branches = ["command-name", "command-message", "command-args"]
        .map(|tag| format!("<{tag}>(?s:.)*?</{tag}>"))
        .join("|");
    Regex::new(&format!(r"^(?:(?:{branches})[{JS_WHITESPACE_CLASS}]*)+$")).expect("명령 기록 패턴")
});

static CONTEXT_BLOCK_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    let branches = [
        "environment_context",
        "skill",
        "system-reminder",
        "llm-wiki-context",
        "recommended_plugins",
        "local-command-stdout",
        "task-notification",
    ]
    .map(|tag| format!("<{tag}>(?s:.)*</{tag}>"))
    .join("|");
    Regex::new(&format!(r"^(?:(?:{branches})[{JS_WHITESPACE_CLASS}]*)$")).expect("자동 문맥 패턴")
});

/// 역할로 구분되는 지침과, CLI가 사용자 역할로 기록하는 독립된 자동 문맥만 숨김 대상으로 삼는다. 일반 요청에 인용된 태그는 유지한다.
/// Claude Code가 슬래시 명령을 실행할 때 남기는 명령 기록과 그 출력, 백그라운드 작업 알림도 여기에 든다
pub fn is_ai_instruction_message(message: &AggregatedAiMessage) -> bool {
    if matches!(
        message.role,
        AiMessageRole::System | AiMessageRole::Developer
    ) {
        return true;
    }
    if message.role != AiMessageRole::User || !message.attachment_list().is_empty() {
        return false;
    }
    let source = if message.full_text.is_empty() {
        &message.text
    } else {
        &message.full_text
    };
    let text = js_trim(source);
    AGENTS_INSTRUCTIONS_PATTERN.is_match(text)
        || SLASH_COMMAND_RECORD_PATTERN.is_match(text)
        || CONTEXT_BLOCK_PATTERN.is_match(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(role: AiMessageRole, text: &str) -> AggregatedAiMessage {
        AggregatedAiMessage::plain(role, None, text)
    }

    fn is_user_instruction(text: &str) -> bool {
        is_ai_instruction_message(&message(AiMessageRole::User, text))
    }

    // parity: TS-aiSessions-messageInstructions-175aec
    #[test]
    fn system_developer_and_standalone_agents_context_are_instructions() {
        assert!(is_ai_instruction_message(&message(
            AiMessageRole::System,
            "Rules"
        )));
        assert!(is_ai_instruction_message(&message(
            AiMessageRole::Developer,
            "Rules"
        )));
        assert!(is_user_instruction(
            "# AGENTS.md instructions for /repo\n<INSTRUCTIONS>Rules</INSTRUCTIONS>\n<environment_context>cwd</environment_context>"
        ));
        assert!(is_user_instruction(
            "<environment_context>cwd</environment_context>"
        ));
    }

    // parity: TS-aiSessions-messageInstructions-1d31ea
    #[test]
    fn real_requests_and_answers_stay_visible() {
        assert!(!is_user_instruction("AGENTS.md 내용을 설명해줘"));
        assert!(!is_user_instruction(
            "다음 <environment_context>cwd</environment_context>를 설명해줘"
        ));
        assert!(!is_ai_instruction_message(&message(
            AiMessageRole::Assistant,
            "<environment_context>example</environment_context>"
        )));
    }

    // parity: TS-aiSessions-messageInstructions-ac4811
    #[test]
    fn slash_command_records_and_notifications_are_instructions() {
        assert!(is_user_instruction(
            "<command-name>/model</command-name>\n            <command-message>model</command-message>\n            <command-args>sonnet</command-args>"
        ));
        assert!(is_user_instruction(
            "<command-message>example-skill</command-message>\n<command-name>/example-skill</command-name>\n<command-args>요청 내용</command-args>"
        ));
        assert!(is_user_instruction(
            "<local-command-stdout>Set model to `Sonnet` and saved as your default for new sessions</local-command-stdout>"
        ));
        assert!(is_user_instruction(
            "<task-notification>\n<task-id>task-1</task-id>\n<status>completed</status>\n</task-notification>"
        ));
    }

    // parity: TS-aiSessions-messageInstructions-1bdc84
    #[test]
    fn quoted_command_tags_stay_visible() {
        assert!(!is_user_instruction(
            "<command-name>/model</command-name> 이 기록이 뭔지 설명해줘"
        ));
        assert!(!is_user_instruction(
            "다음 <local-command-stdout>출력</local-command-stdout>을 설명해줘"
        ));
    }
}
