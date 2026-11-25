//! 보드 이벤트가 OS 알림과 알림 목록에 어떤 제목·본문·이동 경로로 남는지 확인한다.
//! TS에서는 렌더러 NotificationListener가 설정을 거른 뒤 useTaskNotification이 Electron 알림 다리로 넘겼고,
//! Rust에서는 런타임의 알림 전달이 같은 이벤트를 받아 설정을 거르고 표시기에 바로 넘긴다.

// 공용 격리 환경 가운데 이 파일이 쓰지 않는 도우미(git·tmux 작업 생성)는 runtime_wiring이 쓴다
#[allow(dead_code)]
mod support;

use std::sync::{Arc, Mutex};

use kanvibe_domain::notifications::{
    AppNotification, AppNotificationAction, BackgroundSyncRegisteredWorktreePayload,
    BackgroundSyncReviewPayload, TaskPrMergedDetectedPayload,
};
use kanvibe_runtime::DesktopRuntime;
use kanvibe_services::board_notifier::{
    BoardEvent, HookStatusTargetMissingPayload, HookStatusTargetMissingReason,
    TaskStatusChangedPayload,
};
use kanvibe_services::desktop_notification::{
    DesktopNotificationPresenter, NotificationClick, NotificationDeliveryCallbacks,
};

use support::RuntimeFixture;

/// 띄운 OS 알림을 그대로 모은다. 실제 알림 데몬에는 닿지 않는다
#[derive(Default)]
struct RecordingPresenter {
    shown_notifications: Mutex<Vec<AppNotification>>,
}

impl DesktopNotificationPresenter for RecordingPresenter {
    fn is_supported(&self) -> bool {
        true
    }

    fn show(&self, notification: &AppNotification, _on_click: NotificationClick) {
        self.shown_notifications
            .lock()
            .unwrap()
            .push(notification.clone());
    }
}

/// 알림 문구 locale을 고정하고 표시기를 기록용으로 바꾼 런타임
fn runtime_with_presenter(
    fixture: &RuntimeFixture,
    locale: &str,
) -> (DesktopRuntime, Arc<RecordingPresenter>) {
    let presenter = Arc::new(RecordingPresenter::default());
    let mut config = fixture.config();
    config.notification_presenter = presenter.clone();
    let fixed_locale = locale.to_string();
    config.notification_locale = Arc::new(move || fixed_locale.clone());
    (DesktopRuntime::new(config).unwrap(), presenter)
}

/// 런타임을 켜고 이벤트 하나를 흘린 뒤 띄운 OS 알림을 돌려준다
fn shown_after(
    runtime: &DesktopRuntime,
    presenter: &RecordingPresenter,
    event: BoardEvent,
) -> Vec<AppNotification> {
    runtime.start();
    runtime.board_notifier().emit(event);
    runtime.shutdown();
    presenter.shown_notifications.lock().unwrap().clone()
}

fn status_changed(
    branch_name: &str,
    task_title: &str,
    description: Option<&str>,
    new_status: &str,
    task_id: &str,
) -> BoardEvent {
    BoardEvent::TaskStatusChanged(TaskStatusChangedPayload {
        project_name: "kanvibe".into(),
        branch_name: branch_name.into(),
        task_title: task_title.into(),
        description: description.map(String::from),
        new_status: new_status.into(),
        task_id: task_id.into(),
    })
}

fn hook_target_missing() -> BoardEvent {
    BoardEvent::HookStatusTargetMissing(HookStatusTargetMissingPayload {
        task_id: "task-404".into(),
        requested_status: "review".into(),
        reason: HookStatusTargetMissingReason::TaskNotFound,
    })
}

// parity: TS-renderer-NotificationListener-a52653
// parity: TS-hooks-useTaskNotification-109e97
#[test]
fn enabled_status_change_raises_os_notification_with_title_status_and_description() {
    let fixture = RuntimeFixture::new();
    let (runtime, presenter) = runtime_with_presenter(&fixture, "ko");

    let shown = shown_after(
        &runtime,
        &presenter,
        status_changed(
            "feat/login",
            "로그인 구현",
            Some("OAuth 연동"),
            "review",
            "task-123",
        ),
    );

    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].title, "kanvibe — feat/login");
    assert_eq!(shown[0].body, "로그인 구현: review로 변경\nOAuth 연동");
    assert_eq!(shown[0].task_id.as_deref(), Some("task-123"));
    assert_eq!(shown[0].locale, "ko");
}

// parity: TS-hooks-useTaskNotification-a24420
#[test]
fn missing_description_leaves_only_the_status_line_in_the_body() {
    let fixture = RuntimeFixture::new();
    let (runtime, presenter) = runtime_with_presenter(&fixture, "en");

    let shown = shown_after(
        &runtime,
        &presenter,
        status_changed("feat/test", "테스트 작업", None, "progress", "task-456"),
    );

    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].title, "kanvibe — feat/test");
    assert_eq!(shown[0].body, "테스트 작업: changed to progress");
    assert_eq!(shown[0].locale, "en");
}

