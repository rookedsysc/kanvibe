use serde_json::json;

use super::*;
use PaneChatModelProvider::{Claude, Codex};

fn claude_turn(model: &str, effort: &str, speed: &str) -> Value {
    json!({"type": "assistant", "effort": effort, "requestedModel": model, "message": {"model": model, "usage": {"speed": speed}}})
}

fn claude_command_output(text: &str) -> Value {
    json!({"type": "user", "message": {"content": format!("<local-command-stdout>{text}</local-command-stdout>")}})
}

fn codex_cache() -> Value {
    json!({"models": [
        {"slug": "gpt-hidden", "display_name": "Hidden", "visibility": "hide", "priority": 1, "supported_reasoning_levels": [{"effort": "low"}], "service_tiers": []},
        {"slug": "gpt-luna", "display_name": "GPT Luna", "visibility": "list", "priority": 3, "supported_reasoning_levels": [], "service_tiers": []},
        {"slug": "gpt-sol", "display_name": "GPT Sol", "visibility": "list", "priority": 2, "supported_reasoning_levels": [{"effort": "low"}, {"effort": "high"}, {"effort": "ultra"}], "service_tiers": [{"id": "priority", "name": "Fast"}]}
    ]})
}

fn state(model: Option<&str>, effort: Option<&str>, is_fast: Option<bool>) -> PaneChatModelState {
    PaneChatModelState {
        model: model.map(str::to_string),
        effort: effort.map(str::to_string),
        is_fast,
    }
}

fn codex_settings() -> PaneChatModelSettings {
    build_pane_chat_model_settings(
        Codex,
        state(Some("gpt-sol"), Some("high"), Some(false)),
        parse_codex_model_catalog(&codex_cache()),
    )
}

fn codex_settings_with_model(model: Option<&str>) -> PaneChatModelSettings {
    PaneChatModelSettings {
        model: model.map(str::to_string),
        ..codex_settings()
    }
}

fn claude_settings(model: Option<&str>) -> PaneChatModelSettings {
    build_pane_chat_model_settings(
        Claude,
        state(model, Some("medium"), Some(false)),
        Vec::new(),
    )
}

fn change(value: Value) -> PaneChatModelSettingChange {
    parse_pane_chat_model_setting_change(&value).expect("변경 모양")
}

// parity: TS-aiSessions-paneChatModelSettings-f01313
#[test]
fn claude_reads_latest_turn_model_and_effort() {
    let records = [
        claude_turn("claude-opus-5-5", "medium", "standard"),
        claude_turn("claude-fable-5-2", "high", "standard"),
    ];
    assert_eq!(
        read_claude_model_state(&records),
        state(Some("claude-fable-5-2"), Some("high"), Some(false))
    );
}

// parity: TS-aiSessions-paneChatModelSettings-611193
#[test]
fn claude_command_output_applies_before_next_turn() {
    let records = [
        claude_turn("claude-opus-5-5", "medium", "standard"),
        claude_command_output(
            "Set model to `Fable 5.2` and saved as your default for new sessions",
        ),
        claude_command_output("Set effort level to high (saved as your default for new sessions)"),
    ];
    let read = read_claude_model_state(&records);
    assert_eq!(
        (read.model.as_deref(), read.effort.as_deref()),
        (Some("Fable 5.2"), Some("high"))
    );
    assert_eq!(
        build_pane_chat_model_settings(Claude, read, Vec::new())
            .model
            .as_deref(),
        Some("fable")
    );
}

// parity: TS-aiSessions-paneChatModelSettings-504dca
#[test]
fn claude_does_not_invent_values() {
    let records = [
        json!({"type": "assistant", "message": {"model": "<synthetic>"}}),
        json!({"type": "user", "message": {"content": "안녕"}}),
    ];
    assert_eq!(read_claude_model_state(&records), state(None, None, None));
}

// parity: TS-aiSessions-paneChatModelSettings-bc709d
#[test]
fn claude_non_standard_speed_is_fast() {
    assert_eq!(
        read_claude_model_state(&[claude_turn("claude-opus-5-5", "medium", "fast")]).is_fast,
        Some(true)
    );
}

