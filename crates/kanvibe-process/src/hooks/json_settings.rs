//! Claude Code·Gemini CLI·Codex CLI가 함께 쓰는 JSON hook 설정 파일 조작(TS `jsonHookSettings.ts`).
//! 세 CLI 모두 `{ hooks: { <이벤트>: [ { matcher?, hooks: [{ type, command, timeout }] } ] } }` 구조라
//! 파싱·upsert·검증 규칙을 한곳에서 관리한다. 사용자가 쓴 키의 순서를 지키도록 serde_json `preserve_order`를 쓴다.

use serde_json::{Map, Value, json};

use super::client_runtime::HookInstallError;

pub type JsonHookSettings = Map<String, Value>;

/// 비어 있으면 빈 설정으로 본다. 주석이 든 설정이나 머지 충돌 중인 파일처럼 JSON 객체로 해석할 수 없는
/// 내용은 hooks만 남기고 덮어쓰면 사용자 설정이 사라지므로, OpenCode `tui.json`처럼 오류로 돌려준다
pub fn parse_json_hook_settings(
    content: &str,
    settings_label: &str,
) -> Result<JsonHookSettings, HookInstallError> {
    if content.trim().is_empty() {
        return Ok(JsonHookSettings::new());
    }
    let invalid_settings_error =
        |reason: String| HookInstallError::Message(format!("Invalid {settings_label}: {reason}"));
    match serde_json::from_str(content) {
        Ok(Value::Object(settings)) => Ok(settings),
        Ok(_) => Err(invalid_settings_error("JSON object가 아닙니다".into())),
        Err(error) => Err(invalid_settings_error(error.to_string())),
    }
}

/// `JSON.stringify(settings, null, 2) + "\n"`과 같은 모양
pub fn serialize_json_hook_settings(settings: &JsonHookSettings) -> String {
    let serialized = serde_json::to_string_pretty(settings).expect("JSON 값은 항상 직렬화된다");
    format!("{serialized}\n")
}

fn is_json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// settings의 hooks bucket 맵을 보장하고 돌려준다. 객체가 아닌 값은 빈 맵으로 바꾼다
pub fn ensure_json_hook_buckets(settings: &mut JsonHookSettings) -> &mut Map<String, Value> {
    let hooks = settings
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    if !is_json_truthy(hooks) || !hooks.is_object() {
        *hooks = Value::Object(Map::new());
    }
    hooks.as_object_mut().expect("방금 객체로 맞췄다")
}

pub fn build_command_hook_entry(command: &str, timeout: u32) -> Value {
    json!({ "hooks": [{ "type": "command", "command": command, "timeout": timeout }] })
}

pub fn build_matcher_command_hook_entry(matcher: &str, command: &str, timeout: u32) -> Value {
    let mut entry = Map::new();
    entry.insert("matcher".into(), Value::String(matcher.to_string()));
    entry.insert(
        "hooks".into(),
        build_command_hook_entry(command, timeout)["hooks"].clone(),
    );
    Value::Object(entry)
}

fn invokes_managed_script(hook: &Value, script_path: &str) -> bool {
    let Some(command) = hook["command"]
        .as_str()
        .filter(|_| hook["type"] == "command")
    else {
        return false;
    };
    let command = command.trim();
    let script = command
        .strip_prefix("bash ")
        .unwrap_or(command)
        .trim_start();
    let quoted_path = script
        .strip_prefix('"')
        .and_then(|path| path.strip_suffix('"'))
        .or_else(|| {
            script
                .strip_prefix('\'')
                .and_then(|path| path.strip_suffix('\''))
        });
    if quoted_path.is_none() && script.contains(char::is_whitespace) {
        return false;
    }
    let path = quoted_path.unwrap_or(script);
    path == script_path
        || path.strip_suffix(script_path).is_some_and(|prefix| {
            prefix.ends_with('/')
                && (prefix.starts_with(['/', '$'])
                    || prefix.starts_with("\"$")
                    || matches!(prefix, "./" | "../"))
        })
}

