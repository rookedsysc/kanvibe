//! TS main `desktopShortcutRouting`이 Electron `before-input-event`로 하던 단축키 가로채기를 GPUI 키 가로채기에서 확인한다.
//! TS 시험은 `electron/main.js` 소스 문자열을 검사했지만, 여기서는 같은 요구를 실제 키 입력으로 확인한다.

use gpui_kit::gpui;
use gpui_kit::{
    AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, Styled, TestAppContext, VisualTestContext, Window,
    WindowHandle, div,
};
use kanvibe_app::app_window::AppWindow;
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions};
use kanvibe_app::shortcuts::{
    OpenCreateTask, ShortcutState, TASK_DETAIL_KEY_CONTEXT, TERMINAL_KEY_CONTEXT,
    ToggleTaskDetailUsage, ToggleTerminalChat, begin_shortcut_capture, reload_shortcut_bindings,
    shortcut_event_from_keystroke,
};
use kanvibe_app::window_open::WindowOpenAction;
use kanvibe_app::windows::{WindowRegistry, open_href};
use kanvibe_domain::platform::ShortcutPlatform;

fn init_app(cx: &mut TestAppContext, data_dir: Option<std::path::PathBuf>) {
    let launch = LaunchOptions {
        data_dir,
        initial_route: "/ko".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: None,
    };
    cx.update(|cx| {
        kanvibe_app::init_app(&launch, cx).expect("app init");
        cx.global_mut::<ShortcutState>().platform = ShortcutPlatform::Linux;
    });
}

/// 포커스를 가진 셸 대역. 앱이 가로채지 않은 키만 여기 닿는다
struct TerminalProbe {
    focus_handle: FocusHandle,
    received_keys: Vec<SharedString>,
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
            .size_full()
    }
}

/// 화면 대역. 작업 상세 키 컨텍스트를 걸지 정할 수 있고, 앱 명령을 받은 순서를 남긴다
struct ScreenProbe {
    is_task_detail: bool,
    terminal: Entity<TerminalProbe>,
    received_commands: Vec<&'static str>,
}

impl Render for ScreenProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let screen = div()
            .on_action(cx.listener(|this, _: &OpenCreateTask, _, _| {
                this.received_commands.push("createTask")
            }))
            .on_action(cx.listener(|this, _: &ToggleTaskDetailUsage, _, _| {
                this.received_commands.push("taskDetailUsage")
            }))
            .on_action(cx.listener(|this, _: &ToggleTerminalChat, _, _| {
                this.received_commands.push("terminalToggleChat")
            }))
            .size_full()
            .child(self.terminal.clone());
        if self.is_task_detail {
            screen.key_context(TASK_DETAIL_KEY_CONTEXT)
        } else {
            screen
        }
    }
}

fn open_screen_probe(
    cx: &mut TestAppContext,
    is_task_detail: bool,
) -> (Entity<ScreenProbe>, &mut VisualTestContext) {
    cx.add_window_view(|window, cx| {
        let terminal = cx.new(|cx| TerminalProbe {
            focus_handle: cx.focus_handle(),
            received_keys: Vec::new(),
        });
        terminal.read(cx).focus_handle.clone().focus(window, cx);
        ScreenProbe {
            is_task_detail,
            terminal,
            received_commands: Vec::new(),
        }
    })
}

fn received(
    probe: &Entity<ScreenProbe>,
    cx: &mut VisualTestContext,
) -> (Vec<&'static str>, Vec<SharedString>) {
    probe.read_with(cx, |probe, cx| {
        (
            probe.received_commands.clone(),
            probe.terminal.read(cx).received_keys.clone(),
        )
    })
}

fn open_app_window(cx: &mut TestAppContext, path: &str) -> WindowHandle<AppWindow> {
    let path = path.to_string();
    let window = cx.add_window(move |window, cx| AppWindow::new(&path, window, cx));
    cx.run_until_parked();
    window
}

fn capturing_window_count(cx: &mut TestAppContext) -> usize {
    cx.update(|cx| cx.global::<ShortcutState>().capturing_windows.len())
}

fn save_shortcut_bindings(data_dir: &std::path::Path, bindings_json: &str) {
    let database = kanvibe_app::services::AppServices::open_database(data_dir).unwrap();
    kanvibe_db::repositories::app_settings::save_app_setting(
        &database.connection(),
        "shortcut_bindings",
        bindings_json,
    )
    .unwrap();
}

