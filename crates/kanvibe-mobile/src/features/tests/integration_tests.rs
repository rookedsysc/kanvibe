//! 실제 보드·diff 서비스와 임시 SQLite, 실제 git 저장소로 모바일 기능을 거친다(TS `mobileFeatureIntegration.test.ts`).
//! TS는 인증된 HTTP 경로까지 지났지만 HTTP는 `kanvibe-server`의 몫이라, 여기서는 기기 인증 판정과 기능 서비스를 이어 확인한다.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use kanvibe_db::repositories::projects::{NewProject, insert_project};
use kanvibe_db::repositories::tasks::{NewKanbanTask, find_task, insert_task};
use kanvibe_domain::notifications::DesktopNotificationPayload;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskStatus};
use kanvibe_process::exec::system::SystemShellCommandRunner;
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::task_target_propagation::PeerTargetResult;
use kanvibe_process::worktree::{HerdrTaskWorkspaces, ResourceCleanupOptions, WorktreeSession};
use kanvibe_services::board_notifier::BoardNotifier;
use kanvibe_services::diff::DiffService;
use kanvibe_services::kanban::ports::{
    HookVerificationFailure, HookVerificationSuccess, KanvibeStateFiles, TaskHookInstallation,
    TaskPeerPropagation, TaskWorktrees,
};
use kanvibe_services::kanban::{KanbanService, KanbanServiceParts, KanbanTimings};
use kanvibe_services::terminal_ports::TaskTerminalSessions;
use kanvibe_services::{ServiceError, ServiceResult};
use kanvibe_terminal::pane_layout_service::get_all_pane_layouts;
use serde_json::json;

use super::super::{MobileFeatures, MobileFeaturesParts};
use crate::adapters::DiffFileAccess;
use crate::clock::system_clock;
use crate::devices::{MobileDevices, MobileDevicesParts};
use crate::test_support::{
    FakeBackgroundSync, FakeImages, FakePanes, FakeProjectCatalog, FakeSessionDependencies,
    FakeTerminalTabs, TestDatabase, lock, notification_store, open_test_database,
};

/// 세션·worktree를 만들지 않는 보드 협력 객체. 이 테스트들은 DB에 있는 task만 다룬다
struct NoSessions;

impl HerdrTaskWorkspaces for NoSessions {
    fn close_task_workspace(&self, _: &str, _: Option<&str>) -> Result<(), String> {
        Ok(())
    }

    fn is_task_alive(&self, _: &str, _: Option<&str>) -> bool {
        false
    }
}

impl TaskTerminalSessions for NoSessions {
    fn detach_session(&self, _: &str, _: &str) {}

    fn replace_herdr_remote_server(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
}

impl TaskWorktrees for NoSessions {
    fn create_worktree_with_session(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: SessionType,
        _: Option<&str>,
        _: Option<&str>,
    ) -> ServiceResult<WorktreeSession> {
        Err(ServiceError::new("이 테스트는 worktree를 만들지 않는다"))
    }

    fn create_session_without_worktree(
        &self,
        _: &str,
        _: &str,
        _: SessionType,
        _: Option<&str>,
        _: &str,
        _: Option<&str>,
    ) -> ServiceResult<String> {
        Err(ServiceError::new("이 테스트는 세션을 만들지 않는다"))
    }

    fn remove_worktree_and_branch(
        &self,
        _: &str,
        _: &str,
        _: Option<&str>,
        _: &ResourceCleanupOptions,
    ) -> ServiceResult<()> {
        Ok(())
    }

    fn remove_session_only(
        &self,
        _: SessionType,
        _: &str,
        _: Option<&str>,
        _: bool,
    ) -> ServiceResult<()> {
        Ok(())
    }

    fn is_session_alive(&self, _: SessionType, _: &str, _: Option<&str>) -> bool {
        false
    }
}

impl TaskHookInstallation for NoSessions {
    fn install_hook_files(&self, _: &str, _: &str, _: Option<&str>) -> Result<(), String> {
        Ok(())
    }