// parity: TS-hooks-useTaskNotification-df0527
#[test]
fn os_notification_carries_the_task_detail_path_and_status_dedupe_key() {
    let fixture = RuntimeFixture::new();
    let (runtime, presenter) = runtime_with_presenter(&fixture, "ko");

    let shown = shown_after(
        &runtime,
        &presenter,
        status_changed(
            "feat/electron",
            "네이티브 알림",
            Some("OS 알림 테스트"),
            "review",
            "task-desktop",
        ),
    );

    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].title, "kanvibe — feat/electron");
    assert_eq!(
        shown[0].body,
        "네이티브 알림: review로 변경\nOS 알림 테스트"
    );
    assert_eq!(shown[0].task_id.as_deref(), Some("task-desktop"));
    assert_eq!(shown[0].relative_path, "/ko/task/task-desktop");
    assert_eq!(shown[0].dedupe_key, "task-status:task-desktop:review");
}

// parity: TS-renderer-NotificationListener-fd5513
#[test]
fn status_outside_the_enabled_statuses_raises_no_notification() {
    let fixture = RuntimeFixture::new();
    let (runtime, presenter) = runtime_with_presenter(&fixture, "ko");
    runtime
        .app_settings()
        .set_notification_statuses(&["progress".to_string()])
        .unwrap();

    let shown = shown_after(
        &runtime,
        &presenter,
        status_changed("feat/login", "로그인", None, "review", "task-1"),
    );

    assert!(shown.is_empty(), "{shown:?}");
    let stored = runtime.notification_store().list_notifications().unwrap();
    assert!(stored.is_empty(), "{stored:?}");
}

fn background_sync_review() -> BackgroundSyncReviewPayload {
    BackgroundSyncReviewPayload {
        merged_pull_requests: vec![TaskPrMergedDetectedPayload {
            task_id: "task-10".into(),
            task_title: "Merged PR task".into(),
            branch_name: "feature/merged-pr".into(),
            pr_url: "https://github.com/kanvibe/kanvibe/pull/211".into(),
            merged_at: "2026-04-30T02:00:00Z".into(),
        }],
        registered_worktrees: vec![BackgroundSyncRegisteredWorktreePayload {
            task_id: "task-worktree".into(),
            project_name: "api".into(),
            branch_name: "feature-sync".into(),
            worktree_path: "/workspace/api__worktrees/feature-sync".into(),
            ssh_host: None,
        }],
        pulled_tasks: None,
        failures: None,
    }
}

// parity: TS-renderer-NotificationListener-daa175
#[test]
fn background_sync_review_lands_in_the_list_with_the_review_action() {
    let fixture = RuntimeFixture::new();
    let (runtime, presenter) = runtime_with_presenter(&fixture, "ko");
    let review = background_sync_review();

    // TS도 메인이 먼저 목록에 남긴 뒤 렌더러 다리 호출이 중복 제거되어 OS 알림은 뜨지 않았다
    let shown = shown_after(
        &runtime,
        &presenter,
        BoardEvent::BackgroundSyncReviewNeeded(review.clone()),
    );

    assert!(shown.is_empty(), "{shown:?}");
    let stored = runtime.notification_store().list_notifications().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].title, "백그라운드 sync 검토 필요");
    assert_eq!(stored[0].body, "merge된 PR 1건 / 새 TODO worktree 1건");
    assert_eq!(stored[0].relative_path, "/ko");
    let Some(Some(AppNotificationAction::BackgroundSyncReview { payload })) = &stored[0].action
    else {
        panic!("검토 동작이 없다: {:?}", stored[0].action);
    };
    assert_eq!(payload.merged_pull_requests, review.merged_pull_requests);
    assert_eq!(payload.registered_worktrees, review.registered_worktrees);
}

// parity: TS-hooks-useTaskNotification-f3eeb1
#[test]
fn missing_hook_target_raises_a_notification_naming_the_task_and_status() {
    let fixture = RuntimeFixture::new();
    let (runtime, presenter) = runtime_with_presenter(&fixture, "ko");

    let shown = shown_after(&runtime, &presenter, hook_target_missing());

    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].title, "Hook target missing — task-404");
    assert_eq!(
        shown[0].body,
        "review 상태로 변경하지 못했습니다.\n연결된 작업을 찾지 못했습니다."
    );
    assert_eq!(shown[0].locale, "ko");
}

// parity: TS-hooks-useTaskNotification-ab2d1c
#[test]
fn missing_hook_target_body_follows_the_notification_locale() {
    let fixture = RuntimeFixture::new();
    let (runtime, presenter) = runtime_with_presenter(&fixture, "en");

    let shown = shown_after(&runtime, &presenter, hook_target_missing());

    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].title, "Hook target missing — task-404");
    assert_eq!(
        shown[0].body,
        "Failed to change status to review.\nNo matching task was found."
    );
    assert_eq!(shown[0].locale, "en");
}

// parity: TS-hooks-useTaskNotification-b8d5b1
#[test]
fn without_an_os_notification_channel_nothing_pops_up_but_the_list_keeps_it() {
    let fixture = RuntimeFixture::new();
    // 기본 설정은 창 없는 실행용 표시기라 OS 알림을 띄울 수단이 없다
    let runtime = fixture.open_runtime();

    let is_shown_by_os = runtime
        .desktop_notifications()
        .deliver_board_event_notification(
            &status_changed("feat/test", "테스트", None, "review", "task-789"),
            "zh",
            &NotificationDeliveryCallbacks::default(),
        )
        .unwrap();

    assert!(!is_shown_by_os);
    let stored = runtime.notification_store().list_notifications().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].body, "테스트: 已变更为review");
}
