use std::{
    error::Error,
    path::{Path, PathBuf},
};

use kanvibe_core::{
    BackgroundSyncSettings, BoardSnapshot, DONE_PAGE_SIZE, KanbanTask, KanvibeDb, SessionType,
    TaskPriority, TaskStatus, ThemePreference,
};
use kanvibe_i18n::{BoardLabels, DEFAULT_LOCALE, Locale, load_board_labels};
use kanvibe_theme::{NEUTRAL_BUTTON_SURFACE, PRIMARY, Rgb, status_color};

#[cfg(all(target_os = "macos", feature = "native-ui"))]
mod native_ui;
pub mod qa_control;

pub const KANVIBE_REPO_ROOT_ENV: &str = "KANVIBE_REPO_ROOT";
pub const KANVIBE_DB_PATH_ENV: &str = "KANVIBE_DB_PATH";
pub const KANVIBE_LOCALE_ENV: &str = "KANVIBE_LOCALE";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RunMode {
    NativeUi,
    HeadlessStub,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ReadOnlyBoardBootstrap {
    pub board: BoardSnapshot,
    pub labels: BoardLabels,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ReadOnlyBoardShell {
    pub columns: Vec<ReadOnlyBoardShellColumn>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ReadOnlyBoardShellColumn {
    pub status: TaskStatus,
    pub label: String,
    pub color: Rgb,
    pub task_count: usize,
    pub cards: Vec<ReadOnlyBoardShellCard>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ReadOnlyBoardShellCard {
    pub id: String,
    pub title: String,
    pub project_id: Option<String>,
    pub priority: Option<TaskPriority>,
    pub branch_name: Option<String>,
    pub pr_url: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeUiLaunchConfig {
    pub repo_root: PathBuf,
    pub database_path: PathBuf,
    pub locale: Locale,
}

impl NativeUiLaunchConfig {
    pub fn from_env() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let current_dir = std::env::current_dir()?;

        Ok(Self::from_scoped_values(
            current_dir,
            std::env::var(KANVIBE_REPO_ROOT_ENV).ok().map(PathBuf::from),
            std::env::var(KANVIBE_DB_PATH_ENV).ok().map(PathBuf::from),
            std::env::var(KANVIBE_LOCALE_ENV).ok(),
        ))
    }

    pub fn from_scoped_values(
        current_dir: impl Into<PathBuf>,
        repo_root: Option<PathBuf>,
        database_path: Option<PathBuf>,
        locale: Option<String>,
    ) -> Self {
        let repo_root = repo_root.unwrap_or_else(|| current_dir.into());
        let database_path =
            database_path.unwrap_or_else(|| repo_root.join("qa/seed/kanvibe-seed.sqlite"));
        let locale = locale
            .as_deref()
            .and_then(Locale::parse)
            .unwrap_or(DEFAULT_LOCALE);

        Self {
            repo_root,
            database_path,
            locale,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeUiRenderSpec {
    pub window_title: String,
    pub route: String,
    pub locale: Locale,
    pub primary_action_label: String,
    pub all_projects_label: String,
    pub project_count: usize,
    pub total_visible_tasks: usize,
    pub done_total: u32,
    pub brand_primary: Rgb,
    pub neutral_button_surface: Rgb,
    pub columns: Vec<NativeUiColumnSpec>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeUiColumnSpec {
    pub status: TaskStatus,
    pub label: String,
    pub color: Rgb,
    pub task_count: usize,
    pub first_card_title: Option<String>,
    pub cards: Vec<NativeUiCardSpec>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeUiCardSpec {
    pub id: String,
    pub title: String,
    pub status: String,
    pub project_id: Option<String>,
    pub branch_name: Option<String>,
    pub base_branch: Option<String>,
    pub session_type: Option<String>,
    pub ssh_host: Option<String>,
    pub pr_url: Option<String>,
    pub priority: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ShortcutPlatform {
    Mac,
    Linux,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ShortcutInput {
    pub key: char,
    pub meta: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TaskDetailDockItem {
    pub id: &'static str,
    pub label: &'static str,
    pub shortcut_index: usize,
    pub shortcut_label: String,
    pub href: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TaskDetailShell {
    pub task_id: String,
    pub href: String,
    pub title: String,
    pub session_type: Option<String>,
    pub session_name: Option<String>,
    pub ssh_host: Option<String>,
    pub dock_items: Vec<TaskDetailDockItem>,
    pub ai_provider_filters: Vec<&'static str>,
    pub sidebar_default_collapsed: bool,
    pub sidebar_hint_dismissed: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TaskNavigationDecision {
    FocusExisting,
    Navigate,
    OpenNewWindow,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SettingsShell {
    pub route: String,
    pub theme_preference: ThemePreference,
    pub default_session_type: SessionType,
    pub task_search_shortcut: String,
    pub vim_mode_enabled: bool,
    pub sidebar_default_collapsed: bool,
    pub done_alert_dismissed: bool,
    pub background_sync: BackgroundSyncSettings,
    pub release_update_dismissed_versions: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct WindowRecord {
    pub id: String,
    pub url: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum WindowOpenAction {
    External,
    OpenInternal {
        route: String,
        outlives_opener: bool,
    },
    FocusExisting {
        route: String,
        existing_window_id: String,
    },
}

pub fn run() -> Result<RunMode, String> {
    #[cfg(all(target_os = "macos", feature = "native-ui"))]
    {
        native_ui::run_native_ui().map_err(|error| error.to_string())?;
        return Ok(RunMode::NativeUi);
    }

    #[cfg(not(all(target_os = "macos", feature = "native-ui")))]
    {
        Ok(RunMode::HeadlessStub)
    }
}

pub fn load_read_only_board(
    repo_root: impl AsRef<Path>,
    database_path: impl AsRef<Path>,
    locale: Locale,
) -> Result<ReadOnlyBoardBootstrap, Box<dyn Error + Send + Sync>> {
    let database = KanvibeDb::open_read_only(database_path)?;
    let board = database.board_snapshot(DONE_PAGE_SIZE)?;
    let labels = load_board_labels(repo_root, locale)?;

    Ok(ReadOnlyBoardBootstrap { board, labels })
}

pub fn build_read_only_board_shell(bootstrap: &ReadOnlyBoardBootstrap) -> ReadOnlyBoardShell {
    let columns = bootstrap
        .labels
        .columns
        .iter()
        .map(|label| {
            let tasks = bootstrap
                .board
                .column(label.status)
                .map(|column| column.tasks.as_slice())
                .unwrap_or_default();

            ReadOnlyBoardShellColumn {
                status: label.status,
                label: label.label.clone(),
                color: status_color(label.status),
                task_count: tasks.len(),
                cards: tasks.iter().map(shell_card_from_task).collect(),
            }
        })
        .collect();

    ReadOnlyBoardShell { columns }
}

pub fn build_native_ui_render_spec(bootstrap: &ReadOnlyBoardBootstrap) -> NativeUiRenderSpec {
    let columns = bootstrap
        .labels
        .columns
        .iter()
        .map(|label| {
            let tasks = bootstrap
                .board
                .column(label.status)
                .map(|column| column.tasks.as_slice())
                .unwrap_or_default();

            NativeUiColumnSpec {
                status: label.status,
                label: label.label.clone(),
                color: status_color(label.status),
                task_count: tasks.len(),
                first_card_title: tasks.first().map(|task| task.title.clone()),
                cards: tasks
                    .iter()
                    .map(|task| NativeUiCardSpec {
                        id: task.id.clone(),
                        title: task.title.clone(),
                        status: task.status.as_str().to_owned(),
                        project_id: task.project_id.clone(),
                        branch_name: task.branch_name.clone(),
                        base_branch: task.base_branch.clone(),
                        session_type: task
                            .session_type
                            .map(|session_type| session_type.as_str().to_owned()),
                        ssh_host: task.ssh_host.clone(),
                        pr_url: task.pr_url.clone(),
                        priority: task.priority.map(|priority| priority.as_str().to_owned()),
                    })
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let total_visible_tasks = columns.iter().map(|column| column.task_count).sum();

    NativeUiRenderSpec {
        window_title: "KanVibe".to_owned(),
        route: localize_href("/", Some(bootstrap.labels.locale.code())),
        locale: bootstrap.labels.locale,
        primary_action_label: bootstrap.labels.new_task.clone(),
        all_projects_label: bootstrap.labels.all_projects.clone(),
        project_count: bootstrap.board.projects.len(),
        total_visible_tasks,
        done_total: bootstrap.board.done_total,
        brand_primary: PRIMARY,
        neutral_button_surface: NEUTRAL_BUTTON_SURFACE,
        columns,
    }
}

fn shell_card_from_task(task: &KanbanTask) -> ReadOnlyBoardShellCard {
    ReadOnlyBoardShellCard {
        id: task.id.clone(),
        title: task.title.clone(),
        project_id: task.project_id.clone(),
        priority: task.priority,
        branch_name: task.branch_name.clone(),
        pr_url: task.pr_url.clone(),
    }
}

pub fn localize_href(href: &str, current_locale: Option<&str>) -> String {
    if !href.starts_with('/') {
        return href.to_owned();
    }

    let locale = match current_locale {
        Some("en") => "en",
        Some("zh") => "zh",
        _ => "ko",
    };
    let first_segment = href
        .split('/')
        .filter(|segment| !segment.is_empty())
        .next()
        .unwrap_or_default();

    if matches!(first_segment, "ko" | "en" | "zh") {
        return href.to_owned();
    }

    if href == "/" {
        format!("/{locale}")
    } else {
        format!("/{locale}{href}")
    }
}

pub fn task_detail_href(task_id: &str, current_locale: Option<&str>) -> String {
    localize_href(&format!("/task/{task_id}"), current_locale)
}

pub fn decide_task_navigation(
    task_id: &str,
    current_locale: Option<&str>,
    open_in_new_window: bool,
    existing_routes: &[String],
) -> TaskNavigationDecision {
    if open_in_new_window {
        return TaskNavigationDecision::OpenNewWindow;
    }

    let href = task_detail_href(task_id, current_locale);
    if existing_routes.iter().any(|route| route == &href) {
        TaskNavigationDecision::FocusExisting
    } else {
        TaskNavigationDecision::Navigate
    }
}

pub fn settings_route(current_locale: Option<&str>) -> String {
    localize_href("/settings", current_locale)
}

pub fn pane_layout_route(current_locale: Option<&str>) -> String {
    localize_href("/pane-layout", current_locale)
}

pub fn build_settings_shell(
    database: &KanvibeDb,
    current_locale: Option<&str>,
) -> Result<SettingsShell, Box<dyn Error + Send + Sync>> {
    Ok(SettingsShell {
        route: settings_route(current_locale),
        theme_preference: database.theme_preference()?,
        default_session_type: database.default_session_type()?,
        task_search_shortcut: database.task_search_shortcut()?,
        vim_mode_enabled: database.vim_mode_enabled()?,
        sidebar_default_collapsed: database.sidebar_default_collapsed()?,
        done_alert_dismissed: database.done_alert_dismissed()?,
        background_sync: database.background_sync_settings()?,
        release_update_dismissed_versions: database.release_update_dismissed_versions()?,
    })
}

pub fn should_keep_current_route_for_notification_activation(action_type: Option<&str>) -> bool {
    action_type == Some("background-sync-review")
}

pub fn extract_internal_route(target_url: &str, renderer_dev_url: Option<&str>) -> Option<String> {
    if target_url.is_empty() {
        return None;
    }

    if target_url.starts_with('/') || target_url.starts_with('#') {
        return normalize_internal_route(target_url);
    }

    if let Some(hash_index) = target_url.find('#') {
        return normalize_internal_route(&target_url[hash_index..]);
    }

    let renderer_dev_url = renderer_dev_url?;
    if !target_url.starts_with(renderer_dev_url) {
        return None;
    }

    let path = target_url
        .strip_prefix(renderer_dev_url)
        .filter(|path| !path.is_empty())?;
    normalize_internal_route(path)
}

pub fn resolve_window_open_action(
    target_url: &str,
    renderer_dev_url: Option<&str>,
    open_windows: &[WindowRecord],
    exclude_window_id: Option<&str>,
) -> WindowOpenAction {
    let Some(route) = extract_internal_route(target_url, renderer_dev_url) else {
        return WindowOpenAction::External;
    };

    if let Some(existing_window) = open_windows.iter().find(|window| {
        Some(window.id.as_str()) != exclude_window_id
            && extract_internal_route(&window.url, renderer_dev_url).as_deref()
                == Some(route.as_str())
    }) {
        return WindowOpenAction::FocusExisting {
            route,
            existing_window_id: existing_window.id.clone(),
        };
    }

    WindowOpenAction::OpenInternal {
        route,
        outlives_opener: true,
    }
}

fn normalize_internal_route(route: &str) -> Option<String> {
    if route.is_empty() {
        return None;
    }

    if let Some(rest) = route.strip_prefix("/#/") {
        return Some(format!("/{rest}"));
    }

    if let Some(rest) = route.strip_prefix("#/") {
        return Some(format!("/{rest}"));
    }

    if route.starts_with('/') {
        return Some(route.to_owned());
    }

    Some(format!("/{route}"))
}

pub fn build_task_detail_shell(
    database: &KanvibeDb,
    task_id: &str,
    current_locale: Option<&str>,
    platform: ShortcutPlatform,
) -> Result<Option<TaskDetailShell>, Box<dyn Error + Send + Sync>> {
    let Some(task) = database.task_by_id(task_id)? else {
        return Ok(None);
    };

    Ok(Some(TaskDetailShell {
        task_id: task.id.clone(),
        href: task_detail_href(&task.id, current_locale),
        title: task.title.clone(),
        session_type: task
            .session_type
            .map(|session_type| session_type.as_str().to_owned()),
        session_name: task.session_name.clone(),
        ssh_host: task.ssh_host.clone(),
        dock_items: task_detail_dock_items(task.pr_url.as_deref(), platform),
        ai_provider_filters: ai_provider_filters(),
        sidebar_default_collapsed: database.sidebar_default_collapsed()?,
        sidebar_hint_dismissed: database.sidebar_hint_dismissed()?,
    }))
}

pub fn task_detail_dock_items(
    pr_url: Option<&str>,
    platform: ShortcutPlatform,
) -> Vec<TaskDetailDockItem> {
    let mut items = vec![
        ("overview", "Overview", None),
        ("status", "Status", None),
        ("terminal", "Terminal", None),
        ("chat", "Chat", None),
        ("aiSessions", "AI Sessions", None),
    ];

    if let Some(url) = pr_url {
        items.insert(3, ("pullRequest", "PR", Some(url.to_owned())));
    }

    items
        .into_iter()
        .enumerate()
        .map(|(index, (id, label, href))| {
            let shortcut_index = index + 1;
            TaskDetailDockItem {
                id,
                label,
                shortcut_index,
                shortcut_label: task_detail_dock_shortcut_label(shortcut_index, platform),
                href,
            }
        })
        .collect()
}

pub fn task_detail_dock_shortcut_label(index: usize, platform: ShortcutPlatform) -> String {
    match platform {
        ShortcutPlatform::Mac => format!("Cmd+{index}"),
        ShortcutPlatform::Linux => format!("Alt+{index}"),
    }
}

pub fn match_task_detail_dock_shortcut(
    input: ShortcutInput,
    platform: ShortcutPlatform,
) -> Option<usize> {
    let Some(index) = input.key.to_digit(10).map(|digit| digit as usize) else {
        return None;
    };

    if !(1..=9).contains(&index) || input.shift {
        return None;
    }

    let matches_platform = match platform {
        ShortcutPlatform::Mac => input.meta && !input.ctrl && !input.alt,
        ShortcutPlatform::Linux => input.alt && !input.meta && !input.ctrl,
    };

    matches_platform.then_some(index)
}

pub fn ai_provider_filters() -> Vec<&'static str> {
    vec!["claude", "codex", "gemini", "opencode"]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn default_build_uses_stub_until_macos_ui_feature_is_enabled() {
        assert_eq!(run(), Ok(RunMode::HeadlessStub));
    }

    #[test]
    fn read_only_bootstrap_loads_seed_board_and_labels() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let seed = repo_root.join("qa/seed/kanvibe-seed.sqlite");
        let bootstrap =
            load_read_only_board(&repo_root, seed, Locale::En).expect("read-only board bootstrap");

        assert_eq!(bootstrap.board.projects.len(), 3);
        assert_eq!(bootstrap.board.done_total, 3);
        assert_eq!(bootstrap.labels.columns.len(), 5);
        assert_eq!(bootstrap.labels.columns[0].label, "Todo");
    }

    #[test]
    fn read_only_board_shell_combines_labels_colors_and_cards() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let seed = repo_root.join("qa/seed/kanvibe-seed.sqlite");
        let bootstrap =
            load_read_only_board(&repo_root, seed, Locale::En).expect("read-only board bootstrap");
        let shell = build_read_only_board_shell(&bootstrap);

        assert_eq!(shell.columns.len(), 5);
        assert_eq!(shell.columns[0].status, TaskStatus::Todo);
        assert_eq!(shell.columns[0].label, "Todo");
        assert_eq!(shell.columns[0].color, status_color(TaskStatus::Todo));
        assert_eq!(shell.columns[0].task_count, 3);
        assert_eq!(shell.columns[0].cards[0].title, "Draft native board shell");
    }

    #[test]
    fn task_detail_dock_shortcuts_insert_pr_at_slot_four() {
        let without_pr = task_detail_dock_items(None, ShortcutPlatform::Mac);
        assert_eq!(
            without_pr
                .iter()
                .map(|item| (item.id, item.shortcut_label.as_str()))
                .collect::<Vec<_>>(),
            [
                ("overview", "Cmd+1"),
                ("status", "Cmd+2"),
                ("terminal", "Cmd+3"),
                ("chat", "Cmd+4"),
                ("aiSessions", "Cmd+5"),
            ]
        );

        let with_pr =
            task_detail_dock_items(Some("https://example.test/pull/1"), ShortcutPlatform::Mac);
        assert_eq!(
            with_pr
                .iter()
                .map(|item| (item.id, item.shortcut_label.as_str()))
                .collect::<Vec<_>>(),
            [
                ("overview", "Cmd+1"),
                ("status", "Cmd+2"),
                ("terminal", "Cmd+3"),
                ("pullRequest", "Cmd+4"),
                ("chat", "Cmd+5"),
                ("aiSessions", "Cmd+6"),
            ]
        );
    }

    #[test]
    fn task_detail_dock_shortcut_matching_uses_platform_modifiers() {
        assert_eq!(
            match_task_detail_dock_shortcut(
                ShortcutInput {
                    key: '4',
                    meta: true,
                    ctrl: false,
                    alt: false,
                    shift: false,
                },
                ShortcutPlatform::Mac,
            ),
            Some(4)
        );
        assert_eq!(
            match_task_detail_dock_shortcut(
                ShortcutInput {
                    key: '4',
                    meta: false,
                    ctrl: false,
                    alt: true,
                    shift: false,
                },
                ShortcutPlatform::Linux,
            ),
            Some(4)
        );
        assert_eq!(
            match_task_detail_dock_shortcut(
                ShortcutInput {
                    key: '4',
                    meta: false,
                    ctrl: true,
                    alt: false,
                    shift: false,
                },
                ShortcutPlatform::Mac,
            ),
            None
        );
    }

    #[test]
    fn task_detail_navigation_localizes_and_focuses_existing_route() {
        assert_eq!(
            task_detail_href("qa-task-progress-pr", Some("ko")),
            "/ko/task/qa-task-progress-pr"
        );
        assert_eq!(
            decide_task_navigation(
                "qa-task-progress-pr",
                Some("ko"),
                false,
                &["/ko/task/qa-task-progress-pr".to_owned()],
            ),
            TaskNavigationDecision::FocusExisting
        );
        assert_eq!(
            decide_task_navigation("qa-task-progress-pr", Some("ko"), false, &[]),
            TaskNavigationDecision::Navigate
        );
        assert_eq!(
            decide_task_navigation(
                "qa-task-progress-pr",
                Some("ko"),
                true,
                &["/ko/task/qa-task-progress-pr".to_owned()],
            ),
            TaskNavigationDecision::OpenNewWindow
        );
    }

    #[test]
    fn task_detail_shell_loads_terminal_pr_and_sidebar_state() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let seed = repo_root.join("qa/seed/kanvibe-seed.sqlite");
        let database = KanvibeDb::open_read_only(seed).expect("seed should open");
        let terminal_shell = build_task_detail_shell(
            &database,
            "qa-task-progress-terminal",
            Some("ko"),
            ShortcutPlatform::Mac,
        )
        .expect("task detail should load")
        .expect("task should exist");

        assert_eq!(terminal_shell.href, "/ko/task/qa-task-progress-terminal");
        assert_eq!(terminal_shell.session_type.as_deref(), Some("tmux"));
        assert_eq!(terminal_shell.dock_items[2].id, "terminal");
        assert_eq!(terminal_shell.ai_provider_filters, ai_provider_filters());

        let pr_shell = build_task_detail_shell(
            &database,
            "qa-task-review-diff",
            Some("ko"),
            ShortcutPlatform::Mac,
        )
        .expect("task detail should load")
        .expect("task should exist");
        assert_eq!(pr_shell.dock_items[3].id, "pullRequest");
        assert!(
            pr_shell.dock_items[3]
                .href
                .as_deref()
                .is_some_and(|href| href.contains("/pull/302"))
        );
    }

    #[test]
    fn settings_shell_reads_app_wide_preferences() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let seed = repo_root.join("qa/seed/kanvibe-seed.sqlite");
        let database = KanvibeDb::open_read_only(seed).expect("seed should open");
        let shell = build_settings_shell(&database, Some("ko")).expect("settings shell");

        assert_eq!(shell.route, "/ko/settings");
        assert_eq!(shell.theme_preference, ThemePreference::Dark);
        assert_eq!(shell.default_session_type, SessionType::Tmux);
        assert_eq!(shell.task_search_shortcut, "Mod+Shift+O");
        assert!(shell.vim_mode_enabled);
        assert!(!shell.background_sync.is_enabled);
        assert_eq!(shell.background_sync.interval_ms, 300_000);
    }

    #[test]
    fn native_ui_launch_config_uses_only_scoped_overrides() {
        let config = NativeUiLaunchConfig::from_scoped_values(
            "/repo/root",
            None,
            Some("/tmp/kanvibe.sqlite".into()),
            Some("en".to_owned()),
        );

        assert_eq!(config.repo_root, Path::new("/repo/root"));
        assert_eq!(config.database_path, Path::new("/tmp/kanvibe.sqlite"));
        assert_eq!(config.locale, Locale::En);

        let fallback = NativeUiLaunchConfig::from_scoped_values(
            "/repo/root",
            None,
            None,
            Some("unsupported".to_owned()),
        );
        assert_eq!(
            fallback.database_path,
            Path::new("/repo/root/qa/seed/kanvibe-seed.sqlite")
        );
        assert_eq!(fallback.locale, DEFAULT_LOCALE);
    }

    #[test]
    fn native_ui_render_spec_maps_seed_board_to_gpui_shell_contract() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let seed = repo_root.join("qa/seed/kanvibe-seed.sqlite");
        let bootstrap =
            load_read_only_board(&repo_root, seed, Locale::En).expect("read-only board bootstrap");
        let spec = build_native_ui_render_spec(&bootstrap);

        assert_eq!(spec.window_title, "KanVibe");
        assert_eq!(spec.route, "/en");
        assert_eq!(spec.locale, Locale::En);
        assert_eq!(spec.primary_action_label, "+ New Task");
        assert_eq!(spec.all_projects_label, "All Projects");
        assert_eq!(spec.project_count, 3);
        assert_eq!(spec.total_visible_tasks, 15);
        assert_eq!(spec.done_total, 3);
        assert_eq!(spec.brand_primary, PRIMARY);
        assert_eq!(spec.neutral_button_surface, NEUTRAL_BUTTON_SURFACE);
        assert_eq!(spec.columns.len(), 5);
        assert_eq!(spec.columns[0].label, "Todo");
        assert_eq!(spec.columns[0].task_count, 3);
        assert_eq!(
            spec.columns[0].first_card_title.as_deref(),
            Some("Draft native board shell")
        );
    }

    #[test]
    fn native_ui_source_uses_gpui_component_root_entrypoint() {
        let source = include_str!("native_ui.rs");

        assert!(source.contains("Application::new()"));
        assert!(source.contains("gpui_component::init(cx)"));
        assert!(source.contains("Root::new(view, window, cx)"));
        assert!(source.contains("Button::new(\"new-task\")"));
        assert!(!source.contains("webview"));
        assert!(!source.contains("electron"));
    }

    #[test]
    fn window_open_policy_extracts_internal_routes_and_focuses_existing_window() {
        assert_eq!(
            extract_internal_route(
                "file:///Applications/Kanvibe.app/Contents/Resources/app.asar/build/renderer/index.html#/ko/task/task-1",
                None,
            )
            .as_deref(),
            Some("/ko/task/task-1")
        );
        assert_eq!(
            extract_internal_route("https://example.com/docs", Some("http://localhost:3000")),
            None
        );

        let open_windows = vec![
            WindowRecord {
                id: "window-1".to_owned(),
                url: "http://localhost:3000/#/ko".to_owned(),
            },
            WindowRecord {
                id: "window-2".to_owned(),
                url: "http://localhost:3000/#/ko/task/task-1".to_owned(),
            },
        ];
        assert_eq!(
            resolve_window_open_action(
                "http://localhost:3000/#/ko/task/task-1",
                Some("http://localhost:3000"),
                &open_windows,
                None,
            ),
            WindowOpenAction::FocusExisting {
                route: "/ko/task/task-1".to_owned(),
                existing_window_id: "window-2".to_owned(),
            }
        );
        assert_eq!(
            resolve_window_open_action(
                "/#/ko/task/task-1",
                Some("http://localhost:3000"),
                &open_windows[1..],
                Some("window-2"),
            ),
            WindowOpenAction::OpenInternal {
                route: "/ko/task/task-1".to_owned(),
                outlives_opener: true,
            }
        );
        assert!(should_keep_current_route_for_notification_activation(Some(
            "background-sync-review"
        )));
        assert!(!should_keep_current_route_for_notification_activation(None));
    }
}