    fn schedule_hooks_verification(
        &self,
        _: &str,
        _: &str,
        _: Option<&str>,
        _: HookVerificationSuccess,
        _: HookVerificationFailure,
    ) {
    }
}

impl KanvibeStateFiles for NoSessions {
    fn persist_task_state(&self, _: &KanbanTask) {}
    fn persist_task_metadata(&self, _: &KanbanTask) {}
    fn persist_task_session(&self, _: &KanbanTask) {}
    fn persist_project_color(&self, _: &Project) {}
}

impl TaskPeerPropagation for NoSessions {
    fn propagate_task_deletion_to_peers(&self, _: &str, _: Option<&str>) -> Vec<PeerTargetResult> {
        Vec::new()
    }
}

struct IntegrationFixture {
    features: MobileFeatures,
    devices: MobileDevices,
    notifications: Arc<kanvibe_services::notification_store::NotificationStore>,
    project_id: String,
    task_id: String,
    fixture_directory: tempfile::TempDir,
    test_database: TestDatabase,
}

fn new_task(title: String, project_id: &str, status: TaskStatus) -> NewKanbanTask {
    NewKanbanTask {
        title,
        description: None,
        status,
        branch_name: None,
        worktree_path: None,
        session_type: None,
        session_name: None,
        ssh_host: None,
        agent_type: None,
        project_id: Some(project_id.to_string()),
        base_branch: None,
        pr_url: None,
        priority: None,
    }
}

fn insert_fixture_rows(test_database: &TestDatabase, repo_path: &Path) -> (Project, String) {
    let connection = test_database.database.connection();
    let project = insert_project(
        &connection,
        &NewProject {
            name: "mobile-provider-fixture".to_string(),
            repo_path: repo_path.to_string_lossy().into_owned(),
            default_branch: "main".to_string(),
            ssh_host: None,
            is_worktree: false,
            color: None,
            icon_data_url: None,
        },
    )
    .unwrap();
    let tasks: Vec<KanbanTask> = (0..25)
        .map(|index| {
            insert_task(
                &connection,
                &new_task(
                    format!("fixture done {index}"),
                    &project.id,
                    TaskStatus::Done,
                ),
            )
            .unwrap()
        })
        .collect();
    (project, tasks[0].id.clone())
}

fn build_kanban(test_database: &TestDatabase, files: Arc<HostFileAccess>) -> Arc<KanbanService> {
    let sessions = Arc::new(NoSessions);
    Arc::new(KanbanService::new(KanbanServiceParts {
        database: Arc::clone(&test_database.database),
        files,
        board: BoardNotifier::new(),
        terminal: sessions.clone(),
        worktrees: sessions.clone(),
        hooks: sessions.clone(),
        state_files: sessions.clone(),
        peers: sessions,
        clock: Arc::new(Instant::now),
        timings: KanbanTimings::default(),
    }))
}

fn fixture() -> IntegrationFixture {
    let test_database = open_test_database();
    let fixture_directory = tempfile::tempdir().unwrap();
    let (project, task_id) = insert_fixture_rows(&test_database, fixture_directory.path());
    let home = fixture_directory.path().to_string_lossy().into_owned();
    let files = Arc::new(HostFileAccess::new(
        Arc::new(SystemShellCommandRunner::for_current_process()),
        home,
    ));
    let projects = Arc::new(FakeProjectCatalog::default());
    lock(&projects.projects).push(project.clone());
    let notifications = notification_store(&test_database.database);
    let features = MobileFeatures::new(MobileFeaturesParts {
        database: Arc::clone(&test_database.database),
        tasks: build_kanban(&test_database, Arc::clone(&files)),
        projects,
        diff: Arc::new(DiffFileAccess {
            diff: DiffService::new(Arc::clone(&test_database.database), Arc::clone(&files)),
            files,
        }),
        tabs: Arc::new(FakeTerminalTabs::default()),
        session_dependencies: Arc::new(FakeSessionDependencies::default()),
        background_sync: Arc::new(FakeBackgroundSync),
        notifications: Arc::clone(&notifications),
        panes: Arc::new(FakePanes::default()),
        images: Arc::new(FakeImages::default()),
    });
    let devices = MobileDevices::new(MobileDevicesParts {
        database: Arc::clone(&test_database.database),
        clock: system_clock(),
    });
    IntegrationFixture {
        features,
        devices,
        notifications,
        project_id: project.id,
        task_id,
        fixture_directory,
        test_database,
    }
}

impl IntegrationFixture {
    fn stored_task(&self, task_id: &str) -> KanbanTask {
        find_task(&self.test_database.database.connection(), task_id)
            .unwrap()
            .unwrap()
    }
}

fn status_of(error: crate::error::MobileError) -> Option<u16> {
    error.status_code()
}

#[test]
fn searchable_tasks_include_tasks_beyond_the_first_done_page() {
    let fixture = fixture();

    let tasks = fixture.features.get_mobile_searchable_tasks().unwrap();

    let project_tasks = tasks
        .iter()
        .filter(|task| task.project_id.as_deref() == Some(fixture.project_id.as_str()));
    assert_eq!(project_tasks.count(), 25);
}

#[test]
fn remaining_done_page_comes_with_the_actual_total() {
    let fixture = fixture();

    let first = fixture.features.get_mobile_done_tasks(0.0, 20.0).unwrap();
    let second = fixture.features.get_mobile_done_tasks(20.0, 20.0).unwrap();

    assert_eq!(
        (first.tasks.len(), second.tasks.len(), second.done_total),
        (20, 5, 25)
    );
    let mut ids: Vec<&str> = first
        .tasks
        .iter()
        .chain(&second.tasks)
        .map(|task| task.id.as_str())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 25);
}

