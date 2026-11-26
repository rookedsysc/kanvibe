//! Codex rollout 기록(`~/.codex/sessions/**/rollout-*-<threadId>.jsonl`) 리더. TS `readCodexSessions.ts`를 옮긴다.
//!
//! 목록은 첫 줄 `session_meta`로 다른 작업의 기록을 먼저 걸러 본문을 읽지 않고, 본문은 수정 시각 캐시를 거쳐 읽는다.
//! 상세는 호출자가 파일을 짚어 주면 뒤에 붙은 줄만 이어 읽어 긴 대화를 다시 만들지 않는다.

mod live;
mod messages;
mod transcript;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde_json::Value;

pub use live::{read_codex_agent_call_graph, read_codex_live_sessions};
use messages::{
    create_codex_message_key, event_timestamp, extract_codex_event_messages,
    should_skip_codex_message,
};
use transcript::{RolloutEvents, read_codex_session_transcript};

use crate::error::AiResult;
use crate::jsonl::{read_appended_json_lines, read_json_lines, read_json_lines_head};
use crate::message_text::{to_iso_string, truncate_text, truncate_text_to};
use crate::node_path;
use crate::parse_cache::{ParseCacheKind, cached_or_parse};
use crate::providers::AiSessionProvider;
use crate::record_fields::{first_present, matches_query, string_field};
use crate::session_listing::{
    create_reader_result, create_unavailable_result, determine_match_scope, map_with_concurrency,
    newest_first_message_order, paginate_messages_in_order, parse_concurrency,
};
use crate::types::{
    AggregatedAiSession, AggregatedAiSessionDetail, AiMessageRole, AiSessionMatchScope,
    AiSessionReaderContext, AiSessionReaderResult,
};

pub const DEFAULT_DETAIL_LIMIT: usize = 20;
const SESSION_TITLE_LENGTH: usize = 80;

static THREAD_ID_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
        .expect("스레드 ID 패턴")
});

/// Codex 스레드 ID(UUID 소문자 꼴)인지
pub fn is_codex_thread_id(thread_id: &str) -> bool {
    THREAD_ID_PATTERN.is_match(thread_id)
}

pub fn read_codex_sessions(context: &AiSessionReaderContext) -> AiResult<AiSessionReaderResult> {
    let sessions_directory = codex_sessions_directory(context)?;
    if !context.host.path_exists(&sessions_directory)? {
        return Ok(create_unavailable_result(
            AiSessionProvider::Codex,
            "Codex sessions directory not found",
        ));
    }
    let rollout_files = context
        .host
        .list_files_recursively_by_suffix(&sessions_directory, ".jsonl")?;
    let summaries = map_with_concurrency(
        &rollout_files,
        parse_concurrency(context),
        |file_path, _| parse_codex_session_summary(file_path, context),
    );
    let sessions: Vec<AggregatedAiSession> = summaries
        .into_iter()
        .collect::<AiResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    let mut result = create_reader_result(AiSessionProvider::Codex, sessions);
    if result.sessions.is_empty() {
        result.reason = Some("No Codex sessions matched this task".to_string());
    }
    Ok(result)
}

/// TS `readCodexSessionDetail(context, sessionId, sourceRef, cursor, limit = 20)`
pub fn read_codex_session_detail(
    context: &AiSessionReaderContext,
    session_id: &str,
    source_ref: Option<&str>,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> AiResult<Option<AggregatedAiSessionDetail>> {
    let source_ref = source_ref.filter(|source| !source.is_empty());
    let rollout_files = match source_ref {
        Some(source) => vec![source.to_string()],
        None => context
            .host
            .list_files_recursively_by_suffix(&codex_sessions_directory(context)?, ".jsonl")?,
    };
    let limit = limit.unwrap_or(DEFAULT_DETAIL_LIMIT);
    for file_path in &rollout_files {
        let detail = parse_codex_session_detail(
            file_path,
            context,
            session_id,
            cursor,
            limit,
            source_ref.is_some(),
        )?;
        if detail.is_some() {
            return Ok(detail);
        }
    }
    Ok(None)
}

/// 화면의 전체 ID와 작업 범위가 모두 일치하는 메인 대화만 찾는다. 같은 ID의 복제 기록도 임의로 고르지 않는다
pub fn find_codex_main_thread_rollout(
    context: &AiSessionReaderContext,
    thread_id: &str,
) -> AiResult<Option<String>> {
    if !is_codex_thread_id(thread_id) {
        return Ok(None);
    }
    let root = codex_sessions_directory(context)?;
    let suffix = format!("-{thread_id}.jsonl");
    let files = context
        .host
        .list_files_recursively_by_suffix(&root, ".jsonl")?;
    let matches: Vec<String> = files
        .into_iter()
        .filter(|file| node_path::basename(file).ends_with(&suffix))
        .filter(|file| {
            read_codex_session_meta(file, context).is_some_and(|meta| {
                meta.thread_id == thread_id
                    && meta.parent_thread_id.as_deref().is_none_or(str::is_empty)
                    && !meta.is_subagent
                    && determine_match_scope(meta.cwd.as_deref(), context).is_some()
            })
        })
        .collect();
    Ok(if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    })
}

