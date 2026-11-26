//! TS OpenCode 설치기가 실제로 만든 산출물과 Rust 산출물을 바이트 단위로 비교한다.
//! 골든은 scratch의 `capture-opencode.ts`를 esbuild로 묶어 Electron의 Node로 실행해 캡처했고, 입력값은 캡처 스크립트와 같다.

use serde_json::json;

use super::HookProviderFile;
use super::open_code::{build_open_code_hook_files, generate_plugin_script};

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
fn plugin_scripts_match_ts_output() {
    assert_matches_golden(
        "opencode-plugin-local",
        &generate_plugin_script("http://localhost:9736", "task-1"),
    );
    assert_matches_golden(
        "opencode-plugin-escaped",
        &generate_plugin_script(
            "http://10.0.0.5:19736",
            "task-\"q\"\\</script>\u{2028}\u{0001}é",
        ),
    );
}

#[test]
fn install_files_match_ts_output() {
    let existing_tui = json!({
        "theme": "existing",
        "plugin": ["other-plugin", "/home/user/my repo/.opencode/kanvibe-tui.mjs", "third"],
        "nested": { "list": [], "object": {}, "flag": false, "count": 3, "text": "a\"b" },
    })
    .to_string();
    let cases = [
        (
            "opencode-files-fresh",
            "/repo",
            "task-1",
            "http://localhost:9736",
            String::new(),
        ),
        (
            "opencode-files-existing",
            "/home/user/my repo",
            "task-\"q\"",
            "http://10.0.0.5:9736",
            existing_tui,
        ),
        (
            "opencode-files-whitespace-tui",
            "/repo/",
            "task-2",
            "http://localhost:19736",
            "  \n\t".to_string(),
        ),
        (
            "opencode-files-no-plugin-key",
            "/repo",
            "task-3",
            "http://localhost:9736",
            r#"{"keybinds":{"leader":"ctrl+x"}}"#.to_string(),
        ),
    ];
    for (name, repo, task_id, url, tui_content) in cases {
        let files = build_open_code_hook_files(repo, task_id, url, &tui_content).unwrap();
        assert_matches_golden(name, &describe_files(&files));
    }
}

#[test]
fn invalid_tui_configs_are_rejected_like_ts() {
    for (name, content) in [
        ("not-json", "invalid existing config"),
        ("array", "[]"),
        ("null", "null"),
        ("string-plugin", r#"{"plugin":"x"}"#),
    ] {
        let golden = std::fs::read_to_string(format!(
            "{}/tests/fixtures/hooks/opencode-invalid-{name}.golden",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        assert!(golden.starts_with("error:"), "{name}");
        assert!(
            build_open_code_hook_files("/repo", "task-1", "http://localhost:9736", content)
                .is_err(),
            "{name}"
        );
    }
}