// parity: TS-aiSessions-paneChatModelSettings-f27432
#[test]
fn codex_settings_between_turns_win() {
    let records = [
        json!({"type": "turn_context", "payload": {"model": "gpt-sol", "effort": "low"}}),
        json!({"type": "event_msg", "payload": {"type": "thread_settings_applied", "thread_settings": {"model": "gpt-sol", "reasoning_effort": "ultra", "service_tier": "priority"}}}),
    ];
    assert_eq!(
        read_codex_model_state(&records),
        state(Some("gpt-sol"), Some("ultra"), Some(true))
    );
}

// parity: TS-aiSessions-paneChatModelSettings-8b0376
#[test]
fn codex_turn_context_does_not_decide_fast() {
    let records =
        [json!({"type": "turn_context", "payload": {"model": "gpt-sol", "effort": "low"}})];
    assert_eq!(
        read_codex_model_state(&records),
        state(Some("gpt-sol"), Some("low"), None)
    );
}

fn levels(ids: &[&str]) -> Vec<PaneChatEffortLevel> {
    to_effort_levels(ids.iter().copied())
}

// parity: TS-aiSessions-paneChatModelSettings-8625e1
#[test]
fn codex_catalog_drops_hidden_and_sorts_by_priority() {
    assert_eq!(
        parse_codex_model_catalog(&codex_cache()),
        [
            PaneChatModelOption {
                id: "gpt-sol".to_string(),
                label: "GPT Sol".to_string(),
                effort_levels: levels(&["low", "high", "ultra"]),
                supports_fast: true,
                key_order: PaneChatModelOptionKeyOrder::EffortLevelsFirst,
            },
            PaneChatModelOption {
                id: "gpt-luna".to_string(),
                label: "GPT Luna".to_string(),
                effort_levels: Vec::new(),
                supports_fast: false,
                key_order: PaneChatModelOptionKeyOrder::EffortLevelsFirst,
            },
        ]
    );
    assert_eq!(
        levels(&["low", "high", "ultra"])
            .iter()
            .map(|level| level.label.as_str())
            .collect::<Vec<_>>(),
        ["Light", "High", "Ultra"]
    );
}

// parity: TS-aiSessions-paneChatModelSettings-bd73f4
#[test]
fn malformed_catalog_is_empty() {
    for cache in [Value::Null, json!("not-json"), json!({"models": "none"})] {
        assert!(parse_codex_model_catalog(&cache).is_empty());
    }
}

// parity: TS-aiSessions-paneChatModelSettings-3f83d0
#[test]
fn claude_aliases_full_model_id_and_limits_fast() {
    let settings = claude_settings(Some("claude-sonnet-5-5"));
    assert_eq!(settings.model.as_deref(), Some("sonnet"));
    let models: Vec<(&str, bool)> = settings
        .models
        .iter()
        .map(|model| (model.id.as_str(), model.supports_fast))
        .collect();
    assert_eq!(
        models,
        [
            ("fable", true),
            ("opus", true),
            ("sonnet", false),
            ("haiku", false)
        ]
    );
}

// parity: TS-aiSessions-paneChatModelSettings-073582
#[test]
fn unknown_current_model_is_listed_without_options() {
    let settings = build_pane_chat_model_settings(
        Codex,
        state(Some("gpt-private"), None, None),
        parse_codex_model_catalog(&codex_cache()),
    );
    assert_eq!(
        settings.models[0],
        PaneChatModelOption {
            id: "gpt-private".to_string(),
            label: "gpt-private".to_string(),
            effort_levels: Vec::new(),
            supports_fast: false,
            key_order: PaneChatModelOptionKeyOrder::EffortLevelsFirst,
        }
    );
}

