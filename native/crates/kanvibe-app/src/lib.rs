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
pub const KANVIBE_APP_DATA_DIR_ENV: &str = "KANVIBE_APP_DATA_DIR";
pub const KANVIBE_LOCALE_ENV: &str = "KANVIBE_LOCALE";

/// `electron-builder.yml`의 `productName`. userData 디렉터리 이름이 Electron과 같아야 데이터가 이어진다.
pub const PRODUCT_NAME: &str = "KanVibe";
pub const RUNTIME_DATABASE_FILE_NAME: &str = "kanvibe.db";
pub const BUNDLED_SEED_RELATIVE_PATH: &str = "resources/database/app.seed.db";
const FALLBACK_DATA_DIR_NAME: &str = ".kanvibe";

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
        let executable_path = std::env::current_exe().ok();
        let current_dir = std::env::current_dir()?;

        Ok(Self::from_scoped_values(
            LaunchPaths {
                executable_path,
                current_dir,
                home_dir: home_directory(),
            },
            LaunchOverrides {
                repo_root: std::env::var(KANVIBE_REPO_ROOT_ENV).ok().map(PathBuf::from),
                database_path: std::env::var(KANVIBE_DB_PATH_ENV).ok().map(PathBuf::from),
                app_data_dir: std::env::var(KANVIBE_APP_DATA_DIR_ENV).ok().map(PathBuf::from),
                locale: std::env::var(KANVIBE_LOCALE_ENV).ok(),
            },
        ))
    }

    /// 제품 기본값과 QA override를 분리해 해석한다.
    ///
    /// - 리소스 루트: 패키징된 `.app`이면 `Contents/Resources`, 아니면 현재 디렉터리.
    /// - DB: Electron `getRuntimeDatabasePath()`와 동일하게 userData 디렉터리의 `kanvibe.db`.
    /// - `KANVIBE_*` 경로는 QA override로만 동작하며 제품 기본 경로를 대체하지 않는다.
    pub fn from_scoped_values(paths: LaunchPaths, overrides: LaunchOverrides) -> Self {
        let repo_root = overrides
            .repo_root
            .unwrap_or_else(|| default_resource_root(&paths));
        let database_path = overrides
            .database_path
            .unwrap_or_else(|| runtime_database_path(overrides.app_data_dir, &paths));
        let locale = overrides
            .locale
            .as_deref()
            .and_then(Locale::parse)
            .unwrap_or(DEFAULT_LOCALE);

        Self {
            repo_root,
            database_path,
            locale,
        }
    }

    /// Electron `ensureRuntimeDatabaseFile()`에 대응한다.
    /// 런타임 DB가 없으면 번들 seed를 복사해 첫 실행에서도 보드를 열 수 있게 한다.
    pub fn ensure_database_file(&self) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
        if self.database_path.exists() {
            return Ok(self.database_path.clone());
        }

        if let Some(parent) = self.database_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let bundled_seed = self.repo_root.join(BUNDLED_SEED_RELATIVE_PATH);
        if !bundled_seed.exists() {
            return Err(format!(
                "bundled seed database not found at `{}`; cannot initialize `{}`",
                bundled_seed.display(),
                self.database_path.display()
            )
            .into());
        }

        std::fs::copy(&bundled_seed, &self.database_path)?;
        Ok(self.database_path.clone())
    }
}

/// 런타임이 관측한 경로. 테스트에서 환경변수 없이 주입할 수 있도록 값으로 받는다.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct LaunchPaths {
    pub executable_path: Option<PathBuf>,
    pub current_dir: PathBuf,
    pub home_dir: Option<PathBuf>,
}

/// QA 전용 스코프 override. 비어 있으면 제품 기본값이 사용된다.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct LaunchOverrides {
    pub repo_root: Option<PathBuf>,
    pub database_path: Option<PathBuf>,
    pub app_data_dir: Option<PathBuf>,
    pub locale: Option<String>,
}

