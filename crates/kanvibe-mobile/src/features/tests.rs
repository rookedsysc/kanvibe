use std::sync::Arc;

use kanvibe_domain::notifications::{
    AppNotificationAction, BackgroundSyncReviewPayload, DesktopNotificationPayload,
    TaskPrMergedDetectedPayload,
};
use kanvibe_domain::task::SessionType;
use kanvibe_domain::task_diff_stats::{DiffFile, DiffFileStatus};
use kanvibe_services::diff::FileSaveResult;
use kanvibe_services::kanban::{SessionTypeChangeTargets, TaskUpdate};
use kanvibe_services::notification_store::NotificationStore;
use kanvibe_terminal::pane_layout_service::get_all_pane_layouts;
use serde_json::{Value, json};

use super::*;
use crate::app_settings::open_app_settings;
use crate::error::MobileError;
use crate::test_support::{
    FakeBackgroundSync, FakeDiffFiles, FakeImages, FakePanes, FakeProjectCatalog,
    FakeSessionDependencies, FakeTaskBoard, FakeTerminalTabs, TestDatabase, dependency_status,
    lock, notification_store, open_test_database, project, task, terminal_tab, with_project,
};

mod integration_tests;

struct FeatureFixture {
    features: MobileFeatures,
    tasks: Arc<FakeTaskBoard>,
    projects: Arc<FakeProjectCatalog>,
    diff: Arc<FakeDiffFiles>,
    tabs: Arc<FakeTerminalTabs>,
    dependencies: Arc<FakeSessionDependencies>,
    panes: Arc<FakePanes>,
    images: Arc<FakeImages>,
    notifications: Arc<NotificationStore>,
    test_database: TestDatabase,
}

/// TS 테스트의 기본 목: 브랜치 없는 task-1, 원본 프로젝트 project-1, 등록된 호스트 known-host, 탭 @1·@2
fn fixture() -> FeatureFixture {
    let test_database = open_test_database();
    let tasks = Arc::new(FakeTaskBoard::default());
    let mut base_task = task("task-1");
    base_task.project_id = Some("project-1".to_string());
    tasks.put_task(with_project(base_task, None));
    let projects = Arc::new(FakeProjectCatalog::default());
    lock(&projects.projects).push(project("project-1"));
    *lock(&projects.hosts) = vec!["known-host".to_string()];
    let diff = Arc::new(FakeDiffFiles::default());
    *lock(&diff.diff_files) = vec![DiffFile {
        path: "src/file.ts".to_string(),
        original_path: None,
        status: DiffFileStatus::Modified,
        additions: 0,
        deletions: 0,
    }];
    let tabs = Arc::new(FakeTerminalTabs::default());
    *lock(&tabs.tabs) = vec![terminal_tab("@1", "one"), terminal_tab("@2", "two")];
    let dependencies = Arc::new(FakeSessionDependencies::default());
    let panes = Arc::new(FakePanes::default());
    let images = Arc::new(FakeImages::default());
    let notifications = notification_store(&test_database.database);
    let features = MobileFeatures::new(MobileFeaturesParts {
        database: Arc::clone(&test_database.database),
        tasks: tasks.clone(),
        projects: projects.clone(),
        diff: diff.clone(),
        tabs: tabs.clone(),
        session_dependencies: dependencies.clone(),
        background_sync: Arc::new(FakeBackgroundSync),
        notifications: Arc::clone(&notifications),
        panes: panes.clone(),
        images: images.clone(),
    });
    FeatureFixture {
        features,
        tasks,
        projects,
        diff,
        tabs,
        dependencies,
        panes,
        images,
        notifications,
        test_database,
    }
}

impl FeatureFixture {
    fn edit_task(&self, edit: impl FnOnce(&mut kanvibe_domain::task::KanbanTask)) {
        let mut tasks = lock(&self.tasks.tasks);
        edit(&mut tasks.get_mut("task-1").unwrap().task);
    }

    fn attach_worktree(&self) {
        self.edit_task(|task| {
            task.branch_name = Some("feature".to_string());
            task.worktree_path = Some("/fixture/repo".to_string());
        });
    }
}

fn status_of(error: MobileError) -> Option<u16> {
    error.status_code()
}

