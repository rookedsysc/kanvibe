//! 작업 상세 화면(TS `TaskDetailRoute`·`TerminalLoader`·`TerminalTabBar`·`useTerminalTabs`·`AutoConnectTerminalSession`)을
//! 실제 DB와 함께 헤드리스 GPUI로 띄워, 보이는 글자·창 상태·열린 주소·확인 대화상자로 TS 동작을 확인한다.
//! 바깥 프로세스(tmux·gh·VS Code)는 부르지 않도록 TS IPC 이름의 QA 고장 주입으로 응답을 정한다.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::gpui;
use gpui_kit::{
    Bounds, Entity, Focusable, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels,
    TestAppContext, VisualTestContext, WeakEntity, WindowHandle,
};
use kanvibe_app::app_events::{RefreshScope, trigger_refresh};
use kanvibe_app::app_window::AppWindow;
use kanvibe_app::qa::QaRuntime;
use kanvibe_app::qa::faults::{FaultMode, ServiceFault};
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions, QaOptions};
use kanvibe_app::qa::targets::{DescribedTarget, QaTargetRegistry, TargetQuery};
use kanvibe_app::routes::{RouteResolution, resolve_route_path};
use kanvibe_app::services::AppServices;
use kanvibe_app::shortcuts::{
    OpenTaskDetailDockItem, RunTerminalTabCommand, ShortcutState, ToggleNotificationCenter,
    ToggleTerminalChat,
};
use kanvibe_app::views::task_detail::TaskDetailView;
use kanvibe_app::views::{ScreenContext, WindowNavigator};
use kanvibe_db::Database;
use kanvibe_db::repositories::app_settings::save_app_setting;
use kanvibe_db::repositories::projects::{NewProject, insert_project};
use kanvibe_db::repositories::tasks::{NewKanbanTask, insert_task, update_task};
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskStatus};
use kanvibe_domain::terminal_tabs::TerminalTabShortcutCommand;
use serde_json::{Value, json};

const CONNECTING: &str = "터미널에 연결하고 있습니다…";
const INFO_PANEL: &str = "작업 정보";
const BACK_TO_TERMINAL: &str = "← 터미널";

fn init_app(cx: &mut TestAppContext, data_dir: PathBuf) -> Arc<Database> {
    let launch = LaunchOptions {
        data_dir: Some(data_dir),
        initial_route: "/ko".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: Some(QaOptions {
            fixture_root: None,
            fixed_now_ms: 1_768_445_820_000,
            network_responses_path: None,
            faults_path: None,
            control_socket_path: None,
            platform_override: None,
            loading_holds: Default::default(),
        }),
    };
    let database = cx.update(|cx| {
        kanvibe_app::init_app(&launch, cx).expect("app init");
        cx.global_mut::<ShortcutState>().platform = ShortcutPlatform::Linux;
        if !cx.has_global::<QaTargetRegistry>() {
            cx.set_global(QaTargetRegistry::default());
        }
        cx.global::<AppServices>()
            .database()
            .expect("database")
            .clone()
    });
    save_app_setting(&database.connection(), "sidebar_default_collapsed", "true").unwrap();
    save_app_setting(&database.connection(), "default_session_type", "tmux").unwrap();
    // 터미널·PR 조회가 바깥 프로세스를 부르지 않게 기본 응답을 건다
    add_fault(cx, result("kanvibe:terminal-open", json!({"ok": true})));
    add_fault(cx, result("kanban.fetchAndSavePrUrl", Value::Null));
    database
}

/// 확인·알림 대화상자를 실제로 띄우고 창 활성 상태를 그대로 쓴다(QA 자동 수락을 끈다)
fn use_real_prompts_and_focus(cx: &mut TestAppContext) {
    cx.update(|cx| {
        if cx.has_global::<QaRuntime>() {
            cx.remove_global::<QaRuntime>();
        }
    });
}

fn fault(target: &str, mode: FaultMode) -> ServiceFault {
    ServiceFault {
        target: target.into(),
        mode,
        message: None,
        ms: None,
        value: None,
        after_calls: None,
        argument_match: None,
    }
}

fn result(target: &str, value: Value) -> ServiceFault {
    ServiceFault {
        value: Some(value),
        ..fault(target, FaultMode::Result)
    }
}

fn failure(target: &str, message: &str) -> ServiceFault {
    ServiceFault {
        message: Some(message.into()),
        ..fault(target, FaultMode::Fail)
    }
}

fn delay(target: &str, ms: u64) -> ServiceFault {
    ServiceFault {
        ms: Some(ms),
        ..fault(target, FaultMode::Delay)
    }
}

fn after(calls: u32, fault: ServiceFault) -> ServiceFault {
    ServiceFault {
        after_calls: Some(calls),
        ..fault
    }
}

fn matching(needle: &str, fault: ServiceFault) -> ServiceFault {
    ServiceFault {
        argument_match: Some(needle.into()),
        ..fault
    }
}

/// 먼저 건 고장이 이긴다. 기본 응답을 바꾸려면 지우고 다시 건다
fn add_fault(cx: &mut TestAppContext, fault: ServiceFault) {
    cx.update(|cx| cx.global::<AppServices>().faults().unwrap().add(fault));
}

fn clear_fault(cx: &mut TestAppContext, target: &str) {
    cx.update(|cx| cx.global::<AppServices>().faults().unwrap().clear(target));
}

fn tabs(entries: &[(&str, bool)]) -> Value {
    let tabs: Vec<Value> = entries
        .iter()
        .enumerate()
        .map(|(index, (id, is_active))| {
            json!({"id": id, "nativeIndex": index, "name": id, "isActive": is_active})
        })
        .collect();
    json!({"ok": true, "tabs": tabs})
}

fn project(database: &Database) -> String {
    insert_project(
        &database.connection(),
        &NewProject {
            name: "kanvibe".into(),
            repo_path: "/repos/kanvibe".into(),
            default_branch: "main".into(),
            ssh_host: None,
            is_worktree: false,
            color: Some("#86EFAC".into()),
            icon_data_url: None,
        },
    )
    .unwrap()
    .id
}

fn task(database: &Database, title: &str, edit: impl FnOnce(&mut NewKanbanTask)) -> KanbanTask {
    let mut new_task = NewKanbanTask {
        title: title.into(),
        description: None,
        status: TaskStatus::Todo,
        branch_name: None,
        worktree_path: None,
        session_type: Some(SessionType::Tmux),
        session_name: Some("task-session".into()),
        ssh_host: None,
        agent_type: None,
        project_id: None,
        base_branch: None,
        pr_url: None,
        priority: None,
    };
    edit(&mut new_task);
    insert_task(&database.connection(), &new_task).unwrap()
}

fn no_session(new_task: &mut NewKanbanTask) {
    new_task.session_type = None;
    new_task.session_name = None;
}

struct Detail {
    view: WeakEntity<TaskDetailView>,
    cx: VisualTestContext,
}

fn open_detail(cx: &mut TestAppContext, task_id: &str) -> Detail {
    let RouteResolution::Render(route) = resolve_route_path(&format!("/ko/task/{task_id}")) else {
        panic!("task route must render");
    };
    let (view, visual) = cx.add_window_view(|_, cx| {
        TaskDetailView::new(
            ScreenContext {
                route,
                navigator: WindowNavigator::new(WeakEntity::new_invalid()),
            },
            cx,
        )
    });
    let mut detail = Detail {
        view: view.downgrade(),
        cx: visual.clone(),
    };
    drop(view);
    // 앱과 QA 재생은 연 창을 앞으로 가져온다(`activate_window`). 창 활성에 묶인 조회·읽음 처리가 그 상태에서 돈다
    detail.cx.update(|window, _| window.activate_window());
    detail.draw();
    detail
}

impl Detail {
    /// 창이 닫혔으면 그리지 않는다
    fn draw(&mut self) {
        self.cx.run_until_parked();
        if self.view.upgrade().is_none() {
            return;
        }
        self.cx.update(|window, cx| {
            let window_id = window.window_handle().window_id();
            cx.global_mut::<QaTargetRegistry>().begin_frame(window_id);
            window.refresh();
        });
        self.cx.run_until_parked();
    }

    fn advance(&mut self, duration: Duration) {
        self.cx.executor().advance_clock(duration);
        self.draw();
    }

    fn entity(&self) -> Entity<TaskDetailView> {
        self.view.upgrade().expect("detail view alive")
    }

