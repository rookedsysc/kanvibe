use std::path::Path;
use std::sync::{Arc, Mutex};

use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{
    AggregatedAiSession, AggregatedAiSessionDetail, AggregatedAiSessionsResult,
    AiSessionAttachmentContent, AiSessionMatchScope,
};
use kanvibe_domain::notifications::DesktopNotificationPayload;
use kanvibe_domain::task::{SessionType, TaskPriority};
use kanvibe_process::task_target_propagation::PeerTargetResult;
use kanvibe_services::ServiceError;
use kanvibe_services::board_notifier::BoardNotifier;
use kanvibe_services::diff::{TaskWorkspaceFile, TaskWorkspaceFileFailure};
use kanvibe_services::kanban::TaskDeletion;
use kanvibe_services::notification_store::NotificationStore;
use kanvibe_services::project::ScanResult;
use serde_json::{Value, json};

use super::*;
use crate::app_settings::open_app_settings;
use crate::error::MobileError;
use crate::ports::AiHistoryReadError;
use crate::test_support::{
    FakeDiffFiles, FakePeers, FakeProjectCatalog, FakeSessionDependencies, FakeTaskBoard,
    TestDatabase, dependency_status, lock, notification_store, open_test_database, project, task,
    with_project,
};

struct BridgeFixture {
    bridge: MobileBridge,
    tasks: Arc<FakeTaskBoard>,
    projects: Arc<FakeProjectCatalog>,
    diff: Arc<FakeDiffFiles>,
    dependencies: Arc<FakeSessionDependencies>,
    peers: Arc<FakePeers>,
    board: Arc<BoardNotifier>,
    notifications: Arc<NotificationStore>,
    test_database: TestDatabase,
}

fn fixture_with_home(home_directory: &Path) -> BridgeFixture {
    let test_database = open_test_database();
    let tasks = Arc::new(FakeTaskBoard::default());
    let projects = Arc::new(FakeProjectCatalog::default());
    let diff = Arc::new(FakeDiffFiles::default());
    let dependencies = Arc::new(FakeSessionDependencies::default());
    let peers = Arc::new(FakePeers::default());
    let board = BoardNotifier::new();
    let notifications = notification_store(&test_database.database);
    let bridge = MobileBridge::new(MobileBridgeParts {
        database: Arc::clone(&test_database.database),
        board: Arc::clone(&board),
        tasks: tasks.clone(),
        projects: projects.clone(),
        diff: diff.clone(),
        session_dependencies: dependencies.clone(),
        peers: peers.clone(),
        notifications: Arc::clone(&notifications),
        home_directory: home_directory.to_path_buf(),
        mobile_tls_port: 9746,
    });
    BridgeFixture {
        bridge,
        tasks,
        projects,
        diff,
        dependencies,
        peers,
        board,
        notifications,
        test_database,
    }
}

fn fixture() -> BridgeFixture {
    fixture_with_home(Path::new("/home/fixture"))
}

impl BridgeFixture {
    fn app_setting(&self, key: &str) -> Option<String> {
        open_app_settings(&self.test_database.database)
            .get_app_setting(key)
            .unwrap()
    }

    fn set_app_setting(&self, key: &str, value: &str) {
        open_app_settings(&self.test_database.database)
            .set_app_setting(key, value)
            .unwrap();
    }

    fn set_default_session_type(&self, session_type: SessionType) {
        open_app_settings(&self.test_database.database)
            .set_default_session_type(session_type)
            .unwrap();
    }
}

fn draft() -> MobileTaskDraft {
    MobileTaskDraft {
        project_id: "project-1".to_string(),
        branch_name: "feat/mobile-create".to_string(),
        ..MobileTaskDraft::default()
    }
}

fn is_task_input(error: &MobileError, fragment: &str) -> bool {
    matches!(error, MobileError::TaskInput(message) if message.contains(fragment))
}

fn stored_notification() -> Value {
    json!({
        "id": "inbox-1", "title": "리뷰 요청", "body": "변경을 확인해 주세요", "taskId": "task-1", "isRead": false,
        "createdAt": "2026-11-01T00:00:00.000Z", "relativePath": "/ko/task/task-1", "locale": "ko", "dedupeKey": "inbox-1",
        "action": { "type": "background-sync-review", "payload": { "mergedPullRequests": [], "registeredWorktrees": [] } }
    })
}

fn mobile_notification() -> Value {
    json!({
        "id": "inbox-1", "title": "리뷰 요청", "body": "변경을 확인해 주세요", "taskId": "task-1", "isRead": false,
        "createdAt": "2026-11-01T00:00:00.000Z", "actionType": "background-sync-review"
    })
}

fn with_fields(base: &Value, fields: Value) -> Value {
    let mut merged = base.clone();
    for (key, value) in fields.as_object().unwrap() {
        merged[key] = value.clone();
    }
    merged
}

fn to_json<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap()
}

fn stored_notifications(fixture: &BridgeFixture) -> Value {
    serde_json::from_str(&fixture.app_setting("app_notifications").unwrap()).unwrap()
}