#[test]
fn invalid_details_are_rejected_before_mutation() {
    let fixture = fixture();
    let bodies = [
        json!({ "priority": "urgent" }),
        json!({ "title": "hidden write" }),
        json!({ "description": 42 }),
        json!({}),
        Value::Null,
    ];
    for body in bodies {
        let error = fixture
            .features
            .update_mobile_task_details("task-1", &body)
            .unwrap_err();
        assert_eq!(status_of(error), Some(400), "{body}");
    }

    assert_eq!(fixture.tasks.calls.count_starting_with("update_task "), 0);
}

#[test]
fn explicit_clears_pass_only_the_approved_fields() {
    let fixture = fixture();
    *lock(&fixture.tasks.updated_task) = Some(task("task-1"));

    fixture
        .features
        .update_mobile_task_details("task-1", &json!({ "description": null, "priority": null }))
        .unwrap();

    let expected = TaskUpdate {
        title: None,
        description: Some(None),
        priority: Some(None),
    };
    assert_eq!(
        fixture
            .tasks
            .calls
            .count_starting_with(&format!("update_task task-1 {expected:?}")),
        1
    );
}

#[test]
fn missing_task_is_not_mutated() {
    let fixture = fixture();

    let error = fixture
        .features
        .update_mobile_task_details("missing", &json!({ "priority": null }))
        .unwrap_err();

    assert_eq!(status_of(error), Some(404));
    assert_eq!(fixture.tasks.calls.count_starting_with("update_task "), 0);
}

#[test]
fn invalid_pagination_is_rejected() {
    let fixture = fixture();
    for (offset, limit) in [(-1.0, 20.0), (0.0, 101.0), (0.5, 20.0)] {
        let error = fixture
            .features
            .get_mobile_done_tasks(offset, limit)
            .unwrap_err();
        assert_eq!(status_of(error), Some(400));
    }

    assert_eq!(
        fixture
            .tasks
            .calls
            .count_starting_with("get_more_done_tasks"),
        0
    );
}

#[test]
fn unconfigured_ssh_alias_is_rejected_without_scanning() {
    let fixture = fixture();

    let error = fixture
        .features
        .scan_mobile_remote_projects("~/repo", &json!("arbitrary-host"))
        .unwrap_err();

    assert_eq!(status_of(error), Some(400));
    assert!(fixture.projects.calls.all().is_empty());
}

#[test]
fn configured_remote_host_reaches_directory_and_scan_services() {
    let fixture = fixture();

    fixture
        .features
        .get_mobile_remote_directories("~/repo", &json!("known-host"))
        .unwrap();
    fixture
        .features
        .scan_mobile_remote_projects("~/repo", &json!("known-host"))
        .unwrap();

    assert_eq!(
        fixture.projects.calls.all(),
        vec![
            r#"list_subdirectories ~/repo Some("known-host")"#,
            r#"scan_and_register_projects ~/repo Some("known-host")"#,
        ]
    );
}

#[test]
fn color_is_validated_before_saving() {
    let fixture = fixture();

    let error = fixture
        .features
        .update_mobile_project_color("project-1", &json!({ "color": "red" }))
        .unwrap_err();
    assert_eq!(status_of(error), Some(400));
    assert_eq!(
        fixture
            .tasks
            .calls
            .count_starting_with("update_project_color"),
        0
    );
    fixture
        .features
        .update_mobile_project_color("project-1", &json!({ "color": "#0064FF" }))
        .unwrap();

    assert_eq!(
        fixture
            .tasks
            .calls
            .count_starting_with("update_project_color project-1 #0064FF"),
        1
    );
}

#[test]
fn branching_reuses_the_task_and_refuses_an_already_connected_task() {
    let fixture = fixture();
    let draft = json!({ "projectId": "project-1", "baseBranch": "main", "branchName": "feat/mobile", "sessionType": "tmux" });
    let mut branched = task("task-1");
    branched.status = kanvibe_domain::task::TaskStatus::Progress;
    *lock(&fixture.tasks.branch_result) = Some(branched);

    let result = fixture
        .features
        .branch_mobile_task("task-1", &draft)
        .unwrap();
    assert_eq!(
        (result.id.as_str(), result.status.as_str()),
        ("task-1", "progress")
    );
    assert_eq!(
        fixture
            .tasks
            .calls
            .count_starting_with("branch_from_task task-1 project-1 main feat/mobile tmux"),
        1
    );
    fixture.edit_task(|task| task.branch_name = Some("already-attached".to_string()));

    let error = fixture
        .features
        .branch_mobile_task("task-1", &draft)
        .unwrap_err();

    assert_eq!(status_of(error), Some(409));
    assert_eq!(
        fixture.tasks.calls.count_starting_with("branch_from_task"),
        1
    );
}

