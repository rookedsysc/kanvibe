use std::{
    error::Error,
    fmt::{Display, Formatter},
    path::{Path, PathBuf},
};

use rusqlite::{
    Connection, OpenFlags, Params, Row, Transaction, TransactionBehavior, params, types::Type,
};
use serde::{Deserialize, Serialize};

pub const DONE_PAGE_SIZE: u32 = 20;

pub fn validate_sqlite_database(
    database_path: impl AsRef<Path>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let connection = Connection::open_with_flags(
        database_path.as_ref(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut statement = connection.prepare("PRAGMA quick_check")?;
    let results = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if results.len() == 1 && results[0] == "ok" {
        return Ok(());
    }
    Err(format!("SQLite quick_check failed: {}", results.join("; ")).into())
}

pub fn create_sqlite_backup_once(
    source_path: impl AsRef<Path>,
    backup_path: impl AsRef<Path>,
) -> Result<bool, Box<dyn Error + Send + Sync>> {
    let source_path = source_path.as_ref();
    let backup_path = backup_path.as_ref();
    if source_path == backup_path {
        return Err("SQLite backup path must differ from its source".into());
    }
    validate_sqlite_database(source_path)?;
    if backup_path.exists() {
        validate_sqlite_database(backup_path)?;
        return Ok(false);
    }
    if let Some(parent) = backup_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let file_name = backup_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("kanvibe-backup");
    let temporary_path =
        backup_path.with_file_name(format!(".{file_name}.{}-{unique}.tmp", std::process::id()));

    let source = Connection::open_with_flags(
        source_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let backup_result = source.backup(rusqlite::MAIN_DB, &temporary_path, None);
    if let Err(error) = backup_result {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error.into());
    }
    if let Err(error) = validate_sqlite_database(&temporary_path) {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error);
    }

    match std::fs::hard_link(&temporary_path, backup_path) {
        Ok(()) => {
            std::fs::remove_file(&temporary_path)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            std::fs::remove_file(&temporary_path)?;
            validate_sqlite_database(backup_path)?;
            Ok(false)
        }
        Err(error) => {
            let _ = std::fs::remove_file(&temporary_path);
            Err(error.into())
        }
    }
}

pub fn restore_sqlite_database_from_backup(
    database_path: impl AsRef<Path>,
    backup_path: impl AsRef<Path>,
) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    let database_path = database_path.as_ref();
    let backup_path = backup_path.as_ref();
    validate_sqlite_database(database_path)?;
    validate_sqlite_database(backup_path)?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let database_name = database_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("kanvibe.db");
    let native_safety_path = database_path.with_file_name(format!(
        "{database_name}.native-before-rollback-{}-{unique}",
        std::process::id()
    ));
    create_sqlite_backup_once(database_path, &native_safety_path)?;

    let restore_result = (|| -> rusqlite::Result<()> {
        let mut destination = Connection::open(database_path)?;
        destination.busy_timeout(std::time::Duration::from_secs(5))?;
        destination.restore(
            rusqlite::MAIN_DB,
            backup_path,
            None::<fn(rusqlite::backup::Progress)>,
        )
    })();
    if let Err(restore_error) = restore_result {
        let recovery_result = (|| -> rusqlite::Result<()> {
            let mut destination = Connection::open(database_path)?;
            destination.restore(
                rusqlite::MAIN_DB,
                &native_safety_path,
                None::<fn(rusqlite::backup::Progress)>,
            )
        })();
        return match recovery_result {
            Ok(()) => Err(format!(
                "SQLite rollback failed and the native database was recovered: {restore_error}"
            )
            .into()),
            Err(recovery_error) => Err(format!(
                "SQLite rollback failed ({restore_error}) and native recovery also failed \
                 ({recovery_error}); safety snapshot: {}",
                native_safety_path.display()
            )
            .into()),
        };
    }
    if let Err(validation_error) = validate_sqlite_database(database_path) {
        let mut destination = Connection::open(database_path)?;
        destination.restore(
            rusqlite::MAIN_DB,
            &native_safety_path,
            None::<fn(rusqlite::backup::Progress)>,
        )?;
        return Err(format!(
            "restored Electron database failed integrity validation and the native database was \
             recovered: {validation_error}"
        )
        .into());
    }
    Ok(native_safety_path)
}

const ELECTRON_MIGRATIONS: &[(i64, &str)] = &[
    (1770854400000, "InitialSchema1770854400000"),
    (1770854400001, "AddPrUrlToKanbanTasks1770854400001"),
    (1770854400002, "AddIsWorktreeToProjects1770854400002"),
    (1771048256887, "AddPaneLayoutConfig1771048256887"),
    (1771166346785, "AssignDisplayOrder1771166346785"),
    (1771166907165, "AddAppSettings1771166907165"),
    (1771171200000, "AddPendingStatus1771171200000"),
    (1771257600000, "RemoveBranchNameUnique1771257600000"),
    (1771343199455, "AddColorIndexToProjects1771343199455"),
    (1771344000000, "AddPriorityToKanbanTasks1771344000000"),
    (1771388085809, "ReplaceColorIndexWithColor1771388085809"),
    (1771400000000, "FillEmptyBaseBranch1771400000000"),
];

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct DatabaseMigrationReport {
    pub baselined_existing_database: bool,
    pub applied_migrations: usize,
}

pub fn migrate_electron_database(
    database_path: impl AsRef<Path>,
) -> Result<DatabaseMigrationReport, Box<dyn Error + Send + Sync>> {
    let database_path = database_path.as_ref();
    let mut connection = Connection::open(database_path)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", true)?;
    let had_tasks = sqlite_table_exists(&connection, "kanban_tasks")?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

    ensure_current_schema(&transaction)?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS migrations (
            id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            timestamp BIGINT NOT NULL,
            name VARCHAR NOT NULL
         );",
    )?;
    let migration_count = transaction.query_row("SELECT COUNT(1) FROM migrations", [], |row| {
        row.get::<_, i64>(0)
    })?;
    let baselined_existing_database = had_tasks && migration_count == 0;
    let mut applied_migrations = 0;

    if baselined_existing_database {
        for (timestamp, name) in ELECTRON_MIGRATIONS {
            transaction.execute(
                "INSERT INTO migrations(timestamp, name) VALUES (?1, ?2)",
                params![timestamp, name],
            )?;
        }
    } else {
        for (timestamp, name) in ELECTRON_MIGRATIONS {
            let recorded = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM migrations WHERE timestamp = ?1 OR name = ?2
                 )",
                params![timestamp, name],
                |row| row.get::<_, bool>(0),
            )?;
            if recorded {
                continue;
            }
            apply_electron_migration(&transaction, *timestamp)?;
            transaction.execute(
                "INSERT INTO migrations(timestamp, name) VALUES (?1, ?2)",
                params![timestamp, name],
            )?;
            applied_migrations += 1;
        }
    }

    transaction.commit()?;
    validate_sqlite_database(database_path)?;
    Ok(DatabaseMigrationReport {
        baselined_existing_database,
        applied_migrations,
    })
}

fn sqlite_table_exists(connection: &Connection, table_name: &str) -> rusqlite::Result<bool> {
    connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
         )",
        [table_name],
        |row| row.get(0),
    )
}

fn transaction_table_columns(
    transaction: &Transaction<'_>,
    table_name: &str,
) -> rusqlite::Result<Vec<String>> {
    let mut statement = transaction.prepare(&format!(
        "PRAGMA table_info(\"{}\")",
        table_name.replace('"', "\"\"")
    ))?;
    statement
        .query_map([], |row| row.get::<_, String>("name"))?
        .collect()
}

fn ensure_column(
    transaction: &Transaction<'_>,
    table_name: &str,
    column_name: &str,
    definition: &str,
) -> rusqlite::Result<()> {
    if transaction_table_columns(transaction, table_name)?
        .iter()
        .any(|column| column == column_name)
    {
        return Ok(());
    }
    transaction.execute_batch(&format!(
        "ALTER TABLE \"{}\" ADD COLUMN {definition};",
        table_name.replace('"', "\"\"")
    ))
}