// parity: TS-main-mobileBridgeService-31d17f
#[test]
fn inbox_lists_newest_first_and_exposes_only_the_action_type() {
    let fixture = fixture();
    let older = with_fields(
        &stored_notification(),
        json!({ "id": "older", "taskId": null, "createdAt": "2026-10-01T00:00:00.000Z" }),
    );
    fixture.set_app_setting(
        "app_notifications",
        &json!([older, stored_notification()]).to_string(),
    );

    let notifications = to_json(fixture.bridge.get_mobile_notifications().unwrap());

    assert_eq!(
        notifications,
        json!([
            mobile_notification(),
            with_fields(
                &mobile_notification(),
                json!({ "id": "older", "taskId": null, "createdAt": "2026-10-01T00:00:00.000Z" })
            ),
        ])
    );
}

// parity: TS-main-mobileBridgeService-9a780e
#[test]
fn inbox_is_empty_without_stored_notifications() {
    assert!(
        fixture()
            .bridge
            .get_mobile_notifications()
            .unwrap()
            .is_empty()
    );
}

// parity: TS-main-mobileBridgeService-bb7c3e
#[test]
fn marking_one_read_reaches_the_desktop_store_and_distinguishes_missing() {
    let fixture = fixture();
    let second = with_fields(
        &stored_notification(),
        json!({ "id": "inbox-2", "taskId": null }),
    );
    fixture.set_app_setting(
        "app_notifications",
        &json!([stored_notification(), second]).to_string(),
    );
    let read = with_fields(&mobile_notification(), json!({ "isRead": true }));

    assert_eq!(
        to_json(
            fixture
                .bridge
                .mark_mobile_notification_read("inbox-1")
                .unwrap()
        ),
        read
    );
    assert_eq!(
        to_json(
            fixture
                .bridge
                .mark_mobile_notification_read("inbox-1")
                .unwrap()
        ),
        read
    );
    assert_eq!(
        fixture
            .bridge
            .mark_mobile_notification_read("missing")
            .unwrap(),
        None
    );
    assert_eq!(
        stored_notifications(&fixture),
        json!([
            with_fields(&stored_notification(), json!({ "isRead": true })),
            second
        ])
    );
}

// parity: TS-main-mobileBridgeService-f4a952
#[test]
fn marking_all_read_covers_notifications_without_task() {
    let fixture = fixture();
    let second = with_fields(
        &stored_notification(),
        json!({ "id": "inbox-2", "taskId": null }),
    );
    fixture.set_app_setting(
        "app_notifications",
        &json!([stored_notification(), second]).to_string(),
    );

    fixture.bridge.mark_all_mobile_notifications_read().unwrap();
    fixture.bridge.mark_all_mobile_notifications_read().unwrap();

    assert_eq!(
        stored_notifications(&fixture),
        json!([
            with_fields(&stored_notification(), json!({ "isRead": true })),
            with_fields(&second, json!({ "isRead": true })),
        ])
    );
}

// parity: TS-main-mobileBridgeService-78514d
#[test]
fn marking_task_read_changes_only_that_task_and_returns_the_count() {
    let fixture = fixture();
    let second = with_fields(&stored_notification(), json!({ "id": "inbox-2" }));
    let other = with_fields(
        &stored_notification(),
        json!({ "id": "inbox-3", "taskId": "other-task" }),
    );
    fixture.set_app_setting(
        "app_notifications",
        &json!([stored_notification(), second, other]).to_string(),
    );

    assert_eq!(
        fixture
            .bridge
            .mark_mobile_task_notifications_read("task-1")
            .unwrap(),
        2
    );
    assert_eq!(
        fixture
            .bridge
            .mark_mobile_task_notifications_read("task-1")
            .unwrap(),
        0
    );
    assert_eq!(
        stored_notifications(&fixture),
        json!([
            with_fields(&stored_notification(), json!({ "isRead": true })),
            with_fields(&second, json!({ "isRead": true })),
            other,
        ])
    );
}

// parity: TS-main-mobileBridgeService-937a9c
#[test]
fn change_listener_sees_completed_saves_until_unsubscribed() {
    let fixture = fixture();
    let saved_read_states = Arc::new(Mutex::new(Vec::<Vec<bool>>::new()));
    let recorded = Arc::clone(&saved_read_states);
    let database = Arc::clone(&fixture.test_database.database);
    let subscription = fixture
        .bridge
        .on_mobile_notifications_changed(Arc::new(move || {
            let stored = open_app_settings(&database)
                .get_app_setting("app_notifications")
                .unwrap();
            let entries: Vec<Value> =
                serde_json::from_str(&stored.unwrap_or_else(|| "[]".to_string())).unwrap();
            lock(&recorded).push(
                entries
                    .iter()
                    .map(|entry| entry["isRead"].as_bool().unwrap())
                    .collect(),
            );
        }));
    let payload = |title: &str| DesktopNotificationPayload {
        title: title.to_string(),
        body: "본문".to_string(),
        task_id: None,
        locale: "ko".to_string(),
        relative_path: None,
        dedupe_key: None,
        action: None,
    };

    let created = fixture
        .notifications
        .create_notification(&payload("모바일 이벤트 연결"))
        .unwrap();
    fixture
        .bridge
        .mark_mobile_notification_read(&created.notification.id)
        .unwrap();
    fixture
        .bridge
        .mark_mobile_notification_read(&created.notification.id)
        .unwrap();
    drop(subscription);
    fixture
        .notifications
        .create_notification(&payload("연결 해제 후 이벤트"))
        .unwrap();

    assert_eq!(*lock(&saved_read_states), vec![vec![false], vec![true]]);
}