    fn is_open(&mut self) -> bool {
        self.view.upgrade().is_some()
    }

    fn find(&mut self, query: TargetQuery) -> Option<Bounds<Pixels>> {
        self.cx.update(|window, cx| {
            cx.global::<QaTargetRegistry>()
                .find(window.window_handle().window_id(), &query)
        })
    }

    fn has(&mut self, role: &str, name: &str) -> bool {
        self.find(described(role, name, true)).is_some()
    }

    fn has_containing(&mut self, role: &str, text: &str) -> bool {
        self.find(described(role, text, false)).is_some()
    }

    fn click(&mut self, role: &str, name: &str) {
        let bounds = self
            .find(described(role, name, true))
            .unwrap_or_else(|| panic!("{role} {name} not found"));
        self.cx
            .simulate_click(bounds.center(), Modifiers::default());
        self.draw();
    }

    fn double_click(&mut self, role: &str, name: &str) {
        let position = self
            .find(described(role, name, true))
            .unwrap_or_else(|| panic!("{role} {name} not found"))
            .center();
        for click_count in [1, 2] {
            self.cx.simulate_event(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count,
                first_mouse: false,
            });
            self.cx.simulate_event(MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count,
            });
        }
        self.draw();
    }

    fn keys(&mut self, keystrokes: &str) {
        self.cx.simulate_keystrokes(keystrokes);
        self.draw();
    }

    fn dispatch(&mut self, action: impl gpui_kit::Action) {
        if !self.is_open() {
            return;
        }
        self.cx.dispatch_action(action);
        self.draw();
    }

    fn is_terminal_focused(&mut self) -> bool {
        let view = self.entity();
        self.cx.update(|window, cx| {
            let focused = window.focused(cx);
            let own_handle = view.read(cx).focus_handle(cx);
            focused
                .is_some_and(|handle| handle != own_handle && own_handle.contains(&handle, window))
        })
    }

    fn pending_prompt(&mut self) -> Option<String> {
        self.cx
            .update(|_, cx| cx.has_global::<AppServices>())
            .then(|| self.cx.pending_prompt().map(|(message, _)| message))
            .flatten()
    }

    fn answer_prompt(&mut self, button: &str) {
        self.cx.simulate_prompt_answer(button);
        self.draw();
    }
}

fn described(role: &str, name: &str, exact: bool) -> TargetQuery {
    TargetQuery::Described(DescribedTarget {
        role: Some(role.into()),
        name: Some(name.into()),
        exact: Some(exact),
        ..DescribedTarget::default()
    })
}

fn store_notification(database: &Database, task_id: &str) {
    let notifications = json!([{
        "id": "notification-1", "title": "알림 제목", "body": "Review 상태로 바뀌었습니다",
        "taskId": task_id, "relativePath": format!("/ko/task/{task_id}"), "locale": "ko",
        "isRead": false, "createdAt": "2026-01-15T02:57:00.000Z", "dedupeKey": "dedupe-1"
    }]);
    save_app_setting(
        &database.connection(),
        "app_notifications",
        &notifications.to_string(),
    )
    .unwrap();
}

fn is_notification_read(cx: &mut TestAppContext) -> bool {
    cx.update(|cx| {
        let runtime = cx.global::<AppServices>().runtime().unwrap().clone();
        runtime.notification_store().list_notifications().unwrap()[0].is_read
    })
}

fn refresh(detail: &mut Detail) {
    detail
        .cx
        .update(|_, cx| trigger_refresh(RefreshScope::All, cx));
    detail.draw();
}

// ───────────── 첫 조회·새로 고침 ─────────────

#[gpui::test]
fn refresh_replaces_the_shown_task_with_the_latest_data(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let mut stored = task(&database, "cached task title", |_| {});
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.has_containing("group", "cached task title"));
    stored.title = "fresh task title".into();
    update_task(&database.connection(), &stored).unwrap();
    refresh(&mut detail);
    assert!(detail.has_containing("group", "fresh task title"));
}

#[gpui::test]
fn another_task_never_shows_the_previous_task(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let task_a = task(&database, "Task A", |_| {});
    let task_b = task(&database, "Task B", |_| {});
    let mut first = open_detail(cx, &task_a.id);
    assert!(first.has_containing("group", "Task A"));
    add_fault(
        cx,
        matching(&task_b.id, failure("kanban.getTaskById", "B unavailable")),
    );
    let mut second = open_detail(cx, &task_b.id);
    assert!(!second.has_containing("group", "Task A"));
    assert!(second.has_containing("alert", "B unavailable"));
}

#[gpui::test]
fn a_loading_task_shows_loading_instead_of_another_task(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let other = task(&database, "Foreign cache", |_| {});
    let target = task(&database, "target", |_| {});
    open_detail(cx, &other.id);
    add_fault(
        cx,
        matching(&target.id, fault("kanban.getTaskById", FaultMode::Hang)),
    );
    let mut detail = open_detail(cx, &target.id);
    assert!(detail.has("status", "Loading..."));
    assert!(!detail.has_containing("group", "Foreign cache"));
}

#[gpui::test]
fn a_slow_first_load_shows_the_delay_notice_not_task_missing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "slow", |_| {});
    add_fault(cx, fault("kanban.getTaskById", FaultMode::Hang));
    let mut detail = open_detail(cx, &stored.id);
    detail.advance(Duration::from_secs(5));
    assert!(detail.has("status", "조회가 지연되고 있습니다. 계속 기다리는 중…"));
    assert!(!detail.has("status", "작업을 찾을 수 없습니다."));
}

#[gpui::test]
fn a_failed_first_load_shows_the_reason_and_retry_loads_again(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    add_fault(cx, failure("kanban.getTaskById", "db locked"));
    let mut detail = open_detail(cx, "missing-task");
    assert!(detail.has_containing("alert", "db locked"));
    assert!(!detail.has("status", "작업을 찾을 수 없습니다."));
    clear_fault(cx, "kanban.getTaskById");
    detail.click("button", "다시 시도");
    assert!(detail.has("status", "작업을 찾을 수 없습니다."));
    assert!(!detail.has_containing("alert", "db locked"));
}

#[gpui::test]
fn a_failed_refresh_keeps_the_task_and_shows_the_banner(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "cached task title", |_| {});
    let mut detail = open_detail(cx, &stored.id);
    add_fault(cx, failure("kanban.getTaskById", "ipc closed"));
    refresh(&mut detail);
    assert!(detail.has_containing(
        "alert",
        "최신 데이터를 불러오지 못해 이전 데이터를 표시합니다"
    ));
    assert!(detail.has_containing("group", "cached task title"));
    assert!(!detail.has("status", "작업을 찾을 수 없습니다."));
}

#[gpui::test]
fn closing_the_info_panel_does_not_reload_the_task(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    let mut detail = open_detail(cx, &stored.id);
    add_fault(cx, failure("kanban.getTaskById", "reloaded"));
    detail.keys("ctrl-1");
    assert!(detail.has("heading", INFO_PANEL));
    detail.keys("ctrl-1");
    assert!(!detail.has("heading", INFO_PANEL));
    assert!(!detail.has_containing("alert", "reloaded"));
}

// ───────────── 알림 읽음 ─────────────

#[gpui::test]
fn opening_the_task_in_an_active_window_marks_its_notifications_read(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    store_notification(&database, &stored.id);
    open_detail(cx, &stored.id);
    assert!(is_notification_read(cx));
}

#[gpui::test]
fn an_inactive_window_does_not_mark_notifications_even_after_refresh(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    use_real_prompts_and_focus(cx);
    let stored = task(&database, "task title", |_| {});
    store_notification(&database, &stored.id);
    add_fault(cx, delay("kanban.getTaskById", 100));
    let mut detail = open_detail(cx, &stored.id);
    detail.cx.deactivate_window();
    detail.advance(Duration::from_millis(100));
    refresh(&mut detail);
    detail.advance(Duration::from_millis(100));
    assert!(detail.has_containing("group", "task title"));
    assert!(!is_notification_read(cx));
}

#[gpui::test]
fn an_inactive_window_marks_notifications_when_it_gets_focus(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    use_real_prompts_and_focus(cx);
    let stored = task(&database, "task title", |_| {});
    store_notification(&database, &stored.id);
    add_fault(cx, delay("kanban.getTaskById", 100));
    let mut detail = open_detail(cx, &stored.id);
    detail.cx.deactivate_window();
    detail.advance(Duration::from_millis(100));
    assert!(!is_notification_read(cx));
    detail.cx.update(|window, _| window.activate_window());
    detail.draw();
    assert!(is_notification_read(cx));
}

