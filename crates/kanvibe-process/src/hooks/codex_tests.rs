//! TS `codexHooksSetup.test.ts`를 옮긴 테스트.

use regex::Regex;

use super::client_runtime::HookInstallContext;
use super::codex::{
    CodexHooksStatus, get_codex_hooks_status, setup_codex_hooks, upsert_codex_config_toml,
    upsert_codex_hooks_json,
};
use super::installer_test_support::{local_install_context, replace_in_file};
use super::provider::ShellHookScriptsStatus;

const SCRIPT_NAMES: [&str; 4] = [
    "kanvibe-prompt-hook.sh",
    "kanvibe-permission-hook.sh",
    "kanvibe-pre-tool-hook.sh",
    "kanvibe-stop-hook.sh",
];

fn count_matches(pattern: &str, text: &str) -> usize {
    Regex::new(pattern).unwrap().find_iter(text).count()
}

fn has_line(pattern: &str, text: &str) -> bool {
    count_matches(pattern, text) > 0
}

struct CodexRepo {
    directory: tempfile::TempDir,
    context: HookInstallContext,
}

impl CodexRepo {
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
        setup_codex_hooks(&self.context, self.path(), task_id, kanvibe_url, None).unwrap();
    }

    fn status(&self, task_id: Option<&str>) -> CodexHooksStatus {
        get_codex_hooks_status(&self.context, self.path(), task_id, None, None).unwrap()
    }

    fn read(&self, relative_path: &str) -> String {
        std::fs::read_to_string(self.directory.path().join(relative_path)).unwrap()
    }
}

#[test]
fn keeps_commented_features_and_following_section_boundaries() {
    let content =
        "[features] # user settings\nfast_mode = true\n[other] # next table\nhooks = false\n";

    let updated = upsert_codex_config_toml(content);

    assert_eq!(
        updated,
        "[features] # user settings\nfast_mode = true\nhooks = true\n[other] # next table\nhooks = false\n"
    );
    assert_eq!(upsert_codex_config_toml(&updated), updated);
}

#[test]
fn enables_the_hooks_feature_flag_under_the_features_table() {
    let updated = upsert_codex_config_toml("model = \"gpt-5\"\n[features]\nfast_mode = true\n");

    assert!(updated.contains("[features]"));
    assert!(updated.contains("fast_mode = true"));
    assert!(has_line(r"(?m)^hooks = true$", &updated));
    assert!(!has_line(r"(?m)^codex_hooks\s*=", &updated));
    assert!(!has_line(r"(?m)^codex_hook\s*=", &updated));
}

#[test]
fn replaces_stale_hook_flags_and_removes_obsolete_codex_flags() {
    let content = [
        "model = \"gpt-5\"",
        "[features]",
        "codex_hooks = false",
        "codex_hook = true",
        "hooks = false",
        "fast_mode = true",
        "",
    ]
    .join("\n");

    let updated = upsert_codex_config_toml(&content);

    assert!(updated.contains("[features]"));
    assert!(updated.contains("fast_mode = true"));
    assert!(has_line(r"(?m)^hooks = true$", &updated));
    assert_eq!(count_matches(r"(?m)^hooks\s*=", &updated), 1);
    assert!(!has_line(r"(?m)^codex_hooks\s*=", &updated));
    assert!(!has_line(r"(?m)^codex_hook\s*=", &updated));
}

#[test]
fn removes_the_legacy_kanvibe_notify_entry() {
    let updated = upsert_codex_config_toml(
        "model = \"gpt-5\"\nnotify = [\".codex/hooks/kanvibe-notify-hook.sh\"]\n",
    );

    assert!(!updated.contains("notify = [\".codex/hooks/kanvibe-notify-hook.sh\"]"));
    assert!(updated.contains("[features]"));
    assert!(has_line(r"(?m)^hooks = true$", &updated));
    assert!(!has_line(r"(?m)^codex_hooks\s*=", &updated));
}