// parity: TS-main-mobileBridgeService-a42f02
#[test]
fn single_task_lookup_adds_the_project_name_or_returns_none() {
    let fixture = fixture();
    let mut done_task = task("done-task");
    done_task.status = kanvibe_domain::task::TaskStatus::Done;
    fixture
        .tasks
        .put_task(with_project(done_task, Some(project("kanvibe"))));

    let found = fixture
        .bridge
        .get_mobile_task("done-task")
        .unwrap()
        .unwrap();

    assert_eq!(found["id"], "done-task");
    assert_eq!(found["status"], "done");
    assert_eq!(found["project"]["name"], "kanvibe");
    assert_eq!(found["projectName"], "kanvibe");
    assert_eq!(fixture.bridge.get_mobile_task("missing").unwrap(), None);
}

// parity: TS-main-mobileBridgeService-65a2e8
#[test]
fn create_uses_the_branch_name_as_title_through_the_desktop_path() {
    let fixture = fixture();
    fixture.set_default_session_type(SessionType::Tmux);

    fixture
        .bridge
        .create_mobile_task(&MobileTaskDraft {
            priority: Some("high".to_string()),
            base_branch: Some("main".to_string()),
            ..draft()
        })
        .unwrap();

    let input = lock(&fixture.tasks.create_inputs)[0].clone();
    assert_eq!(input.title, "feat/mobile-create");
    assert_eq!(input.branch_name.as_deref(), Some("feat/mobile-create"));
    assert_eq!(input.project_id.as_deref(), Some("project-1"));
    assert_eq!(input.base_branch.as_deref(), Some("main"));
    assert_eq!(input.description, None);
    assert_eq!(input.priority, Some(TaskPriority::High));
    assert_eq!(input.session_type, Some(SessionType::Tmux));
}

// parity: TS-main-mobileBridgeService-acf53f
#[test]
fn created_worktree_task_registers_only_on_peer_desktops() {
    let fixture = fixture();
    let mut created = task("task-1");
    created.project_id = Some("project-1".to_string());
    created.branch_name = Some("feat/mobile-create".to_string());
    created.worktree_path = Some("/remote/repo__worktrees/feat-mobile-create".to_string());
    *lock(&fixture.tasks.created_task) = Some(created);
    let mut remote_project = project("project-1");
    remote_project.repo_path = "/remote/repo".to_string();
    remote_project.ssh_host = Some("devbox".to_string());
    lock(&fixture.projects.projects).push(remote_project);
    *lock(&fixture.peers.results) = vec![PeerTargetResult {
        url: "http://10.0.0.9:9736".to_string(),
        is_success: false,
        http_status: "000".to_string(),
        message: "연결할 수 없습니다".to_string(),
    }];

    let created_task = fixture.bridge.create_mobile_task(&draft()).unwrap();

    assert_eq!(created_task.id, "task-1");
    assert_eq!(
        fixture.peers.calls.all(),
        vec![
            r#"propagate_task_registration_to_peers /remote/repo feat/mobile-create Some("devbox")"#
        ]
    );
}

// parity: TS-main-mobileBridgeService-1be9f6
#[test]
fn task_without_worktree_is_not_propagated() {
    let fixture = fixture();

    fixture.bridge.create_mobile_task(&draft()).unwrap();

    assert!(fixture.peers.calls.all().is_empty());
}

// parity: TS-main-mobileBridgeService-29f754
#[test]
fn shell_expanding_branch_names_are_blocked_before_the_desktop() {
    let fixture = fixture();
    for branch_name in [r#"x"; touch /tmp/pwned; echo ""#, "$(id)", "a`id`b"] {
        let error = fixture
            .bridge
            .create_mobile_task(&MobileTaskDraft {
                branch_name: branch_name.to_string(),
                ..draft()
            })
            .unwrap_err();
        assert!(
            is_task_input(&error, "브랜치 이름"),
            "{branch_name}: {error:?}"
        );
    }

    assert_eq!(fixture.tasks.calls.count_starting_with("create_task"), 0);
}

// parity: TS-main-mobileBridgeService-2314bf
#[test]
fn parent_path_branch_names_are_blocked() {
    let fixture = fixture();

    let error = fixture
        .bridge
        .create_mobile_task(&MobileTaskDraft {
            branch_name: "../../etc/kanvibe".to_string(),
            ..draft()
        })
        .unwrap_err();

    assert!(is_task_input(&error, "브랜치 이름"));
    assert_eq!(fixture.tasks.calls.count_starting_with("create_task"), 0);
}

// parity: TS-main-mobileBridgeService-7b9cc1
#[test]
fn base_branch_follows_the_same_rule() {
    let fixture = fixture();

    let error = fixture
        .bridge
        .create_mobile_task(&MobileTaskDraft {
            base_branch: Some("main;id".to_string()),
            ..draft()
        })
        .unwrap_err();

    assert!(is_task_input(&error, "베이스 브랜치"));
    assert_eq!(fixture.tasks.calls.count_starting_with("create_task"), 0);
}

