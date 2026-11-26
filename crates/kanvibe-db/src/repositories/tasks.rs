use kanvibe_domain::task::{KanbanTask, SessionType, TaskPriority, TaskStatus};
use rusqlite::{Connection, OptionalExtension, Row, ToSql, params};

use super::{new_record_id, parse_stored_value};
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

/// 상태는 보드 열을 정하므로 모르는 값이면 실패한다. 선택 열(`session_type`, `priority`)의 모르는 값은 비어 있는 것으로 읽어,
/// TS `simple-enum`처럼 한 행 때문에 보드 전체가 불러오기에 실패하지 않게 한다
fn task_from_row(row: &Row<'_>) -> rusqlite::Result<KanbanTask> {
    Ok(KanbanTask {
        id: row.get("id")?,
        title: row.get("title")?,
        description: row.get("description")?,
        status: parse_stored_value("kanban_tasks.status", row.get("status")?, TaskStatus::parse)?,
        branch_name: row.get("branch_name")?,
        worktree_path: row.get("worktree_path")?,
        session_type: row
            .get::<_, Option<String>>("session_type")?
            .as_deref()
            .and_then(SessionType::parse),
        session_name: row.get("session_name")?,
        ssh_host: row.get("ssh_host")?,
        agent_type: row.get("agent_type")?,
        project_id: row.get("project_id")?,
        base_branch: row.get("base_branch")?,
        pr_url: row.get("pr_url")?,
        priority: row
            .get::<_, Option<String>>("priority")?
            .as_deref()
            .and_then(TaskPriority::parse),
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
        .prepare_cached(&sql)?
        .query_row([task_id], task_from_row)
        .optional()?)
}

/// `kanban_tasks` 뒤에 붙일 조건·순서 절(`WHERE ... ORDER BY ... LIMIT ?1`)에 맞는 작업을 SELECT 한 번으로 읽는다
pub fn select_tasks_where(
    connection: &Connection,
    filter_and_order_clause: &str,
    parameters: &[&dyn ToSql],
) -> DatabaseResult<Vec<KanbanTask>> {
    let sql = format!("SELECT {TASK_COLUMNS} FROM kanban_tasks {filter_and_order_clause}");
    let mut statement = connection.prepare_cached(&sql)?;
    let tasks = statement
        .query_map(parameters, task_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tasks)
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

#[cfg(test)]
mod tests {
    use kanvibe_domain::task::TaskStatus;

    use super::{NewKanbanTask, insert_task, list_tasks};
    use crate::database::Database;
    use crate::location::DatabaseLocation;

    fn new_task(title: &str) -> NewKanbanTask {
        NewKanbanTask {
            title: title.to_string(),
            description: None,
            status: TaskStatus::Todo,
            branch_name: None,
            worktree_path: None,
            session_type: None,
            session_name: None,
            ssh_host: None,
            agent_type: None,
            project_id: None,
            base_branch: None,
            pr_url: None,
            priority: None,
        }
    }

    #[test]
    fn an_unknown_session_type_or_priority_reads_as_empty_without_failing_the_list() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&DatabaseLocation {
            database_path: directory.path().join("kanvibe.db"),
            seed_database_path: None,
        })
        .unwrap();
        let connection = database.connection();
        let unknown = insert_task(&connection, &new_task("from a newer build")).unwrap();
        insert_task(&connection, &new_task("ordinary")).unwrap();
        connection
            .execute(
                "UPDATE kanban_tasks SET session_type = 'future-session', priority = 'urgent-new' WHERE id = ?1",
                [&unknown.id],
            )
            .unwrap();

        let tasks = list_tasks(&connection).unwrap();

        let read_back = tasks.iter().find(|task| task.id == unknown.id).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!((read_back.session_type, read_back.priority), (None, None));
    }

    #[test]
    fn an_unknown_status_still_fails_the_read() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&DatabaseLocation {
            database_path: directory.path().join("kanvibe.db"),
            seed_database_path: None,
        })
        .unwrap();
        let connection = database.connection();
        let task = insert_task(&connection, &new_task("broken")).unwrap();
        connection
            .execute(
                "UPDATE kanban_tasks SET status = 'archived-new' WHERE id = ?1",
                [&task.id],
            )
            .unwrap();

        assert!(list_tasks(&connection).is_err());
    }
}