#[test]
fn unsafe_paths_are_rejected_before_file_access() {
    let fixture = fixture();
    fixture.attach_worktree();
    for file_path in [
        "../outside",
        "/etc/passwd",
        "a/../../outside",
        ".git/config",
        "src\\file",
        "src//file",
    ] {
        let error = fixture
            .features
            .get_mobile_diff_file("task-1", Some(file_path))
            .unwrap_err();
        assert_eq!(status_of(error), Some(400), "{file_path}");
    }

    assert_eq!(
        fixture.diff.calls.count_starting_with("get_file_content"),
        0
    );
}

#[test]
fn changed_file_behind_a_symbolic_link_is_rejected() {
    let fixture = fixture();
    fixture.attach_worktree();
    *lock(&fixture.diff.symbolic_link) = Some("/fixture/repo/src".to_string());

    let error = fixture
        .features
        .save_mobile_diff_file("task-1", Some("src/file.ts"), &json!({ "content": "text" }))
        .unwrap_err();

    assert_eq!(status_of(error), Some(400));
    assert_eq!(
        fixture.diff.calls.count_starting_with("save_file_content"),
        0
    );
}

#[test]
fn non_diff_paths_and_oversized_edits_write_nothing() {
    let fixture = fixture();
    fixture.attach_worktree();

    let unknown = fixture
        .features
        .save_mobile_diff_file("task-1", Some("unknown.ts"), &json!({ "content": "text" }))
        .unwrap_err();
    let oversized = fixture
        .features
        .save_mobile_diff_file(
            "task-1",
            Some("src/file.ts"),
            &json!({ "content": "x".repeat(1024 * 1024 + 1) }),
        )
        .unwrap_err();

    assert_eq!(
        (status_of(unknown), status_of(oversized)),
        (Some(404), Some(400))
    );
    assert_eq!(
        fixture.diff.calls.count_starting_with("save_file_content"),
        0
    );
}

#[test]
fn accepted_edits_are_delegated_and_write_failures_reported() {
    let fixture = fixture();
    fixture.attach_worktree();

    fixture
        .features
        .save_mobile_diff_file("task-1", Some("src/file.ts"), &json!({ "content": "한글" }))
        .unwrap();
    assert_eq!(
        fixture
            .diff
            .calls
            .count_starting_with("save_file_content task-1 src/file.ts 한글"),
        1
    );
    *lock(&fixture.diff.save_result) = Some(FileSaveResult {
        success: false,
        error: Some("secret path".to_string()),
    });

    let error = fixture
        .features
        .save_mobile_diff_file("task-1", Some("src/file.ts"), &json!({ "content": "next" }))
        .unwrap_err();

    assert_eq!(
        error,
        MobileError::Feature {
            status: 409,
            message: "파일을 저장하지 못했습니다".to_string()
        }
    );
}

#[test]
fn tab_identity_is_checked_before_rename_or_move() {
    let fixture = fixture();

    let missing = fixture
        .features
        .rename_mobile_tab("task-1", "other-session-tab", &json!({ "name": "renamed" }))
        .unwrap_err();
    let outside = fixture
        .features
        .move_mobile_tab("task-1", "@1", &json!({ "index": 2 }))
        .unwrap_err();
    assert_eq!(
        (status_of(missing), status_of(outside)),
        (Some(404), Some(400))
    );
    assert!(fixture.tabs.calls.all().is_empty());
    fixture
        .features
        .rename_mobile_tab("task-1", "@1", &json!({ "name": "새 탭" }))
        .unwrap();
    fixture
        .features
        .move_mobile_tab("task-1", "@1", &json!({ "index": 1 }))
        .unwrap();

    assert_eq!(
        fixture.tabs.calls.all(),
        vec![
            "rename_terminal_tab task-1 @1 새 탭",
            "move_terminal_tab task-1 @1 1"
        ]
    );
}

