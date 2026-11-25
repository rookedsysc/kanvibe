//! TS `openCodeHooksSetup.test.ts`. 실제 임시 repo에 설치하고, OpenCode 등록 조회(`vi.mock`)는 가짜 조회기로 바꾼다.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;

use regex::Regex;

use super::client_runtime::HookInstallContext;
use super::installer_test_support::local_install_context;
use super::open_code::{OpenCodeHooksStatus, get_open_code_hooks_status, setup_open_code_hooks};
use super::open_code_registry::OpenCodePluginRegistration;
use super::socket_path::HOOK_TRANSPORT_VERSION_MARKER;

/// 다음 조회 하나에만 쓸 결과를 쌓아 두고, 없으면 repo의 플러그인 경로 하나를 돌려준다
pub(super) struct FakeOpenCodeRegistration {
    next_results: Mutex<VecDeque<Result<Vec<String>, String>>>,
    pub invalidated_paths: Mutex<Vec<String>>,
}

impl FakeOpenCodeRegistration {
    pub(super) fn new() -> Self {
        Self {
            next_results: Mutex::new(VecDeque::new()),
            invalidated_paths: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn answer_once(&self, result: Result<Vec<String>, String>) {
        self.next_results.lock().unwrap().push_back(result);
    }
}

impl OpenCodePluginRegistration for FakeOpenCodeRegistration {
    fn registered_kanvibe_plugin_urls(&self, repo_path: &str) -> Result<Vec<String>, String> {
        let next_result = self.next_results.lock().unwrap().pop_front();
        next_result.unwrap_or_else(|| {
            Ok(vec![format!(
                "file://{repo_path}/.opencode/plugins/kanvibe-plugin.ts"
            )])
        })
    }

    fn invalidate(&self, repo_path: &str) {
        self.invalidated_paths
            .lock()
            .unwrap()
            .push(repo_path.to_string());
    }
}

struct OpenCodeFixture {
    _directory: tempfile::TempDir,
    repo: String,
    context: HookInstallContext,
    registration: FakeOpenCodeRegistration,
}

impl OpenCodeFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let (context, _) = local_install_context(&repo, |_| {});
        Self {
            _directory: directory,
            repo,
            context,
            registration: FakeOpenCodeRegistration::new(),
        }
    }

    fn setup(
        &self,
        task_id: &str,
        kanvibe_url: &str,
    ) -> Result<(), super::client_runtime::HookInstallError> {
        setup_open_code_hooks(
            &self.context,
            &self.registration,
            &self.repo,
            task_id,
            kanvibe_url,
            None,
        )
    }

    fn status(&self, task_id: Option<&str>) -> OpenCodeHooksStatus {
        get_open_code_hooks_status(
            &self.context,
            &self.registration,
            &self.repo,
            task_id,
            None,
            None,
        )
        .unwrap()
    }

    fn path(&self, relative_path: &str) -> String {
        format!("{}/{relative_path}", self.repo)
    }

    fn plugin_content(&self) -> String {
        std::fs::read_to_string(self.path(".opencode/plugins/kanvibe-plugin.ts")).unwrap()
    }
}

fn write_file(path: &str, content: &str) {
    std::fs::create_dir_all(Path::new(path).parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn assert_matches(pattern: &str, content: &str) {
    assert!(
        Regex::new(&format!("(?s){pattern}"))
            .unwrap()
            .is_match(content),
        "{pattern}"
    );
}

// parity: TS-lib-openCodeHooksSetup-c10854
#[test]
fn preserves_tui_settings_and_other_plugins_and_reinstalls_a_missing_observer() {
    let fixture = OpenCodeFixture::new();
    let tui_path = fixture.path(".opencode/tui.json");
    write_file(
        &tui_path,
        r#"{"theme":"existing","plugin":["other-plugin"]}"#,
    );

    fixture.setup("task-1", "http://localhost:3000").unwrap();
    fixture.setup("task-1", "http://localhost:3000").unwrap();

    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&tui_path).unwrap()).unwrap();
    let observer = fixture.path(".opencode/kanvibe-tui.mjs");
    assert_eq!(
        config,
        serde_json::json!({ "theme": "existing", "plugin": ["other-plugin", observer] })
    );
    assert!(
        std::fs::read_to_string(&observer)
            .unwrap()
            .contains("api.route.current")
    );
    assert!(fixture.status(None).is_installed);
    std::fs::remove_file(&observer).unwrap();
    assert!(!fixture.status(None).is_installed);
}

// parity: TS-lib-openCodeHooksSetup-c8db45
#[test]
fn does_not_overwrite_an_unparseable_tui_config() {
    let fixture = OpenCodeFixture::new();
    let tui_path = fixture.path(".opencode/tui.json");
    write_file(&tui_path, "invalid existing config");

    assert!(fixture.setup("task-1", "http://localhost:3000").is_err());
    assert_eq!(
        std::fs::read_to_string(&tui_path).unwrap(),
        "invalid existing config"
    );
}