#[test]
fn description_and_nullable_priority_are_persisted_on_the_same_task() {
    let fixture = fixture();

    let saved = fixture
        .features
        .update_mobile_task_details(
            &fixture.task_id,
            &json!({ "description": "한글 새 설명", "priority": "high" }),
        )
        .unwrap();
    assert_eq!(
        (
            saved.id.as_str(),
            saved.description.as_deref(),
            saved.priority.map(|p| p.as_str())
        ),
        (fixture.task_id.as_str(), Some("한글 새 설명"), Some("high"))
    );
    fixture
        .features
        .update_mobile_task_details(&fixture.task_id, &json!({ "priority": null }))
        .unwrap();

    assert_eq!(fixture.stored_task(&fixture.task_id).priority, None);
}

#[test]
fn unknown_fields_bad_priorities_missing_tasks_and_unpaired_requests_write_nothing() {
    let fixture = fixture();
    let before = fixture.stored_task(&fixture.task_id);

    let forbidden = fixture
        .features
        .update_mobile_task_details(&fixture.task_id, &json!({ "title": "forbidden" }))
        .unwrap_err();
    let urgent = fixture
        .features
        .update_mobile_task_details(&fixture.task_id, &json!({ "priority": "urgent" }))
        .unwrap_err();
    let missing = fixture
        .features
        .update_mobile_task_details("not-found", &json!({ "priority": "low" }))
        .unwrap_err();
    let unpaired = fixture
        .devices
        .authorize_mobile_request(Some("Bearer wrong"))
        .unwrap();

    assert_eq!(
        (status_of(forbidden), status_of(urgent), status_of(missing)),
        (Some(400), Some(400), Some(404))
    );
    assert_eq!(unpaired, None);
    assert_eq!(fixture.stored_task(&fixture.task_id), before);
}