// parity: TS-aiSessions-paneChatModelSettings-33e521
#[test]
fn changes_become_cli_commands() {
    let cases = [
        (
            Claude,
            json!({"kind": "model", "model": "sonnet"}),
            "/model sonnet",
        ),
        (
            Claude,
            json!({"kind": "effort", "effort": "xhigh"}),
            "/effort xhigh",
        ),
        (Claude, json!({"kind": "fast", "isFast": true}), "/fast on"),
        (
            Claude,
            json!({"kind": "fast", "isFast": false}),
            "/fast off",
        ),
        (Claude, json!({"kind": "reset"}), "/effort auto"),
        (
            Codex,
            json!({"kind": "model", "model": "gpt-luna"}),
            "/model gpt-luna",
        ),
        (
            Codex,
            json!({"kind": "effort", "effort": "ultra"}),
            "/model --reasoning=ultra",
        ),
        (
            Codex,
            json!({"kind": "fast", "isFast": true}),
            "/model --speed=priority",
        ),
        (
            Codex,
            json!({"kind": "fast", "isFast": false}),
            "/model --speed=standard",
        ),
        (Codex, json!({"kind": "reset"}), "/model gpt-sol"),
    ];
    for (provider, value, command) in cases {
        let settings = if provider == Claude {
            claude_settings(Some("claude-opus-5-5"))
        } else {
            codex_settings()
        };
        assert_eq!(
            build_pane_chat_model_command(provider, &settings, &change(value)).as_deref(),
            Some(command)
        );
    }
}

// parity: TS-aiSessions-paneChatModelSettings-3b1a17
#[test]
fn unavailable_changes_make_no_command() {
    let cases = [
        (
            codex_settings(),
            json!({"kind": "model", "model": "gpt-sol; rm -rf /"}),
        ),
        (
            codex_settings(),
            json!({"kind": "effort", "effort": "medium"}),
        ),
        (
            codex_settings_with_model(Some("gpt-luna")),
            json!({"kind": "effort", "effort": "low"}),
        ),
        (
            codex_settings_with_model(Some("gpt-luna")),
            json!({"kind": "fast", "isFast": true}),
        ),
        (
            codex_settings_with_model(Some("gpt-luna")),
            json!({"kind": "reset"}),
        ),
        (
            codex_settings_with_model(None),
            json!({"kind": "effort", "effort": "low"}),
        ),
    ];
    for (settings, value) in cases {
        assert_eq!(
            build_pane_chat_model_command(Codex, &settings, &change(value)),
            None
        );
    }
}

#[test]
fn change_parsing_rejects_wrong_shapes() {
    assert_eq!(
        parse_pane_chat_model_setting_change(&json!({"kind": "fast", "isFast": "yes"})),
        None
    );
    assert_eq!(
        parse_pane_chat_model_setting_change(&json!(["reset"])),
        None
    );
    assert_eq!(
        serde_json::to_value(change(json!({"kind": "fast", "isFast": true}))).unwrap(),
        json!({"kind": "fast", "isFast": true})
    );
}

/// TS는 Claude 계열을 `{ ...family, effortLevels }`로, 현재 모델 항목과 Codex 목록은 `effortLevels`를 먼저 적는다
#[test]
fn serialized_model_options_keep_ts_key_order() {
    let mut state = read_claude_model_state(&[]);
    state.model = Some("claude-private-model".to_string());
    let settings = build_pane_chat_model_settings(Claude, state, Vec::new());
    let serialized: Vec<String> = settings
        .models
        .iter()
        .map(|option| serde_json::to_string(option).unwrap())
        .collect();
    assert_eq!(
        serialized[0],
        r#"{"id":"claude-private-model","label":"claude-private-model","effortLevels":[],"supportsFast":false}"#
    );
    assert!(
        serialized[1]
            .starts_with(r#"{"id":"fable","label":"Fable","supportsFast":true,"effortLevels":[{"#)
    );
    let codex = serde_json::to_string(&parse_codex_model_catalog(&codex_cache())[1]).unwrap();
    assert_eq!(
        codex,
        r#"{"id":"gpt-luna","label":"GPT Luna","effortLevels":[],"supportsFast":false}"#
    );
}
