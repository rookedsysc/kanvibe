use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, RecvTimeoutError, TryRecvError, channel},
    },
    time::Duration,
};

use gpui::{
    AnyElement, App, Application, Bounds, Context, Entity, Focusable, IntoElement, KeyBinding,
    KeyDownEvent, Menu, MenuItem, Render, SharedString, TitlebarOptions, Window, WindowAppearance,
    WindowBounds, WindowOptions, div, prelude::*, px, rgb, rgba, size,
};
use gpui_component::{
    Disableable, Root, Selectable,
    button::{Button, ButtonVariants},
    input::{Input, InputState},
    menu::{ContextMenuExt, PopupMenuItem},
    scroll::ScrollableElement,
    theme::{ActiveTheme, Theme, ThemeMode},
};
use gpui_terminal::{TerminalConfig, TerminalView};
use kanvibe_ai::{AiMessageRole, AiSessionDetail, AiSessionProvider, AiSessionsPage};
use kanvibe_core::{
    DONE_PAGE_SIZE, KanvibeDb, PaneLayoutConfig, PaneLayoutType, SessionType, TaskPriority,
    TaskStatus, ThemePreference,
};
use kanvibe_hooks::{AppNotification, HookProviderStatus};
use kanvibe_pty::{
    PtyController, ShellPlatform, SpawnedPty, build_task_session_pty_request,
    create_local_shell_environment, spawn_pty,
};
use kanvibe_theme::{
    PRIMARY, PROJECT_COLOR_FALLBACK, Rgb, TAG_BRANCH_TEXT, TAG_SESSION_TEXT, TAG_SSH_TEXT,
    agent_tag_color, priority_tag_color,
};

const DIFF_SIDEBAR_DEFAULT_WIDTH: f32 = 280.0;
const DIFF_SIDEBAR_MIN_WIDTH: f32 = 200.0;
const DIFF_SIDEBAR_MAX_WIDTH: f32 = 480.0;
const DIFF_SIDEBAR_RESIZE_STEP: f32 = 40.0;
const PANE_LAYOUT_TYPES: &[(PaneLayoutType, &str)] = &[
    (PaneLayoutType::Single, "Single"),
    (PaneLayoutType::Horizontal2, "Top / Bottom"),
    (PaneLayoutType::Vertical2, "Left / Right"),
    (PaneLayoutType::LeftRightTb, "Left + Right split"),
    (PaneLayoutType::LeftTbRight, "Left split + Right"),
    (PaneLayoutType::Quad, "Quad"),
];

fn pane_position_label(layout_type: PaneLayoutType, position: u32) -> &'static str {
    match (layout_type, position) {
        (PaneLayoutType::Single, 0) => "Pane",
        (PaneLayoutType::Horizontal2, 0) => "Top",
        (PaneLayoutType::Horizontal2, 1) => "Bottom",
        (PaneLayoutType::Vertical2, 0) => "Left",
        (PaneLayoutType::Vertical2, 1) => "Right",
        (PaneLayoutType::LeftRightTb, 0) => "Left",
        (PaneLayoutType::LeftRightTb, 1) => "Right top",
        (PaneLayoutType::LeftRightTb, 2) => "Right bottom",
        (PaneLayoutType::LeftTbRight, 0) => "Left top",
        (PaneLayoutType::LeftTbRight, 1) => "Left bottom",
        (PaneLayoutType::LeftTbRight, 2) => "Right",
        (PaneLayoutType::Quad, 0) => "Top left",
        (PaneLayoutType::Quad, 1) => "Top right",
        (PaneLayoutType::Quad, 2) => "Bottom left",
        (PaneLayoutType::Quad, 3) => "Bottom right",
        _ => "Pane",
    }
}

use crate::{
    INITIAL_NATIVE_LOAD_TIMEOUT_MS, NativeBackgroundSyncService, NativeGitHubCliStatus,
    NativeNavigationHistory, NativeNotificationService, NativeReleaseUpdate, NativeRoute,
    NativeSessionDependencyStatus, NativeSettingsPatch, NativeShortcutCapture,
    NativeSystemAppearance, NativeTaskFormInput, NativeUiCardSpec, NativeUiColumnSpec,
    NativeUiLaunchConfig, NativeUiRenderSpec, NativeVimCommand, PROJECT_COLOR_PRESETS,
    SettingsShell, ShortcutPlatform, branch_native_task_from_form, build_native_ui_render_spec,
    build_settings_shell, build_unavailable_native_ui_render_spec, complete_native_vim_command,
    connect_native_task_terminal, create_native_task_from_form, cycle_native_pane_layout,
    delete_native_project, delete_native_task, dismiss_native_release_update,
    dismiss_native_sidebar_hint, filter_native_cards_by_projects, get_native_github_cli_status,
    get_native_task_hook_status, get_native_task_session_dependency_status,
    install_native_github_cli, install_native_task_hooks, install_native_task_session_dependency,
    load_native_diff_snapshot, load_native_task_ai_session_detail, load_native_task_ai_sessions,
    load_read_only_board_with_done_limit, move_native_task, native_diagnostic_line,
    native_gpui_keybinding, native_notification_platform, native_release_update_service,
    native_shortcut_from_capture, native_theme_is_dark, ordered_task_ids_for_drop,
    parse_native_route, parse_native_vim_command, register_native_local_project,
    register_native_remote_project, reset_native_project_pane_layout, save_native_diff_file,
    save_native_pane_layout_type, scan_and_register_native_local_projects,
    should_keep_current_route_for_notification_activation,
    spawn_native_hook_server_with_notifications, start_native_background_sync_with_notifications,
    task_detail_dock_items, update_native_pane_command, update_native_project_color,
    update_native_settings, update_native_task_metadata, update_native_task_status,
};

gpui::actions!(
    kanvibe,
    [
        NewWindow,
        CreateTask,
        BoardFind,
        OpenProjectFilter,
        QuickSearch,
        VimNewTask,
        VimCommandPalette,
        VimDeleteFocused,
        FocusNextTask,
        FocusPreviousTask,
        FocusNextTaskArrow,
        FocusPreviousTaskArrow,
        OpenFocusedTask,
        OpenFocusedTaskNewWindow,
        OpenFocusedContextMenu,
        SaveActiveEditor,
        NavigateBack,
        NavigateForward,
        OpenSettings,
        OpenPaneLayouts,
        OpenNotifications,
        DismissOverlay,
        SubmitTaskEditor,
        ApplyQuickSearch,
        CompleteVimCommand,
        SubmitVimCommand,
        ConfirmDone
    ]
);

fn bind_native_keys(cx: &mut App, task_search_shortcut: &str) {
    let task_search = native_gpui_keybinding(task_search_shortcut, ShortcutPlatform::Mac)
        .unwrap_or_else(|_| "cmd-shift-o".to_owned());
    cx.clear_key_bindings();
    cx.bind_keys([
        KeyBinding::new("cmd-shift-n", NewWindow, None),
        KeyBinding::new("cmd-n", CreateTask, None),
        KeyBinding::new("cmd-f", BoardFind, None),
        KeyBinding::new("cmd-shift-p", OpenProjectFilter, Some("Board")),
        KeyBinding::new(&task_search, QuickSearch, None),
        KeyBinding::new("cmd-shift-[", NavigateBack, None),
        KeyBinding::new("cmd-shift-]", NavigateForward, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-shift-i", OpenNotifications, None),
        KeyBinding::new("escape", DismissOverlay, Some("Modal")),
        KeyBinding::new("enter", SubmitTaskEditor, Some("TaskEditor")),
        KeyBinding::new("enter", ApplyQuickSearch, Some("QuickSearch")),
        KeyBinding::new("tab", CompleteVimCommand, Some("VimCommand")),
        KeyBinding::new("enter", SubmitVimCommand, Some("VimCommand")),
        KeyBinding::new("enter", ConfirmDone, Some("DoneConfirmation")),
        KeyBinding::new("n", VimNewTask, Some("Board")),
        KeyBinding::new(":", VimCommandPalette, Some("Board")),
        KeyBinding::new("d d", VimDeleteFocused, Some("Board")),
        KeyBinding::new("j", FocusNextTask, Some("Board")),
        KeyBinding::new("down", FocusNextTaskArrow, Some("Board")),
        KeyBinding::new("k", FocusPreviousTask, Some("Board")),
        KeyBinding::new("up", FocusPreviousTaskArrow, Some("Board")),
        KeyBinding::new("enter", OpenFocusedTask, Some("Board")),
        KeyBinding::new("shift-enter", OpenFocusedTaskNewWindow, Some("Board")),
        KeyBinding::new("shift-f10", OpenFocusedContextMenu, Some("Board")),
        KeyBinding::new("cmd-s", SaveActiveEditor, None),
    ]);
}

const fn native_system_appearance(appearance: WindowAppearance) -> NativeSystemAppearance {
    match appearance {
        WindowAppearance::Dark | WindowAppearance::VibrantDark => NativeSystemAppearance::Dark,
        WindowAppearance::Light | WindowAppearance::VibrantLight => NativeSystemAppearance::Light,
    }
}

fn apply_native_theme(
    preference: ThemePreference,
    appearance: WindowAppearance,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    let mode = if native_theme_is_dark(preference, native_system_appearance(appearance)) {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    Theme::change(mode, window, cx);
}

#[derive(Clone)]
struct TaskDragInfo {
    task_id: String,
    title: String,
}

#[derive(Clone)]
enum PendingDoneMutation {
    Status {
        task_id: String,
    },
    Move {
        task_id: String,
        destination_ids: Vec<String>,
        error_event: &'static str,
    },
}

impl Render for TaskDragInfo {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(320.0))
            .rounded_md()
            .border_1()
            .border_color(rgb(0x0064ff))
            .bg(rgb(0x111827))
            .p_3()
            .child(self.title.clone())
    }
}

pub fn run_native_ui() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = NativeUiLaunchConfig::from_env()?;
    let database_path = config.ensure_database_file()?;
    let notification_revision = Arc::new(AtomicU64::new(0));
    let notifications = Arc::new(NativeNotificationService::new(
        database_path.clone(),
        Arc::clone(&notification_revision),
        native_notification_platform()?,
    ));
    let (_hook_server, hook_revision) = spawn_native_hook_server_with_notifications(
        database_path.clone(),
        Some(Arc::clone(&notifications)),
        config.locale,
    )?;
    let background_sync = Arc::new(start_native_background_sync_with_notifications(
        database_path,
        Arc::clone(&hook_revision),
        Some(Arc::clone(&notifications)),
        config.locale,
    )?);
    let state = load_native_state(&config);
    let (qa_sender, qa_receiver) = crate::qa_control::qa_runtime_channel();
    let qa_receiver = Arc::new(Mutex::new(qa_receiver));
    let _qa_socket = crate::qa_control::spawn_debug_qa_runtime_socket_from_env(qa_sender)?;
    let app = Application::new();
    let reopen_config = config.clone();
    let reopen_state = state.clone();
    let reopen_hook_revision = Arc::clone(&hook_revision);
    let reopen_background_sync = Arc::clone(&background_sync);
    let reopen_notification_revision = Arc::clone(&notification_revision);
    let reopen_notifications = Arc::clone(&notifications);
    let reopen_qa_receiver = Arc::clone(&qa_receiver);
    app.on_reopen(move |cx| {
        if cx.windows().is_empty() {
            open_native_window(
                cx,
                reopen_config.clone(),
                reopen_state.clone(),
                NativeRoute::Board {
                    locale: reopen_state.spec.locale,
                },
                Arc::clone(&reopen_hook_revision),
                Arc::clone(&reopen_background_sync),
                Arc::clone(&reopen_notification_revision),
                Arc::clone(&reopen_notifications),
                Arc::clone(&reopen_qa_receiver),
            );
        }
    });

    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        let task_search_shortcut = state
            .settings
            .as_ref()
            .map(|settings| settings.task_search_shortcut.as_str())
            .unwrap_or("Mod+Shift+O");
        bind_native_keys(cx, task_search_shortcut);
        cx.set_menus(vec![Menu {
            name: "KanVibe".into(),
            items: vec![
                MenuItem::action("New Window", NewWindow),
                MenuItem::separator(),
                MenuItem::action("Settings", OpenSettings),
                MenuItem::action("Pane Layout", OpenPaneLayouts),
                MenuItem::action("Notifications", OpenNotifications),
            ],
        }]);

        let new_window_config = config.clone();
        let new_window_state = state.clone();
        let new_window_hook_revision = Arc::clone(&hook_revision);
        let new_window_background_sync = Arc::clone(&background_sync);
        let new_window_notification_revision = Arc::clone(&notification_revision);
        let new_window_notifications = Arc::clone(&notifications);
        let new_window_qa_receiver = Arc::clone(&qa_receiver);
        cx.on_action(move |_: &NewWindow, cx| {
            open_native_window(
                cx,
                new_window_config.clone(),
                new_window_state.clone(),
                NativeRoute::Board {
                    locale: new_window_state.spec.locale,
                },
                Arc::clone(&new_window_hook_revision),
                Arc::clone(&new_window_background_sync),
                Arc::clone(&new_window_notification_revision),
                Arc::clone(&new_window_notifications),
                Arc::clone(&new_window_qa_receiver),
            );
        });

        let initial_route = parse_native_route(&state.spec.route);
        open_native_window(
            cx,
            config,
            state,
            initial_route,
            hook_revision,
            background_sync,
            notification_revision,
            notifications,
            qa_receiver,
        );
        cx.activate(true);
        if let Err(error) = crate::native_updater::acknowledge_update_health_from_process_args(
            env!("CARGO_PKG_VERSION"),
        ) {
            eprintln!("native update health acknowledgement failed: {error}");
        }
    });

    Ok(())
}

#[derive(Clone)]
struct NativeLoadedState {
    spec: NativeUiRenderSpec,
    settings: Result<SettingsShell, String>,
    pane_layouts: Result<Vec<PaneLayoutConfig>, String>,
    startup_error: Option<String>,
    done_limit: u32,
}

fn load_native_state(config: &NativeUiLaunchConfig) -> NativeLoadedState {
    load_native_state_with_done_limit(config, DONE_PAGE_SIZE)
}

fn load_native_state_with_done_limit(
    config: &NativeUiLaunchConfig,
    done_limit: u32,
) -> NativeLoadedState {
    let worker_config = config.clone();
    let (sender, receiver) = channel();
    std::thread::spawn(move || {
        let _ = sender.send(load_native_state_unbounded(&worker_config, done_limit));
    });

    match receiver.recv_timeout(Duration::from_millis(INITIAL_NATIVE_LOAD_TIMEOUT_MS)) {
        Ok(state) => state,
        Err(RecvTimeoutError::Timeout) => {
            let error = format!(
                "Initial native data load timed out after {INITIAL_NATIVE_LOAD_TIMEOUT_MS} ms"
            );
            log_native_failure(
                config,
                "initial-load-timeout",
                &error,
                Some(INITIAL_NATIVE_LOAD_TIMEOUT_MS),
            );
            unavailable_native_state(config, error, done_limit)
        }
        Err(RecvTimeoutError::Disconnected) => {
            let error = "Initial native data loader stopped unexpectedly".to_owned();
            log_native_failure(config, "initial-load-disconnected", &error, None);
            unavailable_native_state(config, error, done_limit)
        }
    }
}

fn load_native_state_unbounded(
    config: &NativeUiLaunchConfig,
    done_limit: u32,
) -> NativeLoadedState {
    let loaded = (|| -> Result<_, Box<dyn Error + Send + Sync>> {
        let database_path = config.ensure_database_file()?;
        let bootstrap = load_read_only_board_with_done_limit(
            &config.repo_root,
            &database_path,
            config.locale,
            done_limit,
        )?;
        let spec = build_native_ui_render_spec(&bootstrap);
        let database = KanvibeDb::open_read_only(&database_path)?;
        let settings = build_settings_shell(&database, Some(config.locale.code()))
            .map_err(|error| error.to_string());
        let pane_layouts = database
            .get_all_pane_layouts()
            .map_err(|error| error.to_string());
        Ok((spec, settings, pane_layouts))
    })();

    match loaded {
        Ok((spec, settings, pane_layouts)) => NativeLoadedState {
            spec,
            settings,
            pane_layouts,
            startup_error: None,
            done_limit,
        },
        Err(error) => {
            let error = error.to_string();
            log_native_failure(config, "startup-error", &error, None);
            unavailable_native_state(config, error, done_limit)
        }
    }
}

fn log_native_failure(
    config: &NativeUiLaunchConfig,
    event: &str,
    message: &str,
    timeout_ms: Option<u64>,
) {
    let route = format!("/{}", config.locale.code());
    eprintln!(
        "{}",
        native_diagnostic_line(event, &route, message, timeout_ms)
    );
}

fn unavailable_native_state(
    config: &NativeUiLaunchConfig,
    error: String,
    done_limit: u32,
) -> NativeLoadedState {
    NativeLoadedState {
        spec: build_unavailable_native_ui_render_spec(config.locale),
        settings: Err(error.clone()),
        pane_layouts: Err(error.clone()),
        startup_error: Some(error),
        done_limit,
    }
}

fn open_native_window(
    cx: &mut App,
    config: NativeUiLaunchConfig,
    state: NativeLoadedState,
    route: NativeRoute,
    hook_revision: Arc<AtomicU64>,
    background_sync: Arc<NativeBackgroundSyncService>,
    notification_revision: Arc<AtomicU64>,
    notifications: Arc<NativeNotificationService>,
    qa_receiver: Arc<Mutex<Receiver<crate::qa_control::QaRuntimeRequest>>>,
) {
    let bounds = Bounds::centered(None, size(px(1280.0), px(860.0)), cx);
    cx.open_window(
        WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::from(state.spec.window_title.clone())),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |window, cx| {
            let initial_appearance = window.appearance();
            let theme_preference = state
                .settings
                .as_ref()
                .map(|settings| settings.theme_preference)
                .unwrap_or(ThemePreference::System);
            apply_native_theme(theme_preference, initial_appearance, Some(window), cx);
            let task_title_input =
                cx.new(|cx| InputState::new(window, cx).placeholder("Task title"));
            let task_description_input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Description")
                    .auto_grow(3, 6)
            });
            let task_base_branch_input =
                cx.new(|cx| InputState::new(window, cx).placeholder("Base branch"));
            let board_search_input =
                cx.new(|cx| InputState::new(window, cx).placeholder("Search task titles"));
            let quick_search_input =
                cx.new(|cx| InputState::new(window, cx).placeholder("Search all tasks"));
            let ai_search_input =
                cx.new(|cx| InputState::new(window, cx).placeholder("Search AI sessions"));
            let vim_command_input =
                cx.new(|cx| InputState::new(window, cx).placeholder(":move progress"));
            let pane_command_input =
                cx.new(|cx| InputState::new(window, cx).placeholder("Pane command"));
            let task_search_shortcut = state
                .settings
                .as_ref()
                .map(|settings| settings.task_search_shortcut.clone())
                .unwrap_or_else(|_| "Mod+Shift+O".to_owned());
            let task_search_shortcut_input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Mod+Shift+O")
                    .default_value(task_search_shortcut)
            });
            let project_name_input =
                cx.new(|cx| InputState::new(window, cx).placeholder("Project name"));
            let project_path_input =
                cx.new(|cx| InputState::new(window, cx).placeholder("/path/to/repository"));
            let project_ssh_host_input =
                cx.new(|cx| InputState::new(window, cx).placeholder("SSH host (optional)"));
            let project_scan_root_input =
                cx.new(|cx| InputState::new(window, cx).placeholder("/path/to/scan"));
            let diff_editor_input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("File contents")
                    .auto_grow(12, 28)
            });
            let root_qa_receiver = Arc::clone(&qa_receiver);
            let task_sidebar_collapsed = state
                .settings
                .as_ref()
                .map(|settings| settings.sidebar_default_collapsed)
                .unwrap_or(false);
            let view = cx.new(|cx| {
                let qa_capture_state = crate::qa_control::QaControlState::new(state.spec.clone());
                let mut root = KanVibeRoot {
                    config,
                    spec: state.spec,
                    navigation: NativeNavigationHistory::new(route),
                    settings: state.settings,
                    pane_layouts: state.pane_layouts,
                    startup_error: state.startup_error,
                    task_title_input,
                    task_description_input,
                    task_base_branch_input,
                    board_search_input,
                    quick_search_input,
                    ai_search_input,
                    vim_command_input,
                    pane_command_input,
                    task_search_shortcut_input,
                    task_search_shortcut_recording: false,
                    project_name_input,
                    project_path_input,
                    project_ssh_host_input,
                    project_scan_root_input,
                    diff_editor_input,
                    show_task_editor: false,
                    task_editor_submitting: false,
                    editing_task_id: None,
                    branching_task_id: None,
                    delete_confirmation_task_id: None,
                    pending_done_mutation: None,
                    done_confirm_dont_ask_again: false,
                    mutation_error: None,
                    board_search_query: String::new(),
                    show_board_find: false,
                    show_project_filter: false,
                    quick_search_query: String::new(),
                    show_quick_search: false,
                    show_vim_command: false,
                    show_keyboard_context_menu: false,
                    focused_task_id: None,
                    selected_task_dock_item: None,
                    terminals: BTreeMap::new(),
                    terminal_controllers: BTreeMap::new(),
                    terminal_errors: BTreeMap::new(),
                    session_dependency_statuses: BTreeMap::new(),
                    session_dependency_loading: BTreeSet::new(),
                    task_hook_statuses: BTreeMap::new(),
                    task_hook_loading: BTreeSet::new(),
                    task_sidebar_collapsed,
                    restore_terminal_focus_on_render: false,
                    ai_sessions: BTreeMap::new(),
                    ai_sessions_loading: BTreeSet::new(),
                    ai_session_details: BTreeMap::new(),
                    ai_session_details_loading: BTreeSet::new(),
                    ai_provider_filters: AiSessionProvider::ALL.into_iter().collect(),
                    ai_role_filters: BTreeSet::new(),
                    ai_session_query: String::new(),
                    github_cli_statuses: BTreeMap::new(),
                    github_cli_loading: BTreeSet::new(),
                    project_color_updating: BTreeSet::new(),
                    background_review_open: false,
                    notification_center_open: false,
                    notification_snapshot: notifications.list(),
                    release_update: None,
                    release_update_dont_show_again: false,
                    release_update_checking: false,
                    release_update_installing: false,
                    release_update_error: None,
                    selected_merged_task_ids: BTreeSet::new(),
                    selected_diff_file_path: None,
                    diff_snapshots: BTreeMap::new(),
                    diff_snapshots_loading: BTreeSet::new(),
                    viewed_diff_files: BTreeSet::new(),
                    diff_sidebar_width: DIFF_SIDEBAR_DEFAULT_WIDTH,
                    editing_pane_command: None,
                    show_project_editor: false,
                    project_registration_in_progress: false,
                    project_delete_confirmation_id: None,
                    project_scan_summary: None,
                    editing_diff_file: None,
                    diff_editor_saving: false,
                    selected_project_ids: BTreeSet::new(),
                    done_limit: state.done_limit,
                    task_form_project_id: None,
                    task_form_priority: None,
                    task_form_session_type: SessionType::Tmux,
                    hook_revision,
                    background_sync,
                    notification_revision,
                    notifications,
                    qa_capture_state,
                    qa_receiver: root_qa_receiver,
                };
                cx.observe_window_appearance(window, |root: &mut KanVibeRoot, window, cx| {
                    let preference = root
                        .settings
                        .as_ref()
                        .map(|settings| settings.theme_preference)
                        .unwrap_or(ThemePreference::System);
                    apply_native_theme(preference, window.appearance(), Some(window), cx);
                    cx.notify();
                })
                .detach();
                root.watch_hook_revision(cx);
                root.watch_release_updates(cx);
                if let NativeRoute::TaskDetail { task_id, .. } = root.navigation.current().clone() {
                    root.load_task_ai_sessions(task_id, cx);
                }
                if let NativeRoute::Diff { task_id, .. } = root.navigation.current().clone() {
                    root.load_diff_snapshot(task_id, false, cx);
                }
                root
            });
            watch_qa_runtime(window, cx, view.clone(), Arc::clone(&qa_receiver));
            cx.new(|cx| Root::new(view, window, cx))
        },
    )
    .expect("failed to open KanVibe native window");
}

