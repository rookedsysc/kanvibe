//! TS hook 설치기(claude·codex·gemini, 설치 번들)가 실제로 만든 산출물과 Rust 산출물을 바이트 단위로 비교한다.
//! 골든은 `capture-hook-installers.ts`를 esbuild로 묶어 Electron의 Node로 실행해 캡처했고, 입력값은 캡처 스크립트와 같다.
//! 파일 목록은 `=== <경로> mode=<8진수|none>` 머리줄 뒤에 본문을 이어 붙인 한 파일로 비교한다.

use serde_json::json;

use super::HookProviderFile;
use super::claude::build_claude_hook_files;
use super::codex::{build_codex_hook_files, upsert_codex_config_toml, upsert_codex_hooks_json};
use super::gemini::build_gemini_hook_files;
use super::install_bundle::{
    HookInstallTicket, build_hook_install_bootstrap_command, build_hook_install_script,
};

const CLIENT_ID: &str = "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed";

fn assert_matches_golden(name: &str, actual: &str) {
    let path = format!(
        "{}/tests/fixtures/hooks/{name}.golden",
        env!("CARGO_MANIFEST_DIR")
    );
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    assert!(
        actual == expected,
        "{name} 골든과 다릅니다\n--- expected\n{expected}\n--- actual\n{actual}"
    );
}

fn describe_files(files: &[HookProviderFile]) -> String {
    files
        .iter()
        .map(|file| {
            let mode = file
                .mode
                .map_or("none".to_string(), |mode| format!("{mode:o}"));
            format!("=== {} mode={mode}\n{}", file.file_path, file.content)
        })
        .collect()
}

#[test]
fn claude_install_files_match_ts_output() {
    let existing = json!({
        "model": "opus",
        "hooks": {
            "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "echo user-prompt", "timeout": 5 }] }],
            "Stop": [{ "hooks": [{ "type": "command", "command": "\"/tmp/old-project/.claude/hooks/kanvibe-stop-hook.sh\"", "timeout": 10 }] }],
            "Notification": [{ "hooks": [{ "type": "command", "command": "notify-send hi" }] }],
        },
        "permissions": { "allow": ["Bash(ls:*)"] },
    })
    .to_string();

    assert_matches_golden(
        "installer-claude-fresh",
        &describe_files(&build_claude_hook_files(
            "/repo",
            "task-1",
            "http://localhost:9736",
            "",
        )),
    );
    assert_matches_golden(
        "installer-claude-existing",
        &describe_files(&build_claude_hook_files(
            "/home/user/my repo/",
            "task-\"q\"",
            "http://10.0.0.5:9736",
            &existing,
        )),
    );
    assert_matches_golden(
        "installer-claude-broken-settings",
        &describe_files(&build_claude_hook_files(
            "/repo",
            "task-1",
            "http://localhost:9736",
            "{not json",
        )),
    );
}

#[test]
fn gemini_install_files_match_ts_output() {
    let existing = json!({
        "hooks": { "BeforeAgent": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "existing-hook.sh", "timeout": 3000 }] }] },
        "customSetting": true,
    })
    .to_string();

    assert_matches_golden(
        "installer-gemini-fresh",
        &describe_files(&build_gemini_hook_files(
            "/repo",
            "task-1",
            "http://localhost:9736",
            "",
        )),
    );
    assert_matches_golden(
        "installer-gemini-existing",
        &describe_files(&build_gemini_hook_files(
            "/repo",
            "task-2",
            "http://localhost:19736",
            &existing,
        )),
    );
}

fn codex_existing_hooks() -> String {
    let hooks = json!({
        "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "say done", "timeout": 1 }] }] },
        "extra": [1, 2],
    });
    // TS는 `JSON.stringify(value, null, 4)`로 만든 4칸 들여쓰기 입력을 썼다
    let mut serialized = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut serialized, formatter);
    serde::Serialize::serialize(&hooks, &mut serializer).unwrap();
    String::from_utf8(serialized).unwrap()
}

