//! TS `geminiHooksSetup.test.ts`를 옮긴 테스트.

use serde_json::{Value, json};

use super::client_runtime::HookInstallContext;
use super::gemini::{GeminiHooksStatus, get_gemini_hooks_status, setup_gemini_hooks};
use super::installer_test_support::{local_install_context, replace_in_file};

const KANVIBE_URL: &str = "http://localhost:9736";

struct GeminiRepo {
    directory: tempfile::TempDir,
    context: HookInstallContext,
}

impl GeminiRepo {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let (context, _) = local_install_context(directory.path().to_str().unwrap(), |_| {});
        Self { directory, context }
    }

    fn path(&self) -> &str {
        self.directory.path().to_str().unwrap()
    }

    fn setup(&self, task_id: &str) {
        setup_gemini_hooks(&self.context, self.path(), task_id, KANVIBE_URL, None).unwrap();
    }

    fn status(&self, task_id: Option<&str>) -> GeminiHooksStatus {
        get_gemini_hooks_status(&self.context, self.path(), task_id, None, None).unwrap()
    }

    fn read(&self, relative_path: &str) -> String {
        std::fs::read_to_string(self.directory.path().join(relative_path)).unwrap()
    }

    fn settings(&self) -> Value {
        serde_json::from_str(&self.read(".gemini/settings.json")).unwrap()
    }

    fn write_settings(&self, content: &str) {
        std::fs::create_dir_all(self.directory.path().join(".gemini")).unwrap();
        std::fs::write(self.directory.path().join(".gemini/settings.json"), content).unwrap();
    }
}

fn first_command(settings: &Value, event: &str, index: usize) -> String {
    settings["hooks"][event][index]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_string()
}

fn bucket_length(settings: &Value, event: &str) -> usize {
    settings["hooks"][event].as_array().unwrap().len()
}

#[test]
fn creates_hook_scripts_and_settings_in_the_gemini_directory() {
    let repo = GeminiRepo::new();

    repo.setup("task-1");

    let prompt_script = repo.read(".gemini/hooks/kanvibe-prompt-hook.sh");
    let stop_script = repo.read(".gemini/hooks/kanvibe-stop-hook.sh");
    let settings = repo.settings();
    for fragment in [
        "#!/bin/bash",
        "BeforeAgent",
        "payloadBase64",
        KANVIBE_URL,
        "task-1",
        "taskId",
        "status.json",
        "\\\"schemaVersion\\\":1",
        "echo '{}'",
    ] {
        assert!(prompt_script.contains(fragment), "{fragment}");
    }
    assert!(first_command(&settings, "SessionStart", 0).contains("kanvibe-prompt-hook.sh"));
    for fragment in ["#!/bin/bash", "AfterAgent", "review", "echo '{}'"] {
        assert!(stop_script.contains(fragment), "{fragment}");
    }
    assert_eq!(bucket_length(&settings, "BeforeAgent"), 1);
    assert_eq!(bucket_length(&settings, "AfterAgent"), 1);
    assert!(first_command(&settings, "BeforeAgent", 0).contains("kanvibe-prompt-hook.sh"));
    assert!(first_command(&settings, "AfterAgent", 0).contains("kanvibe-stop-hook.sh"));
}

#[test]
fn preserves_existing_settings_entries_when_adding_hooks() {
    let repo = GeminiRepo::new();
    let existing = json!({
        "hooks": {
            "BeforeAgent": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "existing-hook.sh", "timeout": 3000 }] }],
        },
        "customSetting": true,
    });
    repo.write_settings(&existing.to_string());

    repo.setup("task-1");

    let settings = repo.settings();
    assert_eq!(settings["customSetting"], true);
    assert_eq!(bucket_length(&settings, "BeforeAgent"), 2);
    assert_eq!(
        first_command(&settings, "BeforeAgent", 0),
        "existing-hook.sh"
    );
    assert!(first_command(&settings, "BeforeAgent", 1).contains("kanvibe-prompt-hook.sh"));
}

