use kanvibe_domain::project::Project;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::new_record_id;
use crate::error::DatabaseResult;

const PROJECT_COLUMNS: &str =
    "id, name, repo_path, default_branch, ssh_host, is_worktree, color, icon_data_url, created_at";

/// 새로 등록할 프로젝트. id와 등록 시각은 저장할 때 정해진다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProject {
    pub name: String,
    pub repo_path: String,
    pub default_branch: String,
    pub ssh_host: Option<String>,
    pub is_worktree: bool,
    pub color: Option<String>,
    pub icon_data_url: Option<String>,
}

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get("id")?,
        name: row.get("name")?,
        repo_path: row.get("repo_path")?,
        default_branch: row.get("default_branch")?,
        ssh_host: row.get("ssh_host")?,
        is_worktree: row.get::<_, i64>("is_worktree")? != 0,
        color: row.get("color")?,
        icon_data_url: row.get("icon_data_url")?,
        created_at: row.get("created_at")?,
    })
}

pub fn insert_project(connection: &Connection, project: &NewProject) -> DatabaseResult<Project> {
    let project_id = new_record_id();
    connection.execute(
        "INSERT INTO projects (id, name, repo_path, default_branch, ssh_host, is_worktree, color, icon_data_url) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            project_id,
            project.name,
            project.repo_path,
            project.default_branch,
            project.ssh_host,
            project.is_worktree,
            project.color,
            project.icon_data_url
        ],
    )?;
    let inserted = find_project(connection, &project_id)?;
    Ok(inserted.expect("방금 넣은 프로젝트는 같은 연결에서 바로 보인다"))
}

pub fn find_project(connection: &Connection, project_id: &str) -> DatabaseResult<Option<Project>> {
    let sql = format!("SELECT {PROJECT_COLUMNS} FROM projects WHERE id = ?1");
    Ok(connection
        .query_row(&sql, [project_id], project_from_row)
        .optional()?)
}

/// 등록 순서대로 돌려준다
pub fn list_projects(connection: &Connection) -> DatabaseResult<Vec<Project>> {
    let sql = format!("SELECT {PROJECT_COLUMNS} FROM projects ORDER BY created_at ASC");
    let mut statement = connection.prepare(&sql)?;
    let projects = statement
        .query_map([], project_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(projects)
}

/// 등록 시각을 뺀 모든 값을 덮어쓴다
pub fn update_project(connection: &Connection, project: &Project) -> DatabaseResult<()> {
    connection.execute(
        "UPDATE projects SET name = ?2, repo_path = ?3, default_branch = ?4, ssh_host = ?5, \
         is_worktree = ?6, color = ?7, icon_data_url = ?8 WHERE id = ?1",
        params![
            project.id,
            project.name,
            project.repo_path,
            project.default_branch,
            project.ssh_host,
            project.is_worktree,
            project.color,
            project.icon_data_url
        ],
    )?;
    Ok(())
}

/// 이 프로젝트의 작업은 project_id만 비워지고, pane 레이아웃은 함께 지워진다
pub fn delete_project(connection: &Connection, project_id: &str) -> DatabaseResult<()> {
    connection.execute("DELETE FROM projects WHERE id = ?1", [project_id])?;
    Ok(())
}