#[test]
fn pane_layout_size_and_scope_are_checked_before_saving() {
    let fixture = fixture();
    let one_pane = json!([{ "position": 0, "command": "" }]);

    let wrong_count = fixture
        .features
        .save_mobile_pane_layout(
            &json!({ "layoutType": "quad", "isGlobal": true, "panes": one_pane }),
        )
        .unwrap_err();
    let global_project = fixture
        .features
        .save_mobile_pane_layout(&json!({ "layoutType": "single", "isGlobal": true, "projectId": "project-1", "panes": one_pane }))
        .unwrap_err();
    assert_eq!(
        (status_of(wrong_count), status_of(global_project)),
        (Some(400), Some(400))
    );
    assert!(
        get_all_pane_layouts(&fixture.test_database.database)
            .unwrap()
            .is_empty()
    );
    fixture
        .features
        .save_mobile_pane_layout(&json!({ "layoutType": "single", "isGlobal": true, "panes": [{ "position": 0, "command": "echo ready" }] }))
        .unwrap();

    let layouts = get_all_pane_layouts(&fixture.test_database.database).unwrap();
    assert_eq!(layouts.len(), 1);
    assert!(layouts[0].is_global);
    assert_eq!(layouts[0].project_id, None);
    assert_eq!(layouts[0].panes[0].command, "echo ready");
}

#[test]
fn all_settings_are_validated_before_any_write() {
    let fixture = fixture();
    let settings = || open_app_settings(&fixture.test_database.database);
    settings()
        .set_default_session_type(SessionType::Herdr)
        .unwrap();

    let invalid = fixture
        .features
        .update_mobile_settings(&json!({ "defaultSessionType": "tmux", "backgroundSync": { "isEnabled": true, "intervalMs": 1 } }))
        .unwrap_err();
    assert_eq!(status_of(invalid), Some(400));
    assert_eq!(
        settings().get_default_session_type().unwrap(),
        SessionType::Herdr
    );
    let unknown = fixture
        .features
        .update_mobile_settings(&json!({ "arbitrarySetting": "unsafe" }))
        .unwrap_err();
    assert_eq!(status_of(unknown), Some(400));
    fixture
        .features
        .update_mobile_settings(&json!({ "defaultSessionType": "zellij", "sortPreference": { "keys": [{ "field": "priority", "direction": "desc" }] } }))
        .unwrap();

    assert_eq!(
        settings().get_default_session_type().unwrap(),
        SessionType::Zellij
    );
    assert_eq!(
        serde_json::to_value(settings().get_board_sort_preference().unwrap()).unwrap(),
        json!({ "keys": [{ "field": "priority", "direction": "desc" }] })
    );
}

#[test]
fn duplicate_sort_criteria_and_missing_bulk_dependencies_are_rejected() {
    let fixture = fixture();
    *lock(&fixture.tasks.change_targets) = Some(SessionTypeChangeTargets {
        task_count: 2,
        live_session_count: 1,
        ssh_hosts: vec![None],
    });

    let duplicate = fixture
        .features
        .update_mobile_settings(&json!({ "sortPreference": { "keys": [{ "field": "title", "direction": "asc" }, { "field": "title", "direction": "desc" }] } }))
        .unwrap_err();
    assert_eq!(status_of(duplicate), Some(400));
    *lock(&fixture.dependencies.status) = Some(dependency_status(false, Some("herdr")));
    let missing = fixture
        .features
        .change_all_mobile_task_sessions(
            &json!({ "sessionType": "herdr", "shouldCloseExistingSessions": false }),
        )
        .unwrap_err();

    assert_eq!(status_of(missing), Some(409));
    assert_eq!(
        fixture
            .tasks
            .calls
            .count_starting_with("change_all_task_session_types"),
        0
    );
}

