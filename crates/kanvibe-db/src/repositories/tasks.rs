use kanvibe_domain::task::{KanbanTask, SessionType, TaskPriority, TaskStatus};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{new_record_id, parse_optional_stored_value, parse_stored_value};
use crate::error::DatabaseResult;

const TASK_COLUMNS: &str = "id, title, description, status, branch_name, worktree_path, session_type, \
     session_name, ssh_host, agent_type, project_id, base_branch, pr_url, priority, created_at, updated_at";

/// 새로 만들 작업. id와 시각은 저장할 때 정해진다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewKanbanTask {
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
}

fn task_from_row(row: &Row<'_>) -> rusqlite::Result<KanbanTask> {
    Ok(KanbanTask {
        id: row.get("id")?,
        title: row.get("title")?,
        description: row.get("description")?,
        status: parse_stored_value("kanban_tasks.status", row.get("status")?, TaskStatus::parse)?,
        branch_name: row.get("branch_name")?,
        worktree_path: row.get("worktree_path")?,
        session_type: parse_optional_stored_value(
            "kanban_tasks.session_type",
            row.get("session_type")?,
            SessionType::parse,
        )?,
        session_name: row.get("session_name")?,
        ssh_host: row.get("ssh_host")?,
        agent_type: row.get("agent_type")?,
        project_id: row.get("project_id")?,
        base_branch: row.get("base_branch")?,
        pr_url: row.get("pr_url")?,
        priority: parse_optional_stored_value(
            "kanban_tasks.priority",
            row.get("priority")?,
            TaskPriority::parse,
        )?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn insert_task(connection: &Connection, task: &NewKanbanTask) -> DatabaseResult<KanbanTask> {
    let task_id = new_record_id();
    connection.execute(
        "INSERT INTO kanban_tasks (id, title, description, status, branch_name, worktree_path, session_type, \
         session_name, ssh_host, agent_type, project_id, base_branch, pr_url, priority) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            task_id,
            task.title,
            task.description,
            task.status.as_str(),
            task.branch_name,
            task.worktree_path,
            task.session_type.map(SessionType::as_str),
            task.session_name,
            task.ssh_host,
            task.agent_type,
            task.project_id,
            task.base_branch,
            task.pr_url,
            task.priority.map(TaskPriority::as_str)
        ],
    )?;
    let inserted = find_task(connection, &task_id)?;
    Ok(inserted.expect("방금 넣은 작업은 같은 연결에서 바로 보인다"))
}

pub fn find_task(connection: &Connection, task_id: &str) -> DatabaseResult<Option<KanbanTask>> {
    let sql = format!("SELECT {TASK_COLUMNS} FROM kanban_tasks WHERE id = ?1");
    Ok(connection
        .query_row(&sql, [task_id], task_from_row)
        .optional()?)
}

/// 보드 기본 순서인 최근 수정순으로 돌려준다
pub fn list_tasks(connection: &Connection) -> DatabaseResult<Vec<KanbanTask>> {
    let sql = format!("SELECT {TASK_COLUMNS} FROM kanban_tasks ORDER BY updated_at DESC");
    let mut statement = connection.prepare(&sql)?;
    let tasks = statement
        .query_map([], task_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tasks)
}

/// 시각을 뺀 모든 값을 덮어쓰고 수정 시각을 지금으로 바꾼다
pub fn update_task(connection: &Connection, task: &KanbanTask) -> DatabaseResult<()> {
    connection.execute(
        "UPDATE kanban_tasks SET title = ?2, description = ?3, status = ?4, branch_name = ?5, \
         worktree_path = ?6, session_type = ?7, session_name = ?8, ssh_host = ?9, agent_type = ?10, \
         project_id = ?11, base_branch = ?12, pr_url = ?13, priority = ?14, updated_at = datetime('now') \
         WHERE id = ?1",
        params![
            task.id,
            task.title,
            task.description,
            task.status.as_str(),
            task.branch_name,
            task.worktree_path,
            task.session_type.map(SessionType::as_str),
            task.session_name,
            task.ssh_host,
            task.agent_type,
            task.project_id,
            task.base_branch,
            task.pr_url,
            task.priority.map(TaskPriority::as_str)
        ],
    )?;
    Ok(())
}

/// 작업의 변경 집계도 함께 지워진다
pub fn delete_task(connection: &Connection, task_id: &str) -> DatabaseResult<()> {
    connection.execute("DELETE FROM kanban_tasks WHERE id = ?1", [task_id])?;
    Ok(())
}
