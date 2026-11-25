//! 도구 호출 기록을 도구 이름, 입력 요약, 만들거나 고친 파일로 옮긴다.

use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

use crate::javascript::{js_trim, js_trim_end, non_empty_str, stringify};
use crate::message_text::{extract_plain_text, split_lines, truncate_text};
use crate::types::{AggregatedAiMessage, AiFileChange, AiMessageFile};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallSummary {
    pub tool_name: String,
    /// 도구 입력을 한눈에 알아볼 수 있게 줄인 글
    pub summary: String,
    pub files: Vec<AiMessageFile>,
}

const CLAUDE_FILE_PATH_TOOLS: [&str; 5] = ["Read", "Write", "Edit", "MultiEdit", "NotebookEdit"];
const CODEX_SUMMARY_LINE_COUNT: usize = 3;

fn claude_file_change(tool_name: &str) -> Option<AiFileChange> {
    match tool_name {
        "Write" => Some(AiFileChange::Created),
        "Edit" | "MultiEdit" | "NotebookEdit" => Some(AiFileChange::Modified),
        _ => None,
    }
}

fn read_claude_tool_file_path(tool_input: &Value) -> Option<String> {
    let file_path = tool_input
        .get("file_path")
        .filter(|value| !value.is_null())
        .or_else(|| tool_input.get("notebook_path"));
    non_empty_str(file_path).map(str::to_string)
}

/// Claude 기록의 `tool_use` 조각 하나를 요약으로 옮긴다. 도구 호출이 아니면 `None`이다
pub fn summarize_claude_tool_use(part: &Value) -> Option<ToolCallSummary> {
    let record = part.as_object()?;
    if record.get("type").and_then(Value::as_str) != Some("tool_use") {
        return None;
    }
    let name = non_empty_str(record.get("name"))?;
    let tool_input = match record.get("input") {
        Some(input @ (Value::Object(_) | Value::Array(_))) => input.clone(),
        _ => Value::Object(Map::new()),
    };
    let file_path = CLAUDE_FILE_PATH_TOOLS
        .contains(&name)
        .then(|| read_claude_tool_file_path(&tool_input))
        .flatten();
    let command = (name == "Bash")
        .then(|| tool_input.get("command").and_then(Value::as_str))
        .flatten()
        .filter(|command| !command.is_empty());
    let summary = command
        .map(str::to_string)
        .or_else(|| file_path.clone())
        .unwrap_or_else(|| truncate_text(&stringify(&tool_input)));
    let files = match (file_path, claude_file_change(name)) {
        (Some(path), Some(change)) => vec![AiMessageFile { path, change }],
        _ => Vec::new(),
    };
    Some(ToolCallSummary {
        tool_name: name.to_string(),
        summary,
        files,
    })
}

static PATCH_HEADER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\*\*\* (Add|Update) File: ([^\n\r\x{2028}\x{2029}]+)$").expect("패치 머리줄 패턴")
});

/// `apply_patch` 본문의 파일 머리줄에서 만든 파일과 고친 파일을 읽는다. 지운 파일은 미리 볼 내용이 없어 넣지 않는다.
/// 머리줄은 줄 맨 앞에서 시작하므로 공백·`+`·`-`로 시작하는 본문 줄은 파일로 읽지 않는다
pub fn parse_apply_patch_files(patch_text: &Value) -> Vec<AiMessageFile> {
    let Some(patch_text) = patch_text.as_str() else {
        return Vec::new();
    };
    parse_apply_patch_text(patch_text)
}

fn parse_apply_patch_text(patch_text: &str) -> Vec<AiMessageFile> {
    let mut files: Vec<AiMessageFile> = Vec::new();
    for line in split_lines(patch_text) {
        let Some(header) = PATCH_HEADER_PATTERN.captures(js_trim_end(line)) else {
            continue;
        };
        let path = header[2].to_string();
        if files.iter().any(|file| file.path == path) {
            continue;
        }
        let change = if &header[1] == "Add" {
            AiFileChange::Created
        } else {
            AiFileChange::Modified
        };
        files.push(AiMessageFile { path, change });
    }
    files
}

fn read_codex_patch_files(tool_name: &str, input: &Value) -> Vec<AiMessageFile> {
    if tool_name == "apply_patch" {
        return parse_apply_patch_files(input);
    }
    // 요즘 Codex는 패치를 `exec` 스크립트 안의 `tools.apply_patch("...")` 문자열로 남기고, 줄바꿈이 `\n` 두 글자로 적혀 있다
    match input.as_str() {
        Some(script) if tool_name == "exec" && script.contains("tools.apply_patch(") => {
            parse_apply_patch_text(&script.replace("\\n", "\n"))
        }
        _ => Vec::new(),
    }
}