fn ensure_current_schema(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY NOT NULL,
            name TEXT NOT NULL UNIQUE,
            repo_path TEXT NOT NULL,
            default_branch TEXT NOT NULL DEFAULT 'main',
            ssh_host TEXT,
            is_worktree INTEGER NOT NULL DEFAULT 0,
            color TEXT DEFAULT NULL,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
         );
         CREATE TABLE IF NOT EXISTS kanban_tasks (
            id TEXT PRIMARY KEY NOT NULL,
            title TEXT NOT NULL,
            description TEXT,
            status TEXT NOT NULL DEFAULT 'todo',
            branch_name TEXT,
            worktree_path TEXT,
            session_type TEXT,
            session_name TEXT,
            ssh_host TEXT,
            agent_type TEXT,
            project_id TEXT,
            base_branch TEXT,
            pr_url TEXT,
            priority TEXT DEFAULT NULL,
            display_order INTEGER NOT NULL DEFAULT 0,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL
         );
         CREATE TABLE IF NOT EXISTS pane_layout_configs (
            id TEXT PRIMARY KEY NOT NULL,
            layout_type TEXT NOT NULL,
            panes TEXT NOT NULL,
            project_id TEXT UNIQUE,
            is_global INTEGER NOT NULL DEFAULT 0,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
         );
         CREATE TABLE IF NOT EXISTS app_settings (
            id TEXT PRIMARY KEY NOT NULL,
            key TEXT NOT NULL UNIQUE,
            value TEXT NOT NULL,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
         );",
    )?;

    let project_columns = transaction_table_columns(transaction, "projects")?;
    if project_columns.iter().any(|column| column == "color_index")
        && !project_columns.iter().any(|column| column == "color")
    {
        transaction.execute_batch("ALTER TABLE projects RENAME COLUMN color_index TO color;")?;
    }
    for (name, definition) in [
        (
            "default_branch",
            "default_branch TEXT NOT NULL DEFAULT 'main'",
        ),
        ("ssh_host", "ssh_host TEXT"),
        ("is_worktree", "is_worktree INTEGER NOT NULL DEFAULT 0"),
        ("color", "color TEXT DEFAULT NULL"),
        (
            "created_at",
            "created_at DATETIME NOT NULL DEFAULT '1970-01-01 00:00:00'",
        ),
    ] {
        ensure_column(transaction, "projects", name, definition)?;
    }
    for (name, definition) in [
        ("description", "description TEXT"),
        ("status", "status TEXT NOT NULL DEFAULT 'todo'"),
        ("branch_name", "branch_name TEXT"),
        ("worktree_path", "worktree_path TEXT"),
        ("session_type", "session_type TEXT"),
        ("session_name", "session_name TEXT"),
        ("ssh_host", "ssh_host TEXT"),
        ("agent_type", "agent_type TEXT"),
        ("project_id", "project_id TEXT"),
        ("base_branch", "base_branch TEXT"),
        ("pr_url", "pr_url TEXT"),
        ("priority", "priority TEXT DEFAULT NULL"),
        ("display_order", "display_order INTEGER NOT NULL DEFAULT 0"),
        (
            "created_at",
            "created_at DATETIME NOT NULL DEFAULT '1970-01-01 00:00:00'",
        ),
        (
            "updated_at",
            "updated_at DATETIME NOT NULL DEFAULT '1970-01-01 00:00:00'",
        ),
    ] {
        ensure_column(transaction, "kanban_tasks", name, definition)?;
    }
    ensure_column(
        transaction,
        "pane_layout_configs",
        "panes",
        "panes TEXT NOT NULL DEFAULT '[]'",
    )?;
    transaction.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_kanban_tasks_status_order
            ON kanban_tasks(status, display_order, created_at);
         CREATE INDEX IF NOT EXISTS idx_kanban_tasks_project_branch
            ON kanban_tasks(project_id, branch_name);
         DROP INDEX IF EXISTS UQ_kanban_tasks_branch_name;",
    )
}