// parity: TS-main-desktopShortcutRouting-096c60
#[gpui::test]
fn platform_shortcuts_reach_create_task_and_new_window_before_the_terminal(
    cx: &mut TestAppContext,
) {
    init_app(cx, None);
    let (probe, visual) = open_screen_probe(cx, false);

    visual.simulate_keystrokes("ctrl-n");
    visual.update(|_, cx| cx.global_mut::<ShortcutState>().platform = ShortcutPlatform::Mac);
    visual.simulate_keystrokes("ctrl-n");
    visual.simulate_keystrokes("cmd-n");

    let (commands, terminal_keys) = received(&probe, visual);
    assert_eq!(commands, vec!["createTask", "createTask"]);
    assert_eq!(
        terminal_keys,
        vec![SharedString::from("ctrl-n")],
        "Ctrl is not Mod on macOS"
    );

    cx.update(|cx| cx.global_mut::<ShortcutState>().platform = ShortcutPlatform::Linux);
    let settings = open_app_window(cx, "/ko/settings");
    let window_count = cx.update(|cx| cx.windows().len());
    VisualTestContext::from_window(settings.into(), cx).simulate_keystrokes("ctrl-shift-n");
    cx.run_until_parked();

    let windows = cx.update(|cx| cx.windows());
    assert_eq!(
        windows.len(),
        window_count + 1,
        "Mod+Shift+N opens a window"
    );
    // 앱이 연 창은 GPUI Kit 루트가 AppWindow를 감싸므로 창 등록부에서 경로를 읽는다
    let new_window_path = cx.update(|cx| {
        windows
            .iter()
            .filter(|window| window.window_id() != settings.window_id())
            .find_map(|window| {
                cx.global::<WindowRegistry>()
                    .current_path(window.window_id())
                    .map(str::to_string)
            })
    });
    assert_eq!(
        new_window_path.as_deref(),
        Some("/ko/settings"),
        "the new window opens on the current route"
    );
}

// parity: TS-main-desktopShortcutRouting-830be7
#[gpui::test]
fn usage_shortcut_is_taken_only_on_task_detail(cx: &mut TestAppContext) {
    init_app(cx, None);
    let (board, visual) = open_screen_probe(cx, false);
    visual.simulate_keystrokes("ctrl-0");
    assert_eq!(
        received(&board, visual),
        (Vec::new(), vec![SharedString::from("ctrl-0")]),
        "outside task detail Mod+0 stays with the focused element (zoom reset)"
    );

    let (detail, visual) = open_screen_probe(cx, true);
    visual.simulate_keystrokes("ctrl-0");
    assert_eq!(
        received(&detail, visual),
        (vec!["taskDetailUsage"], Vec::new())
    );
}

// parity: TS-main-desktopShortcutRouting-2fb50f
#[gpui::test]
fn pane_chat_toggle_beats_the_terminal_only_on_task_detail(cx: &mut TestAppContext) {
    init_app(cx, None);
    let (board, visual) = open_screen_probe(cx, false);
    visual.simulate_keystrokes("ctrl-shift-j");
    assert_eq!(received(&board, visual).0, Vec::<&str>::new());
    assert_eq!(received(&board, visual).1.len(), 1);

    let (detail, visual) = open_screen_probe(cx, true);
    visual.simulate_keystrokes("ctrl-shift-j");
    assert_eq!(
        received(&detail, visual),
        (vec!["terminalToggleChat"], Vec::new()),
        "the PTY must not see the toggle"
    );
}

