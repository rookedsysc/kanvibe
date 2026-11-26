//! OpenCode 기록. `~/.local/share/opencode/opencode.db`(SQLite)를 먼저 읽고, SQLite로 옮기기 전의 파일 저장소는
//! [`crate::legacy_opencode_sessions`]가 읽어 SQLite에 없는 세션만 덧붙인다.
//!
//! 로컬은 읽기 전용 rusqlite 연결로 읽고, 그 연결이 실패하면 TS처럼 Python `sqlite3`로 한 번 더 읽는다.
//! 원격은 같은 Python 스크립트를 SSH 명령으로 돌린다.

use std::collections::HashSet;

use serde_json::{Value, json};

use crate::error::{AiError, AiResult};
use crate::host::{CommandOptions, quote_shell_argument};
use crate::javascript::{is_truthy, parse_json};
use crate::legacy_opencode_sessions::{
    read_legacy_opencode_session_detail, read_legacy_opencode_sessions,
};
use crate::message_text::{
    extract_plain_text, make_preview_message, to_iso_string, truncate_text, truncate_text_to,
};
use crate::node_path;
use crate::providers::AiSessionProvider;
use crate::session_listing::{
    determine_match_scope, paginate_items, pick_live_session_files, sort_messages_descending,
};
use crate::sqlite_rows::{
    LocalPythonLimits, SqlParameters, encode_base64_json, parse_query_rows, query_native_rows,
    row_number, row_text, row_value, row_value_or, run_local_python,
};
use crate::types::{
    AgentCallNode, AggregatedAiMessage, AggregatedAiSession, AggregatedAiSessionDetail,
    AiMessageRole, AiSessionReaderContext, AiSessionReaderResult, LiveAiSessionWindows,
    LiveAiSubtask, LiveProviderSnapshot,
};

mod queries;

use queries::{
    CHILD_SESSIONS_SQL, DESCENDANT_SESSIONS_SQL, DETAIL_ROWS_SQL, KNOWN_SESSION_IDS_SQL,
    LIVE_SESSIONS_SQL, SESSION_ROW_SQL, SQLITE_QUERY_SCRIPT, session_list_sql,
};

const DEFAULT_DETAIL_LIMIT: usize = 20;
/// 원격 상세 조회는 메시지 본문을 돌려주므로 JSONL 기록 읽기와 같은 큰 세션 여유를 둔다
const OPEN_CODE_REMOTE_QUERY_MAX_BUFFER_BYTES: usize = 64 * 1024 * 1024;
const OPEN_CODE_LOCAL_PYTHON_MAX_BUFFER_BYTES: usize = 10 * 1024 * 1024;
const SESSION_TITLE_MAX_LENGTH: usize = 80;

/// 로컬 SQLite 조회. 파일이 없으면 `None`이다. 테스트는 연결 실패나 고정 행을 주입한다
pub(crate) type NativeQuery<'a> =
    &'a (dyn Fn(&str, &str, &SqlParameters) -> AiResult<Option<Vec<Value>>> + Sync);

/// 한 번의 조회가 쓰는 OpenCode DB 접근
#[derive(Clone, Copy)]
pub(crate) struct OpenCodeDatabase<'a> {
    pub context: &'a AiSessionReaderContext,
    pub native: NativeQuery<'a>,
}

