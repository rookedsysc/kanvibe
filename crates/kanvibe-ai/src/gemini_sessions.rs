//! Gemini CLI 대화 기록(`~/.gemini/tmp/<projectHash>/chats/*.json|*.jsonl`) 리더. TS `readGeminiSessions.ts`를 옮긴다.
//!
//! 기록 경로는 프로젝트 해시라서 `projects.json`이나 프로젝트 디렉터리의 `.project_root`로 실제 작업 경로를 되찾아 worktree를 맞춘다.

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};

use crate::error::{AiError, AiResult};
use crate::javascript::{
    is_present_truthy, is_truthy, iso_string_from_ms, js_string, js_trim, parse_json, stringify,
};
use crate::jsonl::read_json_lines;
use crate::message_text::{
    extract_plain_text, make_preview_message, to_iso_string, truncate_text, truncate_text_to,
};
use crate::node_path;
use crate::parse_cache::{ParseCacheKind, cached_or_parse};
use crate::providers::AiSessionProvider;
use crate::record_fields::{first_present, first_present_or_null, matches_query};
use crate::session_listing::{
    create_reader_result, create_unavailable_result, determine_match_scope, map_with_concurrency,
    paginate_items, parse_concurrency, pick_live_session_files, sort_messages_descending,
};
use crate::types::{
    AggregatedAiMessage, AggregatedAiSession, AggregatedAiSessionDetail, AiMessageRole,
    AiSessionMatchScope, AiSessionReaderContext, AiSessionReaderResult, LiveAiSessionWindows,
    LiveProviderSnapshot,
};

pub const DEFAULT_DETAIL_LIMIT: usize = 20;
const SESSION_TITLE_LENGTH: usize = 80;

/// 프로젝트 해시에서 실제 작업 경로로. `.project_root`에서 찾은 경로도 읽는 동안 채워 넣는다
type ProjectPathMap = Mutex<HashMap<String, String>>;

/// worktree와 맞춰 본 대화 파일 하나
struct GeminiParsedChat {
    value: Arc<Option<Value>>,
    matched_path: String,
}

impl GeminiParsedChat {
    fn chat(&self) -> &Value {
        self.value.as_ref().as_ref().unwrap_or(&Value::Null)
    }
}

pub fn read_gemini_sessions(context: &AiSessionReaderContext) -> AiResult<AiSessionReaderResult> {
    let gemini_root = gemini_root_directory(context)?;
    if !context.host.path_exists(&gemini_root)? {
        return Ok(create_unavailable_result(
            AiSessionProvider::Gemini,
            "Gemini CLI directory not found",
        ));
    }
    let tmp_directory = node_path::join(&[&gemini_root, "tmp"]);
    if !context.host.path_exists(&tmp_directory)? {
        let mut result = create_reader_result(AiSessionProvider::Gemini, Vec::new());
        result.reason = Some("No local Gemini session files found".to_string());
        return Ok(result);
    }
    let project_paths = Mutex::new(read_gemini_project_path_map(&gemini_root, context)?);
    let chat_files = find_gemini_chat_files(&tmp_directory, context)?;
    let summaries =
        map_with_concurrency(&chat_files, parse_concurrency(context), |file_path, _| {
            parse_gemini_session_summary(file_path, context, &project_paths)
        });
    let sessions: Vec<AggregatedAiSession> = summaries
        .into_iter()
        .collect::<AiResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    let mut result = create_reader_result(AiSessionProvider::Gemini, sessions);
    if result.sessions.is_empty() {
        result.reason = Some("No Gemini sessions matched this task".to_string());
    }
    Ok(result)
}

