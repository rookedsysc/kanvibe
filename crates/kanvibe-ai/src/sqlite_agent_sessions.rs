//! OpenCode 2(`~/.local/share/opencode/opencode[-*].db`의 `session_v2`)와 ZCode(`~/.zcode/cli/db/db.sqlite`) 기록.
//!
//! 원격은 TS와 같은 Python 스크립트를 읽기 전용(`?mode=ro`)으로 돌린다. 로컬은 같은 스크립트의 질의를 그대로 옮긴
//! 읽기 전용 rusqlite 연결로 읽어 Python 프로세스를 띄우지 않는다.

use std::collections::HashSet;

use kanvibe_domain::locale_order::compare_like_locale;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

use crate::error::{AiError, AiResult};
use crate::host::{CommandOptions, quote_shell_argument};
use crate::javascript::{is_truthy, js_string, parse_json};
use crate::message_text::{
    extract_plain_text, make_preview_message, to_iso_string, truncate_text, truncate_text_to,
};
use crate::node_path;
use crate::providers::AiSessionProvider;
use crate::session_listing::{determine_match_scope, paginate_items, sort_sessions_descending};
use crate::sqlite_rows::{
    SqlParameters, encode_base64_json, query_connection_rows, row_text, row_value,
};
use crate::types::{
    AggregatedAiMessage, AggregatedAiSession, AggregatedAiSessionDetail, AiMessageRole,
    AiSessionReaderContext, AiSessionReaderResult,
};

mod local_query;

const DEFAULT_DETAIL_LIMIT: usize = 20;
const SQLITE_READ_MAX_BYTES: usize = 64 * 1024 * 1024;
const SESSION_TITLE_MAX_LENGTH: usize = 80;
const SQLITE_PROVIDERS: [AiSessionProvider; 2] =
    [AiSessionProvider::Opencode2, AiSessionProvider::Zcode];

/// Python SQLite는 Electron/Node ABI와 무관하고 SSH 호스트에서도 같은 읽기 전용 계약을 쓴다
const SQLITE_AGENT_READ_SCRIPT: &str = include_str!("sqlite_agent_sessions/read_script.py");

static OPENCODE_DATABASE_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^opencode(?:-[A-Za-z0-9_.-]+)?\.db$").expect("OpenCode DB 이름 패턴")
});

/// 스크립트가 돌려주는 값. 세션 행에는 `message_count`·`first_user_data`가 붙는다
pub(crate) struct SqliteReadResult {
    pub is_schema_available: bool,
    pub sessions: Vec<Value>,
    pub messages: Vec<Value>,
}

/// 스크립트에 넘기는 요청
pub(crate) struct AgentDatabaseRequest<'a> {
    pub db_path: &'a str,
    pub provider: AiSessionProvider,
    pub worktree: String,
    pub separator: &'static str,
    pub query: String,
    pub session_id: Option<&'a str>,
}

/// 두 provider의 목록. 한 DB가 깨져도 다른 DB의 세션은 보여 준다
pub fn read_sqlite_agent_sessions(
    context: &AiSessionReaderContext,
) -> AiResult<Vec<AiSessionReaderResult>> {
    Ok(SQLITE_PROVIDERS
        .iter()
        .map(|provider| read_provider_sessions(context, *provider))
        .collect())
}

fn read_provider_sessions(
    context: &AiSessionReaderContext,
    provider: AiSessionProvider,
) -> AiSessionReaderResult {
    match read_provider_sessions_checked(context, provider) {
        Ok(result) => result,
        Err(error) => AiSessionReaderResult {
            provider,
            available: false,
            session_count: 0,
            reason: Some(error.to_string()),
            sessions: Vec::new(),
        },
    }
}