/// 작업을 읽기 전에 창을 뒤로 보낸 상세 화면. 작업은 100ms 뒤에 읽힌다
fn detail_loaded_in_an_inactive_window(cx: &mut TestAppContext) -> Detail {
    let database = init_app(cx, target_temp_dir().keep());
    use_real_prompts_and_focus(cx);
    let stored = task(&database, "task title", |_| {});
    store_notification(&database, &stored.id);
    add_fault(cx, delay("kanban.getTaskById", 100));
    let mut detail = open_detail(cx, &stored.id);
    detail.cx.deactivate_window();
    detail.advance(Duration::from_millis(100));
    detail
}

#[gpui::test]
fn an_open_detail_in_an_inactive_window_leaves_notifications_unread(cx: &mut TestAppContext) {
    let mut detail = detail_loaded_in_an_inactive_window(cx);
    assert!(detail.has_containing("group", "task title"));
    assert!(!is_notification_read(cx));
}

#[gpui::test]
fn redrawing_an_inactive_detail_leaves_notifications_unread(cx: &mut TestAppContext) {
    let mut detail = detail_loaded_in_an_inactive_window(cx);
    for _ in 0..3 {
        detail.draw();
    }
    assert!(!is_notification_read(cx));
}

#[gpui::test]
fn notifications_wait_until_the_task_is_confirmed(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    store_notification(&database, &stored.id);
    add_fault(cx, delay("kanban.getTaskById", 100));
    let mut detail = open_detail(cx, &stored.id);
    assert!(!is_notification_read(cx));
    detail.advance(Duration::from_millis(100));
    assert!(is_notification_read(cx));
}

#[gpui::test]
fn a_failed_mark_read_keeps_the_detail_screen(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    store_notification(&database, &stored.id);
    add_fault(
        cx,
        failure(
            "kanvibe:notifications-mark-task-read",
            "app settings write failed",
        ),
    );
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.has_containing("group", "task title"));
    assert!(!is_notification_read(cx));
}

#[gpui::test]
fn a_missing_task_does_not_mark_notifications(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    store_notification(&database, "missing-task");
    let mut detail = open_detail(cx, "missing-task");
    assert!(detail.has("status", "작업을 찾을 수 없습니다."));
    assert!(!is_notification_read(cx));
}

// ───────────── dock: PR·VS Code·단축키 ─────────────

#[gpui::test]
fn a_task_with_a_pr_url_shows_a_pr_link_in_the_dock(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |new_task| {
        new_task.pr_url = Some("https://github.com/kanvibe/kanvibe/pull/236".into());
    });
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.has("link", "PR"));
    assert!(!detail.has("button", "PR"));
}

#[gpui::test]
fn dock_shortcuts_win_over_the_focused_terminal(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.is_terminal_focused());
    detail.keys("ctrl-3");
    assert!(detail.has("button", BACK_TO_TERMINAL));
}

#[gpui::test]
fn dock_slot_four_opens_the_pr_in_the_browser(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let pr_url = "https://github.com/kanvibe/kanvibe/pull/236";
    let stored = task(&database, "task title", |new_task| {
        new_task.pr_url = Some(pr_url.into());
    });
    let mut detail = open_detail(cx, &stored.id);
    detail.keys("ctrl-4");
    assert_eq!(detail.cx.opened_url().as_deref(), Some(pr_url));
}

#[gpui::test]
fn a_task_without_a_pr_keeps_slot_four_and_looks_it_up_again(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    use_real_prompts_and_focus(cx);
    let stored = task(&database, "task title", |new_task| {
        new_task.branch_name = Some("feat/detail-shortcut".into());
    });
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.has("button", "PR"));
    detail.keys("ctrl-4");
    assert_eq!(
        detail.pending_prompt().as_deref(),
        Some("이 브랜치에 연결된 PR을 찾지 못했습니다.")
    );
}

#[gpui::test]
fn a_failed_pr_lookup_is_reported_as_a_lookup_failure(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    use_real_prompts_and_focus(cx);
    let stored = task(&database, "task title", |new_task| {
        new_task.branch_name = Some("feat/detail-shortcut".into());
    });
    clear_fault(cx, "kanban.fetchAndSavePrUrl");
    add_fault(
        cx,
        matching(
            &format!("[\"{}\"]", stored.id),
            failure("kanban.fetchAndSavePrUrl", "gh auth failed"),
        ),
    );
    add_fault(cx, result("kanban.fetchAndSavePrUrl", Value::Null));
    let mut detail = open_detail(cx, &stored.id);
    detail.keys("ctrl-4");
    let message = detail.pending_prompt().unwrap_or_default();
    assert!(
        message.starts_with("PR을 조회하지 못했습니다:"),
        "{message}"
    );
    assert!(message.contains("gh auth failed"), "{message}");
}

#[gpui::test]
fn pressing_pr_again_while_looking_up_starts_no_second_lookup(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    use_real_prompts_and_focus(cx);
    let stored = task(&database, "task title", |_| {});
    clear_fault(cx, "kanban.fetchAndSavePrUrl");
    add_fault(
        cx,
        after(1, failure("kanban.fetchAndSavePrUrl", "second lookup")),
    );
    add_fault(cx, delay("kanban.fetchAndSavePrUrl", 1_000));
    let mut detail = open_detail(cx, &stored.id);
    detail.keys("ctrl-4");
    detail.keys("ctrl-4");
    assert_eq!(detail.pending_prompt(), None);
    detail.advance(Duration::from_millis(1_000));
    assert_eq!(
        detail.pending_prompt().as_deref(),
        Some("이 브랜치에 연결된 PR을 찾지 못했습니다.")
    );
    detail.answer_prompt("OK");
    assert_eq!(detail.pending_prompt(), None);
}

// ───────────── 원격 PR 조회 전 gh 설치 확인 ─────────────

const GH_STATUS: &str = "githubCliDependency.getGitHubCliStatus";
const GH_INSTALL: &str = "githubCliDependency.installGitHubCli";
const FOUND_PR: &str = "https://github.com/kanvibe/kanvibe/pull/165";

fn gh_status(is_available: bool) -> Value {
    json!({
        "toolName": "gh", "sshHost": "remote-a", "isRemote": true,
        "available": is_available, "blockedReason": null
    })
}

/// 브랜치만 있고 PR 링크가 없는 작업. 조회하면 `FOUND_PR`을 찾는다
fn pr_lookup_task(
    cx: &mut TestAppContext,
    database: &Database,
    ssh_host: Option<&str>,
) -> KanbanTask {
    use_real_prompts_and_focus(cx);
    clear_fault(cx, "kanban.fetchAndSavePrUrl");
    add_fault(cx, result("kanban.fetchAndSavePrUrl", json!(FOUND_PR)));
    task(database, "task title", |new_task| {
        new_task.branch_name = Some("fix/wtf".into());
        new_task.ssh_host = ssh_host.map(str::to_string);
    })
}

#[gpui::test]
fn declining_the_remote_gh_install_stops_the_pr_lookup(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = pr_lookup_task(cx, &database, Some("remote-a"));
    add_fault(cx, result(GH_STATUS, gh_status(false)));
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.pending_prompt().unwrap_or_default().contains("gh"));
    detail.answer_prompt("Cancel");
    assert!(!detail.has("link", "PR"));
    assert!(detail.has("button", "PR"));
}

#[gpui::test]
fn a_ready_remote_gh_continues_the_pr_lookup(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = pr_lookup_task(cx, &database, Some("remote-a"));
    add_fault(cx, result(GH_STATUS, gh_status(false)));
    add_fault(cx, result(GH_INSTALL, gh_status(true)));
    let mut detail = open_detail(cx, &stored.id);
    detail.answer_prompt("OK");
    assert!(detail.has("link", "PR"));
}

#[gpui::test]
fn a_local_task_looks_up_the_pr_without_asking(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = pr_lookup_task(cx, &database, None);
    add_fault(cx, failure(GH_STATUS, "gh status was read"));
    let mut detail = open_detail(cx, &stored.id);
    assert_eq!(detail.pending_prompt(), None);
    assert!(detail.has("link", "PR"));
}