/// rollout 첫 줄 `session_meta`가 알려 주는 스레드 정보
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CodexThreadMeta {
    pub thread_id: String,
    pub parent_thread_id: Option<String>,
    pub is_subagent: bool,
    pub agent_nickname: Option<String>,
    pub cwd: Option<String>,
    pub started_at: Option<String>,
}

/// rollout 첫 줄의 `session_meta`만 읽는다. 본문까지 파싱하면 폴링마다 전체 대화를 훑게 된다. 읽지 못하면 `None`이다
pub(crate) fn read_codex_session_meta(
    file_path: &str,
    context: &AiSessionReaderContext,
) -> Option<CodexThreadMeta> {
    let first_line = read_json_lines_head(&context.host, file_path, 1)
        .ok()?
        .into_iter()
        .next()?;
    if first_line.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    let meta = first_line
        .get("payload")
        .filter(|payload| crate::javascript::is_truthy(payload))?;
    let thread_id = string_field(meta, "id").or_else(|| string_field(meta, "session_id"))?;
    if thread_id.is_empty() {
        return None;
    }
    let is_subagent = meta
        .get("source")
        .and_then(Value::as_object)
        .is_some_and(|source| source.contains_key("subagent"));
    Some(CodexThreadMeta {
        thread_id: thread_id.to_string(),
        parent_thread_id: string_field(meta, "parent_thread_id").map(str::to_string),
        is_subagent,
        agent_nickname: string_field(meta, "agent_nickname").map(str::to_string),
        cwd: string_field(meta, "cwd").map(str::to_string),
        started_at: to_iso_string(meta.get("timestamp").unwrap_or(&Value::Null)),
    })
}

fn read_full_rollout(
    file_path: &str,
    context: &AiSessionReaderContext,
) -> AiResult<Arc<RolloutEvents>> {
    cached_or_parse(ParseCacheKind::Full, &context.host, file_path, || {
        read_json_lines(&context.host, file_path).map(RolloutEvents::Parsed)
    })
}

/// 목록 한 줄을 만드는 동안 모으는 값
#[derive(Default)]
struct SummaryProgress {
    session_id: Option<String>,
    matched_path: Option<String>,
    started_at: Option<String>,
    updated_at: Option<String>,
    first_user_prompt: Option<String>,
    message_count: u64,
    has_query_match: bool,
    seen_messages: HashSet<String>,
    tool_names: HashMap<String, String>,
}

fn parse_codex_session_summary(
    file_path: &str,
    context: &AiSessionReaderContext,
) -> AiResult<Option<AggregatedAiSession>> {
    let Some(meta) = read_codex_session_meta(file_path, context) else {
        return Ok(None);
    };
    if determine_match_scope(meta.cwd.as_deref(), context).is_none() {
        return Ok(None);
    }
    let events = read_full_rollout(file_path, context)?;
    let mut progress = SummaryProgress::default();
    for event in events.iter() {
        if event.get("type").and_then(Value::as_str) == Some("session_meta") {
            if !read_summary_meta(&mut progress, event, file_path, context) {
                return Ok(None);
            }
            continue;
        }
        read_summary_messages(&mut progress, event, file_path, context);
    }
    Ok(finish_codex_summary(progress, file_path, context))
}

/// session_meta 한 줄을 반영한다. 다른 작업 경로의 기록이면 `false`다
fn read_summary_meta(
    progress: &mut SummaryProgress,
    event: &Value,
    file_path: &str,
    context: &AiSessionReaderContext,
) -> bool {
    let payload = event
        .get("payload")
        .filter(|payload| !payload.is_null())
        .unwrap_or(&Value::Null);
    let cwd = string_field(payload, "cwd");
    if determine_match_scope(cwd, context).is_none() {
        return false;
    }
    let session_id = string_field(payload, "id")
        .map(str::to_string)
        .unwrap_or_else(|| node_path::basename_without(file_path, ".jsonl"));
    let timestamp =
        first_present(payload, &["timestamp"]).or_else(|| first_present(event, &["timestamp"]));
    let started_at = to_iso_string(timestamp.unwrap_or(&Value::Null));
    if matches_query(context.query.as_deref(), &[cwd, Some(&session_id)]) {
        progress.has_query_match = true;
    }
    progress.matched_path = cwd.map(str::to_string);
    progress.session_id = Some(session_id);
    progress.updated_at = started_at.clone();
    progress.started_at = started_at;
    true
}