impl OpenCodeDatabase<'_> {
    fn database_path(&self) -> AiResult<String> {
        let home = self.context.host.home_directory()?;
        Ok(node_path::join(&[
            &home,
            ".local",
            "share",
            "opencode",
            "opencode.db",
        ]))
    }

    /// TS `queryOpenCodeRows`. DB가 없으면 `None`이다
    pub(crate) fn query_rows(
        &self,
        sql: &str,
        parameters: &SqlParameters,
    ) -> AiResult<Option<Vec<Value>>> {
        let db_path = self.database_path()?;
        if self.context.host.is_remote() {
            return self.query_remote_rows(&db_path, sql, parameters);
        }
        let native_error = match (self.native)(&db_path, sql, parameters) {
            Ok(Some(rows)) => return Ok(Some(rows)),
            Ok(None) => None,
            Err(error) => Some(error),
        };
        if !self.context.host.path_exists(&db_path)? {
            return Ok(None);
        }
        let payload = sqlite_query_payload(&db_path, sql, parameters);
        let limits = LocalPythonLimits {
            max_output_bytes: OPEN_CODE_LOCAL_PYTHON_MAX_BUFFER_BYTES,
        };
        let python_rows =
            run_local_python(SQLITE_QUERY_SCRIPT, &payload, limits).and_then(|output| {
                parse_query_rows(&output, "Failed to parse OpenCode database query result")
            });
        match (python_rows, native_error) {
            (Ok(rows), _) => Ok(Some(rows)),
            (Err(python_error), Some(native_error)) => Err(AiError::message(format!(
                "{python_error}; native sqlite unavailable: {native_error}"
            ))),
            (Err(python_error), None) => Err(python_error),
        }
    }

    fn query_remote_rows(
        &self,
        db_path: &str,
        sql: &str,
        parameters: &SqlParameters,
    ) -> AiResult<Option<Vec<Value>>> {
        if !self.context.host.path_exists(db_path)? {
            return Ok(None);
        }
        let command = format!(
            "python3 -c {} {}",
            quote_shell_argument(SQLITE_QUERY_SCRIPT),
            quote_shell_argument(&sqlite_query_payload(db_path, sql, parameters))
        );
        let options = CommandOptions {
            max_buffer_bytes: Some(OPEN_CODE_REMOTE_QUERY_MAX_BUFFER_BYTES),
            timeout_ms: None,
        };
        let output = self.context.host.run(&command, options)?;
        parse_query_rows(&output, "Failed to parse OpenCode database query result").map(Some)
    }
}

fn sqlite_query_payload(db_path: &str, sql: &str, parameters: &SqlParameters) -> String {
    encode_base64_json(&json!({ "dbPath": db_path, "sql": sql, "parameters": parameters }))
}

fn native_database(context: &AiSessionReaderContext) -> OpenCodeDatabase<'_> {
    OpenCodeDatabase {
        context,
        native: &query_native_rows,
    }
}

/// worktree와 그 하위 경로만 고르는 SQL 인자
fn worktree_scope(context: &AiSessionReaderContext) -> SqlParameters {
    let worktree_path = context
        .worktree_path
        .as_deref()
        .filter(|path| !path.is_empty())
        .map(|path| node_path::resolve(&[path]))
        .unwrap_or_default();
    let worktree_prefix = if worktree_path == "/" {
        "/".to_string()
    } else {
        format!("{worktree_path}/")
    };
    let mut parameters = SqlParameters::new();
    parameters.insert("worktreePath".to_string(), Value::String(worktree_path));
    parameters.insert("worktreePrefix".to_string(), Value::String(worktree_prefix));
    parameters
}

/// SQLite 세션과, SQLite에 없는 이전 파일 세션을 합친다
pub fn read_opencode_sessions(context: &AiSessionReaderContext) -> AiResult<AiSessionReaderResult> {
    read_opencode_sessions_with(native_database(context))
}

pub(crate) fn read_opencode_sessions_with(
    database: OpenCodeDatabase<'_>,
) -> AiResult<AiSessionReaderResult> {
    let (sqlite, legacy) = std::thread::scope(|scope| {
        let legacy = scope.spawn(|| read_legacy_opencode_sessions(database.context));
        let sqlite = read_opencode_sqlite_sessions(database);
        (sqlite, legacy.join().expect("이전 OpenCode 조회"))
    });
    let legacy = legacy?;
    let known_session_ids = sqlite.known_session_ids.unwrap_or_else(|| {
        sqlite
            .result
            .sessions
            .iter()
            .map(|session| session.id.clone())
            .collect()
    });
    let mut sessions = sqlite.result.sessions;
    sessions.extend(
        legacy
            .sessions
            .into_iter()
            .filter(|session| !known_session_ids.contains(&session.id)),
    );
    let is_available = sqlite.result.available || legacy.available;
    let reason = match (sessions.is_empty(), is_available) {
        (false, _) => None,
        (true, true) => Some("No OpenCode sessions matched this task".to_string()),
        (true, false) => sqlite.result.reason,
    };
    Ok(AiSessionReaderResult {
        provider: AiSessionProvider::Opencode,
        available: is_available,
        session_count: sessions.len() as u64,
        reason,
        sessions,
    })
}

