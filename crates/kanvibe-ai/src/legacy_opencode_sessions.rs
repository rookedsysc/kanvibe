//! SQLite로 옮기기 전 OpenCode의 파일 저장소(`${XDG_DATA_HOME:-~/.local/share}/opencode/storage`).
//! `session/**/<id>.json`, `message/<id>/*.json`, `part/<messageId>/*.json`을 읽는다.
//! 저장소 밖을 가리키는 경로·링크와 안전하지 않은 ID는 읽지 않는다.

use std::cmp::Ordering;

use kanvibe_domain::locale_order::compare_like_locale;
use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

use crate::error::AiResult;
use crate::javascript::{js_trim, parse_date_ms, parse_json};
use crate::message_text::{
    extract_plain_text, make_preview_message, to_iso_string, truncate_text, truncate_text_to,
};
use crate::node_path;
use crate::parse_cache::{ParseCacheKind, cached_or_parse};
use crate::providers::AiSessionProvider;
use crate::session_listing::{determine_match_scope, map_with_concurrency, paginate_items};
use crate::types::{
    AggregatedAiMessage, AggregatedAiSession, AggregatedAiSessionDetail, AiMessageRole,
    AiSessionMatchScope, AiSessionReaderContext, AiSessionReaderResult,
};

const FILE_CONCURRENCY: usize = 4;
const DEFAULT_DETAIL_LIMIT: usize = 20;
const SESSION_TITLE_MAX_LENGTH: usize = 80;

static SAFE_STORAGE_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^[A-Za-z0-9_-]+$").expect("저장소 ID 패턴"));

type JsonRecord = Map<String, Value>;

struct LegacySession {
    summary: AggregatedAiSession,
    messages: Vec<AggregatedAiMessage>,
}

/// TS `string()`. 공백만 있는 글은 없는 값으로 보되, 값 자체는 다듬지 않는다
fn non_blank_string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|text| !js_trim(text).is_empty())
}

fn record_field<'a>(record: &'a JsonRecord, key: &str) -> Option<&'a Value> {
    record.get(key)
}

/// `record(value.time).created` 같은 객체 안 필드. 객체가 아니면 없는 값이다
fn nested_field<'a>(record: &'a JsonRecord, outer: &str, inner: &str) -> &'a Value {
    record
        .get(outer)
        .and_then(Value::as_object)
        .and_then(|nested| nested.get(inner))
        .unwrap_or(&Value::Null)
}

/// 파일이 `root` 아래에 있는지. 같은 경로이거나 밖이면 아니다
pub(crate) fn is_within(file: &str, root: &str) -> bool {
    let resolved_root = node_path::resolve(&[root]);
    let resolved_file = node_path::resolve(&[file]);
    if resolved_file == resolved_root {
        return false;
    }
    if resolved_root == "/" {
        return true;
    }
    resolved_file.starts_with(&format!("{resolved_root}/"))
}

fn storage_directory(context: &AiSessionReaderContext) -> AiResult<String> {
    let home = context.host.home_directory()?;
    let configured = context
        .host
        .variable("XDG_DATA_HOME")
        .map(js_trim)
        .filter(|value| !value.is_empty() && node_path::is_absolute(value));
    let data_home = configured.map_or_else(
        || node_path::join(&[&home, ".local", "share"]),
        str::to_string,
    );
    Ok(node_path::join(&[&data_home, "opencode", "storage"]))
}

/// 로컬 파일이 링크이거나 실제 경로가 저장소 밖이면 거른다
fn is_local_link_or_outside(file: &str, root: &str) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(file) else {
        return true;
    };
    if metadata.file_type().is_symlink() {
        return true;
    }
    let (Ok(real_file), Ok(real_root)) = (std::fs::canonicalize(file), std::fs::canonicalize(root))
    else {
        return true;
    };
    !is_within(&real_file.to_string_lossy(), &real_root.to_string_lossy())
}