fn read_summary_messages(
    progress: &mut SummaryProgress,
    event: &Value,
    file_path: &str,
    context: &AiSessionReaderContext,
) {
    let timestamp = event_timestamp(event);
    for message in extract_codex_event_messages(event, file_path, &mut progress.tool_names) {
        if should_skip_codex_message(message.role, &message.text) {
            continue;
        }
        let message_key =
            create_codex_message_key(message.role, timestamp, &message.text, &message.attachments);
        if !progress.seen_messages.insert(message_key) {
            continue;
        }
        if message.role == AiMessageRole::User && progress.first_user_prompt.is_none() {
            progress.first_user_prompt = first_prompt_text(&message.text, &message.attachments);
        }
        if matches_query(context.query.as_deref(), &[Some(&message.text)]) {
            progress.has_query_match = true;
        }
        progress.message_count += 1;
        if matches!(message.role, AiMessageRole::User | AiMessageRole::Assistant) {
            progress.updated_at = to_iso_string(timestamp).or(progress.updated_at.take());
        }
    }
}

/// `message.text || 첨부 이름들 || null`
fn first_prompt_text(
    text: &str,
    attachments: &[crate::types::AiMessageAttachment],
) -> Option<String> {
    if !text.is_empty() {
        return Some(text.to_string());
    }
    let names = attachments
        .iter()
        .map(|attachment| attachment.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    (!names.is_empty()).then_some(names)
}

fn finish_codex_summary(
    progress: SummaryProgress,
    file_path: &str,
    context: &AiSessionReaderContext,
) -> Option<AggregatedAiSession> {
    let session_id = progress.session_id?;
    let matched_path = progress.matched_path.filter(|path| !path.is_empty())?;
    let query = context.query.as_deref().filter(|query| !query.is_empty());
    let first_user_prompt = progress.first_user_prompt;
    let is_metadata_match = matches_query(
        query,
        &[
            first_user_prompt.as_deref(),
            Some(&matched_path),
            Some(&session_id),
        ],
    );
    if query.is_some() && !is_metadata_match && !progress.has_query_match {
        return None;
    }
    Some(AggregatedAiSession {
        id: session_id,
        provider: AiSessionProvider::Codex,
        started_at: progress.started_at,
        updated_at: progress.updated_at,
        matched_path: Some(matched_path),
        match_scope: AiSessionMatchScope::Worktree,
        title: first_user_prompt
            .as_deref()
            .map(|prompt| truncate_text_to(prompt, SESSION_TITLE_LENGTH)),
        first_user_prompt: first_user_prompt.as_deref().map(truncate_text),
        message_count: progress.message_count,
        source_ref: Some(file_path.to_string()),
    })
}

/// 호출자가 이 대화의 파일을 짚어 준 로컬 기록은 되풀이해 읽히므로 붙은 줄만 이어 읽는다
fn parse_codex_session_detail(
    file_path: &str,
    context: &AiSessionReaderContext,
    session_id: &str,
    cursor: Option<&str>,
    limit: usize,
    is_known_session_file: bool,
) -> AiResult<Option<AggregatedAiSessionDetail>> {
    let events = cached_or_parse(ParseCacheKind::Full, &context.host, file_path, || {
        if is_known_session_file && !context.host.is_remote() {
            return read_appended_json_lines(file_path).map(RolloutEvents::Appended);
        }
        read_json_lines(&context.host, file_path).map(RolloutEvents::Parsed)
    })?;
    let shared = read_codex_session_transcript(&events, file_path, session_id, context);
    let mut transcript = shared.lock().expect("Codex 대화");
    if transcript.is_other_session || transcript.matched_path.as_deref().is_none_or(str::is_empty) {
        return Ok(None);
    }
    let transcript = &mut *transcript;
    let newest_first = transcript
        .newest_first_order
        .get_or_insert_with(|| newest_first_message_order(&transcript.messages));
    let page = paginate_messages_in_order(&transcript.messages, newest_first, cursor, limit);
    Ok(Some(AggregatedAiSessionDetail {
        session_id: session_id.to_string(),
        provider: AiSessionProvider::Codex,
        title: transcript.title.clone(),
        matched_path: transcript.matched_path.clone(),
        source_ref: Some(file_path.to_string()),
        messages: page.items,
        next_cursor: page.next_cursor,
    }))
}

pub(crate) fn codex_sessions_directory(context: &AiSessionReaderContext) -> AiResult<String> {
    Ok(node_path::join(&[
        &context.host.home_directory()?,
        ".codex",
        "sessions",
    ]))
}
