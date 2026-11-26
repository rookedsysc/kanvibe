//! TS `claudeHooksSetup.test.ts`를 옮긴 테스트. 실제 임시 repo에 설치하고 상태를 다시 읽는다.

use serde_json::{Value, json};

use super::claude::{ClaudeHooksStatus, get_claude_hooks_status, setup_claude_hooks};
use super::client_runtime::HookInstallContext;
use super::installer_test_support::{local_install_context, replace_in_file};

const SCRIPT_NAMES: [&str; 3] = [
    "kanvibe-prompt-hook.sh",
    "kanvibe-stop-hook.sh",
    "kanvibe-question-hook.sh",
];

struct ClaudeRepo {
    directory: tempfile::TempDir,
    context: HookInstallContext,
}

impl ClaudeRepo {
    fn new(is_local_server_healthy: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let (context, _) = local_install_context(directory.path().to_str().unwrap(), |runtime| {
            runtime.is_local_server_healthy = is_local_server_healthy;
        });
        Self { directory, context }
    }

    fn path(&self) -> &str {
        self.directory.path().to_str().unwrap()
    }

    fn setup(&self, task_id: &str, kanvibe_url: &str) {
        setup_claude_hooks(&self.context, self.path(), task_id, kanvibe_url, None).unwrap();
    }

    fn status(&self, task_id: Option<&str>) -> ClaudeHooksStatus {
        get_claude_hooks_status(&self.context, self.path(), task_id, None, None).unwrap()
    }

    fn hook_script(&self, script_name: &str) -> std::path::PathBuf {
        self.directory
            .path()
            .join(".claude/hooks")
            .join(script_name)
    }

    fn settings_path(&self) -> std::path::PathBuf {
        self.directory.path().join(".claude/settings.json")
    }
}

#[test]
fn every_hook_script_sends_only_the_status_without_auth_headers() {
    let repo = ClaudeRepo::new(true);

    repo.setup("task-1", "http://192.168.0.10:9736");

    let scripts: Vec<String> = SCRIPT_NAMES
        .iter()
        .map(|name| std::fs::read_to_string(repo.hook_script(name)).unwrap())
        .collect();
    assert!(
        scripts
            .iter()
            .all(|script| !script.contains("X-Kanvibe-Token"))
    );
    let prompt_script = &scripts[0];
    for fragment in [
        ".kanvibe",
        "status.json",
        "targets.json",
        "KANVIBE_TARGET_ROWS",
    ] {
        assert!(prompt_script.contains(fragment), "{fragment}");
    }
    assert!(!prompt_script.contains("hooks-targets.json"));
    assert!(!prompt_script.contains("task-state.json"));
    let status = repo.status(None);
    assert!(status.is_installed);
    assert!(status.scripts.has_status_json_persistence);
    assert!(status.scripts.has_target_fanout);
}

#[test]
fn legacy_status_md_hooks_are_not_installed_and_reinstall_repairs_them() {
    let repo = ClaudeRepo::new(true);
    repo.setup("task-1", "http://localhost:9736");
    for script_name in SCRIPT_NAMES {
        replace_in_file(&repo.hook_script(script_name), "status.json", "status.md");
    }

    let legacy_status = repo.status(Some("task-1"));
    assert!(legacy_status.scripts.has_task_id_binding);
    assert!(legacy_status.scripts.has_status_mappings);
    assert!(!legacy_status.scripts.has_status_json_persistence);
    assert!(!legacy_status.is_installed);

    repo.setup("task-1", "http://localhost:9736");
    let repaired_status = repo.status(Some("task-1"));
    assert!(repaired_status.scripts.has_status_json_persistence);
    assert!(repaired_status.is_installed);
}

fn stale_settings() -> Value {
    let stale_command = |script: &str| format!("\"/tmp/old-project/.claude/hooks/{script}\"");
    json!({
        "hooks": {
            "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": stale_command("kanvibe-prompt-hook.sh"), "timeout": 10 }] }],
            "PreToolUse": [{ "matcher": "AskUserQuestion", "hooks": [{ "type": "command", "command": stale_command("kanvibe-question-hook.sh"), "timeout": 10 }] }],
            "PostToolUse": [{ "matcher": "AskUserQuestion", "hooks": [{ "type": "command", "command": stale_command("kanvibe-prompt-hook.sh"), "timeout": 10 }] }],
            "Stop": [{ "hooks": [{ "type": "command", "command": stale_command("kanvibe-stop-hook.sh"), "timeout": 10 }] }],
        },
    })
}

#[test]
fn stale_command_entries_are_not_installed_and_are_repaired_on_reinstall() {
    let repo = ClaudeRepo::new(true);
    repo.setup("task-1", "http://localhost:9736");
    let stale_content = format!(
        "{}\n",
        serde_json::to_string_pretty(&stale_settings()).unwrap()
    );
    std::fs::write(repo.settings_path(), stale_content).unwrap();

    let stale_status = repo.status(None);
    assert!(!stale_status.has_settings_entry);
    assert!(!stale_status.is_installed);

    repo.setup("task-1", "http://localhost:9736");
    let repaired: Value =
        serde_json::from_str(&std::fs::read_to_string(repo.settings_path()).unwrap()).unwrap();
    let command = |event: &str| repaired["hooks"][event][0]["hooks"][0]["command"].clone();
    assert_eq!(
        command("UserPromptSubmit"),
        "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/kanvibe-prompt-hook.sh"
    );
    assert_eq!(
        command("PreToolUse"),
        "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/kanvibe-question-hook.sh"
    );
    assert_eq!(
        command("PostToolUse"),
        "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/kanvibe-prompt-hook.sh"
    );
    assert_eq!(
        command("Stop"),
        "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/kanvibe-stop-hook.sh"
    );
    let repaired_status = repo.status(None);
    assert!(repaired_status.has_settings_entry);
    assert!(repaired_status.is_installed);
}

#[test]
fn stays_installed_when_reachability_fails_but_configuration_is_correct() {
    let repo = ClaudeRepo::new(false);

    repo.setup("task-1", "http://localhost:9736");
    let status = repo.status(Some("task-1"));

    assert!(status.is_installed);
    assert!(status.scripts.has_expected_hook_server_url);
    assert!(!status.scripts.has_reachable_hook_server);
}

#[test]
fn task_not_registered_in_targets_is_not_installed() {
    let repo = ClaudeRepo::new(true);

    repo.setup("task-1", "http://localhost:9736");
    let status = repo.status(Some("task-2"));

    assert!(!status.is_installed);
    assert!(status.scripts.has_task_id_binding);
    assert!(!status.scripts.has_registered_hook_target);
    assert_eq!(status.scripts.bound_task_id.as_deref(), Some("task-1"));
}

#[test]
fn registered_client_stays_installed_when_another_client_rewrites_the_scripts() {
    let repo = ClaudeRepo::new(true);

    repo.setup("task-1", "http://localhost:9736");
    // 같은 worktree를 보는 다른 client가 자기 URL과 task로 hook script를 다시 기록한다
    repo.setup("task-9", "http://10.0.0.5:9736");
    let status = repo.status(Some("task-1"));

    assert!(status.is_installed);
    assert!(status.scripts.has_registered_hook_target);
    assert_eq!(
        status.scripts.registered_hook_target_url.as_deref(),
        Some("http://localhost:9736")
    );
    assert_eq!(status.scripts.bound_task_id.as_deref(), Some("task-9"));
}