#[test]
fn keeps_inline_hooks_tables_outside_the_features_table() {
    let content = "model = \"gpt-5\"\n[features]\nfast_mode = true\n\n[[hooks.PreToolUse]]\nmatcher = \"^Bash$\"\n";

    let updated = upsert_codex_config_toml(content);

    assert!(has_line(
        r"\[features\][\s\S]*hooks = true[\s\S]*\[\[hooks\.PreToolUse\]\]",
        &updated
    ));
    assert!(updated.contains("matcher = \"^Bash$\""));
    assert_eq!(count_matches(r"(?m)^hooks\s*=", &updated), 1);
}

#[test]
fn registers_the_current_codex_lifecycle_hooks() {
    let updated = upsert_codex_hooks_json("").unwrap();

    for event in [
        "\"UserPromptSubmit\"",
        "\"PermissionRequest\"",
        "\"PreToolUse\"",
        "\"Stop\"",
    ] {
        assert!(updated.contains(event), "{event}");
    }
    for script_name in SCRIPT_NAMES {
        assert!(updated.contains(script_name), "{script_name}");
    }
}

#[test]
fn reinstall_preserves_user_hooks_in_codex_groups() {
    use serde_json::{Value, json};

    let settings: Value = serde_json::from_str(&upsert_codex_hooks_json("").unwrap()).unwrap();
    let mut grouped = settings.clone();
    let user_group = json!({
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "echo user", "timeout": 5 }],
    });
    grouped["hooks"]["PreToolUse"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(user_group["hooks"][0].clone());

    let updated: Value =
        serde_json::from_str(&upsert_codex_hooks_json(&grouped.to_string()).unwrap()).unwrap();
    assert_eq!(
        updated["hooks"]["PreToolUse"],
        json!([user_group, settings["hooks"]["PreToolUse"][0]])
    );
}

#[test]
fn creates_all_codex_hook_script_files() {
    let repo = CodexRepo::new(true);

    repo.setup("task-1", "http://localhost:3000");

    let status = repo.status(None);
    assert!(status.has_prompt_hook);
    assert!(status.has_permission_hook);
    assert!(status.has_pre_tool_hook);
    assert!(status.has_stop_hook);
}

#[test]
fn creates_config_toml_and_hooks_json_entries() {
    let repo = CodexRepo::new(true);

    repo.setup("task-1", "http://localhost:3000");

    let status = repo.status(None);
    assert!(status.has_config_entry);
    assert!(status.has_hooks_file);
    assert!(status.has_hook_entries);
    let prompt_hook = repo.read(".codex/hooks/kanvibe-prompt-hook.sh");
    for fragment in [
        "TASK_ID=\"task-1\"",
        "taskId",
        "status.json",
        "\\\"schemaVersion\\\":1",
    ] {
        assert!(prompt_hook.contains(fragment), "{fragment}");
    }
    let config = repo.read(".codex/config.toml");
    assert!(config.contains("[features]"));
    assert!(has_line(r"(?m)^hooks = true$", &config));
    assert!(!has_line(r"(?m)^codex_hooks\s*=", &config));
    let hooks = repo.read(".codex/hooks.json");
    for event in [
        "\"UserPromptSubmit\"",
        "\"PermissionRequest\"",
        "\"PreToolUse\"",
        "\"Stop\"",
    ] {
        assert!(hooks.contains(event), "{event}");
    }
}

#[test]
fn marks_installed_when_all_hook_files_and_config_exist() {
    let repo = CodexRepo::new(true);

    repo.setup("task-1", "http://localhost:3000");

    let status = repo.status(None);
    assert!(status.is_installed);
    assert!(status.scripts.has_status_json_persistence);
}

