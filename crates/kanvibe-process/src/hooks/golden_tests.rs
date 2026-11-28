//! TS 생성기가 실제로 만든 출력(`tests/fixtures/hooks/*.golden`)과 Rust 생성기 출력을 바이트 단위로 비교한다.
//! 골든은 TS 생성기를 esbuild로 묶어 Electron의 Node로 실행해 캡처했다. 입력값은 캡처 스크립트와 같다.

use serde_json::json;

use super::json_settings::{
    build_command_hook_entry, build_matcher_command_hook_entry, ensure_json_hook_buckets,
    parse_json_hook_settings, serialize_json_hook_settings, upsert_json_hook_entries,
};
use super::shell_script::{
    ShellHookAgentProvider, ShellHookStatus, build_shell_kanvibe_status_updater,
    build_shell_task_id_resolver,
};
use super::socket_path::{
    build_hook_socket_file_name, build_remote_hook_socket_prepare_command,
    build_shell_hook_socket_resolver, build_shell_hook_transport_resolver,
    build_shell_owned_hook_socket_assignment,
};
use crate::task_cli::script::{build_task_cli_command, build_task_cli_file, get_task_cli_path};

const CLIENT_ID: &str = "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed";

fn golden(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/hooks/{name}.golden",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn assert_matches_golden(name: &str, actual: &str) {
    let expected = golden(name);
    assert!(
        actual == expected,
        "{name} 골든과 다릅니다\n--- expected\n{expected}\n--- actual\n{actual}"
    );
}

#[test]
fn status_updaters_match_ts_output_for_every_status_and_provider() {
    let statuses = [
        ShellHookStatus::Progress,
        ShellHookStatus::Pending,
        ShellHookStatus::Review,
        ShellHookStatus::Done,
    ];
    let providers = [
        ShellHookAgentProvider::Claude,
        ShellHookAgentProvider::Codex,
        ShellHookAgentProvider::Gemini,
    ];
    for status in statuses {
        let name = format!("status-updater-{}", status.as_str());
        assert_matches_golden(&name, &build_shell_kanvibe_status_updater(status, None));
        for provider in providers {
            assert_matches_golden(
                &format!("{name}-{}", provider.as_str()),
                &build_shell_kanvibe_status_updater(status, Some(provider)),
            );
        }
    }
}

#[test]
fn task_id_resolvers_match_ts_output() {
    assert_matches_golden(
        "task-id-resolver-plain",
        &build_shell_task_id_resolver("fallback-task"),
    );
    assert_matches_golden(
        "task-id-resolver-special",
        &build_shell_task_id_resolver("task-\"quoted\"-\\-`cmd`-$HOME"),
    );
}

#[test]
fn socket_path_shell_fragments_match_ts_output() {
    assert_matches_golden("socket-resolver", &build_shell_hook_socket_resolver());
    assert_matches_golden("transport-resolver", &build_shell_hook_transport_resolver());
    assert_matches_golden(
        "socket-file-name",
        &build_hook_socket_file_name(CLIENT_ID).unwrap(),
    );
    assert_matches_golden(
        "socket-assignment-legacy",
        &build_shell_owned_hook_socket_assignment("found", CLIENT_ID, None).unwrap(),
    );
    for namespace in ["dev", "prod", "qa-fixture"] {
        assert_matches_golden(
            &format!("socket-assignment-{namespace}"),
            &build_shell_owned_hook_socket_assignment("kanvibe_socket", CLIENT_ID, Some(namespace))
                .unwrap(),
        );
    }
    assert_matches_golden(
        "socket-prepare-command",
        &build_remote_hook_socket_prepare_command(CLIENT_ID).unwrap(),
    );
}

#[test]
fn task_cli_outputs_match_ts_output() {
    assert_matches_golden(
        "task-cli-script",
        &build_task_cli_file("/repo", None).content,
    );
    assert_matches_golden(
        "task-cli-path-local",
        &get_task_cli_path("/home/user/repo with space", None),
    );
    assert_matches_golden(
        "task-cli-path-remote",
        &get_task_cli_path("/srv/repo/", Some("remote-devbox")),
    );
    assert_matches_golden(
        "task-cli-command-local",
        &build_task_cli_command("/home/user/repo with space", &["delete", "--yes"], None),
    );
    assert_matches_golden(
        "task-cli-command-remote",
        &build_task_cli_command(
            "/srv/repo",
            &["propagate-register", "--branch", "feat/it's new"],
            Some("remote-devbox"),
        ),
    );
}

#[test]
fn json_hook_settings_serialization_matches_ts_output() {
    let stop_command = "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/kanvibe-stop-hook.sh";
    let question_command = "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/kanvibe-question-hook.sh";
    let existing = json!({
        "model": "opus",
        "hooks": { "Stop": [
            { "hooks": [{ "type": "command", "command": "echo user-stop", "timeout": 5 }] },
            { "hooks": [{ "type": "command", "command": stop_command, "timeout": 10 }] },
        ] },
        "permissions": { "allow": ["Bash(ls:*)"] },
    });
    let mut settings = parse_json_hook_settings(&existing.to_string(), "settings.json").unwrap();
    let buckets = ensure_json_hook_buckets(&mut settings);
    let stop_entries = upsert_json_hook_entries(
        buckets.get("Stop"),
        ".claude/hooks/kanvibe-stop-hook.sh",
        build_command_hook_entry(stop_command, 10),
    );
    buckets.insert("Stop".into(), stop_entries.into());
    let question_entries = upsert_json_hook_entries(
        buckets.get("PreToolUse"),
        ".claude/hooks/kanvibe-question-hook.sh",
        build_matcher_command_hook_entry("AskUserQuestion", question_command, 10),
    );
    buckets.insert("PreToolUse".into(), question_entries.into());
    assert_matches_golden(
        "json-settings-upserted",
        &serialize_json_hook_settings(&settings),
    );

    let mut fresh = parse_json_hook_settings("", "settings.json").unwrap();
    let prompt_entries = upsert_json_hook_entries(
        None,
        ".codex/hooks/kanvibe-prompt-hook.sh",
        build_command_hook_entry(
            "bash \"$(git rev-parse --show-toplevel)/.codex/hooks/kanvibe-prompt-hook.sh\"",
            10,
        ),
    );
    ensure_json_hook_buckets(&mut fresh).insert("UserPromptSubmit".into(), prompt_entries.into());
    assert_matches_golden("json-settings-fresh", &serialize_json_hook_settings(&fresh));

    let unicode = json!({ "hooks": {}, "note": "한글 \u{0001} \"q\" \\ </script>", "n": 1.5, "list": [], "nested": { "empty": {} } });
    assert_matches_golden(
        "json-settings-unicode",
        &serialize_json_hook_settings(unicode.as_object().unwrap()),
    );
}
