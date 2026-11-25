//! Devin CLI `sessions.db`의 세션 색인(작업 경로, 제목, 숨김 여부).
//! DB와 WAL을 따로 캐시하지 않고 조회 한 번 안에서만 공유해 숨김·경로 변경을 바로 반영한다.

use std::collections::HashMap;

use serde_json::Value;

use crate::error::{AiError, AiResult};
use crate::host::{CommandOptions, SessionHost, quote_shell_argument};
use crate::sqlite_agent_sessions::table_columns;
use crate::sqlite_rows::{SqlParameters, open_read_only_with_timeout, query_connection_rows};

const INDEX_MAX_BUFFER_BYTES: usize = 4 * 1024 * 1024;
/// TS 템플릿 문자열 그대로. 앞뒤 줄바꿈까지 원격 명령에 실린다
const READ_DEVIN_INDEX: &str = include_str!("devin_session_index/read_index.py");
const INDEX_COLUMNS: [&str; 6] = [
    "id",
    "working_directory",
    "title",
    "created_at",
    "last_activity_at",
    "hidden",
];
const LOCAL_OPEN_TIMEOUT_MS: u64 = 2_000;

/// 세션 ID별 색인 행. 문자열 ID가 없는 행은 버리고, 같은 ID는 뒤 행이 이긴다
pub fn read_devin_session_index(
    host: &SessionHost,
    db_path: &str,
) -> AiResult<HashMap<String, Value>> {
    if !host.path_exists(db_path)? {
        return Ok(HashMap::new());
    }
    let rows = if host.is_remote() {
        let command = format!(
            "python3 -c {} {}",
            quote_shell_argument(READ_DEVIN_INDEX),
            quote_shell_argument(db_path)
        );
        let options = CommandOptions {
            max_buffer_bytes: Some(INDEX_MAX_BUFFER_BYTES),
            timeout_ms: None,
        };
        let output = host.run(&command, options)?;
        serde_json::from_str::<Vec<Value>>(&output)
            .map_err(|error| AiError::message(error.to_string()))?
    } else {
        read_local_index(db_path)?
    };
    Ok(rows
        .into_iter()
        .filter_map(|row| Some((row.get("id")?.as_str()?.to_string(), row)))
        .collect())
}

/// 스크립트와 같은 질의. `sessions` 표에 `id` 열이 없으면 빈 목록이다
fn read_local_index(db_path: &str) -> AiResult<Vec<Value>> {
    let connection = open_read_only_with_timeout(db_path, LOCAL_OPEN_TIMEOUT_MS)?;
    let columns = table_columns(&connection, "sessions")?;
    if !columns.contains("id") {
        return Ok(Vec::new());
    }
    let selected: Vec<&str> = INDEX_COLUMNS
        .into_iter()
        .filter(|name| columns.contains(*name))
        .collect();
    let sql = format!("SELECT {} FROM sessions", selected.join(","));
    query_connection_rows(&connection, &sql, &SqlParameters::new())
}
