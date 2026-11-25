use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use kanvibe_domain::notifications::{
    AppNotificationAction, BackgroundSyncRegisteredWorktreePayload, BackgroundSyncReviewPayload,
    TaskPrMergedDetectedPayload,
};

use super::*;
use crate::board_notifier::TaskStatusChangedPayload;
use crate::notification_store::DatabaseNotificationListStorage;
use crate::test_support::TemporaryDatabase;

/// 띄운 알림과 클릭 처리를 모아 두어 테스트가 직접 누를 수 있게 한다
struct RecordingPresenter {
    is_supported: bool,
    shown: Mutex<Vec<(AppNotification, Option<NotificationClick>)>>,
}

impl RecordingPresenter {
    fn new(is_supported: bool) -> Arc<Self> {
        Arc::new(Self {
            is_supported,
            shown: Mutex::new(Vec::new()),
        })
    }

    fn shown_count(&self) -> usize {
        self.shown.lock().unwrap().len()
    }

    fn click(&self, index: usize) {
        let on_click = self.shown.lock().unwrap()[index].1.take().unwrap();
        on_click();
    }
}

impl DesktopNotificationPresenter for RecordingPresenter {
    fn is_supported(&self) -> bool {
        self.is_supported
    }

    fn show(&self, notification: &AppNotification, on_click: NotificationClick) {
        self.shown
            .lock()
            .unwrap()
            .push((notification.clone(), Some(on_click)));
    }
}

struct Fixture {
    database: TemporaryDatabase,
    store: Arc<NotificationStore>,
    presenter: Arc<RecordingPresenter>,
    service: DesktopNotificationService,
}

fn fixture() -> Fixture {
    let database = TemporaryDatabase::open();
    let storage = DatabaseNotificationListStorage::new(Arc::clone(&database.database));
    let store = Arc::new(NotificationStore::new(Arc::new(storage)));
    let presenter = RecordingPresenter::new(true);
    let service = DesktopNotificationService::new(
        Arc::clone(&database.database),
        Arc::clone(&store),
        Arc::clone(&presenter) as Arc<dyn DesktopNotificationPresenter>,
    );
    Fixture {
        database,
        store,
        presenter,
        service,
    }
}

impl Fixture {
    fn set_notification_settings(&self, is_enabled: bool, statuses: &[&str]) {
        let settings =
            AppSettingsService::new(&self.database.database, ShortcutPlatform::current());
        settings.set_notification_enabled(is_enabled).unwrap();
        let statuses: Vec<String> = statuses.iter().map(|status| status.to_string()).collect();
        settings.set_notification_statuses(&statuses).unwrap();
    }
}

fn counting_callbacks() -> (NotificationDeliveryCallbacks, Arc<AtomicUsize>) {
    let change_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&change_count);
    let callbacks = NotificationDeliveryCallbacks {
        on_notifications_changed: Some(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        })),
        on_notification_click: None,
    };
    (callbacks, change_count)
}

fn task_payload() -> DesktopNotificationPayload {
    DesktopNotificationPayload {
        title: "title".into(),
        body: "body".into(),
        task_id: Some("task-1".into()),
        locale: "ko".into(),
        relative_path: Some("/ko/task/task-1".into()),
        dedupe_key: Some("task-status:task-1:review".into()),
        action: None,
    }
}

fn status_changed(new_status: &str) -> BoardEvent {
    BoardEvent::TaskStatusChanged(TaskStatusChangedPayload {
        project_name: "kanvibe".into(),
        branch_name: "feat/login".into(),
        task_title: "로그인 구현".into(),
        description: None,
        new_status: new_status.into(),
        task_id: "task-1".into(),
    })
}

// parity: TS-main-desktopNotificationService-be6804
#[test]
fn stores_sync_results_quietly_even_when_os_notifications_are_off() {
    let fixture = fixture();
    fixture.set_notification_settings(false, &[]);
    let (callbacks, change_count) = counting_callbacks();
    let review = BoardEvent::BackgroundSyncReviewNeeded(BackgroundSyncReviewPayload {
        merged_pull_requests: Vec::new(),
        registered_worktrees: Vec::new(),
        pulled_tasks: None,
        failures: None,
    });

    let is_handled = fixture
        .service
        .deliver_board_event_notification(&review, "ko", &callbacks)
        .unwrap();

    assert!(is_handled);
    assert_eq!(fixture.store.list_notifications().unwrap().len(), 1);
    assert_eq!(change_count.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.presenter.shown_count(), 0);
}

