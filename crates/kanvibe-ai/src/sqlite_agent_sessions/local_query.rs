//! `read_script.py`를 로컬에서 rusqlite로 그대로 옮긴 것. 질의 문자열과 인자 이름, 결과 행 모양이 스크립트와 같다.

use std::collections::HashSet;

use rusqlite::Connection;
use serde_json::{Value, json};

use super::{AgentDatabaseRequest, SqliteReadResult, table_columns};
use crate::error::{AiError, AiResult};
use crate::providers::AiSessionProvider;
use crate::sqlite_rows::{SqlParameters, open_read_only_with_timeout, query_connection_rows};

/// 스크립트가 provider별로 고르는 테이블과 식
struct TranscriptShape {
    session_table: &'static str,
    source: &'static str,
    role: &'static str,
    content: &'static str,
    visible: &'static str,
    order: String,
    count_query: String,
}

fn is_subset(required: &[&str], columns: &HashSet<String>) -> bool {
    required.iter().all(|column| columns.contains(*column))
}

fn opencode2_shape(connection: &Connection) -> AiResult<TranscriptShape> {
    let message_columns = table_columns(connection, "session_message")?;
    if !is_subset(
        &["session_id", "type", "data", "time_created", "seq"],
        &message_columns,
    ) {
        return Err(AiError::message(
            "OpenCode 2 transcript schema is unreadable",
        ));
    }
    Ok(TranscriptShape {
        session_table: "session_v2",
        source: "session_message m",
        role: "m.type",
        content: "m.data",
        visible: "1=1",
        order: "m.time_created ASC,m.seq ASC".to_string(),
        count_query: "SELECT COUNT(*) FROM session_message m WHERE m.session_id=:sessionId"
            .to_string(),
    })
}

fn zcode_shape(connection: &Connection) -> AiResult<TranscriptShape> {
    let message_columns = table_columns(connection, "message")?;
    let part_columns = table_columns(connection, "part")?;
    let is_readable = is_subset(
        &["id", "session_id", "data", "time_created"],
        &message_columns,
    ) && is_subset(&["id", "message_id", "data", "time_created"], &part_columns);
    if !is_readable {
        return Err(AiError::message("ZCode transcript schema is unreadable"));
    }
    let visible =
        "COALESCE(json_extract(m.data,'$.semantics.transcriptVisibility'),'visible')!='hidden'";
    let message_sequence = if message_columns.contains("sequence") {
        "m.sequence IS NULL ASC,m.sequence ASC,"
    } else {
        ""
    };
    let part_sequence = if part_columns.contains("sequence") {
        "p.sequence IS NULL ASC,p.sequence ASC,"
    } else {
        ""
    };
    Ok(TranscriptShape {
        session_table: "session",
        source: "message m JOIN part p ON p.message_id=m.id",
        role: "json_extract(m.data,'$.role')",
        content: "p.data",
        visible,
        order: format!(
            "{message_sequence}m.time_created ASC,m.rowid ASC,{part_sequence}p.time_created ASC,p.id ASC"
        ),
        count_query: format!(
            "SELECT COUNT(*) FROM message m WHERE m.session_id=:sessionId AND {visible}"
        ),
    })
}

fn script_parameters(request: &AgentDatabaseRequest<'_>) -> SqlParameters {
    let prefix = format!(
        "{}{}",
        request.worktree.trim_end_matches(['/', '\\']),
        request.separator
    );
    let mut parameters = SqlParameters::new();
    parameters.insert("worktree".to_string(), json!(request.worktree));
    parameters.insert("prefix".to_string(), json!(prefix));
    parameters.insert("query".to_string(), json!(request.query.to_lowercase()));
    parameters.insert("sessionId".to_string(), json!(request.session_id));
    parameters
}

