//! 헤드리스 GPUI 시험: 단축키가 터미널보다 앞서는지, 창 닫기·뒤로/앞으로 단축키, 알림 클릭이 작업 창을 재사용하는지.

use gpui_kit::gpui;
use gpui_kit::{
    AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement, KeyBinding,
    KeyDownEvent, ParentElement, Render, SharedString, Styled, SystemNotificationResponse,
    TestAppContext, VisualTestContext, Window, WindowHandle, actions, div,
};
use kanvibe_app::app_window::AppWindow;
use kanvibe_app::notifications::{TaskNotificationTarget, handle_system_notification_response};
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions};
use kanvibe_app::shortcuts::{
    OpenTaskDetailDockItem, ShortcutState, TASK_DETAIL_KEY_CONTEXT, TERMINAL_KEY_CONTEXT,
};
use kanvibe_app::windows::{TaskNavigationOutcome, TaskNavigationRequest, navigate_to_task_detail};
use kanvibe_domain::i18n::Locale;
use kanvibe_domain::platform::ShortcutPlatform;

actions!(terminal_probe, [SendKeysToShell]);

fn launch_options(data_dir: Option<std::path::PathBuf>) -> LaunchOptions {
    LaunchOptions {
        data_dir,
        initial_route: "/ko".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: None,
    }
}

fn init_app(cx: &mut TestAppContext, data_dir: Option<std::path::PathBuf>) {
    cx.update(|cx| {
        kanvibe_app::init_app(&launch_options(data_dir), cx).expect("app init");
        cx.global_mut::<ShortcutState>().platform = ShortcutPlatform::Linux;
    });
}

/// 셸 입력을 흉내 내는 터미널. 같은 조합을 자기 컨텍스트에 묶어 두어 앱 단축키와 겨루게 한다
struct TerminalProbe {
    focus_handle: FocusHandle,
    received_keys: Vec<SharedString>,
    shell_actions: usize,
}

impl Render for TerminalProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(TERMINAL_KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, _| {
                this.received_keys.push(
                    event
                        .keystroke
                        .unparse(gpui_kit::OperatingSystem::Linux)
                        .into(),
                );
            }))
            .on_action(cx.listener(|this, _: &SendKeysToShell, _, _| this.shell_actions += 1))
            .size_full()
    }
}

/// 작업 상세 화면 대역. dock 동작을 받고 그 안에 터미널을 둔다
struct TaskDetailProbe {
    terminal: Entity<TerminalProbe>,
    opened_dock_positions: Vec<u8>,
}

impl Render for TaskDetailProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(TASK_DETAIL_KEY_CONTEXT)
            .on_action(cx.listener(|this, action: &OpenTaskDetailDockItem, _, _| {
                this.opened_dock_positions.push(action.position);
            }))
            .size_full()
            .child(self.terminal.clone())
    }
}

fn open_task_detail_probe(
    cx: &mut TestAppContext,
) -> (Entity<TaskDetailProbe>, &mut VisualTestContext) {
    cx.update(|cx| {
        cx.bind_keys([KeyBinding::new(
            "ctrl-1",
            SendKeysToShell,
            Some(TERMINAL_KEY_CONTEXT),
        )])
    });
    cx.add_window_view(|window, cx| {
        let terminal = cx.new(|cx| TerminalProbe {
            focus_handle: cx.focus_handle(),
            received_keys: Vec::new(),
            shell_actions: 0,
        });
        terminal.read(cx).focus_handle.clone().focus(window, cx);
        TaskDetailProbe {
            terminal,
            opened_dock_positions: Vec::new(),
        }
    })
}

#[gpui::test]
fn dock_shortcut_beats_a_focused_terminal_binding(cx: &mut TestAppContext) {
    init_app(cx, None);
    let (detail, cx) = open_task_detail_probe(cx);

    cx.simulate_keystrokes("ctrl-1");
    cx.simulate_keystrokes("ctrl-a");

    detail.read_with(cx, |detail, cx| {
        assert_eq!(detail.opened_dock_positions, vec![1]);
        let terminal = detail.terminal.read(cx);
        assert_eq!(
            terminal.shell_actions, 0,
            "terminal binding must not see Mod+1"
        );
        assert_eq!(terminal.received_keys, vec![SharedString::from("ctrl-a")]);
    });
}

