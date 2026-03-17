use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::db::open_database;

pub const MODULE_NAME: &str = "settings";

const SIDEBAR_COLLAPSED_KEY: &str = "sidebar_default_collapsed";
const NOTIFICATION_ENABLED_KEY: &str = "notification_enabled";
const NOTIFICATION_STATUSES_KEY: &str = "notification_statuses";
const DEFAULT_SESSION_TYPE_KEY: &str = "default_session_type";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSettingsSnapshot {
    pub is_enabled: bool,
    pub enabled_statuses: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsSnapshot {
    pub sidebar_default_collapsed: bool,
    pub notification_settings: NotificationSettingsSnapshot,
    pub default_session_type: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsPatch {
    pub sidebar_default_collapsed: Option<bool>,
    pub notification_enabled: Option<bool>,
    pub notification_statuses: Option<Vec<String>>,
    pub default_session_type: Option<String>,
}

fn get_setting(connection: &rusqlite::Connection, key: &str) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1 LIMIT 1",
            [key],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("setting query failed: {error}"))
}

fn upsert_setting(connection: &rusqlite::Connection, key: &str, value: &str) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO app_settings (id, key, value) VALUES (lower(hex(randomblob(16))), ?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
            params![key, value],
        )
        .map_err(|error| format!("setting upsert failed: {error}"))?;
    Ok(())
}

#[tauri::command]
pub fn get_app_settings_snapshot() -> Result<AppSettingsSnapshot, String> {
    let (connection, _) = open_database()?;
    let sidebar_default_collapsed = get_setting(&connection, SIDEBAR_COLLAPSED_KEY)?
        .map(|value| value == "true")
        .unwrap_or(false);
    let notification_enabled = get_setting(&connection, NOTIFICATION_ENABLED_KEY)?
        .map(|value| value != "false")
        .unwrap_or(true);
    let enabled_statuses = get_setting(&connection, NOTIFICATION_STATUSES_KEY)?
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_else(|| vec!["progress".into(), "pending".into(), "review".into()]);
    let default_session_type =
        get_setting(&connection, DEFAULT_SESSION_TYPE_KEY)?.unwrap_or_else(|| "tmux".to_string());

    Ok(AppSettingsSnapshot {
        sidebar_default_collapsed,
        notification_settings: NotificationSettingsSnapshot {
            is_enabled: notification_enabled,
            enabled_statuses,
        },
        default_session_type,
    })
}

#[tauri::command]
pub fn update_app_settings(patch: AppSettingsPatch) -> Result<AppSettingsSnapshot, String> {
    let (connection, _) = open_database()?;

    if let Some(value) = patch.sidebar_default_collapsed {
        upsert_setting(
            &connection,
            SIDEBAR_COLLAPSED_KEY,
            if value { "true" } else { "false" },
        )?;
    }
    if let Some(value) = patch.notification_enabled {
        upsert_setting(
            &connection,
            NOTIFICATION_ENABLED_KEY,
            if value { "true" } else { "false" },
        )?;
    }
    if let Some(value) = patch.notification_statuses {
        let serialized = serde_json::to_string(&value)
            .map_err(|error| format!("notification statuses serialize failed: {error}"))?;
        upsert_setting(&connection, NOTIFICATION_STATUSES_KEY, &serialized)?;
    }
    if let Some(value) = patch.default_session_type {
        upsert_setting(&connection, DEFAULT_SESSION_TYPE_KEY, &value)?;
    }

    drop(connection);
    get_app_settings_snapshot()
}