/// 패키징된 `.app`의 `Contents/MacOS/<exe>`에서 `Contents/Resources`를 유도한다.
pub fn bundle_resource_root(executable_path: &Path) -> Option<PathBuf> {
    let macos_dir = executable_path.parent()?;
    if macos_dir.file_name()? != "MacOS" {
        return None;
    }

    let contents_dir = macos_dir.parent()?;
    if contents_dir.file_name()? != "Contents" {
        return None;
    }

    Some(contents_dir.join("Resources"))
}

fn default_resource_root(paths: &LaunchPaths) -> PathBuf {
    paths
        .executable_path
        .as_deref()
        .and_then(bundle_resource_root)
        .unwrap_or_else(|| paths.current_dir.clone())
}

/// Electron `app.getPath("userData")`와 같은 위치를 가리킨다.
pub fn default_user_data_dir(home_dir: Option<&Path>) -> Option<PathBuf> {
    let home_dir = home_dir?;

    if cfg!(target_os = "macos") {
        return Some(home_dir.join("Library/Application Support").join(PRODUCT_NAME));
    }
    if cfg!(target_os = "windows") {
        return Some(home_dir.join("AppData/Roaming").join(PRODUCT_NAME));
    }

    Some(home_dir.join(".config").join(PRODUCT_NAME))
}

fn runtime_database_path(app_data_dir: Option<PathBuf>, paths: &LaunchPaths) -> PathBuf {
    app_data_dir
        .or_else(|| default_user_data_dir(paths.home_dir.as_deref()))
        .unwrap_or_else(|| paths.current_dir.join(FALLBACK_DATA_DIR_NAME))
        .join(RUNTIME_DATABASE_FILE_NAME)
}

fn home_directory() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|home| !home.as_os_str().is_empty())
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

/// Electron `extractInternalRoute`(`src/desktop/main/windowOpen.ts`)의 포팅.
///
/// 절대 URL은 hash 유무와 무관하게 먼저 origin을 검증한다. `file:` 스킴만 예외로 hash 경로를
/// 내부 라우트로 받아들이며, 그 외에는 renderer dev origin과 일치할 때만 내부 라우트로 취급한다.
pub fn extract_internal_route(target_url: &str, renderer_dev_url: Option<&str>) -> Option<String> {
    if target_url.is_empty() {
        return None;
    }

    if target_url.starts_with('/') || target_url.starts_with('#') {
        return normalize_internal_route(target_url);
    }

    let parsed = ParsedUrl::parse(target_url)?;
    let hash_route = parsed.hash.as_deref().and_then(normalize_internal_route);

    if parsed.scheme == "file" {
        return hash_route;
    }

    let renderer_origin = ParsedUrl::parse(renderer_dev_url?)?.origin();
    if parsed.origin() != renderer_origin {
        return None;
    }

    hash_route.or_else(|| normalize_internal_route(&parsed.path))
}

/// 내부 라우트 판정에 필요한 최소 URL 구성요소(스킴/오리진/경로/해시)만 다루는 파서.
#[derive(Debug, Clone, Eq, PartialEq)]
struct ParsedUrl {
    scheme: String,
    host: String,
    port: Option<String>,
    path: String,
    hash: Option<String>,
}

impl ParsedUrl {
    fn parse(value: &str) -> Option<Self> {
        let (scheme, rest) = value.split_once(':')?;
        if !scheme.starts_with(|character: char| character.is_ascii_alphabetic())
            || !scheme
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.'))
        {
            return None;
        }

        // 계층형 URL만 origin 비교 대상이다. `mailto:`처럼 authority가 없는 스킴은 외부로 취급한다.
        let rest = rest.strip_prefix("//")?;

        let (before_hash, hash) = match rest.find('#') {
            Some(index) => (&rest[..index], Some(rest[index..].to_owned())),
            None => (rest, None),
        };

        let (authority, path_and_query) = match before_hash.find(['/', '?']) {
            Some(index) => (&before_hash[..index], &before_hash[index..]),
            None => (before_hash, ""),
        };

        // origin은 userinfo를 포함하지 않는다.
        let host_port = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
        let (host, port) = match host_port.rsplit_once(':') {
            Some((host, port)) if port.chars().all(|character| character.is_ascii_digit()) => {
                (host, Some(port.to_owned()))
            }
            _ => (host_port, None),
        };

        let path = path_and_query.split('?').next().unwrap_or("");
        let path = if path.is_empty() { "/" } else { path };

        Some(Self {
            scheme: scheme.to_ascii_lowercase(),
            host: host.to_ascii_lowercase(),
            port,
            path: path.to_owned(),
            hash,
        })
    }