// parity: TS-lib-openCodeHooksSetup-7f270b
#[test]
fn reports_a_missing_pane_chat_binding_as_the_failed_check() {
    let fixture = OpenCodeFixture::new();
    fixture.setup("task-1", "http://localhost:3000").unwrap();
    assert!(fixture.status(None).has_pane_chat_binding);

    std::fs::remove_file(fixture.path(".opencode/kanvibe-tui.mjs")).unwrap();

    let status = fixture.status(None);
    assert!(!status.is_installed);
    assert!(!status.has_pane_chat_binding);
}

// parity: TS-lib-openCodeHooksSetup-53269a
#[test]
fn creates_the_plugin_file_under_opencode_plugins() {
    let fixture = OpenCodeFixture::new();

    fixture.setup("task-1", "http://localhost:3000").unwrap();

    assert!(fixture.status(None).has_plugin);
}

// parity: TS-lib-openCodeHooksSetup-235d07
#[test]
fn generates_a_plugin_with_task_binding_state_file_and_target_fanout() {
    let fixture = OpenCodeFixture::new();

    fixture.setup("task-1", "http://localhost:3000").unwrap();

    let content = fixture.plugin_content();
    for fragment in [
        "KanvibePlugin",
        "/api/hooks/status",
        "const TASK_ID = \"task-1\";",
        "status.json",
        "targets.json",
        "readKanvibeTargets",
        "seenUrls",
        "--git-common-dir",
        ".kanvibe/",
        "writeFileSync(",
        "schemaVersion: 1",
        "taskId: target.taskId",
    ] {
        assert!(content.contains(fragment), "{fragment}");
    }
    for absent in [".kanvibe/task-id", "hooks-targets.json", "task-state.json"] {
        assert!(!content.contains(absent), "{absent}");
    }
}

// parity: TS-lib-openCodeHooksSetup-b7b43d
#[test]
fn generates_a_plugin_with_every_status_event_handler() {
    let fixture = OpenCodeFixture::new();

    fixture.setup("task-1", "http://localhost:3000").unwrap();

    let content = fixture.plugin_content();
    for event in [
        "\"message.updated\"",
        "\"question.asked\"",
        "\"question.replied\"",
        "\"session.idle\"",
        "\"session.deleted\"",
    ] {
        assert!(content.contains(event), "{event}");
    }
}

// parity: TS-lib-openCodeHooksSetup-6c01ca
#[test]
fn maps_event_types_to_statuses() {
    let fixture = OpenCodeFixture::new();

    fixture.setup("task-1", "http://localhost:3000").unwrap();

    let content = fixture.plugin_content();
    assert_matches(r"message\.updated.*?role.*?user.*?progress", &content);
    assert_matches(r"question\.asked.*?pending", &content);
    assert_matches(r"question\.replied.*?progress", &content);
    assert_matches(r"session\.idle.*?review", &content);
    assert_matches(r"session\.deleted.*?done", &content);
}

// parity: TS-lib-openCodeHooksSetup-dd7909
#[test]
fn filters_subagent_sessions_before_updating_statuses() {
    let fixture = OpenCodeFixture::new();

    fixture.setup("task-1", "http://localhost:3000").unwrap();

    let content = fixture.plugin_content();
    for fragment in [
        "const sessionCache = new Map<string, boolean>()",
        "function getSessionID(source: any): string | undefined",
        "function getParentSessionID(source: any): string | null | undefined",
        "client.session.get",
        "sessionCache.has(sessionID)",
        "sessionCache.set(sessionID, isMain)",
        "result.data?.parentID",
        "properties?.info ?? (event as any).properties?.message",
        "return sessionCache.get(sessionID) ?? false",
        "lastUserMessageBySession",
        "buildMessageSignature",
        "dedupeMessage: true",
    ] {
        assert!(content.contains(fragment), "{fragment}");
    }
    assert_matches(r"message\.updated.*?isMainSession\(message\)", &content);
    for event in [
        "question\\.asked",
        "question\\.replied",
        "session\\.idle",
        "session\\.deleted",
    ] {
        assert_matches(
            &format!(r"{event}.*?isMainSession\(event\.properties\)"),
            &content,
        );
    }
}

// parity: TS-lib-openCodeHooksSetup-20cc77
#[test]
fn reinstalling_overwrites_the_existing_plugin() {
    let fixture = OpenCodeFixture::new();
    fixture.setup("task-1", "http://localhost:3000").unwrap();

    fixture.setup("task-1", "http://localhost:3000").unwrap();

    let status = fixture.status(None);
    assert!(status.is_installed);
    assert!(status.has_status_json_persistence);
    assert!(status.has_target_fanout);
}