#[test]
fn does_not_duplicate_hooks_when_called_multiple_times() {
    let repo = GeminiRepo::new();

    repo.setup("task-1");
    repo.setup("task-1");

    let settings = repo.settings();
    assert_eq!(bucket_length(&settings, "BeforeAgent"), 1);
    assert_eq!(bucket_length(&settings, "AfterAgent"), 1);
}

#[test]
fn repairs_stale_gemini_hook_command_entries_on_reinstall() {
    let repo = GeminiRepo::new();
    repo.setup("task-1");
    let stale = json!({
        "hooks": {
            "BeforeAgent": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "\"/tmp/old-project/.gemini/hooks/kanvibe-prompt-hook.sh\"", "timeout": 10000 }] }],
            "AfterAgent": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "\"/tmp/old-project/.gemini/hooks/kanvibe-stop-hook.sh\"", "timeout": 10000 }] }],
        },
    });
    repo.write_settings(&format!(
        "{}\n",
        serde_json::to_string_pretty(&stale).unwrap()
    ));

    let stale_status = repo.status(None);
    assert!(!stale_status.has_settings_entry);
    assert!(!stale_status.is_installed);

    repo.setup("task-1");
    let settings = repo.settings();
    assert_eq!(
        first_command(&settings, "BeforeAgent", 0),
        "\"$GEMINI_PROJECT_DIR\"/.gemini/hooks/kanvibe-prompt-hook.sh"
    );
    assert_eq!(
        first_command(&settings, "AfterAgent", 0),
        "\"$GEMINI_PROJECT_DIR\"/.gemini/hooks/kanvibe-stop-hook.sh"
    );
}

#[test]
fn reports_installed_after_setup() {
    let repo = GeminiRepo::new();
    repo.setup("task-1");

    let status = repo.status(None);

    assert!(status.is_installed);
    assert!(status.has_prompt_hook);
    assert!(status.has_stop_hook);
    assert!(status.has_settings_entry);
    assert!(status.scripts.has_status_json_persistence);
}

#[test]
fn legacy_status_md_gemini_hooks_are_not_installed() {
    let repo = GeminiRepo::new();
    repo.setup("task-1");
    for script_name in ["kanvibe-prompt-hook.sh", "kanvibe-stop-hook.sh"] {
        let script_path = repo
            .directory
            .path()
            .join(".gemini/hooks")
            .join(script_name);
        replace_in_file(&script_path, "status.json", "status.md");
    }

    let status = repo.status(Some("task-1"));

    assert!(status.scripts.has_task_id_binding);
    assert!(status.scripts.has_status_mappings);
    assert!(!status.scripts.has_status_json_persistence);
    assert!(!status.is_installed);
}

#[test]
fn reports_not_installed_when_no_hooks_exist() {
    let repo = GeminiRepo::new();

    let status = repo.status(None);

    assert!(!status.is_installed);
    assert!(!status.has_prompt_hook);
    assert!(!status.has_stop_hook);
    assert!(!status.has_settings_entry);
}

#[test]
fn reports_not_installed_when_scripts_exist_but_settings_entry_is_missing() {
    let repo = GeminiRepo::new();
    let hooks_directory = repo.directory.path().join(".gemini/hooks");
    std::fs::create_dir_all(&hooks_directory).unwrap();
    std::fs::write(
        hooks_directory.join("kanvibe-prompt-hook.sh"),
        "#!/bin/bash",
    )
    .unwrap();
    std::fs::write(hooks_directory.join("kanvibe-stop-hook.sh"), "#!/bin/bash").unwrap();

    let status = repo.status(None);

    assert!(status.has_prompt_hook);
    assert!(status.has_stop_hook);
    assert!(!status.has_settings_entry);
    assert!(!status.is_installed);
}

#[test]
fn task_not_registered_in_targets_is_not_installed() {
    let repo = GeminiRepo::new();
    repo.setup("task-1");

    let status = repo.status(Some("task-2"));

    assert!(!status.is_installed);
    assert!(status.scripts.has_task_id_binding);
    assert!(!status.scripts.has_registered_hook_target);
    assert_eq!(status.scripts.bound_task_id.as_deref(), Some("task-1"));
}