/// TS `readGeminiSessionDetail(context, sessionId, sourceRef, cursor, limit = 20)`
pub fn read_gemini_session_detail(
    context: &AiSessionReaderContext,
    session_id: &str,
    source_ref: Option<&str>,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> AiResult<Option<AggregatedAiSessionDetail>> {
    let gemini_root = gemini_root_directory(context)?;
    let project_paths = Mutex::new(read_gemini_project_path_map(&gemini_root, context)?);
    let chat_files = match source_ref.filter(|source| !source.is_empty()) {
        Some(source) => vec![source.to_string()],
        None => find_gemini_chat_files(&node_path::join(&[&gemini_root, "tmp"]), context)?,
    };
    for file_path in &chat_files {
        let Some(parsed) = parse_gemini_chat_file(file_path, context, &project_paths)? else {
            continue;
        };
        if resolve_gemini_session_id(parsed.chat(), file_path) != session_id {
            continue;
        }
        let messages: Vec<AggregatedAiMessage> = flatten_gemini_messages(parsed.chat())
            .into_iter()
            .filter(|message| is_selected_message(context, message))
            .collect();
        let page = paginate_items(
            &sort_messages_descending(&messages),
            cursor,
            limit.unwrap_or(DEFAULT_DETAIL_LIMIT),
        );
        let first_user_prompt = first_user_prompt(&messages);
        return Ok(Some(AggregatedAiSessionDetail {
            session_id: session_id.to_string(),
            provider: AiSessionProvider::Gemini,
            title: chat_title(parsed.chat(), first_user_prompt.as_deref()),
            matched_path: Some(parsed.matched_path.clone()),
            source_ref: Some(file_path.clone()),
            messages: page.items,
            next_cursor: page.next_cursor,
        }));
    }
    Ok(None)
}

fn is_selected_message(context: &AiSessionReaderContext, message: &AggregatedAiMessage) -> bool {
    if let Some(roles) = context.roles.as_ref().filter(|roles| !roles.is_empty())
        && !roles.contains(&message.role)
    {
        return false;
    }
    match context.query.as_deref().filter(|query| !query.is_empty()) {
        Some(query) => message
            .full_text
            .to_lowercase()
            .contains(&query.to_lowercase()),
        None => true,
    }
}

fn first_user_prompt(messages: &[AggregatedAiMessage]) -> Option<String> {
    messages
        .iter()
        .find(|message| message.role == AiMessageRole::User)
        .map(|message| message.full_text.clone())
}

/// `value.title ?? (firstUserPrompt ? truncateText(firstUserPrompt, 80) : null)`
fn chat_title(chat: &Value, first_user_prompt: Option<&str>) -> Option<String> {
    match first_present(chat, &["title"]) {
        Some(Value::String(title)) => Some(title.clone()),
        Some(other) => Some(js_string(other)),
        None => first_user_prompt
            .filter(|prompt| !prompt.is_empty())
            .map(|prompt| truncate_text_to(prompt, SESSION_TITLE_LENGTH)),
    }
}

fn parse_gemini_session_summary(
    file_path: &str,
    context: &AiSessionReaderContext,
    project_paths: &ProjectPathMap,
) -> AiResult<Option<AggregatedAiSession>> {
    let Some(parsed) = parse_gemini_chat_file(file_path, context, project_paths)? else {
        return Ok(None);
    };
    let chat = parsed.chat();
    let messages = flatten_gemini_messages(chat);
    let first_user_prompt = first_user_prompt(&messages);
    let session_id = resolve_gemini_session_id(chat, file_path);
    let title = chat_title(chat, first_user_prompt.as_deref());
    if !matches_summary_query(
        context,
        &title,
        &first_user_prompt,
        &parsed.matched_path,
        &session_id,
        &messages,
    ) {
        return Ok(None);
    }
    Ok(Some(AggregatedAiSession {
        id: session_id,
        provider: AiSessionProvider::Gemini,
        started_at: to_iso_string(first_present_or_null(
            chat,
            &["startTime", "createdAt", "timestamp"],
        )),
        updated_at: latest_conversation_at(&messages).or_else(|| {
            to_iso_string(first_present_or_null(
                chat,
                &["lastUpdated", "updatedAt", "timestamp"],
            ))
        }),
        matched_path: Some(parsed.matched_path.clone()),
        match_scope: AiSessionMatchScope::Worktree,
        title,
        first_user_prompt: first_user_prompt
            .as_deref()
            .filter(|prompt| !prompt.is_empty())
            .map(truncate_text),
        message_count: messages.len() as u64,
        source_ref: Some(file_path.to_string()),
    }))
}

/// 사용자·AI 메시지 중 가장 최근 시각. 추론·도구 기록은 대화 활동으로 치지 않는다
fn latest_conversation_at(messages: &[AggregatedAiMessage]) -> Option<String> {
    let conversation: Vec<AggregatedAiMessage> = messages
        .iter()
        .filter(|message| matches!(message.role, AiMessageRole::User | AiMessageRole::Assistant))
        .cloned()
        .collect();
    sort_messages_descending(&conversation)
        .into_iter()
        .next()
        .and_then(|message| message.timestamp)
}

fn matches_summary_query(
    context: &AiSessionReaderContext,
    title: &Option<String>,
    first_user_prompt: &Option<String>,
    matched_path: &str,
    session_id: &str,
    messages: &[AggregatedAiMessage],
) -> bool {
    let mut values = vec![
        title.as_deref(),
        first_user_prompt.as_deref(),
        Some(matched_path),
        Some(session_id),
    ];
    values.extend(
        messages
            .iter()
            .map(|message| Some(message.full_text.as_str())),
    );
    matches_query(context.query.as_deref(), &values)
}

fn find_gemini_chat_files(
    tmp_directory: &str,
    context: &AiSessionReaderContext,
) -> AiResult<Vec<String>> {
    let mut files = context
        .host
        .list_files_recursively_by_suffix(tmp_directory, ".json")?;
    files.extend(
        context
            .host
            .list_files_recursively_by_suffix(tmp_directory, ".jsonl")?,
    );
    Ok(files
        .into_iter()
        .filter(|file_path| node_path::basename(&node_path::dirname(file_path)) == "chats")
        .collect())
}

fn parse_gemini_chat_file(
    file_path: &str,
    context: &AiSessionReaderContext,
    project_paths: &ProjectPathMap,
) -> AiResult<Option<GeminiParsedChat>> {
    let value = cached_or_parse(ParseCacheKind::Full, &context.host, file_path, || {
        read_gemini_chat_value(file_path, context)
    })?;
    let Some(chat) = value
        .as_ref()
        .as_ref()
        .filter(|chat| chat.is_object() || chat.is_array())
    else {
        return Ok(None);
    };
    let project_id = gemini_project_id_from_chat_path(file_path);
    let matched_path = resolve_gemini_matched_path(
        chat,
        file_path,
        project_id.as_deref(),
        project_paths,
        context,
    )?;
    let Some(matched_path) =
        matched_path.filter(|path| determine_match_scope(Some(path), context).is_some())
    else {
        return Ok(None);
    };
    Ok(Some(GeminiParsedChat {
        value,
        matched_path,
    }))
}

/// `.json`은 대화 객체 하나, `.jsonl`은 머리 줄·`$set` 갱신 줄·메시지 줄을 이어 붙여 같은 모양의 객체로 만든다
fn read_gemini_chat_value(
    file_path: &str,
    context: &AiSessionReaderContext,
) -> AiResult<Option<Value>> {
    if !file_path.ends_with(".jsonl") {
        return Ok(parse_json(&context.host.read_text_file(file_path)?));
    }
    let records = read_json_lines(&context.host, file_path)?;
    let mut chat = Map::new();
    chat.insert("messages".to_string(), Value::Array(Vec::new()));
    for record in records.iter().filter(|record| record.is_object()) {
        match record.get("$set") {
            Some(update) if is_truthy(update) && (update.is_object() || update.is_array()) => {
                assign(&mut chat, update)
            }
            _ if record.get("sessionId").is_some_and(Value::is_string) => assign(&mut chat, record),
            _ => {}
        }
        let is_message = record
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| {
                resolve_gemini_role(Some(&Value::String(kind.to_string())))
                    != AiMessageRole::Unknown
            });
        if is_message {
            push_chat_message(&mut chat, record.clone())?;
        }
    }
    Ok(Some(Value::Object(chat)))
}