#[gpui::test]
fn an_installed_remote_gh_passes_without_a_prompt(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = pr_lookup_task(cx, &database, Some("remote-a"));
    add_fault(cx, result(GH_STATUS, gh_status(true)));
    add_fault(cx, failure(GH_INSTALL, "gh was installed"));
    let mut detail = open_detail(cx, &stored.id);
    assert_eq!(detail.pending_prompt(), None);
    assert!(detail.has("link", "PR"));
}

#[gpui::test]
fn cancelling_the_gh_install_prompt_installs_nothing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = pr_lookup_task(cx, &database, Some("remote-a"));
    add_fault(cx, result(GH_STATUS, gh_status(false)));
    add_fault(cx, failure(GH_INSTALL, "gh was installed"));
    let mut detail = open_detail(cx, &stored.id);
    detail.answer_prompt("Cancel");
    // 직접 누른 조회는 실패를 알림으로 보인다. 설치가 돌았다면 조회 실패 알림이, 아니면 PR 없음 알림이 뜬다
    detail.keys("ctrl-4");
    assert!(detail.pending_prompt().unwrap_or_default().contains("gh"));
    detail.answer_prompt("Cancel");
    assert_eq!(
        detail.pending_prompt().as_deref(),
        Some("이 브랜치에 연결된 PR을 찾지 못했습니다.")
    );
    assert!(!detail.has("link", "PR"));
}

#[gpui::test]
fn confirming_the_gh_install_installs_on_that_host_then_passes(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = pr_lookup_task(cx, &database, Some("remote-a"));
    add_fault(cx, result(GH_STATUS, gh_status(false)));
    add_fault(
        cx,
        matching("[\"remote-a\"]", result(GH_INSTALL, gh_status(true))),
    );
    add_fault(cx, failure(GH_INSTALL, "installed on another host"));
    let mut detail = open_detail(cx, &stored.id);
    detail.answer_prompt("OK");
    assert!(detail.has("link", "PR"));
}

#[gpui::test]
fn dock_slot_six_opens_the_task_folder_in_vs_code(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    use_real_prompts_and_focus(cx);
    let stored = task(&database, "task title", |_| {});
    add_fault(
        cx,
        matching(
            &stored.id,
            result("editor.openTaskInVsCode", json!({"ok": true})),
        ),
    );
    add_fault(cx, failure("editor.openTaskInVsCode", "another task"));
    let mut detail = open_detail(cx, &stored.id);
    detail.keys("ctrl-6");
    assert_eq!(detail.pending_prompt(), None);
}

#[gpui::test]
fn a_thrown_vs_code_request_shows_the_failure_alert(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    use_real_prompts_and_focus(cx);
    let stored = task(&database, "task title", |_| {});
    add_fault(cx, failure("editor.openTaskInVsCode", "db down"));
    let mut detail = open_detail(cx, &stored.id);
    detail.keys("ctrl-6");
    assert_eq!(
        detail.pending_prompt().as_deref(),
        Some("VS Code를 열지 못했습니다. `code` 명령을 PATH에서 찾을 수 있는지 확인하세요.")
    );
}

#[gpui::test]
fn a_dock_command_sent_by_the_app_runs_the_same_dock_action(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    let mut detail = open_detail(cx, &stored.id);
    detail.dispatch(OpenTaskDetailDockItem { position: 3 });
    assert!(detail.has("button", BACK_TO_TERMINAL));
}

#[gpui::test]
fn dock_commands_arriving_right_after_the_load_use_the_loaded_dock(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    add_fault(cx, delay("kanban.getTaskById", 100));
    let mut detail = open_detail(cx, &stored.id);
    detail.dispatch(OpenTaskDetailDockItem { position: 1 });
    assert!(!detail.has("heading", INFO_PANEL));
    detail.advance(Duration::from_millis(100));
    detail.dispatch(OpenTaskDetailDockItem { position: 1 });
    assert!(detail.has("heading", INFO_PANEL));
}

#[gpui::test]
fn the_pr_is_looked_up_automatically_on_load_and_on_refresh(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |new_task| {
        new_task.branch_name = Some("feat/auto-pr".into());
    });
    clear_fault(cx, "kanban.fetchAndSavePrUrl");
    add_fault(
        cx,
        matching(
            "\"isAutomatic\":true",
            result(
                "kanban.fetchAndSavePrUrl",
                json!("https://github.com/k/k/pull/9"),
            ),
        ),
    );
    add_fault(cx, failure("kanban.fetchAndSavePrUrl", "manual lookup"));
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.has("link", "PR"));
    refresh(&mut detail);
    assert!(detail.has("link", "PR"));
}

#[gpui::test]
fn the_dock_rail_has_no_notification_button(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    let mut detail = open_detail(cx, &stored.id);
    let bell = detail
        .find(described("button", "알림", true))
        .expect("chrome bell");
    assert!(
        f32::from(bell.origin.x) > 200.0,
        "the only bell sits in the terminal chrome"
    );
    let second = TargetQuery::Described(DescribedTarget {
        role: Some("button".into()),
        name: Some("알림".into()),
        exact: Some(true),
        nth: Some(1),
        ..DescribedTarget::default()
    });
    assert!(detail.find(second).is_none());
}

#[gpui::test]
fn pressing_the_terminal_closes_an_open_drawer(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    let mut detail = open_detail(cx, &stored.id);
    detail.keys("ctrl-1");
    assert!(detail.has("heading", INFO_PANEL));
    let terminal_point = detail.cx.update(|window, _| {
        let size = window.viewport_size();
        gpui_kit::point(size.width - gpui_kit::px(100.0), size.height / 2.0)
    });
    detail
        .cx
        .simulate_click(terminal_point, Modifiers::default());
    detail.draw();
    assert!(!detail.has("heading", INFO_PANEL));
}

#[gpui::test]
fn closing_a_tab_without_a_terminal_closes_the_window(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "no terminal", no_session);
    let mut detail = open_detail(cx, &stored.id);
    detail.dispatch(RunTerminalTabCommand {
        command: TerminalTabShortcutCommand::CloseTab,
    });
    assert!(!detail.is_open());
}

// ───────────── 포커스·알림 센터 ─────────────

#[gpui::test]
fn entering_the_detail_focuses_the_terminal_input(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.is_terminal_focused());
}

#[gpui::test]
fn closing_a_panel_returns_input_focus_to_the_terminal(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    let mut detail = open_detail(cx, &stored.id);
    detail.keys("ctrl-1");
    detail.click("button", "닫기");
    assert!(!detail.has("heading", INFO_PANEL));
    assert!(detail.is_terminal_focused());
}

#[gpui::test]
fn the_notification_shortcut_toggles_the_detail_notification_center(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    let mut detail = open_detail(cx, &stored.id);
    detail.dispatch(ToggleNotificationCenter);
    assert!(detail.has("button", "지금 동기화"));
    detail.dispatch(ToggleNotificationCenter);
    assert!(!detail.has("button", "지금 동기화"));
}

#[gpui::test]
fn closing_the_notification_dropdown_returns_focus_to_the_terminal(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    let mut detail = open_detail(cx, &stored.id);
    detail.click("button", "알림");
    assert!(detail.has("button", "지금 동기화"));
    detail.dispatch(ToggleNotificationCenter);
    assert!(!detail.has("button", "지금 동기화"));
    assert!(detail.is_terminal_focused());
}

#[gpui::test]
fn page_back_wins_over_the_focused_terminal(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    let window: WindowHandle<AppWindow> =
        cx.add_window(|window, cx| AppWindow::new("/ko", window, cx));
    cx.run_until_parked();
    let task_path = format!("/ko/task/{}", stored.id);
    window
        .update(cx, |app_window, _, cx| app_window.navigate(&task_path, cx))
        .unwrap();
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    visual.simulate_keystrokes("ctrl-[");
    let path = window
        .read_with(cx, |app_window, _| app_window.current_path().to_string())
        .unwrap();
    assert_eq!(path, "/ko");
}