// parity: TS-main-desktopShortcutRouting-17cf33
#[gpui::test]
fn saved_rebinding_is_read_again_after_the_change_notification(cx: &mut TestAppContext) {
    let data_dir = tempfile::tempdir().unwrap();
    save_shortcut_bindings(data_dir.path(), r#"{"createTask":"Mod+Alt+N"}"#);
    init_app(cx, Some(data_dir.path().to_path_buf()));
    let (probe, visual) = open_screen_probe(cx, false);
    visual.simulate_keystrokes("ctrl-alt-n");
    assert_eq!(
        received(&probe, visual).0,
        vec!["createTask"],
        "startup reads the rebinding"
    );

    save_shortcut_bindings(data_dir.path(), r#"{"createTask":"Mod+Alt+M"}"#);
    visual.update(|_, cx| reload_shortcut_bindings(cx));
    visual.simulate_keystrokes("ctrl-alt-n");
    visual.simulate_keystrokes("ctrl-alt-m");

    let (commands, terminal_keys) = received(&probe, visual);
    assert_eq!(commands, vec!["createTask", "createTask"]);
    assert_eq!(terminal_keys, vec![SharedString::from("ctrl-alt-n")]);
}

// parity: TS-main-desktopShortcutRouting-25f491
#[gpui::test]
fn capturing_window_passes_combos_through_before_command_lookup(cx: &mut TestAppContext) {
    init_app(cx, None);
    let (probe, visual) = open_screen_probe(cx, false);
    visual.update(|window, cx| begin_shortcut_capture(window.window_handle().window_id(), cx));

    visual.simulate_keystrokes("ctrl-n");
    visual.simulate_keystrokes("ctrl-shift-n");

    let (commands, terminal_keys) = received(&probe, visual);
    assert!(commands.is_empty(), "no command runs while recording");
    assert_eq!(
        terminal_keys,
        vec![
            SharedString::from("ctrl-n"),
            SharedString::from("ctrl-shift-n")
        ]
    );
}

// parity: TS-main-desktopShortcutRouting-88dded
#[gpui::test]
fn closing_a_capturing_window_clears_its_capture_mark(cx: &mut TestAppContext) {
    init_app(cx, None);
    let window = open_app_window(cx, "/ko/settings/shortcuts");
    cx.update(|cx| begin_shortcut_capture(window.window_id(), cx));
    assert_eq!(capturing_window_count(cx), 1);

    window
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();

    assert_eq!(capturing_window_count(cx), 0);
}

// parity: TS-main-desktopShortcutRouting-0677b2
#[gpui::test]
fn route_change_clears_the_capture_mark_of_its_window(cx: &mut TestAppContext) {
    init_app(cx, None);
    let window = open_app_window(cx, "/ko/settings/shortcuts");
    cx.update(|cx| begin_shortcut_capture(window.window_id(), cx));

    window
        .update(cx, |app_window, _, cx| app_window.navigate("/ko", cx))
        .unwrap();

    assert_eq!(capturing_window_count(cx), 0);
}

// parity: TS-main-desktopShortcutRouting-617139
#[gpui::test]
fn reloaded_rebinding_applies_to_every_open_window(cx: &mut TestAppContext) {
    let data_dir = tempfile::tempdir().unwrap();
    init_app(cx, Some(data_dir.path().to_path_buf()));
    let (first, _) = open_screen_probe(cx, false);
    let first_window = cx.update(|cx| cx.windows()[0]);
    let (second, _) = open_screen_probe(cx, false);
    let second_window = cx.update(|cx| cx.windows()[1]);

    save_shortcut_bindings(data_dir.path(), r#"{"createTask":"Mod+Alt+M"}"#);
    cx.update(reload_shortcut_bindings);
    for window in [first_window, second_window] {
        VisualTestContext::from_window(window, cx).simulate_keystrokes("ctrl-alt-m");
    }

    for probe in [first, second] {
        let commands = cx.read_entity(&probe, |probe, _| probe.received_commands.clone());
        assert_eq!(commands, vec!["createTask"]);
    }
}

// parity: TS-main-desktopShortcutRouting-e2122f
#[gpui::test]
fn mod_r_is_swallowed_without_a_refresh_command(cx: &mut TestAppContext) {
    init_app(cx, None);
    let (probe, visual) = open_screen_probe(cx, true);

    visual.simulate_keystrokes("ctrl-r");

    assert_eq!(received(&probe, visual), (Vec::new(), Vec::new()));
    let ctrl_r = shortcut_event_from_keystroke(&gpui_kit::Keystroke::parse("ctrl-r").unwrap());
    let command = cx.update(|cx| {
        let state = cx.global::<ShortcutState>();
        state
            .bindings
            .find_command_for_event(&ctrl_r, state.platform)
    });
    assert_eq!(
        command, None,
        "Mod+R is blocked, not bound to a refresh command"
    );
}

#[gpui::test]
fn internal_links_focus_the_window_already_on_that_route(cx: &mut TestAppContext) {
    init_app(cx, None);
    let board = open_app_window(cx, "/ko");
    let task_window = open_app_window(cx, "/ko/task/task-1");
    let window_count = cx.update(|cx| cx.windows().len());

    let action = cx.update(|cx| open_href("#/ko/task/task-1", Some(board.window_id()), cx));
    assert_eq!(
        action,
        WindowOpenAction::FocusExisting {
            route: "/ko/task/task-1".into(),
            existing_window: task_window.window_id()
        }
    );

    let action = cx.update(|cx| open_href("/ko/task/task-1", Some(task_window.window_id()), cx));
    cx.run_until_parked();
    assert_eq!(
        action,
        WindowOpenAction::OpenInternal {
            route: "/ko/task/task-1".into()
        }
    );
    assert_eq!(cx.update(|cx| cx.windows().len()), window_count + 1);
}