/// 저장소 안의 JSON 객체 하나. 부분 쓰기·삭제 파일 하나가 나머지 대화를 가리지 않도록 실패는 모두 `None`이다
fn read_json(file: &str, root: &str, context: &AiSessionReaderContext) -> Option<JsonRecord> {
    if !is_within(file, root) || !file.ends_with(".json") {
        return None;
    }
    if !context.host.is_remote() && is_local_link_or_outside(file, root) {
        return None;
    }
    let raw = cached_or_parse(ParseCacheKind::Full, &context.host, file, || {
        context.host.read_text_file(file)
    })
    .ok()?;
    match parse_json(&raw)? {
        Value::Object(record) => Some(record),
        _ => None,
    }
}

/// JS 기본 정렬(UTF-16 코드 단위 순서)
fn sort_like_js(values: &mut [String]) {
    values.sort_by(|left, right| left.encode_utf16().cmp(right.encode_utf16()));
}

fn message_content(
    message: &JsonRecord,
    id: &str,
    root: &str,
    context: &AiSessionReaderContext,
) -> AiResult<String> {
    let inline = extract_plain_text(record_field(message, "content").unwrap_or(&Value::Null));
    if !inline.is_empty() {
        return Ok(inline);
    }
    if !SAFE_STORAGE_ID.is_match(id) {
        return Ok(String::new());
    }
    let part_directory = node_path::join(&[root, "part", id]);
    let mut files = context
        .host
        .read_directory_files_by_suffix(&part_directory, ".json")?;
    sort_like_js(&mut files);
    let parts = map_with_concurrency(&files, FILE_CONCURRENCY, |file, _| {
        read_json(file, root, context)
    });
    Ok(parts
        .into_iter()
        .flatten()
        .filter(|part| match part.get("messageID") {
            None => true,
            Some(message_id) => message_id.as_str() == Some(id),
        })
        .map(|part| extract_plain_text(&Value::Object(part)))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n"))
}

/// 만든 시각 순서, 같으면 파일 이름 순서. 시각이 없는 메시지는 뒤로 간다
fn compare_message_files(left: &(String, JsonRecord), right: &(String, JsonRecord)) -> Ordering {
    let time_value = |record: &JsonRecord| {
        to_iso_string(nested_field(record, "time", "created"))
            .map_or(Some(f64::INFINITY), |iso| parse_date_ms(&iso))
    };
    let difference = match (time_value(&left.1), time_value(&right.1)) {
        (Some(left_time), Some(right_time)) => left_time - right_time,
        _ => f64::NAN,
    };
    if difference.is_nan() || difference == 0.0 {
        return compare_like_locale(&left.0, &right.0);
    }
    difference.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
}

fn to_legacy_message(
    message_file: &str,
    value: &JsonRecord,
    session_id: &str,
    root: &str,
    context: &AiSessionReaderContext,
) -> AiResult<Option<AggregatedAiMessage>> {
    if let Some(owner) = value.get("sessionID")
        && owner.as_str() != Some(session_id)
    {
        return Ok(None);
    }
    let role = match value.get("role").and_then(Value::as_str) {
        Some("user") => AiMessageRole::User,
        Some("assistant") => AiMessageRole::Assistant,
        Some("tool") => AiMessageRole::Tool,
        Some("system") => AiMessageRole::System,
        _ => return Ok(None),
    };
    let message_id = non_blank_string(value.get("id"))
        .map(str::to_string)
        .unwrap_or_else(|| node_path::basename_without(message_file, ".json"));
    let text = message_content(value, &message_id, root, context)?;
    Ok(make_preview_message(
        role,
        nested_field(value, "time", "created"),
        &text,
        Vec::new(),
    ))
}

fn read_session_messages(
    session_id: &str,
    root: &str,
    context: &AiSessionReaderContext,
) -> AiResult<Vec<AggregatedAiMessage>> {
    let message_directory = node_path::join(&[root, "message", session_id]);
    let message_files = context
        .host
        .read_directory_files_by_suffix(&message_directory, ".json")?;
    let values = map_with_concurrency(&message_files, FILE_CONCURRENCY, |file, _| {
        read_json(file, root, context).map(|value| (file.clone(), value))
    });
    let mut ordered: Vec<(String, JsonRecord)> = values.into_iter().flatten().collect();
    ordered.sort_by(compare_message_files);
    let messages = map_with_concurrency(&ordered, FILE_CONCURRENCY, |(file, value), _| {
        to_legacy_message(file, value, session_id, root, context)
    });
    Ok(messages
        .into_iter()
        .collect::<AiResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

fn read_legacy_session(
    file: &str,
    root: &str,
    context: &AiSessionReaderContext,
) -> AiResult<Option<LegacySession>> {
    if !is_within(file, &node_path::join(&[root, "session"])) {
        return Ok(None);
    }
    let Some(metadata) = read_json(file, root, context) else {
        return Ok(None);
    };
    let id = non_blank_string(metadata.get("id"))
        .map(str::to_string)
        .unwrap_or_else(|| node_path::basename_without(file, ".json"));
    let directory = non_blank_string(metadata.get("directory"));
    let Some(match_scope) = determine_match_scope(directory, context) else {
        return Ok(None);
    };
    if !SAFE_STORAGE_ID.is_match(&id) {
        return Ok(None);
    }
    let messages = read_session_messages(&id, root, context)?;
    let summary = SummaryParts {
        id,
        file,
        directory,
        match_scope,
        metadata: &metadata,
    }
    .build(&messages);
    Ok(Some(LegacySession { summary, messages }))
}

/// 세션 목록 한 줄을 만드는 데 필요한 값
struct SummaryParts<'a> {
    id: String,
    file: &'a str,
    directory: Option<&'a str>,
    match_scope: AiSessionMatchScope,
    metadata: &'a JsonRecord,
}

impl SummaryParts<'_> {
    /// 제목이 없으면 첫 사용자 요청을, 갱신 시각은 가장 늦은 메시지 시각을 쓴다
    fn build(self, messages: &[AggregatedAiMessage]) -> AggregatedAiSession {
        let first_user_prompt = messages
            .iter()
            .find(|message| message.role == AiMessageRole::User)
            .map(|message| message.full_text.clone())
            .filter(|prompt| !prompt.is_empty());
        let title = non_blank_string(self.metadata.get("title"))
            .map(str::to_string)
            .or_else(|| {
                first_user_prompt
                    .as_deref()
                    .map(|prompt| truncate_text_to(prompt, SESSION_TITLE_MAX_LENGTH))
            });
        let mut timestamps: Vec<String> = messages
            .iter()
            .filter_map(|message| message.timestamp.clone())
            .collect();
        sort_like_js(&mut timestamps);
        let updated_at = timestamps
            .pop()
            .or_else(|| to_iso_string(nested_field(self.metadata, "time", "updated")));
        AggregatedAiSession {
            id: self.id,
            provider: AiSessionProvider::Opencode,
            started_at: to_iso_string(nested_field(self.metadata, "time", "created")),
            updated_at,
            matched_path: self.directory.map(str::to_string),
            match_scope: self.match_scope,
            title,
            first_user_prompt: first_user_prompt.map(|prompt| truncate_text(&prompt)),
            message_count: messages.len() as u64,
            source_ref: Some(self.file.to_string()),
        }
    }
}

fn unavailable(reason: &str) -> AiSessionReaderResult {
    AiSessionReaderResult {
        provider: AiSessionProvider::Opencode,
        available: false,
        session_count: 0,
        reason: Some(reason.to_string()),
        sessions: Vec::new(),
    }
}

fn matches_listing_query(session: &LegacySession, query: &str) -> bool {
    let summary = &session.summary;
    let fields = [
        Some(summary.id.as_str()),
        summary.title.as_deref(),
        summary.matched_path.as_deref(),
    ];
    fields
        .into_iter()
        .flatten()
        .chain(
            session
                .messages
                .iter()
                .map(|message| message.full_text.as_str()),
        )
        .any(|text| text.to_lowercase().contains(query))
}

/// 이전 파일 저장소의 세션. 저장소가 없거나 읽을 수 없어도 SQLite 조회를 막지 않도록 실패를 결과로 알린다
pub fn read_legacy_opencode_sessions(
    context: &AiSessionReaderContext,
) -> AiResult<AiSessionReaderResult> {
    Ok(read_legacy_sessions_checked(context)
        .unwrap_or_else(|_| unavailable("Legacy OpenCode storage could not be read")))
}

fn read_legacy_sessions_checked(
    context: &AiSessionReaderContext,
) -> AiResult<AiSessionReaderResult> {
    let root = storage_directory(context)?;
    let session_root = node_path::join(&[&root, "session"]);
    if !context.host.path_exists(&session_root)? {
        return Ok(unavailable("Legacy OpenCode storage not found"));
    }
    let files = context
        .host
        .list_files_recursively_by_suffix(&session_root, ".json")?;
    let values = map_with_concurrency(&files, FILE_CONCURRENCY, |file, _| {
        read_legacy_session(file, &root, context)
    })
    .into_iter()
    .collect::<AiResult<Vec<_>>>()?;
    let query = context
        .query
        .as_deref()
        .filter(|query| !query.is_empty())
        .map(str::to_lowercase);
    let sessions: Vec<AggregatedAiSession> = values
        .into_iter()
        .flatten()
        .filter(|session| {
            query
                .as_deref()
                .is_none_or(|query| matches_listing_query(session, query))
        })
        .map(|session| session.summary)
        .collect();
    Ok(AiSessionReaderResult {
        provider: AiSessionProvider::Opencode,
        available: true,
        session_count: sessions.len() as u64,
        reason: None,
        sessions,
    })
}

fn is_message_visible(message: &AggregatedAiMessage, context: &AiSessionReaderContext) -> bool {
    let is_role_allowed = context
        .roles
        .as_ref()
        .is_none_or(|roles| roles.is_empty() || roles.contains(&message.role));
    let is_query_match = context
        .query
        .as_deref()
        .filter(|query| !query.is_empty())
        .is_none_or(|query| {
            message
                .full_text
                .to_lowercase()
                .contains(&query.to_lowercase())
        });
    is_role_allowed && is_query_match
}

/// 이전 파일 저장소의 대화 하나. 최근 메시지가 먼저 오도록 뒤집어 페이지를 나눈다
pub fn read_legacy_opencode_session_detail(
    context: &AiSessionReaderContext,
    session_id: &str,
    source_ref: Option<&str>,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> AiResult<Option<AggregatedAiSessionDetail>> {
    if !SAFE_STORAGE_ID.is_match(session_id) {
        return Ok(None);
    }
    let root = storage_directory(context)?;
    let session_root = node_path::join(&[&root, "session"]);
    let source_ref = source_ref.filter(|reference| !reference.is_empty());
    if source_ref.is_some_and(|reference| !is_within(reference, &session_root)) {
        return Ok(None);
    }
    let files = match source_ref {
        Some(reference) => vec![reference.to_string()],
        None => context
            .host
            .list_files_recursively_by_suffix(&session_root, ".json")?,
    };
    for file in files {
        let Some(parsed) = read_legacy_session(&file, &root, context)? else {
            continue;
        };
        if parsed.summary.id != session_id {
            continue;
        }
        let filtered: Vec<AggregatedAiMessage> = parsed
            .messages
            .iter()
            .rev()
            .filter(|message| is_message_visible(message, context))
            .cloned()
            .collect();
        let page = paginate_items(&filtered, cursor, limit.unwrap_or(DEFAULT_DETAIL_LIMIT));
        return Ok(Some(AggregatedAiSessionDetail {
            session_id: session_id.to_string(),
            provider: AiSessionProvider::Opencode,
            title: parsed.summary.title,
            matched_path: parsed.summary.matched_path,
            source_ref: Some(file),
            messages: page.items,
            next_cursor: page.next_cursor,
        }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