#[gpui::test]
fn a_slow_diff_does_not_hold_the_task_screen_or_terminal_input(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let changed =
        json!([{ "path": "src/app.rs", "status": "modified", "additions": 7, "deletions": 1 }]);
    // 첫 변경 파일 조회는 끝나지 않는다. 다시 열 때 두 번째 조회가 답하면 첫 조회가 실제로 나갔던 것이다
    add_fault(cx, after(1, result("diff.getGitDiffFiles", changed)));
    add_fault(cx, fault("diff.getGitDiffFiles", FaultMode::Hang));
    let stored = task(&database, "slow diff task", |task| {
        task.branch_name = Some("perf/slow-diff".into());
        task.base_branch = Some("dev".into());
    });
    let mut detail = open_detail(cx, &stored.id);

    detail.keys("ctrl-1");
    assert!(detail.has("heading", INFO_PANEL));
    assert!(detail.has_containing("group", "slow diff task"));
    detail.click("button", "닫기");
    assert!(!detail.has("heading", INFO_PANEL));
    assert!(!detail.has("status", CONNECTING));
    assert!(detail.is_terminal_focused());

    detail.keys("ctrl-1");
    assert!(detail.find(TargetQuery::ExactText("+7".into())).is_some());
}

#[gpui::test]
fn the_pane_chat_shortcut_keeps_the_terminal_and_the_dock_opens_history(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "pane shortcut", |_| {});
    let mut detail = open_detail(cx, &stored.id);
    // 키보드와 앱 명령 두 길 모두 화면이 받고, 터미널은 입력 포커스를 잃지 않는다
    detail.keys("ctrl-shift-j");
    detail.dispatch(ToggleTerminalChat);
    assert!(detail.is_terminal_focused());
    assert!(!detail.has("button", BACK_TO_TERMINAL));

    // dock은 기록을 연다. 기록이 보이는 동안 전환 단축키는 터미널 쪽을 건드리지 않는다
    detail.keys("ctrl-3");
    assert!(detail.has("button", BACK_TO_TERMINAL));
    detail.keys("ctrl-shift-j");
    assert!(detail.has("button", BACK_TO_TERMINAL));
    assert!(!detail.is_terminal_focused());
}

// ───────────── 탭 닫기 ─────────────

#[derive(Clone, Copy, PartialEq)]
enum CloseSource {
    AppCommand,
    Keyboard,
    MacKeyboard,
    Button,
}

struct CloseCase {
    source: CloseSource,
    is_accepted: bool,
    preview: Value,
}

fn open_two_tab_detail(cx: &mut TestAppContext, database: &Database) -> Detail {
    let stored = task(database, "task title", |_| {});
    clear_fault(cx, "terminalTabs.listTerminalTabs");
    add_fault(
        cx,
        result(
            "terminalTabs.listTerminalTabs",
            tabs(&[("work", true), ("logs", false)]),
        ),
    );
    open_detail(cx, &stored.id)
}

fn start_close(detail: &mut Detail, source: CloseSource) {
    match source {
        CloseSource::AppCommand => {
            // TS 시험처럼 두 명령이 같은 순간에 온다. 두 번째는 확인 중이라 무시된다
            detail.cx.update(|window, cx| {
                for _ in 0..2 {
                    window.dispatch_action(
                        Box::new(RunTerminalTabCommand {
                            command: TerminalTabShortcutCommand::CloseTab,
                        }),
                        cx,
                    );
                }
            });
            detail.draw();
        }
        CloseSource::Keyboard => detail.keys("ctrl-w"),
        CloseSource::MacKeyboard => detail.keys("cmd-w"),
        CloseSource::Button => {
            let tab = detail.find(described("tab", "work", true)).unwrap();
            detail
                .cx
                .simulate_mouse_move(tab.center(), None, Modifiers::default());
            detail.draw();
            detail.click("button", "work 탭 닫기");
        }
    }
}

fn run_close_case(cx: &mut TestAppContext, database: &Database, case: CloseCase) -> Detail {
    for target in [
        "terminalTabs.getTerminalTabCloseInfo",
        "terminalTabs.closeTerminalTab",
    ] {
        clear_fault(cx, target);
    }
    add_fault(
        cx,
        result("terminalTabs.getTerminalTabCloseInfo", case.preview.clone()),
    );
    // 닫기 요청이 가면 창까지 닫히게 해 요청이 갔는지 창으로 본다
    add_fault(
        cx,
        result(
            "terminalTabs.closeTerminalTab",
            json!({"ok": true, "shouldCloseWindow": true}),
        ),
    );
    let mac = case.source == CloseSource::MacKeyboard;
    cx.update(|cx| {
        cx.global_mut::<ShortcutState>().platform = match mac {
            true => ShortcutPlatform::Mac,
            false => ShortcutPlatform::Linux,
        }
    });
    let mut detail = open_two_tab_detail(cx, database);
    start_close(&mut detail, case.source);
    if case.preview["ok"] == json!(true) && case.preview["shouldCloseWindow"] != json!(true) {
        let message = detail.pending_prompt().expect("confirm prompt");
        let shortcut = if mac { "Cmd+Shift+W" } else { "Ctrl+Shift+W" };
        assert!(message.contains(shortcut), "{message}");
        let processes = case.preview["processNames"]
            .as_array()
            .map(|names| {
                names
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_else(|| "프로세스 정보를 확인할 수 없습니다".into());
        assert!(message.contains(&processes), "{message}");
        detail.answer_prompt(if case.is_accepted { "OK" } else { "Cancel" });
        assert_eq!(detail.is_open(), !case.is_accepted);
    }
    detail
}

#[gpui::test]
fn closing_a_tab_asks_first_and_handles_every_answer(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    use_real_prompts_and_focus(cx);
    let processes =
        json!({"ok": true, "shouldCloseWindow": false, "processNames": ["claude", "node"]});
    let unknown = json!({"ok": true, "shouldCloseWindow": false, "processNames": null});
    for (source, is_accepted, preview) in [
        (CloseSource::AppCommand, false, processes.clone()),
        (CloseSource::AppCommand, true, processes.clone()),
        (CloseSource::AppCommand, false, unknown),
        (
            CloseSource::MacKeyboard,
            false,
            json!({"ok": true, "shouldCloseWindow": false, "processNames": ["claude"]}),
        ),
        (
            CloseSource::Keyboard,
            false,
            json!({"ok": true, "shouldCloseWindow": false, "processNames": ["claude"]}),
        ),
        (
            CloseSource::Button,
            false,
            json!({"ok": true, "shouldCloseWindow": false, "processNames": ["claude"]}),
        ),
    ] {
        run_close_case(
            cx,
            &database,
            CloseCase {
                source,
                is_accepted,
                preview,
            },
        );
    }
    // 마지막 멀티플렉서 탭: 묻지 않고 창을 닫는다
    let case = CloseCase {
        source: CloseSource::AppCommand,
        is_accepted: false,
        preview: json!({"ok": true, "shouldCloseWindow": true, "processNames": null}),
    };
    let mut detail = run_close_case(cx, &database, case);
    assert!(!detail.is_open());
    // 실패 응답과 조회 예외: 확인 실패를 알리고 묻지도 닫지도 않는다
    let case = CloseCase {
        source: CloseSource::AppCommand,
        is_accepted: true,
        preview: json!({"ok": false, "shouldCloseWindow": false, "processNames": null}),
    };
    let mut detail = run_close_case(cx, &database, case);
    assert!(
        detail
            .pending_prompt()
            .unwrap_or_default()
            .starts_with("닫을 탭을 확인하지 못했습니다.")
    );
    detail.answer_prompt("OK");
    assert!(detail.is_open());
    clear_fault(cx, "terminalTabs.getTerminalTabCloseInfo");
    add_fault(
        cx,
        failure("terminalTabs.getTerminalTabCloseInfo", "lookup failed"),
    );
    let mut detail = open_two_tab_detail(cx, &database);
    start_close(&mut detail, CloseSource::AppCommand);
    assert!(
        detail
            .pending_prompt()
            .unwrap_or_default()
            .starts_with("닫을 탭을 확인하지 못했습니다.")
    );
    detail.answer_prompt("OK");
    assert!(detail.is_open());
    // 조회 중 화면 이탈: 묻지도 닫지도 않는다
    clear_fault(cx, "terminalTabs.getTerminalTabCloseInfo");
    add_fault(cx, delay("terminalTabs.getTerminalTabCloseInfo", 1_000));
    let mut detail = open_two_tab_detail(cx, &database);
    start_close(&mut detail, CloseSource::AppCommand);
    detail.cx.update(|window, _| window.remove_window());
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt(), "{:?}", cx.pending_prompt());
}

// ───────────── 터미널 탭 목록(TerminalLoader·useTerminalTabs) ─────────────

fn terminal_session(new_task: &mut NewKanbanTask) {
    new_task.session_type = Some(SessionType::Terminal);
}

#[gpui::test]
fn a_multiplexer_terminal_mounts_at_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    clear_fault(cx, "kanvibe:terminal-open");
    add_fault(cx, fault("kanvibe:terminal-open", FaultMode::Hang));
    add_fault(cx, fault("terminalTabs.listTerminalTabs", FaultMode::Hang));
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.has("status", CONNECTING));
}