const LEGACY_BRANCH_BOUND_PLUGIN: &str = r#"import type { Plugin } from "@opencode-ai/plugin";

export const KanvibePlugin: Plugin = async ({ $ }) => {
  const KANVIBE_URL = "http://localhost:3000";
  const PROJECT_NAME = "kanvibe";

  async function updateStatus(status: string): Promise<void> {
    const branchName = "feature/legacy";
    await fetch(`${KANVIBE_URL}/api/hooks/status`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ branchName, projectName: PROJECT_NAME, status }),
    });
  }

  return {
    event: async () => {
      await updateStatus("review");
    },
  };
};
"#;

// parity: TS-lib-openCodeHooksSetup-9ecd09
#[test]
fn repairs_stale_branch_bound_plugin_content_on_reinstall() {
    let fixture = OpenCodeFixture::new();
    write_file(
        &fixture.path(".opencode/plugins/kanvibe-plugin.ts"),
        LEGACY_BRANCH_BOUND_PLUGIN,
    );

    fixture.setup("task-2", "http://localhost:3000").unwrap();

    let content = fixture.plugin_content();
    let status = fixture.status(None);
    assert!(content.contains("const TASK_ID = \"task-2\";"));
    assert!(content.contains("taskId: TASK_ID"));
    assert!(!content.contains("branchName"));
    assert!(!content.contains("projectName"));
    assert_eq!(status.bound_task_id.as_deref(), Some("task-2"));
    assert!(status.has_task_id_binding);
}

// parity: TS-lib-openCodeHooksSetup-c022f9
#[test]
fn invalidates_registration_after_writing_the_plugin() {
    let fixture = OpenCodeFixture::new();

    fixture.setup("task-1", "http://localhost:3000").unwrap();

    assert_eq!(
        *fixture.registration.invalidated_paths.lock().unwrap(),
        std::slice::from_ref(&fixture.repo)
    );
}

// parity: TS-lib-openCodeHooksSetup-88b2a1
#[test]
fn keeps_cli_errors_distinct_from_an_unregistered_plugin() {
    let fixture = OpenCodeFixture::new();
    fixture
        .registration
        .answer_once(Err("OpenCode timeout".into()));

    let status = fixture.status(None);

    assert!(!status.is_installed);
    assert_eq!(status.registration.has_registered_plugin, None);
    assert_eq!(
        status.registration.plugin_registration_error.as_deref(),
        Some("OpenCode timeout")
    );
}

// parity: TS-lib-openCodeHooksSetup-2c71db
#[test]
fn preserves_global_plugin_diagnostics_when_the_local_plugin_file_is_missing() {
    let fixture = OpenCodeFixture::new();
    fixture.registration.answer_once(Ok(vec![
        "file:///global/kanvibe-plugin.ts".into(),
        "file:///other/kanvibe-plugin.ts".into(),
    ]));

    let status = fixture.status(None);

    assert!(!status.has_plugin);
    assert_eq!(status.registration.has_registered_plugin, Some(false));
    assert_eq!(
        status.registration.has_duplicate_kanvibe_plugins,
        Some(true)
    );
}

// parity: TS-lib-openCodeHooksSetup-eb3901
#[test]
fn reports_installed_after_setup() {
    let fixture = OpenCodeFixture::new();
    fixture.setup("task-1", "http://localhost:3000").unwrap();

    let status = fixture.status(None);

    assert!(status.is_installed);
    assert!(status.has_duplicate_progress_guard);
    assert!(status.has_event_mappings);
    assert!(status.has_status_json_persistence);
    assert_eq!(status.target_path, fixture.repo);
    assert_eq!(
        status.plugin_path,
        fixture.path(".opencode/plugins/kanvibe-plugin.ts")
    );
    assert_eq!(
        status.registration.registered_plugin_urls,
        Some(vec![format!(
            "file://{}/.opencode/plugins/kanvibe-plugin.ts",
            fixture.repo
        )])
    );
}

// parity: TS-lib-openCodeHooksSetup-2ebc6d
#[test]
fn treats_an_old_transport_version_plugin_as_needing_reinstall() {
    let fixture = OpenCodeFixture::new();
    fixture.setup("task-1", "http://localhost:3000").unwrap();
    let plugin_path = fixture.path(".opencode/plugins/kanvibe-plugin.ts");
    std::fs::write(
        &plugin_path,
        fixture
            .plugin_content()
            .replacen(HOOK_TRANSPORT_VERSION_MARKER, "", 1),
    )
    .unwrap();

    let status = fixture.status(Some("task-1"));

    assert!(!status.has_target_fanout);
    assert!(!status.is_installed);
}

