//! 앱이 런타임 하나를 열어 보드 이벤트·알림·새로 고침 신호를 전경 스레드로 옮기는지, 화면 전환 뒤 창 루트가
//! 포커스를 되찾는지 확인한다.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::gpui;
use gpui_kit::{AppContext, Context, Entity, Subscription, TestAppContext};
use kanvibe_app::app_events::{RefreshScope, subscribe_to_refresh};
use kanvibe_app::app_window::AppWindow;
use kanvibe_app::notifications::NotificationCenter;
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions};
use kanvibe_app::services::{AppServices, format_service_error};
use kanvibe_domain::notifications::DesktopNotificationPayload;
use kanvibe_services::board_notifier::BoardEvent;

fn init_app(cx: &mut TestAppContext, data_dir: PathBuf) {
    let launch = LaunchOptions {
        data_dir: Some(data_dir),
        initial_route: "/ko".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: None,
    };
    cx.update(|cx| kanvibe_app::init_app(&launch, cx).expect("app init"));
}

/// 받은 새로 고침 수를 센다
struct RefreshProbe {
    refresh_count: usize,
    _subscription: Option<Subscription>,
}

impl RefreshProbe {
    fn new(cx: &mut Context<Self>) -> Self {
        let subscription = subscribe_to_refresh(&[RefreshScope::Board], cx, |probe, _| {
            probe.refresh_count += 1;
        });
        Self {
            refresh_count: 0,
            _subscription: subscription,
        }
    }
}

fn emit_board_event(cx: &mut TestAppContext, event: BoardEvent) {
    cx.update(|cx| {
        let runtime = cx.global::<AppServices>().runtime().unwrap().clone();
        runtime.board_notifier().emit(event);
    });
}

#[gpui::test]
fn board_updated_bursts_become_one_refresh_after_the_ts_debounce(cx: &mut TestAppContext) {
    let data_dir = tempfile::tempdir().unwrap();
    init_app(cx, data_dir.path().to_path_buf());
    let probe: Entity<RefreshProbe> = cx.update(|cx| cx.new(RefreshProbe::new));

    emit_board_event(cx, BoardEvent::BoardUpdated);
    emit_board_event(cx, BoardEvent::BoardUpdated);
    cx.run_until_parked();
    assert_eq!(probe.read_with(cx, |probe, _| probe.refresh_count), 0);
    cx.executor().advance_clock(Duration::from_millis(300));
    cx.run_until_parked();

    assert_eq!(probe.read_with(cx, |probe, _| probe.refresh_count), 1);
}

#[gpui::test]
fn notification_center_lists_what_the_runtime_store_holds(cx: &mut TestAppContext) {
    let data_dir = tempfile::tempdir().unwrap();
    init_app(cx, data_dir.path().to_path_buf());

    cx.update(|cx| {
        let runtime = cx.global::<AppServices>().runtime().unwrap().clone();
        runtime
            .notification_store()
            .create_notification(&DesktopNotificationPayload {
                title: "Review 상태로 바뀌었습니다".into(),
                body: "알림 확인".into(),
                task_id: Some("task-1".into()),
                locale: "ko".into(),
                relative_path: None,
                dedupe_key: None,
                action: None,
            })
            .unwrap();
        kanvibe_app::notifications::reload_notification_center(cx);
    });

    let unread_count = cx.update(|cx| cx.global::<NotificationCenter>().unread_count());
    assert_eq!(unread_count, 1);
}

// TS `electron/main.js` `subscribeToNotificationChanges(broadcastNotificationsChanged)`: 모바일이 읽음 처리한 것도
// 데스크톱 알림 목록에 바로 반영된다
#[gpui::test]
fn notification_center_follows_store_writes_from_other_writers(cx: &mut TestAppContext) {
    let data_dir = tempfile::tempdir().unwrap();
    init_app(cx, data_dir.path().to_path_buf());
    let store = cx.update(|cx| {
        cx.global::<AppServices>()
            .runtime()
            .unwrap()
            .notification_store()
            .clone()
    });

    let created = store
        .create_notification(&DesktopNotificationPayload {
            title: "모바일 읽음".into(),
            body: "모바일에서 읽을 알림".into(),
            task_id: None,
            locale: "ko".into(),
            relative_path: None,
            dedupe_key: None,
            action: None,
        })
        .unwrap();
    cx.run_until_parked();
    let unread_after_create = cx.update(|cx| cx.global::<NotificationCenter>().unread_count());
    store
        .mark_notification_read(&created.notification.id)
        .unwrap();
    cx.run_until_parked();
    let unread_after_read = cx.update(|cx| cx.global::<NotificationCenter>().unread_count());

    assert_eq!(unread_after_create, 1);
    assert_eq!(unread_after_read, 0);
}

#[gpui::test]
fn route_change_takes_focus_back_from_the_previous_screen(cx: &mut TestAppContext) {
    let data_dir = tempfile::tempdir().unwrap();
    init_app(cx, data_dir.path().to_path_buf());
    let window = cx.add_window(|window, cx| AppWindow::new("/ko", window, cx));
    cx.run_until_parked();
    let terminal_focus = cx.update(|cx| cx.focus_handle());

    window
        .update(cx, |_, window, cx| terminal_focus.focus(window, cx))
        .unwrap();
    window
        .update(cx, |app_window, _, cx| {
            app_window.navigate("/ko/settings", cx)
        })
        .unwrap();
    cx.run_until_parked();

    let is_terminal_focused = window
        .update(cx, |_, window, _| terminal_focus.is_focused(window))
        .unwrap();
    assert!(!is_terminal_focused);
}

#[test]
fn service_errors_keep_the_ts_ipc_prefix() {
    assert_eq!(
        format_service_error("database is locked"),
        "Error invoking remote method 'kanvibe:invoke': Error: database is locked"
    );
}

/// 상태 변경 알림이 런타임 저장소에 남긴 본문
fn stored_status_bodies(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| {
        let runtime = cx.global::<AppServices>().runtime().unwrap().clone();
        runtime
            .notification_store()
            .list_notifications()
            .unwrap()
            .into_iter()
            .map(|notification| notification.body)
            .collect()
    })
}

#[gpui::test]
fn notification_text_follows_the_locale_of_the_first_window(cx: &mut TestAppContext) {
    let data_dir = tempfile::tempdir().unwrap();
    init_app(cx, data_dir.path().to_path_buf());
    cx.update(|cx| {
        let runtime = cx.global::<AppServices>().runtime().unwrap().clone();
        runtime.start();
    });
    let _window = cx.add_window(|window, cx| AppWindow::new("/en", window, cx));
    cx.run_until_parked();

    emit_board_event(
        cx,
        BoardEvent::TaskStatusChanged(kanvibe_services::board_notifier::TaskStatusChangedPayload {
            project_name: "sample-app".into(),
            branch_name: "feat/notify".into(),
            task_title: "Locale".into(),
            description: None,
            new_status: "review".into(),
            task_id: "task-1".into(),
        }),
    );

    assert_eq!(stored_status_bodies(cx), ["Locale: changed to review"]);
}