fn preserve_hook_group(mut group: Value, script_path: &str) -> Option<Value> {
    if let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) {
        let original_len = hooks.len();
        hooks.retain(|hook| !invokes_managed_script(hook, script_path));
        if original_len != 0 && hooks.is_empty() {
            return None;
        }
    }
    Some(group)
}

/// 그룹 안의 KanVibe 명령만 새로 설치하고 사용자 명령과 매처는 보존한다
pub fn upsert_json_hook_entries(
    hook_entries: Option<&Value>,
    script_path: &str,
    next_entry: Value,
) -> Vec<Value> {
    let mut preserved_entries: Vec<Value> = match hook_entries {
        Some(Value::Array(entries)) => entries
            .iter()
            .cloned()
            .filter_map(|group| preserve_hook_group(group, script_path))
            .collect(),
        _ => Vec::new(),
    };
    preserved_entries.push(next_entry);
    preserved_entries
}

fn has_command_hook(entry: &Value, command: &str) -> bool {
    entry["hooks"].as_array().is_some_and(|hooks| {
        hooks
            .iter()
            .any(|hook| hook["type"] == "command" && hook["command"] == command)
    })
}

pub fn has_command_hook_entry(hook_entries: &Value, command: &str) -> bool {
    hook_entries
        .as_array()
        .is_some_and(|entries| entries.iter().any(|entry| has_command_hook(entry, command)))
}

pub fn has_matcher_command_hook_entry(hook_entries: &Value, matcher: &str, command: &str) -> bool {
    hook_entries.as_array().is_some_and(|entries| {
        entries
            .iter()
            .any(|entry| entry["matcher"] == matcher && has_command_hook(entry, command))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_replaces_only_entries_that_reference_the_script() {
        let entries = json!([
            { "hooks": [{ "type": "command", "command": "echo user", "timeout": 5 }] },
            { "hooks": [{ "type": "command", "command": "/old/.claude/hooks/kanvibe-stop-hook.sh", "timeout": 10 }] },
        ]);

        let upserted = upsert_json_hook_entries(
            Some(&entries),
            ".claude/hooks/kanvibe-stop-hook.sh",
            build_command_hook_entry(".claude/hooks/kanvibe-stop-hook.sh", 10),
        );

        assert_eq!(upserted.len(), 2);
        assert_eq!(upserted[0]["hooks"][0]["command"], "echo user");
        assert!(has_command_hook_entry(
            &Value::Array(upserted.clone()),
            ".claude/hooks/kanvibe-stop-hook.sh"
        ));
        assert!(!has_command_hook_entry(
            &Value::Array(upserted),
            "/old/.claude/hooks/kanvibe-stop-hook.sh"
        ));
    }

    #[test]
    fn matcher_check_requires_both_matcher_and_command() {
        let entries = Value::Array(vec![build_matcher_command_hook_entry("Bash", "run.sh", 10)]);

        assert!(has_matcher_command_hook_entry(&entries, "Bash", "run.sh"));
        assert!(!has_matcher_command_hook_entry(&entries, "*", "run.sh"));
        assert!(!has_matcher_command_hook_entry(
            &json!({}),
            "Bash",
            "run.sh"
        ));
    }

    #[test]
    fn empty_settings_become_an_empty_object() {
        assert!(
            parse_json_hook_settings("", "settings.json")
                .unwrap()
                .is_empty()
        );
        assert!(
            parse_json_hook_settings(" \n", "settings.json")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            parse_json_hook_settings("{\"a\":1}", "settings.json").unwrap()["a"],
            1
        );
    }

    #[test]
    fn unparsable_or_non_object_settings_are_rejected_with_the_settings_label() {
        for content in [
            "{oops",
            "// 주석\n{\"a\": 1}",
            "<<<<<<< HEAD\n{}",
            "[1, 2]",
            "\"text\"",
        ] {
            let error = parse_json_hook_settings(content, "Claude settings.json")
                .expect_err("해석할 수 없는 설정은 거절한다");
            assert!(
                error
                    .to_string()
                    .starts_with("Invalid Claude settings.json: "),
                "{content:?} → {error}"
            );
        }
    }
}