fn watch_qa_runtime(
    window: &Window,
    cx: &App,
    view: Entity<KanVibeRoot>,
    receiver: Arc<Mutex<Receiver<crate::qa_control::QaRuntimeRequest>>>,
) {
    window
        .spawn(cx, async move |window| {
            loop {
                window
                    .background_executor()
                    .timer(Duration::from_millis(20))
                    .await;
                let request = receiver
                    .lock()
                    .ok()
                    .and_then(|receiver| receiver.try_recv().ok());
                let Some(request) = request else {
                    continue;
                };
                let command = request.command.clone();
                let response = view
                    .update_in(window, move |root, window, cx| {
                        root.handle_runtime_qa_command(command, window, cx)
                    })
                    .unwrap_or_else(|error| crate::qa_control::QaControlResponse::Error {
                        message: format!("GPUI QA target window is unavailable: {error}"),
                    });
                request.respond(response);
            }
        })
        .detach();
}

struct KanVibeRoot {
    config: NativeUiLaunchConfig,
    spec: NativeUiRenderSpec,
    navigation: NativeNavigationHistory,
    settings: Result<SettingsShell, String>,
    pane_layouts: Result<Vec<PaneLayoutConfig>, String>,
    startup_error: Option<String>,
    task_title_input: Entity<InputState>,
    task_description_input: Entity<InputState>,
    task_base_branch_input: Entity<InputState>,
    board_search_input: Entity<InputState>,
    quick_search_input: Entity<InputState>,
    ai_search_input: Entity<InputState>,
    vim_command_input: Entity<InputState>,
    pane_command_input: Entity<InputState>,
    task_search_shortcut_input: Entity<InputState>,
    task_search_shortcut_recording: bool,
    project_name_input: Entity<InputState>,
    project_path_input: Entity<InputState>,
    project_ssh_host_input: Entity<InputState>,
    project_scan_root_input: Entity<InputState>,
    diff_editor_input: Entity<InputState>,
    show_task_editor: bool,
    task_editor_submitting: bool,
    editing_task_id: Option<String>,
    branching_task_id: Option<String>,
    delete_confirmation_task_id: Option<String>,
    pending_done_mutation: Option<PendingDoneMutation>,
    done_confirm_dont_ask_again: bool,
    mutation_error: Option<String>,
    board_search_query: String,
    show_board_find: bool,
    show_project_filter: bool,
    quick_search_query: String,
    show_quick_search: bool,
    show_vim_command: bool,
    show_keyboard_context_menu: bool,
    focused_task_id: Option<String>,
    selected_task_dock_item: Option<String>,
    terminals: BTreeMap<String, Entity<TerminalView>>,
    terminal_controllers: BTreeMap<String, PtyController>,
    terminal_errors: BTreeMap<String, String>,
    session_dependency_statuses: BTreeMap<String, Result<NativeSessionDependencyStatus, String>>,
    session_dependency_loading: BTreeSet<String>,
    task_hook_statuses: BTreeMap<String, Result<Vec<HookProviderStatus>, String>>,
    task_hook_loading: BTreeSet<String>,
    task_sidebar_collapsed: bool,
    restore_terminal_focus_on_render: bool,
    ai_sessions: BTreeMap<String, Result<AiSessionsPage, String>>,
    ai_sessions_loading: BTreeSet<String>,
    ai_session_details: BTreeMap<(String, String), Result<AiSessionDetail, String>>,
    ai_session_details_loading: BTreeSet<(String, String)>,
    ai_provider_filters: BTreeSet<AiSessionProvider>,
    ai_role_filters: BTreeSet<AiMessageRole>,
    ai_session_query: String,
    github_cli_statuses: BTreeMap<String, Result<NativeGitHubCliStatus, String>>,
    github_cli_loading: BTreeSet<String>,
    project_color_updating: BTreeSet<String>,
    background_review_open: bool,
    notification_center_open: bool,
    notification_snapshot: Result<Vec<AppNotification>, String>,
    release_update: Option<NativeReleaseUpdate>,
    release_update_dont_show_again: bool,
    release_update_checking: bool,
    release_update_installing: bool,
    release_update_error: Option<String>,
    selected_merged_task_ids: BTreeSet<String>,
    selected_diff_file_path: Option<String>,
    diff_snapshots: BTreeMap<String, Result<crate::NativeDiffSnapshot, String>>,
    diff_snapshots_loading: BTreeSet<String>,
    viewed_diff_files: BTreeSet<(String, String)>,
    diff_sidebar_width: f32,
    editing_pane_command: Option<(String, u32)>,
    show_project_editor: bool,
    project_registration_in_progress: bool,
    project_delete_confirmation_id: Option<String>,
    project_scan_summary: Option<String>,
    editing_diff_file: Option<(String, String, String)>,
    diff_editor_saving: bool,
    selected_project_ids: BTreeSet<String>,
    done_limit: u32,
    task_form_project_id: Option<String>,
    task_form_priority: Option<TaskPriority>,
    task_form_session_type: SessionType,
    hook_revision: Arc<AtomicU64>,
    background_sync: Arc<NativeBackgroundSyncService>,
    notification_revision: Arc<AtomicU64>,
    notifications: Arc<NativeNotificationService>,
    qa_capture_state: crate::qa_control::QaControlState,
    qa_receiver: Arc<Mutex<Receiver<crate::qa_control::QaRuntimeRequest>>>,
}

impl Drop for KanVibeRoot {
    fn drop(&mut self) {
        for controller in self.terminal_controllers.values() {
            let _ = controller.terminate();
        }
    }
}