/// JS `Object.assign(chat, source)`. 배열은 번호를 키로 옮긴다
fn assign(chat: &mut Map<String, Value>, source: &Value) {
    match source {
        Value::Object(entries) => {
            for (key, value) in entries {
                chat.insert(key.clone(), value.clone());
            }
        }
        Value::Array(items) => {
            for (index, value) in items.iter().enumerate() {
                chat.insert(index.to_string(), value.clone());
            }
        }
        _ => {}
    }
}

/// `chat.messages.push(record)`. 갱신 줄이 messages를 배열이 아닌 값으로 덮었으면 TS처럼 읽기를 실패시킨다
fn push_chat_message(chat: &mut Map<String, Value>, record: Value) -> AiResult<()> {
    match chat.get_mut("messages") {
        Some(Value::Array(messages)) => {
            messages.push(record);
            Ok(())
        }
        _ => Err(AiError::message(
            "Gemini 대화 기록의 messages가 배열이 아닙니다.",
        )),
    }
}

fn flatten_gemini_messages(chat: &Value) -> Vec<AggregatedAiMessage> {
    let Some(Value::Array(records)) = first_present(chat, &["messages"]) else {
        return Vec::new();
    };
    let mut flattened = Vec::new();
    for message in records
        .iter()
        .filter(|record| record.is_object() || record.is_array())
    {
        let timestamp = first_present_or_null(message, &["timestamp", "createdAt", "time"]);
        flattened.extend(extract_gemini_tool_messages(message, timestamp));
        flattened.extend(extract_gemini_thought_messages(message, timestamp));
        let role = resolve_gemini_role(first_present(message, &["type", "role"]));
        let text = extract_plain_text(first_present_or_null(
            message,
            &["content", "parts", "text", "message"],
        ));
        if !text.is_empty() {
            flattened.extend(make_preview_message(role, timestamp, &text, Vec::new()));
        }
    }
    flattened
}