struct SqliteListing {
    result: AiSessionReaderResult,
    /// SQLite로 옮긴 세션은 이전 파일의 경로·본문이 검색에 맞아도 다시 노출하지 않는다
    known_session_ids: Option<HashSet<String>>,
}

fn unavailable_listing(reason: String) -> SqliteListing {
    SqliteListing {
        result: AiSessionReaderResult {
            provider: AiSessionProvider::Opencode,
            available: false,
            session_count: 0,
            reason: Some(reason),
            sessions: Vec::new(),
        },
        known_session_ids: None,
    }
}

/// TS `escapeSqliteLikePattern`
fn escape_sqlite_like_pattern(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn read_opencode_sqlite_sessions(database: OpenCodeDatabase<'_>) -> SqliteListing {
    let context = database.context;
    let normalized_query = context
        .query
        .as_deref()
        .filter(|query| !query.is_empty())
        .map(str::to_lowercase);
    let escaped_query = normalized_query.as_deref().map(escape_sqlite_like_pattern);
    let scope = worktree_scope(context);
    // 검색어로 거른 행에는 검색에 맞지 않는 SQLite 세션이 빠지므로, 이전 파일 세션을 가릴 ID는 worktree 범위에서 따로 조회한다
    let known_id_rows = escaped_query.as_ref().and_then(|_| {
        database
            .query_rows(KNOWN_SESSION_IDS_SQL, &scope)
            .ok()
            .flatten()
    });
    let mut parameters = scope.clone();
    if let Some(query) = &escaped_query {
        parameters.insert("query".to_string(), Value::String(query.clone()));
    }
    let rows = match database.query_rows(&session_list_sql(escaped_query.is_some()), &parameters) {
        Ok(Some(rows)) => rows,
        Ok(None) => return unavailable_listing("OpenCode database not found".to_string()),
        Err(error) => return unavailable_listing(error.to_string()),
    };
    let sessions: Vec<AggregatedAiSession> = rows
        .iter()
        .filter_map(|row| to_listed_session(row, context, normalized_query.as_deref()))
        .collect();
    let known_session_ids = known_id_rows
        .as_ref()
        .unwrap_or(&rows)
        .iter()
        .filter_map(|row| row_text(row, "id").map(str::to_string))
        .collect();
    SqliteListing {
        result: AiSessionReaderResult {
            provider: AiSessionProvider::Opencode,
            available: true,
            session_count: sessions.len() as u64,
            reason: sessions
                .is_empty()
                .then(|| "No OpenCode sessions matched this task".to_string()),
            sessions,
        },
        known_session_ids: Some(known_session_ids),
    }
}

fn row_title(row: &Value, first_user_prompt: &str) -> Option<String> {
    match row_text(row, "title") {
        Some(title) => Some(title.to_string()),
        None if row_value(row, "title").is_null() && !first_user_prompt.is_empty() => Some(
            truncate_text_to(first_user_prompt, SESSION_TITLE_MAX_LENGTH),
        ),
        None => None,
    }
}

fn to_listed_session(
    row: &Value,
    context: &AiSessionReaderContext,
    query: Option<&str>,
) -> Option<AggregatedAiSession> {
    let directory = row_text(row, "directory");
    let match_scope = determine_match_scope(directory, context)?;
    let first_user_prompt =
        extract_opencode_part_text(row_text(row, "first_user_part").unwrap_or_default());
    let title = row_title(row, &first_user_prompt);
    let id = row_text(row, "id").unwrap_or_default();
    if let Some(query) = query {
        let candidates = [
            title.as_deref(),
            Some(first_user_prompt.as_str()),
            directory,
            Some(id),
        ];
        let is_text_match = candidates
            .iter()
            .flatten()
            .any(|value| value.to_lowercase().contains(query));
        if !is_text_match && row_number(row, "matching_part_count") <= 0.0 {
            return None;
        }
    }
    Some(AggregatedAiSession {
        id: id.to_string(),
        provider: AiSessionProvider::Opencode,
        started_at: to_iso_string(row_value(row, "time_created")),
        updated_at: to_iso_string(row_value_or(
            row,
            "last_message_at",
            row_value(row, "time_updated"),
        )),
        matched_path: directory.map(str::to_string),
        match_scope,
        title,
        first_user_prompt: (!first_user_prompt.is_empty())
            .then(|| truncate_text(&first_user_prompt)),
        message_count: row_number(row, "part_count") as u64,
        source_ref: Some(id.to_string()),
    })
}

/// 이 worktree에서 지금 돌고 있는 OpenCode 세션과, 각 세션이 띄운 자식 세션을 찾는다.
/// OpenCode는 Task 도구가 만든 서브에이전트를 자식 세션으로 저장하고 부모를 `parent_id`로 가리킨다.
/// 그 컬럼은 버전에 따라 없을 수 있어, 조회가 실패하면 서브태스크 없이 세션만 돌려준다
pub fn read_opencode_live_sessions(
    context: &AiSessionReaderContext,
    windows: LiveAiSessionWindows,
    now_ms: f64,
) -> AiResult<Vec<LiveProviderSnapshot>> {
    read_opencode_live_sessions_with(native_database(context), windows, now_ms)
}

pub(crate) fn read_opencode_live_sessions_with(
    database: OpenCodeDatabase<'_>,
    windows: LiveAiSessionWindows,
    now_ms: f64,
) -> AiResult<Vec<LiveProviderSnapshot>> {
    let rows = database
        .query_rows(LIVE_SESSIONS_SQL, &worktree_scope(database.context))?
        .unwrap_or_default();
    let own_rows: Vec<Value> = rows
        .into_iter()
        .filter(|row| determine_match_scope(row_text(row, "directory"), database.context).is_some())
        .collect();
    let live_rows =
        pick_live_session_files(&own_rows, windows.running_window_ms as f64, now_ms, |row| {
            row_number(row, "time_updated")
        });
    Ok(live_rows
        .iter()
        .map(|row| {
            let session_id = row_text(row, "id").unwrap_or_default();
            LiveProviderSnapshot {
                session_id: Some(session_id.to_string()),
                current_task: row_text(row, "title").map(str::to_string),
                last_active_at: to_iso_string(row_value(row, "time_updated")),
                running_subtasks: read_running_children(database, session_id, windows, now_ms),
            }
        })
        .collect())
}

fn read_running_children(
    database: OpenCodeDatabase<'_>,
    parent_session_id: &str,
    windows: LiveAiSessionWindows,
    now_ms: f64,
) -> Vec<LiveAiSubtask> {
    let mut parameters = SqlParameters::new();
    parameters.insert("parentSessionId".to_string(), json!(parent_session_id));
    let Ok(rows) = database.query_rows(CHILD_SESSIONS_SQL, &parameters) else {
        return Vec::new();
    };
    let running_since = now_ms - windows.running_window_ms as f64;
    rows.unwrap_or_default()
        .iter()
        .filter(|row| row_number(row, "time_updated") >= running_since)
        .map(|row| LiveAiSubtask {
            id: row_text(row, "id").unwrap_or_default().to_string(),
            name: row_text(row, "title").map(str::to_string),
            last_active_at: to_iso_string(row_value(row, "time_updated")),
        })
        .collect()
}

/// SQLite 상세를 먼저 열고, 스키마가 없거나 세션이 없으면 이전 파일 저장소를 연다
pub fn read_opencode_session_detail(
    context: &AiSessionReaderContext,
    session_id: &str,
    source_ref: Option<&str>,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> AiResult<Option<AggregatedAiSessionDetail>> {
    read_opencode_session_detail_with(
        native_database(context),
        session_id,
        source_ref,
        cursor,
        limit,
    )
}

pub(crate) fn read_opencode_session_detail_with(
    database: OpenCodeDatabase<'_>,
    session_id: &str,
    source_ref: Option<&str>,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> AiResult<Option<AggregatedAiSessionDetail>> {
    let limit = limit.unwrap_or(DEFAULT_DETAIL_LIMIT);
    // SQLite 스키마가 없는 이전 설치도 파일 기반 대화를 열 수 있어야 하므로 SQLite 오류는 삼킨다
    if let Ok(Some(detail)) = read_sqlite_session_detail(database, session_id, cursor, limit) {
        return Ok(Some(detail));
    }
    read_legacy_opencode_session_detail(
        database.context,
        session_id,
        source_ref,
        cursor,
        Some(limit),
    )
}

fn session_id_parameter(session_id: &str) -> SqlParameters {
    let mut parameters = SqlParameters::new();
    parameters.insert("sessionId".to_string(), json!(session_id));
    parameters
}

fn read_sqlite_session_detail(
    database: OpenCodeDatabase<'_>,
    session_id: &str,
    cursor: Option<&str>,
    limit: usize,
) -> AiResult<Option<AggregatedAiSessionDetail>> {
    let context = database.context;
    let Some(rows) = database.query_rows(DETAIL_ROWS_SQL, &session_id_parameter(session_id))?
    else {
        return Ok(None);
    };
    let Some(first_row) = rows.first() else {
        return read_empty_session_detail(database, session_id);
    };
    if determine_match_scope(row_text(first_row, "directory"), context).is_none() {
        return Ok(None);
    }
    let messages: Vec<AggregatedAiMessage> = rows
        .iter()
        .filter_map(|row| to_detail_message(row, context))
        .collect();
    let sorted = sort_messages_descending(&messages);
    let page = paginate_items(&sorted, cursor, limit);
    let first_user_prompt = sorted
        .iter()
        .find(|message| message.role == AiMessageRole::User);
    let title = row_title(
        first_row,
        first_user_prompt.map_or("", |message| message.full_text.as_str()),
    );
    Ok(Some(AggregatedAiSessionDetail {
        session_id: session_id.to_string(),
        provider: AiSessionProvider::Opencode,
        title,
        matched_path: row_text(first_row, "directory").map(str::to_string),
        source_ref: row_text(first_row, "session_id").map(str::to_string),
        messages: page.items,
        next_cursor: page.next_cursor,
    }))
}

/// 메시지가 하나도 없는 세션도 작업 경로가 맞으면 빈 상세로 연다
fn read_empty_session_detail(
    database: OpenCodeDatabase<'_>,
    session_id: &str,
) -> AiResult<Option<AggregatedAiSessionDetail>> {
    let rows = database.query_rows(SESSION_ROW_SQL, &session_id_parameter(session_id))?;
    let Some(session) = rows.as_ref().and_then(|rows| rows.first()) else {
        return Ok(None);
    };
    if determine_match_scope(row_text(session, "directory"), database.context).is_none() {
        return Ok(None);
    }
    let mut detail = AggregatedAiSessionDetail::empty(AiSessionProvider::Opencode);
    detail.session_id = session_id.to_string();
    detail.title = row_text(session, "title").map(str::to_string);
    detail.matched_path = row_text(session, "directory").map(str::to_string);
    detail.source_ref = row_text(session, "id").map(str::to_string);
    Ok(Some(detail))
}

fn to_detail_message(row: &Value, context: &AiSessionReaderContext) -> Option<AggregatedAiMessage> {
    let part_data = row_text(row, "part_data").unwrap_or_default();
    let message_role = row_text(row, "message_data")
        .and_then(parse_json)
        .and_then(|message| {
            message
                .get("role")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let role = resolve_opencode_role(message_role.as_deref(), part_data);
    let text = extract_opencode_part_text(part_data);
    if context
        .roles
        .as_ref()
        .is_some_and(|roles| !roles.is_empty() && !roles.contains(&role))
    {
        return None;
    }
    if let Some(query) = context.query.as_deref().filter(|query| !query.is_empty())
        && !text.to_lowercase().contains(&query.to_lowercase())
    {
        return None;
    }
    make_preview_message(role, row_value(row, "time_created"), &text, Vec::new())
}

/// 파트 하나를 글로 옮긴다. 본문·생각은 글을, 도구 호출은 `도구: 결과`를 쓴다
pub(crate) fn extract_opencode_part_text(raw_data: &str) -> String {
    let Some(parsed) = parse_json(raw_data).filter(is_truthy) else {
        return String::new();
    };
    match parsed.get("type").and_then(Value::as_str) {
        Some("text" | "reasoning") => match parsed.get("text") {
            Some(text) if !text.is_null() => extract_plain_text(text),
            _ => extract_plain_text(&parsed),
        },
        Some("tool") => {
            let tool_name = parsed.get("tool").and_then(Value::as_str).unwrap_or("tool");
            let output = extract_plain_text(row_value(&parsed, "state"));
            if output.is_empty() {
                format!("{tool_name} executed")
            } else {
                format!("{tool_name}: {output}")
            }
        }
        _ => String::new(),
    }
}

fn resolve_opencode_role(role: Option<&str>, raw_part_data: &str) -> AiMessageRole {
    let part_type = (!raw_part_data.is_empty())
        .then(|| parse_json(raw_part_data))
        .flatten()
        .and_then(|part| part.get("type").and_then(Value::as_str).map(str::to_string));
    match (part_type.as_deref(), role) {
        (Some("tool"), _) => AiMessageRole::Tool,
        (Some("reasoning"), _) => AiMessageRole::Reasoning,
        (_, Some("user")) => AiMessageRole::User,
        (_, Some("assistant")) => AiMessageRole::Assistant,
        (_, Some("system")) => AiMessageRole::System,
        (_, Some("developer")) => AiMessageRole::Developer,
        (_, Some("tool")) => AiMessageRole::Tool,
        _ => AiMessageRole::Unknown,
    }
}

/// 세션 하나가 띄운 OpenCode 서브에이전트 호출 그래프. 자식 세션이 부모를 `parent_id`로 가리키므로 재귀 질의 한 번이면
/// 손자 이하까지 나온다. 질의가 실패하면 빈 목록이고, 끝났다는 기록이 없어 한동안 움직이지 않은 세션을 끝난 것으로 본다
pub fn read_opencode_agent_call_graph(
    context: &AiSessionReaderContext,
    session_id: &str,
    windows: LiveAiSessionWindows,
    now_ms: f64,
) -> AiResult<Vec<AgentCallNode>> {
    read_opencode_agent_call_graph_with(native_database(context), session_id, windows, now_ms)
}

pub(crate) fn read_opencode_agent_call_graph_with(
    database: OpenCodeDatabase<'_>,
    session_id: &str,
    windows: LiveAiSessionWindows,
    now_ms: f64,
) -> AiResult<Vec<AgentCallNode>> {
    let mut parameters = SqlParameters::new();
    parameters.insert("rootSessionId".to_string(), json!(session_id));
    let rows = database
        .query_rows(DESCENDANT_SESSIONS_SQL, &parameters)
        .ok()
        .flatten()
        .unwrap_or_default();
    let mut graph = OpenCodeGraph {
        rows: &rows,
        running_since: now_ms - windows.running_window_ms as f64,
        visited_session_ids: HashSet::new(),
    };
    Ok(graph.children_of(session_id))
}

struct OpenCodeGraph<'a> {
    rows: &'a [Value],
    running_since: f64,
    /// 세션이 서로를 부모로 가리켜도 재귀가 멈추도록 이미 그린 세션을 기억한다
    visited_session_ids: HashSet<String>,
}

impl OpenCodeGraph<'_> {
    fn children_of(&mut self, parent_session_id: &str) -> Vec<AgentCallNode> {
        let rows = self.rows;
        let child_rows: Vec<&Value> = rows
            .iter()
            .filter(|row| row_text(row, "parent_id") == Some(parent_session_id))
            .filter(|row| {
                !self
                    .visited_session_ids
                    .contains(row_text(row, "id").unwrap_or_default())
            })
            .collect();
        child_rows
            .into_iter()
            .map(|row| self.build_node(row))
            .collect()
    }

    fn build_node(&mut self, row: &Value) -> AgentCallNode {
        let id = row_text(row, "id").unwrap_or_default().to_string();
        self.visited_session_ids.insert(id.clone());
        let is_running = row_number(row, "time_updated") >= self.running_since;
        AgentCallNode {
            children: self.children_of(&id),
            id,
            agent_type: None,
            skill: None,
            task: row_text(row, "title").map(str::to_string),
            started_at: to_iso_string(row_value(row, "time_created")),
            ended_at: if is_running {
                None
            } else {
                to_iso_string(row_value(row, "time_updated"))
            },
        }
    }
}

#[cfg(test)]
mod tests;
