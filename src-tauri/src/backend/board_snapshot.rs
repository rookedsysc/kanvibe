use rusqlite::{Connection, Row};
use serde::Serialize;
use tauri::AppHandle;

use super::db::open_database;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardProjectSummary {
    pub id: String,
    pub name: String,
    pub repo_path: String,
    pub default_branch: String,
    pub color: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardTaskSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub branch_name: Option<String>,
    pub session_type: Option<String>,
    pub session_name: Option<String>,
    pub priority: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardSnapshot {
    pub database_path: String,
    pub projects: Vec<BoardProjectSummary>,
    pub tasks: Vec<BoardTaskSummary>,
}

fn map_project(row: &Row<'_>) -> rusqlite::Result<BoardProjectSummary> {
    Ok(BoardProjectSummary {
        id: row.get("id")?,
        name: row.get("name")?,
        repo_path: row.get("repo_path")?,
        default_branch: row.get("default_branch")?,
        color: row.get("color")?,
    })
}

fn map_task(row: &Row<'_>) -> rusqlite::Result<BoardTaskSummary> {
    Ok(BoardTaskSummary {
        id: row.get("id")?,
        title: row.get("title")?,
        status: row.get("status")?,
        project_id: row.get("project_id")?,
        project_name: row.get("project_name")?,
        branch_name: row.get("branch_name")?,
        session_type: row.get("session_type")?,
        session_name: row.get("session_name")?,
        priority: row.get("priority")?,
    })
}

fn table_exists(connection: &Connection, table_name: &str) -> Result<bool, String> {
    let mut statement = connection
        .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1")
        .map_err(|error| format!("table existence query prepare failed: {error}"))?;

    let mut rows = statement
        .query([table_name])
        .map_err(|error| format!("table existence query failed: {error}"))?;

    rows.next()
        .map(|row| row.is_some())
        .map_err(|error| format!("table existence row read failed: {error}"))
}

#[tauri::command]
pub fn get_board_snapshot(app_handle: AppHandle) -> Result<BoardSnapshot, String> {
    let (connection, database_path) = open_database(&app_handle)?;

    let has_projects = table_exists(&connection, "projects")?;
    let has_tasks = table_exists(&connection, "kanban_tasks")?;

    if !has_projects || !has_tasks {
        return Ok(BoardSnapshot {
            database_path,
            projects: Vec::new(),
            tasks: Vec::new(),
        });
    }

    let mut project_statement = connection
        .prepare(
            "SELECT id, name, repo_path, default_branch, color
             FROM projects
             ORDER BY name ASC",
        )
        .map_err(|error| format!("project query prepare failed: {error}"))?;

    let projects = project_statement
        .query_map([], map_project)
        .map_err(|error| format!("project query failed: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("project rows parse failed: {error}"))?;

    let mut task_statement = connection
        .prepare(
            "SELECT
                tasks.id,
                tasks.title,
                tasks.status,
                tasks.project_id,
                projects.name AS project_name,
                tasks.branch_name,
                tasks.session_type,
                tasks.session_name,
                tasks.priority
             FROM kanban_tasks tasks
             LEFT JOIN projects ON projects.id = tasks.project_id
             ORDER BY tasks.display_order ASC, tasks.created_at DESC",
        )
        .map_err(|error| format!("task query prepare failed: {error}"))?;

    let tasks = task_statement
        .query_map([], map_task)
        .map_err(|error| format!("task query failed: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("task rows parse failed: {error}"))?;

    Ok(BoardSnapshot {
        database_path,
        projects,
        tasks,
    })
}