// parity: TS-main-desktopNotificationService-52044a
#[test]
fn an_existing_notification_counts_as_delivered_without_showing_again() {
    let fixture = fixture();
    let (callbacks, _) = counting_callbacks();
    fixture.store.create_notification(&task_payload()).unwrap();

    let is_delivered = fixture
        .service
        .deliver_desktop_notification(&task_payload(), &callbacks)
        .unwrap();

    assert!(is_delivered);
    assert_eq!(fixture.presenter.shown_count(), 0);
}

// parity: TS-main-desktopNotificationService-8e883c
#[test]
fn shows_a_new_notification_and_reads_it_on_click() {
    let fixture = fixture();
    let (mut callbacks, change_count) = counting_callbacks();
    let clicked = Arc::new(Mutex::new(Vec::new()));
    let clicked_sink = Arc::clone(&clicked);
    callbacks.on_notification_click = Some(Arc::new(move |notification: &AppNotification| {
        clicked_sink.lock().unwrap().push(notification.id.clone());
    }));

    let is_delivered = fixture
        .service
        .deliver_desktop_notification(&task_payload(), &callbacks)
        .unwrap();
    fixture.presenter.click(0);

    let stored = fixture.store.list_notifications().unwrap();
    assert!(is_delivered);
    assert_eq!(fixture.presenter.shown_count(), 1);
    assert_eq!(change_count.load(Ordering::SeqCst), 2);
    assert!(stored[0].is_read);
    assert_eq!(*clicked.lock().unwrap(), [stored[0].id.clone()]);
}

// parity: TS-main-desktopNotificationService-8e2506
#[test]
fn delivers_board_events_whose_status_is_enabled() {
    let fixture = fixture();
    fixture.set_notification_settings(true, &["review"]);

    let is_delivered = fixture
        .service
        .deliver_board_event_notification(&status_changed("review"), "ko", &Default::default())
        .unwrap();

    let stored = fixture.store.list_notifications().unwrap();
    assert!(is_delivered);
    assert_eq!(stored[0].title, "kanvibe — feat/login");
    assert_eq!(stored[0].dedupe_key, "task-status:task-1:review");
    assert_eq!(fixture.presenter.shown_count(), 1);
}

// parity: TS-main-desktopNotificationService-624873
#[test]
fn ignores_board_events_whose_status_is_disabled() {
    let fixture = fixture();
    fixture.set_notification_settings(true, &["progress"]);

    let is_delivered = fixture
        .service
        .deliver_board_event_notification(&status_changed("review"), "ko", &Default::default())
        .unwrap();

    assert!(!is_delivered);
    assert!(fixture.store.list_notifications().unwrap().is_empty());
}

// parity: TS-main-desktopNotificationService-6910d3
#[test]
fn background_sync_review_creates_a_notification_with_review_action() {
    let fixture = fixture();
    fixture.set_notification_settings(true, &["progress"]);
    let review = BackgroundSyncReviewPayload {
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
        pulled_tasks: Some(Vec::new()),
        failures: None,
    };

    let is_handled = fixture
        .service
        .deliver_board_event_notification(
            &BoardEvent::BackgroundSyncReviewNeeded(review.clone()),
            "ko",
            &Default::default(),
        )
        .unwrap();

    let stored = fixture.store.list_notifications().unwrap();
    assert!(is_handled);
    assert_eq!(
        (stored[0].locale.as_str(), stored[0].relative_path.as_str()),
        ("ko", "/ko")
    );
    let Some(Some(AppNotificationAction::BackgroundSyncReview { payload })) = &stored[0].action
    else {
        panic!("review action이 있어야 한다");
    };
    assert_eq!(payload.merged_pull_requests, review.merged_pull_requests);
    assert_eq!(payload.registered_worktrees, review.registered_worktrees);
    assert_eq!(payload.pulled_tasks, Some(Vec::new()));
}

#[test]
fn reports_unsupported_os_notifications_after_storing() {
    let database = TemporaryDatabase::open();
    let storage = DatabaseNotificationListStorage::new(Arc::clone(&database.database));
    let store = Arc::new(NotificationStore::new(Arc::new(storage)));
    let service = DesktopNotificationService::new(
        Arc::clone(&database.database),
        Arc::clone(&store),
        RecordingPresenter::new(false),
    );

    let is_delivered = service
        .deliver_desktop_notification(&task_payload(), &Default::default())
        .unwrap();

    assert!(!is_delivered);
    assert_eq!(store.list_notifications().unwrap().len(), 1);
}