fn create_review_notification(store: &NotificationStore) -> String {
    let payload = DesktopNotificationPayload {
        title: "검토".to_string(),
        body: "본문".to_string(),
        task_id: None,
        locale: "ko".to_string(),
        relative_path: None,
        dedupe_key: None,
        action: Some(AppNotificationAction::BackgroundSyncReview {
            payload: BackgroundSyncReviewPayload {
                merged_pull_requests: vec![TaskPrMergedDetectedPayload {
                    task_id: "task-1".to_string(),
                    task_title: "task-1".to_string(),
                    branch_name: "feature".to_string(),
                    pr_url: "https://github.com/p/r/pull/1".to_string(),
                    merged_at: "2026-11-01T00:00:00.000Z".to_string(),
                }],
                registered_worktrees: Vec::new(),
                pulled_tasks: None,
                failures: None,
            },
        }),
    };
    store.create_notification(&payload).unwrap().notification.id
}

#[test]
fn sync_review_deletes_only_unchanged_tasks_owned_by_the_notification() {
    let fixture = fixture();
    let notification_id = create_review_notification(&fixture.notifications);

    let foreign = fixture
        .features
        .delete_mobile_sync_review_tasks(&notification_id, &json!({ "taskIds": ["other"] }))
        .unwrap_err();
    let changed = fixture
        .features
        .delete_mobile_sync_review_tasks(&notification_id, &json!({ "taskIds": ["task-1"] }))
        .unwrap_err();
    assert_eq!(
        (status_of(foreign), status_of(changed)),
        (Some(400), Some(409))
    );
    assert_eq!(fixture.tasks.calls.count_starting_with("delete_tasks"), 0);
    fixture.edit_task(|task| task.branch_name = Some("feature".to_string()));
    *lock(&fixture.tasks.deleted_task_ids) = vec!["task-1".to_string()];

    let deleted = fixture
        .features
        .delete_mobile_sync_review_tasks(
            &notification_id,
            &json!({ "taskIds": ["task-1", "task-1"] }),
        )
        .unwrap();

    assert_eq!(deleted, vec!["task-1"]);
    assert_eq!(
        fixture
            .tasks
            .calls
            .count_starting_with(r#"delete_tasks ["task-1"]"#),
        1
    );
}

#[test]
fn marks_only_the_requested_existing_task_notifications_read() {
    let fixture = fixture();
    for title in ["하나", "둘", "셋"] {
        fixture
            .notifications
            .create_notification(&DesktopNotificationPayload {
                title: title.to_string(),
                body: "본문".to_string(),
                task_id: Some("task-1".to_string()),
                locale: "ko".to_string(),
                relative_path: None,
                dedupe_key: None,
                action: None,
            })
            .unwrap();
    }

    assert_eq!(
        fixture
            .features
            .mark_mobile_task_notifications_read("task-1")
            .unwrap(),
        3
    );
    assert_eq!(
        status_of(
            fixture
                .features
                .mark_mobile_task_notifications_read("missing")
                .unwrap_err()
        ),
        Some(404)
    );
}

const PNG_BASE64: &str = "iVBORw0KGgo=";

#[test]
fn malformed_images_and_local_targets_are_rejected_before_upload() {
    let fixture = fixture();

    let malformed = fixture
        .features
        .paste_mobile_remote_image(
            "task-1",
            "%1",
            &json!({ "mimeType": "image/png", "base64": "not-base64" }),
        )
        .unwrap_err();
    let local = fixture
        .features
        .paste_mobile_remote_image(
            "task-1",
            "%1",
            &json!({ "mimeType": "image/png", "base64": PNG_BASE64 }),
        )
        .unwrap_err();

    assert_eq!(
        (status_of(malformed), status_of(local)),
        (Some(400), Some(409))
    );
    assert!(fixture.images.calls.all().is_empty());
}

#[test]
fn images_go_only_to_an_owned_remote_pane_and_insert_a_quoted_path() {
    let fixture = fixture();
    fixture.edit_task(|task| task.ssh_host = Some("known-host".to_string()));
    *lock(&fixture.panes.pane_ids) = vec!["%1".to_string()];
    *lock(&fixture.images.remote_path) = Some("/tmp/kanvibe-paste.png".to_string());

    let remote_path = fixture
        .features
        .paste_mobile_remote_image(
            "task-1",
            "%1",
            &json!({ "mimeType": "image/png", "base64": PNG_BASE64 }),
        )
        .unwrap();

    assert_eq!(remote_path, "/tmp/kanvibe-paste.png");
    assert_eq!(
        fixture.panes.calls.all(),
        vec!["write_to_pane task-1 %1 '/tmp/kanvibe-paste.png'"]
    );
}
