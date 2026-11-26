//! 에이전트 SQLite 기록을 JSON 행으로 읽는다. 로컬은 읽기 전용 rusqlite 연결을 경로별로 재사용하고,
//! 원격과 로컬 대체 경로는 TS와 같은 Python `sqlite3` 스크립트에 base64 JSON 요청을 넘긴다.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};

use base64::Engine;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, Statement};
use serde_json::{Map, Value};

use crate::error::{AiError, AiResult};
use crate::javascript::{js_trim, parse_json, stringify};

/// 이름 붙은 SQL 인자. 키는 `@`·`:` 같은 접두사를 뺀 이름이고, 넣은 순서가 원격 요청 JSON 순서가 된다
pub type SqlParameters = Map<String, Value>;

static NATIVE_CONNECTIONS: LazyLock<Mutex<HashMap<String, Connection>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// TS `getSqliteConnection` + `querySqlite`. 파일이 없으면 `None`이고, 같은 경로의 읽기 전용 연결은 다시 쓴다
pub fn query_native_rows(
    db_path: &str,
    sql: &str,
    parameters: &SqlParameters,
) -> AiResult<Option<Vec<Value>>> {
    let mut connections = NATIVE_CONNECTIONS.lock().expect("SQLite 연결 목록");
    if !Path::new(db_path).exists() {
        connections.remove(db_path);
        return Ok(None);
    }
    if !connections.contains_key(db_path) {
        let connection = open_read_only(db_path)?;
        connections.insert(db_path.to_string(), connection);
    }
    let connection = connections.get(db_path).expect("방금 넣었다");
    query_connection_rows(connection, sql, parameters).map(Some)
}

/// 읽기 전용으로 연다. 다른 프로세스가 쓰는 중이면 `timeout_ms`까지 기다린다
pub fn open_read_only_with_timeout(db_path: &str, timeout_ms: u64) -> AiResult<Connection> {
    let connection = open_read_only(db_path)?;
    connection
        .busy_timeout(std::time::Duration::from_millis(timeout_ms))
        .map_err(sqlite_error)?;
    Ok(connection)
}

fn open_read_only(db_path: &str) -> AiResult<Connection> {
    Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(sqlite_error)
}

pub fn sqlite_error(error: rusqlite::Error) -> AiError {
    match error {
        rusqlite::Error::SqliteFailure(_, Some(message)) => AiError::message(message),
        other => AiError::message(other.to_string()),
    }
}

/// 문장을 돌려 각 행을 `{열 이름: 값}` 객체로 만든다
pub fn query_connection_rows(
    connection: &Connection,
    sql: &str,
    parameters: &SqlParameters,
) -> AiResult<Vec<Value>> {
    let mut statement = connection.prepare(sql).map_err(sqlite_error)?;
    bind_named_parameters(&mut statement, parameters)?;
    let column_names: Vec<String> = statement.column_names().map(str::to_string).collect();
    let mut rows = statement.raw_query();
    let mut values = Vec::new();
    while let Some(row) = rows.next().map_err(sqlite_error)? {
        let mut record = Map::new();
        for (index, name) in column_names.iter().enumerate() {
            let cell = row.get_ref(index).map_err(sqlite_error)?;
            record.insert(name.clone(), sqlite_cell_value(cell));
        }
        values.push(Value::Object(record));
    }
    Ok(values)
}

/// 이름 붙은 자리표시자마다 접두사를 뗀 이름의 값을 묶는다. 값이 없으면 better-sqlite3처럼 실패한다
fn bind_named_parameters(
    statement: &mut Statement<'_>,
    parameters: &SqlParameters,
) -> AiResult<()> {
    for index in 1..=statement.parameter_count() {
        let Some(name) = statement.parameter_name(index).map(str::to_string) else {
            continue;
        };
        let value = parameters.get(&name[1..]).ok_or_else(|| {
            AiError::message(format!("Missing named parameter \"{}\"", &name[1..]))
        })?;
        statement
            .raw_bind_parameter(index, json_to_sql(value))
            .map_err(sqlite_error)?;
    }
    Ok(())
}

fn json_to_sql(value: &Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as SqlValue;
    match value {
        Value::Null => SqlValue::Null,
        Value::Bool(flag) => SqlValue::Integer(i64::from(*flag)),
        Value::Number(number) => match number.as_i64() {
            Some(integer) => SqlValue::Integer(integer),
            None => SqlValue::Real(number.as_f64().unwrap_or_default()),
        },
        Value::String(text) => SqlValue::Text(text.clone()),
        other => SqlValue::Text(stringify(other)),
    }
}

