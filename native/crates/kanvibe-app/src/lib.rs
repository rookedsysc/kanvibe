use std::{
    backtrace::Backtrace,
    collections::{BTreeMap, BTreeSet, VecDeque},
    error::Error,
    fs::OpenOptions,
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use kanvibe_ai::{
    AiDetailQuery, AiSessionDataSource, AiSessionDetail, AiSessionError, AiSessionProvider,
    AiSessionQuery, AiSessionsPage, LocalAiSessionDataSource, OpenCodeMessageRow,
    OpenCodeSessionRow, read_ai_session_detail, read_ai_sessions,
};
use kanvibe_core::{
    BackgroundSyncSettings, BoardSnapshot, CreateTaskInput, DONE_PAGE_SIZE, DoneCleanupPlan,
    DoneCleanupResult, KanbanTask, KanvibeDb, PaneLayoutConfig, PaneLayoutType, Project,
    SavePaneLayoutInput, SessionType, TaskPriority, TaskStatus, TaskUpdatePatch, ThemePreference,
    create_sqlite_backup_once, migrate_electron_database, restore_sqlite_database_from_backup,
    validate_sqlite_database,
};
use kanvibe_git::{
    DiffFileStatus, GitErrorKind, PullRequestInfo, RegisteredWorktree, RemoteGitClient,
    changed_files, common_repository_root, create_worktree_with_session, default_branch,
    ensure_git_exclude_pattern, file_content, format_session_name,
    github_cli_available as local_github_cli_available,
    install_github_cli as install_local_github_cli,
    install_session_dependency as install_local_session_dependency, is_submodule_repository,
    list_worktrees, original_file_content, pull_current_branch, pull_request_for_branch,
    remote_branch_exists, remove_worktree_and_branch, save_file_content_if_unchanged,
    scan_git_repositories, session_dependency_available as local_session_dependency_available,
};
use kanvibe_hooks::{
    AppNotification, DEFAULT_HOOK_SERVER_PORT, DEV_HOOK_SERVER_PORT, HOOK_SERVER_PORT_ENV,
    HookHttpRequest, HookHttpResponse, HookHttpRoute, HookHttpServer, HookProviderStatus,
    NotificationCreation, NotificationDraft, NotificationStore, PRESERVABLE_PROVIDER_HOOK_PATHS,
    PROVIDER_HOOK_EXCLUDE_LINES, inspect_provider_hook_status, install_local_provider_hooks,
    local_hook_server_url, provider_hook_required_paths, remote_hook_server_url,
    render_provider_hooks,
};
use kanvibe_i18n::{
    BoardLabels, DEFAULT_LOCALE, Locale, MessageCatalog, load_board_labels, load_message_catalog,
};
use kanvibe_pty::{
    SESSION_DEPENDENCY_SUCCESS_CACHE_MS, is_local_tmux_session_alive, remove_session_only,
    session_dependency_tool_name,
};
use kanvibe_theme::{NEUTRAL_BUTTON_SURFACE, PRIMARY, Rgb, status_color};
use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
mod macos_notifications;
#[cfg(all(target_os = "macos", feature = "native-ui"))]
mod native_ui;
pub mod native_updater;
pub mod qa_control;

pub const KANVIBE_REPO_ROOT_ENV: &str = "KANVIBE_REPO_ROOT";
pub const KANVIBE_DB_PATH_ENV: &str = "KANVIBE_DB_PATH";
pub const KANVIBE_APP_DATA_DIR_ENV: &str = "KANVIBE_APP_DATA_DIR";
pub const KANVIBE_LOCALE_ENV: &str = "KANVIBE_LOCALE";
pub const INITIAL_NATIVE_LOAD_TIMEOUT_MS: u64 = 5_000;
pub const NATIVE_APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const NATIVE_BUILD_COMMIT: &str = match option_env!("KANVIBE_BUILD_COMMIT") {
    Some(commit) => commit,
    None => "unknown",
};
const MAX_NATIVE_CRASH_LOG_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeHookStartInput {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    branch_name: Option<String>,
    #[serde(default)]
    agent_type: Option<String>,
    #[serde(default)]
    session_type: Option<SessionType>,
    #[serde(default)]
    ssh_host: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    base_branch: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeHookStatusInput {
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

pub fn native_diagnostic_line(
    event: &str,
    route: &str,
    message: &str,
    timeout_ms: Option<u64>,
) -> String {
    serde_json::json!({
        "source": "kanvibe-native",
        "event": event,
        "route": route,
        "message": message,
        "timeoutMs": timeout_ms,
        "version": NATIVE_APP_VERSION,
        "buildCommit": NATIVE_BUILD_COMMIT,
        "pid": std::process::id(),
    })
    .to_string()
}

pub fn install_native_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        let payload = panic
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| panic.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("non-string panic payload");
        let location = panic
            .location()
            .map(|location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            })
            .unwrap_or_else(|| "unknown".to_owned());
        append_native_crash_log(
            &serde_json::json!({
                "source": "kanvibe-native",
                "event": "panic",
                "message": payload,
                "location": location,
                "backtrace": Backtrace::force_capture().to_string(),
                "version": NATIVE_APP_VERSION,
                "buildCommit": NATIVE_BUILD_COMMIT,
                "pid": std::process::id(),
            })
            .to_string(),
        );
        previous(panic);
    }));
}

pub fn append_native_crash_log(line: &str) {
    let data_dir = std::env::var_os(KANVIBE_APP_DATA_DIR_ENV)
        .map(PathBuf::from)
        .or_else(|| default_user_data_dir(home_directory().as_deref()));
    let Some(data_dir) = data_dir else {
        return;
    };
    if std::fs::create_dir_all(&data_dir).is_err() {
        return;
    }
    let path = data_dir.join("native-crash.log");
    if path
        .metadata()
        .is_ok_and(|metadata| metadata.len() >= MAX_NATIVE_CRASH_LOG_BYTES)
    {
        let previous = data_dir.join("native-crash.previous.log");
        let _ = std::fs::remove_file(&previous);
        let _ = std::fs::rename(&path, previous);
    }
    if let Ok(mut log) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(log, "{line}");
    }
}

/// `electron-builder.yml`의 `productName`. userData 디렉터리 이름이 Electron과 같아야 데이터가 이어진다.
pub const PRODUCT_NAME: &str = "KanVibe";
pub const RUNTIME_DATABASE_FILE_NAME: &str = "kanvibe.db";
pub const BUNDLED_SEED_RELATIVE_PATH: &str = "resources/database/app.seed.db";
const FALLBACK_DATA_DIR_NAME: &str = ".kanvibe";
const ELECTRON_BACKUP_SUFFIX: &str = ".electron-backup";
const NATIVE_TRANSITION_MARKER_SUFFIX: &str = ".native-transition.json";
const GITHUB_CLI_SUCCESS_CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeTransitionMarker {
    schema_version: u32,
    initialized_from_seed: bool,
    backup_file: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RunMode {
    NativeUi,
    HeadlessStub,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ReadOnlyBoardBootstrap {
    pub board: BoardSnapshot,
    pub labels: BoardLabels,
    pub catalog: MessageCatalog,
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
                app_data_dir: std::env::var(KANVIBE_APP_DATA_DIR_ENV)
                    .ok()
                    .map(PathBuf::from),
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
            ensure_native_transition_backup(&self.database_path)?;
            migrate_electron_database(&self.database_path)?;
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

        validate_sqlite_database(&bundled_seed)?;
        let initialized = create_sqlite_backup_once(&bundled_seed, &self.database_path)?;
        if initialized {
            write_native_transition_marker(
                &self.database_path,
                NativeTransitionMarker {
                    schema_version: 1,
                    initialized_from_seed: true,
                    backup_file: None,
                },
            )?;
        } else {
            ensure_native_transition_backup(&self.database_path)?;
        }
        migrate_electron_database(&self.database_path)?;
        Ok(self.database_path.clone())
    }
}

pub fn database_transition_backup_path(database_path: impl AsRef<Path>) -> PathBuf {
    database_sidecar_path(database_path.as_ref(), ELECTRON_BACKUP_SUFFIX)
}

pub fn database_transition_marker_path(database_path: impl AsRef<Path>) -> PathBuf {
    database_sidecar_path(database_path.as_ref(), NATIVE_TRANSITION_MARKER_SUFFIX)
}

pub fn rollback_native_database_to_electron_backup(
    database_path: impl AsRef<Path>,
) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    let database_path = database_path.as_ref();
    let backup_path = database_transition_backup_path(database_path);
    if !backup_path.exists() {
        return Err(format!(
            "Electron rollback backup does not exist at `{}`",
            backup_path.display()
        )
        .into());
    }
    restore_sqlite_database_from_backup(database_path, backup_path)
}

fn database_sidecar_path(database_path: &Path, suffix: &str) -> PathBuf {
    let file_name = database_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(RUNTIME_DATABASE_FILE_NAME);
    database_path.with_file_name(format!("{file_name}{suffix}"))
}

fn ensure_native_transition_backup(
    database_path: &Path,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let marker_path = database_transition_marker_path(database_path);
    if marker_path.exists() {
        let content = std::fs::read_to_string(&marker_path)?;
        let marker: NativeTransitionMarker = serde_json::from_str(&content)?;
        if marker.schema_version != 1 {
            return Err(format!(
                "unsupported native transition marker version {}",
                marker.schema_version
            )
            .into());
        }
        validate_sqlite_database(database_path)?;
        if let Some(backup_file) = marker.backup_file {
            if Path::new(&backup_file).components().count() != 1 {
                return Err("native transition marker contains an invalid backup path".into());
            }
            let backup_path = marker_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(backup_file);
            validate_sqlite_database(backup_path)?;
        }
        return Ok(());
    }

    let backup_path = database_transition_backup_path(database_path);
    create_sqlite_backup_once(database_path, &backup_path)?;
    write_native_transition_marker(
        database_path,
        NativeTransitionMarker {
            schema_version: 1,
            initialized_from_seed: false,
            backup_file: backup_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
        },
    )
}

