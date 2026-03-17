use std::{env, fs, path::PathBuf};

use rusqlite::Connection;
use tauri::{AppHandle, Manager};

const DEFAULT_DATABASE_FILE: &str = "kanvibe.sqlite";

pub fn resolve_database_path(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let configured_path = env::var("KANVIBE_DB_PATH").unwrap_or_default();
    let database_path = if configured_path.trim().is_empty() {
        app_handle
            .path()
            .app_data_dir()
            .map_err(|error| format!("app data directory resolve failed: {error}"))?
            .join(DEFAULT_DATABASE_FILE)
    } else {
        PathBuf::from(configured_path)
    };

    let database_directory = database_path
        .parent()
        .ok_or_else(|| "database directory could not be resolved".to_string())?;

    if !database_directory.exists() {
        fs::create_dir_all(database_directory)
            .map_err(|error| format!("database directory create failed: {error}"))?;
    }

    Ok(database_path)
}

pub fn open_database(app_handle: &AppHandle) -> Result<(Connection, String), String> {
    let database_path = resolve_database_path(app_handle)?;
    let connection =
        Connection::open(&database_path).map_err(|error| format!("sqlite open failed: {error}"))?;
    ensure_schema(&connection)?;
    Ok((connection, database_path.to_string_lossy().to_string()))
}

pub fn ensure_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                repo_path TEXT NOT NULL,
                default_branch TEXT NOT NULL DEFAULT 'main',
                ssh_host TEXT,
                is_worktree INTEGER NOT NULL DEFAULT 0,
                color TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS kanban_tasks (
                id TEXT PRIMARY KEY,
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
                priority TEXT,
                display_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS app_settings (
                id TEXT PRIMARY KEY,
                key TEXT NOT NULL UNIQUE,
                value TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            ",
        )
        .map_err(|error| format!("schema ensure failed: {error}"))
}
