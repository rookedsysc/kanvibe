use kanvibe_domain::task_diff_stats::{TaskDiffStats, TaskDiffStatsRecord};
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::DatabaseResult;

pub fn find_task_diff_stats(
    connection: &Connection,
    task_id: &str,
) -> DatabaseResult<Option<TaskDiffStatsRecord>> {
    let record = connection
        .query_row(
            "SELECT task_id, file_count, additions, deletions, updated_at FROM task_diff_stats WHERE task_id = ?1",
            [task_id],
            |row| {
                Ok(TaskDiffStatsRecord {
                    task_id: row.get(0)?,
                    stats: TaskDiffStats { file_count: row.get(1)?, additions: row.get(2)?, deletions: row.get(3)? },
                    updated_at: row.get(4)?,
                })
            },
        )
        .optional()?;
    Ok(record)
}

/// 집계를 저장한다. 값이 그대로면 아무것도 쓰지 않아 수정 시각도 밀리지 않는다
pub fn save_task_diff_stats(
    connection: &Connection,
    task_id: &str,
    stats: TaskDiffStats,
) -> DatabaseResult<()> {
    match find_task_diff_stats(connection, task_id)? {
        Some(existing) if existing.stats == stats => {}
        Some(_) => {
            connection.execute(
                "UPDATE task_diff_stats SET file_count = ?2, additions = ?3, deletions = ?4, \
                 updated_at = datetime('now') WHERE task_id = ?1",
                params![task_id, stats.file_count, stats.additions, stats.deletions],
            )?;
        }
        None => {
            connection.execute(
                "INSERT INTO task_diff_stats (task_id, file_count, additions, deletions) VALUES (?1, ?2, ?3, ?4)",
                params![task_id, stats.file_count, stats.additions, stats.deletions],
            )?;
        }
    }
    Ok(())
}
