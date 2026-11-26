//! 프로젝트 서비스가 쓰는 DB 조회·저장. TS TypeORM 호출(`findOneBy`, 조건부 `update`·`delete`, 트랜잭션)과 같은 조건을 SQL로 옮긴다.

use kanvibe_db::Database;
use kanvibe_db::repositories::projects::{find_project, list_projects};
use kanvibe_db::repositories::tasks::{NewKanbanTask, insert_task, list_tasks, update_task};
use kanvibe_domain::project::Project;
use kanvibe_domain::task::KanbanTask;
use rusqlite::params;

use crate::error::{ServiceError, ServiceResult};

pub(super) fn all_projects(database: &Database) -> ServiceResult<Vec<Project>> {
    Ok(list_projects(&database.connection())?)
}

pub(super) fn project_by_id(
    database: &Database,
    project_id: &str,
) -> ServiceResult<Option<Project>> {
    Ok(find_project(&database.connection(), project_id)?)
}

pub(super) fn task_by_id(database: &Database, task_id: &str) -> ServiceResult<Option<KanbanTask>> {
    Ok(kanvibe_db::repositories::tasks::find_task(
        &database.connection(),
        task_id,
    )?)
}

fn all_tasks(database: &Database) -> ServiceResult<Vec<KanbanTask>> {
    Ok(list_tasks(&database.connection())?)
}

/// `findOneBy({ projectId, branchName })`
pub(super) fn project_branch_task(
    database: &Database,
    project_id: &str,
    branch_name: &str,
) -> ServiceResult<Option<KanbanTask>> {
    Ok(all_tasks(database)?.into_iter().find(|task| {
        task.project_id.as_deref() == Some(project_id)
            && task.branch_name.as_deref() == Some(branch_name)
    }))
}

/// `findOneBy({ branchName, projectId: IsNull() })`
pub(super) fn orphan_branch_task(
    database: &Database,
    branch_name: &str,
) -> ServiceResult<Option<KanbanTask>> {
    Ok(all_tasks(database)?
        .into_iter()
        .find(|task| task.project_id.is_none() && task.branch_name.as_deref() == Some(branch_name)))
}

/// `findOneBy({ projectId, baseBranch, branchName })`
pub(super) fn project_main_branch_task(
    database: &Database,
    project_id: &str,
    default_branch: &str,
) -> ServiceResult<Option<KanbanTask>> {
    Ok(project_branch_task(database, project_id, default_branch)?
        .filter(|task| task.base_branch.as_deref() == Some(default_branch)))
}

pub(super) fn project_tasks(
    database: &Database,
    project_id: &str,
) -> ServiceResult<Vec<KanbanTask>> {
    Ok(all_tasks(database)?
        .into_iter()
        .filter(|task| task.project_id.as_deref() == Some(project_id))
        .collect())
}

pub(super) fn insert_new_task(
    database: &Database,
    task: &NewKanbanTask,
) -> ServiceResult<KanbanTask> {
    Ok(insert_task(&database.connection(), task)?)
}

/// `save(task)`. 수정 시각을 지금으로 바꾼 저장 결과를 다시 읽어 돌려준다
pub(super) fn save_task(database: &Database, task: &KanbanTask) -> ServiceResult<KanbanTask> {
    let connection = database.connection();
    update_task(&connection, task)?;
    let saved = kanvibe_db::repositories::tasks::find_task(&connection, &task.id)?;
    saved.ok_or_else(|| ServiceError::new(format!("task를 찾을 수 없습니다: {}", task.id)))
}

/// `delete({ id, projectId, worktreePath })`. 지운 행이 있으면 `true`
pub(super) fn delete_project_worktree_task(
    database: &Database,
    task: &KanbanTask,
    project_id: &str,
) -> ServiceResult<bool> {
    let affected = database
        .connection()
        .execute(
            "DELETE FROM kanban_tasks WHERE id = ?1 AND project_id = ?2 AND worktree_path = ?3",
            params![task.id, project_id, task.worktree_path],
        )
        .map_err(|error| ServiceError::new(error.to_string()))?;
    Ok(affected > 0)
}

/// 프로젝트와 그 task를 한 트랜잭션에서 지운다. 프로젝트가 없으면 `false`
pub(super) fn delete_project_with_tasks(
    database: &Database,
    project_id: &str,
) -> ServiceResult<bool> {
    let mut connection = database.connection();
    let transaction = connection
        .transaction()
        .map_err(|error| ServiceError::new(error.to_string()))?;
    let exists = transaction
        .query_row("SELECT 1 FROM projects WHERE id = ?1", [project_id], |_| {
            Ok(())
        })
        .is_ok();
    if !exists {
        return Ok(false);
    }
    let run = |sql: &str| {
        transaction
            .execute(sql, [project_id])
            .map_err(|error| ServiceError::new(error.to_string()))
    };
    run("DELETE FROM kanban_tasks WHERE project_id = ?1")?;
    run("DELETE FROM projects WHERE id = ?1")?;
    transaction
        .commit()
        .map_err(|error| ServiceError::new(error.to_string()))?;
    Ok(true)
}

/// `remove(project)`. 등록을 되돌릴 때 쓴다
pub(super) fn remove_project(database: &Database, project_id: &str) -> ServiceResult<()> {
    Ok(kanvibe_db::repositories::projects::delete_project(
        &database.connection(),
        project_id,
    )?)
}

/// 이미 있는 행에만 색상을 바꾼다. 읽어 둔 색상이 아직 DB에 남아 있을 때만 갱신해 그 사이 사용자 편집을 덮지 않는다
pub(super) fn update_existing_project_color(
    database: &Database,
    project_id: &str,
    expected_color: Option<&str>,
    color: &str,
) -> ServiceResult<bool> {
    let affected = database
        .connection()
        .execute(
            "UPDATE projects SET color = ?2 WHERE id = ?1 AND color IS ?3",
            params![project_id, color, expected_color],
        )
        .map_err(|error| ServiceError::new(error.to_string()))?;
    Ok(affected > 0)
}

/// 이미 있는 행에만 아이콘을 넣는다. 조회하는 동안 지워진 프로젝트를 되살리지 않는다
pub(super) fn update_existing_project_icon(
    database: &Database,
    project_id: &str,
    icon_data_url: &str,
) -> ServiceResult<bool> {
    let affected = database
        .connection()
        .execute(
            "UPDATE projects SET icon_data_url = ?2 WHERE id = ?1",
            params![project_id, icon_data_url],
        )
        .map_err(|error| ServiceError::new(error.to_string()))?;
    Ok(affected > 0)
}

/// 모든 선택 필드가 빈 새 task. 호출자가 필요한 필드만 채운다
pub(super) fn empty_new_task() -> NewKanbanTask {
    NewKanbanTask {
        title: String::new(),
        description: None,
        status: kanvibe_domain::task::TaskStatus::Todo,
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
