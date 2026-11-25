//! 기록 파일 옆에 따로 저장된 메시지·메타데이터(곁 파일)를 provider별로 읽는다.

use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde_json::{Map, Value};

use super::{DevinIndex, ReadScope};
use crate::additional_file_session_formats::{
    RecordValue, decode_additional_file_session, field, session_record, session_string,
};
use crate::devin_session_index::read_devin_session_index;
use crate::error::AiResult;
use crate::javascript::{js_string, parse_date_ms};
use crate::node_path;
use crate::providers::AiSessionProvider;
use crate::types::AiMessageRole;

const ANTIGRAVITY_HISTORY_MATCH_WINDOW_MS: f64 = 2_000.0;

static AGENT_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^[A-Za-z0-9_-]+$").expect("에이전트 ID 패턴"));

fn lines_record(lines: &Value) -> RecordValue {
    let mut record = Map::new();
    record.insert("lines".to_string(), lines.clone());
    record
}

/// provider마다 정해진 곁 파일. 없으면 빈 객체다
pub(super) fn read_sidecar(
    scope: &ReadScope<'_>,
    provider: AiSessionProvider,
    file: &str,
    raw: &Value,
) -> AiResult<RecordValue> {
    let directory = node_path::dirname(file);
    match provider {
        AiSessionProvider::Grok => {
            let lines =
                scope.optional_file(&node_path::join(&[&directory, "chat_history.jsonl"]))?;
            Ok(lines_record(&lines.0))
        }
        AiSessionProvider::Rovo => {
            let context =
                scope.optional_file(&node_path::join(&[&directory, "session_context.json"]))?;
            Ok(session_record(&context.0).clone())
        }
        AiSessionProvider::Cline => {
            let messages_file = match file.strip_suffix(".json") {
                Some(stem) => format!("{stem}.messages.json"),
                None => file.to_string(),
            };
            Ok(session_record(&scope.optional_file(&messages_file)?.0).clone())
        }
        AiSessionProvider::Kiro => Ok(lines_record(&scope.optional_file(&format!("{file}l"))?.0)),
        AiSessionProvider::Cursor => read_cursor_metadata(scope, file),
        AiSessionProvider::Antigravity => read_antigravity_workspace(scope, file, raw),
        AiSessionProvider::Devin => read_devin_sidecar(scope, file, &directory, raw),
        AiSessionProvider::Kimi => read_kimi_sidecar(scope, &directory, raw),
        _ => Ok(Map::new()),
    }
}

fn devin_index(scope: &ReadScope<'_>, db_path: &str) -> AiResult<DevinIndex> {
    if let Some(known) = scope.devin_indexes.lock().expect("Devin 색인").get(db_path) {
        return known.clone();
    }
    let index = read_devin_session_index(&scope.context.host, db_path).map(Arc::new);
    scope
        .devin_indexes
        .lock()
        .expect("Devin 색인")
        .insert(db_path.to_string(), index.clone());
    index
}

fn read_devin_sidecar(
    scope: &ReadScope<'_>,
    file: &str,
    directory: &str,
    raw: &Value,
) -> AiResult<RecordValue> {
    let db_path = node_path::join(&[&node_path::dirname(directory), "sessions.db"]);
    let index = devin_index(scope, &db_path)?;
    let document = session_record(raw);
    let id = session_string(field(document, "session_id"))
        .or_else(|| session_string(field(document, "sessionId")))
        .unwrap_or_else(|| node_path::basename_without(file, ".json"));
    Ok(index
        .get(&id)
        .map(|row| session_record(row).clone())
        .unwrap_or_default())
}

/// state.json의 에이전트 중 부모가 없는 main. 없으면 `main`이다
fn primary_agent(raw: &Value) -> String {
    let agents = session_record(field(session_record(raw), "agents"));
    agents
        .iter()
        .find(|(_, value)| {
            let agent = session_record(value);
            field(agent, "type").as_str() == Some("main") && field(agent, "parentAgentId").is_null()
        })
        .map_or_else(|| "main".to_string(), |(key, _)| key.clone())
}

fn read_kimi_sidecar(scope: &ReadScope<'_>, directory: &str, raw: &Value) -> AiResult<RecordValue> {
    let sessions_root = node_path::dirname(&node_path::dirname(directory));
    let index_file = node_path::join(&[&node_path::dirname(&sessions_root), "session_index.jsonl"]);
    let index = scope.optional_file(&index_file)?;
    let session_id = node_path::basename(directory);
    let entry = index.0.as_array().and_then(|records| {
        records
            .iter()
            .map(session_record)
            .rfind(|record| field(record, "sessionId").as_str() == Some(session_id.as_str()))
    });
    let primary = primary_agent(raw);
    // state.json의 에이전트 ID가 경로 밖을 가리키지 못하게 한다
    if !AGENT_ID_PATTERN.is_match(&primary) {
        return Ok(Map::new());
    }
    let mut sidecar = Map::new();
    if let Some(work_directory) = entry.and_then(|record| record.get("workDir")) {
        sidecar.insert("workDir".to_string(), work_directory.clone());
    }
    let wire_file = node_path::join(&[directory, "agents", &primary, "wire.jsonl"]);
    sidecar.insert(
        "lines".to_string(),
        scope.optional_file(&wire_file)?.0.clone(),
    );
    Ok(sidecar)
}