// parity: TS-renderer-shortcutBindings-4454dc
#[gpui::test]
fn user_rebound_dock_shortcut_is_read_from_app_settings(cx: &mut TestAppContext) {
    let data_dir = tempfile::tempdir().unwrap();
    let database = kanvibe_app::services::AppServices::open_database(data_dir.path()).unwrap();
    kanvibe_db::repositories::app_settings::save_app_setting(
        &database.connection(),
        "shortcut_bindings",
        r#"{"taskDetailDock1":"Mod+Alt+9"}"#,
    )
    .unwrap();
    drop(database);
    init_app(cx, Some(data_dir.path().to_path_buf()));
    let (detail, cx) = open_task_detail_probe(cx);

    cx.simulate_keystrokes("ctrl-1");
    cx.simulate_keystrokes("ctrl-alt-9");

    detail.read_with(cx, |detail, cx| {
        assert_eq!(detail.opened_dock_positions, vec![1]);
        assert_eq!(
            detail.terminal.read(cx).shell_actions,
            1,
            "the old combo goes back to the terminal"
        );
    });
}

fn open_app_window(cx: &mut TestAppContext, path: &str) -> WindowHandle<AppWindow> {
    let path = path.to_string();
    let window = cx.add_window(move |window, cx| AppWindow::new(&path, window, cx));
    cx.run_until_parked();
    window
}

fn current_path(window: WindowHandle<AppWindow>, cx: &mut TestAppContext) -> String {
    window
        .read_with(cx, |app_window, _| app_window.current_path().to_string())
        .unwrap()
}

// parity: TS-renderer-App-d31421
#[gpui::test]
fn mod_w_on_the_board_closes_the_window(cx: &mut TestAppContext) {
    init_app(cx, None);
    let window = open_app_window(cx, "/ko");

    VisualTestContext::from_window(window.into(), cx).simulate_keystrokes("ctrl-w");
    cx.run_until_parked();

    assert!(
        window.read_with(cx, |_, _| ()).is_err(),
        "board window must close"
    );
}

// parity: TS-renderer-App-b1e8ab
#[gpui::test]
fn window_close_command_closes_any_screen(cx: &mut TestAppContext) {
    init_app(cx, None);
    let window = open_app_window(cx, "/ko/settings");

    VisualTestContext::from_window(window.into(), cx).simulate_keystrokes("ctrl-shift-w");
    cx.run_until_parked();

    assert!(
        window.read_with(cx, |_, _| ()).is_err(),
        "Mod+Shift+W closes the window"
    );
}

// parity: TS-renderer-App-749f11
#[gpui::test]
fn mod_w_on_task_detail_leaves_the_window_open(cx: &mut TestAppContext) {
    init_app(cx, None);
    let window = open_app_window(cx, "/ko/task/task-1");

    VisualTestContext::from_window(window.into(), cx).simulate_keystrokes("ctrl-w");
    cx.run_until_parked();

    assert_eq!(current_path(window, cx), "/ko/task/task-1");
}

