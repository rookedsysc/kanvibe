//! 보드 서비스가 쓰는 조회·부분 갱신 SQL. 행 변환은 kanvibe-db의 `find_task`·`find_project`를 그대로 써서
//! 컬럼 해석 규칙을 한곳에 둔다. TypeORM `update()`처럼 부분 갱신도 수정 시각을 지금으로 바꾼다.
//! 같은 수정 시각끼리는 TypeORM이 정하지 않던 순서를 rowid로 고정한다.

use kanvibe_db::repositories::projects::{find_project, list_projects};
use kanvibe_db::repositories::tasks::find_task;
use kanvibe_db::{Database, DatabaseResult};
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskStatus};
use rusqlite::{Connection, params_from_iter};

fn query_ids(
    connection: &Connection,
    sql: &str,
    parameters: &[&dyn rusqlite::ToSql],
) -> DatabaseResult<Vec<String>> {
    let mut statement = connection.prepare(sql)?;
    let ids = statement
        .query_map(parameters, |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

fn load_tasks(connection: &Connection, ids: &[String]) -> DatabaseResult<Vec<KanbanTask>> {
    let mut tasks = Vec::with_capacity(ids.len());
    for task_id in ids {
        if let Some(task) = find_task(connection, task_id)? {
            tasks.push(task);
        }
    }
    Ok(tasks)
}

/// 조건에 맞는 task를 SQL이 정한 순서대로 읽는다
pub(super) fn select_tasks(
    database: &Database,
    sql: &str,
    parameters: &[&dyn rusqlite::ToSql],
) -> DatabaseResult<Vec<KanbanTask>> {
    let connection = database.connection();
    let ids = query_ids(&connection, sql, parameters)?;
    load_tasks(&connection, &ids)
}

pub(super) fn find_task_by_id(
    database: &Database,
    task_id: &str,
) -> DatabaseResult<Option<KanbanTask>> {
    find_task(&database.connection(), task_id)
}

pub(super) fn find_project_by_id(
    database: &Database,
    project_id: &str,
) -> DatabaseResult<Option<Project>> {
    find_project(&database.connection(), project_id)
}

pub(super) fn all_projects(database: &Database) -> DatabaseResult<Vec<Project>> {
    list_projects(&database.connection())
}

pub(super) fn find_task_id_by_project_and_branch(
    database: &Database,
    project_id: &str,
    branch_name: &str,
) -> DatabaseResult<Option<String>> {
    let ids = query_ids(
        &database.connection(),
        "SELECT id FROM kanban_tasks WHERE project_id = ?1 AND branch_name = ?2 LIMIT 1",
        &[&project_id, &branch_name],
    )?;
    Ok(ids.into_iter().next())
}

pub(super) fn count_done_tasks(database: &Database) -> DatabaseResult<usize> {
    let count: i64 = database.connection().query_row(
        "SELECT COUNT(*) FROM kanban_tasks WHERE status = 'done'",
        [],
        |row| row.get(0),
    )?;
    Ok(usize::try_from(count).unwrap_or_default())
}

/// TS `repo.update(id, { status })`. 바뀐 행 수를 돌려준다
pub(super) fn update_task_status(
    database: &Database,
    task_id: &str,
    status: TaskStatus,
) -> DatabaseResult<usize> {
    Ok(database.connection().execute(
        "UPDATE kanban_tasks SET status = ?1, updated_at = datetime('now') WHERE id = ?2",
        (status.as_str(), task_id),
    )?)
}

/// Done 이동 저장과 그 실패를 되돌릴 때 쓰는 리소스 필드 묶음
pub(super) struct TaskResourceFields<'a> {
    pub status: TaskStatus,
    pub session_type: Option<SessionType>,
    pub session_name: Option<&'a str>,
    pub worktree_path: Option<&'a str>,
}

pub(super) fn update_task_resource_fields(
    database: &Database,
    task_id: &str,
    fields: &TaskResourceFields<'_>,
) -> DatabaseResult<usize> {
    Ok(database.connection().execute(
        "UPDATE kanban_tasks SET status = ?1, session_type = ?2, session_name = ?3, worktree_path = ?4, \
         updated_at = datetime('now') WHERE id = ?5",
        (
            fields.status.as_str(),
            fields.session_type.map(SessionType::as_str),
            fields.session_name,
            fields.worktree_path,
            task_id,
        ),
    )?)
}

/// Done 이동이 실패했을 때 원래 상태·리소스·호스트로 되돌린다
pub(super) fn restore_task_resource_fields(
    database: &Database,
    task: &KanbanTask,
) -> DatabaseResult<usize> {
    Ok(database.connection().execute(
        "UPDATE kanban_tasks SET status = ?1, session_type = ?2, session_name = ?3, worktree_path = ?4, \
         ssh_host = ?5, updated_at = datetime('now') WHERE id = ?6",
        (
            task.status.as_str(),
            task.session_type.map(SessionType::as_str),
            task.session_name.as_deref(),
            task.worktree_path.as_deref(),
            task.ssh_host.as_deref(),
            &task.id,
        ),
    )?)
}

/// 조회하는 동안 task가 지워지거나 브랜치가 바뀌었으면 쓰지 않도록 id와 브랜치가 그대로일 때만 PR URL을 바꾼다
pub(super) fn update_pr_url_if_branch_unchanged(
    database: &Database,
    task_id: &str,
    branch_name: &str,
    pr_url: &str,
) -> DatabaseResult<usize> {
    Ok(database.connection().execute(
        "UPDATE kanban_tasks SET pr_url = ?1, updated_at = datetime('now') WHERE id = ?2 AND branch_name = ?3",
        (pr_url, task_id, branch_name),
    )?)
}

pub(super) fn task_exists_with_branch(
    database: &Database,
    task_id: &str,
    branch_name: &str,
) -> DatabaseResult<bool> {
    let count: i64 = database.connection().query_row(
        "SELECT COUNT(*) FROM kanban_tasks WHERE id = ?1 AND branch_name = ?2",
        (task_id, branch_name),
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// 여러 프로젝트의 색상을 한 번에 바꾼다(TS `repo.update({ id: In(ids) }, { color })`)
pub(super) fn update_project_colors(
    database: &Database,
    project_ids: &[String],
    color: &str,
) -> DatabaseResult<()> {
    if project_ids.is_empty() {
        return Ok(());
    }
    let placeholders = vec!["?"; project_ids.len()].join(", ");
    let sql = format!("UPDATE projects SET color = ? WHERE id IN ({placeholders})");
    let parameters = std::iter::once(color).chain(project_ids.iter().map(String::as_str));
    database
        .connection()
        .execute(&sql, params_from_iter(parameters))?;
    Ok(())
}

pub(super) fn delete_task_record(database: &Database, task_id: &str) -> DatabaseResult<()> {
    kanvibe_db::repositories::tasks::delete_task(&database.connection(), task_id)
}