    fn origin(&self) -> String {
        let port = self
            .port
            .as_deref()
            .filter(|port| !self.is_default_port(port));

        match port {
            Some(port) => format!("{}://{}:{port}", self.scheme, self.host),
            None => format!("{}://{}", self.scheme, self.host),
        }
    }

    fn is_default_port(&self, port: &str) -> bool {
        matches!(
            (self.scheme.as_str(), port),
            ("http", "80") | ("https", "443") | ("ws", "80") | ("wss", "443")
        )
    }
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

    fn bundled_launch_paths() -> LaunchPaths {
        LaunchPaths {
            executable_path: Some("/Applications/KanVibe.app/Contents/MacOS/KanVibe".into()),
            current_dir: PathBuf::from("/"),
            home_dir: Some(PathBuf::from("/Users/qa")),
        }
    }

    #[test]
    fn native_ui_launch_config_uses_only_scoped_overrides() {
        let config = NativeUiLaunchConfig::from_scoped_values(
            LaunchPaths {
                current_dir: PathBuf::from("/repo/root"),
                ..LaunchPaths::default()
            },
            LaunchOverrides {
                repo_root: Some("/repo/root".into()),
                database_path: Some("/tmp/kanvibe.sqlite".into()),
                locale: Some("en".to_owned()),
                ..LaunchOverrides::default()
            },
        );

        assert_eq!(config.repo_root, Path::new("/repo/root"));
        assert_eq!(config.database_path, Path::new("/tmp/kanvibe.sqlite"));
        assert_eq!(config.locale, Locale::En);

        let fallback = NativeUiLaunchConfig::from_scoped_values(
            LaunchPaths {
                current_dir: PathBuf::from("/repo/root"),
                home_dir: Some(PathBuf::from("/home/dev")),
                ..LaunchPaths::default()
            },
            LaunchOverrides {
                locale: Some("unsupported".to_owned()),
                ..LaunchOverrides::default()
            },
        );
        assert_eq!(fallback.repo_root, Path::new("/repo/root"));
        assert_eq!(
            fallback.database_path,
            default_user_data_dir(Some(Path::new("/home/dev")))
                .expect("user data dir")
                .join(RUNTIME_DATABASE_FILE_NAME)
        );
        assert_eq!(fallback.locale, DEFAULT_LOCALE);
    }

    /// 환경변수 없이 Finder에서 실행한 `.app`은 번들 리소스와 userData DB만으로 부팅해야 한다.
    #[test]
    fn packaged_bundle_launch_resolves_product_paths_without_env_overrides() {
        let config =
            NativeUiLaunchConfig::from_scoped_values(bundled_launch_paths(), LaunchOverrides::default());

        assert_eq!(
            config.repo_root,
            Path::new("/Applications/KanVibe.app/Contents/Resources")
        );
        assert_eq!(
            config.repo_root.join(Locale::Ko.catalog_relative_path()),
            Path::new("/Applications/KanVibe.app/Contents/Resources/messages/ko.json")
        );
        assert!(!config.database_path.starts_with("/Applications"));
        assert_eq!(config.database_path.file_name().expect("db file name"), "kanvibe.db");
        assert_eq!(
            config.database_path,
            default_user_data_dir(Some(Path::new("/Users/qa")))
                .expect("user data dir")
                .join(RUNTIME_DATABASE_FILE_NAME)
        );
        assert_eq!(config.locale, DEFAULT_LOCALE);
    }