// parity: TS-renderer-navigation-799ff2
#[gpui::test]
fn linux_ctrl_brackets_move_back_and_forward(cx: &mut TestAppContext) {
    init_app(cx, None);
    let window = open_app_window(cx, "/ko");
    window
        .update(cx, |app_window, _, cx| {
            app_window.navigate("/ko/task/task-1", cx);
            app_window.navigate("/ko/task/task-2", cx);
            app_window.go_back(cx);
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);

    visual.simulate_keystrokes("ctrl-[");
    assert_eq!(current_path(window, cx), "/ko");
    VisualTestContext::from_window(window.into(), cx).simulate_keystrokes("ctrl-]");
    assert_eq!(current_path(window, cx), "/ko/task/task-1");
}

// parity: TS-renderer-navigation-407ae1
#[gpui::test]
fn macos_cmd_brackets_move_back_and_forward(cx: &mut TestAppContext) {
    init_app(cx, None);
    cx.update(|cx| cx.global_mut::<ShortcutState>().platform = ShortcutPlatform::Mac);
    let window = open_app_window(cx, "/ko");
    window
        .update(cx, |app_window, _, cx| {
            app_window.navigate("/ko/task/task-1", cx)
        })
        .unwrap();

    VisualTestContext::from_window(window.into(), cx).simulate_keystrokes("ctrl-[");
    assert_eq!(
        current_path(window, cx),
        "/ko/task/task-1",
        "Ctrl is not Mod on macOS"
    );
    VisualTestContext::from_window(window.into(), cx).simulate_keystrokes("cmd-[");
    assert_eq!(current_path(window, cx), "/ko");
    VisualTestContext::from_window(window.into(), cx).simulate_keystrokes("cmd-]");
    assert_eq!(current_path(window, cx), "/ko/task/task-1");
}

// parity: TS-renderer-taskNavigation-fa4433
#[gpui::test]
fn task_navigation_focuses_the_window_already_showing_the_task(cx: &mut TestAppContext) {
    init_app(cx, None);
    let board = open_app_window(cx, "/en");
    let task_window = open_app_window(cx, "/en/task/task-1");

    let outcome = cx.update(|cx| {
        navigate_to_task_detail(
            "task-1",
            Locale::En,
            TaskNavigationRequest {
                origin_window: Some(board.window_id()),
                should_open_new_window: false,
            },
            cx,
        )
    });

    assert_eq!(outcome, TaskNavigationOutcome::FocusedExistingWindow);
    assert_eq!(current_path(board, cx), "/en");
    assert_eq!(current_path(task_window, cx), "/en/task/task-1");
}

// parity: TS-renderer-taskNavigation-b4e621
#[gpui::test]
fn task_navigation_moves_the_origin_window_when_no_window_shows_the_task(cx: &mut TestAppContext) {
    init_app(cx, None);
    let board = open_app_window(cx, "/en");

    let outcome = cx.update(|cx| {
        navigate_to_task_detail(
            "task-1",
            Locale::En,
            TaskNavigationRequest {
                origin_window: Some(board.window_id()),
                should_open_new_window: false,
            },
            cx,
        )
    });

    assert_eq!(outcome, TaskNavigationOutcome::NavigatedInWindow);
    assert_eq!(current_path(board, cx), "/en/task/task-1");
}

#[gpui::test]
fn os_notification_click_reuses_the_open_task_window(cx: &mut TestAppContext) {
    init_app(cx, None);
    let board = open_app_window(cx, "/ko");
    let task_window = open_app_window(cx, "/ko/task/task-7");
    let window_count = cx.update(|cx| cx.windows().len());
    let click = |task_id: &str| SystemNotificationResponse {
        tag: TaskNotificationTarget {
            task_id: task_id.to_string(),
            locale: Locale::Ko,
        }
        .to_tag()
        .into(),
        action_id: None,
    };

    let outcome = cx.update(|cx| handle_system_notification_response(&click("task-7"), cx));

    assert_eq!(outcome, Some(TaskNavigationOutcome::FocusedExistingWindow));
    assert_eq!(cx.update(|cx| cx.windows().len()), window_count);
    assert_eq!(current_path(board, cx), "/ko");
    assert_eq!(current_path(task_window, cx), "/ko/task/task-7");
    let foreign = SystemNotificationResponse {
        tag: "other".into(),
        action_id: None,
    };
    assert_eq!(
        cx.update(|cx| handle_system_notification_response(&foreign, cx)),
        None
    );
}

fn locale_shell_state<T>(
    window: WindowHandle<AppWindow>,
    cx: &mut TestAppContext,
    read: impl FnOnce(&kanvibe_app::views::locale_shell::LocaleShell, &gpui_kit::App) -> T,
) -> T {
    window
        .read_with(cx, |app_window, cx| {
            read(app_window.locale_shell().read(cx), cx)
        })
        .unwrap()
}

// TS `LocaleShell`은 모든 경로에 커맨드 팔레트·빠른 검색을 둔다
#[gpui::test]
fn command_palette_and_quick_search_open_outside_the_board(cx: &mut TestAppContext) {
    init_app(cx, None);
    let window = open_app_window(cx, "/ko/settings");
    let mut visual = VisualTestContext::from_window(window.into(), cx);

    visual.simulate_keystrokes("ctrl-shift-p");
    visual.run_until_parked();
    assert!(locale_shell_state(window, cx, |shell, _| shell.is_command_palette_open()));

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_keystrokes("escape");
    visual.dispatch_action(kanvibe_app::shortcuts::OpenTaskSearch);
    visual.run_until_parked();
    assert!(locale_shell_state(window, cx, |shell, _| shell.is_quick_search_open()));
    assert_eq!(current_path(window, cx), "/ko/settings");
}

// TS main은 검토 알림을 지금 창으로 보내고 경로는 그대로 둔다
#[gpui::test]
fn activated_review_notification_opens_the_review_dialog_on_the_current_route(
    cx: &mut TestAppContext,
) {
    init_app(cx, None);
    let window = open_app_window(cx, "/ko/pane-layout");
    let notification: kanvibe_domain::notifications::AppNotification =
        serde_json::from_value(serde_json::json!({
            "id": "review", "title": "백그라운드 sync 검토", "body": "확인할 항목 1개",
            "taskId": null, "relativePath": "/ko", "locale": "ko", "isRead": false,
            "createdAt": "2026-01-15T02:55:00.000Z", "dedupeKey": "review",
            "action": {"type": "background-sync-review", "payload": {
                "mergedPullRequests": [{"taskId": "t1", "taskTitle": "로그인 수정",
                    "branchName": "fix/login", "prUrl": "https://example.com/pull/1",
                    "mergedAt": "2026-01-15T02:30:00.000Z"}],
                "registeredWorktrees": []
            }}
        }))
        .unwrap();
    cx.update(|cx| kanvibe_app::app_events::activate_notification(notification, None, cx));
    cx.run_until_parked();

    assert!(locale_shell_state(window, cx, |shell, cx| shell.has_open_overlay(cx)));
    assert_eq!(current_path(window, cx), "/ko/pane-layout");
}