fn read_provider_sessions_checked(
    context: &AiSessionReaderContext,
    provider: AiSessionProvider,
) -> AiResult<AiSessionReaderResult> {
    let databases = discover_databases(context, provider)?;
    let mut sessions: Vec<AggregatedAiSession> = Vec::new();
    let mut is_available = false;
    let mut failures: Vec<String> = Vec::new();
    for db_path in &databases {
        match read_database(context, provider, db_path, None) {
            Ok(result) => {
                is_available |= result.is_schema_available;
                for row in &result.sessions {
                    let is_known = sessions
                        .iter()
                        .any(|session| Some(session.id.as_str()) == row_text(row, "id"));
                    if let Some(session) =
                        to_listed_session(row, provider, db_path, context).filter(|_| !is_known)
                    {
                        sessions.push(session);
                    }
                }
            }
            Err(error) => failures.push(error.to_string()),
        }
    }
    sort_sessions_descending(&mut sessions);
    let reason = match (failures.is_empty(), is_available) {
        (false, _) => Some(failures.join("; ")),
        (true, true) => None,
        (true, false) => Some("Agent History database or schema not found".to_string()),
    };
    Ok(AiSessionReaderResult {
        provider,
        available: is_available,
        session_count: sessions.len() as u64,
        reason,
        sessions,
    })
}

fn derived_title(row: &Value, prompt: &str) -> Option<String> {
    match row_text(row, "title") {
        Some(title) => Some(title.to_string()),
        None => (!prompt.is_empty()).then(|| truncate_text_to(prompt, SESSION_TITLE_MAX_LENGTH)),
    }
}

fn to_listed_session(
    row: &Value,
    provider: AiSessionProvider,
    db_path: &str,
    context: &AiSessionReaderContext,
) -> Option<AggregatedAiSession> {
    let directory = row_text(row, "directory");
    let match_scope = determine_match_scope(directory, context)?;
    let prompt = decode_sqlite_content(provider, row_text(row, "first_user_data")).text;
    Some(AggregatedAiSession {
        id: row_text(row, "id").unwrap_or_default().to_string(),
        provider,
        started_at: to_iso_string(row_value(row, "time_created")),
        updated_at: to_iso_string(row_value(row, "time_updated")),
        matched_path: directory.map(str::to_string),
        match_scope,
        title: derived_title(row, &prompt),
        first_user_prompt: (!prompt.is_empty()).then(|| truncate_text(&prompt)),
        message_count: row_value(row, "message_count").as_f64().unwrap_or_default() as u64,
        source_ref: Some(db_path.to_string()),
    })
}