/// Cursor 대화의 `~/.cursor/chats/**/<id>/meta.json`. 하나로 정해지지 않으면 빈 객체다
pub(super) fn read_cursor_metadata(
    scope: &ReadScope<'_>,
    transcript: &str,
) -> AiResult<RecordValue> {
    let parent = node_path::dirname(transcript);
    let id = if node_path::basename(&parent) == "agent-transcripts" {
        node_path::basename_without(transcript, ".jsonl")
    } else {
        node_path::basename(&parent)
    };
    let files = cursor_metadata_files(scope)?;
    let matches: Vec<&String> = files
        .iter()
        .filter(|file| {
            node_path::basename(file) == "meta.json"
                && node_path::basename(&node_path::dirname(file)) == id
        })
        .collect();
    let [only_match] = matches.as_slice() else {
        return Ok(Map::new());
    };
    Ok(session_record(&scope.read_parsed_file(only_match)?.0).clone())
}

fn cursor_metadata_files(scope: &ReadScope<'_>) -> AiResult<Arc<Vec<String>>> {
    let mut known = scope
        .cursor_metadata_files
        .lock()
        .expect("Cursor 메타데이터 목록");
    if let Some(files) = known.as_ref() {
        return files.clone();
    }
    let chats = node_path::join(&[&scope.home, ".cursor", "chats"]);
    let files = scope
        .context
        .host
        .list_files_recursively_by_suffix(&chats, ".json")
        .map(Arc::new);
    *known = Some(files.clone());
    files
}

/// 넣은 순서를 지키며 겹치지 않게 모은다(JS `Set`)
fn add_unique(paths: &mut Vec<String>, value: String) {
    if !paths.contains(&value) {
        paths.push(value);
    }
}

fn read_antigravity_workspace(
    scope: &ReadScope<'_>,
    file: &str,
    raw: &Value,
) -> AiResult<RecordValue> {
    let root = node_path::join(&[&scope.home, ".gemini", "antigravity-cli"]);
    let id = node_path::basename(&node_path::dirname(&node_path::dirname(
        &node_path::dirname(file),
    )));
    let history = scope.optional_file(&node_path::join(&[&root, "history.jsonl"]))?;
    let records: Vec<&RecordValue> = history
        .0
        .as_array()
        .map_or_else(Vec::new, |items| items.iter().map(session_record).collect());
    let mut paths: Vec<String> = Vec::new();
    for record in records
        .iter()
        .filter(|record| field(record, "conversationId").as_str() == Some(id.as_str()))
    {
        if let Some(workspace) = session_string(field(record, "workspace")) {
            add_unique(&mut paths, workspace);
        }
    }
    let cache_file = |name: &str| scope.optional_file(&node_path::join(&[&root, "cache", name]));
    let metadata = cache_file("conversation_metadata.json")?;
    let projects = cache_file("projects.json")?;
    let last_conversations = cache_file("last_conversations.json")?;
    for (workspace, conversation_id) in session_record(&last_conversations.0) {
        if conversation_id.as_str() == Some(id.as_str()) {
            add_unique(&mut paths, workspace.clone());
        }
    }
    add_project_paths(&mut paths, &id, &metadata.0, &projects.0);
    if paths.is_empty() {
        add_history_match(&mut paths, file, raw, &records);
    }
    // 같은 ID가 서로 다른 작업에 이어져 있으면 아무 작업 경로나 고르지 않는다
    let mut sidecar = Map::new();
    if let [workspace] = paths.as_slice() {
        sidecar.insert("workspace".to_string(), Value::String(workspace.clone()));
    }
    Ok(sidecar)
}

fn add_project_paths(paths: &mut Vec<String>, id: &str, metadata: &Value, projects: &Value) {
    let conversations = session_record(field(session_record(metadata), "conversations"));
    let summary = field(session_record(field(conversations, id)), "summary");
    let Some(target_project) = session_string(field(session_record(summary), "ProjectID")) else {
        return;
    };
    for (key, value) in session_record(projects) {
        if key == &target_project
            && let Some(project_path) = value.as_str().filter(|path| node_path::is_absolute(path))
        {
            add_unique(paths, project_path.to_string());
        }
        if value.as_str() == Some(target_project.as_str()) && node_path::is_absolute(key) {
            add_unique(paths, key.clone());
        }
    }
}

/// 메타데이터가 없으면 첫 사용자 요청과 같은 글·같은 시각(2초 안)의 history 기록에서 작업 경로를 찾는다
fn add_history_match(paths: &mut Vec<String>, file: &str, raw: &Value, records: &[&RecordValue]) {
    let transcript =
        decode_additional_file_session(AiSessionProvider::Antigravity, file, raw, &Map::new());
    let Some(first) = transcript
        .messages
        .iter()
        .find(|message| message.role == AiMessageRole::User)
    else {
        return;
    };
    let Some(first_timestamp) = first
        .timestamp
        .as_deref()
        .filter(|timestamp| !timestamp.is_empty())
    else {
        return;
    };
    let first_time = parse_date_ms(first_timestamp).unwrap_or(f64::NAN);
    for record in records {
        let recorded = field(record, "timestamp");
        let timestamp = match recorded.as_f64() {
            Some(number) if recorded.is_number() && number < 1e12 => number * 1000.0,
            _ => {
                parse_date_ms(&js_string_or_undefined(record.get("timestamp"))).unwrap_or(f64::NAN)
            }
        };
        let is_same_display = field(record, "display").as_str() == Some(first.full_text.as_str());
        if is_same_display
            && (timestamp - first_time).abs() <= ANTIGRAVITY_HISTORY_MATCH_WINDOW_MS
            && let Some(workspace) = session_string(field(record, "workspace"))
        {
            add_unique(paths, workspace);
        }
    }
}

/// JS `String(value)`. 없는 값은 `"undefined"`다
fn js_string_or_undefined(value: Option<&Value>) -> String {
    value.map_or_else(|| "undefined".to_string(), js_string)
}