#[gpui::test]
fn no_terminal_is_opened_before_the_tab_list_arrives(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", terminal_session);
    clear_fault(cx, "kanvibe:terminal-open");
    add_fault(cx, fault("kanvibe:terminal-open", FaultMode::Hang));
    add_fault(cx, fault("terminalTabs.listTerminalTabs", FaultMode::Hang));
    let mut detail = open_detail(cx, &stored.id);
    assert!(!detail.has("status", CONNECTING));
}

#[gpui::test]
fn waiting_for_the_tab_list_says_loading(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", terminal_session);
    add_fault(cx, fault("terminalTabs.listTerminalTabs", FaultMode::Hang));
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.has("status", "로딩 중..."));
}

#[gpui::test]
fn before_and_after_the_first_tab_list_look_different(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", terminal_session);
    add_fault(cx, delay("terminalTabs.listTerminalTabs", 500));
    add_fault(cx, result("terminalTabs.listTerminalTabs", tabs(&[])));
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.has("status", "로딩 중..."));
    clear_fault(cx, "terminalTabs.listTerminalTabs");
    add_fault(cx, result("terminalTabs.listTerminalTabs", tabs(&[])));
    detail.advance(Duration::from_millis(500));
    detail.dispatch(RunTerminalTabCommand {
        command: TerminalTabShortcutCommand::PreviousTab,
    });
    assert!(!detail.has("status", "로딩 중..."));
    assert!(detail.has("button", "새 탭"));
}

#[gpui::test]
fn an_empty_tab_list_offers_a_new_tab(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", terminal_session);
    add_fault(
        cx,
        after(
            1,
            result("terminalTabs.listTerminalTabs", tabs(&[("t1", true)])),
        ),
    );
    add_fault(cx, result("terminalTabs.listTerminalTabs", tabs(&[])));
    add_fault(
        cx,
        result("terminalTabs.createTerminalTab", json!({"ok": true})),
    );
    let mut detail = open_detail(cx, &stored.id);
    detail.click("button", "새 탭");
    assert!(detail.has("tab", "t1"));
}

#[gpui::test]
fn an_unreadable_tab_list_shows_the_reason_and_retry(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", terminal_session);
    add_fault(
        cx,
        result(
            "terminalTabs.listTerminalTabs",
            json!({"ok": false, "tabs": [], "error": "session missing"}),
        ),
    );
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.has_containing("alert", "session missing"));
    assert!(!detail.has("status", CONNECTING));
    clear_fault(cx, "terminalTabs.listTerminalTabs");
    add_fault(
        cx,
        result("terminalTabs.listTerminalTabs", tabs(&[("t1", true)])),
    );
    detail.click("button", "다시 시도");
    assert!(detail.has("tab", "t1"));
}

#[gpui::test]
fn an_inactive_tab_keeps_its_terminal_connection(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", terminal_session);
    clear_fault(cx, "kanvibe:terminal-open");
    add_fault(
        cx,
        after(2, fault("kanvibe:terminal-open", FaultMode::Hang)),
    );
    add_fault(cx, result("kanvibe:terminal-open", json!({"ok": true})));
    add_fault(
        cx,
        result(
            "terminalTabs.listTerminalTabs",
            tabs(&[("t1", false), ("t2", true)]),
        ),
    );
    add_fault(
        cx,
        result("terminalTabs.selectTerminalTab", json!({"ok": true})),
    );
    let mut detail = open_detail(cx, &stored.id);
    detail.click("tab", "t1");
    assert!(!detail.has("status", CONNECTING));
    detail.click("tab", "t2");
    assert!(
        !detail.has("status", CONNECTING),
        "t2 reuses its first connection"
    );
}

#[gpui::test]
fn the_chat_history_keeps_the_same_terminal_connection(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    clear_fault(cx, "kanvibe:terminal-open");
    add_fault(
        cx,
        after(1, fault("kanvibe:terminal-open", FaultMode::Hang)),
    );
    add_fault(cx, result("kanvibe:terminal-open", json!({"ok": true})));
    let mut detail = open_detail(cx, &stored.id);
    detail.keys("ctrl-3");
    assert!(!detail.is_terminal_focused());
    detail.click("button", BACK_TO_TERMINAL);
    assert!(!detail.has("status", CONNECTING));
    assert!(detail.is_terminal_focused());
}

#[gpui::test]
fn slow_tab_polls_stay_single_flight(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    // 터미널 연결이 끝나며 다시 읽는 일을 빼고 주기 조회만 본다
    clear_fault(cx, "kanvibe:terminal-open");
    add_fault(cx, fault("kanvibe:terminal-open", FaultMode::Hang));
    add_fault(
        cx,
        after(
            1,
            result("terminalTabs.listTerminalTabs", tabs(&[("second", true)])),
        ),
    );
    add_fault(
        cx,
        ServiceFault {
            ms: Some(2_500),
            value: Some(tabs(&[])),
            ..delay("terminalTabs.listTerminalTabs", 2_500)
        },
    );
    let mut detail = open_detail(cx, &stored.id);
    detail.advance(Duration::from_millis(2_500));
    assert!(
        !detail.has("tab", "second"),
        "no queued re-read right after a slow poll"
    );
    detail.advance(Duration::from_millis(500));
    assert!(detail.has("tab", "second"));
}

#[gpui::test]
fn a_command_during_a_read_ends_with_one_final_read(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", terminal_session);
    add_fault(
        cx,
        after(
            1,
            result("terminalTabs.listTerminalTabs", tabs(&[("new", true)])),
        ),
    );
    add_fault(cx, delay("terminalTabs.listTerminalTabs", 1_000));
    add_fault(
        cx,
        result("terminalTabs.createTerminalTab", json!({"ok": true})),
    );
    let mut detail = open_detail(cx, &stored.id);
    detail.dispatch(RunTerminalTabCommand {
        command: TerminalTabShortcutCommand::NewTab,
    });
    assert!(detail.has("tab", "new"));
    detail.advance(Duration::from_millis(1_000));
    assert!(
        detail.has("tab", "new"),
        "the earlier read does not overwrite the final one"
    );
}

#[gpui::test]
fn a_previous_session_late_tab_list_is_ignored(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let mut stored = task(&database, "task title", terminal_session);
    add_fault(
        cx,
        after(
            1,
            result("terminalTabs.listTerminalTabs", tabs(&[("second", true)])),
        ),
    );
    add_fault(cx, delay("terminalTabs.listTerminalTabs", 1_000));
    let mut detail = open_detail(cx, &stored.id);
    stored.session_name = Some("second-session".into());
    update_task(&database.connection(), &stored).unwrap();
    refresh(&mut detail);
    assert!(detail.has("tab", "second"));
    detail.advance(Duration::from_millis(1_000));
    assert!(detail.has("tab", "second"));
}

#[gpui::test]
fn a_second_new_tab_is_not_sent_while_one_is_being_added(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", terminal_session);
    add_fault(
        cx,
        result(
            "terminalTabs.listTerminalTabs",
            tabs(&[("a", true), ("b", false)]),
        ),
    );
    add_fault(
        cx,
        after(
            1,
            failure("terminalTabs.createTerminalTab", "second create sent"),
        ),
    );
    add_fault(
        cx,
        ServiceFault {
            ms: Some(1_000),
            ..failure("terminalTabs.createTerminalTab", "first create done")
        },
    );
    let mut detail = open_detail(cx, &stored.id);
    detail.click("button", "새 탭");
    detail.click("button", "새 탭");
    assert!(
        detail
            .find(TargetQuery::Described(DescribedTarget {
                role: Some("button".into()),
                name: Some("새 탭".into()),
                text: Some("…".into()),
                exact: Some(true),
                ..DescribedTarget::default()
            }))
            .is_some()
    );
    detail.advance(Duration::from_millis(1_000));
    assert!(has_tab_error(&mut detail, "first create done"));
    assert!(!has_tab_error(&mut detail, "second create sent"));
}