/// 값 하나나 배열을 항목 목록으로 편다. 거짓 값이면 비어 있다
fn truthy_items(value: Option<&Value>) -> Vec<&Value> {
    match value {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(item) if is_truthy(item) => vec![item],
        _ => Vec::new(),
    }
}

fn extract_gemini_thought_messages(message: &Value, timestamp: &Value) -> Vec<AggregatedAiMessage> {
    truthy_items(first_present(
        message,
        &["thoughts", "thought", "reasoning"],
    ))
    .into_iter()
    .filter_map(|thought| {
        make_preview_message(
            AiMessageRole::Reasoning,
            timestamp,
            &extract_plain_text(thought),
            Vec::new(),
        )
    })
    .collect()
}

fn extract_gemini_tool_messages(message: &Value, timestamp: &Value) -> Vec<AggregatedAiMessage> {
    let keys = [
        "toolCalls",
        "tool_calls",
        "functionCalls",
        "function_calls",
        "tools",
    ];
    truthy_items(first_present(message, &keys))
        .into_iter()
        .filter_map(|tool_call| {
            make_preview_message(
                AiMessageRole::Tool,
                timestamp,
                &format_gemini_tool_call(tool_call),
                Vec::new(),
            )
        })
        .collect()
}

fn format_gemini_tool_call(tool_call: &Value) -> String {
    if !(tool_call.is_object() || tool_call.is_array()) {
        return extract_plain_text(tool_call);
    }
    let name = ["name", "tool", "functionName"]
        .iter()
        .find_map(|key| tool_call.get(*key).and_then(Value::as_str))
        .unwrap_or("tool");
    let args = stringify_unknown(first_present(tool_call, &["args", "arguments", "input"]));
    let result = extract_plain_text(first_present_or_null(
        tool_call,
        &["result", "output", "response", "content"],
    ));
    let parts = [
        name.to_string(),
        if args.is_empty() {
            String::new()
        } else {
            format!("args: {args}")
        },
        if result.is_empty() {
            String::new()
        } else {
            format!("result: {result}")
        },
    ];
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" | ")
}

fn stringify_unknown(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(scalar @ (Value::Number(_) | Value::Bool(_))) => js_string(scalar),
        Some(other) => stringify(other),
    }
}

fn resolve_gemini_role(role: Option<&Value>) -> AiMessageRole {
    match role.and_then(Value::as_str) {
        Some("user") => AiMessageRole::User,
        Some("gemini" | "assistant" | "model") => AiMessageRole::Assistant,
        Some("system" | "info") => AiMessageRole::System,
        Some("tool") => AiMessageRole::Tool,
        Some("reasoning" | "thought") => AiMessageRole::Reasoning,
        Some("developer") => AiMessageRole::Developer,
        _ => AiMessageRole::Unknown,
    }
}

/// `value.sessionId ?? value.id ?? 파일 이름`
fn resolve_gemini_session_id(chat: &Value, file_path: &str) -> String {
    match first_present(chat, &["sessionId", "id"]) {
        Some(Value::String(id)) => id.clone(),
        Some(other) => js_string(other),
        None => node_path::basename_without(file_path, &node_path::extname(file_path)),
    }
}