impl Render for KanVibeRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.restore_terminal_focus_if_requested(window, cx);
        let content = match self.navigation.current() {
            NativeRoute::Board { .. } => self.render_board(cx),
            NativeRoute::Settings { .. } => self.render_settings(cx).into_any_element(),
            NativeRoute::PaneLayout { .. } => self.render_pane_layouts(cx).into_any_element(),
            NativeRoute::TaskDetail { task_id, .. } => {
                self.render_task_detail(task_id, cx).into_any_element()
            }
            NativeRoute::Diff { task_id, .. } => {
                self.render_diff_loading(task_id, cx).into_any_element()
            }
            NativeRoute::NotFound { path, .. } => {
                self.render_not_found(path, cx).into_any_element()
            }
        };

        div()
            .size_full()
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.capture_task_search_shortcut(event, window, cx);
            }))
            .key_context(if self.has_open_overlay() {
                "Modal"
            } else {
                "App"
            })
            .on_action(cx.listener(|this, _: &NavigateBack, _, cx| this.go_back(cx)))
            .on_action(cx.listener(|this, _: &NavigateForward, _, cx| {
                this.go_forward(cx);
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| {
                this.navigate(
                    NativeRoute::Settings {
                        locale: this.navigation.current().locale(),
                    },
                    cx,
                );
            }))
            .on_action(cx.listener(|this, _: &OpenPaneLayouts, _, cx| {
                this.navigate(
                    NativeRoute::PaneLayout {
                        locale: this.navigation.current().locale(),
                    },
                    cx,
                );
            }))
            .on_action(cx.listener(|this, _: &OpenNotifications, _, cx| {
                this.notification_center_open = !this.notification_center_open;
                this.notification_snapshot = this.notifications.list();
                if !this.notification_center_open {
                    this.request_terminal_focus_restore();
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &DismissOverlay, _, cx| {
                this.dismiss_top_overlay(cx);
            }))
            .on_action(cx.listener(|this, _: &SubmitTaskEditor, window, cx| {
                let description_is_focused = this
                    .task_description_input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window);
                if !description_is_focused {
                    this.submit_task_editor(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ApplyQuickSearch, _, cx| {
                this.apply_quick_search(cx);
            }))
            .on_action(cx.listener(|this, _: &SubmitVimCommand, _, cx| {
                this.submit_vim_command(cx);
            }))
            .on_action(cx.listener(|this, _: &CompleteVimCommand, window, cx| {
                this.complete_vim_command(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ConfirmDone, _, cx| {
                this.confirm_done_mutation(cx);
            }))
            .on_action(cx.listener(|this, _: &CreateTask, window, cx| {
                this.open_task_editor(window, cx);
            }))
            .on_action(cx.listener(|this, _: &BoardFind, window, cx| {
                this.focus_board_search(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenProjectFilter, _, cx| {
                this.toggle_project_filter_visibility(cx);
            }))
            .on_action(cx.listener(|this, _: &QuickSearch, window, cx| {
                this.open_quick_search(window, cx);
            }))
            .on_action(cx.listener(|this, _: &VimNewTask, window, cx| {
                if this.vim_shortcuts_available() {
                    this.open_task_editor(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &VimCommandPalette, window, cx| {
                this.open_vim_command(window, cx);
            }))
            .on_action(cx.listener(|this, _: &VimDeleteFocused, _, cx| {
                this.delete_vim_focused_task(cx);
            }))
            .on_action(cx.listener(|this, _: &FocusNextTask, _, cx| {
                this.move_vim_focus(1, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusPreviousTask, _, cx| {
                this.move_vim_focus(-1, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusNextTaskArrow, _, cx| {
                this.move_board_focus(1, false, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusPreviousTaskArrow, _, cx| {
                this.move_board_focus(-1, false, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenFocusedTask, _, cx| {
                this.open_focused_task(false, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenFocusedTaskNewWindow, _, cx| {
                this.open_focused_task(true, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenFocusedContextMenu, _, cx| {
                this.open_keyboard_context_menu(cx);
            }))
            .on_action(cx.listener(|this, _: &SaveActiveEditor, _, cx| {
                this.submit_diff_editor(cx);
            }))
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .p_4()
            .child(self.render_navigation(cx))
            .child(self.render_task_editor(cx))
            .child(self.render_quick_search(cx))
            .child(self.render_vim_command(cx))
            .child(self.render_keyboard_context_menu(cx))
            .child(self.render_done_confirmation(cx))
            .child(self.render_pane_command_editor(cx))
            .child(self.render_project_editor(cx))
            .child(self.render_diff_editor(cx))
            .child(self.render_notification_center(cx))
            .child(self.render_release_update(cx))
            .child(self.render_background_sync_review(cx))
            .child(content)
    }
}

impl KanVibeRoot {
    fn handle_runtime_qa_command(
        &mut self,
        command: crate::qa_control::QaControlCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> crate::qa_control::QaControlResponse {
        use crate::qa_control::{QaControlCommand, QaControlResponse};

        match command {
            QaControlCommand::SyntheticClick { id, payload, .. } => {
                let accepted = match id.as_str() {
                    "board.primaryAction" => {
                        window.dispatch_action(Box::new(CreateTask), cx);
                        true
                    }
                    "app.settingsButton" => {
                        window.dispatch_action(Box::new(OpenSettings), cx);
                        true
                    }
                    "settings.paneLayout" => {
                        if let Some(project_id) = payload
                            .as_ref()
                            .and_then(|value| value["projectId"].as_str())
                        {
                            self.selected_project_ids.clear();
                            self.selected_project_ids.insert(project_id.to_owned());
                        }
                        window.dispatch_action(Box::new(OpenPaneLayouts), cx);
                        true
                    }
                    "notification.centerButton" => {
                        window.dispatch_action(Box::new(OpenNotifications), cx);
                        true
                    }
                    "board.projectFilter" => {
                        window.dispatch_action(Box::new(OpenProjectFilter), cx);
                        true
                    }
                    "createTask.submit" | "branchTask.submit" => {
                        window.dispatch_action(Box::new(SubmitTaskEditor), cx);
                        true
                    }
                    "createTask.form" => {
                        self.fill_runtime_qa_task_form(payload.as_ref(), false, window, cx)
                    }
                    "branchTask.form" => {
                        self.fill_runtime_qa_task_form(payload.as_ref(), true, window, cx)
                    }
                    "projectFilter.selection" => {
                        self.apply_runtime_qa_project_filter(payload.as_ref(), cx)
                    }
                    "settings.vim_mode_enabled" => {
                        let value = payload
                            .as_ref()
                            .and_then(|value| value["value"].as_str())
                            .and_then(|value| value.parse::<bool>().ok());
                        if let Some(value) = value {
                            self.apply_settings_patch(
                                NativeSettingsPatch {
                                    vim_mode_enabled: Some(value),
                                    ..NativeSettingsPatch::default()
                                },
                                cx,
                            );
                            true
                        } else {
                            false
                        }
                    }
                    "paneLayout.option.vertical_2" => {
                        self.apply_runtime_qa_pane_layout("vertical_2", cx)
                    }
                    "taskSidebar.collapse" => {
                        if !self.task_sidebar_collapsed {
                            self.toggle_task_sidebar(cx);
                        }
                        true
                    }
                    "taskSidebar.dismissHint" => {
                        self.dismiss_task_sidebar_hint(cx);
                        true
                    }
                    "dialog.confirm" if self.delete_confirmation_task_id.is_some() => {
                        self.confirm_task_delete(cx);
                        true
                    }
                    "dialog.confirm" if self.pending_done_mutation.is_some() => {
                        window.dispatch_action(Box::new(ConfirmDone), cx);
                        true
                    }
                    "context.action.branchFromTask" => {
                        if let Some(task_id) = self.focused_task_id.clone() {
                            self.open_branch_task_editor(task_id, window, cx);
                            true
                        } else {
                            false
                        }
                    }
                    "context.action.moveToReview" => {
                        if let Some(task_id) = self.focused_task_id.clone() {
                            self.change_task_status(task_id, TaskStatus::Review, cx);
                            true
                        } else {
                            false
                        }
                    }
                    "context.action.deleteTask" => {
                        if let Some(task_id) = self.focused_task_id.clone() {
                            self.request_task_delete(task_id, cx);
                            true
                        } else {
                            false
                        }
                    }
                    id if id.starts_with("projectColor.") => {
                        let project_id = id.trim_start_matches("projectColor.");
                        let color = payload
                            .as_ref()
                            .and_then(|value| value["color"].as_str())
                            .map(str::to_owned);
                        let project_exists = self
                            .spec
                            .projects
                            .iter()
                            .any(|project| project.id == project_id);
                        if project_exists {
                            if let Some(color) = color {
                                self.set_project_color(project_id.to_owned(), color, cx);
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                    id if id.starts_with("dock.") => {
                        self.select_task_dock_item(id.trim_start_matches("dock."), cx)
                    }
                    "sessionDependency.panelTrigger" => self.select_task_dock_item("terminal", cx),
                    "diff.fileList.firstChangedFile" => self.select_runtime_qa_first_diff_file(cx),
                    id if id.starts_with("column.") && id.ends_with(".dropTarget") => {
                        self.apply_runtime_qa_column_drop(id, payload.as_ref(), cx)
                    }
                    _ => self.apply_runtime_qa_task_target(&id, payload.as_ref(), cx),
                };
                QaControlResponse::SyntheticInput {
                    accepted,
                    dispatch_status: if accepted {
                        "gpui-production-action-dispatched"
                    } else {
                        "unsupported-gpui-semantic-target"
                    }
                    .to_owned(),
                }
            }
            QaControlCommand::SyntheticKey { key, .. } => {
                let accepted = self.apply_runtime_qa_key(&key, window, cx);
                QaControlResponse::SyntheticInput {
                    accepted,
                    dispatch_status: if accepted {
                        "gpui-production-action-dispatched"
                    } else {
                        "unsupported-gpui-key-target"
                    }
                    .to_owned(),
                }
            }
            QaControlCommand::SyntheticMouse { .. } => QaControlResponse::SyntheticInput {
                accepted: false,
                dispatch_status: "unsupported-gpui-input-command".to_owned(),
            },
            QaControlCommand::QueryElement { id } | QaControlCommand::QueryText { id } => {
                self.runtime_qa_query(id)
            }
            command @ (QaControlCommand::StartVideoCapture { .. }
            | QaControlCommand::StopVideoCapture { .. }) => self.qa_capture_state.handle(command),
            command => self.runtime_qa_state().handle(command),
        }
    }

    fn runtime_qa_query(&self, id: String) -> crate::qa_control::QaControlResponse {
        use crate::qa_control::QaControlResponse;

        let text = if id.starts_with("route.") {
            Some(self.navigation.current().path())
        } else {
            match id.as_str() {
                "diff.sidebar" | "diff.pane"
                    if matches!(self.navigation.current(), NativeRoute::Diff { .. }) =>
                {
                    Some(id.clone())
                }
                "notification.center" if self.notification_center_open => {
                    Some("Notifications".to_owned())
                }
                "dock.root" => self.runtime_qa_dock_summary(),
                "ai.providerFilters" => Some(
                    AiSessionProvider::ALL
                        .into_iter()
                        .map(AiSessionProvider::as_str)
                        .collect::<Vec<_>>()
                        .join(","),
                ),
                "protocol.blocker.externalTool"
                    if matches!(self.navigation.current(), NativeRoute::Diff { .. }) =>
                {
                    Some("external tool blocker allowed".to_owned())
                }
                "taskVisible.target" => self
                    .spec
                    .columns
                    .iter()
                    .flat_map(|column| column.cards.iter())
                    .find_map(|card| card.branch_name.clone()),
                _ if id.starts_with("hooks.status.") => id
                    .strip_prefix("hooks.status.")
                    .filter(|task_id| self.runtime_qa_task_visible(task_id))
                    .map(|_| "Hook status".to_owned()),
                _ if id.starts_with("sessionDependency.") => {
                    let session_type = id.trim_start_matches("sessionDependency.");
                    self.current_runtime_qa_card()
                        .filter(|card| card.ssh_host.is_some())
                        .map(|_| session_type.to_owned())
                }
                _ if id.starts_with("projectFilter.") => id
                    .strip_prefix("projectFilter.")
                    .filter(|project_id| self.selected_project_ids.contains(*project_id))
                    .map(str::to_owned),
                _ if id.starts_with("searchResultVisible.") => None,
                _ => {
                    let response = self.runtime_qa_state().handle(
                        crate::qa_control::QaControlCommand::QueryElement { id: id.clone() },
                    );
                    return self.apply_runtime_qa_visibility(response);
                }
            }
        };
        QaControlResponse::Element {
            id,
            exists: text.is_some(),
            text,
        }
    }

    fn apply_runtime_qa_visibility(
        &self,
        response: crate::qa_control::QaControlResponse,
    ) -> crate::qa_control::QaControlResponse {
        let crate::qa_control::QaControlResponse::Element { id, exists, text } = response else {
            return response;
        };
        let task_id = id
            .strip_prefix("task.")
            .and_then(|target| target.split('.').next());
        let visible = task_id.is_none_or(|task_id| self.runtime_qa_task_visible(task_id));
        crate::qa_control::QaControlResponse::Element {
            id,
            exists: exists && visible,
            text: visible.then_some(text).flatten(),
        }
    }

    fn runtime_qa_task_visible(&self, task_id: &str) -> bool {
        self.spec
            .columns
            .iter()
            .flat_map(|column| column.cards.iter())
            .find(|card| card.id == task_id)
            .is_some_and(|card| {
                self.selected_project_ids.is_empty()
                    || card
                        .project_id
                        .as_ref()
                        .is_some_and(|project_id| self.selected_project_ids.contains(project_id))
            })
    }

    fn current_runtime_qa_card(&self) -> Option<&NativeUiCardSpec> {
        let task_id = match self.navigation.current() {
            NativeRoute::TaskDetail { task_id, .. } | NativeRoute::Diff { task_id, .. } => task_id,
            _ => self.focused_task_id.as_ref()?,
        };
        self.spec
            .columns
            .iter()
            .flat_map(|column| column.cards.iter())
            .find(|card| &card.id == task_id)
    }

    fn runtime_qa_dock_summary(&self) -> Option<String> {
        let card = self.current_runtime_qa_card()?;
        let items = task_detail_dock_items(card.pr_url.as_deref(), ShortcutPlatform::Mac);
        Some(format!(
            "items={};shortcuts={}",
            items
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>()
                .join(","),
            items
                .iter()
                .map(|item| item.shortcut_label.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ))
    }

    fn select_task_dock_item(&mut self, item_id: &str, cx: &mut Context<Self>) -> bool {
        let Some(card) = self.current_runtime_qa_card() else {
            return false;
        };
        let exists = task_detail_dock_items(card.pr_url.as_deref(), ShortcutPlatform::Mac)
            .iter()
            .any(|item| item.id == item_id);
        if !exists {
            return false;
        }
        self.selected_task_dock_item = Some(item_id.to_owned());
        cx.notify();
        true
    }

    fn select_runtime_qa_first_diff_file(&mut self, cx: &mut Context<Self>) -> bool {
        let NativeRoute::Diff { task_id, .. } = self.navigation.current() else {
            return false;
        };
        let task_id = task_id.clone();
        if self.diff_snapshots_loading.contains(&task_id) {
            // The production loader selects the first changed file when its snapshot arrives.
            return true;
        }
        let path = self
            .diff_snapshots
            .get(&task_id)
            .and_then(|snapshot| snapshot.as_ref().ok())
            .and_then(|snapshot| snapshot.files.first())
            .map(|file| file.path.clone());
        let Some(path) = path else {
            return false;
        };
        self.select_diff_file(task_id, path, cx);
        true
    }

    fn apply_runtime_qa_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if key.eq_ignore_ascii_case("enter") {
            if self.delete_confirmation_task_id.is_some() {
                self.confirm_task_delete(cx);
            } else if self.pending_done_mutation.is_some() {
                window.dispatch_action(Box::new(ConfirmDone), cx);
            } else if self.show_task_editor {
                window.dispatch_action(Box::new(SubmitTaskEditor), cx);
            } else if self.show_vim_command {
                window.dispatch_action(Box::new(SubmitVimCommand), cx);
            } else if self.show_quick_search {
                window.dispatch_action(Box::new(ApplyQuickSearch), cx);
            } else {
                return false;
            }
            return true;
        }
        if key == ":" {
            window.dispatch_action(Box::new(VimCommandPalette), cx);
            return true;
        }
        if key == "taskSearchDefault" {
            window.dispatch_action(Box::new(QuickSearch), cx);
            return true;
        }
        if self.show_vim_command {
            let key = key.to_owned();
            self.vim_command_input
                .update(cx, |input, cx| input.set_value(key, window, cx));
            cx.notify();
            return true;
        }
        if self.show_quick_search {
            let query = key.trim().to_owned();
            let key = key.to_owned();
            self.quick_search_input
                .update(cx, |input, cx| input.set_value(key, window, cx));
            self.quick_search_query = query;
            cx.notify();
            return true;
        }
        false
    }

    fn runtime_qa_state(&self) -> crate::qa_control::QaControlState {
        let settings = self
            .settings
            .as_ref()
            .map(|settings| {
                BTreeMap::from([
                    (
                        "theme_preference".to_owned(),
                        settings.theme_preference.as_str().to_owned(),
                    ),
                    (
                        "default_session_type".to_owned(),
                        settings.default_session_type.as_str().to_owned(),
                    ),
                    (
                        "task_search_shortcut".to_owned(),
                        settings.task_search_shortcut.clone(),
                    ),
                    (
                        "vim_mode_enabled".to_owned(),
                        settings.vim_mode_enabled.to_string(),
                    ),
                    (
                        "sidebar_default_collapsed".to_owned(),
                        settings.sidebar_default_collapsed.to_string(),
                    ),
                    (
                        "background_sync_enabled".to_owned(),
                        settings.background_sync.is_enabled.to_string(),
                    ),
                ])
            })
            .unwrap_or_default();
        let pane_layouts = self
            .pane_layouts
            .as_ref()
            .map(|layouts| {
                layouts
                    .iter()
                    .map(|layout| crate::qa_control::QaPaneLayoutSnapshot {
                        project_id: layout.project_id.clone(),
                        layout_type: layout.layout_type.as_str().to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        crate::qa_control::QaControlState::from_runtime_snapshots(
            self.spec.clone(),
            settings,
            pane_layouts,
        )
    }

    fn fill_runtime_qa_task_form(
        &mut self,
        payload: Option<&serde_json::Value>,
        branch: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(payload) = payload else {
            return false;
        };
        let title_key = if branch { "branchName" } else { "title" };
        let Some(title) = payload[title_key].as_str() else {
            return false;
        };
        let title = title.to_owned();
        self.task_title_input
            .update(cx, |input, cx| input.set_value(title, window, cx));
        if let Some(description) = payload["description"].as_str() {
            let description = description.to_owned();
            self.task_description_input
                .update(cx, |input, cx| input.set_value(description, window, cx));
        }
        if let Some(base_branch) = payload["baseBranch"].as_str() {
            let base_branch = base_branch.to_owned();
            self.task_base_branch_input
                .update(cx, |input, cx| input.set_value(base_branch, window, cx));
        }
        if let Some(project_id) = payload["projectId"].as_str() {
            self.task_form_project_id = Some(project_id.to_owned());
        }
        if let Some(priority) = payload["priority"]
            .as_str()
            .and_then(|priority| TaskPriority::parse(priority).ok())
        {
            self.task_form_priority = Some(priority);
        }
        if let Some(session_type) = payload["sessionType"]
            .as_str()
            .and_then(|session_type| SessionType::parse(session_type).ok())
        {
            self.task_form_session_type = session_type;
        }
        cx.notify();
        true
    }

    fn apply_runtime_qa_project_filter(
        &mut self,
        payload: Option<&serde_json::Value>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(project_ids) = payload
            .and_then(|payload| payload["projectIds"].as_array())
            .map(|ids| ids.iter().filter_map(|id| id.as_str()).collect::<Vec<_>>())
        else {
            return false;
        };
        self.toggle_project_filter(None, cx);
        for project_id in project_ids {
            self.toggle_project_filter(Some(project_id.to_owned()), cx);
        }
        true
    }

    fn apply_runtime_qa_column_drop(
        &mut self,
        id: &str,
        payload: Option<&serde_json::Value>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(task_id) = payload.and_then(|payload| payload["taskId"].as_str()) else {
            return false;
        };
        let Some(status) = id
            .strip_prefix("column.")
            .and_then(|target| target.strip_suffix(".dropTarget"))
            .and_then(|status| TaskStatus::parse(status).ok())
        else {
            return false;
        };
        let title = self
            .spec
            .columns
            .iter()
            .flat_map(|column| column.cards.iter())
            .find(|card| card.id == task_id)
            .map(|card| card.title.clone());
        let Some(title) = title else {
            return false;
        };
        self.drop_task_on_column(
            TaskDragInfo {
                task_id: task_id.to_owned(),
                title,
            },
            status,
            cx,
        );
        true
    }

    fn apply_runtime_qa_pane_layout(
        &mut self,
        target_layout: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Ok(layout_type) = PaneLayoutType::parse(target_layout) else {
            return false;
        };
        let selected_project_id = self.selected_project_ids.iter().next().cloned();
        let layout_id = self.pane_layouts.as_ref().ok().and_then(|layouts| {
            selected_project_id
                .as_ref()
                .and_then(|project_id| {
                    layouts
                        .iter()
                        .find(|layout| layout.project_id.as_ref() == Some(project_id))
                })
                .or_else(|| {
                    selected_project_id
                        .is_none()
                        .then(|| layouts.iter().find(|layout| layout.is_global))
                        .flatten()
                })
                .map(|layout| layout.id.clone())
        });
        let is_global = selected_project_id.is_none();
        self.select_pane_layout_type(layout_id, selected_project_id, is_global, layout_type, cx);
        self.mutation_error.is_none()
    }

    fn apply_runtime_qa_task_target(
        &mut self,
        id: &str,
        payload: Option<&serde_json::Value>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(target) = id.strip_prefix("task.") else {
            return false;
        };
        let (task_id, open_diff) = target
            .strip_suffix(".diff")
            .map_or((target, false), |task_id| (task_id, true));
        let exists = self
            .spec
            .columns
            .iter()
            .flat_map(|column| column.cards.iter())
            .any(|card| card.id == task_id);
        if !exists || task_id.contains('.') {
            return false;
        }

        self.focused_task_id = Some(task_id.to_owned());
        match payload.and_then(|payload| payload["action"].as_str()) {
            Some("focusTask") => {
                cx.notify();
                return true;
            }
            Some("openTaskContextMenu") => {
                self.open_keyboard_context_menu(cx);
                return true;
            }
            _ => {}
        }
        let locale = self.navigation.current().locale();
        if open_diff {
            self.navigate(
                NativeRoute::Diff {
                    locale,
                    task_id: task_id.to_owned(),
                },
                cx,
            );
            self.load_diff_snapshot(task_id.to_owned(), false, cx);
        } else {
            self.navigate(
                NativeRoute::TaskDetail {
                    locale,
                    task_id: task_id.to_owned(),
                },
                cx,
            );
            self.load_task_ai_sessions(task_id.to_owned(), cx);
        }
        true
    }

    fn message(&self, path: &str, fallback: &str) -> String {
        self.spec
            .messages
            .get(path)
            .cloned()
            .unwrap_or_else(|| fallback.to_owned())
    }

    fn formatted_message(&self, path: &str, fallback: &str, values: &[(&str, &str)]) -> String {
        let mut message = self.message(path, fallback);
        for (key, value) in values {
            message = message.replace(&format!("{{{key}}}"), value);
        }
        message
    }

    fn has_open_overlay(&self) -> bool {
        self.task_search_shortcut_recording
            || self.release_update.is_some()
            || self.show_task_editor
            || self.show_project_filter
            || self.show_quick_search
            || self.show_vim_command
            || self.show_keyboard_context_menu
            || self.delete_confirmation_task_id.is_some()
            || self.pending_done_mutation.is_some()
            || self.editing_pane_command.is_some()
            || self.show_project_editor
            || self.editing_diff_file.is_some()
            || self.notification_center_open
            || self.background_review_open
    }

    fn restore_terminal_focus_if_requested(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.restore_terminal_focus_on_render || self.has_open_overlay() {
            return;
        }
        self.restore_terminal_focus_on_render = false;
        if let NativeRoute::TaskDetail { task_id, .. } = self.navigation.current()
            && let Some(terminal) = self.terminals.get(task_id)
        {
            terminal.read(cx).focus_handle().focus(window);
        }
    }

    fn request_terminal_focus_restore(&mut self) {
        self.restore_terminal_focus_on_render = true;
    }

    fn dismiss_top_overlay(&mut self, cx: &mut Context<Self>) {
        if self.task_search_shortcut_recording {
            self.task_search_shortcut_recording = false;
            self.mutation_error = None;
            cx.notify();
        } else if self.release_update.is_some() {
            self.close_release_update(cx);
        } else if self.show_task_editor {
            self.cancel_task_editor(cx);
        } else if self.show_project_filter {
            self.show_project_filter = false;
            cx.notify();
        } else if self.show_quick_search {
            self.close_quick_search(cx);
        } else if self.show_vim_command {
            self.close_vim_command(cx);
        } else if self.show_keyboard_context_menu {
            self.close_keyboard_context_menu(cx);
        } else if self.delete_confirmation_task_id.is_some() {
            self.cancel_task_delete(cx);
        } else if self.pending_done_mutation.is_some() {
            self.cancel_done_mutation(cx);
        } else if self.editing_pane_command.is_some() {
            self.close_pane_command_editor(cx);
        } else if self.show_project_editor {
            self.close_project_editor(cx);
        } else if self.editing_diff_file.is_some() {
            self.close_diff_editor(cx);
        } else if self.notification_center_open {
            self.notification_center_open = false;
            cx.notify();
        } else if self.background_review_open {
            self.background_review_open = false;
            cx.notify();
        }
        if !self.has_open_overlay() {
            self.request_terminal_focus_restore();
        }
    }

    fn watch_hook_revision(&mut self, cx: &mut Context<Self>) {
        let hook_revision = Arc::clone(&self.hook_revision);
        let background_sync = Arc::clone(&self.background_sync);
        let notification_revision = Arc::clone(&self.notification_revision);
        let notifications = Arc::clone(&self.notifications);
        let mut observed_revision = hook_revision.load(Ordering::Acquire);
        let mut observed_notification_revision = notification_revision.load(Ordering::Acquire);
        let mut observed_background_runs = background_sync.snapshot().completed_run_count;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let current_revision = hook_revision.load(Ordering::Acquire);
                let current_notification_revision = notification_revision.load(Ordering::Acquire);
                let background_snapshot = background_sync.snapshot();
                let background_changed =
                    background_snapshot.completed_run_count != observed_background_runs;
                let notifications_changed =
                    current_notification_revision != observed_notification_revision;
                if current_revision == observed_revision
                    && !background_changed
                    && !notifications_changed
                {
                    continue;
                }
                let board_changed = current_revision != observed_revision;
                observed_revision = current_revision;
                observed_notification_revision = current_notification_revision;
                observed_background_runs = background_snapshot.completed_run_count;
                let activated_notification = if notifications_changed {
                    notifications.consume_activation().unwrap_or_else(|error| {
                        eprintln!(
                            "{}",
                            native_diagnostic_line(
                                "notification-activation-read-error",
                                "notification-center",
                                &error,
                                None,
                            )
                        );
                        None
                    })
                } else {
                    None
                };
                if this
                    .update(cx, |root, cx| {
                        if board_changed {
                            root.retry_startup(cx);
                        }
                        if background_snapshot.pending_review.needs_review() {
                            root.background_review_open = true;
                            root.selected_merged_task_ids = background_snapshot
                                .pending_review
                                .merged_pull_requests
                                .iter()
                                .map(|merged| merged.task_id.clone())
                                .collect();
                        }
                        if let Some(notification) = activated_notification {
                            let action_type = notification
                                .action
                                .as_ref()
                                .and_then(|action| action.get("type"))
                                .and_then(serde_json::Value::as_str);
                            if should_keep_current_route_for_notification_activation(action_type) {
                                root.background_review_open = true;
                            } else {
                                root.navigate(parse_native_route(&notification.relative_path), cx);
                            }
                        }
                        if notifications_changed {
                            root.notification_snapshot = root.notifications.list();
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn watch_release_updates(&mut self, cx: &mut Context<Self>) {
        self.check_release_update(cx);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(60 * 60))
                    .await;
                if this
                    .update(cx, |root, cx| root.check_release_update(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn check_release_update(&mut self, cx: &mut Context<Self>) {
        if self.release_update_checking {
            return;
        }
        self.release_update_checking = true;
        let dismissed_versions = self
            .settings
            .as_ref()
            .map(|settings| settings.release_update_dismissed_versions.clone())
            .unwrap_or_default();
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let result = native_release_update_service()
                .check(env!("CARGO_PKG_VERSION"), &dismissed_versions);
            let _ = sender.send(result);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => {
                        let _ = this.update(cx, |root, cx| {
                            root.release_update_checking = false;
                            root.release_update_error =
                                Some("Release update worker stopped unexpectedly.".to_owned());
                            cx.notify();
                        });
                        break;
                    }
                };
                let _ = this.update(cx, |root, cx| {
                    root.release_update_checking = false;
                    root.release_update_error = result.error;
                    if let Some(release) = result.release
                        && native_release_update_service().claim(&release.version)
                    {
                        root.release_update = Some(release);
                        root.release_update_dont_show_again = false;
                    }
                    cx.notify();
                });
                break;
            }
        })
        .detach();
        cx.notify();
    }

    fn install_release_update(&mut self, cx: &mut Context<Self>) {
        if self.release_update_installing {
            return;
        }
        let Some(release) = self.release_update.clone() else {
            return;
        };
        let Some(installer) = release.installer else {
            self.release_update_error =
                Some("This release has no verified native installer asset.".to_owned());
            cx.notify();
            return;
        };
        let Some(app_data_dir) = self.config.database_path.parent().map(Path::to_path_buf) else {
            self.release_update_error =
                Some("Could not resolve the KanVibe application-data directory.".to_owned());
            cx.notify();
            return;
        };
        self.release_update_installing = true;
        self.release_update_error = None;
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let result = crate::native_updater::prepare_native_update(
                &installer,
                &release.version,
                app_data_dir,
            );
            let _ = sender.send(result);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => {
                        let _ = this.update(cx, |root, cx| {
                            root.release_update_installing = false;
                            root.release_update_error =
                                Some("Native update worker stopped unexpectedly.".to_owned());
                            cx.notify();
                        });
                        break;
                    }
                };
                let _ = this.update(cx, |root, cx| {
                    root.release_update_installing = false;
                    match result {
                        Ok(journal_path) => {
                            match crate::native_updater::spawn_update_helper(journal_path) {
                                Ok(()) => cx.quit(),
                                Err(error) => root.release_update_error = Some(error),
                            }
                        }
                        Err(error) => root.release_update_error = Some(error),
                    }
                    cx.notify();
                });
                break;
            }
        })
        .detach();
        cx.notify();
    }

    fn vim_shortcuts_available(&self) -> bool {
        matches!(self.navigation.current(), NativeRoute::Board { .. })
            && self
                .settings
                .as_ref()
                .is_ok_and(|settings| settings.vim_mode_enabled)
            && !self.show_task_editor
            && !self.show_quick_search
            && !self.show_vim_command
            && !self.show_keyboard_context_menu
    }

    fn visible_board_task_ids(&self) -> Vec<String> {
        self.spec
            .columns
            .iter()
            .flat_map(|column| {
                filter_native_cards_by_projects(column.cards.iter(), "", &self.selected_project_ids)
            })
            .map(|card| card.id.clone())
            .collect()
    }

    fn move_vim_focus(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.move_board_focus(delta, true, cx);
    }

    fn move_board_focus(&mut self, delta: isize, require_vim_mode: bool, cx: &mut Context<Self>) {
        if !matches!(self.navigation.current(), NativeRoute::Board { .. })
            || (require_vim_mode && !self.vim_shortcuts_available())
            || self.show_task_editor
            || self.show_quick_search
            || self.show_vim_command
            || self.show_keyboard_context_menu
        {
            return;
        }
        let task_ids = self.visible_board_task_ids();
        if task_ids.is_empty() {
            self.focused_task_id = None;
            return;
        }
        if self.focused_task_id.is_none() {
            let initial_index = if delta < 0 { task_ids.len() - 1 } else { 0 };
            self.focused_task_id = Some(task_ids[initial_index].clone());
            cx.notify();
            return;
        }
        let current_index = self
            .focused_task_id
            .as_ref()
            .and_then(|focused| task_ids.iter().position(|task_id| task_id == focused))
            .unwrap_or_default();
        let next_index = if delta < 0 {
            current_index.saturating_sub(delta.unsigned_abs())
        } else {
            current_index
                .saturating_add(delta as usize)
                .min(task_ids.len() - 1)
        };
        self.focused_task_id = Some(task_ids[next_index].clone());
        cx.notify();
    }

    fn open_vim_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.vim_shortcuts_available() {
            return;
        }
        if self.focused_task_id.is_none() {
            self.focused_task_id = self.visible_board_task_ids().into_iter().next();
        }
        self.show_vim_command = true;
        self.vim_command_input.update(cx, |input, cx| {
            input.set_value(":", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn close_vim_command(&mut self, cx: &mut Context<Self>) {
        self.show_vim_command = false;
        self.request_terminal_focus_restore();
        cx.notify();
    }

    fn submit_vim_command(&mut self, cx: &mut Context<Self>) {
        let Some(task_id) = self.focused_task_id.clone() else {
            self.mutation_error = Some("No task is focused.".to_owned());
            cx.notify();
            return;
        };
        match parse_native_vim_command(self.vim_command_input.read(cx).value().as_ref()) {
            Ok(NativeVimCommand::Move(status)) => {
                self.show_vim_command = false;
                self.change_task_status(task_id, status, cx);
            }
            Err(error) => {
                self.mutation_error = Some(error);
                cx.notify();
            }
        }
    }

    fn complete_vim_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let completed =
            complete_native_vim_command(self.vim_command_input.read(cx).value().as_ref());
        self.vim_command_input.update(cx, |input, cx| {
            input.set_value(completed, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn delete_vim_focused_task(&mut self, cx: &mut Context<Self>) {
        if !self.vim_shortcuts_available() {
            return;
        }
        let Some(task_id) = self.focused_task_id.clone() else {
            return;
        };
        let locale = self.navigation.current().locale();
        self.navigate(
            NativeRoute::TaskDetail {
                locale,
                task_id: task_id.clone(),
            },
            cx,
        );
        self.request_task_delete(task_id, cx);
    }

    fn focused_or_first_task_id(&self) -> Option<String> {
        self.focused_task_id
            .clone()
            .or_else(|| self.visible_board_task_ids().into_iter().next())
    }

    fn open_focused_task(&mut self, new_window: bool, cx: &mut Context<Self>) {
        if !matches!(self.navigation.current(), NativeRoute::Board { .. }) {
            return;
        }
        let Some(task_id) = self.focused_or_first_task_id() else {
            return;
        };
        self.focused_task_id = Some(task_id.clone());
        let route = NativeRoute::TaskDetail {
            locale: self.navigation.current().locale(),
            task_id,
        };
        if new_window {
            let state = load_native_state_with_done_limit(&self.config, self.done_limit);
            open_native_window(
                cx,
                self.config.clone(),
                state,
                route,
                Arc::clone(&self.hook_revision),
                Arc::clone(&self.background_sync),
                Arc::clone(&self.notification_revision),
                Arc::clone(&self.notifications),
                Arc::clone(&self.qa_receiver),
            );
        } else {
            self.navigate(route, cx);
        }
    }

    fn open_keyboard_context_menu(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.navigation.current(), NativeRoute::Board { .. }) {
            return;
        }
        self.focused_task_id = self.focused_or_first_task_id();
        self.show_keyboard_context_menu = self.focused_task_id.is_some();
        cx.notify();
    }

    fn close_keyboard_context_menu(&mut self, cx: &mut Context<Self>) {
        self.show_keyboard_context_menu = false;
        self.request_terminal_focus_restore();
        cx.notify();
    }

    fn start_task_terminal(
        &mut self,
        task_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(terminal) = self.terminals.get(&task_id) {
            terminal.read(cx).focus_handle().focus(window);
            return;
        }
        let card = self
            .spec
            .columns
            .iter()
            .flat_map(|column| column.cards.iter())
            .find(|card| card.id == task_id)
            .cloned();
        let session_type = card
            .as_ref()
            .and_then(|card| card.session_type.as_deref())
            .and_then(|value| SessionType::parse(value).ok());
        if let Some(session_type) = session_type {
            let dependency = self
                .config
                .ensure_database_file()
                .map_err(|error| error.to_string())
                .and_then(|database_path| {
                    get_native_task_session_dependency_status(database_path, &task_id, session_type)
                });
            self.session_dependency_statuses
                .insert(task_id.clone(), dependency.clone());
            match dependency {
                Ok(status) if status.available => {}
                Ok(status) => {
                    let error = format!(
                        "{} is not installed on {}. Select Install, then retry.",
                        status.tool_name, status.target
                    );
                    self.terminal_errors.insert(task_id, error);
                    cx.notify();
                    return;
                }
                Err(error) => {
                    self.terminal_errors.insert(task_id, error);
                    cx.notify();
                    return;
                }
            }
        }
        let result = (|| -> Result<(Entity<TerminalView>, PtyController), String> {
            let card = card.ok_or_else(|| "Task no longer exists.".to_owned())?;
            let project = card.project_id.as_ref().and_then(|project_id| {
                self.spec
                    .projects
                    .iter()
                    .find(|project| project.id == *project_id)
            });
            let worktree_path = card
                .worktree_path
                .clone()
                .or_else(|| project.map(|project| project.repo_path.clone()))
                .ok_or_else(|| "Task has no project or worktree path.".to_owned())?;
            let home_directory = std::env::var("HOME").unwrap_or_else(|_| "/".to_owned());
            let environment = create_local_shell_environment(
                std::env::vars(),
                &home_directory,
                ShellPlatform::Mac,
            );
            let shell = environment
                .get("SHELL")
                .cloned()
                .unwrap_or_else(|| "/bin/zsh".to_owned());
            let request = build_task_session_pty_request(
                &shell,
                &worktree_path,
                session_type,
                card.session_name.as_deref(),
                card.ssh_host.as_deref(),
                environment,
            );
            let SpawnedPty {
                reader,
                writer,
                controller,
            } = spawn_pty(request).map_err(|error| error.to_string())?;
            let resize_controller = controller.clone();
            let terminal = cx.new(|cx| {
                TerminalView::new(writer, reader, TerminalConfig::default(), cx)
                    .with_resize_callback(move |cols, rows| {
                        let _ = resize_controller.resize(cols, rows);
                    })
            });
            Ok((terminal, controller))
        })();

        match result {
            Ok((terminal, controller)) => {
                terminal.read(cx).focus_handle().focus(window);
                self.terminals.insert(task_id.clone(), terminal);
                self.terminal_controllers
                    .insert(task_id.clone(), controller);
                self.terminal_errors.remove(&task_id);
                cx.notify();
            }
            Err(error) => {
                log_native_failure(&self.config, "start-terminal-error", &error, None);
                self.terminal_errors.insert(task_id, error);
                cx.notify();
            }
        }
    }

    fn stop_task_terminal(&mut self, task_id: &str, cx: &mut Context<Self>) {
        if let Some(controller) = self.terminal_controllers.remove(task_id) {
            let _ = controller.terminate();
        }
        self.terminals.remove(task_id);
        cx.notify();
    }

    fn restart_task_terminal(
        &mut self,
        task_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_task_terminal(&task_id, cx);
        self.start_task_terminal(task_id, window, cx);
    }

    fn connect_task_terminal(
        &mut self,
        task_id: String,
        session_type: SessionType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dependency = self
            .config
            .ensure_database_file()
            .map_err(|error| error.to_string())
            .and_then(|database_path| {
                get_native_task_session_dependency_status(database_path, &task_id, session_type)
            });
        self.session_dependency_statuses
            .insert(task_id.clone(), dependency.clone());
        let result = dependency.and_then(|status| {
            if !status.available {
                return Err(format!(
                    "{} is not installed on {}. Select Install, then retry.",
                    status.tool_name, status.target
                ));
            }
            self.config
                .ensure_database_file()
                .map_err(|error| error.to_string())
                .and_then(|database_path| {
                    connect_native_task_terminal(database_path, &task_id, session_type)
                })
        });
        match result {
            Ok(_) => {
                self.terminal_errors.remove(&task_id);
                self.retry_startup(cx);
                self.start_task_terminal(task_id, window, cx);
            }
            Err(error) => {
                log_native_failure(&self.config, "connect-terminal-error", &error, None);
                self.terminal_errors.insert(task_id, error);
                cx.notify();
            }
        }
    }

    fn run_session_dependency_operation(
        &mut self,
        task_id: String,
        session_type: SessionType,
        install: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.session_dependency_loading.insert(task_id.clone()) {
            return;
        }
        let config = self.config.clone();
        let worker_task_id = task_id.clone();
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let result = config
                .ensure_database_file()
                .map_err(|error| error.to_string())
                .and_then(|database_path| {
                    if install {
                        install_native_task_session_dependency(
                            database_path,
                            &worker_task_id,
                            session_type,
                        )
                    } else {
                        get_native_task_session_dependency_status(
                            database_path,
                            &worker_task_id,
                            session_type,
                        )
                    }
                });
            let _ = sender.send(result);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => {
                        Err("Session dependency worker stopped unexpectedly.".to_owned())
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.session_dependency_loading.remove(&task_id);
                    if result.as_ref().is_ok_and(|status| status.available) {
                        this.terminal_errors.remove(&task_id);
                    }
                    this.session_dependency_statuses.insert(task_id, result);
                    cx.notify();
                });
                break;
            }
        })
        .detach();
    }

    fn run_task_hook_operation(&mut self, task_id: String, install: bool, cx: &mut Context<Self>) {
        if !self.task_hook_loading.insert(task_id.clone()) {
            return;
        }
        let config = self.config.clone();
        let worker_task_id = task_id.clone();
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let result = config
                .ensure_database_file()
                .map_err(|error| error.to_string())
                .and_then(|database_path| {
                    if install {
                        install_native_task_hooks(database_path, &worker_task_id)
                    } else {
                        get_native_task_hook_status(database_path, &worker_task_id)
                    }
                });
            let _ = sender.send(result);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => {
                        Err("Provider hook worker stopped unexpectedly.".to_owned())
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.task_hook_loading.remove(&task_id);
                    this.task_hook_statuses.insert(task_id, result);
                    cx.notify();
                });
                break;
            }
        })
        .detach();
    }

    fn open_quick_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_quick_search = true;
        self.quick_search_query.clear();
        self.quick_search_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn apply_quick_search(&mut self, cx: &mut Context<Self>) {
        self.quick_search_query = self.quick_search_input.read(cx).value().trim().to_owned();
        cx.notify();
    }

    fn close_quick_search(&mut self, cx: &mut Context<Self>) {
        self.show_quick_search = false;
        self.quick_search_query.clear();
        self.request_terminal_focus_restore();
        cx.notify();
    }

    fn focus_board_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.navigation.current(), NativeRoute::Board { .. }) {
            self.navigate(
                NativeRoute::Board {
                    locale: self.navigation.current().locale(),
                },
                cx,
            );
        }
        self.show_board_find = true;
        self.board_search_input.update(cx, |input, cx| {
            input.focus(window, cx);
        });
    }

    fn apply_board_search(&mut self, cx: &mut Context<Self>) {
        self.board_search_query = self.board_search_input.read(cx).value().trim().to_owned();
        cx.notify();
    }

    fn clear_board_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.board_search_query.clear();
        self.show_board_find = false;
        self.board_search_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        cx.notify();
    }

    fn toggle_project_filter(&mut self, project_id: Option<String>, cx: &mut Context<Self>) {
        if let Some(project_id) = project_id {
            if !self.selected_project_ids.remove(&project_id) {
                self.selected_project_ids.insert(project_id);
            }
        } else {
            self.selected_project_ids.clear();
        }
        cx.notify();
    }

    fn toggle_project_filter_visibility(&mut self, cx: &mut Context<Self>) {
        self.show_project_filter = !self.show_project_filter;
        if !self.show_project_filter {
            self.request_terminal_focus_restore();
        }
        cx.notify();
    }

    fn open_task_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let default_project = self
            .spec
            .projects
            .iter()
            .find(|project| !project.is_worktree);
        self.show_task_editor = true;
        self.editing_task_id = None;
        self.branching_task_id = None;
        self.mutation_error = None;
        self.task_form_project_id = default_project.map(|project| project.id.clone());
        self.task_form_priority = None;
        self.task_form_session_type = self
            .settings
            .as_ref()
            .map_or(SessionType::Tmux, |settings| settings.default_session_type);
        self.task_title_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        self.task_description_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        let default_branch = default_project
            .map(|project| project.default_branch.clone())
            .unwrap_or_default();
        self.task_base_branch_input.update(cx, |input, cx| {
            input.set_value(default_branch, window, cx);
        });
        cx.notify();
    }

    fn open_task_title_editor(
        &mut self,
        task_id: String,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let card = self
            .spec
            .columns
            .iter()
            .flat_map(|column| column.cards.iter())
            .find(|card| card.id == task_id)
            .cloned();
        self.show_task_editor = true;
        self.editing_task_id = Some(task_id);
        self.branching_task_id = None;
        self.mutation_error = None;
        self.task_form_project_id = card.as_ref().and_then(|card| card.project_id.clone());
        self.task_form_priority = card
            .as_ref()
            .and_then(|card| card.priority.as_deref())
            .and_then(|priority| TaskPriority::parse(priority).ok());
        self.task_form_session_type = card
            .as_ref()
            .and_then(|card| card.session_type.as_deref())
            .and_then(|session_type| SessionType::parse(session_type).ok())
            .unwrap_or(SessionType::Tmux);
        self.task_title_input.update(cx, |input, cx| {
            input.set_value(title, window, cx);
            input.focus(window, cx);
        });
        let description = card
            .as_ref()
            .and_then(|card| card.description.clone())
            .unwrap_or_default();
        self.task_description_input
            .update(cx, |input, cx| input.set_value(description, window, cx));
        let base_branch = card
            .as_ref()
            .and_then(|card| card.base_branch.clone())
            .unwrap_or_default();
        self.task_base_branch_input.update(cx, |input, cx| {
            input.set_value(base_branch, window, cx);
        });
        cx.notify();
    }

    fn open_branch_task_editor(
        &mut self,
        task_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let card = self
            .spec
            .columns
            .iter()
            .flat_map(|column| column.cards.iter())
            .find(|card| card.id == task_id)
            .cloned();
        let default_project = card
            .as_ref()
            .and_then(|card| card.project_id.as_deref())
            .and_then(|project_id| {
                self.spec
                    .projects
                    .iter()
                    .find(|project| project.id == project_id && !project.is_worktree)
            })
            .or_else(|| {
                self.spec
                    .projects
                    .iter()
                    .find(|project| !project.is_worktree)
            });
        self.show_task_editor = true;
        self.editing_task_id = None;
        self.branching_task_id = Some(task_id);
        self.mutation_error = None;
        self.task_form_project_id = default_project.map(|project| project.id.clone());
        self.task_form_priority = card
            .as_ref()
            .and_then(|card| card.priority.as_deref())
            .and_then(|priority| TaskPriority::parse(priority).ok());
        self.task_form_session_type = self
            .settings
            .as_ref()
            .map_or(SessionType::Tmux, |settings| settings.default_session_type);
        self.task_title_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        self.task_description_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        let default_branch = default_project
            .map(|project| project.default_branch.clone())
            .unwrap_or_default();
        self.task_base_branch_input.update(cx, |input, cx| {
            input.set_value(default_branch, window, cx);
        });
        cx.notify();
    }

    fn cancel_task_editor(&mut self, cx: &mut Context<Self>) {
        if self.task_editor_submitting {
            return;
        }
        self.show_task_editor = false;
        self.editing_task_id = None;
        self.branching_task_id = None;
        self.mutation_error = None;
        self.task_form_project_id = None;
        self.request_terminal_focus_restore();
        cx.notify();
    }

    fn select_task_form_project(
        &mut self,
        project_id: String,
        default_branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.task_form_project_id = Some(project_id);
        self.task_base_branch_input.update(cx, |input, cx| {
            input.set_value(default_branch, window, cx);
        });
        cx.notify();
    }

    fn select_task_form_priority(
        &mut self,
        priority: Option<TaskPriority>,
        cx: &mut Context<Self>,
    ) {
        self.task_form_priority = priority;
        cx.notify();
    }

    fn select_task_form_session_type(&mut self, session_type: SessionType, cx: &mut Context<Self>) {
        self.task_form_session_type = session_type;
        cx.notify();
    }

    fn submit_task_editor(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.task_editor_submitting {
            return;
        }
        let title = self.task_title_input.read(cx).value().trim().to_owned();
        if title.is_empty() {
            self.mutation_error = Some("Task title is required.".to_owned());
            cx.notify();
            return;
        }

        let config = self.config.clone();
        let editing_task_id = self.editing_task_id.clone();
        let branching_task_id = self.branching_task_id.clone();
        let description = self.task_description_input.read(cx).value().to_string();
        let base_branch = self.task_base_branch_input.read(cx).value().to_string();
        let session_type = self.task_form_session_type;
        let project_id = self.task_form_project_id.clone().unwrap_or_default();
        let priority = self.task_form_priority;
        let (sender, receiver) = channel();
        self.task_editor_submitting = true;
        self.mutation_error = None;
        std::thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                let database_path = config
                    .ensure_database_file()
                    .map_err(|error| error.to_string())?;
                if let Some(task_id) = &editing_task_id {
                    update_native_task_metadata(
                        &database_path,
                        task_id,
                        &title,
                        &description,
                        priority,
                    )?
                    .ok_or_else(|| "Task no longer exists.".to_owned())?;
                } else if let Some(task_id) = &branching_task_id {
                    branch_native_task_from_form(
                        &database_path,
                        task_id,
                        NativeTaskFormInput {
                            branch_name: title,
                            description,
                            base_branch,
                            session_type,
                            project_id,
                            priority,
                        },
                    )?;
                } else {
                    create_native_task_from_form(
                        &database_path,
                        NativeTaskFormInput {
                            branch_name: title,
                            description,
                            base_branch,
                            session_type,
                            project_id,
                            priority,
                        },
                    )?;
                }
                Ok(())
            })();
            let _ = sender.send(result);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => {
                        Err("Task editor worker stopped unexpectedly.".to_owned())
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.task_editor_submitting = false;
                    match result {
                        Ok(()) => {
                            this.show_task_editor = false;
                            this.editing_task_id = None;
                            this.branching_task_id = None;
                            this.mutation_error = None;
                            this.request_terminal_focus_restore();
                            this.retry_startup(cx);
                        }
                        Err(error) => {
                            log_native_failure(&this.config, "save-task-error", &error, None);
                            this.mutation_error = Some(error);
                            cx.notify();
                        }
                    }
                });
                break;
            }
        })
        .detach();
        cx.notify();
    }

    fn request_task_delete(&mut self, task_id: String, cx: &mut Context<Self>) {
        self.delete_confirmation_task_id = Some(task_id);
        self.mutation_error = None;
        cx.notify();
    }

    fn cancel_task_delete(&mut self, cx: &mut Context<Self>) {
        self.delete_confirmation_task_id = None;
        self.mutation_error = None;
        self.request_terminal_focus_restore();
        cx.notify();
    }

    fn confirm_task_delete(&mut self, cx: &mut Context<Self>) {
        let Some(task_id) = self.delete_confirmation_task_id.clone() else {
            return;
        };
        if let Some(controller) = self.terminal_controllers.remove(&task_id) {
            let _ = controller.terminate();
        }
        self.terminals.remove(&task_id);
        let result = self
            .config
            .ensure_database_file()
            .map_err(|error| error.to_string())
            .and_then(|database_path| delete_native_task(database_path, &task_id));

        match result {
            Ok(true) => {
                self.delete_confirmation_task_id = None;
                self.mutation_error = None;
                self.retry_startup(cx);
                self.navigate(
                    NativeRoute::Board {
                        locale: self.navigation.current().locale(),
                    },
                    cx,
                );
            }
            Ok(false) => {
                self.mutation_error = Some("Task no longer exists.".to_owned());
                cx.notify();
            }
            Err(error) => {
                log_native_failure(&self.config, "delete-task-error", &error, None);
                self.mutation_error = Some(error);
                cx.notify();
            }
        }
    }

    fn change_task_status(&mut self, task_id: String, status: TaskStatus, cx: &mut Context<Self>) {
        if self.should_confirm_done(&task_id, status) {
            self.pending_done_mutation = Some(PendingDoneMutation::Status { task_id });
            self.done_confirm_dont_ask_again = false;
            cx.notify();
            return;
        }
        self.execute_task_status_change(task_id, status, cx);
    }

    fn execute_task_status_change(
        &mut self,
        task_id: String,
        status: TaskStatus,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .config
            .ensure_database_file()
            .map_err(|error| error.to_string())
            .and_then(|database_path| update_native_task_status(database_path, &task_id, status));

        match result {
            Ok(Some(_)) => {
                self.mutation_error = None;
                self.retry_startup(cx);
            }
            Ok(None) => {
                self.mutation_error = Some("Task no longer exists.".to_owned());
                cx.notify();
            }
            Err(error) => {
                log_native_failure(&self.config, "update-task-status-error", &error, None);
                self.mutation_error = Some(error);
                cx.notify();
            }
        }
    }

    fn drop_task_on_column(
        &mut self,
        drag: TaskDragInfo,
        status: TaskStatus,
        cx: &mut Context<Self>,
    ) {
        let mut destination_ids = self
            .spec
            .columns
            .iter()
            .find(|column| column.status == status)
            .into_iter()
            .flat_map(|column| column.cards.iter())
            .filter(|card| card.id != drag.task_id)
            .map(|card| card.id.clone())
            .collect::<Vec<_>>();
        destination_ids.push(drag.task_id.clone());

        if self.should_confirm_done(&drag.task_id, status) {
            self.pending_done_mutation = Some(PendingDoneMutation::Move {
                task_id: drag.task_id,
                destination_ids,
                error_event: "drag-task-error",
            });
            self.done_confirm_dont_ask_again = false;
            cx.notify();
            return;
        }
        self.execute_task_move(drag.task_id, status, destination_ids, "drag-task-error", cx);
    }

    fn execute_task_move(
        &mut self,
        task_id: String,
        status: TaskStatus,
        destination_ids: Vec<String>,
        error_event: &'static str,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .config
            .ensure_database_file()
            .map_err(|error| error.to_string())
            .and_then(|database_path| {
                move_native_task(database_path, &task_id, status, &destination_ids)
            });
        match result {
            Ok(Some(_)) => {
                self.mutation_error = None;
                self.retry_startup(cx);
            }
            Ok(None) => {
                self.mutation_error = Some("Task no longer exists.".to_owned());
                cx.notify();
            }
            Err(error) => {
                log_native_failure(&self.config, error_event, &error, None);
                self.mutation_error = Some(error);
                cx.notify();
            }
        }
    }

    fn drop_task_before(
        &mut self,
        drag: TaskDragInfo,
        status: TaskStatus,
        before_task_id: String,
        cx: &mut Context<Self>,
    ) {
        let destination_ids = self
            .spec
            .columns
            .iter()
            .find(|column| column.status == status)
            .map(|column| {
                ordered_task_ids_for_drop(
                    column.cards.iter().map(|card| card.id.as_str()),
                    &drag.task_id,
                    &before_task_id,
                )
            })
            .unwrap_or_else(|| vec![drag.task_id.clone()]);
        if self.should_confirm_done(&drag.task_id, status) {
            self.pending_done_mutation = Some(PendingDoneMutation::Move {
                task_id: drag.task_id,
                destination_ids,
                error_event: "reorder-task-error",
            });
            self.done_confirm_dont_ask_again = false;
            cx.notify();
            return;
        }
        self.execute_task_move(
            drag.task_id,
            status,
            destination_ids,
            "reorder-task-error",
            cx,
        );
    }

    fn should_confirm_done(&self, task_id: &str, status: TaskStatus) -> bool {
        if status != TaskStatus::Done
            || self
                .settings
                .as_ref()
                .is_ok_and(|settings| settings.done_alert_dismissed)
        {
            return false;
        }
        self.spec
            .columns
            .iter()
            .flat_map(|column| column.cards.iter())
            .find(|card| card.id == task_id)
            .is_some_and(|card| {
                card.status != TaskStatus::Done.as_str()
                    && (card.branch_name.is_some()
                        || card.session_type.is_some()
                        || card.worktree_path.is_some())
            })
    }

    fn confirm_done_mutation(&mut self, cx: &mut Context<Self>) {
        let Some(mutation) = self.pending_done_mutation.take() else {
            return;
        };
        if self.done_confirm_dont_ask_again {
            let result = self
                .config
                .ensure_database_file()
                .map_err(|error| error.to_string())
                .and_then(|path| {
                    KanvibeDb::open_read_write(path)
                        .map_err(|error| error.to_string())?
                        .dismiss_done_alert()
                        .map_err(|error| error.to_string())
                });
            if let Err(error) = result {
                self.pending_done_mutation = Some(mutation);
                self.mutation_error = Some(error);
                cx.notify();
                return;
            }
            if let Ok(settings) = &mut self.settings {
                settings.done_alert_dismissed = true;
            }
        }
        match mutation {
            PendingDoneMutation::Status { task_id } => {
                self.execute_task_status_change(task_id, TaskStatus::Done, cx);
            }
            PendingDoneMutation::Move {
                task_id,
                destination_ids,
                error_event,
            } => {
                self.execute_task_move(task_id, TaskStatus::Done, destination_ids, error_event, cx)
            }
        }
    }

    fn cancel_done_mutation(&mut self, cx: &mut Context<Self>) {
        self.pending_done_mutation = None;
        self.done_confirm_dont_ask_again = false;
        self.request_terminal_focus_restore();
        cx.notify();
    }

    fn retry_startup(&mut self, cx: &mut Context<Self>) {
        let state = load_native_state_with_done_limit(&self.config, self.done_limit);
        self.spec = state.spec;
        self.settings = state.settings;
        self.pane_layouts = state.pane_layouts;
        self.startup_error = state.startup_error;
        cx.notify();
    }

    fn load_more_done(&mut self, cx: &mut Context<Self>) {
        self.done_limit = self
            .done_limit
            .saturating_add(DONE_PAGE_SIZE)
            .min(self.spec.done_total.max(DONE_PAGE_SIZE));
        self.retry_startup(cx);
    }

    fn load_diff_snapshot(&mut self, task_id: String, force: bool, cx: &mut Context<Self>) {
        if self.diff_snapshots_loading.contains(&task_id)
            || (!force && self.diff_snapshots.contains_key(&task_id))
        {
            return;
        }
        self.diff_snapshots_loading.insert(task_id.clone());
        let config = self.config.clone();
        let worker_task_id = task_id.clone();
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let result = config
                .ensure_database_file()
                .map_err(|error| error.to_string())
                .and_then(|path| load_native_diff_snapshot(path, &worker_task_id));
            let _ = sender.send(result);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => {
                        Err("Diff loader stopped unexpectedly.".to_owned())
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.diff_snapshots_loading.remove(&task_id);
                    if let Ok(snapshot) = &result
                        && let Some(first) = snapshot.files.first()
                        && this
                            .selected_diff_file_path
                            .as_deref()
                            .is_none_or(|selected| {
                                !snapshot.files.iter().any(|file| file.path == selected)
                            })
                    {
                        this.selected_diff_file_path = Some(first.path.clone());
                        this.viewed_diff_files
                            .insert((task_id.clone(), first.path.clone()));
                    }
                    this.diff_snapshots.insert(task_id.clone(), result);
                    cx.notify();
                });
                break;
            }
        })
        .detach();
        cx.notify();
    }

    fn select_diff_file(&mut self, task_id: String, path: String, cx: &mut Context<Self>) {
        self.selected_diff_file_path = Some(path.clone());
        self.viewed_diff_files.insert((task_id, path));
        cx.notify();
    }

    fn resize_diff_sidebar(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.diff_sidebar_width =
            (self.diff_sidebar_width + delta).clamp(DIFF_SIDEBAR_MIN_WIDTH, DIFF_SIDEBAR_MAX_WIDTH);
        cx.notify();
    }

    fn load_task_ai_sessions(&mut self, task_id: String, cx: &mut Context<Self>) {
        if self.ai_sessions.contains_key(&task_id)
            || !self.ai_sessions_loading.insert(task_id.clone())
        {
            return;
        }
        self.load_task_ai_sessions_page(task_id, None, false, cx);
    }

    fn load_task_ai_sessions_page(
        &mut self,
        task_id: String,
        cursor: Option<String>,
        append: bool,
        cx: &mut Context<Self>,
    ) {
        if append && !self.ai_sessions_loading.insert(task_id.clone()) {
            return;
        }
        let config = self.config.clone();
        let worker_task_id = task_id.clone();
        let query = (!self.ai_session_query.is_empty()).then(|| self.ai_session_query.clone());
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let result = config
                .ensure_database_file()
                .map_err(|error| error.to_string())
                .and_then(|database_path| {
                    load_native_task_ai_sessions(
                        database_path,
                        &worker_task_id,
                        query,
                        cursor,
                        None,
                    )
                    .and_then(|sessions| {
                        sessions
                            .ok_or_else(|| "Task is not linked to an available project.".to_owned())
                    })
                });
            let _ = sender.send(result);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => {
                        Err("AI session worker stopped unexpectedly.".to_owned())
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.ai_sessions_loading.remove(&task_id);
                    if append
                        && let Ok(page) = &result
                        && let Some(Ok(existing)) = this.ai_sessions.get_mut(&task_id)
                    {
                        existing.sessions.extend(page.sessions.clone());
                        existing.next_cursor = page.next_cursor.clone();
                        existing.sources = page.sources.clone();
                    } else {
                        this.ai_sessions.insert(task_id.clone(), result);
                    }
                    cx.notify();
                });
                break;
            }
        })
        .detach();
    }

    fn search_task_ai_sessions(&mut self, task_id: String, cx: &mut Context<Self>) {
        self.ai_session_query = self.ai_search_input.read(cx).value().trim().to_owned();
        self.ai_sessions.remove(&task_id);
        self.ai_session_details
            .retain(|(detail_task_id, _), _| detail_task_id != &task_id);
        self.load_task_ai_sessions(task_id, cx);
    }

    fn load_more_task_ai_sessions(&mut self, task_id: String, cx: &mut Context<Self>) {
        let cursor = self
            .ai_sessions
            .get(&task_id)
            .and_then(|result| result.as_ref().ok())
            .and_then(|page| page.next_cursor.clone());
        if cursor.is_some() {
            self.load_task_ai_sessions_page(task_id, cursor, true, cx);
        }
    }

    fn toggle_ai_provider_filter(&mut self, provider: AiSessionProvider, cx: &mut Context<Self>) {
        if !self.ai_provider_filters.remove(&provider) {
            self.ai_provider_filters.insert(provider);
        }
        cx.notify();
    }

    fn toggle_ai_role_filter(&mut self, role: AiMessageRole, cx: &mut Context<Self>) {
        if !self.ai_role_filters.remove(&role) {
            self.ai_role_filters.insert(role);
        }
        self.ai_session_details.clear();
        cx.notify();
    }

    fn load_task_ai_session_detail(
        &mut self,
        task_id: String,
        provider: AiSessionProvider,
        session_id: String,
        source_ref: String,
        cx: &mut Context<Self>,
    ) {
        let key = (task_id.clone(), session_id.clone());
        if self.ai_session_details.contains_key(&key)
            || !self.ai_session_details_loading.insert(key.clone())
        {
            return;
        }
        let config = self.config.clone();
        let worker_task_id = task_id.clone();
        let worker_session_id = session_id.clone();
        let roles = self.ai_role_filters.iter().copied().collect();
        let query = (!self.ai_session_query.is_empty()).then(|| self.ai_session_query.clone());
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let result = config
                .ensure_database_file()
                .map_err(|error| error.to_string())
                .and_then(|database_path| {
                    load_native_task_ai_session_detail(
                        database_path,
                        &worker_task_id,
                        provider,
                        &worker_session_id,
                        Some(source_ref),
                        query,
                        roles,
                        None,
                        None,
                    )
                    .and_then(|detail| {
                        detail.ok_or_else(|| "AI session detail was not found.".to_owned())
                    })
                });
            let _ = sender.send(result);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => {
                        Err("AI session detail worker stopped unexpectedly.".to_owned())
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.ai_session_details_loading.remove(&key);
                    this.ai_session_details.insert(key.clone(), result);
                    cx.notify();
                });
                break;
            }
        })
        .detach();
    }

    fn navigate(&mut self, route: NativeRoute, cx: &mut Context<Self>) {
        if route == *self.navigation.current() {
            return;
        }
        let task_detail_id = match &route {
            NativeRoute::TaskDetail { task_id, .. } => Some(task_id.clone()),
            _ => None,
        };
        let diff_task_id = match &route {
            NativeRoute::Diff { task_id, .. } => Some(task_id.clone()),
            _ => None,
        };
        self.navigation.navigate(route);
        if let Some(task_id) = task_detail_id {
            self.load_task_ai_sessions(task_id, cx);
        }
        if let Some(task_id) = diff_task_id {
            self.load_diff_snapshot(task_id, false, cx);
        }
        cx.notify();
    }

    fn go_back(&mut self, cx: &mut Context<Self>) {
        if self.navigation.go_back().is_some() {
            if let NativeRoute::Diff { task_id, .. } = self.navigation.current().clone() {
                self.load_diff_snapshot(task_id, false, cx);
            }
            cx.notify();
        }
    }

    fn go_forward(&mut self, cx: &mut Context<Self>) {
        if self.navigation.go_forward().is_some() {
            if let NativeRoute::Diff { task_id, .. } = self.navigation.current().clone() {
                self.load_diff_snapshot(task_id, false, cx);
            }
            cx.notify();
        }
    }

    fn render_navigation(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.navigation.current().locale();
        let unread_count = self
            .notification_snapshot
            .as_ref()
            .map(|notifications| {
                notifications
                    .iter()
                    .filter(|notification| !notification.is_read)
                    .count()
            })
            .unwrap_or_default();
        let settings_label = self.message("common.settings", "Settings");
        let notifications_label = self.message("common.notifications", "Notifications");
        let notification_button_label = if unread_count > 0 {
            self.formatted_message(
                "common.unreadCount",
                "{count} unread",
                &[("count", &unread_count.to_string())],
            )
        } else {
            notifications_label.clone()
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .mb_4()
            .child(
                Button::new("history-back")
                    .label("←")
                    .on_click(cx.listener(|this, _, _, cx| this.go_back(cx))),
            )
            .child(
                Button::new("history-forward")
                    .label("→")
                    .on_click(cx.listener(|this, _, _, cx| this.go_forward(cx))),
            )
            .child(
                Button::new("board-route")
                    .label("Board")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.navigate(NativeRoute::Board { locale }, cx);
                    })),
            )
            .child(
                Button::new("settings-route")
                    .label(settings_label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.navigate(NativeRoute::Settings { locale }, cx);
                    })),
            )
            .child(
                Button::new("pane-layout-route")
                    .label(self.message("settings.paneLayoutLink", "Pane Layout"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.navigate(NativeRoute::PaneLayout { locale }, cx);
                    })),
            )
            .child(
                Button::new("notification-center")
                    .label(notification_button_label)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.notification_center_open = !this.notification_center_open;
                        this.notification_snapshot = this.notifications.list();
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .ml_auto()
                    .text_sm()
                    .text_color(rgb(0xaeb3bd))
                    .child(self.navigation.current().path()),
            )
    }

    fn activate_in_app_notification(
        &mut self,
        notification: AppNotification,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.notifications.mark_read(&notification.id) {
            self.mutation_error = Some(error);
            cx.notify();
            return;
        }
        self.notification_snapshot = self.notifications.list();
        self.notification_center_open = false;
        self.request_terminal_focus_restore();
        let action_type = notification
            .action
            .as_ref()
            .and_then(|action| action.get("type"))
            .and_then(serde_json::Value::as_str);
        if should_keep_current_route_for_notification_activation(action_type) {
            self.background_review_open = true;
            cx.notify();
        } else {
            self.navigate(parse_native_route(&notification.relative_path), cx);
        }
    }

    fn mark_all_notifications_read(&mut self, cx: &mut Context<Self>) {
        match self.notifications.mark_all_read() {
            Ok(_) => self.notification_snapshot = self.notifications.list(),
            Err(error) => self.mutation_error = Some(error),
        }
        cx.notify();
    }

    fn close_notification_center(&mut self, cx: &mut Context<Self>) {
        self.notification_center_open = false;
        self.request_terminal_focus_restore();
        cx.notify();
    }

    fn render_notification_center(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.notification_center_open {
            return div().into_any_element();
        }
        let content = match &self.notification_snapshot {
            Err(error) => {
                render_error_state("Notifications unavailable", error.clone()).into_any_element()
            }
            Ok(notifications) if notifications.is_empty() => div()
                .p_4()
                .text_color(rgb(0xaeb3bd))
                .child(self.message("common.noNotifications", "No notifications"))
                .into_any_element(),
            Ok(notifications) => notifications
                .iter()
                .cloned()
                .enumerate()
                .fold(
                    div().flex().flex_col().gap_2(),
                    |rows, (index, notification)| {
                        let label = if notification.is_read {
                            "Read"
                        } else {
                            "Unread"
                        };
                        rows.child(
                            Button::new(("notification-row", index))
                                .w_full()
                                .label(format!(
                                    "{label} · {}\n{}\n{}",
                                    notification.title, notification.body, notification.created_at
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.activate_in_app_notification(notification.clone(), cx);
                                })),
                        )
                    },
                )
                .into_any_element(),
        };

        div()
            .absolute()
            .top(px(58.0))
            .right(px(20.0))
            .w(px(440.0))
            .max_h(px(620.0))
            .overflow_y_scrollbar()
            .rounded_md()
            .border_1()
            .border_color(rgb(0x334155))
            .bg(rgb(0x171b23))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(self.message("common.notifications", "Notifications"))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("notification-mark-all")
                                    .label(self.message("common.markAllRead", "Mark all read"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.mark_all_notifications_read(cx);
                                    })),
                            )
                            .child(
                                Button::new("notification-close")
                                    .label(self.message("common.close", "Close"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_notification_center(cx);
                                    })),
                            ),
                    ),
            )
            .child(content)
            .into_any_element()
    }

    fn close_release_update(&mut self, cx: &mut Context<Self>) {
        if self.release_update_dont_show_again
            && let Some(release) = &self.release_update
        {
            let result = self
                .config
                .ensure_database_file()
                .map_err(|error| error.to_string())
                .and_then(|path| dismiss_native_release_update(path, &release.version));
            match result {
                Ok(()) => {
                    if let Ok(settings) = &mut self.settings
                        && !settings
                            .release_update_dismissed_versions
                            .contains(&release.version)
                    {
                        settings
                            .release_update_dismissed_versions
                            .push(release.version.clone());
                    }
                }
                Err(error) => {
                    log_native_failure(&self.config, "release-update-dismiss-error", &error, None);
                    self.mutation_error = Some(error);
                }
            }
        }
        self.release_update = None;
        self.release_update_dont_show_again = false;
        self.request_terminal_focus_restore();
        cx.notify();
    }

    fn open_release_update_page(&mut self, cx: &mut Context<Self>) {
        if let Some(release) = &self.release_update {
            cx.open_url(&release.html_url);
        }
        self.close_release_update(cx);
    }

    fn render_release_update(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(release) = &self.release_update else {
            return div().into_any_element();
        };
        let release_name = release.name.clone();
        let release_version = release.version.clone();
        let can_install = release.installer.is_some();
        let install_label = if self.release_update_installing {
            "Downloading and verifying…"
        } else {
            "Install update"
        };
        let release_body = if release.body.is_empty() {
            self.message(
                "common.releaseUpdate.emptyBody",
                "No release notes were provided.",
            )
        } else {
            release.body.clone()
        };
        let dismiss_label = if self.release_update_dont_show_again {
            format!(
                "✓ {}",
                self.message(
                    "common.releaseUpdate.dontShowVersionAgain",
                    "Don’t show this version again"
                )
            )
        } else {
            self.message(
                "common.releaseUpdate.dontShowVersionAgain",
                "Don’t show this version again",
            )
        };
        let title = self.formatted_message(
            "common.releaseUpdate.title",
            "KanVibe {version} is available",
            &[("version", &release_version)],
        );

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x000000aa))
            .child(
                div()
                    .w(px(760.0))
                    .tab_group()
                    .max_h(px(680.0))
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_lg().child(title))
                    .child(div().text_sm().child(release_name))
                    .child(
                        div()
                            .max_h(px(480.0))
                            .overflow_y_scrollbar()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .p_4()
                            .text_sm()
                            .child(release_body),
                    )
                    .when_some(self.release_update_error.clone(), |panel, error| {
                        panel.child(div().text_sm().text_color(rgb(0xdc2626)).child(error))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("release-update-dismiss-version")
                                    .label(dismiss_label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.release_update_dont_show_again =
                                            !this.release_update_dont_show_again;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .ml_auto()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("release-update-close")
                                            .label(self.message("common.close", "Close"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.close_release_update(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("release-update-view")
                                            .label(self.message(
                                                "common.releaseUpdate.viewRelease",
                                                "View release",
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.open_release_update_page(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("release-update-install")
                                            .primary()
                                            .disabled(
                                                !can_install || self.release_update_installing,
                                            )
                                            .label(install_label)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.install_release_update(cx);
                                            })),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_task_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.show_task_editor {
            return div().into_any_element();
        }

        let editor_title = if self.editing_task_id.is_some() {
            "Edit Task"
        } else if self.branching_task_id.is_some() {
            "Create Branch for Task"
        } else {
            "Create Task"
        };
        let submit_label = if self.task_editor_submitting && self.editing_task_id.is_some() {
            "Saving…"
        } else if self.task_editor_submitting && self.branching_task_id.is_some() {
            "Creating branch…"
        } else if self.task_editor_submitting {
            "Creating…"
        } else if self.editing_task_id.is_some() {
            "Save"
        } else if self.branching_task_id.is_some() {
            "Create branch"
        } else {
            "Create"
        };
        let error = self
            .mutation_error
            .as_ref()
            .map(|error| render_error_state("Task was not saved", error.clone()).into_any_element())
            .unwrap_or_else(|| div().into_any_element());
        let is_editing = self.editing_task_id.is_some();
        let project_choices = self
            .spec
            .projects
            .iter()
            .filter(|project| !project.is_worktree)
            .enumerate()
            .fold(
                div().flex().gap_2().flex_wrap(),
                |choices, (index, project)| {
                    let project_id = project.id.clone();
                    let default_branch = project.default_branch.clone();
                    let selected =
                        self.task_form_project_id.as_deref() == Some(project.id.as_str());
                    choices.child(
                        Button::new(("task-form-project", index))
                            .label(if selected {
                                format!("✓ {}", project.name)
                            } else {
                                project.name.clone()
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_task_form_project(
                                    project_id.clone(),
                                    default_branch.clone(),
                                    window,
                                    cx,
                                );
                            })),
                    )
                },
            );
        let priority_choices = [
            (None, "None"),
            (Some(TaskPriority::Low), "Low"),
            (Some(TaskPriority::Medium), "Medium"),
            (Some(TaskPriority::High), "High"),
        ]
        .into_iter()
        .enumerate()
        .fold(
            div().flex().gap_2(),
            |choices, (index, (priority, label))| {
                let selected = self.task_form_priority == priority;
                choices.child(
                    Button::new(("task-form-priority", index))
                        .label(if selected {
                            format!("✓ {label}")
                        } else {
                            label.to_owned()
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_task_form_priority(priority, cx);
                        })),
                )
            },
        );
        let session_choices = [SessionType::Tmux, SessionType::Zellij]
            .into_iter()
            .enumerate()
            .fold(div().flex().gap_2(), |choices, (index, session_type)| {
                let selected = self.task_form_session_type == session_type;
                choices.child(
                    Button::new(("task-form-session", index))
                        .label(if selected {
                            format!("✓ {}", session_type.as_str())
                        } else {
                            session_type.as_str().to_owned()
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_task_form_session_type(session_type, cx);
                        })),
                )
            });

        div()
            .tab_group()
            .key_context("TaskEditor")
            .flex()
            .flex_col()
            .gap_3()
            .mb_4()
            .rounded_md()
            .border_1()
            .border_color(rgb(0x293241))
            .p_4()
            .child(div().text_xl().child(editor_title))
            .when(!is_editing, |form| {
                form.child(div().text_sm().child("Project *"))
                    .child(project_choices)
                    .child(div().text_sm().child("Base branch"))
                    .child(Input::new(&self.task_base_branch_input))
            })
            .child(div().text_sm().child(if is_editing {
                "Task title *"
            } else {
                "Branch name *"
            }))
            .child(Input::new(&self.task_title_input))
            .child(div().text_sm().child("Description"))
            .child(Input::new(&self.task_description_input).h(px(96.0)))
            .child(div().text_sm().child("Priority"))
            .child(priority_choices)
            .when(!is_editing, |form| {
                form.child(div().text_sm().child("Session type"))
                    .child(session_choices)
            })
            .child(error)
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("create-task-submit")
                            .primary()
                            .disabled(self.task_editor_submitting)
                            .label(submit_label)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.submit_task_editor(window, cx);
                            })),
                    )
                    .child(
                        Button::new("create-task-cancel")
                            .disabled(self.task_editor_submitting)
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_task_editor(cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_quick_search(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.show_quick_search {
            return div().into_any_element();
        }

        let matches = crate::search_native_cards(
            self.spec
                .columns
                .iter()
                .flat_map(|column| column.cards.iter()),
            &self.quick_search_query,
        );
        let locale = self.navigation.current().locale();
        let results = matches.into_iter().take(20).enumerate().fold(
            div().flex().flex_col().gap_2(),
            |results, (index, card)| {
                let task_id = card.id.clone();
                let subtitle = [
                    card.project_name.as_deref(),
                    card.branch_name.as_deref(),
                    card.ssh_host.as_deref(),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
                results.child(
                    Button::new(("quick-search-result", index))
                        .label(if subtitle.is_empty() {
                            card.title.clone()
                        } else {
                            format!("{} — {subtitle}", card.title)
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.show_quick_search = false;
                            this.navigate(
                                NativeRoute::TaskDetail {
                                    locale,
                                    task_id: task_id.clone(),
                                },
                                cx,
                            );
                        })),
                )
            },
        );

        div()
            .tab_group()
            .key_context("QuickSearch")
            .flex()
            .flex_col()
            .gap_3()
            .mb_4()
            .rounded_md()
            .border_1()
            .border_color(rgb(0x0064ff))
            .bg(rgb(0x171b23))
            .p_4()
            .child(div().text_xl().child("Quick Search"))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(Input::new(&self.quick_search_input).w_full())
                    .child(
                        Button::new("quick-search-apply")
                            .primary()
                            .label("Search")
                            .on_click(cx.listener(|this, _, _, cx| this.apply_quick_search(cx))),
                    )
                    .child(
                        Button::new("quick-search-close")
                            .label("Close")
                            .on_click(cx.listener(|this, _, _, cx| this.close_quick_search(cx))),
                    ),
            )
            .child(results)
            .into_any_element()
    }

    fn render_vim_command(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.show_vim_command {
            return div().into_any_element();
        }
        let focused_label = self
            .focused_task_id
            .as_ref()
            .and_then(|task_id| {
                self.spec
                    .columns
                    .iter()
                    .flat_map(|column| column.cards.iter())
                    .find(|card| card.id == *task_id)
                    .map(|card| card.title.clone())
            })
            .unwrap_or_else(|| "No focused task".to_owned());

        div()
            .tab_group()
            .key_context("VimCommand")
            .flex()
            .flex_col()
            .gap_2()
            .mb_4()
            .rounded_md()
            .border_1()
            .border_color(rgb(0x0064ff))
            .bg(rgb(0x171b23))
            .p_3()
            .child(format!("Command for {focused_label}"))
            .child(Input::new(&self.vim_command_input))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("vim-command-submit")
                            .primary()
                            .label("Run")
                            .on_click(cx.listener(|this, _, _, cx| this.submit_vim_command(cx))),
                    )
                    .child(
                        Button::new("vim-command-close")
                            .label("Close")
                            .on_click(cx.listener(|this, _, _, cx| this.close_vim_command(cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_keyboard_context_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.show_keyboard_context_menu {
            return div().into_any_element();
        }
        let Some(card) = self.focused_task_id.as_ref().and_then(|task_id| {
            self.spec
                .columns
                .iter()
                .flat_map(|column| column.cards.iter())
                .find(|card| card.id == *task_id)
        }) else {
            return div().into_any_element();
        };
        let edit_task_id = card.id.clone();
        let edit_title = card.title.clone();
        let branch_task_id = card.id.clone();
        let can_create_branch = card.branch_name.is_none() && card.worktree_path.is_none();
        let status_actions = TaskStatus::ALL.into_iter().enumerate().fold(
            div().flex().gap_2().flex_wrap(),
            |actions, (index, status)| {
                let task_id = card.id.clone();
                actions.child(
                    Button::new(("keyboard-context-status", index))
                        .label(format!(
                            "{}{}",
                            if card.status == status.as_str() {
                                "✓ "
                            } else {
                                ""
                            },
                            status.as_str()
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.show_keyboard_context_menu = false;
                            this.change_task_status(task_id.clone(), status, cx);
                        })),
                )
            },
        );
        let delete_task_id = card.id.clone();
        let locale = self.navigation.current().locale();

        div()
            .tab_group()
            .flex()
            .flex_col()
            .gap_2()
            .mb_4()
            .rounded_md()
            .border_1()
            .border_color(rgb(0x0064ff))
            .bg(rgb(0x171b23))
            .p_3()
            .child(format!("Task menu — {}", card.title))
            .child(
                Button::new("keyboard-context-edit")
                    .label("Edit task")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.show_keyboard_context_menu = false;
                        this.open_task_title_editor(
                            edit_task_id.clone(),
                            edit_title.clone(),
                            window,
                            cx,
                        );
                    })),
            )
            .when(can_create_branch, |menu| {
                menu.child(
                    Button::new("keyboard-context-create-branch")
                        .label("Create branch")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.show_keyboard_context_menu = false;
                            this.open_branch_task_editor(branch_task_id.clone(), window, cx);
                        })),
                )
            })
            .child(status_actions)
            .child(
                Button::new("keyboard-context-delete")
                    .label("Delete task")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.show_keyboard_context_menu = false;
                        this.navigate(
                            NativeRoute::TaskDetail {
                                locale,
                                task_id: delete_task_id.clone(),
                            },
                            cx,
                        );
                        this.request_task_delete(delete_task_id.clone(), cx);
                    })),
            )
            .child(
                Button::new("keyboard-context-close")
                    .label("Close")
                    .on_click(cx.listener(|this, _, _, cx| this.close_keyboard_context_menu(cx))),
            )
            .into_any_element()
    }

    fn render_done_confirmation(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.pending_done_mutation.is_none() {
            return div().into_any_element();
        }
        let checkbox_label = if self.done_confirm_dont_ask_again {
            "☑ Don't ask again"
        } else {
            "☐ Don't ask again"
        };

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x090b10))
            .child(
                div()
                    .w(px(420.0))
                    .tab_group()
                    .key_context("DoneConfirmation")
                    .flex()
                    .flex_col()
                    .gap_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0x334155))
                    .bg(rgb(0x171b23))
                    .p_5()
                    .child(
                        div()
                            .text_lg()
                            .child("Move task to Done?"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xaab4c3))
                            .child("The linked terminal session, worktree, and branch will be cleaned up. The task transition is rolled back if cleanup fails."),
                    )
                    .child(
                        Button::new("done-confirm-dont-ask")
                            .label(checkbox_label)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.done_confirm_dont_ask_again =
                                    !this.done_confirm_dont_ask_again;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("done-confirm-cancel")
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.cancel_done_mutation(cx);
                                    })),
                            )
                            .child(
                                Button::new("done-confirm-submit")
                                    .primary()
                                    .label("Confirm")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_done_mutation(cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_board(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(error) = &self.startup_error {
            return route_panel(
                "Board unavailable",
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(render_error_state(
                        "KanVibe could not load its data",
                        error.clone(),
                    ))
                    .child(
                        Button::new("retry-startup")
                            .primary()
                            .label("Retry")
                            .on_click(cx.listener(|this, _, _, cx| this.retry_startup(cx))),
                    ),
            )
            .into_any_element();
        }

        let columns = self
            .spec
            .columns
            .iter()
            .fold(div().flex().gap_3().w_full(), |row, column| {
                row.child(self.render_board_column(column, cx))
            });
        let project_filters = self.spec.projects.iter().enumerate().fold(
            div().flex().gap_2().flex_wrap().child(
                Button::new("project-filter-all")
                    .label(if self.selected_project_ids.is_empty() {
                        "✓ All Projects"
                    } else {
                        "All Projects"
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_project_filter(None, cx))),
            ),
            |filters, (index, project)| {
                let project_id = project.id.clone();
                let selected = self.selected_project_ids.contains(&project_id);
                filters.child(
                    Button::new(("project-filter", index))
                        .label(if selected {
                            format!("✓ {}", project.name)
                        } else {
                            project.name.clone()
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_project_filter(Some(project_id.clone()), cx);
                        })),
                )
            },
        );
        let find_match_count = self
            .spec
            .columns
            .iter()
            .flat_map(|column| column.cards.iter())
            .filter(|card| {
                !self.board_search_query.is_empty()
                    && card
                        .title
                        .to_lowercase()
                        .contains(&self.board_search_query.to_lowercase())
            })
            .count();
        let board_controls = div()
            .flex()
            .flex_col()
            .gap_2()
            .mb_3()
            .when(self.show_board_find, |controls| {
                controls.child(
                    div()
                        .flex()
                        .gap_2()
                        .items_center()
                        .child(Input::new(&self.board_search_input).w_full())
                        .child(
                            Button::new("board-search-apply")
                                .primary()
                                .label("Find")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.apply_board_search(cx)),
                                ),
                        )
                        .child(format!("{find_match_count} matches"))
                        .child(Button::new("board-search-clear").label("Close").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.clear_board_search(window, cx);
                            }),
                        )),
                )
            })
            .child(
                Button::new("board-project-filter")
                    .label(if self.selected_project_ids.is_empty() {
                        "Projects"
                    } else {
                        "Projects (filtered)"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_project_filter_visibility(cx);
                    })),
            )
            .when(self.show_project_filter, |controls| {
                controls.child(project_filters)
            });

        div()
            .key_context("Board")
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xl()
                                    .text_color(rgb(0xf8fafc))
                                    .child(self.spec.window_title.clone()),
                            )
                            .child(format!(
                                "{} projects · {} visible tasks · {} done",
                                self.spec.project_count,
                                self.spec.total_visible_tasks,
                                self.spec.done_total
                            )),
                    )
                    .child(
                        Button::new("new-task")
                            .primary()
                            .label(self.spec.primary_action_label.clone())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_task_editor(window, cx);
                            })),
                    ),
            )
            .child(board_controls)
            .child(
                div()
                    .mb_3()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0x202632))
                    .child(format!(
                        "{} · route {} · locale {}",
                        self.spec.all_projects_label,
                        self.spec.route,
                        self.spec.locale.code()
                    )),
            )
            .child(columns)
            .into_any_element()
    }

    fn render_board_column(
        &self,
        column: &NativeUiColumnSpec,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let visible_cards =
            filter_native_cards_by_projects(column.cards.iter(), "", &self.selected_project_ids);
        let visible_count = visible_cards.len();
        let cards = visible_cards
            .iter()
            .fold(div().flex().flex_col().gap_2(), |list, card| {
                list.child(self.render_task_card(card, cx))
            });
        let destination_status = column.status;

        div()
            .flex_1()
            .min_w(px(180.0))
            .rounded_md()
            .border_1()
            .border_color(rgb(0x293241))
            .bg(rgb(0x171b23))
            .p_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb_3()
                    .child(
                        div()
                            .text_color(rgb(color_to_u32(column.color)))
                            .child(column.label.clone()),
                    )
                    .child(format!("{visible_count}")),
            )
            .child(if visible_count == 0 {
                div()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(0x293241))
                    .bg(rgb(0x111827))
                    .p_3()
                    .child("No tasks")
            } else {
                cards
            })
            .when(
                column.status == TaskStatus::Done
                    && column.task_count < self.spec.done_total as usize,
                |column| {
                    column.child(
                        Button::new("load-more-done")
                            .label(format!(
                                "Load more ({}/{})",
                                self.spec
                                    .columns
                                    .iter()
                                    .find(|column| column.status == TaskStatus::Done)
                                    .map_or(0, |column| column.task_count),
                                self.spec.done_total
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.load_more_done(cx))),
                    )
                },
            )
            .on_drop(cx.listener(move |this, drag: &TaskDragInfo, _, cx| {
                this.drop_task_on_column(drag.clone(), destination_status, cx);
            }))
    }

    fn open_project_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_project_editor = true;
        self.mutation_error = None;
        self.project_name_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        self.project_path_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.project_ssh_host_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }

    fn close_project_editor(&mut self, cx: &mut Context<Self>) {
        self.show_project_editor = false;
        self.mutation_error = None;
        self.request_terminal_focus_restore();
        cx.notify();
    }

    fn submit_project_editor(&mut self, cx: &mut Context<Self>) {
        if self.project_registration_in_progress {
            return;
        }
        let name = self.project_name_input.read(cx).value().to_string();
        let path = self.project_path_input.read(cx).value().to_string();
        let ssh_host = self.project_ssh_host_input.read(cx).value().to_string();
        let config = self.config.clone();
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let result = config
                .ensure_database_file()
                .map_err(|error| error.to_string())
                .and_then(|database_path| {
                    if ssh_host.trim().is_empty() {
                        register_native_local_project(database_path, &name, path.trim())
                    } else {
                        register_native_remote_project(
                            database_path,
                            &name,
                            path.trim(),
                            ssh_host.trim(),
                        )
                    }
                });
            let _ = sender.send(result);
        });
        self.project_registration_in_progress = true;
        self.mutation_error = None;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => {
                        Err("Project registration worker stopped unexpectedly.".to_owned())
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.project_registration_in_progress = false;
                    match result {
                        Ok(_) => {
                            this.show_project_editor = false;
                            this.mutation_error = None;
                            this.retry_startup(cx);
                        }
                        Err(error) => {
                            this.mutation_error = Some(error);
                            cx.notify();
                        }
                    }
                });
                break;
            }
        })
        .detach();
        cx.notify();
    }

    fn confirm_project_delete(&mut self, cx: &mut Context<Self>) {
        let Some(project_id) = self.project_delete_confirmation_id.clone() else {
            return;
        };
        let result = self
            .config
            .ensure_database_file()
            .map_err(|error| error.to_string())
            .and_then(|database_path| delete_native_project(database_path, &project_id));
        match result {
            Ok(true) => {
                self.project_delete_confirmation_id = None;
                self.mutation_error = None;
                self.retry_startup(cx);
            }
            Ok(false) => {
                self.mutation_error = Some("Project no longer exists.".to_owned());
                cx.notify();
            }
            Err(error) => {
                self.mutation_error = Some(error);
                cx.notify();
            }
        }
    }

    fn submit_project_scan(&mut self, cx: &mut Context<Self>) {
        let root_path = self.project_scan_root_input.read(cx).value().to_string();
        let result = self
            .config
            .ensure_database_file()
            .map_err(|error| error.to_string())
            .and_then(|database_path| {
                scan_and_register_native_local_projects(database_path, root_path.trim())
            });
        match result {
            Ok(result) => {
                let registered = result.registered.len();
                let skipped = result.skipped.len();
                let worktrees = result.registered_worktrees.len();
                let errors = result.errors.len();
                self.project_scan_summary = Some(format!(
                    "Scan complete: {registered} projects and {worktrees} worktrees registered, \
                     {skipped} skipped, {errors} failed."
                ));
                self.mutation_error = if result.errors.is_empty() {
                    None
                } else {
                    Some(result.errors.join("\n"))
                };
                self.retry_startup(cx);
            }
            Err(error) => {
                self.project_scan_summary = None;
                self.mutation_error = Some(error);
                cx.notify();
            }
        }
    }

    fn render_project_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.show_project_editor {
            return div().into_any_element();
        }
        let error = self
            .mutation_error
            .as_ref()
            .map(|error| {
                render_error_state("Project was not registered", error.clone()).into_any_element()
            })
            .unwrap_or_else(|| div().into_any_element());
        let submit_label = if self.project_registration_in_progress {
            "Registering..."
        } else {
            "Register"
        };
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x090b10))
            .child(
                div()
                    .w(px(520.0))
                    .tab_group()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0x334155))
                    .bg(rgb(0x171b23))
                    .p_5()
                    .child("Register Git project")
                    .child(Input::new(&self.project_name_input))
                    .child(Input::new(&self.project_path_input))
                    .child(Input::new(&self.project_ssh_host_input))
                    .child(error)
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("project-register-cancel")
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_project_editor(cx);
                                    })),
                            )
                            .child(
                                Button::new("project-register-submit")
                                    .primary()
                                    .label(submit_label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_project_editor(cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn apply_settings_patch(&mut self, patch: NativeSettingsPatch, cx: &mut Context<Self>) {
        let updates_task_search_shortcut = patch.task_search_shortcut.is_some();
        let updates_theme = patch.theme_preference.is_some();
        let result = self
            .config
            .ensure_database_file()
            .map_err(|error| error.to_string())
            .and_then(|path| update_native_settings(path, self.config.locale, patch));
        match result {
            Ok(settings) => {
                self.background_sync
                    .reconfigure(settings.background_sync.clone());
                if updates_task_search_shortcut {
                    bind_native_keys(cx, &settings.task_search_shortcut);
                }
                if updates_theme {
                    let mode = match settings.theme_preference {
                        ThemePreference::Dark => ThemeMode::Dark,
                        ThemePreference::Light => ThemeMode::Light,
                        ThemePreference::System => {
                            if matches!(
                                cx.window_appearance(),
                                WindowAppearance::Dark | WindowAppearance::VibrantDark
                            ) {
                                ThemeMode::Dark
                            } else {
                                ThemeMode::Light
                            }
                        }
                    };
                    Theme::change(mode, None, cx);
                }
                self.settings = Ok(settings);
                self.mutation_error = None;
            }
            Err(error) => {
                log_native_failure(&self.config, "update-settings-error", &error, None);
                self.mutation_error = Some(error);
            }
        }
        cx.notify();
    }

    fn submit_task_search_shortcut(&mut self, cx: &mut Context<Self>) {
        let shortcut = self.task_search_shortcut_input.read(cx).value().to_string();
        self.apply_settings_patch(
            NativeSettingsPatch {
                task_search_shortcut: Some(shortcut),
                ..NativeSettingsPatch::default()
            },
            cx,
        );
    }

    fn capture_task_search_shortcut(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.task_search_shortcut_recording {
            return;
        }
        window.prevent_default();
        cx.stop_propagation();
        if event.is_held {
            return;
        }

        let key = event.keystroke.key.trim().to_ascii_lowercase();
        let is_modifier = matches!(
            key.as_str(),
            "shift"
                | "control"
                | "ctrl"
                | "alt"
                | "option"
                | "command"
                | "cmd"
                | "meta"
                | "fn"
                | "function"
        );
        if is_modifier {
            return;
        }
        if matches!(key.as_str(), "escape" | "esc") && !event.keystroke.modifiers.modified() {
            self.task_search_shortcut_recording = false;
            self.mutation_error = None;
            cx.notify();
            return;
        }

        let modifiers = event.keystroke.modifiers;
        match native_shortcut_from_capture(NativeShortcutCapture {
            platform: modifiers.platform,
            control: modifiers.control,
            alt: modifiers.alt,
            shift: modifiers.shift,
            function: modifiers.function,
            key,
        }) {
            Ok(shortcut) => {
                self.task_search_shortcut_input
                    .update(cx, |input, cx| input.set_value(shortcut, window, cx));
                self.task_search_shortcut_recording = false;
                self.mutation_error = None;
            }
            Err(error) => {
                self.task_search_shortcut_recording = false;
                self.mutation_error = Some(error);
            }
        }
        cx.notify();
    }

    fn run_github_cli_operation(
        &mut self,
        ssh_host: Option<String>,
        install: bool,
        cx: &mut Context<Self>,
    ) {
        let target_key = github_cli_target_key(ssh_host.as_deref());
        if !self.github_cli_loading.insert(target_key.clone()) {
            return;
        }
        let config = self.config.clone();
        let worker_ssh_host = ssh_host.clone();
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let result = config
                .ensure_database_file()
                .map_err(|error| error.to_string())
                .and_then(|database_path| {
                    if install {
                        install_native_github_cli(database_path, worker_ssh_host.as_deref())
                    } else {
                        get_native_github_cli_status(database_path, worker_ssh_host.as_deref())
                    }
                });
            let _ = sender.send(result);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => {
                        Err("GitHub CLI dependency worker stopped unexpectedly.".to_owned())
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.github_cli_loading.remove(&target_key);
                    this.github_cli_statuses.insert(target_key, result);
                    cx.notify();
                });
                break;
            }
        })
        .detach();
        cx.notify();
    }

    fn set_project_color(&mut self, project_id: String, color: String, cx: &mut Context<Self>) {
        if !self.project_color_updating.insert(project_id.clone()) {
            return;
        }
        let config = self.config.clone();
        let worker_project_id = project_id.clone();
        let (sender, receiver) = channel();
        self.mutation_error = None;
        std::thread::spawn(move || {
            let result = config
                .ensure_database_file()
                .map_err(|error| error.to_string())
                .and_then(|database_path| {
                    update_native_project_color(database_path, &worker_project_id, &color)
                });
            let _ = sender.send(result);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => {
                        Err("Project color worker stopped unexpectedly.".to_owned())
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.project_color_updating.remove(&project_id);
                    match result {
                        Ok(()) => {
                            this.mutation_error = None;
                            this.retry_startup(cx);
                        }
                        Err(error) => {
                            log_native_failure(
                                &this.config,
                                "save-project-color-error",
                                &error,
                                None,
                            );
                            this.mutation_error = Some(error);
                            cx.notify();
                        }
                    }
                });
                break;
            }
        })
        .detach();
        cx.notify();
    }

    fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let background_snapshot = self.background_sync.snapshot();
        let content = match &self.settings {
            Ok(settings) => {
                let next_theme = match settings.theme_preference {
                    kanvibe_core::ThemePreference::System => kanvibe_core::ThemePreference::Dark,
                    kanvibe_core::ThemePreference::Dark => kanvibe_core::ThemePreference::Light,
                    kanvibe_core::ThemePreference::Light => kanvibe_core::ThemePreference::System,
                };
                let next_session = match settings.default_session_type {
                    SessionType::Tmux => SessionType::Zellij,
                    SessionType::Zellij => SessionType::Tmux,
                };
                let vim_enabled = settings.vim_mode_enabled;
                let sidebar_collapsed = settings.sidebar_default_collapsed;
                let background_enabled = settings.background_sync.is_enabled;
                let notification_enabled = settings.notification_enabled;
                let interval_minutes = settings.background_sync.interval_ms / 60_000;
                let background_status = match background_snapshot.last_result.as_ref() {
                    None => "Waiting for first run".to_owned(),
                    Some(Ok(result)) if result.errors.is_empty() => format!(
                        "Last run: {} registered, {} repaired",
                        result.registered_worktrees, result.repaired_tasks
                    ),
                    Some(Ok(result)) => {
                        format!("Last run completed with {} errors", result.errors.len())
                    }
                    Some(Err(error)) => format!("Last run failed: {error}"),
                };
                let release_update_status = if self.release_update_checking {
                    "Checking…".to_owned()
                } else if let Some(error) = &self.release_update_error {
                    format!("Check failed: {error}")
                } else if let Some(release) = &self.release_update {
                    format!("Version {} available", release.version)
                } else {
                    format!("KanVibe {} is current", env!("CARGO_PKG_VERSION"))
                };
                let next_interval = match interval_minutes {
                    0..=5 => 15,
                    6..=15 => 30,
                    16..=30 => 60,
                    _ => 5,
                };
                let shortcut_record_label = if self.task_search_shortcut_recording {
                    "Press shortcut…"
                } else {
                    "Record shortcut"
                };
                let error = self
                    .mutation_error
                    .as_ref()
                    .map(|error| {
                        render_error_state("Settings were not saved", error.clone())
                            .into_any_element()
                    })
                    .unwrap_or_else(|| div().into_any_element());
                let projects = self.spec.projects.iter().enumerate().fold(
                    div().flex().flex_col().gap_2(),
                    |rows, (index, project)| {
                        let project_id = project.id.clone();
                        let confirmed = self.project_delete_confirmation_id.as_deref()
                            == Some(project.id.as_str());
                        let actions = if confirmed {
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    Button::new(("project-delete-confirm", index))
                                        .danger()
                                        .label("Confirm delete")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.confirm_project_delete(cx);
                                        })),
                                )
                                .child(
                                    Button::new(("project-delete-cancel", index))
                                        .label("Cancel")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.project_delete_confirmation_id = None;
                                            cx.notify();
                                        })),
                                )
                        } else {
                            div().child(
                                Button::new(("project-delete", index))
                                    .danger()
                                    .label("Delete")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.project_delete_confirmation_id =
                                            Some(project_id.clone());
                                        cx.notify();
                                    })),
                            )
                        };
                        rows.child(
                            div()
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(0x293241))
                                .p_3()
                                .child(format!("{} · {}", project.name, project.repo_path))
                                .child(actions),
                        )
                    },
                );
                let scan_summary = self
                    .project_scan_summary
                    .as_ref()
                    .map(|summary| div().child(summary.clone()).into_any_element())
                    .unwrap_or_else(|| div().into_any_element());
                let github_cli_targets = std::iter::once(None)
                    .chain(
                        self.spec
                            .projects
                            .iter()
                            .filter_map(|project| project.ssh_host.clone().map(Some)),
                    )
                    .collect::<BTreeSet<_>>();
                let github_cli_dependencies = github_cli_targets.into_iter().enumerate().fold(
                    div().flex().flex_col().gap_2(),
                    |rows, (index, ssh_host)| {
                        let target_key = github_cli_target_key(ssh_host.as_deref());
                        let is_loading = self.github_cli_loading.contains(&target_key);
                        let status = self.github_cli_statuses.get(&target_key);
                        let target_label = ssh_host
                            .as_deref()
                            .map(|host| format!("SSH {host}"))
                            .unwrap_or_else(|| "Local machine".to_owned());
                        let status_label = if is_loading {
                            "Checking…".to_owned()
                        } else {
                            match status {
                                Some(Ok(status)) if status.available => "Available".to_owned(),
                                Some(Ok(status)) if status.blocked_reason.is_some() => format!(
                                    "Install blocked until restart: {}",
                                    status.blocked_reason.as_deref().unwrap_or_default()
                                ),
                                Some(Ok(_)) => "Not installed".to_owned(),
                                Some(Err(error)) => format!("Check failed: {error}"),
                                None => "Not checked".to_owned(),
                            }
                        };
                        let check_host = ssh_host.clone();
                        let install_host = ssh_host.clone();
                        let can_install = !is_loading
                            && !matches!(
                                status,
                                Some(Ok(NativeGitHubCliStatus {
                                    available: true,
                                    ..
                                }))
                            )
                            && !matches!(
                                status,
                                Some(Ok(NativeGitHubCliStatus {
                                    blocked_reason: Some(_),
                                    ..
                                }))
                            );
                        let actions = div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new(("github-cli-check", index))
                                    .label("Check")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.run_github_cli_operation(
                                            check_host.clone(),
                                            false,
                                            cx,
                                        );
                                    })),
                            )
                            .when(can_install, |actions| {
                                actions.child(
                                    Button::new(("github-cli-install", index))
                                        .primary()
                                        .label("Install")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.run_github_cli_operation(
                                                install_host.clone(),
                                                true,
                                                cx,
                                            );
                                        })),
                                )
                            });
                        rows.child(
                            div()
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(0x293241))
                                .p_3()
                                .child(format!("GitHub CLI · {target_label}: {status_label}"))
                                .child(actions),
                        )
                    },
                );
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(error)
                    .child(
                        Button::new("settings-register-project")
                            .primary()
                            .label("Register local project")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_project_editor(window, cx);
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(Input::new(&self.project_scan_root_input))
                            .child(
                                Button::new("settings-scan-projects")
                                    .label("Scan for Git projects")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_project_scan(cx);
                                    })),
                            ),
                    )
                    .child(scan_summary)
                    .child(projects)
                    .child(github_cli_dependencies)
                    .child(
                        Button::new("settings-theme")
                            .label(format!("Theme: {}", settings.theme_preference.as_str()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.apply_settings_patch(
                                    NativeSettingsPatch {
                                        theme_preference: Some(next_theme),
                                        ..NativeSettingsPatch::default()
                                    },
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("settings-session")
                            .label(format!(
                                "Default session: {}",
                                settings.default_session_type.as_str()
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.apply_settings_patch(
                                    NativeSettingsPatch {
                                        default_session_type: Some(next_session),
                                        ..NativeSettingsPatch::default()
                                    },
                                    cx,
                                );
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child("Task search shortcut")
                            .child(Input::new(&self.task_search_shortcut_input).w(px(220.0)))
                            .child(
                                Button::new("settings-task-search-shortcut-record")
                                    .label(shortcut_record_label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.task_search_shortcut_recording =
                                            !this.task_search_shortcut_recording;
                                        this.mutation_error = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("settings-task-search-shortcut-save")
                                    .label("Save shortcut")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_task_search_shortcut(cx);
                                    })),
                            ),
                    )
                    .child(
                        Button::new("settings-vim")
                            .label(format!("Vim mode: {vim_enabled}"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.apply_settings_patch(
                                    NativeSettingsPatch {
                                        vim_mode_enabled: Some(!vim_enabled),
                                        ..NativeSettingsPatch::default()
                                    },
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("settings-sidebar")
                            .label(format!("Sidebar collapsed: {sidebar_collapsed}"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.apply_settings_patch(
                                    NativeSettingsPatch {
                                        sidebar_default_collapsed: Some(!sidebar_collapsed),
                                        ..NativeSettingsPatch::default()
                                    },
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("settings-notifications")
                            .label(format!("Notifications: {notification_enabled}"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.apply_settings_patch(
                                    NativeSettingsPatch {
                                        notification_enabled: Some(!notification_enabled),
                                        ..NativeSettingsPatch::default()
                                    },
                                    cx,
                                );
                            })),
                    )
                    .child(TaskStatus::ALL.into_iter().enumerate().fold(
                        div().flex().gap_2().flex_wrap(),
                        |buttons, (index, status)| {
                            let status_value = status.as_str().to_owned();
                            let selected = settings.notification_statuses.contains(&status_value);
                            let mut next_statuses = settings.notification_statuses.clone();
                            if selected {
                                next_statuses.retain(|value| value != &status_value);
                            } else {
                                next_statuses.push(status_value);
                            }
                            buttons.child(
                                Button::new(("notification-status", index))
                                    .label(format!(
                                        "{} {}",
                                        if selected { "✓" } else { "○" },
                                        status.as_str()
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.apply_settings_patch(
                                            NativeSettingsPatch {
                                                notification_statuses: Some(next_statuses.clone()),
                                                ..NativeSettingsPatch::default()
                                            },
                                            cx,
                                        );
                                    })),
                            )
                        },
                    ))
                    .child(
                        Button::new("settings-background")
                            .label(format!("Background sync: {background_enabled}"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.apply_settings_patch(
                                    NativeSettingsPatch {
                                        background_sync_enabled: Some(!background_enabled),
                                        ..NativeSettingsPatch::default()
                                    },
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("settings-background-interval")
                            .label(format!("Background interval: {interval_minutes} minutes"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.apply_settings_patch(
                                    NativeSettingsPatch {
                                        background_sync_interval_minutes: Some(next_interval),
                                        ..NativeSettingsPatch::default()
                                    },
                                    cx,
                                );
                            })),
                    )
                    .child(setting_row("Background sync status", background_status))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(setting_row("Release update", release_update_status))
                            .child(
                                Button::new("settings-release-update-check")
                                    .label("Check now")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.check_release_update(cx);
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            Err(error) => {
                render_error_state("Settings unavailable", error.clone()).into_any_element()
            }
        };

        route_panel(self.message("settings.title", "Settings"), content).into_any_element()
    }

    fn open_pane_command_editor(
        &mut self,
        layout_id: String,
        position: u32,
        command: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editing_pane_command = Some((layout_id, position));
        self.mutation_error = None;
        self.pane_command_input.update(cx, |input, cx| {
            input.set_value(command, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn close_pane_command_editor(&mut self, cx: &mut Context<Self>) {
        self.editing_pane_command = None;
        self.mutation_error = None;
        self.request_terminal_focus_restore();
        cx.notify();
    }

    fn submit_pane_command_editor(&mut self, cx: &mut Context<Self>) {
        let Some((layout_id, position)) = self.editing_pane_command.clone() else {
            return;
        };
        let command = self.pane_command_input.read(cx).value().to_string();
        let result = self
            .config
            .ensure_database_file()
            .map_err(|error| error.to_string())
            .and_then(|path| update_native_pane_command(path, &layout_id, position, &command));
        match result {
            Ok(layouts) => {
                self.pane_layouts = Ok(layouts);
                self.editing_pane_command = None;
                self.mutation_error = None;
            }
            Err(error) => self.mutation_error = Some(error),
        }
        cx.notify();
    }

    fn mutate_pane_layout(&mut self, layout_id: String, reset: bool, cx: &mut Context<Self>) {
        let result = self
            .config
            .ensure_database_file()
            .map_err(|error| error.to_string())
            .and_then(|path| {
                if reset {
                    reset_native_project_pane_layout(path, &layout_id)
                } else {
                    cycle_native_pane_layout(path, &layout_id)
                }
            });
        match result {
            Ok(layouts) => {
                self.pane_layouts = Ok(layouts);
                self.mutation_error = None;
            }
            Err(error) => self.mutation_error = Some(error),
        }
        cx.notify();
    }

    fn select_pane_layout_type(
        &mut self,
        layout_id: Option<String>,
        project_id: Option<String>,
        is_global: bool,
        layout_type: PaneLayoutType,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .config
            .ensure_database_file()
            .map_err(|error| error.to_string())
            .and_then(|path| {
                save_native_pane_layout_type(
                    path,
                    layout_id.as_deref(),
                    project_id.as_deref(),
                    is_global,
                    layout_type,
                )
            });
        match result {
            Ok(layouts) => {
                self.pane_layouts = Ok(layouts);
                self.mutation_error = None;
            }
            Err(error) => self.mutation_error = Some(error),
        }
        cx.notify();
    }

    fn render_pane_command_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some((_, position)) = &self.editing_pane_command else {
            return div().into_any_element();
        };
        let error = self
            .mutation_error
            .as_ref()
            .map(|error| {
                render_error_state("Pane command was not saved", error.clone()).into_any_element()
            })
            .unwrap_or_else(|| div().into_any_element());
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x090b10))
            .child(
                div()
                    .w(px(520.0))
                    .tab_group()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0x334155))
                    .bg(rgb(0x171b23))
                    .p_5()
                    .child(format!("Pane {} command", position + 1))
                    .child(Input::new(&self.pane_command_input))
                    .child(error)
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(Button::new("pane-command-cancel").label("Cancel").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.close_pane_command_editor(cx);
                                }),
                            ))
                            .child(
                                Button::new("pane-command-save")
                                    .primary()
                                    .label("Save")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_pane_command_editor(cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_pane_layouts(&self, cx: &mut Context<Self>) -> AnyElement {
        let content = match &self.pane_layouts {
            Ok(layouts) => {
                let error = self
                    .mutation_error
                    .as_ref()
                    .map(|error| {
                        render_error_state("Pane layout was not saved", error.clone())
                            .into_any_element()
                    })
                    .unwrap_or_else(|| div().into_any_element());
                let configured_projects = layouts
                    .iter()
                    .filter_map(|layout| layout.project_id.clone())
                    .collect::<BTreeSet<_>>();
                let mut list = layouts.iter().enumerate().fold(
                    div().flex().flex_col().gap_3().child(error),
                    |list, (index, layout)| {
                        let type_buttons = PANE_LAYOUT_TYPES.iter().enumerate().fold(
                            div().flex().gap_2().flex_wrap(),
                            |buttons, (type_index, (layout_type, label))| {
                                let layout_id = layout.id.clone();
                                let project_id = layout.project_id.clone();
                                let is_global = layout.is_global;
                                let selected = layout.layout_type == *layout_type;
                                let layout_type = *layout_type;
                                buttons.child(
                                    Button::new((
                                        "pane-layout-type",
                                        index * PANE_LAYOUT_TYPES.len() + type_index,
                                    ))
                                    .selected(selected)
                                    .label(format!("{} {label}", if selected { "✓" } else { "○" }))
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.select_pane_layout_type(
                                                Some(layout_id.clone()),
                                                project_id.clone(),
                                                is_global,
                                                layout_type,
                                                cx,
                                            );
                                        },
                                    )),
                                )
                            },
                        );
                        let pane_commands = layout.panes.iter().fold(
                            div().flex().flex_col().gap_1(),
                            |rows, pane| {
                                let layout_id = layout.id.clone();
                                let position = pane.position;
                                let command = pane.command.clone();
                                rows.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(format!(
                                            "{} · {}",
                                            pane_position_label(layout.layout_type, position),
                                            if pane.command.is_empty() {
                                                "(default shell)"
                                            } else {
                                                pane.command.as_str()
                                            }
                                        ))
                                        .child(
                                            Button::new((
                                                "pane-command-edit",
                                                index * 10 + position as usize,
                                            ))
                                            .label("Edit command")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.open_pane_command_editor(
                                                    layout_id.clone(),
                                                    position,
                                                    command.clone(),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                        ),
                                )
                            },
                        );
                        let reset = if layout.is_global {
                            div().into_any_element()
                        } else {
                            let reset_id = layout.id.clone();
                            Button::new(("pane-reset", index))
                                .label("Reset to global")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.mutate_pane_layout(reset_id.clone(), true, cx);
                                }))
                                .into_any_element()
                        };
                        let target = layout
                            .project_id
                            .as_deref()
                            .and_then(|project_id| {
                                self.spec
                                    .projects
                                    .iter()
                                    .find(|project| project.id == project_id)
                            })
                            .map(|project| project.name.clone())
                            .unwrap_or_else(|| "Global default".to_owned());
                        list.child(
                            div()
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(0x293241))
                                .p_3()
                                .child(target)
                                .child(type_buttons)
                                .child(pane_commands)
                                .child(reset),
                        )
                    },
                );
                if !layouts.iter().any(|layout| layout.is_global) {
                    let buttons = PANE_LAYOUT_TYPES.iter().enumerate().fold(
                        div().flex().gap_2().flex_wrap(),
                        |buttons, (index, (layout_type, label))| {
                            let layout_type = *layout_type;
                            buttons.child(
                                Button::new(("pane-global-create", index))
                                    .label(*label)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.select_pane_layout_type(
                                            None,
                                            None,
                                            true,
                                            layout_type,
                                            cx,
                                        );
                                    })),
                            )
                        },
                    );
                    list = list.child(
                        div()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(0x293241))
                            .p_3()
                            .child("Global default · Not configured")
                            .child(buttons),
                    );
                }
                for (project_index, project) in self
                    .spec
                    .projects
                    .iter()
                    .filter(|project| {
                        !project.is_worktree && !configured_projects.contains(&project.id)
                    })
                    .enumerate()
                {
                    let buttons = PANE_LAYOUT_TYPES.iter().enumerate().fold(
                        div().flex().gap_2().flex_wrap(),
                        |buttons, (type_index, (layout_type, label))| {
                            let project_id = project.id.clone();
                            let layout_type = *layout_type;
                            buttons.child(
                                Button::new((
                                    "pane-project-create",
                                    project_index * PANE_LAYOUT_TYPES.len() + type_index,
                                ))
                                .label(*label)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.select_pane_layout_type(
                                            None,
                                            Some(project_id.clone()),
                                            false,
                                            layout_type,
                                            cx,
                                        );
                                    },
                                )),
                            )
                        },
                    );
                    list = list.child(
                        div()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(0x293241))
                            .p_3()
                            .child(format!("{} · Uses global default", project.name))
                            .child(buttons),
                    );
                }
                list.into_any_element()
            }
            Err(error) => {
                render_error_state("Pane layouts unavailable", error.clone()).into_any_element()
            }
        };

        route_panel(self.message("paneLayout.title", "Pane Layout"), content).into_any_element()
    }

    fn render_task_detail(&self, task_id: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(card) = self
            .spec
            .columns
            .iter()
            .flat_map(|column| column.cards.iter())
            .find(|card| card.id == task_id)
        else {
            return render_empty_state(
                "Task not found",
                "The task may have been deleted or filtered from this database.",
            )
            .into_any_element();
        };
        let locale = self.navigation.current().locale();
        let diff_task_id = task_id.to_owned();
        let edit_task_id = task_id.to_owned();
        let edit_task_title = card.title.clone();
        let delete_task_id = task_id.to_owned();
        let confirm_delete = self.delete_confirmation_task_id.as_deref() == Some(task_id);
        let dock = task_detail_dock_items(card.pr_url.as_deref(), ShortcutPlatform::Mac)
            .into_iter()
            .fold(div().flex().gap_2().flex_wrap(), |items, item| {
                let item_id = item.id.to_owned();
                let selected = self
                    .selected_task_dock_item
                    .as_deref()
                    .unwrap_or("overview")
                    == item.id;
                items.child(
                    Button::new(item.id)
                        .selected(selected)
                        .label(format!("{}  {}", item.label, item.shortcut_label))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_task_dock_item(&item_id, cx);
                        })),
                )
            });
        let status_task_id = task_id.to_owned();
        let status_buttons =
            TaskStatus::ALL.into_iter().enumerate().fold(
                div().flex().gap_2().flex_wrap(),
                |items, (index, status)| {
                    let task_id = status_task_id.clone();
                    let label = if card.status == status.as_str() {
                        format!("✓ {}", status.as_str())
                    } else {
                        status.as_str().to_owned()
                    };
                    items.child(Button::new(("task-status", index)).label(label).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.change_task_status(task_id.clone(), status, cx);
                        }),
                    ))
                },
            );
        let delete_confirmation =
            if confirm_delete {
                let error = self
                    .mutation_error
                    .as_ref()
                    .map(|error| {
                        render_error_state("Task was not deleted", error.clone()).into_any_element()
                    })
                    .unwrap_or_else(|| div().into_any_element());
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(0xff7d73))
                    .p_3()
                    .child("Delete this task? This action cannot be undone.")
                    .child(error)
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("delete-task-confirm")
                                    .danger()
                                    .label("Delete")
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.confirm_task_delete(cx)),
                                    ),
                            )
                            .child(Button::new("delete-task-cancel").label("Cancel").on_click(
                                cx.listener(|this, _, _, cx| this.cancel_task_delete(cx)),
                            )),
                    )
                    .into_any_element()
            } else {
                div().into_any_element()
            };
        let operation_error = if confirm_delete {
            div().into_any_element()
        } else {
            self.mutation_error
                .as_ref()
                .map(|error| {
                    render_error_state("Task was not updated", error.clone()).into_any_element()
                })
                .unwrap_or_else(|| div().into_any_element())
        };
        let project_color_editor = card
            .project_id
            .as_ref()
            .map(|project_id| {
                let updating = self.project_color_updating.contains(project_id);
                let current_color = card.project_color.as_deref().unwrap_or_default();
                let colors = PROJECT_COLOR_PRESETS.into_iter().enumerate().fold(
                    div().flex().gap_2().flex_wrap(),
                    |colors, (index, color)| {
                        let project_id = project_id.clone();
                        let selected = current_color.eq_ignore_ascii_case(color);
                        colors.child(
                            Button::new(("task-project-color", index))
                                .selected(selected)
                                .disabled(updating)
                                .label(format!("{} {color}", if selected { "✓" } else { "○" }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_project_color(
                                        project_id.clone(),
                                        color.to_owned(),
                                        cx,
                                    );
                                })),
                        )
                    },
                );
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(0x293241))
                    .p_3()
                    .child(if updating {
                        "Project color · Saving…"
                    } else {
                        "Project color"
                    })
                    .child(colors)
                    .into_any_element()
            })
            .unwrap_or_else(|| div().into_any_element());
        let terminal_panel = if let Some(terminal) = self.terminals.get(task_id) {
            let stop_task_id = task_id.to_owned();
            let restart_task_id = task_id.to_owned();
            div()
                .flex()
                .flex_col()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(rgb(0x293241))
                .bg(rgb(0x0b0e14))
                .p_2()
                .child(div().h(px(420.0)).child(terminal.clone()))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("terminal-restart")
                                .label("Restart Terminal")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.restart_task_terminal(restart_task_id.clone(), window, cx);
                                })),
                        )
                        .child(
                            Button::new("terminal-stop")
                                .label("Stop Terminal")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.stop_task_terminal(&stop_task_id, cx);
                                })),
                        ),
                )
                .into_any_element()
        } else {
            let start_task_id = task_id.to_owned();
            let error = self
                .terminal_errors
                .get(task_id)
                .map(|error| {
                    render_error_state("Terminal could not start", error.clone()).into_any_element()
                })
                .unwrap_or_else(|| div().into_any_element());
            let dependency_session_type = self
                .session_dependency_statuses
                .get(task_id)
                .and_then(|status| status.as_ref().ok())
                .map(|status| status.session_type)
                .or_else(|| {
                    card.session_type
                        .as_deref()
                        .and_then(|value| SessionType::parse(value).ok())
                });
            let dependency_panel = dependency_session_type
                .map(|session_type| {
                    let loading = self.session_dependency_loading.contains(task_id);
                    let status = self.session_dependency_statuses.get(task_id);
                    let label = if loading {
                        "Checking session dependency…".to_owned()
                    } else {
                        match status {
                            Some(Ok(status)) if status.available => {
                                format!("{} · {}: Available", status.tool_name, status.target)
                            }
                            Some(Ok(status)) if status.blocked_reason.is_some() => format!(
                                "{} · {}: Install blocked until restart: {}",
                                status.tool_name,
                                status.target,
                                status.blocked_reason.as_deref().unwrap_or_default()
                            ),
                            Some(Ok(status)) => {
                                format!("{} · {}: Not installed", status.tool_name, status.target)
                            }
                            Some(Err(error)) => format!("Dependency check failed: {error}"),
                            None => "Session dependency not checked".to_owned(),
                        }
                    };
                    let check_task_id = task_id.to_owned();
                    let install_task_id = task_id.to_owned();
                    let can_install = !loading
                        && matches!(status, Some(Ok(status)) if !status.available && status.blocked_reason.is_none());
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(label)
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    Button::new("session-dependency-check")
                                        .label(if status.is_some() { "Retry check" } else { "Check" })
                                        .disabled(loading)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.run_session_dependency_operation(
                                                check_task_id.clone(),
                                                session_type,
                                                false,
                                                cx,
                                            );
                                        })),
                                )
                                .when(can_install, |actions| {
                                    actions.child(
                                        Button::new("session-dependency-install")
                                            .primary()
                                            .label("Install")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.run_session_dependency_operation(
                                                    install_task_id.clone(),
                                                    session_type,
                                                    true,
                                                    cx,
                                                );
                                            })),
                                    )
                                }),
                        )
                        .into_any_element()
                })
                .unwrap_or_else(|| div().into_any_element());
            let action = if card.session_type.is_none() && card.project_id.is_some() {
                let tmux_task_id = task_id.to_owned();
                let zellij_task_id = task_id.to_owned();
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("terminal-connect-tmux")
                            .primary()
                            .label("Connect with tmux")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.connect_task_terminal(
                                    tmux_task_id.clone(),
                                    SessionType::Tmux,
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("terminal-connect-zellij")
                            .label("Connect with zellij")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.connect_task_terminal(
                                    zellij_task_id.clone(),
                                    SessionType::Zellij,
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .into_any_element()
            } else if card.session_type.is_none() {
                render_empty_state(
                    "No terminal",
                    "Assign this task to a project before connecting a persistent terminal.",
                )
                .into_any_element()
            } else {
                div()
                    .child(
                        Button::new("terminal-start")
                            .primary()
                            .label("Start Terminal")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.start_task_terminal(start_task_id.clone(), window, cx);
                            })),
                    )
                    .into_any_element()
            };
            div()
                .flex()
                .flex_col()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(rgb(0x293241))
                .p_3()
                .child("Terminal")
                .child(error)
                .child(dependency_panel)
                .child(action)
                .into_any_element()
        };
        let ai_sessions_panel = if self.ai_sessions_loading.contains(task_id) {
            div()
                .rounded_md()
                .border_1()
                .border_color(rgb(0x293241))
                .p_3()
                .child("Loading AI sessions…")
                .into_any_element()
        } else {
            match self.ai_sessions.get(task_id) {
                Some(Ok(page)) => {
                    let search_task_id = task_id.to_owned();
                    let search = div()
                        .flex()
                        .gap_2()
                        .child(Input::new(&self.ai_search_input).w_full())
                        .child(Button::new("ai-session-search").label("Search").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.search_task_ai_sessions(search_task_id.clone(), cx);
                            }),
                        ));
                    let provider_filters = AiSessionProvider::ALL.into_iter().enumerate().fold(
                        div().flex().gap_2().flex_wrap(),
                        |filters, (index, provider)| {
                            let selected = self.ai_provider_filters.contains(&provider);
                            filters.child(
                                Button::new(("ai-provider-filter", index))
                                    .label(format!(
                                        "{} {}",
                                        if selected { "✓" } else { "○" },
                                        provider.as_str()
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.toggle_ai_provider_filter(provider, cx);
                                    })),
                            )
                        },
                    );
                    let role_filters = [
                        AiMessageRole::User,
                        AiMessageRole::Assistant,
                        AiMessageRole::Tool,
                        AiMessageRole::System,
                        AiMessageRole::Developer,
                        AiMessageRole::Reasoning,
                    ]
                    .into_iter()
                    .enumerate()
                    .fold(
                        div().flex().gap_2().flex_wrap(),
                        |filters, (index, role)| {
                            let selected = self.ai_role_filters.contains(&role);
                            filters.child(
                                Button::new(("ai-role-filter", index))
                                    .label(format!(
                                        "{} {:?}",
                                        if selected { "✓" } else { "○" },
                                        role
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.toggle_ai_role_filter(role, cx);
                                    })),
                            )
                        },
                    );
                    let sources = page.sources.iter().fold(
                        div().flex().gap_2().flex_wrap(),
                        |sources, source| {
                            sources.child(div().text_sm().child(format!(
                                    "{}: {}{}",
                                    source.provider.as_str(),
                                    source.session_count,
                                    source
                                        .reason
                                        .as_deref()
                                        .map(|reason| format!(" ({reason})"))
                                        .unwrap_or_default()
                                )))
                        },
                    );
                    let sessions = page
                        .sessions
                        .iter()
                        .filter(|session| self.ai_provider_filters.contains(&session.provider))
                        .enumerate()
                        .fold(
                            div().flex().flex_col().gap_2(),
                            |sessions, (index, session)| {
                                let task_id = task_id.to_owned();
                                let provider = session.provider;
                                let session_id = session.id.clone();
                                let source_ref = session.source_ref.clone();
                                let label = format!(
                                    "{} · {} · {} messages",
                                    session.provider.as_str(),
                                    session.title.as_deref().unwrap_or(&session.id),
                                    session.message_count
                                );
                                sessions.child(
                                    Button::new(("ai-session", index)).label(label).on_click(
                                        cx.listener(move |this, _, _, cx| {
                                            this.load_task_ai_session_detail(
                                                task_id.clone(),
                                                provider,
                                                session_id.clone(),
                                                source_ref.clone(),
                                                cx,
                                            );
                                        }),
                                    ),
                                )
                            },
                        );
                    let load_more = if page.next_cursor.is_some() {
                        let task_id = task_id.to_owned();
                        Button::new("ai-session-load-more")
                            .label("Load more")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.load_more_task_ai_sessions(task_id.clone(), cx);
                            }))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    };
                    let details =
                        self.ai_session_details
                            .iter()
                            .filter(|((detail_task_id, _), _)| detail_task_id == task_id)
                            .fold(
                                div().flex().flex_col().gap_2(),
                                |details, ((_, _), detail)| match detail {
                                    Ok(detail) => {
                                        details.child(
                                            div()
                                                .rounded_md()
                                                .border_1()
                                                .border_color(rgb(0x374151))
                                                .p_3()
                                                .child(div().text_sm().child(
                                                    detail.title.clone().unwrap_or_else(|| {
                                                        detail.session_id.clone()
                                                    }),
                                                ))
                                                .children(detail.messages.iter().map(|message| {
                                                    div().text_sm().child(format!(
                                                        "{:?}: {}",
                                                        message.role, message.text
                                                    ))
                                                })),
                                        )
                                    }
                                    Err(error) => details
                                        .child(render_error_state("AI session unavailable", error)),
                                },
                            );
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(0x293241))
                        .p_3()
                        .child(div().text_lg().child("AI Sessions"))
                        .child(search)
                        .child(provider_filters)
                        .child(role_filters)
                        .child(sources)
                        .child(sessions)
                        .child(load_more)
                        .child(details)
                        .into_any_element()
                }
                Some(Err(error)) => {
                    render_error_state("AI sessions unavailable", error).into_any_element()
                }
                None => div()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(0x293241))
                    .p_3()
                    .child("AI sessions have not been loaded.")
                    .into_any_element(),
            }
        };
        let hook_panel = if card.project_id.is_some() || card.worktree_path.is_some() {
            let loading = self.task_hook_loading.contains(task_id);
            let statuses = self.task_hook_statuses.get(task_id);
            let status_rows = match statuses {
                Some(Ok(statuses)) => statuses
                    .iter()
                    .fold(div().flex().flex_col().gap_1(), |rows, status| {
                        let state = if !status.installed {
                            "Not installed"
                        } else if !status.has_expected_hook_server_url {
                            "Callback URL mismatch"
                        } else if !status.has_reachable_hook_server {
                            "Callback unreachable"
                        } else {
                            "Ready"
                        };
                        rows.child(format!("{} · {state}", status.provider.as_str()))
                    })
                    .into_any_element(),
                Some(Err(error)) => {
                    render_error_state("Provider hooks unavailable", error).into_any_element()
                }
                None => div()
                    .text_sm()
                    .text_color(rgb(0xaeb3bd))
                    .child("Provider hooks have not been checked.")
                    .into_any_element(),
            };
            let check_task_id = task_id.to_owned();
            let install_task_id = task_id.to_owned();
            let all_ready = matches!(
                statuses,
                Some(Ok(statuses))
                    if !statuses.is_empty()
                        && statuses.iter().all(|status| {
                            status.installed
                                && status.has_expected_hook_server_url
                                && status.has_reachable_hook_server
                        })
            );
            div()
                .flex()
                .flex_col()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(rgb(0x293241))
                .p_3()
                .child("AI provider hooks")
                .child(status_rows)
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("task-hooks-check")
                                .label(if statuses.is_some() {
                                    "Recheck hooks"
                                } else {
                                    "Check hooks"
                                })
                                .disabled(loading)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.run_task_hook_operation(check_task_id.clone(), false, cx);
                                })),
                        )
                        .when(!all_ready, |actions| {
                            actions.child(
                                Button::new("task-hooks-install")
                                    .primary()
                                    .label(if loading {
                                        "Installing…"
                                    } else {
                                        "Install / repair all"
                                    })
                                    .disabled(loading)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.run_task_hook_operation(
                                            install_task_id.clone(),
                                            true,
                                            cx,
                                        );
                                    })),
                            )
                        }),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

        let main = div()
            .flex_1()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_xl().child(card.title.clone()))
            .child(dock)
            .child(terminal_panel)
            .child(ai_sessions_panel)
            .child(hook_panel)
            .child(project_color_editor)
            .child(status_buttons)
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("edit-task-title")
                            .label("Edit")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_task_title_editor(
                                    edit_task_id.clone(),
                                    edit_task_title.clone(),
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("delete-task")
                            .danger()
                            .label("Delete")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.request_task_delete(delete_task_id.clone(), cx);
                            })),
                    ),
            )
            .child(operation_error)
            .child(delete_confirmation)
            .child(
                Button::new("task-diff")
                    .primary()
                    .label("Open Diff")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.navigate(
                            NativeRoute::Diff {
                                locale,
                                task_id: diff_task_id.clone(),
                            },
                            cx,
                        );
                    })),
            );
        let show_sidebar_hint = self
            .settings
            .as_ref()
            .is_ok_and(|settings| !settings.sidebar_hint_dismissed);
        let sidebar = if self.task_sidebar_collapsed {
            let hint = if show_sidebar_hint {
                div()
                    .rounded_md()
                    .bg(rgb(0x333333))
                    .p_2()
                    .text_sm()
                    .child("Open the task sidebar for project and session context.")
                    .child(
                        Button::new("task-sidebar-hint-dismiss")
                            .label("Don't show again")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dismiss_task_sidebar_hint(cx);
                            })),
                    )
                    .into_any_element()
            } else {
                div().into_any_element()
            };
            div()
                .w(px(220.0))
                .flex()
                .flex_col()
                .gap_2()
                .child(hint)
                .child(
                    Button::new("task-sidebar-expand")
                        .label("Open sidebar")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_task_sidebar(cx);
                        })),
                )
                .into_any_element()
        } else {
            let project = card
                .project_id
                .as_deref()
                .and_then(|project_id| {
                    self.spec
                        .projects
                        .iter()
                        .find(|project| project.id == project_id)
                })
                .map(|project| project.name.as_str())
                .unwrap_or("Unassigned");
            div()
                .w(px(280.0))
                .flex()
                .flex_col()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(rgb(0x293241))
                .p_3()
                .child("Task context")
                .child(format!("Project · {project}"))
                .child(format!(
                    "Branch · {}",
                    card.branch_name.as_deref().unwrap_or("None")
                ))
                .child(format!(
                    "Session · {}",
                    card.session_type.as_deref().unwrap_or("None")
                ))
                .child(format!(
                    "SSH · {}",
                    card.ssh_host.as_deref().unwrap_or("Local")
                ))
                .child(
                    Button::new("task-sidebar-collapse")
                        .label("Collapse sidebar")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_task_sidebar(cx);
                        })),
                )
                .into_any_element()
        };

        route_panel(
            "Task Detail",
            div().flex().gap_4().child(sidebar).child(main),
        )
        .into_any_element()
    }

    fn toggle_task_sidebar(&mut self, cx: &mut Context<Self>) {
        self.task_sidebar_collapsed = !self.task_sidebar_collapsed;
        cx.notify();
    }

    fn dismiss_task_sidebar_hint(&mut self, cx: &mut Context<Self>) {
        let result = self
            .config
            .ensure_database_file()
            .map_err(|error| error.to_string())
            .and_then(dismiss_native_sidebar_hint);
        match result {
            Ok(()) => {
                if let Ok(settings) = &mut self.settings {
                    settings.sidebar_hint_dismissed = true;
                }
                self.mutation_error = None;
            }
            Err(error) => self.mutation_error = Some(error),
        }
        cx.notify();
    }

    fn open_diff_editor(
        &mut self,
        task_id: String,
        path: String,
        current: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editing_diff_file = Some((task_id, path, current.clone()));
        self.mutation_error = None;
        self.diff_editor_input.update(cx, |input, cx| {
            input.set_value(current, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn close_diff_editor(&mut self, cx: &mut Context<Self>) {
        if self.diff_editor_saving {
            return;
        }
        self.editing_diff_file = None;
        self.mutation_error = None;
        self.request_terminal_focus_restore();
        cx.notify();
    }

    fn submit_diff_editor(&mut self, cx: &mut Context<Self>) {
        if self.diff_editor_saving {
            return;
        }
        let Some((task_id, path, expected_current)) = self.editing_diff_file.clone() else {
            return;
        };
        let content = self.diff_editor_input.read(cx).value().to_string();
        if content == expected_current {
            return;
        }
        let config = self.config.clone();
        let worker_task_id = task_id.clone();
        let (sender, receiver) = channel();
        self.diff_editor_saving = true;
        self.mutation_error = None;
        std::thread::spawn(move || {
            let result = config
                .ensure_database_file()
                .map_err(|error| error.to_string())
                .and_then(|database_path| {
                    save_native_diff_file(
                        database_path,
                        &worker_task_id,
                        &path,
                        &expected_current,
                        &content,
                    )
                });
            let _ = sender.send(result);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => {
                        Err("Diff save worker stopped unexpectedly.".to_owned())
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.diff_editor_saving = false;
                    match result {
                        Ok(()) => {
                            this.editing_diff_file = None;
                            this.mutation_error = None;
                            this.diff_snapshots.remove(&task_id);
                            this.load_diff_snapshot(task_id.clone(), true, cx);
                        }
                        Err(error) => {
                            this.mutation_error = Some(error);
                            cx.notify();
                        }
                    }
                });
                break;
            }
        })
        .detach();
        cx.notify();
    }

    fn render_diff_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some((_, path, _)) = &self.editing_diff_file else {
            return div().into_any_element();
        };
        let error = self
            .mutation_error
            .as_ref()
            .map(|error| render_error_state("File was not saved", error.clone()).into_any_element())
            .unwrap_or_else(|| div().into_any_element());
        let is_dirty = self.diff_editor_input.read(cx).value().as_ref()
            != self
                .editing_diff_file
                .as_ref()
                .map(|(_, _, expected)| expected.as_str())
                .unwrap_or_default();
        let save_label = if self.diff_editor_saving {
            "Saving…"
        } else {
            "Save"
        };
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x090b10))
            .child(
                div()
                    .w(px(820.0))
                    .tab_group()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0x334155))
                    .bg(rgb(0x171b23))
                    .p_5()
                    .child(format!("Edit {path}"))
                    .child(Input::new(&self.diff_editor_input))
                    .child(if is_dirty {
                        "Unsaved changes"
                    } else {
                        "No changes"
                    })
                    .child("Cmd+S saves only if the file has not changed on disk.")
                    .child(error)
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("diff-editor-cancel")
                                    .disabled(self.diff_editor_saving)
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_diff_editor(cx);
                                    })),
                            )
                            .child(
                                Button::new("diff-editor-save")
                                    .primary()
                                    .disabled(self.diff_editor_saving || !is_dirty)
                                    .label(save_label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_diff_editor(cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn dismiss_background_sync_review(&mut self, cx: &mut Context<Self>) {
        self.background_sync.acknowledge_review();
        self.background_review_open = false;
        self.selected_merged_task_ids.clear();
        self.request_terminal_focus_restore();
        cx.notify();
    }

    fn confirm_background_sync_review(&mut self, cx: &mut Context<Self>) {
        let task_ids = self
            .selected_merged_task_ids
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let database_path = match self.config.ensure_database_file() {
            Ok(path) => path,
            Err(error) => {
                self.mutation_error = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        let mut errors = Vec::new();
        for task_id in task_ids {
            if let Some(controller) = self.terminal_controllers.remove(&task_id) {
                let _ = controller.terminate();
            }
            self.terminals.remove(&task_id);
            if let Err(error) = delete_native_task(&database_path, &task_id) {
                errors.push(format!("{task_id}: {error}"));
            }
        }
        if errors.is_empty() {
            self.mutation_error = None;
            self.background_sync.acknowledge_review();
            self.background_review_open = false;
            self.selected_merged_task_ids.clear();
            self.retry_startup(cx);
        } else {
            self.mutation_error = Some(errors.join("; "));
            cx.notify();
        }
    }

    fn render_background_sync_review(&self, cx: &mut Context<Self>) -> AnyElement {
        let review = self.background_sync.snapshot().pending_review;
        if !self.background_review_open || !review.needs_review() {
            return div().into_any_element();
        }
        let merged = review.merged_pull_requests.iter().enumerate().fold(
            div().flex().flex_col().gap_2(),
            |rows, (index, pull_request)| {
                let task_id = pull_request.task_id.clone();
                let selected = self.selected_merged_task_ids.contains(&task_id);
                rows.child(
                    div()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(0x334155))
                        .p_3()
                        .child(
                            Button::new(("background-review-merged", index))
                                .label(format!(
                                    "{} {} · {}",
                                    if selected { "☑" } else { "☐" },
                                    pull_request.task_title,
                                    pull_request.branch_name
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !this.selected_merged_task_ids.remove(&task_id) {
                                        this.selected_merged_task_ids.insert(task_id.clone());
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(pull_request.pr_url.clone())
                        .child(format!("Merged at {}", pull_request.merged_at)),
                )
            },
        );
        let pulled = review.pulled_tasks.iter().enumerate().fold(
            div().flex().flex_col().gap_2(),
            |rows, (index, pull)| {
                rows.child(
                    div()
                        .id(("background-review-pull", index))
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(0x334155))
                        .p_3()
                        .child(format!(
                            "{} · {} · {}",
                            pull.task_title,
                            pull.branch_name,
                            match pull.status {
                                crate::NativeTaskPullStatus::Updated => "Updated",
                                crate::NativeTaskPullStatus::Failed => "Failed",
                            }
                        ))
                        .child(pull.summary.clone()),
                )
            },
        );
        let failures = review.errors.iter().enumerate().fold(
            div().flex().flex_col().gap_2(),
            |rows, (index, error)| {
                rows.child(
                    div()
                        .id(("background-review-error", index))
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(0xef4444))
                        .p_3()
                        .child(error.clone()),
                )
            },
        );
        let mutation_error = self
            .mutation_error
            .as_ref()
            .map(|error| {
                render_error_state("Merged tasks were not deleted", error.clone())
                    .into_any_element()
            })
            .unwrap_or_else(|| div().into_any_element());
        let confirm_label = if self.selected_merged_task_ids.is_empty() {
            "Confirm"
        } else {
            "Delete selected merged tasks"
        };

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x090b10))
            .child(
                div()
                    .w(px(720.0))
                    .tab_group()
                    .max_h(px(680.0))
                    .overflow_y_scrollbar()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0x334155))
                    .bg(rgb(0x171b23))
                    .p_5()
                    .child(div().text_lg().child("Background sync review"))
                    .child(format!(
                        "{} merged PRs · {} new worktrees · {} pull events · {} failures",
                        review.merged_pull_requests.len(),
                        review.registered_worktrees,
                        review.pulled_tasks.len(),
                        review.errors.len()
                    ))
                    .child(merged)
                    .child(pulled)
                    .child(failures)
                    .child(mutation_error)
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("background-review-cancel")
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.dismiss_background_sync_review(cx);
                                    })),
                            )
                            .child(
                                Button::new("background-review-confirm")
                                    .primary()
                                    .label(confirm_label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_background_sync_review(cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_diff_loading(&self, task_id: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.navigation.current().locale();
        let detail_task_id = task_id.to_owned();
        let content = match self.diff_snapshots.get(task_id) {
            None => render_empty_state(
                "Loading repository changes…",
                "Git and remote SSH work run outside the UI thread.",
            )
            .into_any_element(),
            Some(Ok(snapshot)) if snapshot.files.is_empty() => {
                render_empty_state("No repository changes", "This branch has no changed files.")
                    .into_any_element()
            }
            Some(Ok(snapshot)) => {
                let selected_path = self
                    .selected_diff_file_path
                    .as_deref()
                    .filter(|path| snapshot.files.iter().any(|file| file.path == *path))
                    .or_else(|| snapshot.files.first().map(|file| file.path.as_str()));
                let selected = selected_path
                    .and_then(|path| snapshot.files.iter().find(|file| file.path == path))
                    .expect("non-empty diff snapshot has a selected file");
                let mut grouped_files = BTreeMap::<String, Vec<(usize, _)>>::new();
                for (index, file) in snapshot.files.iter().enumerate() {
                    let folder = Path::new(&file.path)
                        .parent()
                        .and_then(Path::to_str)
                        .filter(|folder| !folder.is_empty())
                        .unwrap_or("(root)")
                        .to_owned();
                    grouped_files.entry(folder).or_default().push((index, file));
                }
                let mut files = div()
                    .w(px(self.diff_sidebar_width))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .overflow_y_scrollbar();
                for (folder, folder_files) in grouped_files {
                    files = files.child(
                        div()
                            .mt_2()
                            .text_sm()
                            .text_color(rgb(0xaab4c3))
                            .child(folder),
                    );
                    for (index, file) in folder_files {
                        let path = file.path.clone();
                        let selection_task_id = task_id.to_owned();
                        let viewed = self
                            .viewed_diff_files
                            .contains(&(task_id.to_owned(), file.path.clone()));
                        files = files.child(
                            Button::new(("diff-file", index))
                                .label(format!(
                                    "{}{}  {}  {}",
                                    if viewed { "✓ " } else { "" },
                                    file.status,
                                    Path::new(&file.path)
                                        .file_name()
                                        .and_then(|name| name.to_str())
                                        .unwrap_or(&file.path),
                                    if file.is_binary {
                                        "binary".to_owned()
                                    } else {
                                        format!("+{} -{}", file.additions, file.deletions)
                                    }
                                ))
                                .selected(selected.path == file.path)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.select_diff_file(
                                        selection_task_id.clone(),
                                        path.clone(),
                                        cx,
                                    );
                                })),
                        );
                    }
                }
                let binary_notice = "Binary file preview and editing are disabled.";
                let original = div()
                    .flex_1()
                    .min_w_0()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(0x293241))
                    .bg(rgb(0x0b0e14))
                    .p_3()
                    .child(div().mb_2().text_color(rgb(0xaab4c3)).child("Original"))
                    .child(if selected.is_binary {
                        binary_notice.to_owned()
                    } else {
                        selected.original.clone()
                    });
                let edit_task_id = task_id.to_owned();
                let edit_path = selected.path.clone();
                let edit_current = selected.current.clone();
                let mut current = div()
                    .flex_1()
                    .min_w_0()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(0x293241))
                    .bg(rgb(0x0b0e14))
                    .p_3()
                    .child(div().mb_2().text_color(rgb(0xaab4c3)).child("Current"))
                    .child(if selected.is_binary {
                        binary_notice.to_owned()
                    } else {
                        selected.current.clone()
                    });
                if selected.status != "deleted" && !selected.is_binary {
                    current = current.child(
                        Button::new("diff-edit-current")
                            .primary()
                            .label("Edit current file")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_diff_editor(
                                    edit_task_id.clone(),
                                    edit_path.clone(),
                                    edit_current.clone(),
                                    window,
                                    cx,
                                );
                            })),
                    );
                }

                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(format!(
                        "{} → {} · {} files",
                        snapshot.base_branch,
                        snapshot.branch_name,
                        snapshot.files.len()
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("diff-sidebar-narrower")
                                    .disabled(self.diff_sidebar_width <= DIFF_SIDEBAR_MIN_WIDTH)
                                    .label("Narrower file tree")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.resize_diff_sidebar(-DIFF_SIDEBAR_RESIZE_STEP, cx);
                                    })),
                            )
                            .child(
                                Button::new("diff-sidebar-wider")
                                    .disabled(self.diff_sidebar_width >= DIFF_SIDEBAR_MAX_WIDTH)
                                    .label("Wider file tree")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.resize_diff_sidebar(DIFF_SIDEBAR_RESIZE_STEP, cx);
                                    })),
                            ),
                    )
                    .child(
                        div().flex().gap_3().child(files).child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .gap_2()
                                .child(original)
                                .child(current),
                        ),
                    )
                    .into_any_element()
            }
            Some(Err(error)) => {
                let retry_task_id = task_id.to_owned();
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(render_error_state("Diff unavailable", error.clone()))
                    .child(Button::new("diff-retry").primary().label("Retry").on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.load_diff_snapshot(retry_task_id.clone(), true, cx);
                        }),
                    ))
                    .into_any_element()
            }
        };
        route_panel(
            "Diff",
            div().flex().flex_col().gap_3().child(content).child(
                Button::new("diff-task-detail")
                    .label("Back to task")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.navigate(
                            NativeRoute::TaskDetail {
                                locale,
                                task_id: detail_task_id.clone(),
                            },
                            cx,
                        );
                    })),
            ),
        )
    }

    fn render_not_found(&self, path: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.navigation.current().locale();
        route_panel(
            "Not Found",
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(render_empty_state(
                    "Route not found",
                    format!("No native screen is registered for {path}."),
                ))
                .child(
                    Button::new("not-found-board")
                        .primary()
                        .label("Return to board")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.navigate(NativeRoute::Board { locale }, cx);
                        })),
                ),
        )
    }

    fn render_task_card(
        &self,
        card: &NativeUiCardSpec,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut badges = Vec::new();
        if let Some(project) = &card.project_name {
            badges.push((
                project.clone(),
                card.project_color
                    .as_deref()
                    .and_then(parse_hex_color)
                    .unwrap_or(PROJECT_COLOR_FALLBACK),
            ));
        }
        if let Some(priority) = card
            .priority
            .as_deref()
            .and_then(|priority| TaskPriority::parse(priority).ok())
        {
            badges.push((format_priority(priority), priority_tag_color(priority)));
        }
        if let Some(agent) = &card.agent_type {
            badges.push((agent.clone(), agent_tag_color(agent)));
        }
        if let Some(branch) = &card.branch_name {
            let branch_label = card
                .base_branch
                .as_ref()
                .map_or_else(|| branch.clone(), |base| format!("{branch} ← {base}"));
            badges.push((branch_label, TAG_BRANCH_TEXT));
        }
        if card.pr_url.is_some() {
            badges.push(("PR".to_owned(), PRIMARY));
        }
        if let Some(session) = &card.session_type {
            badges.push((session.clone(), TAG_SESSION_TEXT));
        }
        if let Some(host) = &card.ssh_host {
            badges.push((host.clone(), TAG_SSH_TEXT));
        }
        let metadata = badges.into_iter().fold(
            div().flex().flex_wrap().gap_1(),
            |container, (label, color)| container.child(render_card_badge(label, color)),
        );
        let task_id = card.id.clone();
        let locale = self.navigation.current().locale();
        let context_root = cx.entity();
        let context_task_id = card.id.clone();
        let context_task_title = card.title.clone();
        let context_status = card.status.clone();
        let context_can_create_branch = card.branch_name.is_none() && card.worktree_path.is_none();
        let drag_info = TaskDragInfo {
            task_id: card.id.clone(),
            title: card.title.clone(),
        };
        let drop_target_id = card.id.clone();
        let drop_target_status = TaskStatus::parse(&card.status).unwrap_or(TaskStatus::Todo);
        let is_focused = self.focused_task_id.as_deref() == Some(card.id.as_str());
        let is_find_match = !self.board_search_query.is_empty()
            && card
                .title
                .to_lowercase()
                .contains(&self.board_search_query.to_lowercase());

        div()
            .id(SharedString::from(card.id.clone()))
            .rounded_md()
            .border_1()
            .border_color(rgb(if is_focused {
                0x0064ff
            } else if is_find_match {
                0xf59e0b
            } else {
                0x293241
            }))
            .bg(rgb(0x111827))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .cursor_pointer()
            .hover(|card| card.border_color(rgb(0x0064ff)))
            .child(div().text_color(rgb(0xf8fafc)).child(card.title.clone()))
            .child(metadata)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.navigate(
                    NativeRoute::TaskDetail {
                        locale,
                        task_id: task_id.clone(),
                    },
                    cx,
                );
            }))
            .on_drag(drag_info, |drag: &TaskDragInfo, _, _, cx| {
                cx.new(|_| drag.clone())
            })
            .on_drop(cx.listener(move |this, drag: &TaskDragInfo, _, cx| {
                this.drop_task_before(drag.clone(), drop_target_status, drop_target_id.clone(), cx);
            }))
            .context_menu(move |menu, _, _| {
                let edit_root = context_root.clone();
                let edit_task_id = context_task_id.clone();
                let edit_task_title = context_task_title.clone();
                let mut menu = menu.item(PopupMenuItem::new("Edit task").on_click(
                    move |_, window, cx| {
                        edit_root.update(cx, |this, cx| {
                            this.open_task_title_editor(
                                edit_task_id.clone(),
                                edit_task_title.clone(),
                                window,
                                cx,
                            );
                        });
                    },
                ));
                if context_can_create_branch {
                    let branch_root = context_root.clone();
                    let branch_task_id = context_task_id.clone();
                    menu = menu.item(PopupMenuItem::new("Create branch").on_click(
                        move |_, window, cx| {
                            branch_root.update(cx, |this, cx| {
                                this.open_branch_task_editor(branch_task_id.clone(), window, cx);
                            });
                        },
                    ));
                }

                for status in TaskStatus::ALL {
                    let status_root = context_root.clone();
                    let status_task_id = context_task_id.clone();
                    menu = menu.item(
                        PopupMenuItem::new(format!("Move to {}", status.as_str()))
                            .checked(context_status == status.as_str())
                            .on_click(move |_, _, cx| {
                                status_root.update(cx, |this, cx| {
                                    this.change_task_status(status_task_id.clone(), status, cx);
                                });
                            }),
                    );
                }

                let delete_root = context_root.clone();
                let delete_task_id = context_task_id.clone();
                menu.item(PopupMenuItem::new("Delete task").on_click(move |_, _, cx| {
                    delete_root.update(cx, |this, cx| {
                        this.navigate(
                            NativeRoute::TaskDetail {
                                locale,
                                task_id: delete_task_id.clone(),
                            },
                            cx,
                        );
                        this.request_task_delete(delete_task_id.clone(), cx);
                    });
                }))
            })
    }
}