#[test]
fn codex_install_files_match_ts_output() {
    let config = "model = \"gpt-5\"\nnotify = [\".codex/hooks/kanvibe-notify-hook.sh\"]\n[features]\ncodex_hooks = false\nfast_mode = true\n\n[[hooks.PreToolUse]]\nmatcher = \"^Bash$\"\n";

    assert_matches_golden(
        "installer-codex-fresh",
        &describe_files(&build_codex_hook_files(
            "/repo",
            "task-1",
            "http://localhost:3000",
            "",
            "",
        )),
    );
    assert_matches_golden(
        "installer-codex-existing",
        &describe_files(&build_codex_hook_files(
            "/repo",
            "task-1",
            "http://localhost:9736",
            config,
            &codex_existing_hooks(),
        )),
    );
    assert_matches_golden("codex-hooks-json-empty", &upsert_codex_hooks_json(""));
    assert_matches_golden(
        "codex-hooks-json-existing",
        &upsert_codex_hooks_json(&codex_existing_hooks()),
    );
}

#[test]
fn codex_config_toml_upserts_match_ts_output() {
    let cases = [
        ("empty", ""),
        (
            "comments-and-next-section",
            "[features] # user settings\nfast_mode = true\n[other] # next table\nhooks = false\n",
        ),
        (
            "no-features",
            "model = \"gpt-5\"\r\napproval = \"never\"\r\n\r\n",
        ),
        (
            "stale-flags",
            "model = \"gpt-5\"\n[features]\ncodex_hooks = false\ncodex_hook = true\nhooks = false\nfast_mode = true\n",
        ),
        (
            "two-codex-hooks",
            "[features]\ncodex_hooks = true\n  codex_hooks = false\nother = 1\n",
        ),
        (
            "legacy-notify-only",
            "notify = [\".codex/hooks/kanvibe-notify-hook.sh\"]   \n\n\nmodel = \"x\"\n",
        ),
        (
            "array-table-after",
            "model = \"gpt-5\"\n[features]\nfast_mode = true\n\n[[hooks.PreToolUse]]\nmatcher = \"^Bash$\"\n",
        ),
        (
            "features-last-with-trailing-blank",
            "[profile]\nname = 1\n[features]   # c\n\n\n",
        ),
    ];
    for (name, content) in cases {
        assert_matches_golden(
            &format!("codex-toml-{name}"),
            &upsert_codex_config_toml(content),
        );
    }
}

#[test]
fn install_bundle_scripts_match_ts_output() {
    let files = [
        HookProviderFile {
            file_path: "/home/user/project/.claude/hooks/kanvibe-stop-hook.sh".into(),
            content: "#!/bin/bash\necho '작업 완료'\n".into(),
            mode: Some(0o755),
        },
        HookProviderFile {
            file_path: "/home/user/project/.claude/settings.json".into(),
            content: "{\n  \"hooks\": {}\n}\n".into(),
            mode: None,
        },
        HookProviderFile {
            file_path: "/home/user/it's/file.txt".into(),
            content: String::new(),
            mode: Some(0o600),
        },
    ];
    let ticket = HookInstallTicket {
        token: "ab".repeat(24),
        script_sha256: "c".repeat(64),
    };

    assert_matches_golden("install-script", &build_hook_install_script(&files));
    assert_matches_golden("install-script-empty", &build_hook_install_script(&[]));
    assert_matches_golden(
        "install-bootstrap-legacy",
        &build_hook_install_bootstrap_command(
            "http://192.168.0.10:9736/",
            &ticket,
            CLIENT_ID,
            None,
        )
        .unwrap(),
    );
    assert_matches_golden(
        "install-bootstrap-namespace",
        &build_hook_install_bootstrap_command(
            "http://192.168.0.10:9736",
            &ticket,
            CLIENT_ID,
            Some("qa-fixture"),
        )
        .unwrap(),
    );
}