/// Codex 기록의 `custom_tool_call` 하나를 요약으로 옮긴다
pub fn summarize_codex_custom_tool_call(payload: &Value) -> ToolCallSummary {
    let tool_name = non_empty_str(payload.get("name"))
        .unwrap_or("tool")
        .to_string();
    let input = payload.get("input").unwrap_or(&Value::Null);
    let files = read_codex_patch_files(&tool_name, input);
    let input_lines: Vec<String> = match input.as_str() {
        Some(text) => split_lines(text)
            .filter(|line| !js_trim(line).is_empty())
            .map(str::to_string)
            .collect(),
        None => extract_plain_text(input)
            .split('\n')
            .map(str::to_string)
            .collect(),
    };
    let summary = if files.is_empty() {
        input_lines
            .iter()
            .take(CODEX_SUMMARY_LINE_COUNT)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    ToolCallSummary {
        summary: if summary.is_empty() {
            format!("{tool_name} called")
        } else {
            summary
        },
        tool_name,
        files,
    }
}

/// 리더가 만든 메시지에 도구 이름과 파일 목록을 얹는다. 없는 값은 필드 자체를 두지 않는다
pub fn with_tool_call(
    mut message: AggregatedAiMessage,
    tool_name: Option<&str>,
    files: &[AiMessageFile],
) -> AggregatedAiMessage {
    if let Some(name) = tool_name.filter(|name| !name.is_empty()) {
        message.tool_name = Some(name.to_string());
    }
    if !files.is_empty() {
        message.files = Some(files.to_vec());
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::javascript::utf16_length;
    use serde_json::json;

    fn file(path: &str, change: AiFileChange) -> AiMessageFile {
        AiMessageFile {
            path: path.to_string(),
            change,
        }
    }

    fn summary(
        tool_name: &str,
        summary: &str,
        files: Vec<AiMessageFile>,
    ) -> Option<ToolCallSummary> {
        Some(ToolCallSummary {
            tool_name: tool_name.to_string(),
            summary: summary.to_string(),
            files,
        })
    }

    // parity: TS-aiSessions-messageFiles-2d4b5a
    #[test]
    fn write_creates_and_edits_modify() {
        assert_eq!(
            summarize_claude_tool_use(
                &json!({"type": "tool_use", "name": "Write", "input": {"file_path": "/repo/a.md", "content": "x"}})
            ),
            summary(
                "Write",
                "/repo/a.md",
                vec![file("/repo/a.md", AiFileChange::Created)]
            )
        );
        assert_eq!(
            summarize_claude_tool_use(
                &json!({"type": "tool_use", "name": "MultiEdit", "input": {"file_path": "/repo/b.ts", "edits": []}})
            ),
            summary(
                "MultiEdit",
                "/repo/b.ts",
                vec![file("/repo/b.ts", AiFileChange::Modified)]
            )
        );
        assert_eq!(
            summarize_claude_tool_use(
                &json!({"type": "tool_use", "name": "NotebookEdit", "input": {"notebook_path": "/repo/c.ipynb", "new_source": "x"}})
            ),
            summary(
                "NotebookEdit",
                "/repo/c.ipynb",
                vec![file("/repo/c.ipynb", AiFileChange::Modified)]
            )
        );
    }

    // parity: TS-aiSessions-messageFiles-802805
    #[test]
    fn read_and_bash_summaries_carry_no_files() {
        assert_eq!(
            summarize_claude_tool_use(
                &json!({"type": "tool_use", "name": "Read", "input": {"file_path": "/repo/a.md", "limit": 10}})
            ),
            summary("Read", "/repo/a.md", vec![])
        );
        assert_eq!(
            summarize_claude_tool_use(
                &json!({"type": "tool_use", "name": "Bash", "input": {"command": "pnpm check", "description": "타입 검사"}})
            ),
            summary("Bash", "pnpm check", vec![])
        );
    }

    // parity: TS-aiSessions-messageFiles-b7f628
    #[test]
    fn other_tools_show_one_line_json() {
        assert_eq!(
            summarize_claude_tool_use(
                &json!({"type": "tool_use", "name": "ToolSearch", "input": {"query": "notebook", "max_results": 5}})
            ),
            summary(
                "ToolSearch",
                "{\"query\":\"notebook\",\"max_results\":5}",
                vec![]
            )
        );
        let long = summarize_claude_tool_use(
            &json!({"type": "tool_use", "name": "Agent", "input": {"prompt": "가".repeat(600)}}),
        )
        .unwrap()
        .summary;
        assert!(utf16_length(&long) <= 242);
        assert!(long.ends_with("..."));
    }

    // parity: TS-aiSessions-messageFiles-9d530e
    #[test]
    fn missing_or_non_string_paths_do_not_throw() {
        assert_eq!(
            summarize_claude_tool_use(
                &json!({"type": "tool_use", "name": "Write", "input": {"content": "x"}})
            ),
            summary("Write", "{\"content\":\"x\"}", vec![])
        );
        assert_eq!(
            summarize_claude_tool_use(
                &json!({"type": "tool_use", "name": "Edit", "input": {"file_path": 42}})
            ),
            summary("Edit", "{\"file_path\":42}", vec![])
        );
        assert_eq!(
            summarize_claude_tool_use(
                &json!({"type": "tool_use", "name": "Write", "input": "not an object"})
            ),
            summary("Write", "{}", vec![])
        );
    }

    // parity: TS-aiSessions-messageFiles-2c8795
    #[test]
    fn non_tool_parts_are_none() {
        assert_eq!(
            summarize_claude_tool_use(&json!({"type": "text", "text": "hello"})),
            None
        );
        assert_eq!(
            summarize_claude_tool_use(&json!({"type": "tool_use", "input": {}})),
            None
        );
        assert_eq!(summarize_claude_tool_use(&Value::Null), None);
        assert_eq!(summarize_claude_tool_use(&json!("tool_use")), None);
    }

    // parity: TS-aiSessions-messageFiles-d64f8b
    #[test]
    fn add_and_update_headers_become_files() {
        let patch = "*** Begin Patch\n*** Add File: docs/a.md\n+x\n*** Update File: src/b.ts\n@@\n-a\n+b\n*** Delete File: src/c.ts\n*** End Patch";
        assert_eq!(
            parse_apply_patch_files(&json!(patch)),
            [
                file("docs/a.md", AiFileChange::Created),
                file("src/b.ts", AiFileChange::Modified)
            ]
        );
    }

    // parity: TS-aiSessions-messageFiles-b075b9
    #[test]
    fn repeated_files_keep_first_header() {
        assert_eq!(
            parse_apply_patch_files(&json!("*** Add File: a.md\n*** Update File: a.md")),
            [file("a.md", AiFileChange::Created)]
        );
    }

    // parity: TS-aiSessions-messageFiles-2737ee
    #[test]
    fn body_lines_shaped_like_headers_are_ignored() {
        let patch = "*** Begin Patch\n*** Update File: src/a.test.ts\n@@\n *** Update File: fake/context.ts\n-*** Add File: fake/removed.ts\n+*** Add File: fake/added.ts\n*** End Patch";
        assert_eq!(
            parse_apply_patch_files(&json!(patch)),
            [file("src/a.test.ts", AiFileChange::Modified)]
        );
    }

    // parity: TS-aiSessions-messageFiles-3c056b
    #[test]
    fn non_string_or_pathless_headers_are_empty() {
        assert!(parse_apply_patch_files(&Value::Null).is_empty());
        assert!(parse_apply_patch_files(&json!({"patch": "*** Add File: a.md"})).is_empty());
        assert!(parse_apply_patch_files(&json!("*** Add File:\n*** Update File: ")).is_empty());
    }

    // parity: TS-aiSessions-messageFiles-8f0461
    #[test]
    fn exec_script_apply_patch_files_are_read() {
        let input = "text(await tools.apply_patch(\"*** Begin Patch\\n*** Add File: docs/a.md\\n+x\\n*** Update File: src/b.ts\\n@@\\n-a\\n+b\\n*** End Patch\"));";
        let result = summarize_codex_custom_tool_call(
            &json!({"type": "custom_tool_call", "name": "exec", "input": input}),
        );
        assert_eq!(result.tool_name, "exec");
        assert_eq!(result.summary, "docs/a.md\nsrc/b.ts");
        assert_eq!(
            result.files,
            [
                file("docs/a.md", AiFileChange::Created),
                file("src/b.ts", AiFileChange::Modified)
            ]
        );
    }

    // parity: TS-aiSessions-messageFiles-9273cf
    #[test]
    fn non_patch_calls_summarize_first_three_lines() {
        let input = "text(await tools.exec_command({cmd:\"ls\"}));\n\nline 2\nline 3\nline 4";
        let result = summarize_codex_custom_tool_call(
            &json!({"type": "custom_tool_call", "name": "exec", "input": input}),
        );
        assert_eq!(
            result.summary,
            "text(await tools.exec_command({cmd:\"ls\"}));\nline 2\nline 3"
        );
        assert!(result.files.is_empty());
    }

    // parity: TS-aiSessions-messageFiles-9f6cc9
    #[test]
    fn missing_name_or_input_does_not_throw() {
        let unnamed = summarize_codex_custom_tool_call(&json!({"type": "custom_tool_call"}));
        assert_eq!(
            (unnamed.tool_name.as_str(), unnamed.summary.as_str()),
            ("tool", "tool called")
        );
        let numeric = summarize_codex_custom_tool_call(
            &json!({"type": "custom_tool_call", "name": "apply_patch", "input": 42}),
        );
        assert_eq!(
            (numeric.tool_name.as_str(), numeric.summary.as_str()),
            ("apply_patch", "apply_patch called")
        );
        assert!(numeric.files.is_empty());
    }
}