#[test]
fn typed_pc_settings_and_pane_layouts_use_the_shared_desktop_stores() {
    let fixture = fixture();
    fixture.features.get_mobile_settings().unwrap();

    let saved = fixture
        .features
        .update_mobile_settings(&json!({ "defaultSessionType": "zellij", "sortPreference": { "keys": [{ "field": "title", "direction": "asc" }] } }))
        .unwrap();
    let saved = serde_json::to_value(saved).unwrap();
    assert_eq!(saved["defaultSessionType"], "zellij");
    assert_eq!(
        saved["sortPreference"],
        json!({ "keys": [{ "field": "title", "direction": "asc" }] })
    );
    let invalid = fixture
        .features
        .update_mobile_settings(&json!({ "defaultSessionType": "herdr", "backgroundSync": { "isEnabled": true, "intervalMs": 1 } }))
        .unwrap_err();
    assert_eq!(status_of(invalid), Some(400));
    assert_eq!(
        fixture
            .features
            .get_mobile_settings()
            .unwrap()
            .default_session_type,
        SessionType::Zellij
    );

    let layout = fixture
        .features
        .save_mobile_pane_layout(&json!({ "isGlobal": false, "projectId": fixture.project_id, "layoutType": "vertical_2", "panes": [{ "position": 0, "command": "echo first" }, { "position": 1, "command": "" }] }))
        .unwrap();
    let layouts = fixture.features.get_mobile_pane_layouts().unwrap();
    assert!(layouts.iter().any(|entry| entry.project_id.as_deref()
        == Some(fixture.project_id.as_str())
        && entry.layout_type.as_str() == "vertical_2"));
    fixture
        .features
        .delete_mobile_pane_layout(&layout.id)
        .unwrap();
    assert!(
        get_all_pane_layouts(&fixture.test_database.database)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn reading_task_notifications_leaves_other_tasks_unread() {
    let fixture = fixture();
    let create = |task_id: &str, body: &str| {
        fixture
            .notifications
            .create_notification(&DesktopNotificationPayload {
                title: "fixture".to_string(),
                body: body.to_string(),
                task_id: Some(task_id.to_string()),
                locale: "ko".to_string(),
                relative_path: None,
                dedupe_key: Some(format!("{body}-{}", fixture.task_id)),
                action: None,
            })
            .unwrap()
            .notification
            .id
    };
    let target = create(&fixture.task_id, "read target");
    let other = create("other-task", "keep unread");

    assert_eq!(
        fixture
            .features
            .mark_mobile_task_notifications_read(&fixture.task_id)
            .unwrap(),
        1
    );

    let is_read = |id: &str| {
        fixture
            .notifications
            .get_notification_by_id(id)
            .unwrap()
            .unwrap()
            .is_read
    };
    assert!(is_read(&target));
    assert!(!is_read(&other));
}

fn git(directory: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(arguments)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git 실행");
    assert!(
        status.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&status.stderr)
    );
}

fn prepare_changed_repository(directory: &Path) {
    git(directory, &["init", "-b", "main"]);
    std::fs::create_dir(directory.join("src")).unwrap();
    std::fs::write(directory.join("src/file.ts"), "base\n").unwrap();
    git(directory, &["add", "src/file.ts"]);
    git(directory, &["commit", "-m", "fixture base"]);
    git(directory, &["switch", "-c", "feat/provider"]);
    std::fs::write(directory.join("src/file.ts"), "committed\n").unwrap();
    std::fs::write(directory.join("src/committed.ts"), "already committed\n").unwrap();
    git(directory, &["add", "src"]);
    git(directory, &["commit", "-m", "fixture feature"]);
    std::fs::write(directory.join("src/file.ts"), "changed\n").unwrap();
    std::fs::write(directory.join("src/new.ts"), "new\nfile\n").unwrap();
}

#[test]
fn real_changed_git_files_are_read_and_saved_and_escapes_rejected() {
    let fixture = fixture();
    let directory = fixture.fixture_directory.path();
    prepare_changed_repository(directory);
    let diff_task = insert_task(
        &fixture.test_database.database.connection(),
        &NewKanbanTask {
            worktree_path: Some(directory.to_string_lossy().into_owned()),
            branch_name: Some("feat/provider".to_string()),
            base_branch: Some("main".to_string()),
            ..new_task(
                "diff fixture".to_string(),
                &fixture.project_id,
                TaskStatus::Todo,
            )
        },
    )
    .unwrap();

    let files =
        serde_json::to_value(fixture.features.get_mobile_diff(&diff_task.id).unwrap()).unwrap();
    assert_eq!(
        files,
        json!([
            { "path": "src/file.ts", "status": "modified", "additions": 1, "deletions": 1 },
            { "path": "src/new.ts", "status": "added", "additions": 2, "deletions": 0 },
        ])
    );
    let read = fixture
        .features
        .get_mobile_diff_file(&diff_task.id, Some("src/file.ts"))
        .unwrap();
    assert_eq!(
        (read.original.as_str(), read.content.as_str()),
        ("committed", "changed\n")
    );
    fixture
        .features
        .save_mobile_diff_file(
            &diff_task.id,
            Some("src/file.ts"),
            &json!({ "content": "한글 저장\n" }),
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(directory.join("src/file.ts")).unwrap(),
        "한글 저장\n"
    );
    let outside = fixture
        .features
        .save_mobile_diff_file(
            &diff_task.id,
            Some("../outside"),
            &json!({ "content": "bad" }),
        )
        .unwrap_err();
    std::os::unix::fs::symlink("/etc/passwd", directory.join("src/escape")).unwrap();
    let escape = fixture
        .features
        .get_mobile_diff_file(&diff_task.id, Some("src/escape"))
        .unwrap_err();
    let git_config = fixture
        .features
        .get_mobile_diff_file(&diff_task.id, Some(".git/config"))
        .unwrap_err();

    assert_eq!(
        (status_of(outside), status_of(escape), status_of(git_config)),
        (Some(400), Some(400), Some(400))
    );
}