fn route_panel(title: impl IntoElement, content: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(div().text_xl().child(title))
        .child(content)
}

fn github_cli_target_key(ssh_host: Option<&str>) -> String {
    ssh_host
        .map(|host| format!("ssh:{host}"))
        .unwrap_or_else(|| "local".to_owned())
}

fn setting_row(label: &'static str, value: String) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .rounded_md()
        .border_1()
        .border_color(rgb(0x293241))
        .p_3()
        .child(label)
        .child(div().text_color(rgb(0xaeb3bd)).child(value))
}

fn render_empty_state(
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(0x293241))
        .p_4()
        .child(div().text_xl().child(title.into()))
        .child(div().text_color(rgb(0xaeb3bd)).child(message.into()))
}

fn render_error_state(
    title: impl Into<SharedString>,
    error: impl Into<SharedString>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(0xff7d73))
        .p_4()
        .child(
            div()
                .text_xl()
                .text_color(rgb(0xffaaa2))
                .child(title.into()),
        )
        .child(error.into())
}

fn render_card_badge(label: String, color: Rgb) -> impl IntoElement {
    let color = color_to_u32(color);
    div()
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(rgb(color))
        .text_color(rgb(color))
        .text_sm()
        .child(label)
}

fn parse_hex_color(value: &str) -> Option<Rgb> {
    let digits = value.strip_prefix('#').unwrap_or(value);
    (digits.len() == 6)
        .then(|| u32::from_str_radix(digits, 16).ok())
        .flatten()
        .map(Rgb::from_hex)
}

fn format_priority(priority: TaskPriority) -> String {
    match priority {
        TaskPriority::Low => "P3",
        TaskPriority::Medium => "P2",
        TaskPriority::High => "P1",
    }
    .to_owned()
}

fn color_to_u32(color: kanvibe_theme::Rgb) -> u32 {
    (u32::from(color.red) << 16) | (u32::from(color.green) << 8) | u32::from(color.blue)
}