// parity: TS-lib-openCodeHooksSetup-219c34
#[test]
fn does_not_treat_a_legacy_status_md_plugin_as_installed() {
    let fixture = OpenCodeFixture::new();
    fixture.setup("task-1", "http://localhost:3000").unwrap();
    let plugin_path = fixture.path(".opencode/plugins/kanvibe-plugin.ts");
    std::fs::write(
        &plugin_path,
        fixture.plugin_content().replace("status.json", "status.md"),
    )
    .unwrap();

    let status = fixture.status(Some("task-1"));

    assert!(status.has_task_id_binding);
    assert!(status.has_event_mappings);
    assert!(!status.has_status_json_persistence);
    assert!(!status.is_installed);
}

// parity: TS-lib-openCodeHooksSetup-dff8a6
#[test]
fn does_not_treat_a_plugin_without_common_exclude_update_as_installed() {
    let fixture = OpenCodeFixture::new();
    fixture.setup("task-1", "http://localhost:3000").unwrap();
    let plugin_path = fixture.path(".opencode/plugins/kanvibe-plugin.ts");
    let pattern = Regex::new(
        r"(?s)\n  const KANVIBE_STATE_DIR_EXCLUDE_PATTERN =.*?\n  function writeKanvibeTaskState",
    )
    .unwrap();
    let stripped = pattern
        .replace(
            &fixture.plugin_content(),
            "\n  function writeKanvibeTaskState",
        )
        .into_owned();
    std::fs::write(&plugin_path, stripped).unwrap();

    let status = fixture.status(Some("task-1"));

    assert!(status.has_task_id_binding);
    assert!(status.has_event_mappings);
    assert!(!status.has_status_json_persistence);
    assert!(!status.is_installed);
}

// parity: TS-lib-openCodeHooksSetup-8498c2
#[test]
fn reports_not_installed_when_no_plugin_exists() {
    let fixture = OpenCodeFixture::new();

    let status = fixture.status(None);

    assert!(!status.is_installed);
    assert!(!status.has_plugin);
}

// parity: TS-lib-openCodeHooksSetup-3886d1
#[test]
fn stays_installed_with_diagnostics_when_the_cli_registry_omits_the_plugin() {
    let fixture = OpenCodeFixture::new();
    fixture.setup("task-1", "http://localhost:9736").unwrap();
    fixture.registration.answer_once(Ok(Vec::new()));

    let status = fixture.status(Some("task-1"));

    assert!(status.is_installed);
    assert!(status.target_registration.has_registered_hook_target);
    assert_eq!(status.registration.has_registered_plugin, Some(false));
    assert_eq!(status.registration.registered_plugin_urls, Some(Vec::new()));
}

// parity: TS-lib-openCodeHooksSetup-233167
#[test]
fn stays_installed_with_diagnostics_when_the_cli_registry_query_fails() {
    let fixture = OpenCodeFixture::new();
    fixture.setup("task-1", "http://localhost:9736").unwrap();
    fixture
        .registration
        .answer_once(Err("OpenCode timeout".into()));

    let status = fixture.status(Some("task-1"));

    assert!(status.is_installed);
    assert!(status.target_registration.has_registered_hook_target);
    assert_eq!(status.registration.has_registered_plugin, None);
    assert_eq!(
        status.registration.plugin_registration_error.as_deref(),
        Some("OpenCode timeout")
    );
}

// parity: TS-lib-openCodeHooksSetup-74202f
#[test]
fn stays_installed_with_diagnostics_when_duplicate_plugins_are_registered() {
    let fixture = OpenCodeFixture::new();
    fixture.setup("task-1", "http://localhost:3000").unwrap();
    fixture.registration.answer_once(Ok(vec![
        format!(
            "file://{}/.opencode/plugins/kanvibe-plugin.ts",
            fixture.repo
        ),
        "file:///home/test/.config/opencode/plugins/kanvibe-plugin.ts".into(),
    ]));

    let status = fixture.status(None);

    assert!(status.is_installed);
    assert_eq!(status.registration.has_registered_plugin, Some(true));
    assert_eq!(
        status.registration.has_duplicate_kanvibe_plugins,
        Some(true)
    );
    assert_eq!(
        status
            .registration
            .registered_plugin_urls
            .map(|urls| urls.len()),
        Some(2)
    );
}

// parity: TS-lib-openCodeHooksSetup-ec1bfa
#[test]
fn does_not_treat_an_unregistered_task_as_installed() {
    let fixture = OpenCodeFixture::new();
    fixture.setup("task-1", "http://localhost:9736").unwrap();

    let status = fixture.status(Some("task-2"));

    assert!(!status.is_installed);
    assert!(status.has_task_id_binding);
    assert!(!status.target_registration.has_registered_hook_target);
    assert_eq!(status.bound_task_id.as_deref(), Some("task-1"));
}