    #[test]
    fn qa_overrides_still_reach_repo_fixtures() {
        let config = NativeUiLaunchConfig::from_scoped_values(
            bundled_launch_paths(),
            LaunchOverrides {
                repo_root: Some("/repo".into()),
                database_path: Some("/repo/qa/seed/kanvibe-seed.sqlite".into()),
                app_data_dir: Some("/tmp/app-data".into()),
                locale: Some("zh".to_owned()),
            },
        );

        assert_eq!(config.repo_root, Path::new("/repo"));
        assert_eq!(
            config.database_path,
            Path::new("/repo/qa/seed/kanvibe-seed.sqlite")
        );
        assert_eq!(config.locale, Locale::Zh);
    }

    #[test]
    fn app_data_dir_override_drives_runtime_database_location() {
        let config = NativeUiLaunchConfig::from_scoped_values(
            bundled_launch_paths(),
            LaunchOverrides {
                app_data_dir: Some("/tmp/app-data".into()),
                ..LaunchOverrides::default()
            },
        );

        assert_eq!(config.database_path, Path::new("/tmp/app-data/kanvibe.db"));
    }

    #[test]
    fn bundle_resource_root_only_matches_macos_app_layout() {
        assert_eq!(
            bundle_resource_root(Path::new("/Applications/KanVibe.app/Contents/MacOS/KanVibe")),
            Some(PathBuf::from("/Applications/KanVibe.app/Contents/Resources"))
        );
        assert_eq!(
            bundle_resource_root(Path::new("/repo/native/target/release/kanvibe-app")),
            None
        );
    }

    #[test]
    fn ensure_database_file_copies_bundled_seed_on_first_launch() {
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-launch-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let resource_root = temp_root.join("Resources");
        let user_data_dir = temp_root.join("user-data");
        std::fs::create_dir_all(resource_root.join("resources/database")).expect("resource dirs");
        std::fs::write(resource_root.join(BUNDLED_SEED_RELATIVE_PATH), b"seed-bytes").expect("seed");

        let config = NativeUiLaunchConfig::from_scoped_values(
            LaunchPaths::default(),
            LaunchOverrides {
                repo_root: Some(resource_root),
                app_data_dir: Some(user_data_dir.clone()),
                ..LaunchOverrides::default()
            },
        );

        let database_path = config.ensure_database_file().expect("initialize runtime database");

        assert_eq!(database_path, user_data_dir.join(RUNTIME_DATABASE_FILE_NAME));
        assert_eq!(
            std::fs::read(&database_path).expect("runtime database bytes"),
            b"seed-bytes"
        );

        std::fs::remove_dir_all(&temp_root).ok();
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

        // 외부 origin은 hash가 붙어 있어도 내부 라우트로 승격되지 않는다.
        assert_eq!(
            extract_internal_route("https://example.com/#/ko/settings", Some("http://localhost:3000")),
            None
        );
        assert_eq!(extract_internal_route("https://example.com/#/ko/settings", None), None);
        assert_eq!(
            extract_internal_route(
                "http://localhost:3000/#/ko/settings",
                Some("http://localhost:3000")
            )
            .as_deref(),
            Some("/ko/settings")
        );
        // renderer origin과 스킴/포트가 다르면 외부다.
        assert_eq!(
            extract_internal_route("http://localhost:4000/#/ko", Some("http://localhost:3000")),
            None
        );
        assert_eq!(
            extract_internal_route("https://localhost:3000/#/ko", Some("http://localhost:3000")),
            None
        );
        // userinfo와 기본 포트 표기는 origin 비교에 영향을 주지 않는다.
        assert_eq!(
            extract_internal_route("http://user:pw@localhost:3000/#/ko", Some("http://localhost:3000"))
                .as_deref(),
            Some("/ko")
        );
        assert_eq!(
            extract_internal_route("https://example.com:443/#/ko", Some("https://example.com")).as_deref(),
            Some("/ko")
        );
        // authority가 없는 스킴은 외부로 취급한다.
        assert_eq!(
            extract_internal_route("mailto:someone@example.com", Some("http://localhost:3000")),
            None
        );
        assert_eq!(
            resolve_window_open_action(
                "https://example.com/#/ko/settings",
                Some("http://localhost:3000"),
                &[],
                None,
            ),
            WindowOpenAction::External
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