/// SQLite 값 하나를 JSON 값으로 바꾼다. BLOB은 기록 리더가 쓰지 않아 `null`로 둔다
pub fn sqlite_cell_value(cell: ValueRef<'_>) -> Value {
    match cell {
        ValueRef::Null | ValueRef::Blob(_) => Value::Null,
        ValueRef::Integer(integer) => Value::from(integer),
        ValueRef::Real(real) => {
            serde_json::Number::from_f64(real).map_or(Value::Null, Value::Number)
        }
        ValueRef::Text(text) => Value::String(String::from_utf8_lossy(text).into_owned()),
    }
}

/// JSON 요청을 UTF-8 base64로 싼다. 원격 Python 스크립트가 받는 인자 모양이다
pub fn encode_base64_json(request: &Value) -> String {
    base64::engine::general_purpose::STANDARD.encode(stringify(request))
}

/// TS `parseSqliteQueryRows`. 배열이 아니면 실패한다
pub fn parse_query_rows(output: &str, failure_message: &str) -> AiResult<Vec<Value>> {
    match parse_json(js_trim(output)) {
        Some(Value::Array(rows)) => Ok(rows),
        _ => Err(AiError::message(failure_message)),
    }
}

/// 로컬 Python 출력 상한. TS `execFile`의 `maxBuffer`에 해당한다
pub struct LocalPythonLimits {
    pub max_output_bytes: usize,
}

/// 로컬 `python3 -c <script> <argument>`. 사용자 셸 환경을 물려주지 않고 PATH만 넘긴다(CLAUDE.md 환경 규칙)
pub fn run_local_python(
    script: &str,
    argument: &str,
    limits: LocalPythonLimits,
) -> AiResult<String> {
    let mut command = Command::new("python3");
    command
        .args(["-c", script, argument])
        .env_clear()
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null());
    if let Some(search_path) = std::env::var_os("PATH") {
        command.env("PATH", search_path);
    }
    let output = command.output().map_err(|error| AiError::Command {
        message: format!("spawn python3 {error}"),
        is_connection_failure: false,
    })?;
    let failure = |detail: String| AiError::Command {
        message: format!("Command failed: python3 -c {script} {argument}\n{detail}"),
        is_connection_failure: false,
    };
    if !output.status.success() {
        return Err(failure(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    if output.stdout.len() > limits.max_output_bytes {
        return Err(failure("stdout maxBuffer length exceeded".to_string()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// 행 객체의 문자열 필드
pub fn row_text<'a>(row: &'a Value, key: &str) -> Option<&'a str> {
    row.get(key).and_then(Value::as_str)
}

/// 행 객체의 필드. 없으면 `null`이다
pub fn row_value<'a>(row: &'a Value, key: &str) -> &'a Value {
    row.get(key).unwrap_or(&Value::Null)
}

/// TS `row.field ?? fallback`
pub fn row_value_or<'a>(row: &'a Value, key: &str, fallback: &'a Value) -> &'a Value {
    match row.get(key) {
        None | Some(Value::Null) => fallback,
        Some(value) => value,
    }
}

/// 숫자 필드. `null`·없음은 0으로 본다(TS `?? 0`)
pub fn row_number(row: &Value, key: &str) -> f64 {
    row.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_rows_bind_prefixed_names_and_report_missing_files() {
        let (_directory, root) = crate::test_support::temp_home();
        let db_path = format!("{root}/rows.db");
        let connection = Connection::open(&db_path).unwrap();
        connection
            .execute_batch("CREATE TABLE t(id TEXT, n INTEGER, r REAL); INSERT INTO t VALUES ('a', 1, 1.5), ('b', NULL, 2);")
            .unwrap();
        drop(connection);
        let mut parameters = SqlParameters::new();
        parameters.insert("id".to_string(), json!("a"));
        let rows = query_native_rows(
            &db_path,
            "SELECT * FROM t WHERE id = @id OR id = :id;",
            &parameters,
        )
        .unwrap()
        .unwrap();
        assert_eq!(rows, [json!({"id": "a", "n": 1, "r": 1.5})]);
        assert!(
            query_native_rows(&format!("{root}/missing.db"), "SELECT 1", &parameters)
                .unwrap()
                .is_none()
        );
        let missing = query_native_rows(&db_path, "SELECT * FROM t WHERE id = @other", &parameters)
            .unwrap_err();
        assert_eq!(missing.to_string(), "Missing named parameter \"other\"");
    }
}
