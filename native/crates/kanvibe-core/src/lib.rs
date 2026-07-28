use std::{
    error::Error,
    fmt::{Display, Formatter},
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OpenFlags, Params, Row, params, types::Type};
use serde::{Deserialize, Serialize};

pub const DONE_PAGE_SIZE: u32 = 20;

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
    pub branch_name: Option<String>,
    pub base_branch: Option<String>,
    pub session_type: Option<SessionType>,
    pub ssh_host: Option<String>,
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
        connection.pragma_update(None, "foreign_keys", true)?;

        Ok(Self { connection })
    }

    pub fn open_read_write(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
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
        let display_order = self.connection.query_row(
            "SELECT COALESCE(MAX(display_order), -1) + 1 FROM kanban_tasks WHERE status = 'todo'",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        let session_type = input.session_type.map(SessionType::as_str);
        let priority = input.priority.map(TaskPriority::as_str);

        self.connection.execute(
            "INSERT INTO kanban_tasks (
                id, title, description, status, branch_name, base_branch, session_type,
                ssh_host, project_id, priority, display_order
             ) VALUES (?1, ?2, ?3, 'todo', ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                title,
                input.description,
                input.branch_name,
                input.base_branch,
                session_type,
                input.ssh_host,
                input.project_id,
                priority,
                display_order,
            ],
        )?;

        self.task_by_id(&id)?
            .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
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
        let done_cleanup = self
            .update_task_status(task_id, new_status)?
            .and_then(|update| update.done_cleanup);
        self.reorder_tasks(dest_ordered_ids)?;

        Ok(done_cleanup)
    }

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
        if let Some(project_id) = project_id {
            if let Some(config) = self.get_project_pane_layout(project_id)? {
                return Ok(Some(config));
            }
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
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

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