/// 대화 하나. `source_ref`는 찾은 DB 목록 안의 경로만 받는다
pub fn read_sqlite_agent_session_detail(
    context: &AiSessionReaderContext,
    provider: AiSessionProvider,
    session_id: &str,
    source_ref: Option<&str>,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> AiResult<Option<AggregatedAiSessionDetail>> {
    if !SQLITE_PROVIDERS.contains(&provider) {
        return Ok(None);
    }
    let databases = discover_databases(context, provider)?;
    let source_ref = source_ref.filter(|reference| !reference.is_empty());
    if source_ref.is_some_and(|reference| !databases.iter().any(|known| known == reference)) {
        return Ok(None);
    }
    let candidates = source_ref.map_or(databases.clone(), |reference| vec![reference.to_string()]);
    for db_path in &candidates {
        let result = read_database(context, provider, db_path, Some(session_id))?;
        let session = result.sessions.iter().find(|row| {
            row_text(row, "id") == Some(session_id)
                && determine_match_scope(row_text(row, "directory"), context).is_some()
        });
        let Some(session) = session else {
            continue;
        };
        let mut messages = detail_messages(&result.messages, provider, context);
        // ZCode의 sequence는 가져온 대화의 timestamp보다 우선하므로 기록 순서를 그대로 뒤집는다
        messages.reverse();
        let page = paginate_items(&messages, cursor, limit.unwrap_or(DEFAULT_DETAIL_LIMIT));
        let prompt = decode_sqlite_content(provider, row_text(session, "first_user_data")).text;
        return Ok(Some(AggregatedAiSessionDetail {
            session_id: session_id.to_string(),
            provider,
            title: derived_title(session, &prompt),
            matched_path: row_text(session, "directory").map(str::to_string),
            source_ref: Some(db_path.clone()),
            messages: page.items,
            next_cursor: page.next_cursor,
        }));
    }
    Ok(None)
}

fn detail_messages(
    rows: &[Value],
    provider: AiSessionProvider,
    context: &AiSessionReaderContext,
) -> Vec<AggregatedAiMessage> {
    let query = context
        .query
        .as_deref()
        .filter(|query| !query.is_empty())
        .map(str::to_lowercase);
    rows.iter()
        .filter_map(|row| {
            let decoded = decode_sqlite_content(provider, row_text(row, "data"));
            let role = decoded
                .role
                .unwrap_or_else(|| resolve_role(row_text(row, "role")));
            if context
                .roles
                .as_ref()
                .is_some_and(|roles| !roles.is_empty() && !roles.contains(&role))
            {
                return None;
            }
            if query
                .as_deref()
                .is_some_and(|query| !decoded.text.to_lowercase().contains(query))
            {
                return None;
            }
            make_preview_message(
                role,
                row_value(row, "time_created"),
                &decoded.text,
                Vec::new(),
            )
        })
        .collect()
}

fn discover_databases(
    context: &AiSessionReaderContext,
    provider: AiSessionProvider,
) -> AiResult<Vec<String>> {
    let home = context.host.home_directory()?;
    if provider == AiSessionProvider::Zcode {
        let db_path = node_path::join(&[&home, ".zcode", "cli", "db", "db.sqlite"]);
        return Ok(if context.host.path_exists(&db_path)? {
            vec![db_path]
        } else {
            Vec::new()
        });
    }
    let directory = node_path::join(&[&home, ".local", "share", "opencode"]);
    if !context.host.path_exists(&directory)? {
        return Ok(Vec::new());
    }
    let mut files: Vec<String> = context
        .host
        .read_directory_files_by_suffix(&directory, ".db")?
        .into_iter()
        .filter(|file| OPENCODE_DATABASE_NAME.is_match(&node_path::basename(file)))
        .collect();
    files.sort_by(|left, right| {
        let is_default = |file: &String| node_path::basename(file) == "opencode.db";
        is_default(right)
            .cmp(&is_default(left))
            .then_with(|| compare_like_locale(left, right))
    });
    Ok(files)
}

fn read_database(
    context: &AiSessionReaderContext,
    provider: AiSessionProvider,
    db_path: &str,
    session_id: Option<&str>,
) -> AiResult<SqliteReadResult> {
    let Some(worktree_path) = context
        .worktree_path
        .as_deref()
        .filter(|path| !path.is_empty())
    else {
        return Ok(SqliteReadResult {
            is_schema_available: true,
            sessions: Vec::new(),
            messages: Vec::new(),
        });
    };
    let request = AgentDatabaseRequest {
        db_path,
        provider,
        worktree: node_path::resolve(&[worktree_path]),
        separator: "/",
        query: if session_id.is_some() {
            String::new()
        } else {
            context.query.clone().unwrap_or_default()
        },
        session_id,
    };
    if context.host.is_remote() {
        return read_remote_database(context, &request);
    }
    local_query::read_local_database(&request)
}

fn read_remote_database(
    context: &AiSessionReaderContext,
    request: &AgentDatabaseRequest<'_>,
) -> AiResult<SqliteReadResult> {
    let mut payload = Map::new();
    payload.insert("dbPath".to_string(), json!(request.db_path));
    payload.insert("provider".to_string(), json!(request.provider.id()));
    payload.insert("worktree".to_string(), json!(request.worktree));
    payload.insert("separator".to_string(), json!(request.separator));
    payload.insert("query".to_string(), json!(request.query));
    if let Some(session_id) = request.session_id {
        payload.insert("sessionId".to_string(), json!(session_id));
    }
    let command = format!(
        "python3 -c {} {}",
        quote_shell_argument(SQLITE_AGENT_READ_SCRIPT),
        quote_shell_argument(&encode_base64_json(&Value::Object(payload)))
    );
    let options = CommandOptions {
        max_buffer_bytes: Some(SQLITE_READ_MAX_BYTES),
        timeout_ms: None,
    };
    parse_read_result(&context.host.run(&command, options)?)
}

/// TS의 응답 검사. 객체이고 `sessions`·`messages`가 배열이어야 한다
fn parse_read_result(output: &str) -> AiResult<SqliteReadResult> {
    let invalid = || AiError::message("Invalid Agent History database response");
    let value = parse_json(crate::javascript::js_trim(output))
        .filter(is_truthy)
        .ok_or_else(invalid)?;
    let (Some(Value::Array(sessions)), Some(Value::Array(messages))) =
        (value.get("sessions"), value.get("messages"))
    else {
        return Err(invalid());
    };
    Ok(SqliteReadResult {
        is_schema_available: value.get("schemaAvailable").is_some_and(is_truthy),
        sessions: sessions.clone(),
        messages: messages.clone(),
    })
}

/// 기록 한 줄의 글과, 기록이 직접 알려 주는 역할(생각·도구)
struct DecodedContent {
    text: String,
    role: Option<AiMessageRole>,
}

fn plain(text: String) -> DecodedContent {
    DecodedContent { text, role: None }
}

fn decode_sqlite_content(provider: AiSessionProvider, raw: Option<&str>) -> DecodedContent {
    let Some(record) = raw
        .filter(|raw| !raw.is_empty())
        .and_then(parse_json)
        .filter(is_truthy)
    else {
        return plain(String::new());
    };
    if provider == AiSessionProvider::Opencode2 {
        return plain(decode_opencode2_content(&record));
    }
    match record.get("type").and_then(Value::as_str) {
        Some("reasoning") => DecodedContent {
            text: extract_plain_text(row_value(&record, "text")),
            role: Some(AiMessageRole::Reasoning),
        },
        Some("tool") => DecodedContent {
            text: decode_zcode_tool(&record),
            role: Some(AiMessageRole::Tool),
        },
        Some("text") => plain(extract_plain_text(row_value(&record, "text"))),
        _ => plain(String::new()),
    }
}

fn decode_opencode2_content(record: &Value) -> String {
    let content = match record.get("content") {
        None | Some(Value::Null) => row_value(record, "text"),
        Some(content) => content,
    };
    let Value::Array(parts) = content else {
        return extract_plain_text(content);
    };
    parts
        .iter()
        .map(|part| match part.get("type").and_then(Value::as_str) {
            Some("tool") if part.is_object() => {
                let name = part.get("name").and_then(Value::as_str).unwrap_or("tool");
                format!("{name}: {}", extract_plain_text(row_value(part, "state")))
            }
            _ => extract_plain_text(part),
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_zcode_tool(record: &Value) -> String {
    let empty = Value::Object(Map::new());
    let state = match record.get("state") {
        Some(state @ (Value::Object(_) | Value::Array(_))) => state,
        _ => &empty,
    };
    let detail = [state.get("output"), state.get("input")]
        .into_iter()
        .flatten()
        .find(|value| !value.is_null())
        .unwrap_or(state);
    let tool = record.get("tool").and_then(Value::as_str).unwrap_or("tool");
    format!("{tool}: {}", extract_plain_text(detail))
}

fn resolve_role(role: Option<&str>) -> AiMessageRole {
    match role {
        Some("user") => AiMessageRole::User,
        Some("assistant") => AiMessageRole::Assistant,
        Some("system") => AiMessageRole::System,
        Some("developer") => AiMessageRole::Developer,
        Some("reasoning") => AiMessageRole::Reasoning,
        Some("tool") => AiMessageRole::Tool,
        _ => AiMessageRole::Unknown,
    }
}

/// 열 이름 집합. 없는 테이블은 빈 집합이다
pub(crate) fn table_columns(
    connection: &rusqlite::Connection,
    table: &str,
) -> AiResult<HashSet<String>> {
    let rows = query_connection_rows(
        connection,
        &format!("PRAGMA table_info({table})"),
        &SqlParameters::new(),
    )?;
    Ok(rows
        .iter()
        .filter_map(|row| row.get("name").map(js_string))
        .collect())
}

#[cfg(test)]
mod tests;