#[gpui::test]
fn a_hidden_remote_window_skips_tab_reads_and_catches_up_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    use_real_prompts_and_focus(cx);
    let stored = task(&database, "task title", |new_task| {
        new_task.session_type = Some(SessionType::Herdr);
        new_task.ssh_host = Some("remote-box".into());
    });
    add_fault(
        cx,
        result("terminalTabs.listTerminalTabs", tabs(&[("w1:t1", true)])),
    );
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.has("tab", "w1:t1"));
    detail.cx.deactivate_window();
    clear_fault(cx, "terminalTabs.listTerminalTabs");
    add_fault(
        cx,
        result("terminalTabs.listTerminalTabs", tabs(&[("w1:t2", true)])),
    );
    detail.advance(Duration::from_secs(10));
    assert!(
        detail.has("tab", "w1:t1"),
        "no read while the window is hidden"
    );
    detail.cx.update(|window, _| window.activate_window());
    detail.draw();
    assert!(detail.has("tab", "w1:t2"));
}

#[gpui::test]
fn a_window_opened_in_the_background_still_reads_the_tabs_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    use_real_prompts_and_focus(cx);
    let stored = task(&database, "task title", |new_task| {
        new_task.session_type = Some(SessionType::Herdr);
        new_task.ssh_host = Some("remote-box".into());
    });
    add_fault(cx, delay("kanban.getTaskById", 100));
    add_fault(
        cx,
        result("terminalTabs.listTerminalTabs", tabs(&[("w1:t1", true)])),
    );
    let mut detail = open_detail(cx, &stored.id);
    detail.cx.deactivate_window();
    detail.advance(Duration::from_millis(100));
    assert!(detail.has("tab", "w1:t1"));
}

// ───────────── 탭 바(TerminalTabBar) ─────────────

fn two_tab_bar(cx: &mut TestAppContext, dir: &tempfile::TempDir) -> Detail {
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    add_fault(
        cx,
        result(
            "terminalTabs.listTerminalTabs",
            tabs(&[("shell", true), ("logs", false)]),
        ),
    );
    open_detail(cx, &stored.id)
}

fn has_tab_error(detail: &mut Detail, reason: &str) -> bool {
    detail.has_containing("alert", reason)
}

#[gpui::test]
fn pressing_an_inactive_tab_selects_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut detail = two_tab_bar(cx, &dir);
    add_fault(cx, failure("terminalTabs.selectTerminalTab", "select sent"));
    detail.click("tab", "logs");
    assert!(has_tab_error(&mut detail, "select sent"));
}

#[gpui::test]
fn pressing_the_active_tab_sends_nothing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut detail = two_tab_bar(cx, &dir);
    add_fault(cx, failure("terminalTabs.selectTerminalTab", "select sent"));
    detail.click("tab", "shell");
    assert!(!has_tab_error(&mut detail, "select sent"));
}

#[gpui::test]
fn a_tab_close_button_does_not_select_the_tab(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut detail = two_tab_bar(cx, &dir);
    add_fault(cx, failure("terminalTabs.selectTerminalTab", "select sent"));
    add_fault(
        cx,
        result(
            "terminalTabs.getTerminalTabCloseInfo",
            json!({"ok": true, "shouldCloseWindow": true, "processNames": null}),
        ),
    );
    let tab = detail.find(described("tab", "logs", true)).unwrap();
    detail
        .cx
        .simulate_mouse_move(tab.center(), None, Modifiers::default());
    detail.draw();
    detail.click("button", "logs 탭 닫기");
    assert!(!detail.is_open(), "the close request ran");
}

#[gpui::test]
fn the_new_tab_button_creates_a_tab(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut detail = two_tab_bar(cx, &dir);
    add_fault(cx, failure("terminalTabs.createTerminalTab", "create sent"));
    detail.click("button", "새 탭");
    assert!(has_tab_error(&mut detail, "create sent"));
}

/// 활성 탭(shell)의 이름을 바꾼다. 비활성 탭을 누르면 첫 클릭이 선택을 먼저 보낸다
fn rename_logs(detail: &mut Detail, keys: &str) {
    detail.double_click("tab", "shell");
    detail.keys("ctrl-a");
    detail.cx.simulate_input("build");
    detail.draw();
    detail.keys(keys);
}

#[gpui::test]
fn double_click_then_enter_renames_the_tab(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut detail = two_tab_bar(cx, &dir);
    add_fault(
        cx,
        matching(
            "\"build\"",
            failure("terminalTabs.renameTerminalTab", "rename sent"),
        ),
    );
    rename_logs(&mut detail, "enter");
    assert!(has_tab_error(&mut detail, "rename sent"));
}

#[gpui::test]
fn escape_cancels_the_rename(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut detail = two_tab_bar(cx, &dir);
    add_fault(cx, failure("terminalTabs.renameTerminalTab", "rename sent"));
    rename_logs(&mut detail, "escape");
    assert!(!has_tab_error(&mut detail, "rename sent"));
    assert!(detail.has("tab", "logs"));
}

fn drag_tab(detail: &mut Detail, from: &str, to: &str) {
    let start = detail.find(described("tab", from, true)).unwrap().center();
    let end = detail.find(described("tab", to, true)).unwrap().center();
    detail
        .cx
        .simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    let step = gpui_kit::point(start.x + gpui_kit::px(20.0), start.y);
    detail
        .cx
        .simulate_mouse_move(step, MouseButton::Left, Modifiers::default());
    detail
        .cx
        .simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
    detail
        .cx
        .simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    detail.draw();
}

#[gpui::test]
fn dropping_a_tab_moves_it_to_that_index(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut detail = two_tab_bar(cx, &dir);
    add_fault(
        cx,
        matching(
            "\"shell\",1",
            failure("terminalTabs.moveTerminalTab", "move sent"),
        ),
    );
    drag_tab(&mut detail, "shell", "logs");
    assert!(has_tab_error(&mut detail, "move sent"));
}

#[gpui::test]
fn dropping_a_tab_on_itself_moves_nothing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut detail = two_tab_bar(cx, &dir);
    add_fault(cx, failure("terminalTabs.moveTerminalTab", "move sent"));
    let start = detail
        .find(described("tab", "shell", true))
        .unwrap()
        .center();
    detail
        .cx
        .simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    let step = gpui_kit::point(start.x + gpui_kit::px(20.0), start.y);
    detail
        .cx
        .simulate_mouse_move(step, MouseButton::Left, Modifiers::default());
    detail
        .cx
        .simulate_mouse_up(step, MouseButton::Left, Modifiers::default());
    detail.draw();
    assert!(!has_tab_error(&mut detail, "move sent"));
}

#[gpui::test]
fn adding_a_tab_blocks_the_add_button_and_shows_progress(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut detail = two_tab_bar(cx, &dir);
    add_fault(cx, fault("terminalTabs.createTerminalTab", FaultMode::Hang));
    detail.click("button", "새 탭");
    let busy = TargetQuery::Described(DescribedTarget {
        role: Some("button".into()),
        name: Some("새 탭".into()),
        text: Some("…".into()),
        exact: Some(true),
        ..DescribedTarget::default()
    });
    assert!(detail.find(busy).is_some());
}

#[gpui::test]
fn a_failed_tab_command_shows_the_reason_and_retries_the_same_command(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut detail = two_tab_bar(cx, &dir);
    add_fault(
        cx,
        after(
            1,
            result("terminalTabs.renameTerminalTab", json!({"ok": true})),
        ),
    );
    add_fault(
        cx,
        matching(
            "\"build\"",
            failure("terminalTabs.renameTerminalTab", "tmux gone"),
        ),
    );
    rename_logs(&mut detail, "enter");
    assert!(has_tab_error(&mut detail, "tmux gone"));
    detail.click("button", "다시 시도");
    assert!(!has_tab_error(&mut detail, "tmux gone"));
}

// ───────────── 자동 연결(AutoConnectTerminalSession) ─────────────

fn connected(task: &KanbanTask) -> Value {
    let mut connected = task.clone();
    connected.session_type = Some(SessionType::Tmux);
    connected.session_name = Some("task-1-tmux".into());
    serde_json::to_value(connected).unwrap()
}

fn auto_connect_task(database: &Database, ssh_host: Option<&str>) -> KanbanTask {
    let project_id = project(database);
    task(database, "auto connect", |new_task| {
        no_session(new_task);
        new_task.project_id = Some(project_id);
        new_task.ssh_host = ssh_host.map(str::to_string);
    })
}