fn session_query(shape: &TranscriptShape, title: &str, has_session_id: bool) -> String {
    let scope = "(s.directory=:worktree OR substr(s.directory,1,length(:prefix))=:prefix)";
    let mut query = format!(
        "SELECT s.id,s.directory,{title} AS title,s.time_created,s.time_updated FROM {} s WHERE {scope}",
        shape.session_table
    );
    if has_session_id {
        query.push_str(" AND s.id=:sessionId");
    } else {
        let message_search = format!(
            "EXISTS(SELECT 1 FROM {} WHERE m.session_id=s.id AND {} AND instr(lower({}),:query)>0)",
            shape.source, shape.visible, shape.content
        );
        query.push_str(&format!(
            " AND (:query='' OR instr(lower(COALESCE({title},'')),:query)>0 OR instr(lower(s.id),:query)>0 OR instr(lower(s.directory),:query)>0 OR {message_search})"
        ));
    }
    query.push_str(" ORDER BY s.time_updated DESC,s.id ASC");
    query
}

/// 스크립트와 같은 결과. 세션 표에 필요한 열이 없으면 스키마 없음으로 알린다
pub(super) fn read_local_database(
    request: &AgentDatabaseRequest<'_>,
) -> AiResult<SqliteReadResult> {
    let connection = open_read_only_with_timeout(request.db_path, 5_000)?;
    let session_table = if request.provider == AiSessionProvider::Opencode2 {
        "session_v2"
    } else {
        "session"
    };
    let session_columns = table_columns(&connection, session_table)?;
    if !is_subset(
        &["id", "directory", "time_created", "time_updated"],
        &session_columns,
    ) {
        return Ok(SqliteReadResult {
            is_schema_available: false,
            sessions: Vec::new(),
            messages: Vec::new(),
        });
    }
    let shape = if request.provider == AiSessionProvider::Opencode2 {
        opencode2_shape(&connection)?
    } else {
        zcode_shape(&connection)?
    };
    let title = if session_columns.contains("title") {
        "s.title"
    } else {
        "NULL"
    };
    let parameters = script_parameters(request);
    let mut sessions = query_connection_rows(
        &connection,
        &session_query(&shape, title, request.session_id.is_some()),
        &parameters,
    )?;
    let mut messages = Vec::new();
    for session in &mut sessions {
        messages = annotate_session(&connection, &shape, session, request.session_id.is_some())?
            .unwrap_or(messages);
    }
    Ok(SqliteReadResult {
        is_schema_available: true,
        sessions,
        messages,
    })
}

/// 세션 행에 메시지 수와 첫 사용자 기록을 붙이고, 상세 조회면 대화 전체를 돌려준다
fn annotate_session(
    connection: &Connection,
    shape: &TranscriptShape,
    session: &mut Value,
    should_read_messages: bool,
) -> AiResult<Option<Vec<Value>>> {
    let mut bindings = SqlParameters::new();
    bindings.insert(
        "sessionId".to_string(),
        session.get("id").cloned().unwrap_or(Value::Null),
    );
    let message_count = first_cell(&query_connection_rows(
        connection,
        &shape.count_query,
        &bindings,
    )?);
    let message_query = format!(
        "SELECT {} AS role,{} AS data,m.time_created FROM {} WHERE m.session_id=:sessionId AND {}",
        shape.role, shape.content, shape.source, shape.visible
    );
    let first_query = format!(
        "{message_query} AND {}='user' ORDER BY {} LIMIT 1",
        shape.role, shape.order
    );
    let first = query_connection_rows(connection, &first_query, &bindings)?;
    let first_user_data = first
        .first()
        .and_then(|row| row.get("data"))
        .cloned()
        .unwrap_or(Value::Null);
    if let Value::Object(record) = session {
        record.insert("message_count".to_string(), message_count);
        record.insert("first_user_data".to_string(), first_user_data);
    }
    if !should_read_messages {
        return Ok(None);
    }
    let full_query = format!("{message_query} ORDER BY {}", shape.order);
    query_connection_rows(connection, &full_query, &bindings).map(Some)
}

fn first_cell(rows: &[Value]) -> Value {
    rows.first()
        .and_then(Value::as_object)
        .and_then(|record| record.values().next())
        .cloned()
        .unwrap_or(Value::Null)
}