#[test]
fn legacy_status_md_codex_hooks_are_not_installed() {
    let repo = CodexRepo::new(true);
    repo.setup("task-1", "http://localhost:3000");
    for script_name in SCRIPT_NAMES {
        let script_path = repo.directory.path().join(".codex/hooks").join(script_name);
        replace_in_file(&script_path, "status.json", "status.md");
    }

    let status = repo.status(Some("task-1"));

    assert!(status.scripts.has_task_id_binding);
    assert!(status.scripts.has_status_mappings);
    assert!(!status.scripts.has_status_json_persistence);
    assert!(!status.is_installed);
}

#[test]
fn does_not_duplicate_hook_registrations_on_reinstall() {
    let repo = CodexRepo::new(true);
    repo.setup("task-1", "http://localhost:3000");

    repo.setup("task-1", "http://localhost:3000");

    let hooks = repo.read(".codex/hooks.json");
    for script_name in SCRIPT_NAMES {
        assert_eq!(hooks.matches(script_name).count(), 1, "{script_name}");
    }
}

#[test]
fn replaces_legacy_notify_config_on_reinstall() {
    let repo = CodexRepo::new(true);
    std::fs::create_dir_all(repo.directory.path().join(".codex")).unwrap();
    std::fs::write(
        repo.directory.path().join(".codex/config.toml"),
        "model = \"gpt-5\"\nnotify = [\".codex/hooks/kanvibe-notify-hook.sh\"]\n",
    )
    .unwrap();

    repo.setup("task-1", "http://localhost:3000");

    let config = repo.read(".codex/config.toml");
    assert!(!config.contains("notify = [\".codex/hooks/kanvibe-notify-hook.sh\"]"));
    assert!(config.contains("[features]"));
    assert!(has_line(r"(?m)^hooks = true$", &config));
    assert!(!has_line(r"(?m)^codex_hooks\s*=", &config));
}

#[test]
fn reports_not_installed_when_no_files_exist() {
    let repo = CodexRepo::new(true);

    let status = repo.status(None);

    assert!(!status.is_installed);
    assert!(!status.has_prompt_hook);
    assert!(!status.has_permission_hook);
    assert!(!status.has_pre_tool_hook);
    assert!(!status.has_stop_hook);
    assert!(!status.has_hooks_file);
    assert!(!status.has_config_entry);
}

#[test]
fn detects_the_installed_status_exactly() {
    let repo = CodexRepo::new(true);
    repo.setup("task-1", "http://localhost:3000");

    let status = repo.status(None);

    assert_eq!(
        status,
        CodexHooksStatus {
            is_installed: true,
            has_prompt_hook: true,
            has_permission_hook: true,
            has_pre_tool_hook: true,
            has_stop_hook: true,
            has_hooks_file: true,
            has_hook_entries: true,
            has_config_entry: true,
            scripts: ShellHookScriptsStatus {
                has_task_id_binding: true,
                has_registered_hook_target: true,
                has_status_mappings: true,
                has_status_json_persistence: true,
                has_bounded_notify_timeout: true,
                has_target_fanout: true,
                has_parallel_target_fanout: true,
                has_expected_hook_server_url: true,
                has_reachable_hook_server: true,
                bound_task_id: Some("task-1".into()),
                registered_hook_target_url: None,
                configured_hook_server_url: Some("http://localhost:3000".into()),
                expected_hook_server_url: None,
            },
        }
    );
}

#[test]
fn stays_installed_when_reachability_fails_but_configuration_matches() {
    let repo = CodexRepo::new(false);

    repo.setup("task-1", "http://localhost:9736");
    let status = repo.status(Some("task-1"));

    assert!(status.is_installed);
    assert!(status.scripts.has_expected_hook_server_url);
    assert!(!status.scripts.has_reachable_hook_server);
}

#[test]
fn task_not_registered_in_targets_is_not_installed() {
    let repo = CodexRepo::new(true);
    repo.setup("task-1", "http://localhost:9736");

    let status = repo.status(Some("task-2"));

    assert!(!status.is_installed);
    assert!(status.scripts.has_task_id_binding);
    assert!(!status.scripts.has_registered_hook_target);
    assert_eq!(status.scripts.bound_task_id.as_deref(), Some("task-1"));
}