#[gpui::test]
fn a_session_less_task_connects_with_the_default_session_type(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = auto_connect_task(&database, Some("remote-box"));
    add_fault(
        cx,
        matching(
            "[\"tmux\",\"remote-box\"]",
            result(
                "sessionDependency.getSessionDependencyStatus",
                json!({"available": true, "toolName": "tmux"}),
            ),
        ),
    );
    add_fault(
        cx,
        matching(
            &format!("[\"{}\",\"tmux\"]", stored.id),
            result("kanban.connectTerminalSession", connected(&stored)),
        ),
    );
    add_fault(cx, fault("terminalTabs.listTerminalTabs", FaultMode::Hang));
    let mut detail = open_detail(cx, &stored.id);
    assert!(!detail.has("paragraph", "이 작업에는 연결된 터미널 세션이 없습니다."));
    assert!(detail.has("button", "알림"), "the terminal chrome is shown");
}

#[gpui::test]
fn the_same_task_is_connected_only_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = auto_connect_task(&database, None);
    add_fault(
        cx,
        ServiceFault {
            value: Some(json!({"available": true})),
            ..delay("sessionDependency.getSessionDependencyStatus", 500)
        },
    );
    add_fault(
        cx,
        after(
            1,
            failure("kanban.connectTerminalSession", "second connect"),
        ),
    );
    add_fault(
        cx,
        result("kanban.connectTerminalSession", connected(&stored)),
    );
    add_fault(cx, fault("terminalTabs.listTerminalTabs", FaultMode::Hang));
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.has("status", "연결 중..."));
    refresh(&mut detail);
    detail.advance(Duration::from_millis(500));
    assert!(!detail.has_containing("alert", "second connect"));
    assert!(detail.has("button", "알림"));
}

#[gpui::test]
fn a_failed_connect_shows_the_error_and_the_button_reconnects(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = auto_connect_task(&database, None);
    add_fault(
        cx,
        result(
            "sessionDependency.getSessionDependencyStatus",
            json!({"available": true}),
        ),
    );
    add_fault(
        cx,
        after(
            1,
            result("kanban.connectTerminalSession", connected(&stored)),
        ),
    );
    add_fault(cx, result("kanban.connectTerminalSession", Value::Null));
    add_fault(cx, fault("terminalTabs.listTerminalTabs", FaultMode::Hang));
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.has(
        "alert",
        "터미널 세션 생성에 실패했습니다. 서버 로그를 확인하세요."
    ));
    detail.click("button", "터미널 연결");
    assert!(detail.has("button", "알림"));
}

#[gpui::test]
fn a_thrown_dependency_check_shows_its_message_without_connecting(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = auto_connect_task(&database, None);
    add_fault(
        cx,
        failure(
            "sessionDependency.getSessionDependencyStatus",
            "zellij 설치 실패",
        ),
    );
    add_fault(
        cx,
        failure("kanban.connectTerminalSession", "connect called"),
    );
    let mut detail = open_detail(cx, &stored.id);
    // 서비스 실패는 TS IPC 오류 문구에 감싸여 보인다(`format_service_error`)
    assert!(detail.has_containing("alert", "zellij 설치 실패"));
    assert!(!detail.has_containing("alert", "connect called"));
}

#[gpui::test]
fn declining_the_install_prompt_leaves_only_the_connect_button(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    use_real_prompts_and_focus(cx);
    let stored = auto_connect_task(&database, None);
    add_fault(
        cx,
        result(
            "sessionDependency.getSessionDependencyStatus",
            json!({"available": false, "toolName": "herdr", "isRemote": false}),
        ),
    );
    add_fault(
        cx,
        failure("kanban.connectTerminalSession", "connect called"),
    );
    let mut detail = open_detail(cx, &stored.id);
    assert!(
        detail
            .pending_prompt()
            .unwrap_or_default()
            .contains("herdr")
    );
    detail.answer_prompt("Cancel");
    assert!(detail.has("paragraph", "이 작업에는 연결된 터미널 세션이 없습니다."));
    assert!(detail.has("button", "터미널 연결"));
    assert!(!detail.has_containing("alert", ""));
}

#[gpui::test]
fn the_usage_command_sent_by_the_app_opens_the_usage_panel(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", |_| {});
    let mut detail = open_detail(cx, &stored.id);
    detail.dispatch(kanvibe_app::shortcuts::ToggleTaskDetailUsage);
    assert!(detail.has("heading", "AI 사용량"));
}

/// 이 파일의 다른 시험처럼 DB를 두되 공용 `/tmp`가 아니라 cargo 대상 폴더 안에 만든다
fn target_temp_dir() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap()
}

/// 프로젝트·브랜치가 있는 작업(TS `feat/detail-shortcut`)
fn branch_task(database: &Database, edit: impl FnOnce(&mut NewKanbanTask)) -> KanbanTask {
    let project_id = project(database);
    task(database, "task title", |new_task| {
        new_task.project_id = Some(project_id);
        new_task.branch_name = Some("feat/detail-shortcut".into());
        new_task.base_branch = Some("main".into());
        edit(new_task);
    })
}

#[gpui::test]
fn the_open_task_becomes_the_board_return_focus_target(cx: &mut TestAppContext) {
    let dir = target_temp_dir();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", no_session);
    let mut detail = open_detail(cx, &stored.id);
    let remembered = detail.cx.update(|window, cx| {
        kanvibe_app::board_focus_target::consume_board_focus_task(
            window.window_handle().window_id(),
            cx,
        )
    });
    assert_eq!(remembered.as_deref(), Some(stored.id.as_str()));
}

#[gpui::test]
fn the_new_task_shortcut_opens_the_modal_based_on_this_branch(cx: &mut TestAppContext) {
    let dir = target_temp_dir();
    let database = init_app(cx, dir.path().to_path_buf());
    // 세션이 없으면 자동 연결이 hook 설치 스레드를 띄워 시험 스케줄러를 깨므로 세션이 있는 작업으로 연다
    let stored = branch_task(&database, |_| {});
    add_fault(cx, result("project.getProjectBranches", json!(["main"])));
    let mut detail = open_detail(cx, &stored.id);
    detail.dispatch(kanvibe_app::shortcuts::OpenCreateTask);
    let defaults = detail.cx.update(|_, cx| {
        detail
            .view
            .upgrade()
            .unwrap()
            .read(cx)
            .create_task_defaults(cx)
    });
    assert_eq!(
        defaults,
        Some((
            stored.project_id.clone().unwrap(),
            "feat/detail-shortcut".to_string(),
            "tmux".to_string(),
        ))
    );
}

#[gpui::test]
fn closing_the_new_task_modal_returns_focus_to_the_terminal(cx: &mut TestAppContext) {
    let dir = target_temp_dir();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = branch_task(&database, |_| {});
    add_fault(cx, result("project.getProjectBranches", json!(["main"])));
    let mut detail = open_detail(cx, &stored.id);
    assert!(detail.is_terminal_focused());
    let terminal_focus = detail.cx.update(|window, cx| window.focused(cx));
    detail.dispatch(kanvibe_app::shortcuts::OpenCreateTask);
    let is_open = |detail: &mut Detail| {
        detail
            .entity()
            .read_with(&detail.cx, |view, _| view.is_create_task_open())
    };
    assert!(is_open(&mut detail));
    // 모달의 베이스 브랜치 입력란이 포커스를 가져간다
    assert_ne!(
        detail.cx.update(|window, cx| window.focused(cx)),
        terminal_focus
    );
    detail.keys("escape");
    assert!(!is_open(&mut detail));
    assert_eq!(
        detail.cx.update(|window, cx| window.focused(cx)),
        terminal_focus
    );
}

#[gpui::test]
fn the_command_palette_moves_this_task_and_updates_the_screen(cx: &mut TestAppContext) {
    let dir = target_temp_dir();
    let database = init_app(cx, dir.path().to_path_buf());
    let stored = task(&database, "task title", no_session);
    let mut detail = open_detail(cx, &stored.id);
    let target = detail
        .entity()
        .read_with(&detail.cx, |view, _| view.palette_move_target());
    assert_eq!(target, Some((stored.id.clone(), TaskStatus::Todo)));
    detail.entity().update(&mut detail.cx, |view, cx| {
        view.move_status_from_palette(TaskStatus::Review, cx)
    });
    detail.draw();
    let moved = detail
        .entity()
        .read_with(&detail.cx, |view, _| view.palette_move_target());
    assert_eq!(moved, Some((stored.id.clone(), TaskStatus::Review)));
}