fn write_native_transition_marker(
    database_path: &Path,
    marker: NativeTransitionMarker,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let marker_path = database_transition_marker_path(database_path);
    let marker_content = format!("{}\n", serde_json::to_string_pretty(&marker)?);
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let temporary_path = marker_path.with_file_name(format!(
        ".{}.{}-{unique}.tmp",
        marker_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("native-transition"),
        std::process::id()
    ));
    std::fs::write(&temporary_path, marker_content)?;
    std::fs::rename(temporary_path, marker_path)?;
    Ok(())
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
        return Some(
            home_dir
                .join("Library/Application Support")
                .join(PRODUCT_NAME),
        );
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
    pub messages: BTreeMap<String, String>,
    pub projects: Vec<NativeUiProjectSpec>,
    pub columns: Vec<NativeUiColumnSpec>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeUiProjectSpec {
    pub id: String,
    pub name: String,
    pub repo_path: String,
    pub default_branch: String,
    pub ssh_host: Option<String>,
    pub is_worktree: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeTaskFormInput {
    pub branch_name: String,
    pub description: String,
    pub base_branch: String,
    pub session_type: SessionType,
    pub project_id: String,
    pub priority: Option<TaskPriority>,
}

impl Default for NativeTaskFormInput {
    fn default() -> Self {
        Self {
            branch_name: String::new(),
            description: String::new(),
            base_branch: String::new(),
            session_type: SessionType::Tmux,
            project_id: String::new(),
            priority: None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum NativeRoute {
    Board { locale: Locale },
    Settings { locale: Locale },
    PaneLayout { locale: Locale },
    TaskDetail { locale: Locale, task_id: String },
    Diff { locale: Locale, task_id: String },
    NotFound { locale: Locale, path: String },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NativeVimCommand {
    Move(TaskStatus),
}

pub fn parse_native_vim_command(command: &str) -> Result<NativeVimCommand, String> {
    let normalized = command.trim().trim_start_matches(':').trim();
    let mut parts = normalized.split_whitespace();
    match (parts.next(), parts.next(), parts.next()) {
        (Some("move"), Some(status), None) => TaskStatus::parse(status)
            .map(NativeVimCommand::Move)
            .map_err(|_| format!("Unknown task status: {status}")),
        _ => Err("Use :move <todo|progress|review|done|cancelled>.".to_owned()),
    }
}

pub fn complete_native_vim_command(command: &str) -> String {
    let normalized = command.trim().trim_start_matches(':').trim_start();
    if normalized.is_empty() || "move".starts_with(normalized) {
        return ":move ".to_owned();
    }
    let Some(status_prefix) = normalized.strip_prefix("move ") else {
        return command.to_owned();
    };
    let status_prefix = status_prefix.trim();
    TaskStatus::ALL
        .into_iter()
        .map(TaskStatus::as_str)
        .find(|status| status.starts_with(status_prefix))
        .map_or_else(|| command.to_owned(), |status| format!(":move {status}"))
}

impl NativeRoute {
    pub const fn locale(&self) -> Locale {
        match self {
            Self::Board { locale }
            | Self::Settings { locale }
            | Self::PaneLayout { locale }
            | Self::TaskDetail { locale, .. }
            | Self::Diff { locale, .. }
            | Self::NotFound { locale, .. } => *locale,
        }
    }

    pub fn path(&self) -> String {
        let locale = self.locale().code();
        match self {
            Self::Board { .. } => format!("/{locale}"),
            Self::Settings { .. } => format!("/{locale}/settings"),
            Self::PaneLayout { .. } => format!("/{locale}/pane-layout"),
            Self::TaskDetail { task_id, .. } => format!("/{locale}/task/{task_id}"),
            Self::Diff { task_id, .. } => format!("/{locale}/task/{task_id}/diff"),
            Self::NotFound { path, .. } => path.clone(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeNavigationHistory {
    entries: Vec<NativeRoute>,
    index: usize,
}

impl NativeNavigationHistory {
    pub fn new(initial_route: NativeRoute) -> Self {
        Self {
            entries: vec![initial_route],
            index: 0,
        }
    }

    pub fn current(&self) -> &NativeRoute {
        &self.entries[self.index]
    }

    pub fn navigate(&mut self, route: NativeRoute) {
        if route == *self.current() {
            return;
        }

        self.entries.truncate(self.index + 1);
        self.entries.push(route);
        self.index = self.entries.len() - 1;
    }

    pub const fn can_go_back(&self) -> bool {
        self.index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.index + 1 < self.entries.len()
    }

    pub fn go_back(&mut self) -> Option<&NativeRoute> {
        if !self.can_go_back() {
            return None;
        }

        self.index -= 1;
        Some(self.current())
    }

    pub fn go_forward(&mut self) -> Option<&NativeRoute> {
        if !self.can_go_forward() {
            return None;
        }

        self.index += 1;
        Some(self.current())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
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
    pub description: Option<String>,
    pub status: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub project_color: Option<String>,
    pub branch_name: Option<String>,
    pub base_branch: Option<String>,
    pub session_type: Option<String>,
    pub session_name: Option<String>,
    pub worktree_path: Option<String>,
    pub ssh_host: Option<String>,
    pub pr_url: Option<String>,
    pub priority: Option<String>,
    pub agent_type: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ShortcutPlatform {
    Mac,
    Linux,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NativeSystemAppearance {
    Light,
    Dark,
}

pub const fn native_theme_is_dark(
    preference: ThemePreference,
    system_appearance: NativeSystemAppearance,
) -> bool {
    match preference {
        ThemePreference::System => matches!(system_appearance, NativeSystemAppearance::Dark),
        ThemePreference::Dark => true,
        ThemePreference::Light => false,
    }
}

const NATIVE_RELEASE_PAGE_PREFIX: &str = "https://github.com/rookedsysc/kanvibe/releases/tag/";
const NATIVE_RELEASE_DOWNLOAD_PREFIX: &str =
    "https://github.com/rookedsysc/kanvibe/releases/download/";
#[cfg(all(target_os = "macos", feature = "native-ui"))]
const NATIVE_RELEASES_API_URL: &str =
    "https://api.github.com/repos/rookedsysc/kanvibe/releases?per_page=100";
const MAX_NATIVE_RELEASE_NOTES_BYTES: usize = 64 * 1024;
const MAX_NATIVE_RELEASE_DMG_BYTES: u64 = 512 * 1024 * 1024;
#[cfg(all(target_os = "macos", feature = "native-ui"))]
const MAX_NATIVE_RELEASE_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_SAFE_RELEASE_VERSION_PART: u64 = 9_007_199_254_740_991;
#[cfg(all(target_os = "macos", feature = "native-ui"))]
const NATIVE_RELEASE_UPDATE_CACHE_TTL: Duration = Duration::from_secs(60 * 60);
#[cfg(all(target_os = "macos", feature = "native-ui"))]
const NATIVE_RELEASE_UPDATE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeReleaseUpdate {
    pub version: String,
    pub tag_name: String,
    pub name: String,
    pub body: String,
    pub html_url: String,
    pub published_at: Option<String>,
    pub installer: Option<NativeReleaseInstaller>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeReleaseInstaller {
    pub asset_name: String,
    pub download_url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeReleaseUpdateCheck {
    pub current_version: String,
    pub release: Option<NativeReleaseUpdate>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubReleasePayload {
    #[serde(default)]
    tag_name: serde_json::Value,
    #[serde(default)]
    name: serde_json::Value,
    #[serde(default)]
    body: serde_json::Value,
    #[serde(default)]
    html_url: serde_json::Value,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: serde_json::Value,
    #[serde(default)]
    assets: Vec<GitHubReleaseAssetPayload>,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseAssetPayload {
    #[serde(default)]
    name: serde_json::Value,
    #[serde(default)]
    browser_download_url: serde_json::Value,
    #[serde(default)]
    size: serde_json::Value,
    #[serde(default)]
    state: serde_json::Value,
    #[serde(default)]
    digest: serde_json::Value,
}

fn parse_native_release_version(value: &str) -> Option<[u64; 3]> {
    let normalized = value.trim().strip_prefix('v').unwrap_or(value.trim());
    let mut parts = normalized.split('.');
    let parsed = [
        parts.next()?.parse::<u64>().ok()?,
        parts.next()?.parse::<u64>().ok()?,
        parts.next()?.parse::<u64>().ok()?,
    ];
    if parts.next().is_some()
        || parsed
            .iter()
            .any(|part| *part > MAX_SAFE_RELEASE_VERSION_PART)
    {
        return None;
    }
    Some(parsed)
}

fn release_payload_string(value: &serde_json::Value) -> &str {
    value.as_str().unwrap_or("").trim()
}

fn bounded_release_notes(value: &str) -> String {
    let mut end = value.len().min(MAX_NATIVE_RELEASE_NOTES_BYTES);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].trim().to_owned()
}

fn parse_release_sha256(value: &str) -> Option<String> {
    let digest = value.trim().strip_prefix("sha256:")?;
    (digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| digest.to_ascii_lowercase())
}

fn select_native_release_installer(
    assets: &[GitHubReleaseAssetPayload],
    tag_name: &str,
    version: &str,
) -> Option<NativeReleaseInstaller> {
    let expected_name = format!("KanVibe-{version}.dmg");
    let expected_url = format!("{NATIVE_RELEASE_DOWNLOAD_PREFIX}{tag_name}/{expected_name}");
    let mut matches = assets.iter().filter_map(|asset| {
        let name = release_payload_string(&asset.name);
        let download_url = release_payload_string(&asset.browser_download_url);
        let state = release_payload_string(&asset.state);
        let size = asset.size.as_u64()?;
        let sha256 = parse_release_sha256(release_payload_string(&asset.digest))?;
        (name == expected_name
            && download_url == expected_url
            && state == "uploaded"
            && size > 0
            && size <= MAX_NATIVE_RELEASE_DMG_BYTES)
            .then(|| NativeReleaseInstaller {
                asset_name: name.to_owned(),
                download_url: download_url.to_owned(),
                size,
                sha256,
            })
    });
    let installer = matches.next()?;
    matches.next().is_none().then_some(installer)
}

pub fn select_native_release_update(
    payload: &str,
    current_version: &str,
) -> Result<Option<NativeReleaseUpdate>, String> {
    let current = parse_native_release_version(current_version)
        .ok_or_else(|| format!("invalid current release version: {current_version}"))?;
    let releases = serde_json::from_str::<Vec<GitHubReleasePayload>>(payload)
        .map_err(|error| format!("invalid GitHub releases response: {error}"))?;
    let mut selected = None::<([u64; 3], NativeReleaseUpdate)>;

    for release in releases {
        if release.draft || release.prerelease {
            continue;
        }
        let tag_name = release_payload_string(&release.tag_name);
        let Some(version) = parse_native_release_version(tag_name) else {
            continue;
        };
        let html_url = release_payload_string(&release.html_url);
        let expected_html_url = format!("{NATIVE_RELEASE_PAGE_PREFIX}{tag_name}");
        if version <= current || html_url != expected_html_url {
            continue;
        }
        let version_label = tag_name.strip_prefix('v').unwrap_or(tag_name).to_owned();
        let name = release_payload_string(&release.name);
        let installer = select_native_release_installer(&release.assets, tag_name, &version_label);
        let update = NativeReleaseUpdate {
            version: version_label,
            tag_name: tag_name.to_owned(),
            name: if name.is_empty() {
                tag_name.to_owned()
            } else {
                name.to_owned()
            },
            body: bounded_release_notes(release_payload_string(&release.body)),
            html_url: html_url.to_owned(),
            published_at: {
                let value = release_payload_string(&release.published_at);
                (!value.is_empty()).then(|| value.to_owned())
            },
            installer,
        };
        if selected
            .as_ref()
            .is_none_or(|(selected_version, _)| version > *selected_version)
        {
            selected = Some((version, update));
        }
    }

    Ok(selected.map(|(_, update)| update))
}

#[cfg(all(target_os = "macos", feature = "native-ui"))]
struct NativeReleaseUpdateCache {
    checked_at: Instant,
    result: NativeReleaseUpdateCheck,
}

pub struct NativeReleaseUpdateService {
    #[cfg(all(target_os = "macos", feature = "native-ui"))]
    agent: ureq::Agent,
    #[cfg(all(target_os = "macos", feature = "native-ui"))]
    cache: Mutex<Option<NativeReleaseUpdateCache>>,
    shown_versions: Mutex<BTreeSet<String>>,
}

static NATIVE_RELEASE_UPDATE_SERVICE: OnceLock<NativeReleaseUpdateService> = OnceLock::new();

pub fn native_release_update_service() -> &'static NativeReleaseUpdateService {
    NATIVE_RELEASE_UPDATE_SERVICE.get_or_init(NativeReleaseUpdateService::new)
}

impl NativeReleaseUpdateService {
    pub fn new() -> Self {
        #[cfg(all(target_os = "macos", feature = "native-ui"))]
        let config = ureq::Agent::config_builder()
            .https_only(true)
            .timeout_global(Some(NATIVE_RELEASE_UPDATE_TIMEOUT))
            .user_agent("KanVibe")
            .build();
        Self {
            #[cfg(all(target_os = "macos", feature = "native-ui"))]
            agent: config.into(),
            #[cfg(all(target_os = "macos", feature = "native-ui"))]
            cache: Mutex::new(None),
            shown_versions: Mutex::new(BTreeSet::new()),
        }
    }

    pub fn check(
        &self,
        current_version: &str,
        dismissed_versions: &[String],
    ) -> NativeReleaseUpdateCheck {
        #[cfg(any(not(target_os = "macos"), not(feature = "native-ui")))]
        {
            let _ = dismissed_versions;
            NativeReleaseUpdateCheck {
                current_version: current_version.to_owned(),
                release: None,
                error: Some("native release checks require the macOS product UI".to_owned()),
            }
        }
        #[cfg(all(target_os = "macos", feature = "native-ui"))]
        {
            let cached = self
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_ref()
                .filter(|cache| cache.checked_at.elapsed() < NATIVE_RELEASE_UPDATE_CACHE_TTL)
                .map(|cache| cache.result.clone());
            let result = cached.unwrap_or_else(|| self.fetch(current_version));
            self.hide_unavailable_release(result, dismissed_versions)
        }
    }

    pub fn claim(&self, version: &str) -> bool {
        let version = version.trim();
        !version.is_empty()
            && self
                .shown_versions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(version.to_owned())
    }

    #[cfg(all(target_os = "macos", feature = "native-ui"))]
    fn fetch(&self, current_version: &str) -> NativeReleaseUpdateCheck {
        let result = (|| -> Result<Option<NativeReleaseUpdate>, String> {
            let mut response = self
                .agent
                .get(NATIVE_RELEASES_API_URL)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2026-03-10")
                .call()
                .map_err(|error| format!("GitHub releases request failed: {error}"))?;
            let payload = response
                .body_mut()
                .with_config()
                .limit(MAX_NATIVE_RELEASE_RESPONSE_BYTES)
                .read_to_string()
                .map_err(|error| format!("GitHub releases response failed: {error}"))?;
            select_native_release_update(&payload, current_version)
        })();
        let check = match result {
            Ok(release) => NativeReleaseUpdateCheck {
                current_version: current_version.to_owned(),
                release,
                error: None,
            },
            Err(error) => NativeReleaseUpdateCheck {
                current_version: current_version.to_owned(),
                release: None,
                error: Some(error),
            },
        };
        if check.error.is_none() {
            *self
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                Some(NativeReleaseUpdateCache {
                    checked_at: Instant::now(),
                    result: check.clone(),
                });
        }
        check
    }

    #[cfg(all(target_os = "macos", feature = "native-ui"))]
    fn hide_unavailable_release(
        &self,
        mut result: NativeReleaseUpdateCheck,
        dismissed_versions: &[String],
    ) -> NativeReleaseUpdateCheck {
        if result.release.as_ref().is_some_and(|release| {
            dismissed_versions
                .iter()
                .any(|version| version == &release.version)
                || self
                    .shown_versions
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .contains(&release.version)
        }) {
            result.release = None;
        }
        result
    }
}

impl Default for NativeReleaseUpdateService {
    fn default() -> Self {
        Self::new()
    }
}

pub fn dismiss_native_release_update(
    database_path: impl AsRef<Path>,
    version: &str,
) -> Result<(), String> {
    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    database
        .dismiss_release_update_version(version)
        .map_err(|error| error.to_string())
}

pub fn native_gpui_keybinding(
    shortcut: &str,
    platform: ShortcutPlatform,
) -> Result<String, String> {
    let mut command = false;
    let mut control = false;
    let mut alt = false;
    let mut shift = false;
    let mut key = None::<String>;

    for token in shortcut
        .split('+')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        match token.to_ascii_lowercase().as_str() {
            "mod" => match platform {
                ShortcutPlatform::Mac => command = true,
                ShortcutPlatform::Linux => control = true,
            },
            "meta" | "cmd" | "command" => command = true,
            "ctrl" | "control" => control = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            token => {
                if key.is_some() {
                    return Err("Task search shortcut must contain exactly one key.".to_owned());
                }
                let normalized = match token {
                    "esc" | "escape" => "escape".to_owned(),
                    "space" => "space".to_owned(),
                    "arrowup" | "up" => "up".to_owned(),
                    "arrowdown" | "down" => "down".to_owned(),
                    "arrowleft" | "left" => "left".to_owned(),
                    "arrowright" | "right" => "right".to_owned(),
                    _ if token.chars().count() == 1 => token.to_owned(),
                    _ => {
                        return Err(format!(
                            "Task search shortcut has an unsupported key: {token}"
                        ));
                    }
                };
                key = Some(normalized);
            }
        }
    }

    if !command && !control && !alt && !shift {
        return Err("Task search shortcut must include a modifier.".to_owned());
    }
    let Some(key) = key else {
        return Err("Task search shortcut must include a non-modifier key.".to_owned());
    };
    let mut parts = Vec::with_capacity(5);
    if command {
        parts.push("cmd");
    }
    if control {
        parts.push("ctrl");
    }
    if alt {
        parts.push("alt");
    }
    if shift {
        parts.push("shift");
    }
    parts.push(&key);
    Ok(parts.join("-"))
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeShortcutCapture {
    pub platform: bool,
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub function: bool,
    pub key: String,
}

pub fn native_shortcut_from_capture(capture: NativeShortcutCapture) -> Result<String, String> {
    if capture.function {
        return Err("Fn-modified shortcuts are not supported.".to_owned());
    }
    if !capture.platform && !capture.control && !capture.alt {
        return Err("Task search shortcut must include Command, Control, or Option.".to_owned());
    }

    let key = capture.key.trim().to_ascii_lowercase();
    let normalized_key = match key.as_str() {
        "" | "shift" | "control" | "ctrl" | "alt" | "option" | "command" | "cmd" | "meta"
        | "fn" | "function" => {
            return Err("Task search shortcut must include a non-modifier key.".to_owned());
        }
        "esc" | "escape" => "Escape".to_owned(),
        "space" => "Space".to_owned(),
        "arrowup" | "up" => "Up".to_owned(),
        "arrowdown" | "down" => "Down".to_owned(),
        "arrowleft" | "left" => "Left".to_owned(),
        "arrowright" | "right" => "Right".to_owned(),
        _ if key.chars().count() == 1 => key.to_uppercase(),
        _ => {
            return Err(format!(
                "Task search shortcut has an unsupported key: {key}"
            ));
        }
    };

    let mut parts = Vec::with_capacity(5);
    if capture.platform {
        parts.push("Mod");
    }
    if capture.control {
        parts.push("Ctrl");
    }
    if capture.alt {
        parts.push("Alt");
    }
    if capture.shift {
        parts.push("Shift");
    }
    parts.push(&normalized_key);
    Ok(parts.join("+"))
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

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeDiffFileSnapshot {
    pub path: String,
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub is_binary: bool,
    pub original: String,
    pub current: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeDiffSnapshot {
    pub task_id: String,
    pub base_branch: String,
    pub branch_name: String,
    pub files: Vec<NativeDiffFileSnapshot>,
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
    pub sidebar_hint_dismissed: bool,
    pub done_alert_dismissed: bool,
    pub notification_enabled: bool,
    pub notification_statuses: Vec<String>,
    pub background_sync: BackgroundSyncSettings,
    pub release_update_dismissed_versions: Vec<String>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct NativeSettingsPatch {
    pub theme_preference: Option<ThemePreference>,
    pub default_session_type: Option<SessionType>,
    pub task_search_shortcut: Option<String>,
    pub vim_mode_enabled: Option<bool>,
    pub sidebar_default_collapsed: Option<bool>,
    pub background_sync_enabled: Option<bool>,
    pub background_sync_interval_minutes: Option<u64>,
    pub notification_enabled: Option<bool>,
    pub notification_statuses: Option<Vec<String>>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeGitHubCliStatus {
    pub target: String,
    pub ssh_host: Option<String>,
    pub available: bool,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeSessionDependencyStatus {
    pub session_type: SessionType,
    pub tool_name: &'static str,
    pub target: String,
    pub ssh_host: Option<String>,
    pub available: bool,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone)]
struct NativeGitHubCliCacheEntry {
    checked_at: Instant,
    available: bool,
    blocked_reason: Option<String>,
}

static NATIVE_GITHUB_CLI_CACHE: OnceLock<Mutex<BTreeMap<String, NativeGitHubCliCacheEntry>>> =
    OnceLock::new();
static NATIVE_SESSION_DEPENDENCY_CACHE: OnceLock<
    Mutex<BTreeMap<String, NativeGitHubCliCacheEntry>>,
> = OnceLock::new();

fn native_github_cli_cache() -> &'static Mutex<BTreeMap<String, NativeGitHubCliCacheEntry>> {
    NATIVE_GITHUB_CLI_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn native_session_dependency_cache() -> &'static Mutex<BTreeMap<String, NativeGitHubCliCacheEntry>>
{
    NATIVE_SESSION_DEPENDENCY_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn native_session_dependency_cache_key(
    session_type: SessionType,
    ssh_host: Option<&str>,
) -> String {
    format!(
        "{}:{}",
        native_github_cli_cache_key(ssh_host),
        session_dependency_tool_name(session_type)
    )
}

fn native_session_dependency_status_value(
    session_type: SessionType,
    ssh_host: Option<&str>,
    available: bool,
    blocked_reason: Option<String>,
) -> NativeSessionDependencyStatus {
    NativeSessionDependencyStatus {
        session_type,
        tool_name: session_dependency_tool_name(session_type),
        target: ssh_host
            .map(|host| format!("SSH {host}"))
            .unwrap_or_else(|| "Local machine".to_owned()),
        ssh_host: ssh_host.map(ToOwned::to_owned),
        available,
        blocked_reason,
    }
}

pub fn get_native_session_dependency_status(
    database_path: impl AsRef<Path>,
    session_type: SessionType,
    ssh_host: Option<&str>,
) -> Result<NativeSessionDependencyStatus, String> {
    let cache_key = native_session_dependency_cache_key(session_type, ssh_host);
    {
        let cache = native_session_dependency_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = cache.get(&cache_key)
            && (entry.blocked_reason.is_some()
                || (entry.available
                    && entry.checked_at.elapsed()
                        < Duration::from_millis(SESSION_DEPENDENCY_SUCCESS_CACHE_MS)))
        {
            return Ok(native_session_dependency_status_value(
                session_type,
                ssh_host,
                entry.available,
                entry.blocked_reason.clone(),
            ));
        }
    }
    let available = match ssh_host {
        Some(host) => {
            RemoteGitClient::new(host, native_ssh_control_directory(database_path.as_ref())?)
                .map_err(|error| error.to_string())?
                .session_dependency_available(session_type)
                .map_err(|error| error.to_string())?
        }
        None => {
            local_session_dependency_available(session_type).map_err(|error| error.to_string())?
        }
    };
    native_session_dependency_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(
            cache_key,
            NativeGitHubCliCacheEntry {
                checked_at: Instant::now(),
                available,
                blocked_reason: None,
            },
        );
    Ok(native_session_dependency_status_value(
        session_type,
        ssh_host,
        available,
        None,
    ))
}

pub fn install_native_session_dependency(
    database_path: impl AsRef<Path>,
    session_type: SessionType,
    ssh_host: Option<&str>,
) -> Result<NativeSessionDependencyStatus, String> {
    let cache_key = native_session_dependency_cache_key(session_type, ssh_host);
    if let Some(reason) = native_session_dependency_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&cache_key)
        .and_then(|entry| entry.blocked_reason.clone())
    {
        return Ok(native_session_dependency_status_value(
            session_type,
            ssh_host,
            false,
            Some(reason),
        ));
    }
    let install_result = match ssh_host {
        Some(host) => {
            RemoteGitClient::new(host, native_ssh_control_directory(database_path.as_ref())?)
                .and_then(|client| client.install_session_dependency(session_type))
        }
        None => install_local_session_dependency(session_type),
    };
    match install_result {
        Ok(()) => {
            let status = native_session_dependency_status_value(session_type, ssh_host, true, None);
            native_session_dependency_cache()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(
                    cache_key,
                    NativeGitHubCliCacheEntry {
                        checked_at: Instant::now(),
                        available: true,
                        blocked_reason: None,
                    },
                );
            Ok(status)
        }
        Err(error)
            if ssh_host.is_some() && should_block_remote_github_cli_install(error.kind()) =>
        {
            let reason = error.to_string();
            native_session_dependency_cache()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(
                    cache_key,
                    NativeGitHubCliCacheEntry {
                        checked_at: Instant::now(),
                        available: false,
                        blocked_reason: Some(reason.clone()),
                    },
                );
            Ok(native_session_dependency_status_value(
                session_type,
                ssh_host,
                false,
                Some(reason),
            ))
        }
        Err(error) => Err(error.to_string()),
    }
}

pub fn get_native_task_session_dependency_status(
    database_path: impl AsRef<Path>,
    task_id: &str,
    session_type: SessionType,
) -> Result<NativeSessionDependencyStatus, String> {
    let database_path = database_path.as_ref();
    let database = KanvibeDb::open_read_only(database_path).map_err(|error| error.to_string())?;
    let task = database
        .task_by_id(task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Task no longer exists.".to_owned())?;
    let project_ssh_host = match task.project_id.as_deref() {
        Some(project_id) => database
            .project_by_id(project_id)
            .map_err(|error| error.to_string())?
            .and_then(|project| project.ssh_host),
        None => None,
    };
    let ssh_host = task.ssh_host.as_deref().or(project_ssh_host.as_deref());
    get_native_session_dependency_status(database_path, session_type, ssh_host)
}

pub fn install_native_task_session_dependency(
    database_path: impl AsRef<Path>,
    task_id: &str,
    session_type: SessionType,
) -> Result<NativeSessionDependencyStatus, String> {
    let database_path = database_path.as_ref();
    let status = get_native_task_session_dependency_status(database_path, task_id, session_type)?;
    install_native_session_dependency(database_path, session_type, status.ssh_host.as_deref())
}

fn native_github_cli_cache_key(ssh_host: Option<&str>) -> String {
    ssh_host
        .map(|host| format!("ssh:{host}"))
        .unwrap_or_else(|| "local".to_owned())
}

fn native_github_cli_status_value(
    ssh_host: Option<&str>,
    available: bool,
    blocked_reason: Option<String>,
) -> NativeGitHubCliStatus {
    NativeGitHubCliStatus {
        target: ssh_host
            .map(|host| format!("SSH {host}"))
            .unwrap_or_else(|| "Local machine".to_owned()),
        ssh_host: ssh_host.map(ToOwned::to_owned),
        available,
        blocked_reason,
    }
}

pub fn get_native_github_cli_status(
    database_path: impl AsRef<Path>,
    ssh_host: Option<&str>,
) -> Result<NativeGitHubCliStatus, String> {
    let cache_key = native_github_cli_cache_key(ssh_host);
    {
        let cache = native_github_cli_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = cache.get(&cache_key)
            && (entry.blocked_reason.is_some()
                || (entry.available && entry.checked_at.elapsed() < GITHUB_CLI_SUCCESS_CACHE_TTL))
        {
            return Ok(native_github_cli_status_value(
                ssh_host,
                entry.available,
                entry.blocked_reason.clone(),
            ));
        }
    }

    let available = match ssh_host {
        Some(host) => {
            RemoteGitClient::new(host, native_ssh_control_directory(database_path.as_ref())?)
                .map_err(|error| error.to_string())?
                .github_cli_available()
                .map_err(|error| error.to_string())?
        }
        None => local_github_cli_available().map_err(|error| error.to_string())?,
    };
    let status = native_github_cli_status_value(ssh_host, available, None);
    native_github_cli_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(
            cache_key,
            NativeGitHubCliCacheEntry {
                checked_at: Instant::now(),
                available,
                blocked_reason: None,
            },
        );
    Ok(status)
}

pub fn install_native_github_cli(
    database_path: impl AsRef<Path>,
    ssh_host: Option<&str>,
) -> Result<NativeGitHubCliStatus, String> {
    let cache_key = native_github_cli_cache_key(ssh_host);
    if let Some(reason) = native_github_cli_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&cache_key)
        .and_then(|entry| entry.blocked_reason.clone())
    {
        return Ok(native_github_cli_status_value(
            ssh_host,
            false,
            Some(reason),
        ));
    }

    let install_result = match ssh_host {
        Some(host) => {
            RemoteGitClient::new(host, native_ssh_control_directory(database_path.as_ref())?)
                .and_then(|client| client.install_github_cli())
        }
        None => install_local_github_cli(),
    };
    match install_result {
        Ok(()) => {
            let status = native_github_cli_status_value(ssh_host, true, None);
            native_github_cli_cache()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(
                    cache_key,
                    NativeGitHubCliCacheEntry {
                        checked_at: Instant::now(),
                        available: true,
                        blocked_reason: None,
                    },
                );
            Ok(status)
        }
        Err(error) => {
            if ssh_host.is_some() && should_block_remote_github_cli_install(error.kind()) {
                let reason = error.to_string();
                native_github_cli_cache()
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(
                        cache_key,
                        NativeGitHubCliCacheEntry {
                            checked_at: Instant::now(),
                            available: false,
                            blocked_reason: Some(reason.clone()),
                        },
                    );
                Ok(native_github_cli_status_value(
                    ssh_host,
                    false,
                    Some(reason),
                ))
            } else {
                Err(error.to_string())
            }
        }
    }
}

const fn should_block_remote_github_cli_install(kind: GitErrorKind) -> bool {
    !matches!(kind, GitErrorKind::Timeout | GitErrorKind::Transport)
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeBackgroundSyncRunResult {
    pub registered_worktrees: usize,
    pub repaired_tasks: usize,
    pub updated_pull_requests: usize,
    pub merged_pull_requests: Vec<NativeMergedPullRequest>,
    pub pulled_tasks: Vec<NativeTaskPullSync>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMergedPullRequest {
    pub task_id: String,
    pub task_title: String,
    pub branch_name: String,
    pub pr_url: String,
    pub merged_at: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTaskPullSync {
    pub task_id: String,
    pub task_title: String,
    pub branch_name: String,
    pub worktree_path: String,
    pub ssh_host: Option<String>,
    pub status: NativeTaskPullStatus,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NativeTaskPullStatus {
    Updated,
    Failed,
}

impl NativeBackgroundSyncRunResult {
    pub fn has_observable_changes(&self) -> bool {
        self.registered_worktrees > 0
            || self.repaired_tasks > 0
            || self.updated_pull_requests > 0
            || !self.merged_pull_requests.is_empty()
            || !self.pulled_tasks.is_empty()
    }

    pub fn has_board_changes(&self) -> bool {
        self.registered_worktrees > 0
            || self.repaired_tasks > 0
            || self.updated_pull_requests > 0
            || self
                .pulled_tasks
                .iter()
                .any(|pull| pull.status == NativeTaskPullStatus::Updated)
    }

    pub fn needs_review(&self) -> bool {
        self.registered_worktrees > 0
            || !self.merged_pull_requests.is_empty()
            || !self.pulled_tasks.is_empty()
            || !self.errors.is_empty()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeBackgroundSyncSnapshot {
    pub settings: BackgroundSyncSettings,
    pub worker_start_count: u64,
    pub completed_run_count: u64,
    pub last_result: Option<Result<NativeBackgroundSyncRunResult, String>>,
    pub pending_review: NativeBackgroundSyncRunResult,
}

struct NativeBackgroundSyncState {
    settings: BackgroundSyncSettings,
    generation: u64,
    run_requested: bool,
    shutdown: bool,
    completed_run_count: u64,
    last_result: Option<Result<NativeBackgroundSyncRunResult, String>>,
    pending_review: NativeBackgroundSyncRunResult,
}

pub struct NativeBackgroundSyncService {
    shared: Arc<(Mutex<NativeBackgroundSyncState>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum NativeNotificationDeliveryStatus {
    #[default]
    Delivered,
    Queued,
    Unsupported,
    PermissionDenied,
    Deduplicated,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeNotificationPublishResult {
    pub creation: NotificationCreation,
    pub delivery: NativeNotificationDeliveryStatus,
}

pub trait NativeNotificationPlatform: Send + Sync {
    fn deliver(
        &self,
        notification: &AppNotification,
        activation: NativeNotificationActivationSink,
    ) -> Result<NativeNotificationDeliveryStatus, String>;
}

struct NativeNotificationShared {
    store: NotificationStore,
    activations: Mutex<VecDeque<String>>,
    revision: Arc<AtomicU64>,
}

#[derive(Clone)]
pub struct NativeNotificationActivationSink {
    shared: Arc<NativeNotificationShared>,
}

impl NativeNotificationActivationSink {
    pub fn activate(&self, notification_id: &str) -> Result<(), String> {
        if self
            .shared
            .store
            .get(notification_id)
            .map_err(|error| error.to_string())?
            .is_none()
        {
            return Err(format!("notification not found: {notification_id}"));
        }
        self.shared
            .store
            .mark_read(notification_id)
            .map_err(|error| error.to_string())?;
        self.shared
            .activations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push_back(notification_id.to_owned());
        self.shared.revision.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

pub struct NativeNotificationService {
    shared: Arc<NativeNotificationShared>,
    platform: Arc<dyn NativeNotificationPlatform>,
}

impl NativeNotificationService {
    pub fn new(
        database_path: impl Into<PathBuf>,
        revision: Arc<AtomicU64>,
        platform: Arc<dyn NativeNotificationPlatform>,
    ) -> Self {
        Self {
            shared: Arc::new(NativeNotificationShared {
                store: NotificationStore::new(database_path),
                activations: Mutex::new(VecDeque::new()),
                revision,
            }),
            platform,
        }
    }

    pub fn publish(
        &self,
        draft: NotificationDraft,
    ) -> Result<NativeNotificationPublishResult, String> {
        let creation = self
            .shared
            .store
            .create(draft)
            .map_err(|error| error.to_string())?;
        if !creation.created {
            return Ok(NativeNotificationPublishResult {
                creation,
                delivery: NativeNotificationDeliveryStatus::Deduplicated,
            });
        }

        self.shared.revision.fetch_add(1, Ordering::AcqRel);
        let delivery = self.platform.deliver(
            &creation.notification,
            NativeNotificationActivationSink {
                shared: Arc::clone(&self.shared),
            },
        )?;
        Ok(NativeNotificationPublishResult { creation, delivery })
    }

    pub fn list(&self) -> Result<Vec<AppNotification>, String> {
        self.shared.store.list().map_err(|error| error.to_string())
    }

    pub fn mark_read(&self, notification_id: &str) -> Result<bool, String> {
        let changed = self
            .shared
            .store
            .mark_read(notification_id)
            .map_err(|error| error.to_string())?;
        if changed {
            self.shared.revision.fetch_add(1, Ordering::AcqRel);
        }
        Ok(changed)
    }

    pub fn mark_all_read(&self) -> Result<usize, String> {
        let changed = self
            .shared
            .store
            .mark_all_read()
            .map_err(|error| error.to_string())?;
        if changed > 0 {
            self.shared.revision.fetch_add(1, Ordering::AcqRel);
        }
        Ok(changed)
    }

    pub fn consume_activation(&self) -> Result<Option<AppNotification>, String> {
        let notification_id = self
            .shared
            .activations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front();
        match notification_id {
            Some(notification_id) => self
                .shared
                .store
                .get(&notification_id)
                .map_err(|error| error.to_string()),
            None => Ok(None),
        }
    }
}

#[derive(Debug, Default)]
pub struct UnsupportedNativeNotificationPlatform;

impl NativeNotificationPlatform for UnsupportedNativeNotificationPlatform {
    fn deliver(
        &self,
        _notification: &AppNotification,
        _activation: NativeNotificationActivationSink,
    ) -> Result<NativeNotificationDeliveryStatus, String> {
        Ok(NativeNotificationDeliveryStatus::Unsupported)
    }
}

pub fn native_notification_platform() -> Result<Arc<dyn NativeNotificationPlatform>, String> {
    #[cfg(target_os = "macos")]
    {
        return Ok(Arc::new(
            macos_notifications::MacOsNotificationPlatform::start()
                .map_err(|error| error.to_string())?,
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(Arc::new(UnsupportedNativeNotificationPlatform))
    }
}

impl NativeBackgroundSyncService {
    pub fn start<F>(settings: BackgroundSyncSettings, sync_once: F) -> std::io::Result<Self>
    where
        F: Fn() -> Result<NativeBackgroundSyncRunResult, String> + Send + 'static,
    {
        let shared = Arc::new((
            Mutex::new(NativeBackgroundSyncState {
                settings,
                generation: 0,
                run_requested: false,
                shutdown: false,
                completed_run_count: 0,
                last_result: None,
                pending_review: NativeBackgroundSyncRunResult::default(),
            }),
            Condvar::new(),
        ));
        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name("kanvibe-background-sync".to_owned())
            .spawn(move || run_native_background_sync_loop(worker_shared, sync_once))?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    pub fn reconfigure(&self, settings: BackgroundSyncSettings) {
        let (state, wake) = &*self.shared;
        let mut state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.settings == settings {
            return;
        }
        state.settings = settings;
        if !state.settings.is_enabled {
            state.run_requested = false;
        }
        state.generation = state.generation.wrapping_add(1);
        wake.notify_one();
    }

    pub fn trigger_now(&self) {
        let (state, wake) = &*self.shared;
        let mut state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !state.settings.is_enabled || state.shutdown {
            return;
        }
        state.run_requested = true;
        state.generation = state.generation.wrapping_add(1);
        wake.notify_one();
    }

    pub fn snapshot(&self) -> NativeBackgroundSyncSnapshot {
        let (state, _) = &*self.shared;
        let state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        NativeBackgroundSyncSnapshot {
            settings: state.settings.clone(),
            worker_start_count: 1,
            completed_run_count: state.completed_run_count,
            last_result: state.last_result.clone(),
            pending_review: state.pending_review.clone(),
        }
    }

    pub fn acknowledge_review(&self) {
        let (state, _) = &*self.shared;
        state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pending_review = NativeBackgroundSyncRunResult::default();
    }
}

impl Drop for NativeBackgroundSyncService {
    fn drop(&mut self) {
        let (state, wake) = &*self.shared;
        {
            let mut state = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.shutdown = true;
            state.generation = state.generation.wrapping_add(1);
            wake.notify_one();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_native_background_sync_loop<F>(
    shared: Arc<(Mutex<NativeBackgroundSyncState>, Condvar)>,
    sync_once: F,
) where
    F: Fn() -> Result<NativeBackgroundSyncRunResult, String>,
{
    let (state_lock, wake) = &*shared;
    loop {
        let mut state = state_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.shutdown {
            return;
        }
        if state.run_requested {
            state.run_requested = false;
        } else if !state.settings.is_enabled {
            drop(
                wake.wait(state)
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            );
            continue;
        } else {
            let generation = state.generation;
            let interval = Duration::from_millis(state.settings.interval_ms.max(1));
            let (next_state, timeout) = wake
                .wait_timeout(state, interval)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next_state;
            if state.shutdown {
                return;
            }
            if state.generation != generation || !timeout.timed_out() {
                continue;
            }
        }
        drop(state);

        let result = sync_once();
        let mut state = state_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Ok(run) = &result {
            merge_native_background_review(&mut state.pending_review, run);
        }
        state.completed_run_count = state.completed_run_count.saturating_add(1);
        state.last_result = Some(result);
    }
}

fn merge_native_background_review(
    pending: &mut NativeBackgroundSyncRunResult,
    run: &NativeBackgroundSyncRunResult,
) {
    pending.registered_worktrees = pending
        .registered_worktrees
        .saturating_add(run.registered_worktrees);
    for merged in &run.merged_pull_requests {
        if !pending.merged_pull_requests.contains(merged) {
            pending.merged_pull_requests.push(merged.clone());
        }
    }
    for pulled in &run.pulled_tasks {
        if !pending.pulled_tasks.contains(pulled) {
            pending.pulled_tasks.push(pulled.clone());
        }
    }
    for error in &run.errors {
        if !pending.errors.contains(error) {
            pending.errors.push(error.clone());
        }
    }
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

pub fn native_hook_server_port() -> u16 {
    std::env::var(HOOK_SERVER_PORT_ENV)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .unwrap_or(if cfg!(debug_assertions) {
            DEV_HOOK_SERVER_PORT
        } else {
            DEFAULT_HOOK_SERVER_PORT
        })
}

pub fn spawn_native_hook_server(
    database_path: impl Into<PathBuf>,
) -> std::io::Result<(HookHttpServer, Arc<AtomicU64>)> {
    spawn_native_hook_server_with_notifications(database_path, None, Locale::En)
}

pub fn spawn_native_hook_server_with_notifications(
    database_path: impl Into<PathBuf>,
    notifications: Option<Arc<NativeNotificationService>>,
    locale: Locale,
) -> std::io::Result<(HookHttpServer, Arc<AtomicU64>)> {
    let database_path = Arc::new(database_path.into());
    let revision = Arc::new(AtomicU64::new(0));
    let handler_revision = Arc::clone(&revision);
    let server = HookHttpServer::start(
        "0.0.0.0",
        native_hook_server_port(),
        Arc::new(move |request| {
            let route = request.route;
            let body = request.body.clone();
            let response = handle_native_hook_request(database_path.as_path(), request);
            if response.status_code == 200 {
                handler_revision.fetch_add(1, Ordering::AcqRel);
                if route == HookHttpRoute::Status
                    && let Some(notifications) = &notifications
                    && let Err(error) = publish_hook_status_notification(
                        database_path.as_path(),
                        &body,
                        notifications,
                        locale,
                    )
                {
                    eprintln!(
                        "{}",
                        native_diagnostic_line(
                            "hook-status-notification-error",
                            "notification-center",
                            &error,
                            None,
                        )
                    );
                }
            }
            response
        }),
    )?;
    Ok((server, revision))
}

fn publish_hook_status_notification(
    database_path: &Path,
    body: &str,
    notifications: &NativeNotificationService,
    locale: Locale,
) -> Result<(), String> {
    let input =
        serde_json::from_str::<NativeHookStatusInput>(body).map_err(|error| error.to_string())?;
    let task_id = input.task_id.as_deref().unwrap_or_default().trim();
    let requested_status = input
        .status
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let database = KanvibeDb::open_read_only(database_path).map_err(|error| error.to_string())?;
    if !database
        .notification_enabled()
        .map_err(|error| error.to_string())?
        || !database
            .notification_statuses()
            .map_err(|error| error.to_string())?
            .iter()
            .any(|status| status == &requested_status)
    {
        return Ok(());
    }
    let Some(task) = database
        .task_by_id(task_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let project_name = match task.project_id.as_deref() {
        Some(project_id) => database
            .project_by_id(project_id)
            .map_err(|error| error.to_string())?
            .map(|project| project.name)
            .unwrap_or_else(|| project_id.to_owned()),
        None => "KanVibe".to_owned(),
    };
    let branch_name = task.branch_name.as_deref().unwrap_or(&task.title);
    let status_body = match locale {
        Locale::Ko => format!("{}: {}로 변경", task.title, requested_status),
        Locale::En => format!("{}: changed to {}", task.title, requested_status),
        Locale::Zh => format!("{}: 已变更为{}", task.title, requested_status),
    };
    notifications.publish(NotificationDraft {
        title: format!("{project_name} — {branch_name}"),
        body: match task.description {
            Some(description) if !description.is_empty() => {
                format!("{status_body}\n{description}")
            }
            _ => status_body,
        },
        task_id: Some(task.id.clone()),
        relative_path: Some(format!("/{}/task/{}", locale.code(), task.id)),
        locale: locale.code().to_owned(),
        dedupe_key: format!("task-status:{}:{requested_status}", task.id),
        action: None,
    })?;
    Ok(())
}

pub fn handle_native_hook_request(
    database_path: impl AsRef<Path>,
    request: HookHttpRequest,
) -> HookHttpResponse {
    match request.route {
        HookHttpRoute::Start => handle_native_hook_start(database_path.as_ref(), &request.body),
        HookHttpRoute::Status => handle_native_hook_status(database_path.as_ref(), &request.body),
    }
}

fn hook_error(status_code: u16, error: impl Into<String>) -> HookHttpResponse {
    HookHttpResponse::json(
        status_code,
        serde_json::json!({ "success": false, "error": error.into() }).to_string(),
    )
}

fn handle_native_hook_start(database_path: &Path, body: &str) -> HookHttpResponse {
    let input = match serde_json::from_str::<NativeHookStartInput>(body) {
        Ok(input) => input,
        Err(_) => return hook_error(400, "Invalid JSON body."),
    };
    let title = input.title.as_deref().unwrap_or_default().trim();
    if title.is_empty() {
        return hook_error(400, "title은 필수입니다.");
    }
    let optional_trimmed = |value: Option<String>| value.filter(|value| !value.trim().is_empty());
    let branch_name = optional_trimmed(input.branch_name);
    let agent_type = optional_trimmed(input.agent_type);
    let mut ssh_host = optional_trimmed(input.ssh_host);
    let project_id = optional_trimmed(input.project_id);
    let mut base_branch = optional_trimmed(input.base_branch);
    let database = match KanvibeDb::open_read_write(database_path) {
        Ok(database) => database,
        Err(error) => return hook_error(500, error.to_string()),
    };
    let project = match project_id.as_deref() {
        Some(project_id) => match database.project_by_id(project_id) {
            Ok(project) => project,
            Err(error) => return hook_error(500, error.to_string()),
        },
        None => None,
    };
    let mut worktree_path = None;
    let mut session_name = None;

    if let (Some(branch_name), Some(session_type), Some(project)) =
        (branch_name.as_deref(), input.session_type, project.as_ref())
        && project.ssh_host.is_none()
    {
        let selected_base = base_branch
            .clone()
            .unwrap_or_else(|| project.default_branch.clone());
        if let Ok(session) = kanvibe_git::create_worktree_with_session(
            &project.repo_path,
            branch_name,
            &selected_base,
            session_type,
        ) {
            base_branch = Some(selected_base);
            ssh_host.clone_from(&project.ssh_host);
            worktree_path = Some(session.worktree_path.to_string_lossy().into_owned());
            session_name = Some(session.session_name);
        }
    }

    let task = match database.create_task(CreateTaskInput {
        title: Some(title.to_owned()),
        status: Some(TaskStatus::Todo),
        branch_name,
        base_branch,
        worktree_path,
        session_type: input.session_type,
        session_name,
        ssh_host,
        agent_type,
        project_id,
        ..CreateTaskInput::default()
    }) {
        Ok(task) => task,
        Err(error) => return hook_error(500, error.to_string()),
    };
    if task.ssh_host.is_none()
        && let Some(worktree_path) = task.worktree_path.as_deref()
    {
        if let Err(error) = install_local_provider_hooks(
            worktree_path,
            &task.id,
            &local_hook_server_url(native_hook_server_port()),
        ) {
            return hook_error(500, error.to_string());
        }
        if let Err(error) = persist_local_task_status(Path::new(worktree_path), task.status) {
            return hook_error(500, error);
        }
    }

    HookHttpResponse::json(
        200,
        serde_json::json!({
            "success": true,
            "data": {
                "id": task.id,
                "status": task.status,
                "sessionName": task.session_name,
            }
        })
        .to_string(),
    )
}

fn handle_native_hook_status(database_path: &Path, body: &str) -> HookHttpResponse {
    let input = match serde_json::from_str::<NativeHookStatusInput>(body) {
        Ok(input) => input,
        Err(_) => return hook_error(400, "Invalid JSON body."),
    };
    let task_id = input.task_id.as_deref().unwrap_or_default().trim();
    let requested_status = input
        .status
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    if task_id.is_empty() || requested_status.is_empty() {
        return hook_error(400, "taskId, status는 필수입니다.");
    }
    let status = match TaskStatus::parse(&requested_status) {
        Ok(status) => status,
        Err(_) => {
            return hook_error(
                400,
                format!(
                    "유효하지 않은 상태입니다: {}",
                    input.status.as_deref().unwrap_or_default()
                ),
            );
        }
    };
    let database = match KanvibeDb::open_read_write(database_path) {
        Ok(database) => database,
        Err(error) => return hook_error(500, error.to_string()),
    };
    let before = match database.task_by_id(task_id) {
        Ok(Some(task)) => task,
        Ok(None) => return hook_error(404, format!("작업을 찾을 수 없습니다: {task_id}")),
        Err(error) => return hook_error(500, error.to_string()),
    };
    let project = match before.project_id.as_deref() {
        Some(project_id) => match database.project_by_id(project_id) {
            Ok(project) => project,
            Err(error) => return hook_error(500, error.to_string()),
        },
        None => None,
    };
    let task = match database.set_task_status_preserving_resources(task_id, status) {
        Ok(Some(task)) => task,
        Ok(None) => return hook_error(404, format!("작업을 찾을 수 없습니다: {task_id}")),
        Err(error) => return hook_error(500, error.to_string()),
    };
    let project_name = project
        .as_ref()
        .map(|project| project.name.as_str())
        .or(before.project_id.as_deref())
        .unwrap_or("Unknown project");
    let state_path = task
        .worktree_path
        .as_deref()
        .or_else(|| project.as_ref().map(|project| project.repo_path.as_str()));
    let is_remote = task.ssh_host.is_some()
        || project
            .as_ref()
            .is_some_and(|project| project.ssh_host.is_some());
    if !is_remote && let Some(state_path) = state_path {
        let _ = persist_local_task_status(Path::new(state_path), status);
    }

    HookHttpResponse::json(
        200,
        serde_json::json!({
            "success": true,
            "data": {
                "id": task.id,
                "status": task.status,
                "branchName": task.branch_name,
                "projectName": project_name,
            }
        })
        .to_string(),
    )
}

pub fn load_read_only_board(
    repo_root: impl AsRef<Path>,
    database_path: impl AsRef<Path>,
    locale: Locale,
) -> Result<ReadOnlyBoardBootstrap, Box<dyn Error + Send + Sync>> {
    load_read_only_board_with_done_limit(repo_root, database_path, locale, DONE_PAGE_SIZE)
}

pub fn load_read_only_board_with_done_limit(
    repo_root: impl AsRef<Path>,
    database_path: impl AsRef<Path>,
    locale: Locale,
    done_limit: u32,
) -> Result<ReadOnlyBoardBootstrap, Box<dyn Error + Send + Sync>> {
    let database = KanvibeDb::open_read_only(database_path)?;
    let board = database.board_snapshot(done_limit)?;
    let labels = load_board_labels(&repo_root, locale)?;
    let catalog = load_message_catalog(&repo_root, locale)?;

    Ok(ReadOnlyBoardBootstrap {
        board,
        labels,
        catalog,
    })
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
    let projects_by_id = bootstrap
        .board
        .projects
        .iter()
        .map(|project| (project.id.as_str(), project))
        .collect::<BTreeMap<_, _>>();
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
                    .map(|task| {
                        let project = task
                            .project_id
                            .as_deref()
                            .and_then(|project_id| projects_by_id.get(project_id).copied());

                        NativeUiCardSpec {
                            id: task.id.clone(),
                            title: task.title.clone(),
                            description: task.description.clone(),
                            status: task.status.as_str().to_owned(),
                            project_id: task.project_id.clone(),
                            project_name: project.map(|project| project.name.clone()),
                            project_color: project.and_then(|project| project.color.clone()),
                            branch_name: task.branch_name.clone(),
                            base_branch: task.base_branch.clone(),
                            session_type: task
                                .session_type
                                .map(|session_type| session_type.as_str().to_owned()),
                            session_name: task.session_name.clone(),
                            worktree_path: task.worktree_path.clone(),
                            ssh_host: task.ssh_host.clone(),
                            pr_url: task.pr_url.clone(),
                            priority: task.priority.map(|priority| priority.as_str().to_owned()),
                            agent_type: task.agent_type.clone(),
                        }
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
        messages: bootstrap.catalog.strings.clone(),
        projects: bootstrap
            .board
            .projects
            .iter()
            .map(|project| NativeUiProjectSpec {
                id: project.id.clone(),
                name: project.name.clone(),
                repo_path: project.repo_path.clone(),
                default_branch: project.default_branch.clone(),
                ssh_host: project.ssh_host.clone(),
                is_worktree: project.is_worktree,
            })
            .collect(),
        columns,
    }
}

pub fn build_unavailable_native_ui_render_spec(locale: Locale) -> NativeUiRenderSpec {
    let (primary_action_label, all_projects_label) = match locale {
        Locale::Ko => ("+ 새 작업", "모든 프로젝트"),
        Locale::En => ("+ New Task", "All Projects"),
        Locale::Zh => ("+ 新任务", "所有项目"),
    };

    NativeUiRenderSpec {
        window_title: "KanVibe".to_owned(),
        route: localize_href("/", Some(locale.code())),
        locale,
        primary_action_label: primary_action_label.to_owned(),
        all_projects_label: all_projects_label.to_owned(),
        project_count: 0,
        total_visible_tasks: 0,
        done_total: 0,
        brand_primary: PRIMARY,
        neutral_button_surface: NEUTRAL_BUTTON_SURFACE,
        messages: BTreeMap::new(),
        projects: Vec::new(),
        columns: TaskStatus::ALL
            .into_iter()
            .map(|status| NativeUiColumnSpec {
                status,
                label: status.as_str().to_owned(),
                color: status_color(status),
                task_count: 0,
                first_card_title: None,
                cards: Vec::new(),
            })
            .collect(),
    }
}

pub fn filter_native_cards<'a>(
    cards: impl IntoIterator<Item = &'a NativeUiCardSpec>,
    query: &str,
    project_id: Option<&str>,
) -> Vec<&'a NativeUiCardSpec> {
    let query = query.trim().to_lowercase();

    cards
        .into_iter()
        .filter(|card| {
            project_id.is_none_or(|project_id| card.project_id.as_deref() == Some(project_id))
                && (query.is_empty() || card.title.to_lowercase().contains(&query))
        })
        .collect()
}

pub fn filter_native_cards_by_projects<'a>(
    cards: impl IntoIterator<Item = &'a NativeUiCardSpec>,
    query: &str,
    project_ids: &BTreeSet<String>,
) -> Vec<&'a NativeUiCardSpec> {
    let query = query.trim().to_lowercase();

    cards
        .into_iter()
        .filter(|card| {
            (project_ids.is_empty()
                || card
                    .project_id
                    .as_ref()
                    .is_some_and(|project_id| project_ids.contains(project_id)))
                && (query.is_empty() || card.title.to_lowercase().contains(&query))
        })
        .collect()
}

pub fn ordered_task_ids_for_drop<'a>(
    task_ids: impl IntoIterator<Item = &'a str>,
    dragged_task_id: &str,
    before_task_id: &str,
) -> Vec<String> {
    let mut ordered = task_ids
        .into_iter()
        .filter(|task_id| *task_id != dragged_task_id)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let insertion_index = ordered
        .iter()
        .position(|task_id| task_id == before_task_id)
        .unwrap_or(ordered.len());
    ordered.insert(insertion_index, dragged_task_id.to_owned());
    ordered
}

pub fn search_native_cards<'a>(
    cards: impl IntoIterator<Item = &'a NativeUiCardSpec>,
    query: &str,
) -> Vec<&'a NativeUiCardSpec> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return cards.into_iter().collect();
    }

    cards
        .into_iter()
        .filter(|card| {
            [
                Some(card.title.as_str()),
                card.description.as_deref(),
                card.project_name.as_deref(),
                card.branch_name.as_deref(),
                card.base_branch.as_deref(),
                card.ssh_host.as_deref(),
                card.agent_type.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|value| value.to_lowercase().contains(&query))
        })
        .collect()
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

pub fn parse_native_route(path: &str) -> NativeRoute {
    let normalized_path = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    };
    let segments = normalized_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let Some(locale) = segments.first().and_then(|segment| match *segment {
        "ko" => Some(Locale::Ko),
        "en" => Some(Locale::En),
        "zh" => Some(Locale::Zh),
        _ => None,
    }) else {
        return NativeRoute::Board {
            locale: DEFAULT_LOCALE,
        };
    };

    match segments.as_slice() {
        [_] => NativeRoute::Board { locale },
        [_, "settings"] => NativeRoute::Settings { locale },
        [_, "pane-layout"] => NativeRoute::PaneLayout { locale },
        [_, "task", task_id] => NativeRoute::TaskDetail {
            locale,
            task_id: (*task_id).to_owned(),
        },
        [_, "task", task_id, "diff"] => NativeRoute::Diff {
            locale,
            task_id: (*task_id).to_owned(),
        },
        _ => NativeRoute::NotFound {
            locale,
            path: normalized_path,
        },
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
        .find(|segment| !segment.is_empty())
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
        sidebar_hint_dismissed: database.sidebar_hint_dismissed()?,
        done_alert_dismissed: database.done_alert_dismissed()?,
        notification_enabled: database.notification_enabled()?,
        notification_statuses: database.notification_statuses()?,
        background_sync: database.background_sync_settings()?,
        release_update_dismissed_versions: database.release_update_dismissed_versions()?,
    })
}

pub fn dismiss_native_sidebar_hint(database_path: impl AsRef<Path>) -> Result<(), String> {
    KanvibeDb::open_read_write(database_path)
        .map_err(|error| error.to_string())?
        .dismiss_sidebar_hint()
        .map_err(|error| error.to_string())
}

pub fn update_native_settings(
    database_path: impl AsRef<Path>,
    locale: Locale,
    patch: NativeSettingsPatch,
) -> Result<SettingsShell, String> {
    if let Some(minutes) = patch.background_sync_interval_minutes
        && !(1..=1_440).contains(&minutes)
    {
        return Err("Background sync interval must be between 1 and 1440 minutes.".to_owned());
    }
    if let Some(shortcut) = patch.task_search_shortcut.as_deref() {
        native_gpui_keybinding(shortcut, ShortcutPlatform::Mac)?;
    }
    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    if let Some(theme) = patch.theme_preference {
        database
            .set_theme_preference(theme)
            .map_err(|error| error.to_string())?;
    }
    if let Some(session_type) = patch.default_session_type {
        database
            .set_default_session_type(session_type)
            .map_err(|error| error.to_string())?;
    }
    if let Some(shortcut) = patch.task_search_shortcut {
        database
            .set_task_search_shortcut(&shortcut)
            .map_err(|error| error.to_string())?;
    }
    if let Some(enabled) = patch.vim_mode_enabled {
        database
            .set_vim_mode_enabled(enabled)
            .map_err(|error| error.to_string())?;
    }
    if let Some(collapsed) = patch.sidebar_default_collapsed {
        database
            .set_sidebar_default_collapsed(collapsed)
            .map_err(|error| error.to_string())?;
    }
    if let Some(enabled) = patch.background_sync_enabled {
        database
            .set_background_sync_enabled(enabled)
            .map_err(|error| error.to_string())?;
    }
    if let Some(minutes) = patch.background_sync_interval_minutes {
        database
            .set_background_sync_interval_ms(minutes * 60_000)
            .map_err(|error| error.to_string())?;
    }
    if let Some(enabled) = patch.notification_enabled {
        database
            .set_notification_enabled(enabled)
            .map_err(|error| error.to_string())?;
    }
    if let Some(statuses) = patch.notification_statuses {
        database
            .set_notification_statuses(&statuses)
            .map_err(|error| error.to_string())?;
    }
    build_settings_shell(&database, Some(locale.code())).map_err(|error| error.to_string())
}

pub fn cycle_native_pane_layout(
    database_path: impl AsRef<Path>,
    layout_id: &str,
) -> Result<Vec<PaneLayoutConfig>, String> {
    let database_path = database_path.as_ref();
    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    let layout = database
        .pane_layout_by_id(layout_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Pane layout no longer exists.".to_owned())?;
    let next = match layout.layout_type {
        PaneLayoutType::Single => PaneLayoutType::Horizontal2,
        PaneLayoutType::Horizontal2 => PaneLayoutType::Vertical2,
        PaneLayoutType::Vertical2 => PaneLayoutType::LeftRightTb,
        PaneLayoutType::LeftRightTb => PaneLayoutType::LeftTbRight,
        PaneLayoutType::LeftTbRight => PaneLayoutType::Quad,
        PaneLayoutType::Quad => PaneLayoutType::Single,
    };
    drop(database);
    save_native_pane_layout_type(database_path, Some(layout_id), None, layout.is_global, next)
}

pub fn save_native_pane_layout_type(
    database_path: impl AsRef<Path>,
    layout_id: Option<&str>,
    project_id: Option<&str>,
    is_global: bool,
    layout_type: PaneLayoutType,
) -> Result<Vec<PaneLayoutConfig>, String> {
    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    let existing = layout_id
        .map(|layout_id| {
            database
                .pane_layout_by_id(layout_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "Pane layout no longer exists.".to_owned())
        })
        .transpose()?;
    let (project_id, is_global, mut panes) = if let Some(layout) = existing {
        (layout.project_id, layout.is_global, layout.panes)
    } else if is_global {
        if project_id.is_some() {
            return Err("The global pane layout cannot target a project.".to_owned());
        }
        (None, true, Vec::new())
    } else {
        let project_id = project_id
            .filter(|project_id| !project_id.trim().is_empty())
            .ok_or_else(|| "Project is required for a pane layout override.".to_owned())?;
        database
            .project_by_id(project_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Pane layout project no longer exists.".to_owned())?;
        (Some(project_id.to_owned()), false, Vec::new())
    };
    panes.resize_with(layout_type.pane_count(), || kanvibe_core::PaneCommand {
        position: 0,
        command: String::new(),
    });
    panes.truncate(layout_type.pane_count());
    for (position, pane) in panes.iter_mut().enumerate() {
        pane.position = position as u32;
    }
    database
        .save_pane_layout(SavePaneLayoutInput {
            layout_type,
            panes,
            project_id,
            is_global,
        })
        .map_err(|error| error.to_string())?;
    database
        .get_all_pane_layouts()
        .map_err(|error| error.to_string())
}

pub fn reset_native_project_pane_layout(
    database_path: impl AsRef<Path>,
    layout_id: &str,
) -> Result<Vec<PaneLayoutConfig>, String> {
    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    let layout = database
        .pane_layout_by_id(layout_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Pane layout no longer exists.".to_owned())?;
    if layout.is_global {
        return Err("The global pane layout cannot be reset to another layout.".to_owned());
    }
    database
        .delete_pane_layout(layout_id)
        .map_err(|error| error.to_string())?;
    database
        .get_all_pane_layouts()
        .map_err(|error| error.to_string())
}

pub fn update_native_pane_command(
    database_path: impl AsRef<Path>,
    layout_id: &str,
    position: u32,
    command: &str,
) -> Result<Vec<PaneLayoutConfig>, String> {
    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    let mut layout = database
        .pane_layout_by_id(layout_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Pane layout no longer exists.".to_owned())?;
    let pane = layout
        .panes
        .iter_mut()
        .find(|pane| pane.position == position)
        .ok_or_else(|| format!("Pane position {position} does not exist in this layout."))?;
    pane.command = command.trim().to_owned();
    database
        .save_pane_layout(SavePaneLayoutInput {
            layout_type: layout.layout_type,
            panes: layout.panes,
            project_id: layout.project_id,
            is_global: layout.is_global,
        })
        .map_err(|error| error.to_string())?;
    database
        .get_all_pane_layouts()
        .map_err(|error| error.to_string())
}

pub fn register_native_local_project(
    database_path: impl AsRef<Path>,
    preferred_name: &str,
    repo_path: impl AsRef<Path>,
) -> Result<Project, String> {
    let preferred_name = preferred_name.trim();
    if preferred_name.is_empty() {
        return Err("Project name is required.".to_owned());
    }
    let repo_root = common_repository_root(repo_path).map_err(|error| error.to_string())?;
    let repo_root_string = repo_root.to_string_lossy().into_owned();
    let branch = default_branch(&repo_root).map_err(|error| error.to_string())?;
    let mut database =
        KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    let projects = database.projects().map_err(|error| error.to_string())?;
    if projects
        .iter()
        .any(|project| project.ssh_host.is_none() && project.repo_path == repo_root_string)
    {
        return Err("This repository is already registered.".to_owned());
    }
    let existing_names = projects
        .iter()
        .map(|project| project.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut name = preferred_name.to_owned();
    let mut suffix = 2;
    while existing_names.contains(name.as_str()) {
        name = format!("{preferred_name}-{suffix}");
        suffix += 1;
    }
    let color = compute_project_color(&name);
    let project = database
        .register_project(&name, &repo_root_string, &branch, None, Some(color))
        .map_err(|error| error.to_string())?;
    let session_type = database
        .default_session_type()
        .map_err(|error| error.to_string())?;
    let session_name = format_session_name(&project.name, &project.default_branch);
    if let Err(error) = database.create_project_root_task(&project, session_type, &session_name) {
        let _ = database.delete_project(&project.id);
        return Err(format!("failed to create project root task: {error}"));
    }
    Ok(project)
}

pub fn register_native_remote_project(
    database_path: impl AsRef<Path>,
    preferred_name: &str,
    repo_path: &str,
    ssh_host: &str,
) -> Result<Project, String> {
    let database_path = database_path.as_ref();
    let client = RemoteGitClient::new(
        ssh_host.trim(),
        native_ssh_control_directory(database_path)?,
    )
    .map_err(|error| error.to_string())?;
    if !client
        .validate_repo(repo_path.trim())
        .map_err(|error| error.to_string())?
    {
        return Err("Remote path is not a Git worktree.".to_owned());
    }
    let default_branch = client
        .default_branch(repo_path.trim())
        .map_err(|error| error.to_string())?;
    persist_native_remote_project(
        database_path,
        preferred_name,
        repo_path,
        &default_branch,
        ssh_host,
    )
}

fn persist_native_remote_project(
    database_path: impl AsRef<Path>,
    preferred_name: &str,
    repo_path: &str,
    default_branch: &str,
    ssh_host: &str,
) -> Result<Project, String> {
    let preferred_name = preferred_name.trim();
    let repo_path = repo_path.trim();
    let default_branch = default_branch.trim();
    let ssh_host = ssh_host.trim();
    if preferred_name.is_empty()
        || repo_path.is_empty()
        || default_branch.is_empty()
        || ssh_host.is_empty()
    {
        return Err(
            "Remote project name, path, default branch, and SSH host are required.".to_owned(),
        );
    }
    let mut database =
        KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    let projects = database.projects().map_err(|error| error.to_string())?;
    if projects.iter().any(|project| {
        project.ssh_host.as_deref() == Some(ssh_host) && project.repo_path == repo_path
    }) {
        return Err("This remote repository is already registered.".to_owned());
    }
    let existing_names = projects
        .iter()
        .map(|project| project.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut name = preferred_name.to_owned();
    let mut suffix = 2;
    while existing_names.contains(name.as_str()) {
        name = format!("{preferred_name}-{suffix}");
        suffix += 1;
    }
    let color = compute_project_color(&name);
    let project = database
        .register_project(
            &name,
            repo_path,
            default_branch,
            Some(ssh_host),
            Some(color),
        )
        .map_err(|error| error.to_string())?;
    let session_type = database
        .default_session_type()
        .map_err(|error| error.to_string())?;
    let session_name = format_session_name(&project.name, &project.default_branch);
    if let Err(error) = database.create_project_root_task(&project, session_type, &session_name) {
        let _ = database.delete_project(&project.id);
        return Err(format!(
            "failed to create remote project root task: {error}"
        ));
    }
    Ok(project)
}

fn native_ssh_control_directory(database_path: &Path) -> Result<PathBuf, String> {
    database_path
        .parent()
        .map(|parent| parent.join("ssh-control"))
        .ok_or_else(|| "Database path has no parent for SSH control sockets.".to_owned())
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeProjectScanResult {
    pub registered: Vec<Project>,
    pub skipped: Vec<PathBuf>,
    pub registered_worktrees: Vec<NativeRegisteredWorktree>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NativeRegisteredWorktree {
    pub task_id: String,
    pub project_name: String,
    pub branch_name: String,
    pub worktree_path: PathBuf,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct NativeWorktreeSyncResult {
    pub registered_worktrees: Vec<NativeRegisteredWorktree>,
    pub repaired_tasks: usize,
    pub errors: Vec<String>,
}

pub fn sync_native_local_project_worktrees(
    database_path: impl AsRef<Path>,
    project: &Project,
) -> NativeWorktreeSyncResult {
    let mut result = NativeWorktreeSyncResult::default();
    if project.ssh_host.is_some() {
        result.errors.push(format!(
            "{}: remote worktree sync is not available",
            project.name
        ));
        return result;
    }
    let worktrees = match list_worktrees(&project.repo_path) {
        Ok(worktrees) => worktrees,
        Err(error) => {
            result
                .errors
                .push(format!("{} worktree scan failed: {error}", project.name));
            return result;
        }
    };
    let database = match KanvibeDb::open_read_write(database_path) {
        Ok(database) => database,
        Err(error) => {
            result.errors.push(error.to_string());
            return result;
        }
    };
    let project_root = std::fs::canonicalize(&project.repo_path)
        .unwrap_or_else(|_| PathBuf::from(&project.repo_path));

    for worktree in worktrees {
        let Some(branch_name) = worktree.branch.as_deref() else {
            continue;
        };
        if worktree.is_bare {
            continue;
        }
        let worktree_path =
            std::fs::canonicalize(&worktree.path).unwrap_or_else(|_| worktree.path.clone());
        if worktree_path == project_root {
            continue;
        }
        let worktree_path_string = worktree_path.to_string_lossy().into_owned();
        let persisted_status = read_local_task_status(&worktree_path);
        let repository_name = Path::new(&project.repo_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(project.name.as_str());
        let live_session_name = format_session_name(repository_name, branch_name);
        let has_live_tmux = is_local_tmux_session_alive(&live_session_name).unwrap_or(false);
        let existing = match database.task_by_project_branch(&project.id, branch_name) {
            Ok(existing) => existing,
            Err(error) => {
                result.errors.push(format!(
                    "{} {branch_name}: failed to read existing task: {error}",
                    project.name
                ));
                continue;
            }
        };
        let candidate_is_project_task = existing.is_some();
        let candidate = if candidate_is_project_task {
            existing
        } else {
            match database.orphan_task_by_branch_and_path(branch_name, &worktree_path_string) {
                Ok(orphan) => orphan,
                Err(error) => {
                    result.errors.push(format!(
                        "{} {branch_name}: failed to read orphan task: {error}",
                        project.name
                    ));
                    continue;
                }
            }
        };

        let (task, should_report_registration) = if let Some(existing) = candidate {
            let desired_status = persisted_status.unwrap_or(existing.status);
            let needs_repair = existing.project_id.as_deref() != Some(project.id.as_str())
                || existing.worktree_path.as_deref() != Some(worktree_path_string.as_str())
                || existing.base_branch.as_deref() != Some(project.default_branch.as_str())
                || existing.ssh_host.is_some()
                || existing.status != desired_status
                || (has_live_tmux && existing.session_type.is_none());
            if !needs_repair {
                let _ = persist_local_task_status(&worktree_path, existing.status);
                continue;
            }
            match database.bind_task_to_worktree(
                &existing.id,
                &project.id,
                branch_name,
                &worktree_path_string,
                &project.default_branch,
                None,
                desired_status,
            ) {
                Ok(Some(mut task)) => {
                    if has_live_tmux {
                        match database.bind_live_session_if_unassigned(
                            &task.id,
                            SessionType::Tmux,
                            &live_session_name,
                            &worktree_path_string,
                            None,
                        ) {
                            Ok(Some(bound)) => task = bound,
                            Ok(None) => {}
                            Err(error) => result.errors.push(format!(
                                "{} {branch_name}: failed to bind live tmux session: {error}",
                                project.name
                            )),
                        }
                    }
                    result.repaired_tasks += 1;
                    (task, !candidate_is_project_task)
                }
                Ok(None) => {
                    result.errors.push(format!(
                        "{} {branch_name}: task disappeared during synchronization",
                        project.name
                    ));
                    continue;
                }
                Err(error) => {
                    result.errors.push(format!(
                        "{} {branch_name}: failed to repair task: {error}",
                        project.name
                    ));
                    continue;
                }
            }
        } else {
            match database.create_task(CreateTaskInput {
                title: Some(branch_name.to_owned()),
                status: persisted_status,
                branch_name: Some(branch_name.to_owned()),
                base_branch: Some(project.default_branch.clone()),
                worktree_path: Some(worktree_path_string),
                session_type: has_live_tmux.then_some(SessionType::Tmux),
                session_name: has_live_tmux.then_some(live_session_name),
                project_id: Some(project.id.clone()),
                ..CreateTaskInput::default()
            }) {
                Ok(task) => (task, true),
                Err(error) => {
                    result.errors.push(format!(
                        "{} {branch_name}: failed to create task: {error}",
                        project.name
                    ));
                    continue;
                }
            }
        };
        let _ = persist_local_task_status(&worktree_path, task.status);
        if let Err(error) = install_local_provider_hooks(
            &worktree_path,
            &task.id,
            &local_hook_server_url(native_hook_server_port()),
        ) {
            result.errors.push(format!(
                "{} {branch_name}: failed to install provider hooks: {error}",
                project.name
            ));
        }
        if should_report_registration {
            result.registered_worktrees.push(NativeRegisteredWorktree {
                task_id: task.id,
                project_name: project.name.clone(),
                branch_name: branch_name.to_owned(),
                worktree_path,
            });
        }
    }
    let repository_name = Path::new(&project.repo_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(project.name.as_str());
    let main_session_name = format_session_name(repository_name, &project.default_branch);
    if is_local_tmux_session_alive(&main_session_name).unwrap_or(false)
        && let Ok(Some(main_task)) =
            database.task_by_project_branch(&project.id, &project.default_branch)
        && main_task.session_type.is_none()
    {
        match database.bind_live_session_if_unassigned(
            &main_task.id,
            SessionType::Tmux,
            &main_session_name,
            &project.repo_path,
            None,
        ) {
            Ok(Some(task)) => {
                let _ = persist_local_task_status(Path::new(&project.repo_path), task.status);
                result.repaired_tasks += 1;
            }
            Ok(None) => {}
            Err(error) => result.errors.push(format!(
                "{} {}: failed to bind live tmux session: {error}",
                project.name, project.default_branch
            )),
        }
    }
    result
}

pub fn sync_native_remote_project_worktrees(
    database_path: impl AsRef<Path>,
    project: &Project,
) -> NativeWorktreeSyncResult {
    let database_path = database_path.as_ref();
    let Some(ssh_host) = project.ssh_host.as_deref() else {
        return NativeWorktreeSyncResult {
            errors: vec![format!("{}: SSH host is required", project.name)],
            ..NativeWorktreeSyncResult::default()
        };
    };
    let control_directory = match native_ssh_control_directory(database_path) {
        Ok(control_directory) => control_directory,
        Err(error) => {
            return NativeWorktreeSyncResult {
                errors: vec![error],
                ..NativeWorktreeSyncResult::default()
            };
        }
    };
    let client = match RemoteGitClient::new(ssh_host, control_directory) {
        Ok(client) => client,
        Err(error) => {
            return NativeWorktreeSyncResult {
                errors: vec![error.to_string()],
                ..NativeWorktreeSyncResult::default()
            };
        }
    };
    let worktrees = match client.list_worktrees(&project.repo_path) {
        Ok(worktrees) => worktrees,
        Err(error) => {
            return NativeWorktreeSyncResult {
                errors: vec![format!(
                    "{} remote worktree scan failed: {error}",
                    project.name
                )],
                ..NativeWorktreeSyncResult::default()
            };
        }
    };
    let mut result =
        sync_native_remote_worktree_snapshot(database_path, project, worktrees.clone());
    let database = match KanvibeDb::open_read_only(database_path) {
        Ok(database) => database,
        Err(error) => {
            result.errors.push(format!(
                "{}: failed to open remote hook task database: {error}",
                project.name
            ));
            return result;
        }
    };
    for worktree in worktrees {
        if worktree.is_bare || worktree.path == Path::new(&project.repo_path) {
            continue;
        }
        let Some(branch_name) = worktree.branch.as_deref() else {
            continue;
        };
        let task = match database.task_by_project_branch(&project.id, branch_name) {
            Ok(Some(task)) => task,
            Ok(None) => continue,
            Err(error) => {
                result.errors.push(format!(
                    "{} {branch_name}: failed to read remote hook task: {error}",
                    project.name
                ));
                continue;
            }
        };
        let worktree_path = worktree.path.to_string_lossy();
        if let Err(error) = install_remote_provider_hooks(&client, &worktree_path, &task.id) {
            result.errors.push(format!(
                "{} {branch_name}: failed to install remote provider hooks: {error}",
                project.name
            ));
        }
    }
    result
}

fn install_remote_provider_hooks(
    client: &RemoteGitClient,
    worktree_path: &str,
    task_id: &str,
) -> Result<(), String> {
    let mut existing_files = BTreeMap::new();
    for relative_path in PRESERVABLE_PROVIDER_HOOK_PATHS {
        if let Some(content) = client
            .read_optional_file(worktree_path, relative_path)
            .map_err(|error| error.to_string())?
        {
            existing_files.insert((*relative_path).to_owned(), content);
        }
    }
    let callback_host = client
        .ssh_client_address()
        .map_err(|error| error.to_string())?;
    let hook_server_url = remote_hook_server_url(&callback_host, native_hook_server_port());
    let rendered = render_provider_hooks(&existing_files, task_id, &hook_server_url)
        .map_err(|error| error.to_string())?;
    for file in rendered {
        client
            .write_file(
                worktree_path,
                &file.relative_path,
                &file.content,
                file.executable,
            )
            .map_err(|error| error.to_string())?;
    }
    client
        .ensure_git_exclude_lines(worktree_path, PROVIDER_HOOK_EXCLUDE_LINES)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn native_task_hook_target(
    database_path: &Path,
    task_id: &str,
) -> Result<(String, Option<String>), String> {
    let database = KanvibeDb::open_read_only(database_path).map_err(|error| error.to_string())?;
    let task = database
        .task_by_id(task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Task no longer exists.".to_owned())?;
    let project = task
        .project_id
        .as_deref()
        .map(|project_id| {
            database
                .project_by_id(project_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "Task project no longer exists.".to_owned())
        })
        .transpose()?;
    let target_path = task
        .worktree_path
        .or_else(|| project.as_ref().map(|project| project.repo_path.clone()))
        .ok_or_else(|| "Task has no project or worktree for provider hooks.".to_owned())?;
    let ssh_host = task
        .ssh_host
        .or_else(|| project.and_then(|project| project.ssh_host));
    Ok((target_path, ssh_host))
}

pub fn get_native_task_hook_status(
    database_path: impl AsRef<Path>,
    task_id: &str,
) -> Result<Vec<HookProviderStatus>, String> {
    let database_path = database_path.as_ref();
    let (target_path, ssh_host) = native_task_hook_target(database_path, task_id)?;
    let all_paths = kanvibe_hooks::AI_PROVIDERS
        .iter()
        .flat_map(|provider| provider_hook_required_paths(*provider).iter().copied())
        .collect::<BTreeSet<_>>();
    let mut files = BTreeMap::new();
    let (expected_url, reachable, is_remote) = if let Some(ssh_host) = ssh_host.as_deref() {
        let client = RemoteGitClient::new(ssh_host, native_ssh_control_directory(database_path)?)
            .map_err(|error| error.to_string())?;
        for path in all_paths {
            if let Some(content) = client
                .read_optional_file(&target_path, path)
                .map_err(|error| error.to_string())?
            {
                files.insert(path.to_owned(), content);
            }
        }
        let callback_host = client
            .ssh_client_address()
            .map_err(|error| error.to_string())?;
        let callback_ip = callback_host
            .parse::<IpAddr>()
            .map_err(|_| "SSH callback address was invalid.".to_owned())?;
        let reachable = client
            .native_hook_server_reachable(callback_ip, native_hook_server_port())
            .map_err(|error| error.to_string())?;
        (
            remote_hook_server_url(&callback_host, native_hook_server_port()),
            reachable,
            true,
        )
    } else {
        for path in all_paths {
            if let Ok(content) = std::fs::read_to_string(Path::new(&target_path).join(path)) {
                files.insert(path.to_owned(), content);
            }
        }
        let reachable = TcpStream::connect_timeout(
            &SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), native_hook_server_port()),
            Duration::from_millis(250),
        )
        .is_ok();
        (
            local_hook_server_url(native_hook_server_port()),
            reachable,
            false,
        )
    };
    Ok(kanvibe_hooks::AI_PROVIDERS
        .iter()
        .map(|provider| {
            inspect_provider_hook_status(
                *provider,
                &files,
                Some(&expected_url),
                is_remote,
                reachable,
            )
        })
        .collect())
}

pub fn install_native_task_hooks(
    database_path: impl AsRef<Path>,
    task_id: &str,
) -> Result<Vec<HookProviderStatus>, String> {
    let database_path = database_path.as_ref();
    let (target_path, ssh_host) = native_task_hook_target(database_path, task_id)?;
    if let Some(ssh_host) = ssh_host.as_deref() {
        let client = RemoteGitClient::new(ssh_host, native_ssh_control_directory(database_path)?)
            .map_err(|error| error.to_string())?;
        install_remote_provider_hooks(&client, &target_path, task_id)?;
    } else {
        install_local_provider_hooks(
            &target_path,
            task_id,
            &local_hook_server_url(native_hook_server_port()),
        )
        .map_err(|error| error.to_string())?;
    }
    get_native_task_hook_status(database_path, task_id)
}

fn sync_native_remote_worktree_snapshot(
    database_path: &Path,
    project: &Project,
    worktrees: Vec<RegisteredWorktree>,
) -> NativeWorktreeSyncResult {
    let mut result = NativeWorktreeSyncResult::default();
    let Some(ssh_host) = project.ssh_host.as_deref() else {
        result
            .errors
            .push(format!("{}: SSH host is required", project.name));
        return result;
    };
    let database = match KanvibeDb::open_read_write(database_path) {
        Ok(database) => database,
        Err(error) => {
            result.errors.push(error.to_string());
            return result;
        }
    };

    for worktree in worktrees {
        let Some(branch_name) = worktree.branch.as_deref() else {
            continue;
        };
        if worktree.is_bare {
            continue;
        }
        let worktree_path = worktree.path.to_string_lossy().into_owned();
        let existing = match database.task_by_project_branch(&project.id, branch_name) {
            Ok(existing) => existing,
            Err(error) => {
                result.errors.push(format!(
                    "{} {branch_name}: failed to read existing remote task: {error}",
                    project.name
                ));
                continue;
            }
        };
        let candidate_is_project_task = existing.is_some();
        let candidate = if candidate_is_project_task {
            existing
        } else {
            match database.orphan_task_by_location(branch_name, &worktree_path, Some(ssh_host)) {
                Ok(orphan) => orphan,
                Err(error) => {
                    result.errors.push(format!(
                        "{} {branch_name}: failed to read remote orphan task: {error}",
                        project.name
                    ));
                    continue;
                }
            }
        };
        let (task, should_report_registration) = if let Some(existing) = candidate {
            let needs_repair = existing.project_id.as_deref() != Some(project.id.as_str())
                || existing.worktree_path.as_deref() != Some(worktree_path.as_str())
                || existing.base_branch.as_deref() != Some(project.default_branch.as_str())
                || existing.ssh_host.as_deref() != Some(ssh_host);
            if !needs_repair {
                continue;
            }
            match database.bind_task_to_worktree(
                &existing.id,
                &project.id,
                branch_name,
                &worktree_path,
                &project.default_branch,
                Some(ssh_host),
                existing.status,
            ) {
                Ok(Some(task)) => {
                    result.repaired_tasks += 1;
                    (task, !candidate_is_project_task)
                }
                Ok(None) => {
                    result.errors.push(format!(
                        "{} {branch_name}: remote task disappeared during synchronization",
                        project.name
                    ));
                    continue;
                }
                Err(error) => {
                    result.errors.push(format!(
                        "{} {branch_name}: failed to repair remote task: {error}",
                        project.name
                    ));
                    continue;
                }
            }
        } else {
            match database.create_task(CreateTaskInput {
                title: Some(branch_name.to_owned()),
                branch_name: Some(branch_name.to_owned()),
                base_branch: Some(project.default_branch.clone()),
                worktree_path: Some(worktree_path),
                ssh_host: Some(ssh_host.to_owned()),
                project_id: Some(project.id.clone()),
                ..CreateTaskInput::default()
            }) {
                Ok(task) => (task, true),
                Err(error) => {
                    result.errors.push(format!(
                        "{} {branch_name}: failed to create remote task: {error}",
                        project.name
                    ));
                    continue;
                }
            }
        };
        if should_report_registration {
            result.registered_worktrees.push(NativeRegisteredWorktree {
                task_id: task.id,
                project_name: project.name.clone(),
                branch_name: branch_name.to_owned(),
                worktree_path: worktree.path,
            });
        }
    }
    result
}

pub fn sync_native_project_worktrees(
    database_path: impl AsRef<Path>,
    project: &Project,
) -> NativeWorktreeSyncResult {
    if project.ssh_host.is_some() {
        sync_native_remote_project_worktrees(database_path, project)
    } else {
        sync_native_local_project_worktrees(database_path, project)
    }
}

pub fn sync_native_background_projects(
    database_path: impl AsRef<Path>,
) -> Result<NativeBackgroundSyncRunResult, String> {
    sync_native_background_projects_with_state(
        database_path,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
    )
}

fn sync_native_background_projects_with_state(
    database_path: impl AsRef<Path>,
    emitted_merge_event_keys: &mut BTreeSet<String>,
    notified_pull_failure_keys: &mut BTreeSet<String>,
) -> Result<NativeBackgroundSyncRunResult, String> {
    let database_path = database_path.as_ref();
    let database = KanvibeDb::open_read_only(database_path).map_err(|error| error.to_string())?;
    let projects = database.projects().map_err(|error| error.to_string())?;
    drop(database);

    let mut result = NativeBackgroundSyncRunResult::default();
    for project in projects {
        let project_result = sync_native_project_worktrees(database_path, &project);
        result.registered_worktrees = result
            .registered_worktrees
            .saturating_add(project_result.registered_worktrees.len());
        result.repaired_tasks = result
            .repaired_tasks
            .saturating_add(project_result.repaired_tasks);
        result.errors.extend(project_result.errors);
    }
    sync_native_pull_requests(database_path, emitted_merge_event_keys, &mut result)?;
    sync_native_active_task_pulls(database_path, notified_pull_failure_keys, &mut result)?;
    Ok(result)
}

fn sync_native_active_task_pulls(
    database_path: &Path,
    notified_pull_failure_keys: &mut BTreeSet<String>,
    result: &mut NativeBackgroundSyncRunResult,
) -> Result<(), String> {
    sync_native_active_task_pulls_with(
        database_path,
        notified_pull_failure_keys,
        result,
        |worktree_path, branch_name, ssh_host| match ssh_host {
            Some(ssh_host) => {
                let client =
                    RemoteGitClient::new(ssh_host, native_ssh_control_directory(database_path)?)
                        .map_err(|error| error.to_string())?;
                client
                    .remote_branch_exists(worktree_path, branch_name)
                    .map_err(|error| error.to_string())
                    .and_then(|exists| {
                        exists
                            .then(|| {
                                client
                                    .pull_current_branch(worktree_path)
                                    .map_err(|error| error.to_string())
                            })
                            .transpose()
                    })
            }
            None => remote_branch_exists(worktree_path, branch_name)
                .map_err(|error| error.to_string())
                .and_then(|exists| {
                    exists
                        .then(|| {
                            pull_current_branch(worktree_path).map_err(|error| error.to_string())
                        })
                        .transpose()
                }),
        },
    )
}

fn sync_native_active_task_pulls_with<F>(
    database_path: &Path,
    notified_pull_failure_keys: &mut BTreeSet<String>,
    result: &mut NativeBackgroundSyncRunResult,
    mut pull: F,
) -> Result<(), String>
where
    F: FnMut(&str, &str, Option<&str>) -> Result<Option<String>, String>,
{
    let database = KanvibeDb::open_read_only(database_path).map_err(|error| error.to_string())?;
    let projects = database
        .projects()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|project| (project.id.clone(), project))
        .collect::<BTreeMap<_, _>>();
    let tasks = database.active_tasks().map_err(|error| error.to_string())?;
    drop(database);

    for task in tasks {
        let (Some(branch_name), Some(worktree_path)) =
            (task.branch_name.as_deref(), task.worktree_path.as_deref())
        else {
            continue;
        };
        let project = task
            .project_id
            .as_deref()
            .and_then(|project_id| projects.get(project_id));
        if project.is_some_and(|project| project.default_branch == branch_name) {
            continue;
        }
        let ssh_host = task
            .ssh_host
            .as_deref()
            .or_else(|| project.and_then(|project| project.ssh_host.as_deref()));
        let failure_key = [
            task.id.as_str(),
            branch_name,
            worktree_path,
            ssh_host.unwrap_or(""),
        ]
        .join("::");
        match pull(worktree_path, branch_name, ssh_host) {
            Ok(None) => {
                notified_pull_failure_keys.remove(&failure_key);
            }
            Ok(Some(output)) => {
                notified_pull_failure_keys.remove(&failure_key);
                if !is_pull_noop(&output) {
                    result.pulled_tasks.push(NativeTaskPullSync {
                        task_id: task.id,
                        task_title: task.title,
                        branch_name: branch_name.to_owned(),
                        worktree_path: worktree_path.to_owned(),
                        ssh_host: ssh_host.map(ToOwned::to_owned),
                        status: NativeTaskPullStatus::Updated,
                        summary: summarize_pull_output(&output),
                    });
                }
            }
            Err(error) if is_missing_remote_branch_pull_error(&error.to_string()) => {
                notified_pull_failure_keys.remove(&failure_key);
            }
            Err(error) => {
                if notified_pull_failure_keys.insert(failure_key) {
                    result.pulled_tasks.push(NativeTaskPullSync {
                        task_id: task.id,
                        task_title: task.title,
                        branch_name: branch_name.to_owned(),
                        worktree_path: worktree_path.to_owned(),
                        ssh_host: ssh_host.map(ToOwned::to_owned),
                        status: NativeTaskPullStatus::Failed,
                        summary: error.to_string(),
                    });
                }
            }
        }
    }
    Ok(())
}

fn summarize_pull_output(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Pull completed")
        .to_owned()
}

fn is_pull_noop(output: &str) -> bool {
    let output = output.to_ascii_lowercase();
    output.contains("already up-to-date")
        || output.contains("already up to date")
        || (output.contains("current branch") && output.contains("is up to date"))
}

fn is_missing_remote_branch_pull_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("no such ref was fetched")
        || message.contains("could not find remote ref")
        || message.contains("couldn't find remote ref")
        || (message.contains("requested upstream branch") && message.contains("does not exist"))
}

fn sync_native_pull_requests(
    database_path: &Path,
    emitted_merge_event_keys: &mut BTreeSet<String>,
    result: &mut NativeBackgroundSyncRunResult,
) -> Result<(), String> {
    sync_native_pull_requests_with(
        database_path,
        emitted_merge_event_keys,
        result,
        |task, project, repository_path, branch_name| {
            let ssh_host = task
                .ssh_host
                .as_deref()
                .or_else(|| project.and_then(|project| project.ssh_host.as_deref()));
            match ssh_host {
                Some(ssh_host) => {
                    let client = RemoteGitClient::new(
                        ssh_host,
                        native_ssh_control_directory(database_path)?,
                    )
                    .map_err(|error| error.to_string())?;
                    client
                        .pull_request_for_branch(repository_path, branch_name)
                        .map_err(|error| error.to_string())
                }
                None => pull_request_for_branch(repository_path, branch_name)
                    .map_err(|error| error.to_string()),
            }
        },
    )
}

fn sync_native_pull_requests_with<F>(
    database_path: &Path,
    emitted_merge_event_keys: &mut BTreeSet<String>,
    result: &mut NativeBackgroundSyncRunResult,
    mut lookup: F,
) -> Result<(), String>
where
    F: FnMut(&KanbanTask, Option<&Project>, &str, &str) -> Result<Option<PullRequestInfo>, String>,
{
    let database = KanvibeDb::open_read_only(database_path).map_err(|error| error.to_string())?;
    let projects = database
        .projects()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|project| (project.id.clone(), project))
        .collect::<BTreeMap<_, _>>();
    let tasks = database.active_tasks().map_err(|error| error.to_string())?;
    drop(database);

    for task in tasks {
        let Some(branch_name) = task.branch_name.as_deref() else {
            continue;
        };
        let project = task
            .project_id
            .as_deref()
            .and_then(|project_id| projects.get(project_id));
        if project.is_some_and(|project| project.default_branch == branch_name) {
            continue;
        }
        let Some(repository_path) = task
            .worktree_path
            .as_deref()
            .or_else(|| project.map(|project| project.repo_path.as_str()))
        else {
            result.errors.push(format!(
                "{} ({branch_name}): pull request sync has no repository path",
                task.title
            ));
            continue;
        };
        let pull_request = match lookup(&task, project, repository_path, branch_name) {
            Ok(Some(pull_request)) => pull_request,
            Ok(None) => continue,
            Err(error) => {
                result.errors.push(format!(
                    "{} ({branch_name}): pull request sync failed: {error}",
                    task.title
                ));
                continue;
            }
        };
        let database =
            KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
        if database
            .set_task_pr_url_if_changed(&task.id, &pull_request.url)
            .map_err(|error| error.to_string())?
        {
            result.updated_pull_requests = result.updated_pull_requests.saturating_add(1);
        }
        if pull_request.state == "MERGED"
            && let Some(merged_at) = pull_request.merged_at
        {
            let event_key = format!("{}:{}:{merged_at}", task.id, pull_request.url);
            if emitted_merge_event_keys.insert(event_key) {
                result.merged_pull_requests.push(NativeMergedPullRequest {
                    task_id: task.id,
                    task_title: task.title,
                    branch_name: branch_name.to_owned(),
                    pr_url: pull_request.url,
                    merged_at,
                });
            }
        }
    }
    Ok(())
}

pub fn start_native_background_sync(
    database_path: impl Into<PathBuf>,
    revision: Arc<AtomicU64>,
) -> Result<NativeBackgroundSyncService, String> {
    start_native_background_sync_with_notifications(database_path, revision, None, Locale::En)
}

pub fn start_native_background_sync_with_notifications(
    database_path: impl Into<PathBuf>,
    revision: Arc<AtomicU64>,
    notifications: Option<Arc<NativeNotificationService>>,
    locale: Locale,
) -> Result<NativeBackgroundSyncService, String> {
    let database_path = database_path.into();
    let database = KanvibeDb::open_read_only(&database_path).map_err(|error| error.to_string())?;
    let settings = database
        .background_sync_settings()
        .map_err(|error| error.to_string())?;
    drop(database);

    let emitted_merge_event_keys = Arc::new(Mutex::new(BTreeSet::new()));
    let notified_pull_failure_keys = Arc::new(Mutex::new(BTreeSet::new()));
    NativeBackgroundSyncService::start(settings, move || {
        let mut emitted_merge_event_keys = emitted_merge_event_keys
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut notified_pull_failure_keys = notified_pull_failure_keys
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match sync_native_background_projects_with_state(
            &database_path,
            &mut emitted_merge_event_keys,
            &mut notified_pull_failure_keys,
        ) {
            Ok(result) => {
                if !result.errors.is_empty() {
                    eprintln!(
                        "{}",
                        native_diagnostic_line(
                            "background-sync-partial-error",
                            "background-sync",
                            &result.errors.join("; "),
                            None,
                        )
                    );
                }
                if result.has_board_changes() {
                    revision.fetch_add(1, Ordering::AcqRel);
                }
                if result.needs_review()
                    && let Some(notifications) = &notifications
                    && KanvibeDb::open_read_only(&database_path)
                        .and_then(|database| database.notification_enabled())
                        .unwrap_or(false)
                    && let Err(error) =
                        notifications.publish(background_sync_notification_draft(&result, locale))
                {
                    eprintln!(
                        "{}",
                        native_diagnostic_line(
                            "background-sync-notification-error",
                            "notification-center",
                            &error,
                            None,
                        )
                    );
                }
                Ok(result)
            }
            Err(error) => {
                eprintln!(
                    "{}",
                    native_diagnostic_line(
                        "background-sync-error",
                        "background-sync",
                        &error,
                        None,
                    )
                );
                Err(error)
            }
        }
    })
    .map_err(|error| error.to_string())
}

fn background_sync_notification_draft(
    result: &NativeBackgroundSyncRunResult,
    locale: Locale,
) -> NotificationDraft {
    let updated_pull_count = result
        .pulled_tasks
        .iter()
        .filter(|pull| pull.status == NativeTaskPullStatus::Updated)
        .count();
    let failed_pull_count = result
        .pulled_tasks
        .iter()
        .filter(|pull| pull.status == NativeTaskPullStatus::Failed)
        .count();
    let (title, labels) = match locale {
        Locale::Ko => (
            "백그라운드 sync 검토 필요",
            [
                "merge된 PR",
                "새 TODO worktree",
                "pull 완료",
                "pull 실패",
                "sync 실패",
            ],
        ),
        Locale::En => (
            "Background sync review needed",
            [
                "merged PRs",
                "new TODO worktrees",
                "pull updates",
                "failed pulls",
                "sync failures",
            ],
        ),
        Locale::Zh => (
            "需要检查后台同步结果",
            [
                "已合并 PR",
                "新建 TODO worktree",
                "pull 完成",
                "pull 失败",
                "sync 失败",
            ],
        ),
    };
    let body = [
        result.merged_pull_requests.len(),
        result.registered_worktrees,
        updated_pull_count,
        failed_pull_count,
        result.errors.len(),
    ]
    .into_iter()
    .zip(labels)
    .filter(|(count, _)| *count > 0)
    .map(|(count, label)| format!("{label} {count}"))
    .collect::<Vec<_>>()
    .join(" / ");
    let mut dedupe_parts = result
        .merged_pull_requests
        .iter()
        .map(|pull_request| {
            format!(
                "{}:{}:{}",
                pull_request.task_id, pull_request.pr_url, pull_request.merged_at
            )
        })
        .chain(result.pulled_tasks.iter().map(|pull| {
            format!(
                "{}:{}:{:?}:{}",
                pull.task_id, pull.branch_name, pull.status, pull.summary
            )
        }))
        .chain(result.errors.iter().cloned())
        .collect::<Vec<_>>();
    dedupe_parts.sort();
    dedupe_parts.push(format!("worktrees:{}", result.registered_worktrees));

    NotificationDraft {
        title: title.to_owned(),
        body,
        task_id: None,
        relative_path: Some(format!("/{}", locale.code())),
        locale: locale.code().to_owned(),
        dedupe_key: format!("background-sync-review:{}", dedupe_parts.join("|")),
        action: Some(serde_json::json!({
            "type": "background-sync-review",
        })),
    }
}

fn read_local_task_status(worktree_path: &Path) -> Option<TaskStatus> {
    let content = std::fs::read_to_string(worktree_path.join(".kanvibe/status.json")).ok()?;
    let payload: serde_json::Value = serde_json::from_str(&content).ok()?;
    if payload.get("schemaVersion")?.as_u64()? != 1 {
        return None;
    }
    TaskStatus::parse(payload.get("status")?.as_str()?).ok()
}

fn persist_local_task_status(worktree_path: &Path, status: TaskStatus) -> Result<(), String> {
    ensure_git_exclude_pattern(worktree_path, ".kanvibe/").map_err(|error| error.to_string())?;
    let state_directory = worktree_path.join(".kanvibe");
    std::fs::create_dir_all(&state_directory).map_err(|error| error.to_string())?;
    let payload = serde_json::json!({
        "schemaVersion": 1,
        "status": status.as_str(),
    });
    std::fs::write(
        state_directory.join("status.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?
        ),
    )
    .map_err(|error| error.to_string())
}

pub fn scan_and_register_native_local_projects(
    database_path: impl AsRef<Path>,
    root_path: impl AsRef<Path>,
) -> Result<NativeProjectScanResult, String> {
    let database_path = database_path.as_ref();
    let repositories = scan_git_repositories(root_path).map_err(|error| error.to_string())?;
    let database = KanvibeDb::open_read_only(database_path).map_err(|error| error.to_string())?;
    let mut existing_paths = database
        .projects()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|project| project.ssh_host.is_none())
        .map(|project| PathBuf::from(project.repo_path))
        .collect::<BTreeSet<_>>();
    drop(database);

    let mut result = NativeProjectScanResult {
        registered: Vec::new(),
        skipped: Vec::new(),
        registered_worktrees: Vec::new(),
        errors: Vec::new(),
    };
    let mut discovered_roots = BTreeSet::new();
    let mut scanned_projects = Vec::new();
    for repository in repositories {
        match is_submodule_repository(&repository) {
            Ok(true) => {
                result.skipped.push(repository);
                continue;
            }
            Ok(false) => {}
            Err(error) => {
                result
                    .errors
                    .push(format!("{}: {error}", repository.to_string_lossy()));
                continue;
            }
        }
        let common_root = match common_repository_root(&repository) {
            Ok(common_root) => common_root,
            Err(error) => {
                result
                    .errors
                    .push(format!("{}: {error}", repository.to_string_lossy()));
                continue;
            }
        };
        if !discovered_roots.insert(common_root.clone()) {
            result.skipped.push(common_root);
            continue;
        }
        if existing_paths.contains(&common_root) {
            if let Ok(database) = KanvibeDb::open_read_only(database_path)
                && let Ok(projects) = database.projects()
                && let Some(project) = projects.into_iter().find(|project| {
                    project.ssh_host.is_none() && Path::new(&project.repo_path) == common_root
                })
            {
                scanned_projects.push(project);
            }
            result.skipped.push(common_root);
            continue;
        }
        let preferred_name = common_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("project");
        match register_native_local_project(database_path, preferred_name, &common_root) {
            Ok(project) => {
                existing_paths.insert(common_root);
                scanned_projects.push(project.clone());
                result.registered.push(project);
            }
            Err(error) => result
                .errors
                .push(format!("{}: {error}", common_root.to_string_lossy())),
        }
    }
    for project in scanned_projects {
        let worktree_result = sync_native_local_project_worktrees(database_path, &project);
        result
            .registered_worktrees
            .extend(worktree_result.registered_worktrees);
        result.errors.extend(worktree_result.errors);
    }
    Ok(result)
}

pub fn delete_native_project(
    database_path: impl AsRef<Path>,
    project_id: &str,
) -> Result<bool, String> {
    let mut database =
        KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    database
        .delete_project(project_id)
        .map_err(|error| error.to_string())
}

pub const PROJECT_COLOR_PRESETS: [&str; 8] = [
    "#F9A8D4", "#93C5FD", "#86EFAC", "#C4B5FD", "#FDBA74", "#FDE047", "#5EEAD4", "#A5B4FC",
];

pub fn compute_project_color(project_name: &str) -> &'static str {
    let hash = project_name.encode_utf16().fold(0_i32, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(i32::from(unit))
    });
    let index = hash.rem_euclid(PROJECT_COLOR_PRESETS.len() as i32) as usize;
    PROJECT_COLOR_PRESETS[index]
}

pub fn update_native_project_color(
    database_path: impl AsRef<Path>,
    project_id: &str,
    color: &str,
) -> Result<(), String> {
    let project_id = project_id.trim();
    if project_id.is_empty() {
        return Err("Project id is required.".to_owned());
    }
    let color = color.trim();
    if color.len() != 7
        || !color.starts_with('#')
        || !color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Project color must use #RRGGBB format.".to_owned());
    }
    let normalized_color = color.to_ascii_uppercase();
    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    if database
        .project_by_id(project_id)
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Err("Project no longer exists.".to_owned());
    }
    database
        .update_project_color(project_id, &normalized_color)
        .map_err(|error| error.to_string())
}

pub fn create_native_task(
    database_path: impl AsRef<Path>,
    title: &str,
) -> Result<KanbanTask, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Task title is required.".to_owned());
    }

    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    database
        .create_task(CreateTaskInput {
            title: Some(title.to_owned()),
            ..Default::default()
        })
        .map_err(|error| error.to_string())
}

pub fn create_native_task_from_form(
    database_path: impl AsRef<Path>,
    input: NativeTaskFormInput,
) -> Result<KanbanTask, String> {
    let database_path = database_path.as_ref();
    let branch_name = input.branch_name.trim();
    if branch_name.is_empty() {
        return Err("Branch name is required.".to_owned());
    }
    let project_id = input.project_id.trim();
    if project_id.is_empty() {
        return Err("Project is required.".to_owned());
    }
    let optional_trimmed = |value: String| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    };
    let description = optional_trimmed(input.description);
    let requested_base_branch = optional_trimmed(input.base_branch);

    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    let project = database
        .project_by_id(project_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Project no longer exists.".to_owned())?;
    if project.is_worktree {
        return Err("A branch task must use a root project.".to_owned());
    }
    if database
        .task_by_project_branch(project_id, branch_name)
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("A task already exists for this project branch.".to_owned());
    }
    let base_branch = requested_base_branch.unwrap_or_else(|| project.default_branch.clone());

    let (worktree_path, session_name) = if let Some(ssh_host) = project.ssh_host.as_deref() {
        let client = RemoteGitClient::new(ssh_host, native_ssh_control_directory(database_path)?)
            .map_err(|error| error.to_string())?;
        let worktree_path = client
            .create_worktree(&project.repo_path, branch_name, &base_branch)
            .map_err(|error| error.to_string())?;
        (
            worktree_path,
            format_session_name(&project.name, branch_name),
        )
    } else {
        let session = create_worktree_with_session(
            &project.repo_path,
            branch_name,
            &base_branch,
            input.session_type,
        )
        .map_err(|error| error.to_string())?;
        (session.worktree_path, session.session_name)
    };
    let worktree_path_text = worktree_path.to_string_lossy().into_owned();

    let task = match database.create_task(CreateTaskInput {
        title: Some(branch_name.to_owned()),
        description,
        branch_name: Some(branch_name.to_owned()),
        base_branch: Some(base_branch),
        worktree_path: Some(worktree_path_text.clone()),
        session_type: Some(input.session_type),
        session_name: Some(session_name),
        ssh_host: project.ssh_host.clone(),
        project_id: Some(project_id.to_owned()),
        priority: input.priority,
        ..Default::default()
    }) {
        Ok(task) => task,
        Err(error) => {
            drop(database);
            let cleanup = rollback_native_branch_worktree(
                database_path,
                &project,
                branch_name,
                &worktree_path,
            );
            return Err(match cleanup {
                Ok(()) => error.to_string(),
                Err(cleanup_error) => {
                    format!("{error}; worktree rollback also failed: {cleanup_error}")
                }
            });
        }
    };
    drop(database);

    let hook_result = if let Some(ssh_host) = project.ssh_host.as_deref() {
        native_ssh_control_directory(database_path)
            .and_then(|control_directory| {
                RemoteGitClient::new(ssh_host, control_directory).map_err(|error| error.to_string())
            })
            .and_then(|client| {
                install_remote_provider_hooks(&client, &worktree_path_text, &task.id)
            })
    } else {
        install_local_provider_hooks(
            &worktree_path,
            &task.id,
            &local_hook_server_url(native_hook_server_port()),
        )
        .map_err(|error| error.to_string())
        .and_then(|_| persist_local_task_status(&worktree_path, task.status))
    };
    if let Err(error) = hook_result {
        return match delete_native_task(database_path, &task.id) {
            Ok(true) => Err(format!("Provider hook installation failed: {error}")),
            Ok(false) => Err(format!(
                "Provider hook installation failed and task rollback lost the row: {error}"
            )),
            Err(rollback_error) => Err(format!(
                "Provider hook installation failed: {error}; task rollback also failed: \
                 {rollback_error}"
            )),
        };
    }

    Ok(task)
}

pub fn branch_native_task_from_form(
    database_path: impl AsRef<Path>,
    task_id: &str,
    input: NativeTaskFormInput,
) -> Result<KanbanTask, String> {
    let database_path = database_path.as_ref();
    let branch_name = input.branch_name.trim();
    if branch_name.is_empty() {
        return Err("Branch name is required.".to_owned());
    }
    let project_id = input.project_id.trim();
    if project_id.is_empty() {
        return Err("Project is required.".to_owned());
    }
    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    let original = database
        .task_by_id(task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Task no longer exists.".to_owned())?;
    if original.branch_name.is_some()
        || original.worktree_path.is_some()
        || original.session_name.is_some()
    {
        return Err("Task already has a branch or live session.".to_owned());
    }
    let project = database
        .project_by_id(project_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Project no longer exists.".to_owned())?;
    if project.is_worktree {
        return Err("A branch task must use a root project.".to_owned());
    }
    if database
        .task_by_project_branch(project_id, branch_name)
        .map_err(|error| error.to_string())?
        .is_some_and(|task| task.id != task_id)
    {
        return Err("A task already exists for this project branch.".to_owned());
    }
    let base_branch = {
        let requested = input.base_branch.trim();
        if requested.is_empty() {
            project.default_branch.clone()
        } else {
            requested.to_owned()
        }
    };
    let (worktree_path, session_name) = if let Some(ssh_host) = project.ssh_host.as_deref() {
        let client = RemoteGitClient::new(ssh_host, native_ssh_control_directory(database_path)?)
            .map_err(|error| error.to_string())?;
        let worktree_path = client
            .create_worktree(&project.repo_path, branch_name, &base_branch)
            .map_err(|error| error.to_string())?;
        (
            worktree_path,
            format_session_name(&project.name, branch_name),
        )
    } else {
        let session = create_worktree_with_session(
            &project.repo_path,
            branch_name,
            &base_branch,
            input.session_type,
        )
        .map_err(|error| error.to_string())?;
        (session.worktree_path, session.session_name)
    };
    let worktree_path_text = worktree_path.to_string_lossy().into_owned();
    let branched = match database.branch_from_task(
        task_id,
        project_id,
        &base_branch,
        branch_name,
        input.session_type,
        &session_name,
        &worktree_path_text,
    ) {
        Ok(Some(task)) => task,
        Ok(None) => {
            drop(database);
            rollback_native_branch_worktree(database_path, &project, branch_name, &worktree_path)?;
            return Err("Task no longer exists.".to_owned());
        }
        Err(error) => {
            drop(database);
            let cleanup = rollback_native_branch_worktree(
                database_path,
                &project,
                branch_name,
                &worktree_path,
            );
            return Err(cleanup.map_or_else(
                |cleanup_error| format!("{error}; worktree rollback also failed: {cleanup_error}"),
                |()| error.to_string(),
            ));
        }
    };
    drop(database);

    let hook_result = if let Some(ssh_host) = project.ssh_host.as_deref() {
        native_ssh_control_directory(database_path)
            .and_then(|directory| {
                RemoteGitClient::new(ssh_host, directory).map_err(|error| error.to_string())
            })
            .and_then(|client| install_remote_provider_hooks(&client, &worktree_path_text, task_id))
    } else {
        install_local_provider_hooks(
            &worktree_path,
            task_id,
            &local_hook_server_url(native_hook_server_port()),
        )
        .map_err(|error| error.to_string())
        .and_then(|_| persist_local_task_status(&worktree_path, branched.status))
    };
    if let Err(error) = hook_result {
        rollback_native_branch_worktree(database_path, &project, branch_name, &worktree_path)
            .map_err(|rollback| {
                format!("Provider hook installation failed: {error}; cleanup failed: {rollback}")
            })?;
        let database =
            KanvibeDb::open_read_write(database_path).map_err(|rollback| rollback.to_string())?;
        database
            .restore_task_branch_binding(&original)
            .map_err(|rollback| {
                format!(
                    "Provider hook installation failed: {error}; task rollback failed: {rollback}"
                )
            })?;
        return Err(format!("Provider hook installation failed: {error}"));
    }

    Ok(branched)
}

fn rollback_native_branch_worktree(
    database_path: &Path,
    project: &Project,
    branch_name: &str,
    worktree_path: &Path,
) -> Result<(), String> {
    if let Some(ssh_host) = project.ssh_host.as_deref() {
        RemoteGitClient::new(ssh_host, native_ssh_control_directory(database_path)?)
            .map_err(|error| error.to_string())?
            .remove_worktree_and_branch(&project.repo_path, branch_name, Some(worktree_path))
            .map_err(|error| error.to_string())
    } else {
        remove_worktree_and_branch(&project.repo_path, branch_name, Some(worktree_path))
            .map_err(|error| error.to_string())
    }
}

pub fn update_native_task_title(
    database_path: impl AsRef<Path>,
    task_id: &str,
    title: &str,
) -> Result<Option<KanbanTask>, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Task title is required.".to_owned());
    }

    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    database
        .update_task(
            task_id,
            TaskUpdatePatch {
                title: Some(title.to_owned()),
                ..Default::default()
            },
        )
        .map_err(|error| error.to_string())
}

pub fn update_native_task_metadata(
    database_path: impl AsRef<Path>,
    task_id: &str,
    title: &str,
    description: &str,
    priority: Option<TaskPriority>,
) -> Result<Option<KanbanTask>, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Task title is required.".to_owned());
    }
    let description = description.trim();
    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    database
        .update_task(
            task_id,
            TaskUpdatePatch {
                title: Some(title.to_owned()),
                description: Some((!description.is_empty()).then(|| description.to_owned())),
                priority: Some(priority),
            },
        )
        .map_err(|error| error.to_string())
}

pub fn delete_native_task(database_path: impl AsRef<Path>, task_id: &str) -> Result<bool, String> {
    let database_path = database_path.as_ref();
    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    let Some(task) = database
        .task_by_id(task_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };
    cleanup_task_resources(&database, database_path, &task)?;
    database
        .delete_task(task_id)
        .map_err(|error| error.to_string())
}

pub fn load_native_diff_snapshot(
    database_path: impl AsRef<Path>,
    task_id: &str,
) -> Result<NativeDiffSnapshot, String> {
    let database_path = database_path.as_ref();
    let database = KanvibeDb::open_read_only(database_path).map_err(|error| error.to_string())?;
    let task = database
        .task_by_id(task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Task no longer exists.".to_owned())?;
    let worktree_path = task
        .worktree_path
        .as_deref()
        .ok_or_else(|| "Task has no worktree.".to_owned())?;
    let base_branch = task
        .base_branch
        .clone()
        .ok_or_else(|| "Task has no base branch.".to_owned())?;
    let branch_name = task
        .branch_name
        .clone()
        .ok_or_else(|| "Task has no branch.".to_owned())?;
    let remote_client = task
        .ssh_host
        .as_deref()
        .map(|ssh_host| {
            RemoteGitClient::new(ssh_host, native_ssh_control_directory(database_path)?)
                .map_err(|error| error.to_string())
        })
        .transpose()?;
    let changed = if let Some(client) = remote_client.as_ref() {
        client.changed_files(worktree_path, &base_branch, &branch_name)
    } else {
        changed_files(worktree_path, &base_branch, &branch_name)
    }
    .map_err(|error| error.to_string())?;
    let files = changed
        .into_iter()
        .map(|file| {
            let original = if file.is_binary || file.status == DiffFileStatus::Added {
                Ok(String::new())
            } else if let Some(client) = remote_client.as_ref() {
                client
                    .original_file_content(worktree_path, &base_branch, &file.path)
                    .map_err(|error| error.to_string())
            } else {
                original_file_content(worktree_path, &base_branch, &file.path)
                    .map_err(|error| error.to_string())
            }?;
            let current = if file.is_binary || file.status == DiffFileStatus::Deleted {
                Ok(String::new())
            } else if let Some(client) = remote_client.as_ref() {
                client
                    .file_content(worktree_path, &file.path)
                    .map_err(|error| error.to_string())
            } else {
                file_content(worktree_path, &file.path).map_err(|error| error.to_string())
            }?;
            Ok(NativeDiffFileSnapshot {
                path: file.path,
                status: file.status.as_str().to_owned(),
                additions: file.additions,
                deletions: file.deletions,
                is_binary: file.is_binary,
                original,
                current,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(NativeDiffSnapshot {
        task_id: task.id,
        base_branch,
        branch_name,
        files,
    })
}

pub fn save_native_diff_file(
    database_path: impl AsRef<Path>,
    task_id: &str,
    file_path: &str,
    expected_current: &str,
    content: &str,
) -> Result<(), String> {
    let database_path = database_path.as_ref();
    let database = KanvibeDb::open_read_only(database_path).map_err(|error| error.to_string())?;
    let task = database
        .task_by_id(task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Task no longer exists.".to_owned())?;
    let worktree_path = task
        .worktree_path
        .as_deref()
        .ok_or_else(|| "Task has no worktree.".to_owned())?;
    if let Some(ssh_host) = task.ssh_host.as_deref() {
        RemoteGitClient::new(ssh_host, native_ssh_control_directory(database_path)?)
            .map_err(|error| error.to_string())?
            .save_file_content_if_unchanged(worktree_path, file_path, expected_current, content)
            .map_err(|error| error.to_string())
    } else {
        save_file_content_if_unchanged(worktree_path, file_path, expected_current, content)
            .map_err(|error| error.to_string())
    }
}

pub fn update_native_task_status(
    database_path: impl AsRef<Path>,
    task_id: &str,
    status: TaskStatus,
) -> Result<Option<KanbanTask>, String> {
    let database_path = database_path.as_ref();
    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    let Some(update) = database
        .update_task_status(task_id, status)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };

    settle_native_done_cleanup(&database, database_path, update.done_cleanup)?;

    database
        .task_by_id(task_id)
        .map_err(|error| error.to_string())
}

pub fn move_native_task(
    database_path: impl AsRef<Path>,
    task_id: &str,
    status: TaskStatus,
    destination_ordered_ids: &[String],
) -> Result<Option<KanbanTask>, String> {
    let database_path = database_path.as_ref();
    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    let cleanup = database
        .move_task_to_column(task_id, status, destination_ordered_ids)
        .map_err(|error| error.to_string())?;
    settle_native_done_cleanup(&database, database_path, cleanup)?;
    database
        .task_by_id(task_id)
        .map_err(|error| error.to_string())
}

fn settle_native_done_cleanup(
    database: &KanvibeDb,
    database_path: &Path,
    cleanup: Option<DoneCleanupPlan>,
) -> Result<(), String> {
    let Some(cleanup) = cleanup else {
        return Ok(());
    };
    if let Err(cleanup_error) =
        cleanup_task_resources(database, database_path, &cleanup.cleanup_task)
    {
        database
            .finish_done_cleanup(&cleanup, DoneCleanupResult::Failed)
            .map_err(|error| error.to_string())?;
        return Err(format!(
            "Task resource cleanup failed; the Done transition was rolled back. {cleanup_error}"
        ));
    }
    database
        .finish_done_cleanup(&cleanup, DoneCleanupResult::Succeeded)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn cleanup_task_resources(
    database: &KanvibeDb,
    database_path: &Path,
    task: &KanbanTask,
) -> Result<(), String> {
    cleanup_task_resources_with_remote(
        database,
        database_path,
        task,
        |ssh_host, control_directory, project_path, branch_name, worktree_path| {
            let client = RemoteGitClient::new(ssh_host, control_directory)
                .map_err(|error| error.to_string())?;
            client
                .remove_worktree_and_branch(project_path, branch_name, Some(worktree_path))
                .map_err(|error| error.to_string())
        },
    )
}

fn cleanup_task_resources_with_remote(
    database: &KanvibeDb,
    database_path: &Path,
    task: &KanbanTask,
    remote_cleanup: impl FnOnce(&str, PathBuf, &str, &str, &Path) -> Result<(), String>,
) -> Result<(), String> {
    let has_session = task.session_type.is_some() && task.session_name.is_some();
    let has_worktree = task.branch_name.is_some() && task.worktree_path.is_some();
    if !has_session && !has_worktree {
        return Ok(());
    }
    let project = task
        .project_id
        .as_ref()
        .map(|project_id| {
            database
                .projects()
                .map_err(|error| error.to_string())?
                .into_iter()
                .find(|project| project.id == *project_id)
                .ok_or_else(|| "linked project no longer exists".to_owned())
        })
        .transpose()?;
    let ssh_host = task.ssh_host.as_deref().or_else(|| {
        project
            .as_ref()
            .and_then(|project| project.ssh_host.as_deref())
    });
    if let (Some(branch_name), Some(worktree_path)) =
        (task.branch_name.as_deref(), task.worktree_path.as_deref())
    {
        let project = project
            .as_ref()
            .ok_or_else(|| "linked project is required for worktree cleanup".to_owned())?;
        if Path::new(worktree_path) != Path::new(&project.repo_path) {
            if let Some(ssh_host) = ssh_host {
                remote_cleanup(
                    ssh_host,
                    native_ssh_control_directory(database_path)?,
                    &project.repo_path,
                    branch_name,
                    Path::new(worktree_path),
                )?;
            } else {
                remove_worktree_and_branch(
                    &project.repo_path,
                    branch_name,
                    Some(Path::new(worktree_path)),
                )
                .map_err(|error| error.to_string())?;
            }
        }
    }

    if let (Some(session_type), Some(session_name)) =
        (task.session_type, task.session_name.as_deref())
    {
        remove_session_only(session_type, session_name, ssh_host)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
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
            || !scheme.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
            })
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
        let host_port = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
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

pub fn connect_native_task_terminal(
    database_path: impl AsRef<Path>,
    task_id: &str,
    session_type: SessionType,
) -> Result<KanbanTask, String> {
    let database_path = database_path.as_ref();
    let dependency =
        get_native_task_session_dependency_status(database_path, task_id, session_type)?;
    if !dependency.available {
        return Err(format!(
            "{} is not installed on {}. Install it before connecting the terminal.",
            dependency.tool_name, dependency.target
        ));
    }
    let database = KanvibeDb::open_read_write(database_path).map_err(|error| error.to_string())?;
    let task = database
        .task_by_id(task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Task no longer exists.".to_owned())?;
    if task.session_type.is_some()
        && task
            .session_name
            .as_deref()
            .is_some_and(|name| !name.is_empty())
        && task
            .worktree_path
            .as_deref()
            .is_some_and(|path| !path.is_empty())
    {
        return Ok(task);
    }

    let project_id = task
        .project_id
        .as_deref()
        .ok_or_else(|| "Task has no project to connect a terminal.".to_owned())?;
    let project = database
        .project_by_id(project_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Task project no longer exists.".to_owned())?;
    let worktree_path = task
        .worktree_path
        .clone()
        .unwrap_or_else(|| project.repo_path.clone());
    let session_branch = task
        .branch_name
        .as_deref()
        .filter(|branch| !branch.is_empty())
        .unwrap_or(task.title.as_str());
    let session_name = format_session_name(&project.name, session_branch);

    database
        .bind_live_session_if_unassigned(
            task_id,
            session_type,
            &session_name,
            &worktree_path,
            project.ssh_host.as_deref(),
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Task no longer exists.".to_owned())
}

struct RemoteAiSessionDataSource {
    client: RemoteGitClient,
}

impl AiSessionDataSource for RemoteAiSessionDataSource {
    fn is_remote(&self) -> bool {
        true
    }

    fn home_directory(&self) -> Result<PathBuf, AiSessionError> {
        self.client
            .home_directory()
            .map(PathBuf::from)
            .map_err(|error| AiSessionError::new(error.to_string()))
    }

    fn path_exists(&self, path: &Path) -> Result<bool, AiSessionError> {
        let path = remote_ai_path(path)?;
        self.client
            .home_path_exists(path)
            .map_err(|error| AiSessionError::new(error.to_string()))
    }

    fn list_files(
        &self,
        root: &Path,
        suffix: &str,
        recursive: bool,
    ) -> Result<Vec<PathBuf>, AiSessionError> {
        let root = remote_ai_path(root)?;
        self.client
            .list_home_files(root, suffix, recursive)
            .map(|paths| paths.into_iter().map(PathBuf::from).collect())
            .map_err(|error| AiSessionError::new(error.to_string()))
    }

    fn read_text(&self, path: &Path) -> Result<String, AiSessionError> {
        let path = remote_ai_path(path)?;
        self.client
            .read_home_text(path)
            .map_err(|error| AiSessionError::new(error.to_string()))
    }

    fn open_code_sessions(
        &self,
        database_path: &Path,
    ) -> Result<Option<Vec<OpenCodeSessionRow>>, AiSessionError> {
        let database_path = remote_ai_path(database_path)?;
        let output = self
            .client
            .query_open_code_history(database_path, None)
            .map_err(|error| AiSessionError::new(error.to_string()))?;
        parse_remote_open_code_rows(&output)
    }

    fn open_code_messages(
        &self,
        database_path: &Path,
        session_id: &str,
    ) -> Result<Option<Vec<OpenCodeMessageRow>>, AiSessionError> {
        let database_path = remote_ai_path(database_path)?;
        let output = self
            .client
            .query_open_code_history(database_path, Some(session_id))
            .map_err(|error| AiSessionError::new(error.to_string()))?;
        parse_remote_open_code_rows(&output)
    }
}

fn remote_ai_path(path: &Path) -> Result<&str, AiSessionError> {
    path.to_str()
        .ok_or_else(|| AiSessionError::new("remote AI session path must be valid UTF-8"))
}

fn parse_remote_open_code_rows<T>(output: &str) -> Result<Option<Vec<T>>, AiSessionError>
where
    T: for<'de> Deserialize<'de>,
{
    if output.trim() == "__KANVIBE_DB_MISSING__" {
        return Ok(None);
    }
    serde_json::from_str(output)
        .map(Some)
        .map_err(|error| AiSessionError::new(format!("invalid remote OpenCode rows: {error}")))
}

pub fn load_native_task_ai_sessions(
    database_path: impl AsRef<Path>,
    task_id: &str,
    query: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<Option<AiSessionsPage>, String> {
    let database_path = database_path.as_ref();
    let database = KanvibeDb::open_read_only(database_path).map_err(|error| error.to_string())?;
    let Some(task) = database
        .task_by_id(task_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let Some(project_id) = task.project_id.as_deref() else {
        return Ok(None);
    };
    let Some(project) = database
        .project_by_id(project_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let target_path = task.worktree_path.as_deref().unwrap_or(&project.repo_path);
    let mut session_query = AiSessionQuery::new(target_path, &project.repo_path);
    session_query.query = query;
    session_query.cursor = cursor;
    if let Some(limit) = limit {
        session_query.limit = limit.max(1);
    }
    let ssh_host = task.ssh_host.as_deref().or(project.ssh_host.as_deref());
    match ssh_host {
        Some(ssh_host) => {
            let source = RemoteAiSessionDataSource {
                client: RemoteGitClient::new(
                    ssh_host,
                    native_ssh_control_directory(database_path)?,
                )
                .map_err(|error| error.to_string())?,
            };
            read_ai_sessions(&source, &session_query)
                .map(Some)
                .map_err(|error| error.to_string())
        }
        None => read_ai_sessions(&LocalAiSessionDataSource, &session_query)
            .map(Some)
            .map_err(|error| error.to_string()),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "arguments mirror the Electron AI session detail IPC contract"
)]
pub fn load_native_task_ai_session_detail(
    database_path: impl AsRef<Path>,
    task_id: &str,
    provider: AiSessionProvider,
    session_id: &str,
    source_ref: Option<String>,
    query: Option<String>,
    roles: Vec<kanvibe_ai::AiMessageRole>,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<Option<AiSessionDetail>, String> {
    let database_path = database_path.as_ref();
    let database = KanvibeDb::open_read_only(database_path).map_err(|error| error.to_string())?;
    let Some(task) = database
        .task_by_id(task_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let Some(project_id) = task.project_id.as_deref() else {
        return Ok(None);
    };
    let Some(project) = database
        .project_by_id(project_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let target_path = task.worktree_path.as_deref().unwrap_or(&project.repo_path);
    let mut sessions = AiSessionQuery::new(target_path, &project.repo_path);
    sessions.query = query;
    let detail_query = AiDetailQuery {
        sessions,
        provider,
        session_id: session_id.to_owned(),
        source_ref,
        roles,
        cursor,
        limit: limit.unwrap_or(kanvibe_ai::DEFAULT_DETAIL_LIMIT).max(1),
    };
    let ssh_host = task.ssh_host.as_deref().or(project.ssh_host.as_deref());
    match ssh_host {
        Some(ssh_host) => {
            let source = RemoteAiSessionDataSource {
                client: RemoteGitClient::new(
                    ssh_host,
                    native_ssh_control_directory(database_path)?,
                )
                .map_err(|error| error.to_string())?,
            };
            read_ai_session_detail(&source, &detail_query).map_err(|error| error.to_string())
        }
        None => read_ai_session_detail(&LocalAiSessionDataSource, &detail_query)
            .map_err(|error| error.to_string()),
    }
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
        ("hooks", "Hooks", None),
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
    let index = input.key.to_digit(10).map(|digit| digit as usize)?;

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
    use std::{
        io::{Read, Write},
        net::TcpStream,
        path::Path,
        process::Command,
        time::Duration,
    };

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_build_uses_headless_stub() {
        assert_eq!(run(), Ok(RunMode::HeadlessStub));
    }

    #[test]
    fn remote_github_cli_install_only_blocks_non_transport_failures() {
        assert!(!should_block_remote_github_cli_install(
            GitErrorKind::Timeout
        ));
        assert!(!should_block_remote_github_cli_install(
            GitErrorKind::Transport
        ));
        assert!(should_block_remote_github_cli_install(
            GitErrorKind::Command
        ));
        assert!(should_block_remote_github_cli_install(
            GitErrorKind::InvalidInput
        ));
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
                ("hooks", "Cmd+6"),
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
                ("hooks", "Cmd+7"),
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
    fn connect_terminal_persists_session_identity_for_project_tasks() {
        for session_type in [SessionType::Tmux, SessionType::Zellij] {
            native_session_dependency_cache()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(
                    native_session_dependency_cache_key(session_type, None),
                    NativeGitHubCliCacheEntry {
                        checked_at: Instant::now(),
                        available: true,
                        blocked_reason: None,
                    },
                );
        }
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-connect-terminal-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("create terminal fixture directory");
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(
            repo_root.join("qa/seed/kanvibe-seed.sqlite"),
            &database_path,
        )
        .expect("copy terminal fixture");

        let connected = connect_native_task_terminal(
            &database_path,
            "qa-task-pending-no-branch",
            SessionType::Zellij,
        )
        .expect("connect project task terminal");
        assert_eq!(connected.session_type, Some(SessionType::Zellij));
        assert!(
            connected
                .session_name
                .as_deref()
                .is_some_and(|name| !name.is_empty())
        );
        assert!(
            connected
                .worktree_path
                .as_deref()
                .is_some_and(|path| !path.is_empty())
        );

        let database = KanvibeDb::open_read_write(&database_path).expect("open terminal fixture");
        let partial = database
            .create_task(CreateTaskInput {
                title: Some("Partial session".to_owned()),
                session_type: Some(SessionType::Tmux),
                project_id: Some("qa-project-kanvibe".to_owned()),
                ..CreateTaskInput::default()
            })
            .expect("create partial-session task");
        drop(database);
        let repaired =
            connect_native_task_terminal(&database_path, &partial.id, SessionType::Zellij)
                .expect("repair partial session identity");
        assert_eq!(repaired.session_type, Some(SessionType::Zellij));
        assert!(
            repaired
                .session_name
                .as_deref()
                .is_some_and(|name| !name.is_empty())
        );
        assert!(
            repaired
                .worktree_path
                .as_deref()
                .is_some_and(|path| !path.is_empty())
        );

        let database = KanvibeDb::open_read_write(&database_path).expect("open no-project fixture");
        let no_project = database
            .create_task(CreateTaskInput {
                title: Some("No project".to_owned()),
                ..CreateTaskInput::default()
            })
            .expect("create no-project task");
        drop(database);
        assert!(
            connect_native_task_terminal(&database_path, &no_project.id, SessionType::Tmux)
                .expect_err("no-project task must not gain a persistent session")
                .contains("no project")
        );

        std::fs::remove_dir_all(temp_root).expect("remove terminal fixture");
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
    fn native_settings_updates_validate_and_persist_product_preferences() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-native-settings-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("settings temp dir");
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(
            repo_root.join("qa/seed/kanvibe-seed.sqlite"),
            &database_path,
        )
        .expect("copy settings fixture");

        let shell = update_native_settings(
            &database_path,
            Locale::En,
            NativeSettingsPatch {
                theme_preference: Some(ThemePreference::Light),
                default_session_type: Some(SessionType::Zellij),
                task_search_shortcut: Some(" Mod+Shift+K ".to_owned()),
                vim_mode_enabled: Some(false),
                sidebar_default_collapsed: Some(true),
                background_sync_enabled: Some(true),
                background_sync_interval_minutes: Some(15),
                notification_enabled: Some(false),
                notification_statuses: Some(vec!["review".to_owned(), "done".to_owned()]),
            },
        )
        .expect("update settings");
        assert_eq!(shell.theme_preference, ThemePreference::Light);
        assert_eq!(shell.default_session_type, SessionType::Zellij);
        assert_eq!(shell.task_search_shortcut, "Mod+Shift+K");
        assert!(!shell.vim_mode_enabled);
        assert!(shell.sidebar_default_collapsed);
        assert!(shell.background_sync.is_enabled);
        assert_eq!(shell.background_sync.interval_ms, 900_000);
        assert!(!shell.notification_enabled);
        assert_eq!(shell.notification_statuses, ["review", "done"]);

        assert!(
            update_native_settings(
                &database_path,
                Locale::En,
                NativeSettingsPatch {
                    background_sync_interval_minutes: Some(1_441),
                    ..NativeSettingsPatch::default()
                },
            )
            .expect_err("interval above one day must fail")
            .contains("1 and 1440")
        );
        assert!(
            update_native_settings(
                &database_path,
                Locale::En,
                NativeSettingsPatch {
                    task_search_shortcut: Some("Mod+Shift".to_owned()),
                    ..NativeSettingsPatch::default()
                },
            )
            .expect_err("modifier-only shortcut should fail")
            .contains("shortcut")
        );

        let layouts = cycle_native_pane_layout(&database_path, "qa-layout-global-quad")
            .expect("cycle global layout");
        assert_eq!(
            layouts
                .iter()
                .find(|layout| layout.id == "qa-layout-global-quad")
                .expect("global layout")
                .layout_type,
            PaneLayoutType::Single
        );
        let layouts =
            reset_native_project_pane_layout(&database_path, "qa-layout-kanvibe-horizontal")
                .expect("reset project override");
        assert!(
            !layouts
                .iter()
                .any(|layout| layout.id == "qa-layout-kanvibe-horizontal")
        );
        assert!(
            reset_native_project_pane_layout(&database_path, "qa-layout-global-quad")
                .expect_err("global layout cannot reset")
                .contains("global")
        );
        let layouts = update_native_pane_command(
            &database_path,
            "qa-layout-global-quad",
            0,
            "  cargo test  ",
        )
        .expect("update pane command");
        assert_eq!(
            layouts
                .iter()
                .find(|layout| layout.id == "qa-layout-global-quad")
                .expect("global layout after command update")
                .panes[0]
                .command,
            "cargo test"
        );
        assert!(
            update_native_pane_command(&database_path, "qa-layout-global-quad", 9, "invalid")
                .expect_err("invalid pane position")
                .contains("does not exist")
        );
    }

    #[test]
    fn native_shortcut_parser_maps_electron_tokens_to_gpui() {
        assert_eq!(
            native_gpui_keybinding("Mod+Shift+O", ShortcutPlatform::Mac),
            Ok("cmd-shift-o".to_owned())
        );
        assert_eq!(
            native_gpui_keybinding("Mod+Shift+O", ShortcutPlatform::Linux),
            Ok("ctrl-shift-o".to_owned())
        );
        assert_eq!(
            native_gpui_keybinding("Command+Option+Space", ShortcutPlatform::Mac),
            Ok("cmd-alt-space".to_owned())
        );
        assert!(native_gpui_keybinding("Mod+Shift", ShortcutPlatform::Mac).is_err());
        assert!(native_gpui_keybinding("Mod+O+P", ShortcutPlatform::Mac).is_err());
        assert!(native_gpui_keybinding("O", ShortcutPlatform::Mac).is_err());
    }

    #[test]
    fn native_shortcut_capture_formats_physical_key_combinations() {
        assert_eq!(
            native_shortcut_from_capture(NativeShortcutCapture {
                platform: true,
                control: false,
                alt: false,
                shift: true,
                function: false,
                key: "o".to_owned(),
            }),
            Ok("Mod+Shift+O".to_owned())
        );
        assert_eq!(
            native_shortcut_from_capture(NativeShortcutCapture {
                platform: false,
                control: true,
                alt: true,
                shift: false,
                function: false,
                key: "space".to_owned(),
            }),
            Ok("Ctrl+Alt+Space".to_owned())
        );
        assert!(
            native_shortcut_from_capture(NativeShortcutCapture {
                platform: false,
                control: false,
                alt: false,
                shift: true,
                function: false,
                key: "k".to_owned(),
            })
            .expect_err("shift-only captures should be rejected")
            .contains("Command, Control, or Option")
        );
        assert!(
            native_shortcut_from_capture(NativeShortcutCapture {
                platform: true,
                control: false,
                alt: false,
                shift: false,
                function: true,
                key: "f1".to_owned(),
            })
            .expect_err("function modifier is not portable")
            .contains("Fn")
        );
        assert!(
            native_shortcut_from_capture(NativeShortcutCapture {
                platform: true,
                control: false,
                alt: false,
                shift: false,
                function: false,
                key: "command".to_owned(),
            })
            .expect_err("modifier-only capture should fail")
            .contains("non-modifier")
        );
    }

    #[test]
    fn native_theme_resolves_system_appearance_without_overriding_explicit_choice() {
        assert!(native_theme_is_dark(
            ThemePreference::System,
            NativeSystemAppearance::Dark
        ));
        assert!(!native_theme_is_dark(
            ThemePreference::System,
            NativeSystemAppearance::Light
        ));
        assert!(native_theme_is_dark(
            ThemePreference::Dark,
            NativeSystemAppearance::Light
        ));
        assert!(!native_theme_is_dark(
            ThemePreference::Light,
            NativeSystemAppearance::Dark
        ));
    }

    #[test]
    fn native_release_update_selects_latest_stable_safe_release() {
        let releases = r#"[
          {
            "tag_name": "v1.2.0",
            "name": "KanVibe 1.2",
            "body": "Stable notes",
            "html_url": "https://github.com/rookedsysc/kanvibe/releases/tag/v1.2.0",
            "draft": false,
            "prerelease": false,
            "published_at": "2026-07-29T00:00:00Z",
            "assets": [{
              "name": "KanVibe-1.2.0.dmg",
              "browser_download_url": "https://github.com/rookedsysc/kanvibe/releases/download/v1.2.0/KanVibe-1.2.0.dmg",
              "size": 1048576,
              "state": "uploaded",
              "digest": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789ABCDEF"
            }]
          },
          {
            "tag_name": "v2.0.0",
            "name": "Draft",
            "html_url": "https://github.com/rookedsysc/kanvibe/releases/tag/v2.0.0",
            "draft": true,
            "prerelease": false
          },
          {
            "tag_name": "v1.3.0-beta.1",
            "html_url": "https://github.com/rookedsysc/kanvibe/releases/tag/v1.3.0-beta.1",
            "draft": false,
            "prerelease": true
          },
          {
            "tag_name": "v9.0.0",
            "html_url": "https://github.com.evil.test/rookedsysc/kanvibe/releases/tag/v9.0.0",
            "draft": false,
            "prerelease": false
          }
        ]"#;

        let release = select_native_release_update(releases, "1.0.3")
            .expect("valid payload")
            .expect("new release");
        assert_eq!(release.version, "1.2.0");
        assert_eq!(release.name, "KanVibe 1.2");
        assert_eq!(release.body, "Stable notes");
        assert_eq!(
            release.published_at.as_deref(),
            Some("2026-07-29T00:00:00Z")
        );
        assert_eq!(
            release.installer,
            Some(NativeReleaseInstaller {
                asset_name: "KanVibe-1.2.0.dmg".to_owned(),
                download_url:
                    "https://github.com/rookedsysc/kanvibe/releases/download/v1.2.0/KanVibe-1.2.0.dmg"
                        .to_owned(),
                size: 1_048_576,
                sha256:
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        .to_owned(),
            })
        );
    }

    #[test]
    fn native_release_update_rejects_invalid_current_version_and_unsafe_payloads() {
        assert!(select_native_release_update("[]", "development").is_err());
        assert!(select_native_release_update("{}", "1.0.0").is_err());
        assert_eq!(
            select_native_release_update(
                r#"[{"tag_name":"v1.0.0","html_url":"https://github.com/rookedsysc/kanvibe/releases/tag/v1.0.0"}]"#,
                "1.0.0"
            )
            .expect("valid payload"),
            None
        );
    }

    #[test]
    fn native_release_update_disables_ambiguous_or_untrusted_installers() {
        let digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        for assets in [
            format!(
                r#"[{{"name":"KanVibe-1.1.0.dmg","browser_download_url":"https://evil.test/KanVibe-1.1.0.dmg","size":12,"state":"uploaded","digest":"{digest}"}}]"#
            ),
            format!(
                r#"[{{"name":"KanVibe-1.1.0.dmg","browser_download_url":"https://github.com/rookedsysc/kanvibe/releases/download/v1.1.0/KanVibe-1.1.0.dmg","size":0,"state":"uploaded","digest":"{digest}"}}]"#
            ),
            format!(
                r#"[{{"name":"KanVibe-1.1.0.dmg","browser_download_url":"https://github.com/rookedsysc/kanvibe/releases/download/v1.1.0/KanVibe-1.1.0.dmg","size":12,"state":"new","digest":"{digest}"}}]"#
            ),
            r#"[{"name":"KanVibe-1.1.0.dmg","browser_download_url":"https://github.com/rookedsysc/kanvibe/releases/download/v1.1.0/KanVibe-1.1.0.dmg","size":12,"state":"uploaded","digest":"md5:bad"}]"#.to_owned(),
            format!(
                r#"[{{"name":"KanVibe-1.1.0.dmg","browser_download_url":"https://github.com/rookedsysc/kanvibe/releases/download/v1.1.0/KanVibe-1.1.0.dmg","size":12,"state":"uploaded","digest":"{digest}"}},{{"name":"KanVibe-1.1.0.dmg","browser_download_url":"https://github.com/rookedsysc/kanvibe/releases/download/v1.1.0/KanVibe-1.1.0.dmg","size":12,"state":"uploaded","digest":"{digest}"}}]"#
            ),
        ] {
            let payload = format!(
                r#"[{{"tag_name":"v1.1.0","html_url":"https://github.com/rookedsysc/kanvibe/releases/tag/v1.1.0","assets":{assets}}}]"#
            );
            let release = select_native_release_update(&payload, "1.0.3")
                .expect("release payload")
                .expect("release notification remains available");
            assert_eq!(release.installer, None);
        }

        let suffix_attack = select_native_release_update(
            r#"[{"tag_name":"v1.1.0","html_url":"https://github.com/rookedsysc/kanvibe/releases/tag/v1.1.0/attacker"}]"#,
            "1.0.3",
        )
        .expect("valid payload");
        assert_eq!(suffix_attack, None);
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
        let config = NativeUiLaunchConfig::from_scoped_values(
            bundled_launch_paths(),
            LaunchOverrides::default(),
        );

        assert_eq!(
            config.repo_root,
            Path::new("/Applications/KanVibe.app/Contents/Resources")
        );
        assert_eq!(
            config.repo_root.join(Locale::Ko.catalog_relative_path()),
            Path::new("/Applications/KanVibe.app/Contents/Resources/messages/ko.json")
        );
        assert!(!config.database_path.starts_with("/Applications"));
        assert_eq!(
            config.database_path.file_name().expect("db file name"),
            "kanvibe.db"
        );
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
            bundle_resource_root(Path::new(
                "/Applications/KanVibe.app/Contents/MacOS/KanVibe"
            )),
            Some(PathBuf::from(
                "/Applications/KanVibe.app/Contents/Resources"
            ))
        );
        assert_eq!(
            bundle_resource_root(Path::new("/repo/native/target/release/kanvibe-app")),
            None
        );
    }

    #[test]
    fn ensure_database_file_copies_bundled_seed_on_first_launch() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-launch-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let resource_root = temp_root.join("Resources");
        let user_data_dir = temp_root.join("user-data");
        std::fs::create_dir_all(resource_root.join("resources/database")).expect("resource dirs");
        std::fs::copy(
            repo_root.join(BUNDLED_SEED_RELATIVE_PATH),
            resource_root.join(BUNDLED_SEED_RELATIVE_PATH),
        )
        .expect("seed");

        let config = NativeUiLaunchConfig::from_scoped_values(
            LaunchPaths::default(),
            LaunchOverrides {
                repo_root: Some(resource_root),
                app_data_dir: Some(user_data_dir.clone()),
                ..LaunchOverrides::default()
            },
        );

        let database_path = config
            .ensure_database_file()
            .expect("initialize runtime database");

        assert_eq!(
            database_path,
            user_data_dir.join(RUNTIME_DATABASE_FILE_NAME)
        );
        validate_sqlite_database(&database_path).expect("runtime database integrity");
        assert!(database_transition_marker_path(&database_path).exists());
        assert!(!database_transition_backup_path(&database_path).exists());
        let first_bytes = std::fs::read(&database_path).expect("first runtime bytes");
        config
            .ensure_database_file()
            .expect("repeat seed initialization");
        assert_eq!(
            std::fs::read(&database_path).expect("repeat runtime bytes"),
            first_bytes
        );

        std::fs::remove_dir_all(&temp_root).ok();
    }

    #[test]
    fn existing_electron_database_is_backed_up_before_migration_and_snapshot_is_immutable() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-electron-backup-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("backup fixture directory");
        let database_path = temp_root.join(RUNTIME_DATABASE_FILE_NAME);
        std::fs::copy(repo_root.join(BUNDLED_SEED_RELATIVE_PATH), &database_path)
            .expect("existing Electron database");
        let original = rusqlite::Connection::open(&database_path).expect("inspect Electron DB");
        let original_task_count = original
            .query_row("SELECT COUNT(1) FROM kanban_tasks", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("original task count");
        let original_project_count = original
            .query_row("SELECT COUNT(1) FROM projects", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("original project count");
        drop(original);
        let config = NativeUiLaunchConfig {
            repo_root,
            database_path: database_path.clone(),
            locale: Locale::En,
        };

        config
            .ensure_database_file()
            .expect("back up existing Electron database");

        let backup_path = database_transition_backup_path(&database_path);
        validate_sqlite_database(&backup_path).expect("backup integrity");
        assert!(database_transition_marker_path(&database_path).exists());
        let backup = rusqlite::Connection::open(&backup_path).expect("inspect rollback snapshot");
        assert_eq!(
            backup
                .query_row("SELECT COUNT(1) FROM kanban_tasks", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("backup task count"),
            original_task_count
        );
        assert_eq!(
            backup
                .query_row("SELECT COUNT(1) FROM projects", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("backup project count"),
            original_project_count
        );
        drop(backup);
        let migrated =
            rusqlite::Connection::open(&database_path).expect("inspect migrated runtime DB");
        assert_eq!(
            migrated
                .query_row("SELECT COUNT(1) FROM migrations", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("runtime migration count"),
            12
        );
        drop(migrated);
        let backup_bytes = std::fs::read(&backup_path).expect("initial backup bytes");

        let connection = rusqlite::Connection::open(&database_path).expect("mutate runtime DB");
        connection
            .execute(
                "UPDATE app_settings SET value = 'light' WHERE key = 'theme_preference'",
                [],
            )
            .expect("mutate runtime setting");
        drop(connection);
        config
            .ensure_database_file()
            .expect("repeat native transition startup");
        assert_eq!(
            std::fs::read(&backup_path).expect("backup after repeat startup"),
            backup_bytes,
            "the rollback snapshot must never be overwritten"
        );
        std::fs::remove_dir_all(temp_root).ok();
    }

    #[test]
    fn electron_native_electron_rollback_restores_rows_and_preserves_native_safety_snapshot() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-electron-round-trip-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("round-trip fixture directory");
        let database_path = temp_root.join(RUNTIME_DATABASE_FILE_NAME);
        std::fs::copy(repo_root.join(BUNDLED_SEED_RELATIVE_PATH), &database_path)
            .expect("Electron database");
        let config = NativeUiLaunchConfig {
            repo_root,
            database_path: database_path.clone(),
            locale: Locale::En,
        };
        config
            .ensure_database_file()
            .expect("transition Electron database to native");
        let native = rusqlite::Connection::open(&database_path).expect("native database");
        native
            .execute(
                "INSERT INTO kanban_tasks(id, title, status, display_order)
                 VALUES ('native-only-task', 'Native only', 'todo', 999)",
                [],
            )
            .expect("native-only mutation");
        drop(native);

        let native_safety_path = rollback_native_database_to_electron_backup(&database_path)
            .expect("restore Electron snapshot");

        validate_sqlite_database(&database_path).expect("restored Electron integrity");
        validate_sqlite_database(&native_safety_path).expect("native safety integrity");
        let restored = rusqlite::Connection::open(&database_path).expect("restored Electron DB");
        assert_eq!(
            restored
                .query_row(
                    "SELECT COUNT(1) FROM kanban_tasks WHERE id = 'native-only-task'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("restored task count"),
            0
        );
        drop(restored);
        let safety =
            rusqlite::Connection::open(&native_safety_path).expect("native safety snapshot");
        assert_eq!(
            safety
                .query_row(
                    "SELECT COUNT(1) FROM kanban_tasks WHERE id = 'native-only-task'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("safety task count"),
            1
        );
        assert_eq!(
            safety
                .query_row("SELECT COUNT(1) FROM migrations", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("native safety migration count"),
            12
        );
        std::fs::remove_dir_all(temp_root).ok();
    }

    #[test]
    fn corrupt_existing_database_fails_before_creating_transition_artifacts() {
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-corrupt-backup-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("corrupt fixture directory");
        let database_path = temp_root.join(RUNTIME_DATABASE_FILE_NAME);
        let corrupt_bytes = b"not a sqlite database";
        std::fs::write(&database_path, corrupt_bytes).expect("corrupt database");
        let config = NativeUiLaunchConfig {
            repo_root: temp_root.clone(),
            database_path: database_path.clone(),
            locale: Locale::En,
        };

        assert!(
            config
                .ensure_database_file()
                .expect_err("corrupt database must fail")
                .to_string()
                .contains("database")
        );
        assert_eq!(
            std::fs::read(&database_path).expect("corrupt bytes after failure"),
            corrupt_bytes
        );
        assert!(!database_transition_backup_path(&database_path).exists());
        assert!(!database_transition_marker_path(&database_path).exists());
        std::fs::remove_dir_all(temp_root).ok();
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
        assert_eq!(spec.projects.len(), 3);
        assert_eq!(spec.projects[0].name, "KanVibe App");
        assert_eq!(spec.projects[0].default_branch, "main");
        assert!(!spec.projects[0].is_worktree);
        assert_eq!(spec.columns.len(), 5);
        assert_eq!(spec.columns[0].label, "Todo");
        assert_eq!(spec.columns[0].task_count, 3);
        assert_eq!(
            spec.columns[0].first_card_title.as_deref(),
            Some("Draft native board shell")
        );
        let first_card = &spec.columns[0].cards[0];
        assert_eq!(first_card.project_name.as_deref(), Some("KanVibe App"));
        assert_eq!(first_card.project_color.as_deref(), Some("#0064FF"));
        assert_eq!(first_card.agent_type.as_deref(), Some("codex"));
        assert_eq!(first_card.priority.as_deref(), Some("high"));
        assert_eq!(
            first_card.branch_name.as_deref(),
            Some("feat/native-board-shell")
        );
        assert_eq!(first_card.base_branch.as_deref(), Some("main"));
        assert_eq!(first_card.pr_url, None);
        assert_eq!(
            spec.columns
                .iter()
                .map(|column| column.cards.len())
                .sum::<usize>(),
            spec.total_visible_tasks
        );
        let limited = load_read_only_board_with_done_limit(
            &repo_root,
            repo_root.join("qa/seed/kanvibe-seed.sqlite"),
            Locale::En,
            1,
        )
        .expect("limited done board");
        assert_eq!(limited.board.done_limit, 1);
        assert_eq!(
            limited
                .board
                .column(TaskStatus::Done)
                .expect("done column")
                .tasks
                .len(),
            1
        );

        let cards = spec
            .columns
            .iter()
            .flat_map(|column| column.cards.iter())
            .collect::<Vec<_>>();
        let first_card = cards.first().expect("seed card");
        let title_match = filter_native_cards(
            cards.iter().copied(),
            &first_card.title.to_lowercase(),
            None,
        );
        assert!(title_match.iter().any(|card| card.id == first_card.id));
        if let Some(project_id) = &first_card.project_id {
            let project_match =
                filter_native_cards(cards.iter().copied(), "", Some(project_id.as_str()));
            assert!(
                project_match
                    .iter()
                    .all(|card| card.project_id.as_deref() == Some(project_id.as_str()))
            );
        }
        let selected_projects = spec
            .projects
            .iter()
            .take(2)
            .map(|project| project.id.clone())
            .collect::<BTreeSet<_>>();
        let multi_project_match =
            filter_native_cards_by_projects(cards.iter().copied(), "", &selected_projects);
        assert!(multi_project_match.iter().all(|card| {
            card.project_id
                .as_ref()
                .is_some_and(|project_id| selected_projects.contains(project_id))
        }));
        assert_eq!(
            ordered_task_ids_for_drop(["a", "b", "c"], "c", "b"),
            vec!["a".to_owned(), "c".to_owned(), "b".to_owned()]
        );
        let branch_search = search_native_cards(cards.iter().copied(), "native-board-shell");
        assert_eq!(branch_search.len(), 1);
        assert_eq!(branch_search[0].id, "qa-task-todo-local");
    }

    #[test]
    fn unavailable_native_ui_render_spec_keeps_a_routable_error_window_contract() {
        let spec = build_unavailable_native_ui_render_spec(Locale::Zh);

        assert_eq!(spec.route, "/zh");
        assert_eq!(spec.locale, Locale::Zh);
        assert_eq!(spec.columns.len(), TaskStatus::ALL.len());
        assert!(spec.columns.iter().all(|column| column.cards.is_empty()));
        assert_eq!(spec.total_visible_tasks, 0);
        assert_eq!(INITIAL_NATIVE_LOAD_TIMEOUT_MS, 5_000);
    }

    #[test]
    fn native_diagnostics_are_structured_for_startup_and_timeout_failures() {
        let line = native_diagnostic_line(
            "initial-load-timeout",
            "/ko",
            "database remained locked",
            Some(INITIAL_NATIVE_LOAD_TIMEOUT_MS),
        );
        let event: serde_json::Value =
            serde_json::from_str(&line).expect("structured diagnostic JSON");

        assert_eq!(event["source"], "kanvibe-native");
        assert_eq!(event["event"], "initial-load-timeout");
        assert_eq!(event["route"], "/ko");
        assert_eq!(event["message"], "database remained locked");
        assert_eq!(event["timeoutMs"], 5_000);
        assert_eq!(event["version"], NATIVE_APP_VERSION);
        assert_eq!(event["buildCommit"], NATIVE_BUILD_COMMIT);
        assert_eq!(event["pid"], std::process::id());
    }

    #[test]
    fn native_create_task_persists_trimmed_title_and_rejects_blank_input() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-native-create-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("create task temp dir");
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(repo_root.join(BUNDLED_SEED_RELATIVE_PATH), &database_path)
            .expect("copy writable seed");

        let created =
            create_native_task(&database_path, "  Native task  ").expect("create native task");
        assert_eq!(created.title, "Native task");
        assert!(
            create_native_task(&database_path, " \n ")
                .expect_err("blank title must fail")
                .contains("required")
        );
        let updated = update_native_task_title(&database_path, &created.id, " Renamed task ")
            .expect("update native task")
            .expect("updated task exists");
        assert_eq!(updated.title, "Renamed task");
        let progressed =
            update_native_task_status(&database_path, &created.id, TaskStatus::Progress)
                .expect("move native task")
                .expect("progressed task exists");
        assert_eq!(progressed.status, TaskStatus::Progress);
        let done = update_native_task_status(&database_path, &created.id, TaskStatus::Done)
            .expect("finish native task without resources")
            .expect("done task exists");
        assert_eq!(done.status, TaskStatus::Done);
        let reviewed = move_native_task(
            &database_path,
            &created.id,
            TaskStatus::Review,
            std::slice::from_ref(&created.id),
        )
        .expect("drag native task")
        .expect("reviewed task exists");
        assert_eq!(reviewed.status, TaskStatus::Review);

        let database = KanvibeDb::open_read_only(&database_path).expect("reopen native DB");
        assert_eq!(
            database
                .task_by_id(&created.id)
                .expect("read created task")
                .expect("created task exists")
                .title,
            "Renamed task"
        );
        drop(database);
        assert!(delete_native_task(&database_path, &created.id).expect("delete native task"));
        let database = KanvibeDb::open_read_only(&database_path).expect("reopen deleted task DB");
        assert!(
            database
                .task_by_id(&created.id)
                .expect("read deleted task")
                .is_none()
        );
        drop(database);
        std::fs::remove_dir_all(&temp_root).expect("remove task temp dir");
    }

    #[test]
    fn native_hook_http_round_trip_creates_and_updates_without_resource_cleanup() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-native-hook-http-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("create hook temp dir");
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(repo_root.join(BUNDLED_SEED_RELATIVE_PATH), &database_path)
            .expect("copy writable seed");
        let handler_database_path = database_path.clone();
        let server = HookHttpServer::start(
            "127.0.0.1",
            0,
            Arc::new(move |request| handle_native_hook_request(&handler_database_path, request)),
        )
        .expect("start hook server");
        let send_json = |path: &str, body: &str| {
            let mut stream =
                TcpStream::connect(server.local_addr()).expect("connect to hook server");
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .expect("set hook read timeout");
            write!(
                stream,
                "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .expect("send hook request");
            let mut response = String::new();
            stream
                .read_to_string(&mut response)
                .expect("read hook response");
            let (headers, body) = response
                .split_once("\r\n\r\n")
                .expect("HTTP response separator");
            (
                headers.to_owned(),
                serde_json::from_str::<serde_json::Value>(body).expect("JSON body"),
            )
        };

        let (start_headers, start) = send_json(
            "/api/hooks/start",
            r#"{"title":"Hook-created task","agentType":"codex"}"#,
        );
        assert!(start_headers.starts_with("HTTP/1.1 200 OK"));
        assert_eq!(start["success"], true);
        assert_eq!(start["data"]["status"], "todo");
        let task_id = start["data"]["id"].as_str().expect("created task id");
        let connection = rusqlite::Connection::open(&database_path).expect("open hook database");
        connection
            .execute(
                "UPDATE kanban_tasks
                 SET worktree_path = ?1, session_type = 'tmux', session_name = 'hook-session'
                 WHERE id = ?2",
                rusqlite::params![temp_root.join("worktree").to_string_lossy(), task_id],
            )
            .expect("attach resource metadata");
        drop(connection);

        let (status_headers, status) = send_json(
            "/api/hooks/status",
            &serde_json::json!({"taskId": task_id, "status": "DONE"}).to_string(),
        );
        assert!(status_headers.starts_with("HTTP/1.1 200 OK"));
        assert_eq!(status["data"]["status"], "done");
        let database = KanvibeDb::open_read_only(&database_path).expect("inspect hook task");
        let task = database
            .task_by_id(task_id)
            .expect("read hook task")
            .expect("hook task exists");
        assert_eq!(task.status, TaskStatus::Done);
        assert_eq!(task.agent_type.as_deref(), Some("codex"));
        assert_eq!(task.session_name.as_deref(), Some("hook-session"));
        assert!(task.worktree_path.is_some());
        drop(database);
        drop(server);
        std::fs::remove_dir_all(&temp_root).expect("remove hook temp dir");
    }

    #[test]
    fn native_hook_handler_rejects_invalid_json_status_and_missing_tasks() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-native-hook-errors-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("create hook error temp dir");
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(repo_root.join(BUNDLED_SEED_RELATIVE_PATH), &database_path)
            .expect("copy writable seed");

        let malformed = handle_native_hook_request(
            &database_path,
            HookHttpRequest {
                route: HookHttpRoute::Start,
                body: "{".to_owned(),
            },
        );
        assert_eq!(malformed.status_code, 400);
        let invalid_status = handle_native_hook_request(
            &database_path,
            HookHttpRequest {
                route: HookHttpRoute::Status,
                body: r#"{"taskId":"qa-task-todo-local","status":"blocked"}"#.to_owned(),
            },
        );
        assert_eq!(invalid_status.status_code, 400);
        let missing = handle_native_hook_request(
            &database_path,
            HookHttpRequest {
                route: HookHttpRoute::Status,
                body: r#"{"taskId":"missing-task","status":"review"}"#.to_owned(),
            },
        );
        assert_eq!(missing.status_code, 404);

        std::fs::remove_dir_all(&temp_root).expect("remove hook error temp dir");
    }

    #[test]
    fn native_hook_start_creates_worktree_and_installs_all_local_provider_files() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-native-hook-worktree-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let git_root = temp_root.join("repo");
        std::fs::create_dir_all(&git_root).expect("create hook repository");
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&git_root)
                .args(args)
                .output()
                .expect("run Git command");
            assert!(
                output.status.success(),
                "git {:?}: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "qa@example.com"]);
        git(&["config", "user.name", "KanVibe QA"]);
        std::fs::write(git_root.join("README.md"), "hook fixture\n").expect("write fixture");
        git(&["add", "README.md"]);
        git(&["commit", "-m", "fixture"]);

        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(repo_root.join(BUNDLED_SEED_RELATIVE_PATH), &database_path)
            .expect("copy writable seed");
        let database = KanvibeDb::open_read_write(&database_path).expect("open hook database");
        let project = database
            .register_project("Hook Repo", &git_root.to_string_lossy(), "main", None, None)
            .expect("register hook project");
        drop(database);

        let response = handle_native_hook_request(
            &database_path,
            HookHttpRequest {
                route: HookHttpRoute::Start,
                body: serde_json::json!({
                    "title": "Hook worktree",
                    "branchName": "feature/hook-runtime",
                    "sessionType": "tmux",
                    "projectId": project.id,
                })
                .to_string(),
            },
        );
        assert_eq!(response.status_code, 200, "{}", response.json_body);
        let payload: serde_json::Value =
            serde_json::from_str(&response.json_body).expect("parse hook response");
        let task_id = payload["data"]["id"].as_str().expect("created task id");
        let database = KanvibeDb::open_read_only(&database_path).expect("inspect hook database");
        let task = database
            .task_by_id(task_id)
            .expect("read hook task")
            .expect("hook task exists");
        let worktree = Path::new(task.worktree_path.as_deref().expect("worktree path"));
        assert!(worktree.join(".claude/settings.json").is_file());
        assert!(worktree.join(".gemini/settings.json").is_file());
        assert!(worktree.join(".codex/hooks.json").is_file());
        assert!(
            worktree
                .join(".opencode/plugins/kanvibe-plugin.ts")
                .is_file()
        );
        assert!(worktree.join(".kanvibe/targets.json").is_file());
        assert_eq!(
            task.session_name.as_deref(),
            Some("repo-feature-hook-runtime")
        );
        drop(database);

        std::fs::remove_dir_all(&temp_root).expect("remove hook worktree fixture");
    }

    #[test]
    fn native_diff_snapshot_loads_original_and_current_file_content() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-native-diff-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let git_root = temp_root.join("repo");
        std::fs::create_dir_all(&git_root).expect("create git repo");
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&git_root)
                .args(args)
                .output()
                .expect("spawn git");
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "qa@kanvibe.test"]);
        git(&["config", "user.name", "KanVibe QA"]);
        std::fs::write(git_root.join("app.txt"), "original\n").expect("write original");
        git(&["add", "."]);
        git(&["commit", "-m", "initial"]);
        git(&["checkout", "-b", "feat/native-diff"]);
        std::fs::write(git_root.join("app.txt"), "current\n").expect("write current");
        std::fs::write(git_root.join("image.bin"), [0, 159, 146, 150])
            .expect("write binary fixture");
        git(&["add", "image.bin"]);
        git(&["commit", "-m", "binary fixture"]);

        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(repo_root.join(BUNDLED_SEED_RELATIVE_PATH), &database_path)
            .expect("copy seed");
        let task = create_native_task(&database_path, "Native diff").expect("create diff task");
        let connection = rusqlite::Connection::open(&database_path).expect("open seed");
        connection
            .execute(
                "UPDATE kanban_tasks
                 SET worktree_path = ?1, branch_name = 'feat/native-diff', base_branch = 'main',
                     ssh_host = NULL
                 WHERE id = ?2",
                rusqlite::params![git_root.to_string_lossy().as_ref(), task.id],
            )
            .expect("point task at temp repo");
        drop(connection);

        let snapshot =
            load_native_diff_snapshot(&database_path, &task.id).expect("load native diff");
        assert_eq!(snapshot.base_branch, "main");
        assert_eq!(snapshot.branch_name, "feat/native-diff");
        assert_eq!(snapshot.files.len(), 2);
        let text = snapshot
            .files
            .iter()
            .find(|file| file.path == "app.txt")
            .expect("text diff");
        assert_eq!(text.original, "original\n");
        assert_eq!(text.current, "current\n");
        let binary = snapshot
            .files
            .iter()
            .find(|file| file.path == "image.bin")
            .expect("binary diff");
        assert!(binary.is_binary);
        assert!(binary.original.is_empty());
        assert!(binary.current.is_empty());
        save_native_diff_file(&database_path, &task.id, "app.txt", "current\n", "saved\n")
            .expect("save unchanged native diff file");
        assert!(
            save_native_diff_file(&database_path, &task.id, "app.txt", "current\n", "stale\n",)
                .expect_err("external modification conflict")
                .contains("changed on disk")
        );
    }

    #[test]
    fn native_local_project_registration_creates_root_task_and_deletes_atomically() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-native-project-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let git_root = temp_root.join("repo");
        std::fs::create_dir_all(&git_root).expect("create local project");
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&git_root)
                .args(args)
                .output()
                .expect("spawn git");
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "qa@kanvibe.test"]);
        git(&["config", "user.name", "KanVibe QA"]);
        std::fs::write(git_root.join("README.md"), "# project\n").expect("write repo file");
        git(&["add", "."]);
        git(&["commit", "-m", "initial"]);
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(repo_root.join(BUNDLED_SEED_RELATIVE_PATH), &database_path)
            .expect("copy app seed");

        let project = register_native_local_project(&database_path, " Local Project ", &git_root)
            .expect("register local project");
        assert_eq!(project.name, "Local Project");
        assert_eq!(project.default_branch, "main");
        assert_eq!(
            project.color.as_deref(),
            Some(compute_project_color("Local Project"))
        );
        assert!(
            register_native_local_project(&database_path, "Duplicate", &git_root)
                .expect_err("duplicate path must fail")
                .contains("already registered")
        );
        let database = KanvibeDb::open_read_only(&database_path).expect("read project database");
        assert!(
            database
                .board_snapshot(DONE_PAGE_SIZE)
                .expect("board snapshot")
                .columns
                .iter()
                .flat_map(|column| column.tasks.iter())
                .any(
                    |task| task.project_id.as_deref() == Some(project.id.as_str())
                        && task.branch_name.as_deref() == Some("main")
                )
        );
        drop(database);

        assert!(delete_native_project(&database_path, &project.id).expect("delete project"));
        let database = KanvibeDb::open_read_only(&database_path).expect("read deleted project");
        assert!(
            database
                .projects()
                .expect("projects")
                .iter()
                .all(|candidate| candidate.id != project.id)
        );
        assert!(
            database
                .board_snapshot(DONE_PAGE_SIZE)
                .expect("board after delete")
                .columns
                .iter()
                .flat_map(|column| column.tasks.iter())
                .all(|task| task.project_id.as_deref() != Some(project.id.as_str()))
        );
        assert!(
            git_root.exists(),
            "project deletion must not touch Git files"
        );
    }

    #[test]
    fn native_project_scan_registers_nested_repositories_and_skips_existing_paths() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-native-project-scan-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let scan_root = temp_root.join("workspace");
        let repositories = [scan_root.join("team/api"), scan_root.join("tools/cli")];
        for repository in &repositories {
            std::fs::create_dir_all(repository).expect("create scanned repository");
            for arguments in [
                vec!["init", "-b", "main"],
                vec!["config", "user.email", "qa@kanvibe.test"],
                vec!["config", "user.name", "KanVibe QA"],
            ] {
                let output = Command::new("git")
                    .arg("-C")
                    .arg(repository)
                    .args(&arguments)
                    .output()
                    .expect("spawn git");
                assert!(
                    output.status.success(),
                    "git {arguments:?}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::fs::write(repository.join("README.md"), "# scanned\n")
                .expect("write scanned fixture");
            for arguments in [vec!["add", "."], vec!["commit", "-m", "initial"]] {
                let output = Command::new("git")
                    .arg("-C")
                    .arg(repository)
                    .args(&arguments)
                    .output()
                    .expect("spawn git");
                assert!(
                    output.status.success(),
                    "git {arguments:?}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        let linked_worktree = temp_root.join("api__worktrees/feature-review");
        let linked_branch = format!("feature/review-{}", std::process::id());
        let output = Command::new("git")
            .arg("-C")
            .arg(&repositories[0])
            .args([
                "worktree",
                "add",
                "-b",
                &linked_branch,
                linked_worktree.to_str().expect("UTF-8 worktree path"),
            ])
            .output()
            .expect("spawn git worktree");
        assert!(
            output.status.success(),
            "git worktree add: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let linked_worktree =
            std::fs::canonicalize(&linked_worktree).expect("canonicalize linked worktree");
        let live_session_name = format_session_name("api", &linked_branch);
        let tmux = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &live_session_name,
                "-c",
                linked_worktree.to_str().expect("UTF-8 worktree path"),
            ])
            .output()
            .expect("spawn tmux fixture");
        assert!(
            tmux.status.success(),
            "tmux fixture: {}",
            String::from_utf8_lossy(&tmux.stderr)
        );
        std::fs::create_dir_all(linked_worktree.join(".kanvibe")).expect("state directory");
        std::fs::write(
            linked_worktree.join(".kanvibe/status.json"),
            r#"{"schemaVersion":1,"status":"review"}"#,
        )
        .expect("persisted task status");
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(repo_root.join(BUNDLED_SEED_RELATIVE_PATH), &database_path)
            .expect("copy app seed");
        let connection =
            rusqlite::Connection::open(&database_path).expect("open orphan task fixture");
        connection
            .execute(
                "INSERT INTO kanban_tasks (
                    id, title, status, branch_name, worktree_path, display_order
                 ) VALUES ('native-orphan-worktree', 'stale orphan title', 'todo', ?1, ?2, 99)",
                rusqlite::params![linked_branch, linked_worktree.to_string_lossy().as_ref()],
            )
            .expect("insert matching orphan task");
        drop(connection);

        let first = scan_and_register_native_local_projects(&database_path, &scan_root)
            .expect("first scan");
        assert_eq!(first.registered.len(), 2);
        assert!(first.skipped.is_empty());
        assert!(first.errors.is_empty());
        assert_eq!(first.registered_worktrees.len(), 1);
        assert_eq!(first.registered_worktrees[0].branch_name, linked_branch);
        assert_eq!(
            first.registered_worktrees[0].task_id,
            "native-orphan-worktree"
        );
        assert_eq!(
            first
                .registered
                .iter()
                .map(|project| project.name.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["api", "cli"])
        );

        let connection =
            rusqlite::Connection::open(&database_path).expect("open scanned project database");
        connection
            .execute(
                "UPDATE kanban_tasks
                 SET worktree_path = '/stale/worktree', base_branch = 'stale', status = 'todo'
                 WHERE id = ?1",
                [&first.registered_worktrees[0].task_id],
            )
            .expect("corrupt linked task binding");
        drop(connection);

        let second = scan_and_register_native_local_projects(&database_path, &scan_root)
            .expect("repeat scan");
        assert!(second.registered.is_empty());
        assert_eq!(second.skipped.len(), 2);
        assert!(second.registered_worktrees.is_empty());
        assert!(second.errors.is_empty());

        let connection =
            rusqlite::Connection::open(&database_path).expect("open background sync fixture");
        connection
            .execute(
                "UPDATE kanban_tasks SET base_branch = 'stale-again'
                 WHERE id = ?1",
                [&first.registered_worktrees[0].task_id],
            )
            .expect("corrupt task before background sync");
        drop(connection);
        let background =
            sync_native_background_projects(&database_path).expect("background project sync");
        assert_eq!(background.registered_worktrees, 0);
        assert_eq!(background.repaired_tasks, 1);
        assert!(
            background
                .errors
                .iter()
                .all(|error| error.contains("pull request sync failed"))
        );

        let mut merge_event_keys = BTreeSet::new();
        let mut pr_sync = NativeBackgroundSyncRunResult::default();
        sync_native_pull_requests_with(
            &database_path,
            &mut merge_event_keys,
            &mut pr_sync,
            |task, _, _, branch_name| {
                Ok((task.id == first.registered_worktrees[0].task_id
                    && branch_name == linked_branch)
                    .then(|| PullRequestInfo {
                        url: "https://github.com/acme/api/pull/310".to_owned(),
                        state: "MERGED".to_owned(),
                        merged_at: Some("2026-07-29T12:00:00Z".to_owned()),
                        updated_at: Some("2026-07-29T12:00:00Z".to_owned()),
                    }))
            },
        )
        .expect("typed pull request sync");
        assert_eq!(pr_sync.updated_pull_requests, 1);
        assert_eq!(pr_sync.merged_pull_requests.len(), 1);
        let mut repeated_pr_sync = NativeBackgroundSyncRunResult::default();
        sync_native_pull_requests_with(
            &database_path,
            &mut merge_event_keys,
            &mut repeated_pr_sync,
            |task, _, _, branch_name| {
                Ok((task.id == first.registered_worktrees[0].task_id
                    && branch_name == linked_branch)
                    .then(|| PullRequestInfo {
                        url: "https://github.com/acme/api/pull/310".to_owned(),
                        state: "MERGED".to_owned(),
                        merged_at: Some("2026-07-29T12:00:00Z".to_owned()),
                        updated_at: Some("2026-07-29T12:00:00Z".to_owned()),
                    }))
            },
        )
        .expect("repeat typed pull request sync");
        assert_eq!(repeated_pr_sync.updated_pull_requests, 0);
        assert!(repeated_pr_sync.merged_pull_requests.is_empty());

        let mut pull_failure_keys = BTreeSet::new();
        let mut pull_sync = NativeBackgroundSyncRunResult::default();
        sync_native_active_task_pulls_with(
            &database_path,
            &mut pull_failure_keys,
            &mut pull_sync,
            |_, branch_name, _| {
                Ok((branch_name == linked_branch)
                    .then(|| "\nUpdating 1111111..2222222\nFast-forward\n".to_owned()))
            },
        )
        .expect("typed active task pull sync");
        assert_eq!(pull_sync.pulled_tasks.len(), 1);
        assert_eq!(
            pull_sync.pulled_tasks[0].status,
            NativeTaskPullStatus::Updated
        );
        assert_eq!(
            pull_sync.pulled_tasks[0].summary,
            "Updating 1111111..2222222"
        );

        let mut noop_pull_sync = NativeBackgroundSyncRunResult::default();
        sync_native_active_task_pulls_with(
            &database_path,
            &mut pull_failure_keys,
            &mut noop_pull_sync,
            |_, branch_name, _| {
                Ok((branch_name == linked_branch).then(|| "Already up to date.\n".to_owned()))
            },
        )
        .expect("no-op active task pull sync");
        assert!(noop_pull_sync.pulled_tasks.is_empty());

        let mut failed_pull_sync = NativeBackgroundSyncRunResult::default();
        sync_native_active_task_pulls_with(
            &database_path,
            &mut pull_failure_keys,
            &mut failed_pull_sync,
            |_, branch_name, _| {
                if branch_name == linked_branch {
                    Err("network failed".to_owned())
                } else {
                    Ok(None)
                }
            },
        )
        .expect("failed active task pull sync");
        assert_eq!(failed_pull_sync.pulled_tasks.len(), 1);
        assert_eq!(
            failed_pull_sync.pulled_tasks[0].status,
            NativeTaskPullStatus::Failed
        );
        let mut repeated_failed_pull_sync = NativeBackgroundSyncRunResult::default();
        sync_native_active_task_pulls_with(
            &database_path,
            &mut pull_failure_keys,
            &mut repeated_failed_pull_sync,
            |_, branch_name, _| {
                if branch_name == linked_branch {
                    Err("network failed".to_owned())
                } else {
                    Ok(None)
                }
            },
        )
        .expect("repeat failed active task pull sync");
        assert!(repeated_failed_pull_sync.pulled_tasks.is_empty());

        let database = KanvibeDb::open_read_only(&database_path).expect("read scanned projects");
        for project in first.registered {
            assert!(
                database
                    .board_snapshot(DONE_PAGE_SIZE)
                    .expect("scanned board")
                    .columns
                    .iter()
                    .flat_map(|column| column.tasks.iter())
                    .any(|task| task.project_id.as_deref() == Some(project.id.as_str()))
            );
        }
        let linked_task = database
            .task_by_id(&first.registered_worktrees[0].task_id)
            .expect("read linked task")
            .expect("linked worktree task");
        assert_eq!(linked_task.title, linked_branch);
        assert!(linked_task.project_id.is_some());
        assert_eq!(linked_task.status, TaskStatus::Review);
        assert_eq!(
            linked_task.worktree_path.as_deref(),
            Some(linked_worktree.to_string_lossy().as_ref())
        );
        assert_eq!(linked_task.base_branch.as_deref(), Some("main"));
        assert_eq!(linked_task.session_type, Some(SessionType::Tmux));
        assert_eq!(
            linked_task.session_name.as_deref(),
            Some(live_session_name.as_str())
        );
        let ignored = Command::new("git")
            .arg("-C")
            .arg(&linked_worktree)
            .args(["check-ignore", ".kanvibe/status.json"])
            .output()
            .expect("check persisted state exclusion");
        assert!(
            ignored.status.success(),
            ".kanvibe state must not pollute Git status: {}",
            String::from_utf8_lossy(&ignored.stderr)
        );
        let exclude = std::fs::read_to_string(repositories[0].join(".git/info/exclude"))
            .expect("read Git exclude");
        assert_eq!(
            exclude
                .lines()
                .filter(|line| line.trim() == ".kanvibe/")
                .count(),
            1,
            "repeated synchronization must keep the exclude entry idempotent"
        );
        let tmux_cleanup = Command::new("tmux")
            .args(["kill-session", "-t", &live_session_name])
            .output()
            .expect("stop tmux fixture");
        assert!(tmux_cleanup.status.success());
        std::fs::remove_dir_all(temp_root).expect("remove project scan fixture");
    }

    #[test]
    fn native_remote_project_sync_persists_host_and_is_idempotent() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-native-remote-sync-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("create remote sync fixture");
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(repo_root.join(BUNDLED_SEED_RELATIVE_PATH), &database_path)
            .expect("copy app seed");
        let project = persist_native_remote_project(
            &database_path,
            " Remote API ",
            "/remote/api",
            "main",
            "remote-host",
        )
        .expect("persist remote project");
        assert!(
            persist_native_remote_project(
                &database_path,
                "Duplicate Remote API",
                "/remote/api",
                "main",
                "remote-host",
            )
            .expect_err("same remote host and path must be unique")
            .contains("already registered")
        );
        let connection =
            rusqlite::Connection::open(&database_path).expect("open remote orphan fixture");
        connection
            .execute(
                "INSERT INTO kanban_tasks (
                    id, title, status, branch_name, worktree_path, display_order
                 ) VALUES (
                    'local-orphan-at-remote-path', 'local orphan', 'todo',
                    'feature/sync', '/remote/api__worktrees/feature-sync', 98
                 )",
                [],
            )
            .expect("insert same-path local orphan");
        drop(connection);
        let worktrees = vec![
            kanvibe_git::RegisteredWorktree {
                path: PathBuf::from("/remote/api"),
                branch: Some("main".to_owned()),
                is_bare: false,
            },
            kanvibe_git::RegisteredWorktree {
                path: PathBuf::from("/remote/api__worktrees/feature-sync"),
                branch: Some("feature/sync".to_owned()),
                is_bare: false,
            },
        ];

        let first =
            sync_native_remote_worktree_snapshot(&database_path, &project, worktrees.clone());
        assert!(first.errors.is_empty());
        assert_eq!(first.registered_worktrees.len(), 1);
        let second = sync_native_remote_worktree_snapshot(&database_path, &project, worktrees);
        assert!(second.errors.is_empty());
        assert!(second.registered_worktrees.is_empty());
        assert_eq!(second.repaired_tasks, 0);

        let database = KanvibeDb::open_read_only(&database_path).expect("read remote sync result");
        let task = database
            .task_by_project_branch(&project.id, "feature/sync")
            .expect("read remote task")
            .expect("remote worktree task");
        assert_eq!(
            task.worktree_path.as_deref(),
            Some("/remote/api__worktrees/feature-sync")
        );
        assert_eq!(task.ssh_host.as_deref(), Some("remote-host"));
        assert_eq!(task.base_branch.as_deref(), Some("main"));
        let local_orphan = database
            .task_by_id("local-orphan-at-remote-path")
            .expect("read local orphan")
            .expect("local orphan remains");
        assert!(local_orphan.project_id.is_none());
        assert!(local_orphan.ssh_host.is_none());
        drop(database);
        std::fs::remove_dir_all(temp_root).expect("remove remote sync fixture");
    }

    #[test]
    fn native_remote_cleanup_routes_exact_identity_and_propagates_failure() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-native-remote-cleanup-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("create remote cleanup fixture");
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(repo_root.join(BUNDLED_SEED_RELATIVE_PATH), &database_path)
            .expect("copy app seed");
        let project = persist_native_remote_project(
            &database_path,
            "Remote Cleanup",
            "/remote/repo",
            "main",
            "remote-host",
        )
        .expect("persist cleanup project");
        let connection = rusqlite::Connection::open(&database_path).expect("open cleanup fixture");
        connection
            .execute(
                "UPDATE kanban_tasks
                 SET branch_name = 'feature/cleanup',
                     worktree_path = '/remote/repo__worktrees/feature-cleanup',
                     session_type = NULL,
                     session_name = NULL
                 WHERE project_id = ?1 AND branch_name = 'main'",
                [&project.id],
            )
            .expect("prepare remote cleanup task");
        drop(connection);
        let database = KanvibeDb::open_read_only(&database_path).expect("read cleanup fixture");
        let task = database
            .task_by_project_branch(&project.id, "feature/cleanup")
            .expect("read cleanup task")
            .expect("cleanup task exists");
        let mut invoked = false;

        let error = cleanup_task_resources_with_remote(
            &database,
            &database_path,
            &task,
            |ssh_host, control_directory, project_path, branch_name, worktree_path| {
                invoked = true;
                assert_eq!(ssh_host, "remote-host");
                assert_eq!(control_directory, temp_root.join("ssh-control"));
                assert_eq!(project_path, "/remote/repo");
                assert_eq!(branch_name, "feature/cleanup");
                assert_eq!(
                    worktree_path,
                    Path::new("/remote/repo__worktrees/feature-cleanup")
                );
                Err("remote transport unavailable".to_owned())
            },
        )
        .expect_err("remote cleanup failure must propagate");

        assert!(invoked);
        assert_eq!(error, "remote transport unavailable");
        assert!(
            database
                .task_by_id(&task.id)
                .expect("read task after cleanup failure")
                .is_some(),
            "cleanup failure must not mutate the task row"
        );
        drop(database);
        std::fs::remove_dir_all(temp_root).expect("remove remote cleanup fixture");
    }

    #[test]
    fn native_project_color_update_validates_and_updates_main_and_worktree_rows() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-native-project-color-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("create project color temp dir");
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(repo_root.join(BUNDLED_SEED_RELATIVE_PATH), &database_path)
            .expect("copy writable seed");
        let project_id = "native-project-color-main";
        let project_path = temp_root.join("project");
        let worktree_id = "native-project-color-worktree";
        let worktree_path = format!("{}__worktrees/color", project_path.to_string_lossy());
        let connection =
            rusqlite::Connection::open(&database_path).expect("open project color fixture");
        connection
            .execute(
                "INSERT INTO projects (
                    id, name, repo_path, default_branch, color, is_worktree
                 ) VALUES (?1, 'Color project', ?2, 'main', '#000000', 0)",
                rusqlite::params![project_id, project_path.to_string_lossy()],
            )
            .expect("insert main project");
        connection
            .execute(
                "INSERT INTO projects (
                    id, name, repo_path, default_branch, color, is_worktree
                 ) VALUES (?1, 'Color worktree', ?2, 'main', '#000000', 1)",
                rusqlite::params![worktree_id, worktree_path],
            )
            .expect("insert related worktree");
        drop(connection);

        update_native_project_color(&database_path, project_id, "#5eead4")
            .expect("update project color");

        let database =
            KanvibeDb::open_read_only(&database_path).expect("reopen project color database");
        assert_eq!(
            database
                .project_by_id(project_id)
                .expect("read main project")
                .expect("main project")
                .color
                .as_deref(),
            Some("#5EEAD4")
        );
        assert_eq!(
            database
                .project_by_id(worktree_id)
                .expect("read worktree project")
                .expect("worktree project")
                .color
                .as_deref(),
            Some("#5EEAD4")
        );
        drop(database);

        let error = update_native_project_color(&database_path, project_id, "red")
            .expect_err("invalid project color must fail");
        assert!(error.contains("#RRGGBB"));
        let database =
            KanvibeDb::open_read_only(&database_path).expect("reopen after invalid color");
        assert_eq!(
            database
                .project_by_id(project_id)
                .expect("read unchanged project")
                .expect("unchanged project")
                .color
                .as_deref(),
            Some("#5EEAD4")
        );
        drop(database);
        assert_eq!(
            update_native_project_color(&database_path, "missing-project", "#93C5FD")
                .expect_err("missing project must fail"),
            "Project no longer exists."
        );

        std::fs::remove_dir_all(temp_root).expect("remove project color fixture");
    }

    #[test]
    fn native_task_form_persists_electron_create_fields_and_editable_metadata() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-native-task-form-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("create form temp dir");
        let git_root = temp_root.join("native-project");
        std::fs::create_dir_all(&git_root).expect("create form repository");
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&git_root)
                .args(args)
                .output()
                .expect("run form Git command");
            assert!(
                output.status.success(),
                "git {:?}: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "qa@example.com"]);
        git(&["config", "user.name", "KanVibe QA"]);
        std::fs::write(git_root.join("README.md"), "native form fixture\n")
            .expect("write form fixture");
        git(&["add", "README.md"]);
        git(&["commit", "-m", "fixture"]);
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(repo_root.join(BUNDLED_SEED_RELATIVE_PATH), &database_path)
            .expect("copy writable seed");
        let project_id = "native-project".to_owned();
        let connection = rusqlite::Connection::open(&database_path).expect("open form database");
        connection
            .execute(
                "INSERT INTO projects (id, name, repo_path, default_branch, is_worktree)
                 VALUES (?1, 'Native Project', ?2, 'main', 0)",
                rusqlite::params![&project_id, git_root.to_string_lossy()],
            )
            .expect("insert form project");
        drop(connection);

        let database =
            KanvibeDb::open_read_write(&database_path).expect("open branch-from-task database");
        let existing = database
            .create_task(CreateTaskInput {
                title: Some("Existing backlog task".to_owned()),
                project_id: Some(project_id.clone()),
                ..CreateTaskInput::default()
            })
            .expect("create existing task without branch");
        drop(database);
        let branched = branch_native_task_from_form(
            &database_path,
            &existing.id,
            NativeTaskFormInput {
                branch_name: "feature/from-existing".to_owned(),
                base_branch: "main".to_owned(),
                session_type: SessionType::Tmux,
                project_id: project_id.clone(),
                ..NativeTaskFormInput::default()
            },
        )
        .expect("create branch from existing task");
        assert_eq!(branched.id, existing.id);
        assert_eq!(branched.title, "Existing backlog task");
        assert_eq!(branched.status, TaskStatus::Progress);
        assert_eq!(
            branched.branch_name.as_deref(),
            Some("feature/from-existing")
        );
        assert!(
            branched
                .worktree_path
                .as_deref()
                .is_some_and(|path| Path::new(path).is_dir())
        );
        assert!(
            std::fs::read_to_string(
                Path::new(
                    branched
                        .worktree_path
                        .as_deref()
                        .expect("branched worktree")
                )
                .join(".kanvibe/targets.json")
            )
            .expect("existing-task provider binding")
            .contains(&existing.id)
        );

        let created = create_native_task_from_form(
            &database_path,
            NativeTaskFormInput {
                branch_name: " feature/native-form ".to_owned(),
                description: " Native description ".to_owned(),
                base_branch: " main ".to_owned(),
                session_type: SessionType::Zellij,
                project_id: project_id.clone(),
                priority: Some(TaskPriority::High),
            },
        )
        .expect("create full native task");
        assert_eq!(created.title, "feature/native-form");
        assert_eq!(created.branch_name.as_deref(), Some("feature/native-form"));
        assert_eq!(created.description.as_deref(), Some("Native description"));
        assert_eq!(created.base_branch.as_deref(), Some("main"));
        assert_eq!(created.session_type, Some(SessionType::Zellij));
        assert_eq!(
            created.session_name.as_deref(),
            Some("native-project-feature-native-form")
        );
        let worktree_path = created
            .worktree_path
            .as_deref()
            .expect("branch form creates worktree");
        assert!(Path::new(worktree_path).is_dir());
        assert!(
            std::fs::read_to_string(Path::new(worktree_path).join(".kanvibe/targets.json"))
                .expect("provider task binding")
                .contains(&created.id)
        );
        assert_eq!(created.project_id.as_deref(), Some(project_id.as_str()));
        assert_eq!(created.priority, Some(TaskPriority::High));

        let updated = update_native_task_metadata(
            &database_path,
            &created.id,
            " Renamed native task ",
            " ",
            Some(TaskPriority::Medium),
        )
        .expect("update native metadata")
        .expect("updated task");
        assert_eq!(updated.title, "Renamed native task");
        assert_eq!(updated.description, None);
        assert_eq!(updated.priority, Some(TaskPriority::Medium));

        assert!(
            create_native_task_from_form(
                &database_path,
                NativeTaskFormInput {
                    branch_name: " ".to_owned(),
                    project_id,
                    ..NativeTaskFormInput::default()
                },
            )
            .expect_err("blank branch must fail")
            .contains("Branch name")
        );

        let connection =
            rusqlite::Connection::open(&database_path).expect("open rollback database");
        connection
            .execute_batch(
                "CREATE TRIGGER reject_native_form_task
                 BEFORE INSERT ON kanban_tasks
                 WHEN NEW.branch_name = 'feature/rollback'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected native form failure');
                 END;",
            )
            .expect("install rollback trigger");
        drop(connection);
        let rollback_error = create_native_task_from_form(
            &database_path,
            NativeTaskFormInput {
                branch_name: "feature/rollback".to_owned(),
                base_branch: "main".to_owned(),
                session_type: SessionType::Tmux,
                project_id: "native-project".to_owned(),
                ..NativeTaskFormInput::default()
            },
        )
        .expect_err("injected DB failure must surface");
        assert!(rollback_error.contains("injected native form failure"));
        let rolled_back_worktree =
            kanvibe_git::build_managed_worktree_path(&git_root, "feature/rollback");
        assert!(!rolled_back_worktree.exists());
        assert!(
            !kanvibe_git::list_branches(&git_root)
                .expect("branches after rollback")
                .contains(&"feature/rollback".to_owned())
        );
        assert!(
            KanvibeDb::open_read_only(&database_path)
                .expect("open rollback verification database")
                .task_by_project_branch("native-project", "feature/rollback")
                .expect("query rollback task")
                .is_none()
        );
        std::fs::remove_dir_all(&temp_root).expect("remove form temp dir");
    }

    #[test]
    fn native_routes_match_electron_locale_and_task_paths() {
        assert_eq!(
            parse_native_route("/"),
            NativeRoute::Board { locale: Locale::Ko }
        );
        assert_eq!(
            parse_native_route("/en/settings"),
            NativeRoute::Settings { locale: Locale::En }
        );
        assert_eq!(
            parse_native_route("/zh/pane-layout"),
            NativeRoute::PaneLayout { locale: Locale::Zh }
        );
        assert_eq!(
            parse_native_route("/en/task/task-1"),
            NativeRoute::TaskDetail {
                locale: Locale::En,
                task_id: "task-1".to_owned(),
            }
        );
        assert_eq!(
            parse_native_route("/en/task/task-1/diff"),
            NativeRoute::Diff {
                locale: Locale::En,
                task_id: "task-1".to_owned(),
            }
        );
        assert_eq!(
            parse_native_route("/fr/settings"),
            NativeRoute::Board { locale: Locale::Ko }
        );
        assert_eq!(
            parse_native_route("/en/unknown"),
            NativeRoute::NotFound {
                locale: Locale::En,
                path: "/en/unknown".to_owned(),
            }
        );
    }

    #[test]
    fn native_vim_move_command_accepts_colon_and_rejects_unknown_status() {
        assert_eq!(
            parse_native_vim_command(":move progress"),
            Ok(NativeVimCommand::Move(TaskStatus::Progress))
        );
        assert!(parse_native_vim_command(":move blocked").is_err());
        assert!(parse_native_vim_command(":delete").is_err());
        assert_eq!(complete_native_vim_command(":"), ":move ");
        assert_eq!(complete_native_vim_command(":mov"), ":move ");
        assert_eq!(complete_native_vim_command(":move pro"), ":move progress");
        assert_eq!(
            complete_native_vim_command(":move cancelled"),
            ":move cancelled"
        );
        assert_eq!(complete_native_vim_command(":delete"), ":delete");
    }

    #[test]
    fn native_navigation_history_truncates_forward_entries_after_new_navigation() {
        let mut history = NativeNavigationHistory::new(NativeRoute::Board { locale: Locale::Ko });
        assert!(!history.can_go_back());
        assert!(!history.can_go_forward());

        history.navigate(NativeRoute::Settings { locale: Locale::Ko });
        history.navigate(NativeRoute::PaneLayout { locale: Locale::Ko });
        assert_eq!(
            history.go_back(),
            Some(&NativeRoute::Settings { locale: Locale::Ko })
        );
        assert!(history.can_go_forward());

        history.navigate(NativeRoute::TaskDetail {
            locale: Locale::Ko,
            task_id: "task-1".to_owned(),
        });
        assert!(!history.can_go_forward());
        assert_eq!(
            history.current(),
            &NativeRoute::TaskDetail {
                locale: Locale::Ko,
                task_id: "task-1".to_owned(),
            }
        );
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn background_sync_reconfigures_one_worker_and_joins_on_drop() {
        struct WorkerLifetime(Arc<std::sync::atomic::AtomicBool>);

        impl Drop for WorkerLifetime {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }

        let completed = Arc::new(AtomicU64::new(0));
        let callback_completed = Arc::clone(&completed);
        let worker_dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let lifetime = WorkerLifetime(Arc::clone(&worker_dropped));
        let service = NativeBackgroundSyncService::start(
            BackgroundSyncSettings {
                is_enabled: false,
                interval_ms: 60_000,
            },
            move || {
                let _lifetime = &lifetime;
                callback_completed.fetch_add(1, Ordering::AcqRel);
                Ok(NativeBackgroundSyncRunResult::default())
            },
        )
        .expect("start background sync");

        assert_eq!(service.snapshot().worker_start_count, 1);
        service.reconfigure(BackgroundSyncSettings {
            is_enabled: true,
            interval_ms: 60_000,
        });
        service.reconfigure(BackgroundSyncSettings {
            is_enabled: true,
            interval_ms: 30_000,
        });
        service.trigger_now();

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while completed.load(Ordering::Acquire) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(completed.load(Ordering::Acquire), 1);
        assert_eq!(service.snapshot().worker_start_count, 1);
        assert_eq!(service.snapshot().completed_run_count, 1);
        assert_eq!(
            service.snapshot().last_result,
            Some(Ok(NativeBackgroundSyncRunResult::default()))
        );

        service.reconfigure(BackgroundSyncSettings {
            is_enabled: false,
            interval_ms: 30_000,
        });
        service.trigger_now();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(completed.load(Ordering::Acquire), 1);

        drop(service);
        assert!(worker_dropped.load(Ordering::Acquire));
    }

    #[test]
    fn background_sync_review_accumulates_unique_events_until_acknowledged() {
        let merged = NativeMergedPullRequest {
            task_id: "task-310".to_owned(),
            task_title: "Native migration".to_owned(),
            branch_name: "perf/rust-backend".to_owned(),
            pr_url: "https://github.com/rookedsysc/kanvibe/pull/310".to_owned(),
            merged_at: "2026-07-29T12:00:00Z".to_owned(),
        };
        let pull = NativeTaskPullSync {
            task_id: "task-311".to_owned(),
            task_title: "Pull update".to_owned(),
            branch_name: "feature/pull".to_owned(),
            worktree_path: "/workspace/pull".to_owned(),
            ssh_host: None,
            status: NativeTaskPullStatus::Updated,
            summary: "Fast-forward".to_owned(),
        };
        let run = NativeBackgroundSyncRunResult {
            registered_worktrees: 1,
            merged_pull_requests: vec![merged.clone()],
            pulled_tasks: vec![pull.clone()],
            errors: vec!["one failure".to_owned()],
            ..NativeBackgroundSyncRunResult::default()
        };
        let mut pending = NativeBackgroundSyncRunResult::default();

        merge_native_background_review(&mut pending, &run);
        merge_native_background_review(&mut pending, &run);

        assert_eq!(pending.registered_worktrees, 2);
        assert_eq!(pending.merged_pull_requests, [merged]);
        assert_eq!(pending.pulled_tasks, [pull]);
        assert_eq!(pending.errors, ["one failure"]);
        assert!(pending.needs_review());
    }

    #[derive(Default)]
    struct FakeNotificationPlatform {
        delivered_ids: Mutex<Vec<String>>,
        activation: Mutex<Option<NativeNotificationActivationSink>>,
        status: Mutex<NativeNotificationDeliveryStatus>,
    }

    impl NativeNotificationPlatform for FakeNotificationPlatform {
        fn deliver(
            &self,
            notification: &kanvibe_hooks::AppNotification,
            activation: NativeNotificationActivationSink,
        ) -> Result<NativeNotificationDeliveryStatus, String> {
            self.delivered_ids
                .lock()
                .expect("delivery lock")
                .push(notification.id.clone());
            *self.activation.lock().expect("activation lock") = Some(activation);
            Ok(*self.status.lock().expect("status lock"))
        }
    }

    #[test]
    fn native_notification_service_persists_delivers_and_consumes_activation_once() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-native-notifications-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("create notification temp dir");
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(
            repo_root.join("qa/seed/kanvibe-seed.sqlite"),
            &database_path,
        )
        .expect("copy notification fixture");

        let revision = Arc::new(AtomicU64::new(0));
        let platform = Arc::new(FakeNotificationPlatform::default());
        let service =
            NativeNotificationService::new(&database_path, Arc::clone(&revision), platform.clone());
        let published = service
            .publish(kanvibe_hooks::NotificationDraft {
                title: "Task moved".to_owned(),
                body: "Native migration moved to review".to_owned(),
                task_id: Some("task-310".to_owned()),
                relative_path: None,
                locale: "en".to_owned(),
                dedupe_key: "task-310::review".to_owned(),
                action: None,
            })
            .expect("publish notification");

        assert!(published.creation.created);
        assert_eq!(
            published.delivery,
            NativeNotificationDeliveryStatus::Delivered
        );
        assert_eq!(revision.load(Ordering::Acquire), 1);
        assert_eq!(
            platform
                .delivered_ids
                .lock()
                .expect("delivery lock")
                .as_slice(),
            [published.creation.notification.id.as_str()]
        );

        platform
            .activation
            .lock()
            .expect("activation lock")
            .clone()
            .expect("activation sink")
            .activate(&published.creation.notification.id)
            .expect("activate notification");
        assert_eq!(revision.load(Ordering::Acquire), 2);
        let activated = service
            .consume_activation()
            .expect("consume activation")
            .expect("activated notification");
        assert_eq!(activated.id, published.creation.notification.id);
        assert!(activated.is_read);
        assert!(
            service
                .consume_activation()
                .expect("consume once")
                .is_none()
        );

        std::fs::remove_dir_all(temp_root).expect("remove notification temp dir");
    }

    #[test]
    fn unsupported_native_delivery_keeps_the_persisted_notification() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-unsupported-notifications-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("create notification temp dir");
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(
            repo_root.join("qa/seed/kanvibe-seed.sqlite"),
            &database_path,
        )
        .expect("copy notification fixture");

        let platform = Arc::new(FakeNotificationPlatform {
            status: Mutex::new(NativeNotificationDeliveryStatus::Unsupported),
            ..FakeNotificationPlatform::default()
        });
        let service =
            NativeNotificationService::new(&database_path, Arc::new(AtomicU64::new(0)), platform);
        let published = service
            .publish(kanvibe_hooks::NotificationDraft {
                title: "Stored".to_owned(),
                body: "Still visible in app".to_owned(),
                task_id: None,
                relative_path: Some("/en".to_owned()),
                locale: "en".to_owned(),
                dedupe_key: "stored".to_owned(),
                action: None,
            })
            .expect("persist unsupported notification");

        assert_eq!(
            published.delivery,
            NativeNotificationDeliveryStatus::Unsupported
        );
        assert_eq!(service.list().expect("list persisted").len(), 1);
        std::fs::remove_dir_all(temp_root).expect("remove notification temp dir");
    }

    #[test]
    fn background_sync_notification_draft_is_localized_and_stably_deduplicated() {
        let result = NativeBackgroundSyncRunResult {
            registered_worktrees: 2,
            merged_pull_requests: vec![NativeMergedPullRequest {
                task_id: "task-310".to_owned(),
                task_title: "Native migration".to_owned(),
                branch_name: "perf/rust-backend".to_owned(),
                pr_url: "https://github.com/rookedsysc/kanvibe/pull/310".to_owned(),
                merged_at: "2026-07-29T12:00:00Z".to_owned(),
            }],
            pulled_tasks: vec![NativeTaskPullSync {
                task_id: "task-311".to_owned(),
                task_title: "Pull update".to_owned(),
                branch_name: "feature/pull".to_owned(),
                worktree_path: "/workspace/pull".to_owned(),
                ssh_host: None,
                status: NativeTaskPullStatus::Failed,
                summary: "conflict".to_owned(),
            }],
            errors: vec!["sync unavailable".to_owned()],
            ..NativeBackgroundSyncRunResult::default()
        };

        let first = background_sync_notification_draft(&result, Locale::Ko);
        let second = background_sync_notification_draft(&result, Locale::Ko);

        assert_eq!(first, second);
        assert_eq!(first.title, "백그라운드 sync 검토 필요");
        assert!(first.body.contains("merge된 PR 1"));
        assert!(first.body.contains("새 TODO worktree 2"));
        assert!(first.body.contains("pull 실패 1"));
        assert_eq!(first.relative_path.as_deref(), Some("/ko"));
        assert_eq!(
            first
                .action
                .as_ref()
                .and_then(|action| action.get("type"))
                .and_then(serde_json::Value::as_str),
            Some("background-sync-review")
        );
    }

    #[test]
    fn hook_status_notification_honors_enabled_statuses_and_task_route() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-hook-notifications-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("create notification temp dir");
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(
            repo_root.join("qa/seed/kanvibe-seed.sqlite"),
            &database_path,
        )
        .expect("copy notification fixture");
        let platform = Arc::new(FakeNotificationPlatform::default());
        let service = NativeNotificationService::new(
            &database_path,
            Arc::new(AtomicU64::new(0)),
            platform.clone(),
        );

        publish_hook_status_notification(
            &database_path,
            r#"{"taskId":"qa-task-progress-terminal","status":"progress"}"#,
            &service,
            Locale::En,
        )
        .expect("publish enabled status");
        publish_hook_status_notification(
            &database_path,
            r#"{"taskId":"qa-task-todo-local","status":"todo"}"#,
            &service,
            Locale::En,
        )
        .expect("skip disabled status");

        let notifications = service.list().expect("list hook notifications");
        assert_eq!(notifications.len(), 1);
        assert_eq!(
            notifications[0].task_id.as_deref(),
            Some("qa-task-progress-terminal")
        );
        assert_eq!(
            notifications[0].relative_path,
            "/en/task/qa-task-progress-terminal"
        );
        assert_eq!(
            platform.delivered_ids.lock().expect("delivery lock").len(),
            1
        );
        std::fs::remove_dir_all(temp_root).expect("remove notification temp dir");
    }

    #[test]
    fn native_task_hook_service_installs_and_reports_all_provider_contracts() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-task-hooks-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let worktree_path = temp_root.join("repo");
        std::fs::create_dir_all(&worktree_path).expect("create hook worktree");
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(
            repo_root.join("qa/seed/kanvibe-seed.sqlite"),
            &database_path,
        )
        .expect("copy hook fixture");
        let database = KanvibeDb::open_read_write(&database_path).expect("open hook fixture");
        let task = database
            .create_task(CreateTaskInput {
                title: Some("Provider hook parity".to_owned()),
                worktree_path: Some(worktree_path.to_string_lossy().into_owned()),
                ..CreateTaskInput::default()
            })
            .expect("create hook task");
        drop(database);

        let statuses =
            install_native_task_hooks(&database_path, &task.id).expect("install task hooks");
        assert_eq!(statuses.len(), kanvibe_hooks::AI_PROVIDERS.len());
        assert!(statuses.iter().all(|status| status.installed));
        assert!(
            statuses
                .iter()
                .all(|status| status.has_expected_hook_server_url)
        );
        assert!(
            statuses
                .iter()
                .all(|status| !status.has_reachable_hook_server)
        );
        std::fs::remove_dir_all(temp_root).expect("remove hook fixture");
    }

    #[test]
    fn pane_layout_type_save_creates_missing_project_override_directly() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-pane-direct-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&temp_root).expect("create pane fixture");
        let database_path = temp_root.join("kanvibe.db");
        std::fs::copy(
            repo_root.join("qa/seed/kanvibe-seed.sqlite"),
            &database_path,
        )
        .expect("copy pane fixture");

        let layouts = save_native_pane_layout_type(
            &database_path,
            None,
            Some("qa-project-api"),
            false,
            PaneLayoutType::Vertical2,
        )
        .expect("save direct pane layout");
        let created = layouts
            .iter()
            .find(|layout| layout.project_id.as_deref() == Some("qa-project-api"))
            .expect("project override");
        assert_eq!(created.layout_type, PaneLayoutType::Vertical2);
        assert_eq!(created.panes.len(), 2);
        std::fs::remove_dir_all(temp_root).expect("remove pane fixture");
    }

    #[test]
    fn native_ui_source_uses_gpui_component_root_entrypoint() {
        let source = include_str!("native_ui.rs");

        assert!(source.contains("Application::new()"));
        assert!(source.contains("gpui_component::init(cx)"));
        assert!(source.contains("Root::new(view, window, cx)"));
        assert!(source.contains("Button::new(\"new-task\")"));
        assert!(source.contains("KeyBinding::new(\"cmd-shift-n\", NewWindow, None)"));
        assert!(source.contains("KeyBinding::new(\"cmd-shift-[\", NavigateBack, None)"));
        assert!(source.contains("KeyBinding::new(\"cmd-shift-]\", NavigateForward, None)"));
        assert!(source.contains("KeyBinding::new(\"cmd-shift-p\", OpenProjectFilter"));
        assert!(source.contains("KeyBinding::new(&task_search, QuickSearch, None)"));
        assert!(source.contains("KeyBinding::new(\"cmd-shift-i\", OpenNotifications, None)"));
        assert!(source.contains("KeyBinding::new(\"escape\", DismissOverlay, Some(\"Modal\"))"));
        assert!(source.contains(".tab_group()"));
        assert!(source.contains("common.releaseUpdate.title"));
        assert!(source.contains("common.markAllRead"));
        assert!(source.contains("KeyBinding::new(\"down\", FocusNextTaskArrow"));
        assert!(source.contains("KeyBinding::new(\"j\", FocusNextTask"));
        assert!(source.contains("render_done_confirmation"));
        assert!(source.contains("dismiss_done_alert"));
        assert!(source.contains("cx.set_menus"));
        assert!(source.contains("app.on_reopen"));
        assert!(source.contains("TitlebarOptions"));
        assert!(source.contains("InputState::new"));
        assert!(source.contains("create_native_task"));
        assert!(source.contains("SSH host (optional)"));
        assert!(source.contains("register_native_remote_project"));
        assert!(source.contains("Registering..."));
        assert!(source.contains("load_native_task_ai_sessions"));
        assert!(source.contains("load_native_task_ai_session_detail"));
        assert!(source.contains("Loading AI sessions"));
        assert!(source.contains("AI session unavailable"));
        assert!(source.contains("terminal-connect-tmux"));
        assert!(source.contains("terminal-connect-zellij"));
        assert!(source.contains("task_editor_submitting"));
        assert!(source.contains("diff_snapshots_loading"));
        assert!(source.contains("viewed_diff_files"));
        assert!(source.contains("Binary file preview and editing are disabled."));
        assert!(source.contains("diff-sidebar-narrower"));
        assert!(source.contains("diff-sidebar-wider"));
        assert!(source.contains("diff_editor_saving"));
        assert!(source.contains("Unsaved changes"));
        assert!(source.contains("restore_terminal_focus_on_render"));
        assert!(source.contains("restore_terminal_focus_if_requested"));
        assert!(source.contains("|| self.delete_confirmation_task_id.is_some()"));
        assert!(source.contains("render_background_sync_review"));
        assert!(source.contains("render_notification_center"));
        assert!(source.contains("notification-mark-all"));
        assert!(source.contains(".capture_key_down("));
        assert!(source.contains("KeyBinding::new(\"enter\", SubmitTaskEditor"));
        assert!(source.contains("KeyBinding::new(\"enter\", ApplyQuickSearch"));
        assert!(source.contains("KeyBinding::new(\"enter\", SubmitVimCommand"));
        assert!(source.contains("settings-task-search-shortcut-record"));
        assert!(source.contains("settings-task-search-shortcut-save"));
        assert!(source.contains("NativeNotificationService"));
        assert!(source.contains("install_native_github_cli"));
        assert!(source.contains("get_native_task_session_dependency_status"));
        assert!(source.contains("install_native_task_session_dependency"));
        assert!(source.contains("session-dependency-check"));
        assert!(source.contains("session-dependency-install"));
        assert!(source.contains("get_native_task_hook_status"));
        assert!(source.contains("install_native_task_hooks"));
        assert!(source.contains("task-hooks-check"));
        assert!(source.contains("task-hooks-install"));
        assert!(source.contains("save_native_pane_layout_type"));
        assert!(source.contains("PANE_LAYOUT_TYPES"));
        assert!(source.contains("pane_position_label"));
        assert!(source.contains("pane-project-create"));
        assert!(source.contains("dismiss_native_sidebar_hint"));
        assert!(source.contains("task-sidebar-collapse"));
        assert!(source.contains("task-sidebar-hint-dismiss"));
        assert!(source.contains("PROJECT_COLOR_PRESETS"));
        assert!(source.contains("update_native_project_color"));
        assert!(source.contains("project_color_updating"));
        assert!(source.contains("task-project-color"));
        assert!(!source.contains("webview"));
        assert!(!source.contains("electron"));
    }

    #[test]
    fn native_ui_qa_socket_dispatches_through_the_live_gpui_entity() {
        let source = include_str!("native_ui.rs");

        assert!(source.contains("spawn_debug_qa_runtime_socket_from_env"));
        assert!(source.contains(".update_in(window"));
        assert!(source.contains("handle_runtime_qa_command"));
        assert!(source.contains("window.dispatch_action(Box::new(CreateTask), cx)"));
        assert!(source.contains("\"gpui-production-action-dispatched\""));
        assert!(!source.contains("spawn_debug_qa_socket_from_env(state.spec.clone())"));
    }

    #[test]
    fn remote_open_code_rows_decode_typed_transport_and_missing_marker() {
        let rows = parse_remote_open_code_rows::<OpenCodeSessionRow>(
            r#"[{"id":"session-1","directory":"/workspace/repo","title":"Review",
                 "time_created":1785301200000,"time_updated":1785301260000,
                 "part_count":2,"first_user_part":"{\"type\":\"text\",\"text\":\"Review\"}"}]"#,
        )
        .expect("typed remote rows")
        .expect("remote database");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "session-1");
        assert_eq!(
            parse_remote_open_code_rows::<OpenCodeSessionRow>("__KANVIBE_DB_MISSING__")
                .expect("missing marker"),
            None
        );
        assert!(
            parse_remote_open_code_rows::<OpenCodeSessionRow>("not-json")
                .expect_err("invalid transport payload")
                .to_string()
                .contains("invalid remote OpenCode rows")
        );
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
            extract_internal_route(
                "https://example.com/#/ko/settings",
                Some("http://localhost:3000")
            ),
            None
        );
        assert_eq!(
            extract_internal_route("https://example.com/#/ko/settings", None),
            None
        );
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
            extract_internal_route(
                "http://user:pw@localhost:3000/#/ko",
                Some("http://localhost:3000")
            )
            .as_deref(),
            Some("/ko")
        );
        assert_eq!(
            extract_internal_route("https://example.com:443/#/ko", Some("https://example.com"))
                .as_deref(),
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