fn apply_electron_migration(transaction: &Transaction<'_>, timestamp: i64) -> rusqlite::Result<()> {
    match timestamp {
        1771166346785 => transaction.execute_batch(
            "UPDATE kanban_tasks
             SET display_order = (
                SELECT COUNT(*)
                FROM kanban_tasks t2
                WHERE t2.status = kanban_tasks.status
                  AND (
                    t2.created_at < kanban_tasks.created_at
                    OR (
                        t2.created_at = kanban_tasks.created_at
                        AND t2.id < kanban_tasks.id
                    )
                  )
             )
             WHERE display_order = 0;",
        ),
        1771257600000 => {
            transaction.execute_batch("DROP INDEX IF EXISTS UQ_kanban_tasks_branch_name;")
        }
        1771400000000 => transaction.execute_batch(
            "UPDATE kanban_tasks
             SET base_branch = (
                SELECT projects.default_branch
                FROM projects
                WHERE projects.id = kanban_tasks.project_id
             )
             WHERE project_id IS NOT NULL
               AND (base_branch IS NULL OR base_branch = '');",
        ),
        _ => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Todo,
    Progress,
    Pending,
    Review,
    Done,
}

impl TaskStatus {
    pub const ALL: [Self; 5] = [
        Self::Todo,
        Self::Progress,
        Self::Pending,
        Self::Review,
        Self::Done,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Progress => "progress",
            Self::Pending => "pending",
            Self::Review => "review",
            Self::Done => "done",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ParseEnumError> {
        match value {
            "todo" => Ok(Self::Todo),
            "progress" => Ok(Self::Progress),
            "pending" => Ok(Self::Pending),
            "review" => Ok(Self::Review),
            "done" => Ok(Self::Done),
            _ => Err(ParseEnumError::new("TaskStatus", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskPriority {
    Low,
    Medium,
    High,
}

impl TaskPriority {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ParseEnumError> {
        match value {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            _ => Err(ParseEnumError::new("TaskPriority", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionType {
    Tmux,
    Zellij,
}

impl SessionType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tmux => "tmux",
            Self::Zellij => "zellij",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ParseEnumError> {
        match value {
            "tmux" => Ok(Self::Tmux),
            "zellij" => Ok(Self::Zellij),
            _ => Err(ParseEnumError::new("SessionType", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum PaneLayoutType {
    #[serde(rename = "single")]
    Single,
    #[serde(rename = "horizontal_2")]
    Horizontal2,
    #[serde(rename = "vertical_2")]
    Vertical2,
    #[serde(rename = "left_right_tb")]
    LeftRightTb,
    #[serde(rename = "left_tb_right")]
    LeftTbRight,
    #[serde(rename = "quad")]
    Quad,
}

impl PaneLayoutType {
    pub const fn pane_count(self) -> usize {
        match self {
            Self::Single => 1,
            Self::Horizontal2 | Self::Vertical2 => 2,
            Self::LeftRightTb | Self::LeftTbRight => 3,
            Self::Quad => 4,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Horizontal2 => "horizontal_2",
            Self::Vertical2 => "vertical_2",
            Self::LeftRightTb => "left_right_tb",
            Self::LeftTbRight => "left_tb_right",
            Self::Quad => "quad",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ParseEnumError> {
        match value {
            "single" => Ok(Self::Single),
            "horizontal_2" => Ok(Self::Horizontal2),
            "vertical_2" => Ok(Self::Vertical2),
            "left_right_tb" => Ok(Self::LeftRightTb),
            "left_tb_right" => Ok(Self::LeftTbRight),
            "quad" => Ok(Self::Quad),
            _ => Err(ParseEnumError::new("PaneLayoutType", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    System,
    Light,
    Dark,
}

impl ThemePreference {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ParseEnumError> {
        match value {
            "system" => Ok(Self::System),
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            _ => Err(ParseEnumError::new("ThemePreference", value)),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ParseEnumError {
    enum_name: &'static str,
    value: String,
}

impl ParseEnumError {
    pub fn new(enum_name: &'static str, value: impl Into<String>) -> Self {
        Self {
            enum_name,
            value: value.into(),
        }
    }
}

impl Display for ParseEnumError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "unknown {} value `{}`",
            self.enum_name, self.value
        )
    }
}

impl Error for ParseEnumError {}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub repo_path: String,
    pub default_branch: String,
    pub ssh_host: Option<String>,
    pub is_worktree: bool,
    pub color: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct KanbanTask {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub branch_name: Option<String>,
    pub worktree_path: Option<String>,
    pub session_type: Option<SessionType>,
    pub session_name: Option<String>,
    pub ssh_host: Option<String>,
    pub agent_type: Option<String>,
    pub project_id: Option<String>,
    pub base_branch: Option<String>,
    pub pr_url: Option<String>,
    pub priority: Option<TaskPriority>,
    pub display_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Done 전환 실패 시 되돌릴 값. Electron `DoneRollbackSnapshot`과 같은 필드를 보존한다.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct DoneRollbackSnapshot {
    pub id: String,
    pub status: TaskStatus,
    pub session_type: Option<SessionType>,
    pub session_name: Option<String>,
    pub worktree_path: Option<String>,
    pub ssh_host: Option<String>,
    pub project_id: Option<String>,
    pub branch_name: Option<String>,
}

impl DoneRollbackSnapshot {
    fn from_task(task: &KanbanTask) -> Self {
        Self {
            id: task.id.clone(),
            status: task.status,
            session_type: task.session_type,
            session_name: task.session_name.clone(),
            worktree_path: task.worktree_path.clone(),
            ssh_host: task.ssh_host.clone(),
            project_id: task.project_id.clone(),
            branch_name: task.branch_name.clone(),
        }
    }
}

/// Done 전환으로 DB에서 지워진 리소스 정보를 담는다. 이것이 없으면 정리 대상을 복구할 수 없다.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct DoneCleanupPlan {
    /// 전환 직전 스냅샷. 세션/worktree 값이 그대로 남아 있어 정리 실행에 사용한다.
    pub cleanup_task: KanbanTask,
    pub rollback: DoneRollbackSnapshot,
}

impl DoneCleanupPlan {
    /// 정리할 리소스가 하나라도 있는지. Electron `cleanupTaskResources`의 진입 조건과 같다.
    pub fn has_resources_to_clean(&self) -> bool {
        let has_session =
            self.cleanup_task.session_type.is_some() && self.cleanup_task.session_name.is_some();
        let has_worktree =
            self.cleanup_task.branch_name.is_some() && self.cleanup_task.worktree_path.is_some();

        has_session || has_worktree
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DoneCleanupResult {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DoneCleanupOutcome {
    /// 정리에 성공해 비워진 상태를 유지한다.
    Cleared,
    /// 정리에 실패해 전환 직전 상태로 되돌렸다.
    RolledBack,
    /// 그 사이 상태가 다시 바뀌어 되돌리지 않았다.
    SkippedRollback,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskStatusUpdate {
    pub task: KanbanTask,
    /// Done 전환일 때만 존재한다.
    pub done_cleanup: Option<DoneCleanupPlan>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct BoardColumn {
    pub status: TaskStatus,
    pub tasks: Vec<KanbanTask>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct BoardSnapshot {
    pub projects: Vec<Project>,
    pub columns: Vec<BoardColumn>,
    pub done_total: u32,
    pub done_limit: u32,
}

impl BoardSnapshot {
    pub fn column(&self, status: TaskStatus) -> Option<&BoardColumn> {
        self.columns.iter().find(|column| column.status == status)
    }

    pub fn task_count(&self, status: TaskStatus) -> usize {
        self.column(status)
            .map(|column| column.tasks.len())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct CreateTaskInput {
    pub id: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub branch_name: Option<String>,
    pub base_branch: Option<String>,
    pub worktree_path: Option<String>,
    pub session_type: Option<SessionType>,
    pub session_name: Option<String>,
    pub ssh_host: Option<String>,
    pub agent_type: Option<String>,
    pub project_id: Option<String>,
    pub priority: Option<TaskPriority>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct TaskUpdatePatch {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub priority: Option<Option<TaskPriority>>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct PaneCommand {
    pub position: u32,
    pub command: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct PaneLayoutConfig {
    pub id: String,
    pub layout_type: PaneLayoutType,
    pub panes: Vec<PaneCommand>,
    pub project_id: Option<String>,
    pub is_global: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SavePaneLayoutInput {
    pub layout_type: PaneLayoutType,
    pub panes: Vec<PaneCommand>,
    pub project_id: Option<String>,
    pub is_global: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BackgroundSyncSettings {
    pub is_enabled: bool,
    pub interval_ms: u64,
}

pub const DEFAULT_BACKGROUND_SYNC_INTERVAL_MS: u64 = 10 * 60_000;
pub const DEFAULT_NOTIFICATION_STATUSES: &[&str] = &["progress", "pending", "review"];
pub const DEFAULT_TASK_SEARCH_SHORTCUT: &str = "Mod+Shift+O";

pub struct KanvibeDb {
    connection: Connection,
}

impl KanvibeDb {
    pub fn open_read_only(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", true)?;

        Ok(Self { connection })
    }

    pub fn open_read_write(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        Self::open_read_write_with_timeout(path, std::time::Duration::from_secs(5))
    }

    fn open_read_write_with_timeout(
        path: impl AsRef<Path>,
        busy_timeout: std::time::Duration,
    ) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(busy_timeout)?;
        connection.pragma_update(None, "foreign_keys", true)?;

        Ok(Self { connection })
    }

    pub fn projects(&self) -> rusqlite::Result<Vec<Project>> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, repo_path, default_branch, ssh_host, is_worktree, color, created_at
             FROM projects
             ORDER BY created_at ASC",
        )?;

        statement
            .query_map([], map_project)?
            .collect::<rusqlite::Result<Vec<_>>>()
    }

    pub fn project_by_id(&self, project_id: &str) -> rusqlite::Result<Option<Project>> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, repo_path, default_branch, ssh_host, is_worktree, color, created_at
             FROM projects WHERE id = ?1",
        )?;
        let mut rows = statement.query([project_id])?;
        rows.next()?.map(map_project).transpose()
    }

    pub fn register_project(
        &self,
        name: &str,
        repo_path: &str,
        default_branch: &str,
        ssh_host: Option<&str>,
        color: Option<&str>,
    ) -> rusqlite::Result<Project> {
        let id = self
            .connection
            .query_row("SELECT lower(hex(randomblob(16)))", [], |row| {
                row.get::<_, String>(0)
            })?;
        self.connection.execute(
            "INSERT INTO projects (
                id, name, repo_path, default_branch, ssh_host, is_worktree, color
             ) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
            params![id, name, repo_path, default_branch, ssh_host, color],
        )?;
        self.project_by_id(&id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn create_project_root_task(
        &self,
        project: &Project,
        session_type: SessionType,
        session_name: &str,
    ) -> rusqlite::Result<KanbanTask> {
        let id = self
            .connection
            .query_row("SELECT lower(hex(randomblob(16)))", [], |row| {
                row.get::<_, String>(0)
            })?;
        let display_order = self.connection.query_row(
            "SELECT COALESCE(MAX(display_order), -1) + 1 FROM kanban_tasks WHERE status = 'todo'",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        self.connection.execute(
            "INSERT INTO kanban_tasks (
                id, title, status, branch_name, worktree_path, session_type, session_name,
                ssh_host, project_id, base_branch, display_order
             ) VALUES (?1, ?2, 'todo', ?2, ?3, ?4, ?5, ?6, ?7, ?2, ?8)",
            params![
                id,
                project.default_branch,
                project.repo_path,
                session_type.as_str(),
                session_name,
                project.ssh_host,
                project.id,
                display_order,
            ],
        )?;
        self.task_by_id(&id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    /// Deletes project-owned task rows and the project record atomically, without touching Git.
    pub fn delete_project(&mut self, project_id: &str) -> rusqlite::Result<bool> {
        let transaction = self.connection.transaction()?;
        let exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)",
            [project_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            transaction.rollback()?;
            return Ok(false);
        }
        transaction.execute(
            "DELETE FROM kanban_tasks WHERE project_id = ?1",
            [project_id],
        )?;
        transaction.execute("DELETE FROM projects WHERE id = ?1", [project_id])?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn task_by_id(&self, task_id: &str) -> rusqlite::Result<Option<KanbanTask>> {
        let mut statement = self.connection.prepare(
            "SELECT id, title, description, status, branch_name, worktree_path, session_type,
                    session_name, ssh_host, agent_type, project_id, base_branch, pr_url, priority,
                    display_order, created_at, updated_at
             FROM kanban_tasks
             WHERE id = ?1",
        )?;
        let mut rows = statement.query([task_id])?;

        rows.next()?.map(map_task).transpose()
    }

    pub fn active_tasks(&self) -> rusqlite::Result<Vec<KanbanTask>> {
        self.tasks_where(
            "WHERE status != 'done'
             ORDER BY updated_at ASC, created_at ASC",
            [],
        )
    }

    pub fn set_task_pr_url_if_changed(
        &self,
        task_id: &str,
        pr_url: &str,
    ) -> rusqlite::Result<bool> {
        let changed = self.connection.execute(
            "UPDATE kanban_tasks
             SET pr_url = ?1, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?2 AND pr_url IS NOT ?1",
            params![pr_url, task_id],
        )?;
        Ok(changed > 0)
    }

    pub fn task_by_project_branch(
        &self,
        project_id: &str,
        branch_name: &str,
    ) -> rusqlite::Result<Option<KanbanTask>> {
        let mut statement = self.connection.prepare(
            "SELECT id, title, description, status, branch_name, worktree_path, session_type,
                    session_name, ssh_host, agent_type, project_id, base_branch, pr_url, priority,
                    display_order, created_at, updated_at
             FROM kanban_tasks
             WHERE project_id = ?1 AND branch_name = ?2
             ORDER BY created_at ASC
             LIMIT 1",
        )?;
        let mut rows = statement.query(params![project_id, branch_name])?;
        rows.next()?.map(map_task).transpose()
    }

    pub fn orphan_task_by_branch_and_path(
        &self,
        branch_name: &str,
        worktree_path: &str,
    ) -> rusqlite::Result<Option<KanbanTask>> {
        self.orphan_task_by_location(branch_name, worktree_path, None)
    }

    pub fn orphan_task_by_location(
        &self,
        branch_name: &str,
        worktree_path: &str,
        ssh_host: Option<&str>,
    ) -> rusqlite::Result<Option<KanbanTask>> {
        let mut statement = self.connection.prepare(
            "SELECT id, title, description, status, branch_name, worktree_path, session_type,
                    session_name, ssh_host, agent_type, project_id, base_branch, pr_url, priority,
                    display_order, created_at, updated_at
             FROM kanban_tasks
             WHERE project_id IS NULL AND branch_name = ?1 AND worktree_path = ?2
                   AND ssh_host IS ?3
             ORDER BY created_at ASC
             LIMIT 1",
        )?;
        let mut rows = statement.query(params![branch_name, worktree_path, ssh_host])?;
        rows.next()?.map(map_task).transpose()
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the arguments mirror the persisted worktree synchronization contract"
    )]
    pub fn bind_task_to_worktree(
        &self,
        task_id: &str,
        project_id: &str,
        branch_name: &str,
        worktree_path: &str,
        base_branch: &str,
        ssh_host: Option<&str>,
        status: TaskStatus,
    ) -> rusqlite::Result<Option<KanbanTask>> {
        self.connection.execute(
            "UPDATE kanban_tasks
             SET title = ?1, project_id = ?2, branch_name = ?3, worktree_path = ?4,
                 base_branch = ?5, ssh_host = ?6, status = ?7,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?8",
            params![
                branch_name,
                project_id,
                branch_name,
                worktree_path,
                base_branch,
                ssh_host,
                status.as_str(),
                task_id,
            ],
        )?;
        self.task_by_id(task_id)
    }

    pub fn board_snapshot(&self, done_limit: u32) -> rusqlite::Result<BoardSnapshot> {
        let projects = self.projects()?;
        let non_done_tasks = self.tasks_where(
            "WHERE status != 'done'
             ORDER BY display_order ASC, created_at ASC",
            [],
        )?;
        let done_tasks = self.tasks_where(
            "WHERE status = 'done'
             ORDER BY display_order ASC, created_at ASC
             LIMIT ?1",
            [i64::from(done_limit)],
        )?;
        let done_total = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM kanban_tasks WHERE status = 'done'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count as u32)?;

        let mut columns = TaskStatus::ALL
            .into_iter()
            .map(|status| BoardColumn {
                status,
                tasks: Vec::new(),
            })
            .collect::<Vec<_>>();

        for task in non_done_tasks {
            if let Some(column) = columns
                .iter_mut()
                .find(|column| column.status == task.status)
            {
                column.tasks.push(task);
            }
        }

        if let Some(done_column) = columns
            .iter_mut()
            .find(|column| column.status == TaskStatus::Done)
        {
            done_column.tasks = done_tasks;
        }

        Ok(BoardSnapshot {
            projects,
            columns,
            done_total,
            done_limit,
        })
    }

    pub fn create_task(&self, input: CreateTaskInput) -> rusqlite::Result<KanbanTask> {
        let id = match input.id {
            Some(id) => id,
            None => self
                .connection
                .query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?,
        };
        let title = input
            .title
            .filter(|title| !title.trim().is_empty())
            .or_else(|| input.branch_name.clone())
            .unwrap_or_else(|| "Untitled".to_owned());
        let status = input.status.unwrap_or(TaskStatus::Todo);
        let session_type = input.session_type.map(SessionType::as_str);
        let priority = input.priority.map(TaskPriority::as_str);

        self.connection.execute(
            "INSERT INTO kanban_tasks (
                id, title, description, status, branch_name, base_branch, worktree_path,
                session_type, session_name, ssh_host, agent_type, project_id, priority, display_order
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                (SELECT COALESCE(MAX(display_order), -1) + 1
                 FROM kanban_tasks WHERE status = ?4)
             )",
            params![
                id,
                title,
                input.description,
                status.as_str(),
                input.branch_name,
                input.base_branch,
                input.worktree_path,
                session_type,
                input.session_name,
                input.ssh_host,
                input.agent_type,
                input.project_id,
                priority,
            ],
        )?;

        self.task_by_id(&id)?
            .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
    }

    /// Hook status updates intentionally retain session/worktree metadata, including for Done.
    ///
    /// Electron's hook service saves only the status field; unlike an interactive board move it
    /// does not clean up resources. This separate method keeps that externally observable contract.
    pub fn set_task_status_preserving_resources(
        &self,
        task_id: &str,
        new_status: TaskStatus,
    ) -> rusqlite::Result<Option<KanbanTask>> {
        self.connection.execute(
            "UPDATE kanban_tasks
             SET status = ?1, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?2",
            params![new_status.as_str(), task_id],
        )?;
        self.task_by_id(task_id)
    }

    /// Electron `updateTaskStatus`의 포팅.
    ///
    /// Done 전환은 세션/worktree 컬럼을 비우기 때문에, 무엇을 정리해야 하는지가 DB에서 사라진다.
    /// 그래서 Done일 때는 전환 직전 스냅샷을 담은 [`DoneCleanupPlan`]을 함께 돌려주고,
    /// 호출자가 실제 정리 후 [`KanvibeDb::finish_done_cleanup`]으로 확정 또는 롤백하게 한다.
    pub fn update_task_status(
        &self,
        task_id: &str,
        new_status: TaskStatus,
    ) -> rusqlite::Result<Option<TaskStatusUpdate>> {
        let Some(previous) = self.task_by_id(task_id)? else {
            return Ok(None);
        };

        if new_status != TaskStatus::Done {
            self.connection.execute(
                "UPDATE kanban_tasks
                 SET status = ?1, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?2",
                params![new_status.as_str(), task_id],
            )?;

            return Ok(self.task_by_id(task_id)?.map(|task| TaskStatusUpdate {
                task,
                done_cleanup: None,
            }));
        }

        self.connection.execute(
            "UPDATE kanban_tasks
             SET status = ?1, session_type = NULL, session_name = NULL,
                 worktree_path = NULL, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?2",
            params![new_status.as_str(), task_id],
        )?;

        Ok(self.task_by_id(task_id)?.map(|task| TaskStatusUpdate {
            task,
            done_cleanup: Some(DoneCleanupPlan {
                rollback: DoneRollbackSnapshot::from_task(&previous),
                cleanup_task: previous,
            }),
        }))
    }

    /// 리소스 정리 결과를 반영한다. 실패하면 Electron `rollbackDoneTransition`과 같이 되돌린다.
    pub fn finish_done_cleanup(
        &self,
        plan: &DoneCleanupPlan,
        result: DoneCleanupResult,
    ) -> rusqlite::Result<DoneCleanupOutcome> {
        match result {
            DoneCleanupResult::Succeeded => Ok(DoneCleanupOutcome::Cleared),
            DoneCleanupResult::Failed => {
                if self.rollback_done_transition(&plan.rollback)? {
                    Ok(DoneCleanupOutcome::RolledBack)
                } else {
                    Ok(DoneCleanupOutcome::SkippedRollback)
                }
            }
        }
    }

    /// 스냅샷 시점 이후 사용자가 상태를 다시 바꿨다면 되돌리지 않는다.
    pub fn rollback_done_transition(
        &self,
        snapshot: &DoneRollbackSnapshot,
    ) -> rusqlite::Result<bool> {
        let current = self.task_by_id(&snapshot.id)?;
        let Some(current) = current else {
            return Ok(false);
        };
        if current.status != TaskStatus::Done {
            return Ok(false);
        }

        self.connection.execute(
            "UPDATE kanban_tasks
             SET status = ?1, session_type = ?2, session_name = ?3, worktree_path = ?4,
                 ssh_host = ?5, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?6",
            params![
                snapshot.status.as_str(),
                snapshot.session_type.map(SessionType::as_str),
                snapshot.session_name,
                snapshot.worktree_path,
                snapshot.ssh_host,
                snapshot.id,
            ],
        )?;

        Ok(true)
    }

    pub fn update_task(
        &self,
        task_id: &str,
        patch: TaskUpdatePatch,
    ) -> rusqlite::Result<Option<KanbanTask>> {
        if let Some(title) = patch.title {
            self.connection.execute(
                "UPDATE kanban_tasks SET title = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
                params![title, task_id],
            )?;
        }

        if let Some(description) = patch.description {
            self.connection.execute(
                "UPDATE kanban_tasks
                 SET description = ?1, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?2",
                params![description, task_id],
            )?;
        }

        if let Some(priority) = patch.priority {
            self.connection.execute(
                "UPDATE kanban_tasks
                 SET priority = ?1, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?2",
                params![priority.map(TaskPriority::as_str), task_id],
            )?;
        }

        self.task_by_id(task_id)
    }

    pub fn update_project_color(&self, project_id: &str, color: &str) -> rusqlite::Result<()> {
        let repo_path = self.connection.query_row(
            "SELECT repo_path FROM projects WHERE id = ?1",
            [project_id],
            |row| row.get::<_, String>(0),
        )?;
        let main_repo_path = repo_path
            .split_once("__worktrees")
            .map(|(main_path, _)| main_path)
            .unwrap_or(repo_path.as_str())
            .to_owned();
        let like_pattern = format!("{main_repo_path}%");

        self.connection.execute(
            "UPDATE projects SET color = ?1 WHERE repo_path LIKE ?2",
            params![color, like_pattern],
        )?;

        Ok(())
    }

    pub fn delete_task(&self, task_id: &str) -> rusqlite::Result<bool> {
        let changed = self
            .connection
            .execute("DELETE FROM kanban_tasks WHERE id = ?1", [task_id])?;

        Ok(changed > 0)
    }

    pub fn reorder_tasks(&self, ordered_ids: &[String]) -> rusqlite::Result<()> {
        for (display_order, task_id) in ordered_ids.iter().enumerate() {
            self.connection.execute(
                "UPDATE kanban_tasks SET display_order = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
                params![display_order as i64, task_id],
            )?;
        }

        Ok(())
    }

    /// Done 컬럼으로 옮기는 경우 호출자가 리소스 정리를 이어받을 수 있도록 계획을 돌려준다.
    pub fn move_task_to_column(
        &self,
        task_id: &str,
        new_status: TaskStatus,
        dest_ordered_ids: &[String],
    ) -> rusqlite::Result<Option<DoneCleanupPlan>> {
        self.connection.execute_batch("BEGIN IMMEDIATE")?;
        let operation = (|| {
            let done_cleanup = self
                .update_task_status(task_id, new_status)?
                .and_then(|update| update.done_cleanup);
            self.reorder_tasks(dest_ordered_ids)?;
            Ok(done_cleanup)
        })();
        match operation {
            Ok(done_cleanup) => {
                if let Err(commit_error) = self.connection.execute_batch("COMMIT") {
                    self.connection.execute_batch("ROLLBACK")?;
                    return Err(commit_error);
                }
                Ok(done_cleanup)
            }
            Err(operation_error) => {
                self.connection.execute_batch("ROLLBACK")?;
                Err(operation_error)
            }
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the arguments mirror the persisted branch-task contract"
    )]
    pub fn branch_from_task(
        &self,
        task_id: &str,
        project_id: &str,
        base_branch: &str,
        branch_name: &str,
        session_type: SessionType,
        session_name: &str,
        worktree_path: &str,
    ) -> rusqlite::Result<Option<KanbanTask>> {
        let project_ssh_host = self.connection.query_row(
            "SELECT ssh_host FROM projects WHERE id = ?1",
            [project_id],
            |row| row.get::<_, Option<String>>(0),
        )?;

        self.connection.execute(
            "UPDATE kanban_tasks
             SET project_id = ?1, branch_name = ?2, base_branch = ?3,
                 session_type = ?4, session_name = ?5, worktree_path = ?6,
                 ssh_host = ?7, status = 'progress', updated_at = CURRENT_TIMESTAMP
             WHERE id = ?8",
            params![
                project_id,
                branch_name,
                base_branch,
                session_type.as_str(),
                session_name,
                worktree_path,
                project_ssh_host,
                task_id,
            ],
        )?;

        self.task_by_id(task_id)
    }

    pub fn restore_task_branch_binding(
        &self,
        original: &KanbanTask,
    ) -> rusqlite::Result<Option<KanbanTask>> {
        self.connection.execute(
            "UPDATE kanban_tasks
             SET project_id = ?1, branch_name = ?2, base_branch = ?3,
                 session_type = ?4, session_name = ?5, worktree_path = ?6,
                 ssh_host = ?7, status = ?8, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?9",
            params![
                original.project_id,
                original.branch_name,
                original.base_branch,
                original.session_type.map(SessionType::as_str),
                original.session_name,
                original.worktree_path,
                original.ssh_host,
                original.status.as_str(),
                original.id,
            ],
        )?;
        self.task_by_id(&original.id)
    }

    pub fn bind_live_session_if_unassigned(
        &self,
        task_id: &str,
        session_type: SessionType,
        session_name: &str,
        worktree_path: &str,
        ssh_host: Option<&str>,
    ) -> rusqlite::Result<Option<KanbanTask>> {
        self.connection.execute(
            "UPDATE kanban_tasks
             SET session_type = ?1, session_name = ?2, worktree_path = ?3, ssh_host = ?4,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?5 AND (
                 session_type IS NULL
                 OR session_name IS NULL OR session_name = ''
                 OR worktree_path IS NULL OR worktree_path = ''
             )",
            params![
                session_type.as_str(),
                session_name,
                worktree_path,
                ssh_host,
                task_id,
            ],
        )?;
        self.task_by_id(task_id)
    }

    pub fn more_done_tasks(
        &self,
        offset: u32,
        limit: u32,
    ) -> rusqlite::Result<(Vec<KanbanTask>, u32)> {
        let tasks = self.tasks_where(
            "WHERE status = 'done'
             ORDER BY display_order ASC, created_at ASC
             LIMIT ?1 OFFSET ?2",
            params![i64::from(limit), i64::from(offset)],
        )?;
        let done_total = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM kanban_tasks WHERE status = 'done'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count as u32)?;

        Ok((tasks, done_total))
    }

    pub fn get_app_setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT value FROM app_settings WHERE key = ?1")?;
        let mut rows = statement.query([key])?;

        rows.next()?.map(|row| row.get(0)).transpose()
    }

    pub fn set_app_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.connection.execute(
            "INSERT INTO app_settings (id, key, value, created_at, updated_at)
             VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
            params![format!("setting-{key}"), key, value],
        )?;

        Ok(())
    }

    pub fn app_setting_bool(&self, key: &str, default: bool) -> rusqlite::Result<bool> {
        Ok(match self.get_app_setting(key)?.as_deref() {
            Some("true") => true,
            Some("false") => false,
            Some(_) | None => default,
        })
    }

    pub fn app_setting_u64(&self, key: &str, default: u64) -> rusqlite::Result<u64> {
        Ok(self
            .get_app_setting(key)?
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(default))
    }

    pub fn sidebar_default_collapsed(&self) -> rusqlite::Result<bool> {
        Ok(self
            .get_app_setting("sidebar_default_collapsed")?
            .as_deref()
            == Some("true"))
    }

    pub fn set_sidebar_default_collapsed(&self, collapsed: bool) -> rusqlite::Result<()> {
        self.set_app_setting(
            "sidebar_default_collapsed",
            if collapsed { "true" } else { "false" },
        )
    }

    pub fn sidebar_hint_dismissed(&self) -> rusqlite::Result<bool> {
        Ok(self.get_app_setting("sidebar_hint_dismissed")?.as_deref() == Some("true"))
    }

    pub fn dismiss_sidebar_hint(&self) -> rusqlite::Result<()> {
        self.set_app_setting("sidebar_hint_dismissed", "true")
    }

    pub fn done_alert_dismissed(&self) -> rusqlite::Result<bool> {
        self.app_setting_bool("done_alert_dismissed", false)
    }

    pub fn dismiss_done_alert(&self) -> rusqlite::Result<()> {
        self.set_app_setting("done_alert_dismissed", "true")
    }

    pub fn vim_mode_enabled(&self) -> rusqlite::Result<bool> {
        self.app_setting_bool("vim_mode_enabled", true)
    }

    pub fn set_vim_mode_enabled(&self, enabled: bool) -> rusqlite::Result<()> {
        self.set_app_setting("vim_mode_enabled", if enabled { "true" } else { "false" })
    }

    pub fn theme_preference(&self) -> rusqlite::Result<ThemePreference> {
        Ok(self
            .get_app_setting("theme_preference")?
            .as_deref()
            .and_then(|value| ThemePreference::parse(value).ok())
            .unwrap_or(ThemePreference::System))
    }

    pub fn set_theme_preference(&self, theme_preference: ThemePreference) -> rusqlite::Result<()> {
        self.set_app_setting("theme_preference", theme_preference.as_str())
    }

    pub fn default_session_type(&self) -> rusqlite::Result<SessionType> {
        Ok(self
            .get_app_setting("default_session_type")?
            .as_deref()
            .and_then(|value| SessionType::parse(value).ok())
            .unwrap_or(SessionType::Tmux))
    }

    pub fn set_default_session_type(&self, session_type: SessionType) -> rusqlite::Result<()> {
        self.set_app_setting("default_session_type", session_type.as_str())
    }

    pub fn task_search_shortcut(&self) -> rusqlite::Result<String> {
        Ok(self
            .get_app_setting("task_search_shortcut")?
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_TASK_SEARCH_SHORTCUT.to_owned()))
    }

    pub fn set_task_search_shortcut(&self, shortcut: &str) -> rusqlite::Result<()> {
        let normalized = shortcut.trim();
        self.set_app_setting(
            "task_search_shortcut",
            if normalized.is_empty() {
                DEFAULT_TASK_SEARCH_SHORTCUT
            } else {
                normalized
            },
        )
    }

    pub fn background_sync_settings(&self) -> rusqlite::Result<BackgroundSyncSettings> {
        Ok(BackgroundSyncSettings {
            is_enabled: self.app_setting_bool("background_sync_enabled", true)?,
            interval_ms: self.app_setting_u64(
                "background_sync_interval_ms",
                DEFAULT_BACKGROUND_SYNC_INTERVAL_MS,
            )?,
        })
    }

    pub fn set_background_sync_enabled(&self, enabled: bool) -> rusqlite::Result<()> {
        self.set_app_setting(
            "background_sync_enabled",
            if enabled { "true" } else { "false" },
        )
    }

    pub fn set_background_sync_interval_ms(&self, interval_ms: u64) -> rusqlite::Result<()> {
        self.set_app_setting(
            "background_sync_interval_ms",
            &interval_ms.max(1).to_string(),
        )
    }

    pub fn release_update_dismissed_versions(&self) -> rusqlite::Result<Vec<String>> {
        let Some(value) = self.get_app_setting("release_update_dismissed_versions")? else {
            return Ok(Vec::new());
        };
        let Ok(parsed) = serde_json::from_str::<Vec<String>>(&value) else {
            return Ok(Vec::new());
        };
        let mut versions = Vec::new();
        for version in parsed {
            let normalized = version.trim();
            if !normalized.is_empty() && !versions.iter().any(|existing| existing == normalized) {
                versions.push(normalized.to_owned());
            }
        }
        Ok(versions)
    }

    pub fn dismiss_release_update_version(&self, version: &str) -> rusqlite::Result<()> {
        let normalized = version.trim();
        if normalized.is_empty() {
            return Ok(());
        }

        let mut versions = self.release_update_dismissed_versions()?;
        if !versions.iter().any(|version| version == normalized) {
            versions.push(normalized.to_owned());
        }
        let serialized = serde_json::to_string(&versions)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        self.set_app_setting("release_update_dismissed_versions", &serialized)
    }

    pub fn notification_statuses(&self) -> rusqlite::Result<Vec<String>> {
        let Some(value) = self.get_app_setting("notification_statuses")? else {
            return Ok(DEFAULT_NOTIFICATION_STATUSES
                .iter()
                .map(|status| (*status).to_owned())
                .collect());
        };

        Ok(
            serde_json::from_str::<Vec<String>>(&value).unwrap_or_else(|_| {
                DEFAULT_NOTIFICATION_STATUSES
                    .iter()
                    .map(|status| (*status).to_owned())
                    .collect()
            }),
        )
    }

    pub fn notification_enabled(&self) -> rusqlite::Result<bool> {
        self.app_setting_bool("notification_enabled", true)
    }

    pub fn set_notification_enabled(&self, enabled: bool) -> rusqlite::Result<()> {
        self.set_app_setting(
            "notification_enabled",
            if enabled { "true" } else { "false" },
        )
    }

    pub fn set_notification_statuses(&self, statuses: &[String]) -> rusqlite::Result<()> {
        let mut normalized = Vec::new();
        for status in statuses {
            let parsed = TaskStatus::parse(status)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
            let value = parsed.as_str().to_owned();
            if !normalized.contains(&value) {
                normalized.push(value);
            }
        }
        let serialized = serde_json::to_string(&normalized)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        self.set_app_setting("notification_statuses", &serialized)
    }

    pub fn get_global_pane_layout(&self) -> rusqlite::Result<Option<PaneLayoutConfig>> {
        self.query_optional_pane_layout(
            "WHERE is_global = 1
             ORDER BY created_at ASC
             LIMIT 1",
            [],
        )
    }

    pub fn get_project_pane_layout(
        &self,
        project_id: &str,
    ) -> rusqlite::Result<Option<PaneLayoutConfig>> {
        self.query_optional_pane_layout(
            "WHERE project_id = ?1
             ORDER BY created_at ASC
             LIMIT 1",
            [project_id],
        )
    }

    pub fn get_effective_pane_layout(
        &self,
        project_id: Option<&str>,
    ) -> rusqlite::Result<Option<PaneLayoutConfig>> {
        if let Some(project_id) = project_id
            && let Some(config) = self.get_project_pane_layout(project_id)?
        {
            return Ok(Some(config));
        }

        self.get_global_pane_layout()
    }

    pub fn get_all_pane_layouts(&self) -> rusqlite::Result<Vec<PaneLayoutConfig>> {
        let mut statement = self.connection.prepare(
            "SELECT id, layout_type, panes, project_id, is_global, created_at, updated_at
             FROM pane_layout_configs
             ORDER BY is_global DESC, created_at ASC",
        )?;

        statement
            .query_map([], map_pane_layout)?
            .collect::<rusqlite::Result<Vec<_>>>()
    }

    pub fn save_pane_layout(
        &self,
        input: SavePaneLayoutInput,
    ) -> rusqlite::Result<PaneLayoutConfig> {
        if input.panes.len() != input.layout_type.pane_count()
            || input
                .panes
                .iter()
                .enumerate()
                .any(|(index, pane)| pane.position != index as u32)
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "pane layout commands must match the layout count and contiguous positions"
                    .to_owned(),
            ));
        }
        let existing = if input.is_global {
            self.get_global_pane_layout()?
        } else if let Some(project_id) = input.project_id.as_deref() {
            self.get_project_pane_layout(project_id)?
        } else {
            None
        };
        let panes = serde_json::to_string(&input.panes)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;

        if let Some(existing) = existing {
            self.connection.execute(
                "UPDATE pane_layout_configs
                 SET layout_type = ?1, panes = ?2, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?3",
                params![input.layout_type.as_str(), panes, existing.id],
            )?;
            return self
                .pane_layout_by_id(&existing.id)?
                .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows);
        }

        let id = if input.is_global {
            "pane-layout-global".to_owned()
        } else if let Some(project_id) = input.project_id.as_deref() {
            format!("pane-layout-{project_id}")
        } else {
            "pane-layout-local-default".to_owned()
        };
        self.connection.execute(
            "INSERT INTO pane_layout_configs (
                id, layout_type, panes, project_id, is_global, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            params![
                id,
                input.layout_type.as_str(),
                panes,
                input.project_id,
                if input.is_global { 1 } else { 0 },
            ],
        )?;

        self.pane_layout_by_id(&id)?
            .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn delete_pane_layout(&self, id: &str) -> rusqlite::Result<bool> {
        let changed = self
            .connection
            .execute("DELETE FROM pane_layout_configs WHERE id = ?1", [id])?;

        Ok(changed > 0)
    }

    pub fn pane_layout_by_id(&self, id: &str) -> rusqlite::Result<Option<PaneLayoutConfig>> {
        self.query_optional_pane_layout("WHERE id = ?1", [id])
    }

    fn query_optional_pane_layout(
        &self,
        clause: &str,
        params: impl Params,
    ) -> rusqlite::Result<Option<PaneLayoutConfig>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT id, layout_type, panes, project_id, is_global, created_at, updated_at
             FROM pane_layout_configs
             {clause}"
        ))?;
        let mut rows = statement.query(params)?;

        rows.next()?.map(map_pane_layout).transpose()
    }

    fn tasks_where(&self, clause: &str, params: impl Params) -> rusqlite::Result<Vec<KanbanTask>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT id, title, description, status, branch_name, worktree_path, session_type,
                    session_name, ssh_host, agent_type, project_id, base_branch, pr_url, priority,
                    display_order, created_at, updated_at
             FROM kanban_tasks
             {clause}"
        ))?;

        statement
            .query_map(params, map_task)?
            .collect::<rusqlite::Result<Vec<_>>>()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct TableContract {
    pub name: &'static str,
    pub min_rows: u32,
    pub columns: &'static [&'static str],
}

impl TableContract {
    pub const fn new(name: &'static str, min_rows: u32, columns: &'static [&'static str]) -> Self {
        Self {
            name,
            min_rows,
            columns,
        }
    }
}

pub const PROJECT_COLUMNS: &[&str] = &[
    "id",
    "name",
    "repo_path",
    "default_branch",
    "ssh_host",
    "is_worktree",
    "color",
    "created_at",
];

pub const KANBAN_TASK_COLUMNS: &[&str] = &[
    "id",
    "title",
    "description",
    "status",
    "branch_name",
    "worktree_path",
    "session_type",
    "session_name",
    "ssh_host",
    "agent_type",
    "project_id",
    "base_branch",
    "pr_url",
    "priority",
    "display_order",
    "created_at",
    "updated_at",
];

pub const PANE_LAYOUT_COLUMNS: &[&str] = &[
    "id",
    "layout_type",
    "panes",
    "project_id",
    "is_global",
    "created_at",
    "updated_at",
];

pub const APP_SETTING_COLUMNS: &[&str] = &["id", "key", "value", "created_at", "updated_at"];

pub const TABLE_CONTRACTS: &[TableContract] = &[
    TableContract::new("app_settings", 1, APP_SETTING_COLUMNS),
    TableContract::new("kanban_tasks", 12, KANBAN_TASK_COLUMNS),
    TableContract::new("pane_layout_configs", 1, PANE_LAYOUT_COLUMNS),
    TableContract::new("projects", 3, PROJECT_COLUMNS),
];

pub const REQUIRED_TASK_STATUSES: &[&str] = &["todo", "progress", "pending", "review", "done"];

pub fn seed_db_path_from_crate_manifest(crate_manifest_dir: impl AsRef<Path>) -> PathBuf {
    crate_manifest_dir
        .as_ref()
        .join("../../../qa/seed/kanvibe-seed.sqlite")
}

fn map_project(row: &Row<'_>) -> rusqlite::Result<Project> {
    let is_worktree = row.get::<_, i64>("is_worktree")? != 0;

    Ok(Project {
        id: row.get("id")?,
        name: row.get("name")?,
        repo_path: row.get("repo_path")?,
        default_branch: row.get("default_branch")?,
        ssh_host: row.get("ssh_host")?,
        is_worktree,
        color: row.get("color")?,
        created_at: row.get("created_at")?,
    })
}

fn map_task(row: &Row<'_>) -> rusqlite::Result<KanbanTask> {
    let status = parse_required_enum(row, "status", TaskStatus::parse)?;
    let session_type = parse_optional_enum(row, "session_type", SessionType::parse)?;
    let priority = parse_optional_enum(row, "priority", TaskPriority::parse)?;

    Ok(KanbanTask {
        id: row.get("id")?,
        title: row.get("title")?,
        description: row.get("description")?,
        status,
        branch_name: row.get("branch_name")?,
        worktree_path: row.get("worktree_path")?,
        session_type,
        session_name: row.get("session_name")?,
        ssh_host: row.get("ssh_host")?,
        agent_type: row.get("agent_type")?,
        project_id: row.get("project_id")?,
        base_branch: row.get("base_branch")?,
        pr_url: row.get("pr_url")?,
        priority,
        display_order: row.get("display_order")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn map_pane_layout(row: &Row<'_>) -> rusqlite::Result<PaneLayoutConfig> {
    let layout_type = parse_required_enum(row, "layout_type", PaneLayoutType::parse)?;
    let panes_json = row.get::<_, String>("panes")?;
    let panes = serde_json::from_str::<Vec<PaneCommand>>(&panes_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error))
    })?;

    Ok(PaneLayoutConfig {
        id: row.get("id")?,
        layout_type,
        panes,
        project_id: row.get("project_id")?,
        is_global: row.get::<_, i64>("is_global")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn parse_required_enum<T>(
    row: &Row<'_>,
    column: &'static str,
    parse: impl FnOnce(&str) -> Result<T, ParseEnumError>,
) -> rusqlite::Result<T> {
    let value = row.get::<_, String>(column)?;
    parse(&value)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error)))
}

fn parse_optional_enum<T>(
    row: &Row<'_>,
    column: &'static str,
    parse: impl FnOnce(&str) -> Result<T, ParseEnumError>,
) -> rusqlite::Result<Option<T>> {
    row.get::<_, Option<String>>(column)?
        .map(|value| {
            parse(&value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error))
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeSet,
        fs,
        sync::{Arc, Barrier},
        thread,
        time::Duration,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn pull_request_url_updates_only_when_value_changes_and_done_tasks_are_inactive() {
        let path = writable_seed_copy("pull-request-sync");
        let database = KanvibeDb::open_read_write(&path).expect("open PR fixture");
        let task = database
            .active_tasks()
            .expect("active tasks")
            .into_iter()
            .next()
            .expect("seed active task");
        let pull_request_url = "https://github.com/acme/repo/pull/310";

        assert!(
            database
                .set_task_pr_url_if_changed(&task.id, pull_request_url)
                .expect("first URL update")
        );
        assert!(
            !database
                .set_task_pr_url_if_changed(&task.id, pull_request_url)
                .expect("idempotent URL update")
        );
        assert_eq!(
            database
                .task_by_id(&task.id)
                .expect("updated task")
                .expect("task exists")
                .pr_url
                .as_deref(),
            Some(pull_request_url)
        );
        database
            .set_task_status_preserving_resources(&task.id, TaskStatus::Done)
            .expect("mark task done");
        assert!(
            database
                .active_tasks()
                .expect("active tasks after done")
                .iter()
                .all(|active| active.id != task.id)
        );
        fs::remove_file(path).expect("remove PR fixture");
    }

    #[test]
    fn locked_and_full_database_writes_fail_without_partial_task_rows() {
        let locked_path = writable_seed_copy("locked-write");
        let locker = Connection::open(&locked_path).expect("open lock owner");
        locker
            .execute_batch("BEGIN EXCLUSIVE")
            .expect("acquire exclusive lock");
        let locked_database = KanvibeDb::open_read_write_with_timeout(&locked_path, Duration::ZERO)
            .expect("open zero-timeout database");
        let locked_error = locked_database
            .create_task(CreateTaskInput {
                id: Some("qa-locked-write".to_owned()),
                title: Some("Locked write".to_owned()),
                ..CreateTaskInput::default()
            })
            .expect_err("exclusive lock must reject the write");
        assert!(matches!(
            locked_error.sqlite_error_code(),
            Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
        ));
        locker.execute_batch("ROLLBACK").expect("release lock");
        let locked_reader = Connection::open(&locked_path).expect("inspect locked fixture");
        assert_eq!(
            locked_reader
                .query_row(
                    "SELECT COUNT(1) FROM kanban_tasks WHERE id = 'qa-locked-write'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("locked row count"),
            0
        );

        let full_path = writable_seed_copy("full-write");
        let full_database = KanvibeDb::open_read_write(&full_path).expect("open capped database");
        let page_count = full_database
            .connection
            .query_row("PRAGMA page_count", [], |row| row.get::<_, i64>(0))
            .expect("read page count");
        full_database
            .connection
            .pragma_update(None, "max_page_count", page_count)
            .expect("cap database pages");
        let full_result = full_database.create_task(CreateTaskInput {
            id: Some("qa-full-write".to_owned()),
            title: Some("Full write".to_owned()),
            description: Some("x".repeat(2 * 1024 * 1024)),
            ..CreateTaskInput::default()
        });
        let Err(full_error) = full_result else {
            panic!("page cap must reject the write");
        };
        assert_eq!(
            full_error.sqlite_error_code(),
            Some(rusqlite::ErrorCode::DiskFull)
        );
        let full_reader = Connection::open(&full_path).expect("inspect full fixture");
        assert_eq!(
            full_reader
                .query_row(
                    "SELECT COUNT(1) FROM kanban_tasks WHERE id = 'qa-full-write'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("full row count"),
            0
        );
        fs::remove_file(locked_path).expect("remove locked fixture");
        fs::remove_file(full_path).expect("remove full fixture");
    }

    #[test]
    fn concurrent_task_creates_keep_unique_per_status_display_order() {
        const WRITERS: usize = 12;
        let path = writable_seed_copy("concurrent-create");
        let barrier = Arc::new(Barrier::new(WRITERS));
        let handles = (0..WRITERS)
            .map(|index| {
                let path = path.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let database =
                        KanvibeDb::open_read_write(path).expect("open concurrent writer");
                    barrier.wait();
                    database.create_task(CreateTaskInput {
                        id: Some(format!("qa-concurrent-{index}")),
                        title: Some(format!("Concurrent {index}")),
                        status: Some(TaskStatus::Todo),
                        ..CreateTaskInput::default()
                    })
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle
                .join()
                .expect("concurrent writer thread")
                .expect("concurrent task create");
        }
        let reader = Connection::open(&path).expect("inspect concurrent fixture");
        let orders = reader
            .prepare(
                "SELECT display_order FROM kanban_tasks
                 WHERE id LIKE 'qa-concurrent-%' ORDER BY id",
            )
            .expect("prepare concurrent orders")
            .query_map([], |row| row.get::<_, i64>(0))
            .expect("query concurrent orders")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("collect concurrent orders");
        assert_eq!(orders.len(), WRITERS);
        assert_eq!(
            orders.iter().copied().collect::<BTreeSet<_>>().len(),
            WRITERS,
            "concurrent creates must not reuse display_order"
        );
        drop(reader);
        fs::remove_file(path).expect("remove concurrent fixture");
    }

    #[test]
    fn move_task_rolls_back_status_when_reorder_write_fails() {
        let path = writable_seed_copy("move-rollback");
        let database = KanvibeDb::open_read_write(&path).expect("open move fixture");
        let task = database
            .board_snapshot(DONE_PAGE_SIZE)
            .expect("read move fixture")
            .column(TaskStatus::Todo)
            .expect("todo column")
            .tasks[0]
            .clone();
        database
            .connection
            .execute_batch(&format!(
                "CREATE TRIGGER fail_qa_reorder
                 BEFORE UPDATE OF display_order ON kanban_tasks
                 WHEN NEW.id = '{}'
                 BEGIN SELECT RAISE(ABORT, 'injected reorder failure'); END;",
                task.id.replace('\'', "''")
            ))
            .expect("install reorder failure trigger");

        database
            .move_task_to_column(&task.id, TaskStatus::Review, std::slice::from_ref(&task.id))
            .expect_err("injected reorder failure must abort the move");

        assert_eq!(
            database
                .task_by_id(&task.id)
                .expect("read task after failed move")
                .expect("task remains")
                .status,
            TaskStatus::Todo,
            "status and reorder writes must roll back together"
        );
        drop(database);
        fs::remove_file(path).expect("remove move rollback fixture");
    }

    #[test]
    fn sqlite_backup_captures_wal_state_and_is_never_overwritten() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kanvibe-sqlite-backup-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("backup test directory");
        let source_path = root.join("source.sqlite");
        let backup_path = root.join("source.sqlite.electron-backup");
        let source = Connection::open(&source_path).expect("open WAL source");
        source
            .pragma_update(None, "journal_mode", "WAL")
            .expect("enable WAL");
        source
            .execute_batch(
                "CREATE TABLE values_table (value TEXT NOT NULL);
                 INSERT INTO values_table (value) VALUES ('before');",
            )
            .expect("seed WAL source");

        assert!(
            create_sqlite_backup_once(&source_path, &backup_path).expect("create first backup")
        );
        source
            .execute("INSERT INTO values_table (value) VALUES ('after')", [])
            .expect("mutate source after backup");
        assert!(
            !create_sqlite_backup_once(&source_path, &backup_path)
                .expect("repeat backup must reuse snapshot")
        );

        let backup = Connection::open_with_flags(&backup_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open backup");
        let values = backup
            .prepare("SELECT value FROM values_table ORDER BY rowid")
            .expect("prepare backup query")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query backup")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("backup values");
        assert_eq!(values, vec!["before"]);
        drop(backup);
        drop(source);
        fs::remove_dir_all(root).expect("remove backup fixture");
    }

    #[test]
    fn native_migration_baselines_legacy_bootstrap_without_losing_rows() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kanvibe-legacy-baseline-{}-{unique}.sqlite",
            std::process::id()
        ));
        let connection = Connection::open(&path).expect("legacy database");
        connection
            .execute_batch(
                "CREATE TABLE projects (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL UNIQUE,
                    repo_path TEXT NOT NULL,
                    default_branch TEXT NOT NULL DEFAULT 'main',
                    ssh_host TEXT,
                    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );
                 CREATE TABLE kanban_tasks (
                    id TEXT PRIMARY KEY NOT NULL,
                    title TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'todo',
                    branch_name TEXT,
                    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );
                 CREATE TABLE pane_layout_configs (
                    id TEXT PRIMARY KEY NOT NULL,
                    layout_type TEXT NOT NULL,
                    project_id TEXT UNIQUE,
                    is_global INTEGER NOT NULL DEFAULT 0,
                    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );
                 CREATE UNIQUE INDEX UQ_kanban_tasks_branch_name
                    ON kanban_tasks(branch_name);
                 INSERT INTO projects(id, name, repo_path, default_branch)
                    VALUES ('project-1', 'Legacy', '/workspace/legacy', 'main');
                 INSERT INTO kanban_tasks(id, title, status, branch_name)
                    VALUES ('task-1', 'Legacy task', 'todo', 'main');",
            )
            .expect("legacy schema and rows");
        drop(connection);

        let report = migrate_electron_database(&path).expect("baseline legacy database");
        assert!(report.baselined_existing_database);
        assert_eq!(report.applied_migrations, 0);
        let database = KanvibeDb::open_read_only(&path).expect("open migrated database");
        let task = database
            .task_by_id("task-1")
            .expect("read legacy task")
            .expect("legacy task survives");
        assert_eq!(task.title, "Legacy task");
        assert_eq!(task.status, TaskStatus::Todo);
        drop(database);
        let connection = Connection::open(&path).expect("inspect migrated database");
        assert_eq!(
            connection
                .query_row("SELECT COUNT(1) FROM migrations", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("migration count"),
            ELECTRON_MIGRATIONS.len() as i64
        );
        assert!(
            !connection
                .prepare("PRAGMA index_list('kanban_tasks')")
                .expect("task indexes")
                .query_map([], |row| row.get::<_, String>("name"))
                .expect("query indexes")
                .collect::<rusqlite::Result<Vec<_>>>()
                .expect("index names")
                .contains(&"UQ_kanban_tasks_branch_name".to_owned())
        );
        drop(connection);
        let repeat = migrate_electron_database(&path).expect("repeat migration");
        assert!(!repeat.baselined_existing_database);
        assert_eq!(repeat.applied_migrations, 0);
        fs::remove_file(path).expect("remove legacy fixture");
    }

    #[test]
    fn native_migration_resumes_partial_typeorm_history_transactionally() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kanvibe-partial-migrations-{}-{unique}.sqlite",
            std::process::id()
        ));
        migrate_electron_database(&path).expect("create current schema");
        let connection = Connection::open(&path).expect("prepare partial history");
        connection
            .execute(
                "DELETE FROM migrations WHERE timestamp >= ?1",
                [1771166346785_i64],
            )
            .expect("truncate migration history");
        connection
            .execute(
                "INSERT INTO projects(id, name, repo_path, default_branch)
                 VALUES ('project-1', 'Partial', '/workspace/partial', 'develop')",
                [],
            )
            .expect("partial project");
        for (id, created_at) in [
            ("task-b", "2026-01-01T00:00:00.000Z"),
            ("task-a", "2026-01-01T00:00:00.000Z"),
        ] {
            connection
                .execute(
                    "INSERT INTO kanban_tasks(
                        id, title, status, branch_name, project_id, display_order,
                        created_at, updated_at
                     ) VALUES (?1, ?1, 'todo', ?1, 'project-1', 0, ?2, ?2)",
                    params![id, created_at],
                )
                .expect("partial task");
        }
        drop(connection);

        let report = migrate_electron_database(&path).expect("resume migrations");
        assert!(!report.baselined_existing_database);
        assert_eq!(report.applied_migrations, 8);
        let connection = Connection::open(&path).expect("inspect resumed migration");
        let rows = connection
            .prepare(
                "SELECT id, display_order, base_branch
                 FROM kanban_tasks ORDER BY id",
            )
            .expect("prepare migrated tasks")
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .expect("query migrated tasks")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("migrated task rows");
        assert_eq!(
            rows,
            vec![
                ("task-a".to_owned(), 0, "develop".to_owned()),
                ("task-b".to_owned(), 1, "develop".to_owned()),
            ]
        );
        drop(connection);
        fs::remove_file(path).expect("remove partial fixture");
    }

    #[test]
    fn native_migration_rolls_back_all_schema_changes_after_mid_transaction_failure() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kanvibe-failed-migration-{}-{unique}.sqlite",
            std::process::id()
        ));
        let connection = Connection::open(&path).expect("failed migration fixture");
        connection
            .execute_batch(
                "CREATE TABLE projects (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL UNIQUE,
                    repo_path TEXT NOT NULL,
                    default_branch TEXT NOT NULL DEFAULT 'main'
                 );
                 CREATE TABLE kanban_tasks (
                    id TEXT PRIMARY KEY NOT NULL,
                    title TEXT NOT NULL
                 );
                 CREATE TABLE migrations (unexpected_column TEXT);",
            )
            .expect("incompatible migration history");
        drop(connection);

        assert!(
            migrate_electron_database(&path)
                .expect_err("invalid migration table must fail")
                .to_string()
                .contains("timestamp")
        );

        let connection = Connection::open(&path).expect("inspect rolled back database");
        let project_columns = connection
            .prepare("PRAGMA table_info('projects')")
            .expect("project columns")
            .query_map([], |row| row.get::<_, String>("name"))
            .expect("query project columns")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("project column names");
        assert!(
            !project_columns.contains(&"is_worktree".to_owned()),
            "schema repair before the failure must roll back"
        );
        let migration_columns = connection
            .prepare("PRAGMA table_info('migrations')")
            .expect("migration columns")
            .query_map([], |row| row.get::<_, String>("name"))
            .expect("query migration columns")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("migration column names");
        assert_eq!(migration_columns, vec!["unexpected_column"]);
        drop(connection);
        fs::remove_file(path).expect("remove failed migration fixture");
    }

    #[test]
    fn schema_contract_tracks_electron_seed_tables() {
        let names = TABLE_CONTRACTS
            .iter()
            .map(|table| table.name)
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                "app_settings",
                "kanban_tasks",
                "pane_layout_configs",
                "projects"
            ]
        );
    }

    #[test]
    fn required_statuses_match_board_columns() {
        let statuses = TaskStatus::ALL.map(TaskStatus::as_str);

        assert_eq!(REQUIRED_TASK_STATUSES, statuses);
    }

    #[test]
    fn typed_app_settings_preserve_electron_defaults_and_updates() {
        let path = writable_seed_copy("core-settings");
        let database = KanvibeDb::open_read_write(path).expect("seed copy should open");

        assert!(!database.sidebar_default_collapsed().expect("sidebar"));
        assert!(
            database
                .vim_mode_enabled()
                .expect("vim starts enabled in seed")
        );
        assert_eq!(
            database.default_session_type().expect("default session"),
            SessionType::Tmux
        );
        assert_eq!(
            database
                .task_search_shortcut()
                .expect("task search shortcut"),
            DEFAULT_TASK_SEARCH_SHORTCUT
        );
        assert_eq!(
            database.theme_preference().expect("theme"),
            ThemePreference::Dark
        );

        database
            .set_vim_mode_enabled(true)
            .expect("vim setting update");
        database
            .set_theme_preference(ThemePreference::Dark)
            .expect("theme setting update");
        database
            .set_default_session_type(SessionType::Zellij)
            .expect("session type setting update");
        database
            .set_task_search_shortcut(" Mod+Shift+K ")
            .expect("shortcut update");

        assert!(database.vim_mode_enabled().expect("vim after update"));
        assert_eq!(
            database.theme_preference().expect("theme after update"),
            ThemePreference::Dark
        );
        assert_eq!(
            database
                .default_session_type()
                .expect("session after update"),
            SessionType::Zellij
        );
        assert_eq!(
            database
                .task_search_shortcut()
                .expect("shortcut after update"),
            "Mod+Shift+K"
        );
    }

    #[test]
    fn pane_layouts_read_effective_fallback_and_project_override() {
        let path = writable_seed_copy("core-pane-layout");
        let database = KanvibeDb::open_read_write(path).expect("seed copy should open");

        let global = database
            .get_global_pane_layout()
            .expect("global layout query")
            .expect("seed global layout");
        assert_eq!(global.layout_type, PaneLayoutType::Quad);
        assert_eq!(global.panes.len(), 4);

        let project = database
            .get_project_pane_layout("qa-project-kanvibe")
            .expect("project layout query")
            .expect("seed project layout");
        assert_eq!(project.layout_type, PaneLayoutType::Horizontal2);

        let saved = database
            .save_pane_layout(SavePaneLayoutInput {
                layout_type: PaneLayoutType::Vertical2,
                panes: vec![
                    PaneCommand {
                        position: 0,
                        command: "pnpm desktop:dev".to_owned(),
                    },
                    PaneCommand {
                        position: 1,
                        command: "git status --short".to_owned(),
                    },
                ],
                project_id: Some("qa-project-kanvibe".to_owned()),
                is_global: false,
            })
            .expect("project layout save");

        assert_eq!(saved.id, "qa-layout-kanvibe-horizontal");
        assert_eq!(saved.layout_type, PaneLayoutType::Vertical2);
        assert_eq!(
            database
                .get_effective_pane_layout(Some("qa-project-kanvibe"))
                .expect("effective project layout")
                .expect("effective project config")
                .layout_type,
            PaneLayoutType::Vertical2
        );
        assert_eq!(
            database
                .get_effective_pane_layout(Some("missing-project"))
                .expect("effective fallback")
                .expect("global fallback")
                .layout_type,
            PaneLayoutType::Quad
        );
    }

    fn writable_seed_copy(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kanvibe-{name}-{}-{unique}.sqlite",
            std::process::id()
        ));

        fs::copy(
            seed_db_path_from_crate_manifest(env!("CARGO_MANIFEST_DIR")),
            &path,
        )
        .expect("seed copy");
        path
    }
}