fn resolve_gemini_matched_path(
    chat: &Value,
    file_path: &str,
    project_id: Option<&str>,
    project_paths: &ProjectPathMap,
    context: &AiSessionReaderContext,
) -> AiResult<Option<String>> {
    if let Some(direct_path) =
        first_present(chat, &["cwd", "projectPath", "directory"]).filter(|path| is_truthy(path))
    {
        return Ok(Some(js_string(direct_path)));
    }
    let Some(project_id) = project_id else {
        return Ok(None);
    };
    if let Some(mapped) = project_paths
        .lock()
        .expect("프로젝트 경로")
        .get(project_id)
        .filter(|path| !path.is_empty())
    {
        return Ok(Some(mapped.clone()));
    }
    let project_root_file = node_path::join(&[
        &node_path::dirname(&node_path::dirname(file_path)),
        ".project_root",
    ]);
    let project_root = js_trim(&context.host.read_text_file(&project_root_file)?).to_string();
    if project_root.is_empty() {
        return Ok(None);
    }
    project_paths
        .lock()
        .expect("프로젝트 경로")
        .insert(project_id.to_string(), project_root.clone());
    Ok(Some(project_root))
}

fn gemini_project_id_from_chat_path(file_path: &str) -> Option<String> {
    let chats_directory = node_path::dirname(file_path);
    (node_path::basename(&chats_directory) == "chats")
        .then(|| node_path::basename(&node_path::dirname(&chats_directory)))
}

/// `projects.json`의 `{ 경로: 해시 }` 또는 `{ 경로: { id|hash|projectId|projectHash } }`를 해시 → 경로로 뒤집는다
fn read_gemini_project_path_map(
    gemini_root: &str,
    context: &AiSessionReaderContext,
) -> AiResult<HashMap<String, String>> {
    let content = context
        .host
        .read_text_file(&node_path::join(&[gemini_root, "projects.json"]))?;
    let mut map = HashMap::new();
    let Some(parsed) = parse_json(&content).filter(is_truthy) else {
        return Ok(map);
    };
    let projects = match parsed.get("projects") {
        Some(projects)
            if is_present_truthy(Some(projects))
                && (projects.is_object() || projects.is_array()) =>
        {
            projects
        }
        _ => &parsed,
    };
    for (project_path, value) in object_entries(projects) {
        let project_id = match value {
            Value::String(hash) => Some(hash.as_str()),
            Value::Object(_) | Value::Array(_) => {
                first_present(value, &["id", "hash", "projectId", "projectHash"])
                    .and_then(Value::as_str)
            }
            _ => None,
        };
        if let Some(project_id) = project_id {
            map.insert(project_id.to_string(), project_path);
        }
    }
    Ok(map)
}

/// JS `Object.entries`. 객체가 아닌 값은 항목이 없다고 본다
fn object_entries(value: &Value) -> Vec<(String, &Value)> {
    match value {
        Value::Object(entries) => entries
            .iter()
            .map(|(key, item)| (key.clone(), item))
            .collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, item)| (index.to_string(), item))
            .collect(),
        _ => Vec::new(),
    }
}

/// 이 worktree에서 지금 돌고 있는 Gemini 세션을 찾는다. Gemini CLI 기록에는 서브에이전트를 가리키는 필드가 없어 서브태스크는 항상 비어 있다
pub fn read_gemini_live_sessions(
    context: &AiSessionReaderContext,
    windows: LiveAiSessionWindows,
    now_ms: f64,
) -> AiResult<Vec<LiveProviderSnapshot>> {
    let gemini_root = gemini_root_directory(context)?;
    let project_paths = read_gemini_project_path_map(&gemini_root, context).unwrap_or_default();
    let recent_files: Vec<_> = context
        .host
        .list_files_modified_within(
            &node_path::join(&[&gemini_root, "tmp"]),
            ".json",
            windows.recent_window_ms as f64,
            now_ms,
        )?
        .into_iter()
        .filter(|file| {
            let chats_directory = node_path::dirname(&file.file_path);
            let project_hash = node_path::basename(&node_path::dirname(&chats_directory));
            node_path::basename(&chats_directory) == "chats"
                && determine_match_scope(
                    project_paths.get(&project_hash).map(String::as_str),
                    context,
                )
                .is_some()
        })
        .collect();
    let live_files = pick_live_session_files(
        &recent_files,
        windows.running_window_ms as f64,
        now_ms,
        |file| file.mtime_ms,
    );
    Ok(live_files
        .iter()
        .map(|session_file| LiveProviderSnapshot {
            session_id: Some(node_path::basename_without(
                &session_file.file_path,
                ".json",
            )),
            current_task: None,
            last_active_at: iso_string_from_ms(session_file.mtime_ms),
            running_subtasks: Vec::new(),
        })
        .collect())
}

fn gemini_root_directory(context: &AiSessionReaderContext) -> AiResult<String> {
    Ok(node_path::join(&[
        &context.host.home_directory()?,
        ".gemini",
    ]))
}
