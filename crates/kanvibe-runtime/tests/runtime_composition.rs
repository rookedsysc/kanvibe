//! 런타임이 장기 서비스를 하나씩만 만들고, QA 입력(고정 시각·네트워크 표)을 실제 어댑터 대신 쓰며,
//! 프로세스 환경을 바꾸지 않는지 확인한다.

// 공용 격리 환경 가운데 이 파일이 쓰지 않는 도우미(git·tmux 작업 생성)는 runtime_wiring이 쓴다
#[allow(dead_code)]
mod support;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use kanvibe_ai::usage::context::HttpRequest;
use kanvibe_diagram::service::DiagramRenderer;
use kanvibe_domain::notifications::AppNotification;
use kanvibe_runtime::{AppClock, DesktopRuntime, NetworkResponseTable};
use kanvibe_services::board_notifier::{BoardEvent, TaskStatusChangedPayload};
use kanvibe_services::desktop_notification::{
    DesktopNotificationPresenter, NotificationClick, NotificationDeliveryCallbacks,
};
use serde_json::json;

use support::{RuntimeFixture, assert_runtime_values_absent};

/// 띄운 OS 알림 제목을 모은다
#[derive(Default)]
struct RecordingPresenter {
    shown_titles: Mutex<Vec<String>>,
}

impl DesktopNotificationPresenter for RecordingPresenter {
    fn is_supported(&self) -> bool {
        true
    }

    fn show(&self, notification: &AppNotification, _on_click: NotificationClick) {
        self.shown_titles
            .lock()
            .unwrap()
            .push(notification.title.clone());
    }
}

fn review_status_event() -> BoardEvent {
    BoardEvent::TaskStatusChanged(TaskStatusChangedPayload {
        project_name: "sample-app".into(),
        branch_name: "feat/notify".into(),
        task_title: "알림 확인".into(),
        description: None,
        new_status: "review".into(),
        task_id: "task-1".into(),
    })
}

#[test]
fn accessors_hand_out_the_same_long_lived_instances() {
    let fixture = RuntimeFixture::new();
    let runtime = fixture.open_runtime();

    assert!(Arc::ptr_eq(runtime.kanban(), runtime.kanban()));
    assert!(Arc::ptr_eq(runtime.pane_chat(), runtime.pane_chat()));
    assert!(Arc::ptr_eq(runtime.pane_mirror(), runtime.pane_mirror()));
    assert!(Arc::ptr_eq(
        runtime.diagram_renderer(),
        runtime.diagram_renderer()
    ));
    assert!(Arc::ptr_eq(
        runtime.desktop_notifications(),
        runtime.desktop_notifications()
    ));
    assert!(fixture.path("data/kanvibe.db").exists());
}

#[test]
fn starting_twice_delivers_each_board_event_once_through_the_shared_bus() {
    let fixture = RuntimeFixture::new();
    let presenter = Arc::new(RecordingPresenter::default());
    let changed_count = Arc::new(AtomicUsize::new(0));
    let mut config = fixture.config();
    config.notification_presenter = presenter.clone();
    let counter = changed_count.clone();
    config.notification_callbacks = NotificationDeliveryCallbacks {
        on_notifications_changed: Some(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        })),
        on_notification_click: None,
    };
    let runtime = DesktopRuntime::new(config).unwrap();

    runtime.start();
    runtime.start();
    runtime.board_notifier().emit(review_status_event());
    runtime.shutdown();
    runtime.board_notifier().emit(review_status_event());

    assert_eq!(presenter.shown_titles.lock().unwrap().len(), 1);
    assert_eq!(changed_count.load(Ordering::SeqCst), 1);
    let stored = runtime.notification_store().list_notifications().unwrap();
    assert_eq!(stored.len(), 1);
}

#[test]
fn qa_inputs_replace_the_network_and_the_clock() {
    let fixture = RuntimeFixture::new();
    let mut config = fixture.config();
    let table: NetworkResponseTable = serde_json::from_value(json!({"responses": [{
        "urlPrefix": "https://api.github.com/repos/rookedsysc/kanvibe/releases",
        "status": 200,
        "body": [{"tag_name": "v99.0.0", "name": "v99", "body": "", "html_url": "https://example.invalid/r"}]
    }]}))
    .unwrap();
    config.qa_network_responses = Some(Arc::new(table));
    config.clock = AppClock::fixed(1_768_446_000_000);
    let runtime = DesktopRuntime::new(config).unwrap();

    let release = runtime.release_update().check_for_release_update();
    let blocked = runtime.ai_usage_context().http.send(&HttpRequest {
        method: "GET".into(),
        url: "https://api.anthropic.com/api/oauth/usage".into(),
        headers: Vec::new(),
        body: None,
        timeout_ms: 1000,
    });

    assert!(release.is_update_available, "{release:?}");
    assert!(blocked.is_err());
    let usage_now = runtime.ai_usage_context().clock.now_ms();
    assert!((1_768_446_000_000.0..1_768_446_060_000.0).contains(&usage_now));
}

#[test]
fn diagram_renderer_reads_fonts_only_when_first_used() {
    let fixture = RuntimeFixture::new();
    let read_count = Arc::new(AtomicUsize::new(0));
    let mut config = fixture.config();
    let counter = read_count.clone();
    config.diagram_font_files = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        bundled_app_fonts()
    });
    let runtime = DesktopRuntime::new(config).unwrap();
    assert_eq!(read_count.load(Ordering::SeqCst), 0);

    let request = json!({"definition": "flowchart TD\n  A --> B", "theme": "dark", "width": 480});
    let first = runtime.diagram_renderer().render_mobile_diagram(&request);
    let second = runtime.diagram_renderer().render_mobile_diagram(&request);

    assert!(first.is_ok(), "{:?}", first.err());
    assert!(second.is_ok());
    assert_eq!(read_count.load(Ordering::SeqCst), 1);
}

#[test]
fn diagram_renderer_without_fonts_fails_with_the_contract_error() {
    let fixture = RuntimeFixture::new();
    let runtime = fixture.open_runtime();

    let failure = runtime
        .diagram_renderer()
        .render_mobile_diagram(
            &json!({"definition": "flowchart TD\n  A --> B", "theme": "dark", "width": 480}),
        )
        .unwrap_err();

    assert_eq!(failure.status_code, 500);
}

#[test]
fn building_and_running_the_runtime_leaves_the_process_environment_alone() {
    let fixture = RuntimeFixture::new();
    let before: BTreeMap<String, String> = std::env::vars().collect();

    let runtime = fixture.open_runtime();
    runtime.start();
    runtime.shutdown();
    drop(runtime);

    let after: BTreeMap<String, String> = std::env::vars().collect();
    assert_eq!(before, after);
}

#[test]
fn ai_account_commands_get_the_narrowed_environment() {
    let fixture = RuntimeFixture::new();
    let runtime = fixture.open_runtime();

    let keys: Vec<String> = runtime
        .ai_usage_context()
        .process_variables
        .keys()
        .cloned()
        .collect();

    assert_runtime_values_absent(&keys, "AI 계정 CLI 환경");
}

fn bundled_app_fonts() -> Vec<Vec<u8>> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../kanvibe-app/assets/fonts");
    std::fs::read_dir(directory)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "ttf" || extension == "otf")
        })
        .map(|path| std::fs::read(path).unwrap())
        .collect()
}