// parity: TS-main-mobileBridgeService-af177f
#[test]
fn omitted_session_type_uses_the_pc_default_and_chosen_values_are_validated() {
    let fixture = fixture();
    fixture.set_default_session_type(SessionType::Zellij);

    fixture.bridge.create_mobile_task(&draft()).unwrap();
    fixture
        .bridge
        .create_mobile_task(&MobileTaskDraft {
            session_type: Some(json!("tmux")),
            ..draft()
        })
        .unwrap();
    for rejected in [json!("screen"), json!("")] {
        let error = fixture
            .bridge
            .create_mobile_task(&MobileTaskDraft {
                session_type: Some(rejected),
                ..draft()
            })
            .unwrap_err();
        assert!(is_task_input(&error, "세션 타입"));
    }

    let inputs = lock(&fixture.tasks.create_inputs).clone();
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0].session_type, Some(SessionType::Zellij));
    assert_eq!(inputs[1].session_type, Some(SessionType::Tmux));
}

// parity: TS-main-mobileBridgeService-082386
#[test]
fn fields_missing_from_the_mobile_screen_never_reach_the_desktop() {
    let fixture = fixture();

    fixture.bridge.create_mobile_task(&draft()).unwrap();

    assert_eq!(lock(&fixture.tasks.create_inputs)[0].ssh_host, None);
}

fn prepare_session_change(fixture: &BridgeFixture) {
    let mut existing = task("task-1");
    existing.ssh_host = Some("build-host".to_string());
    existing.session_type = Some(SessionType::Tmux);
    existing.session_name = Some("repo-feat".to_string());
    fixture.tasks.put_task(with_project(existing, None));
    let mut changed = task("task-1");
    changed.session_type = Some(SessionType::Zellij);
    *lock(&fixture.tasks.session_change_result) = Some(changed);
    *lock(&fixture.dependencies.status) = Some(dependency_status(true, Some("zellij")));
}

