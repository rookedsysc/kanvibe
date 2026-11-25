use kanvibe_domain::app_setting::AppSetting;
use rusqlite::{Connection, OptionalExtension, params};

use super::new_record_id;
use crate::error::DatabaseResult;

pub fn find_app_setting(connection: &Connection, key: &str) -> DatabaseResult<Option<AppSetting>> {
    let setting = connection
        .query_row(
            "SELECT id, key, value, created_at, updated_at FROM app_settings WHERE key = ?1",
            [key],
            |row| {
                Ok(AppSetting {
                    id: row.get(0)?,
                    key: row.get(1)?,
                    value: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )
        .optional()?;
    Ok(setting)
}

/// 키가 있으면 값을 바꾸고 없으면 만든다. 값이 그대로면 아무것도 쓰지 않아 수정 시각도 밀리지 않는다
pub fn save_app_setting(connection: &Connection, key: &str, value: &str) -> DatabaseResult<()> {
    match find_app_setting(connection, key)? {
        Some(existing) if existing.value == value => {}
        Some(existing) => {
            connection.execute(
                "UPDATE app_settings SET value = ?2, updated_at = datetime('now') WHERE id = ?1",
                params![existing.id, value],
            )?;
        }
        None => {
            connection.execute(
                "INSERT INTO app_settings (id, key, value) VALUES (?1, ?2, ?3)",
                params![new_record_id(), key, value],
            )?;
        }
    }
    Ok(())
}
