//! Claude Code·Gemini CLI·Codex CLI가 함께 쓰는 JSON hook 설정 파일 조작(TS `jsonHookSettings.ts`).
//! 세 CLI 모두 `{ hooks: { <이벤트>: [ { matcher?, hooks: [{ type, command, timeout }] } ] } }` 구조라
//! 파싱·upsert·검증 규칙을 한곳에서 관리한다. 사용자가 쓴 키의 순서를 지키도록 serde_json `preserve_order`를 쓴다.

use serde_json::{Map, Value, json};

pub type JsonHookSettings = Map<String, Value>;

/// 비었거나 JSON 객체가 아니면 빈 설정으로 본다
pub fn parse_json_hook_settings(content: &str) -> JsonHookSettings {
    if content.is_empty() {
        return JsonHookSettings::new();
    }
    match serde_json::from_str(content) {
        Ok(Value::Object(settings)) => settings,
        _ => JsonHookSettings::new(),
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

fn references_script_name(entry: &Value, script_name: &str) -> bool {
    serde_json::to_string(entry)
        .unwrap_or_default()
        .contains(script_name)
}

/// 같은 스크립트를 가리키는 기존 entry만 교체하고 사용자가 추가한 다른 entry는 보존한다
pub fn upsert_json_hook_entries(
    hook_entries: Option<&Value>,
    script_name: &str,
    next_entry: Value,
) -> Vec<Value> {
    let mut preserved_entries: Vec<Value> = match hook_entries {
        Some(Value::Array(entries)) => entries
            .iter()
            .filter(|entry| !references_script_name(entry, script_name))
            .cloned()
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
            { "hooks": [{ "type": "command", "command": "old/kanvibe-stop-hook.sh", "timeout": 10 }] },
        ]);

        let upserted = upsert_json_hook_entries(
            Some(&entries),
            "kanvibe-stop-hook.sh",
            build_command_hook_entry("new/kanvibe-stop-hook.sh", 10),
        );

        assert_eq!(upserted.len(), 2);
        assert_eq!(upserted[0]["hooks"][0]["command"], "echo user");
        assert!(has_command_hook_entry(
            &Value::Array(upserted.clone()),
            "new/kanvibe-stop-hook.sh"
        ));
        assert!(!has_command_hook_entry(
            &Value::Array(upserted),
            "old/kanvibe-stop-hook.sh"
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
    fn unreadable_settings_become_an_empty_object() {
        assert!(parse_json_hook_settings("").is_empty());
        assert!(parse_json_hook_settings("{oops").is_empty());
        assert_eq!(parse_json_hook_settings("{\"a\":1}")["a"], 1);
    }
}