// parity: TS-main-mobileBridgeService-731a98
#[test]
fn ready_session_tool_hands_over_to_the_desktop_change_path() {
    let fixture = fixture();
    prepare_session_change(&fixture);

    let changed = fixture
        .bridge
        .change_mobile_task_session_type("task-1", "zellij", true)
        .unwrap()
        .unwrap();

    assert_eq!(
        fixture.dependencies.calls.all(),
        vec![r#"get_session_dependency_status zellij Some("build-host")"#]
    );
    assert_eq!(
        fixture
            .tasks
            .calls
            .count_starting_with("change_task_session_type task-1 zellij true"),
        1
    );
    assert_eq!(changed.session_type, Some(SessionType::Zellij));
}

// parity: TS-main-mobileBridgeService-97ecbd
#[test]
fn missing_session_tool_is_rejected_with_its_name() {
    let fixture = fixture();
    prepare_session_change(&fixture);
    *lock(&fixture.dependencies.status) = Some(dependency_status(false, Some("zellij")));

    let error = fixture
        .bridge
        .change_mobile_task_session_type("task-1", "zellij", false)
        .unwrap_err();

    assert_eq!(error.ts_name(), "MobileSessionDependencyMissingError");
    assert!(error.to_string().contains("zellij"));
    assert_eq!(
        fixture
            .tasks
            .calls
            .count_starting_with("change_task_session_type"),
        0
    );
}

// parity: TS-main-mobileBridgeService-b21dc4
#[test]
fn unknown_session_type_is_blocked() {
    let fixture = fixture();
    prepare_session_change(&fixture);

    let error = fixture
        .bridge
        .change_mobile_task_session_type("task-1", "screen", false)
        .unwrap_err();

    assert!(is_task_input(&error, "세션 타입"));
    assert_eq!(
        fixture
            .tasks
            .calls
            .count_starting_with("change_task_session_type"),
        0
    );
}

// parity: TS-main-mobileBridgeService-50d514
#[test]
fn session_change_of_missing_task_returns_none() {
    let fixture = fixture();
    prepare_session_change(&fixture);

    assert_eq!(
        fixture
            .bridge
            .change_mobile_task_session_type("task-404", "zellij", false)
            .unwrap(),
        None
    );
    assert_eq!(
        fixture
            .tasks
            .calls
            .count_starting_with("change_task_session_type"),
        0
    );
}

// parity: TS-main-mobileBridgeService-35c62d
#[test]
fn session_change_watch_forwards_only_the_watched_task_until_released() {
    let fixture = fixture();
    let change_count = Arc::new(Mutex::new(0));
    let counted = Arc::clone(&change_count);
    let watch = fixture
        .bridge
        .watch_task_session_change("task-1", Arc::new(move || *lock(&counted) += 1));

    fixture.board.broadcast_task_session_changed("task-2");
    fixture.board.broadcast_task_session_changed("task-1");
    watch.unsubscribe();
    fixture.board.broadcast_task_session_changed("task-1");

    assert_eq!(*lock(&change_count), 1);
}

// parity: TS-main-mobileBridgeService-2ca367
#[test]
fn registration_removal_does_not_take_the_resource_deletion_path() {
    let fixture = fixture();
    *lock(&fixture.tasks.is_registration_removed) = true;

    assert!(
        fixture
            .bridge
            .remove_mobile_task_registration("task-1")
            .unwrap()
    );
    assert_eq!(
        fixture.tasks.calls.all(),
        vec!["remove_task_registration task-1"]
    );
}

fn peer_result(url: &str, is_success: bool) -> PeerTargetResult {
    PeerTargetResult {
        url: url.to_string(),
        is_success,
        http_status: if is_success { "200" } else { "000" }.to_string(),
        message: "ok".to_string(),
    }
}

// parity: TS-main-mobileBridgeService-069a38
#[test]
fn default_delete_rejects_dirty_worktrees_and_counts_failed_peers() {
    let fixture = fixture();
    *lock(&fixture.tasks.deletion) = Some(TaskDeletion {
        is_deleted: true,
        peer_results: vec![
            peer_result("http://10.0.0.7:9736", true),
            peer_result("http://10.0.0.9:9736", false),
        ],
    });

    let deletion = fixture.bridge.delete_mobile_task("task-1", false).unwrap();

    assert_eq!(to_json(deletion), json!({ "failedPeerCount": 1 }));
    assert_eq!(
        fixture.tasks.calls.all(),
        vec!["delete_task_across_targets task-1 reject_dirty=true propagate=true"]
    );
}

// parity: TS-main-mobileBridgeService-d1fe0d
#[test]
fn forced_delete_ignores_changes_and_missing_task_returns_none() {
    let fixture = fixture();

    assert_eq!(
        fixture
            .bridge
            .delete_mobile_task("missing-task", true)
            .unwrap(),
        None
    );
    assert_eq!(
        fixture.tasks.calls.all(),
        vec!["delete_task_across_targets missing-task reject_dirty=false propagate=true"]
    );
}

// parity: TS-main-mobileBridgeService-560e3e
#[test]
fn known_status_goes_through_the_desktop_path() {
    let fixture = fixture();

    fixture
        .bridge
        .update_mobile_task_status("task-1", "review")
        .unwrap();

    assert_eq!(
        fixture.tasks.calls.all(),
        vec!["update_task_status task-1 review"]
    );
}

// parity: TS-main-mobileBridgeService-50cfbd
#[test]
fn unknown_status_is_blocked() {
    let fixture = fixture();

    let error = fixture
        .bridge
        .update_mobile_task_status("task-1", "archived")
        .unwrap_err();

    assert!(is_task_input(&error, "상태"));
    assert!(fixture.tasks.calls.all().is_empty());
}

// parity: TS-main-mobileBridgeService-ba664a
#[test]
fn status_move_of_deleted_task_returns_none() {
    let fixture = fixture();
    *lock(&fixture.tasks.status_result) = Some(None);

    assert_eq!(
        fixture
            .bridge
            .update_mobile_task_status("task-1", "done")
            .unwrap(),
        None
    );
}

// parity: TS-main-mobileBridgeService-0e02a7
#[test]
fn project_branches_use_the_desktop_lookup() {
    let fixture = fixture();
    *lock(&fixture.projects.branches) = vec!["main".to_string(), "dev".to_string()];

    assert_eq!(
        fixture
            .bridge
            .get_mobile_project_branches("project-1")
            .unwrap(),
        vec!["main", "dev"]
    );
    assert_eq!(
        fixture.projects.calls.all(),
        vec!["get_project_branches project-1"]
    );
}

// parity: TS-main-mobileBridgeService-66b16d
#[test]
fn deduplicated_board_sends_each_icon_once_and_default_board_is_unchanged() {
    let fixture = fixture();
    let icon = format!("data:image/svg+xml;base64,{}", "a".repeat(2000));
    let mut first = project("p1");
    first.icon_data_url = Some(icon.clone());
    let mut second = project("p2");
    second.icon_data_url = Some(icon.clone());
    *lock(&fixture.projects.projects) = vec![first, second, project("p3")];

    let compact = to_json(fixture.bridge.get_mobile_board(true).unwrap());

    let compact_icons: Vec<(Value, Value)> = compact["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|project| (project["iconDataUrl"].clone(), project["iconKey"].clone()))
        .collect();
    assert_eq!(
        compact_icons,
        vec![
            (Value::Null, json!("0")),
            (Value::Null, json!("0")),
            (Value::Null, Value::Null)
        ]
    );
    assert_eq!(compact["projectIcons"], json!({ "0": icon }));
    assert_eq!(compact.to_string().matches(&icon).count(), 1);
    let full = to_json(fixture.bridge.get_mobile_board(false).unwrap());
    assert_eq!(
        full["projects"],
        to_json(lock(&fixture.projects.projects).clone())
    );
    assert!(full.get("projectIcons").is_none());
}

// parity: TS-main-mobileBridgeService-571bfa
#[test]
fn board_carries_the_desktop_default_session_type() {
    let fixture = fixture();
    fixture.set_default_session_type(SessionType::Zellij);

    let board = fixture.bridge.get_mobile_board(false).unwrap();

    assert_eq!(board.default_session_type, SessionType::Zellij);
}

// parity: TS-main-mobileBridgeService-f0f0f1
#[test]
fn notification_settings_are_deduplicated_and_empty_lists_kept() {
    let fixture = fixture();

    let saved = fixture
        .bridge
        .update_mobile_notification_settings(
            &json!({ "isEnabled": true, "enabledStatuses": ["review", "review", "done"] }),
        )
        .unwrap();
    assert_eq!(
        to_json(saved),
        json!({ "isEnabled": true, "enabledStatuses": ["review", "done"] })
    );
    assert_eq!(
        fixture.app_setting("notification_statuses").as_deref(),
        Some(r#"["review","done"]"#)
    );
    fixture
        .bridge
        .update_mobile_notification_settings(&json!({ "isEnabled": false, "enabledStatuses": [] }))
        .unwrap();

    assert_eq!(
        to_json(fixture.bridge.get_mobile_notification_settings().unwrap()),
        json!({ "isEnabled": false, "enabledStatuses": [] })
    );
}

// parity: TS-main-mobileBridgeService-a1f34a
#[test]
fn invalid_notification_settings_are_rejected_before_saving() {
    let inputs = [
        Value::Null,
        json!({}),
        json!({ "isEnabled": "false", "enabledStatuses": [] }),
        json!({ "isEnabled": true, "enabledStatuses": ["unknown"] }),
        json!({ "isEnabled": true, "enabledStatuses": [null] }),
    ];
    for input in inputs {
        let fixture = fixture();
        let error = fixture
            .bridge
            .update_mobile_notification_settings(&input)
            .unwrap_err();

        assert_eq!(error.ts_name(), "MobileTaskInputError", "{input}");
        assert_eq!(fixture.app_setting("notification_statuses"), None);
    }
}

fn session(session_id: &str, source_ref: &str) -> AggregatedAiSession {
    AggregatedAiSession {
        id: session_id.to_string(),
        provider: AiSessionProvider::Codex,
        started_at: None,
        updated_at: None,
        matched_path: None,
        match_scope: AiSessionMatchScope::Worktree,
        title: None,
        first_user_prompt: None,
        message_count: 0,
        source_ref: Some(source_ref.to_string()),
    }
}

fn history(sessions: Vec<AggregatedAiSession>) -> AggregatedAiSessionsResult {
    AggregatedAiSessionsResult {
        sessions,
        ..AggregatedAiSessionsResult::default()
    }
}

fn detail(session_id: &str, source_ref: Option<&str>) -> AggregatedAiSessionDetail {
    AggregatedAiSessionDetail {
        session_id: session_id.to_string(),
        provider: AiSessionProvider::Codex,
        title: None,
        matched_path: None,
        source_ref: source_ref.map(str::to_string),
        messages: Vec::new(),
        next_cursor: None,
    }
}

fn history_options(cursor: Option<&str>) -> MobileAiHistoryOptions {
    MobileAiHistoryOptions {
        query: None,
        cursor: cursor.map(str::to_string),
        limit: 20,
        roles: None,
    }
}

fn last_detail_request(fixture: &BridgeFixture) -> (String, Option<String>, Option<String>, usize) {
    let (task_id, request) = lock(&fixture.projects.detail_requests)
        .last()
        .cloned()
        .unwrap();
    (task_id, request.source_ref, request.cursor, request.limit)
}

fn queue_history(fixture: &BridgeFixture, sessions: Vec<AggregatedAiSession>) {
    lock(&fixture.projects.ai_sessions).push_back(history(sessions));
}

fn queue_detail(
    fixture: &BridgeFixture,
    result: Result<Option<AggregatedAiSessionDetail>, AiHistoryReadError>,
) {
    lock(&fixture.projects.ai_details).push_back(result);
}

const CODEX: AiSessionProvider = AiSessionProvider::Codex;

// parity: TS-main-mobileBridgeService-a428ce
#[test]
fn attachments_come_only_from_history_found_in_the_current_task() {
    let fixture = fixture();
    queue_history(
        &fixture,
        vec![session("image-session", "/trusted/image-rollout.jsonl")],
    );
    *lock(&fixture.projects.attachment) = Some(Ok(Some(AiSessionAttachmentContent {
        id: "image-id".to_string(),
        name: "image.png".to_string(),
        mime_type: "image/png".to_string(),
        data_url: "data:image/png;base64,image".to_string(),
    })));

    let attachment = fixture
        .bridge
        .get_mobile_ai_session_attachment(
            "history-image-task",
            CODEX,
            "image-session",
            "image-id",
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(attachment.id, "image-id");
    let (task_id, request) = lock(&fixture.projects.attachment_requests)
        .last()
        .cloned()
        .unwrap();
    assert_eq!(task_id, "history-image-task");
    assert_eq!(
        request.source_ref.as_deref(),
        Some("/trusted/image-rollout.jsonl")
    );
    queue_history(&fixture, Vec::new());
    let before = lock(&fixture.projects.attachment_requests).len();

    let other = fixture
        .bridge
        .get_mobile_ai_session_attachment(
            "other-image-task",
            CODEX,
            "image-session",
            "image-id",
            None,
        )
        .unwrap();

    assert_eq!(other, None);
    assert_eq!(lock(&fixture.projects.attachment_requests).len(), before);
}

// parity: TS-main-mobileBridgeService-ec098b
#[test]
fn task_file_is_read_from_the_workspace_and_failure_reason_is_kept() {
    let fixture = fixture();
    *lock(&fixture.diff.workspace_file) = Some(Ok(TaskWorkspaceFile {
        path: "docs/plan.md".to_string(),
        content: "# plan".to_string(),
    }));

    let file = fixture
        .bridge
        .get_mobile_task_file("file-task", "/work/task/docs/plan.md")
        .unwrap();
    assert_eq!(
        to_json(file),
        json!({ "path": "docs/plan.md", "content": "# plan" })
    );
    assert_eq!(
        fixture.diff.calls.all(),
        vec!["read_task_workspace_file file-task /work/task/docs/plan.md"]
    );
    *lock(&fixture.diff.workspace_file) = Some(Err(TaskWorkspaceFileFailure::TooLarge));

    let error = fixture
        .bridge
        .get_mobile_task_file("file-task", "big.log")
        .unwrap_err();

    assert_eq!(error.ts_name(), "TaskWorkspaceFileError");
    assert_eq!(
        error,
        MobileError::TaskWorkspaceFile(TaskWorkspaceFileFailure::TooLarge)
    );
    assert_eq!(error.to_string(), "too-large");
}

// parity: TS-main-mobileBridgeService-705019
#[test]
fn attachment_reader_errors_are_not_exposed() {
    let fixture = fixture();
    *lock(&fixture.projects.attachment) = Some(Err(ServiceError::new("private host path")));

    let error = fixture
        .bridge
        .get_mobile_ai_session_attachment(
            "history-image-explicit",
            CODEX,
            "image-session",
            "image-id",
            Some("/trusted/source"),
        )
        .unwrap_err();

    assert_eq!(
        error,
        MobileError::TaskInput(
            "첨부 파일을 불러올 수 없습니다. 파일 형식·크기와 원본 파일을 확인해 주세요."
                .to_string()
        )
    );
}

// parity: TS-main-mobileBridgeService-5ad3db
#[test]
fn listed_source_is_reused_for_details_and_stripped_from_responses() {
    let fixture = fixture();
    queue_history(&fixture, vec![session("s1", "/trusted/rollout.jsonl")]);
    queue_detail(
        &fixture,
        Ok(Some(detail("s1", Some("/trusted/rollout.jsonl")))),
    );
    queue_detail(
        &fixture,
        Ok(Some(detail("s1", Some("/trusted/rollout.jsonl")))),
    );

    let list = to_json(
        fixture
            .bridge
            .get_mobile_ai_sessions("history-cache-task", &history_options(None))
            .unwrap(),
    );
    let first = fixture
        .bridge
        .get_mobile_ai_session_detail("history-cache-task", CODEX, "s1", &history_options(None))
        .unwrap()
        .unwrap();
    fixture
        .bridge
        .get_mobile_ai_session_detail(
            "history-cache-task",
            CODEX,
            "s1",
            &history_options(Some("20")),
        )
        .unwrap();

    assert!(list["sessions"][0].get("sourceRef").is_none());
    assert!(first.get("sourceRef").is_none());
    assert_eq!(
        fixture
            .projects
            .calls
            .count_starting_with("get_task_ai_sessions"),
        1
    );
    assert_eq!(
        last_detail_request(&fixture),
        (
            "history-cache-task".to_string(),
            Some("/trusted/rollout.jsonl".to_string()),
            Some("20".to_string()),
            20
        )
    );
}

// parity: TS-main-mobileBridgeService-186647
#[test]
fn stale_cached_source_is_looked_up_once_more() {
    let fixture = fixture();
    queue_history(&fixture, vec![session("s2", "/old/file")]);
    fixture
        .bridge
        .get_mobile_ai_sessions("history-stale-task", &history_options(None))
        .unwrap();
    queue_history(&fixture, vec![session("s2", "/new/file")]);
    queue_detail(&fixture, Ok(None));
    queue_detail(&fixture, Ok(Some(detail("s2", None))));

    let found = fixture
        .bridge
        .get_mobile_ai_session_detail("history-stale-task", CODEX, "s2", &history_options(None))
        .unwrap()
        .unwrap();

    assert_eq!(found["sessionId"], "s2");
    assert_eq!(
        last_detail_request(&fixture),
        (
            "history-stale-task".to_string(),
            Some("/new/file".to_string()),
            None,
            20
        )
    );
}

// parity: TS-main-mobileBridgeService-d427bf
#[test]
fn same_session_id_in_another_task_does_not_share_the_cache() {
    let fixture = fixture();
    queue_history(&fixture, vec![session("same", "/task-a/file")]);
    fixture
        .bridge
        .get_mobile_ai_sessions("history-cache-a", &history_options(None))
        .unwrap();
    queue_history(&fixture, Vec::new());

    let found = fixture
        .bridge
        .get_mobile_ai_session_detail("history-cache-b", CODEX, "same", &history_options(None))
        .unwrap();

    assert_eq!(found, None);
    assert!(lock(&fixture.projects.detail_requests).is_empty());
}

// parity: TS-main-mobileBridgeService-1d083d
#[test]
fn chosen_session_is_found_even_beyond_the_cache_limit() {
    let fixture = fixture();
    let sessions = (0..600)
        .map(|index| session(&format!("many-{index}"), &format!("/many/{index}")))
        .collect();
    queue_history(&fixture, sessions);
    queue_detail(&fixture, Ok(Some(detail("many-0", None))));

    let found = fixture
        .bridge
        .get_mobile_ai_session_detail("history-many-task", CODEX, "many-0", &history_options(None))
        .unwrap()
        .unwrap();

    assert_eq!(found["sessionId"], "many-0");
    assert_eq!(
        last_detail_request(&fixture),
        (
            "history-many-task".to_string(),
            Some("/many/0".to_string()),
            None,
            20
        )
    );
}

// parity: TS-main-mobileBridgeService-65cec0
#[test]
fn deleted_cached_source_is_looked_up_once_more() {
    let fixture = fixture();
    queue_history(&fixture, vec![session("gone", "/deleted/file")]);
    fixture
        .bridge
        .get_mobile_ai_sessions("history-deleted-file", &history_options(None))
        .unwrap();
    queue_history(&fixture, vec![session("gone", "/current/file")]);
    queue_detail(
        &fixture,
        Err(AiHistoryReadError::SourceMissing(
            "missing source".to_string(),
        )),
    );
    queue_detail(&fixture, Ok(Some(detail("gone", None))));

    let found = fixture
        .bridge
        .get_mobile_ai_session_detail(
            "history-deleted-file",
            CODEX,
            "gone",
            &history_options(None),
        )
        .unwrap()
        .unwrap();

    assert_eq!(found["sessionId"], "gone");
    assert_eq!(
        last_detail_request(&fixture),
        (
            "history-deleted-file".to_string(),
            Some("/current/file".to_string()),
            None,
            20
        )
    );
}

// parity: TS-main-mobileBridgeService-0793a9
#[test]
fn permission_errors_stay_detail_failures() {
    let fixture = fixture();
    queue_history(&fixture, vec![session("denied", "/denied/file")]);
    fixture
        .bridge
        .get_mobile_ai_sessions("history-permission-file", &history_options(None))
        .unwrap();
    queue_detail(
        &fixture,
        Err(AiHistoryReadError::Failed("permission denied".to_string())),
    );

    let error = fixture
        .bridge
        .get_mobile_ai_session_detail(
            "history-permission-file",
            CODEX,
            "denied",
            &history_options(None),
        )
        .unwrap_err();

    assert_eq!(
        error,
        MobileError::Internal("permission denied".to_string())
    );
}

fn folder_fixture() -> (BridgeFixture, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let fixture = fixture_with_home(directory.path());
    *lock(&fixture.projects.subdirectories) = vec!["repo".to_string()];
    *lock(&fixture.projects.scan_result) = ScanResult {
        registered: vec!["repo".to_string()],
        ..ScanResult::default()
    };
    (fixture, directory)
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

// parity: TS-main-mobileBridgeService-289274
#[test]
fn existing_folder_is_verified_then_listed_by_the_desktop_service() {
    let (fixture, directory) = folder_fixture();
    std::fs::create_dir(directory.path().join("repo")).unwrap();
    let root = path_text(directory.path());

    assert_eq!(
        fixture.bridge.get_mobile_directories(&root).unwrap(),
        vec!["repo"]
    );
    assert_eq!(
        fixture.projects.calls.all(),
        vec![format!("list_subdirectories {root} None")]
    );
}

// parity: TS-main-mobileBridgeService-f1bdf0
#[test]
fn home_shorthand_resolves_to_the_connected_pc_home() {
    let (fixture, directory) = folder_fixture();

    fixture.bridge.get_mobile_directories("~").unwrap();

    assert_eq!(
        fixture.projects.calls.all(),
        vec![format!(
            "list_subdirectories {} None",
            path_text(directory.path())
        )]
    );
}

// parity: TS-main-mobileBridgeService-5eee20
#[test]
fn disallowed_paths_are_rejected_before_any_lookup() {
    let (fixture, _directory) = folder_fixture();
    for submitted in [
        "relative/path",
        "~another/path",
        "/bad\u{0}path",
        "/bad\npath",
        "",
    ] {
        let error = fixture
            .bridge
            .get_mobile_directories(submitted)
            .unwrap_err();
        assert_eq!(error.ts_name(), "MobileTaskInputError", "{submitted:?}");
    }

    assert!(fixture.projects.calls.all().is_empty());
}

// parity: TS-main-mobileBridgeService-2d9063
#[test]
fn missing_folders_and_plain_files_are_not_shown_as_empty_folders() {
    let (fixture, directory) = folder_fixture();
    std::fs::write(directory.path().join("file.txt"), "content").unwrap();

    for submitted in [
        directory.path().join("missing"),
        directory.path().join("file.txt"),
    ] {
        let error = fixture
            .bridge
            .get_mobile_directories(&path_text(&submitted))
            .unwrap_err();
        assert_eq!(error.ts_name(), "MobileTaskInputError");
    }

    assert!(fixture.projects.calls.all().is_empty());
}

// parity: TS-main-mobileBridgeService-459621
#[test]
fn scan_verifies_the_folder_and_keeps_the_scan_result() {
    let (fixture, directory) = folder_fixture();
    let root = path_text(directory.path());

    let result = fixture.bridge.scan_mobile_projects(&root).unwrap();
    assert_eq!(result.registered, vec!["repo"]);
    let error = fixture
        .bridge
        .scan_mobile_projects(&format!("{root}/missing"))
        .unwrap_err();

    assert_eq!(error.ts_name(), "MobileTaskInputError");
    assert_eq!(
        fixture.projects.calls.all(),
        vec![format!("scan_and_register_projects {root} None")]
    );
}
