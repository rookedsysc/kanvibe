//! Claude Code 세션 기록(`~/.claude/projects/<encoded-cwd>/*.jsonl`) 리더. TS `readClaudeSessions.ts`를 옮긴다.
//!
//! 목록은 파일 앞 60줄로 제목을, 끝 60줄로 최근 대화 시각을 읽어 긴 JSONL 전체 전송을 피한다.
//! 상세는 호출자가 짚어 준 파일이면 붙은 줄만 이어 읽고, 이미 만든 메시지는 다시 만들지 않는다.

mod call_graph;
mod events;
mod live;
mod transcripts;

use serde_json::Value;
use std::sync::Arc;

use crate::error::AiResult;
use crate::jsonl::{
    read_appended_json_lines, read_json_lines, read_json_lines_head, read_json_lines_tail,
};
use crate::message_attachments::extract_message_attachments;
use crate::message_text::{extract_plain_text, to_iso_string, truncate_text_to};
use crate::node_path;
use crate::parse_cache::{ParseCacheKind, cached_or_parse};
use crate::providers::AiSessionProvider;
use crate::session_listing::{
    LOCAL_SESSION_FILE_PARSE_CONCURRENCY, Page, REMOTE_SESSION_FILE_PARSE_CONCURRENCY,
    create_reader_result, create_unavailable_result, determine_match_scope, get_candidate_paths,
    map_with_concurrency, newest_first_message_order, paginate_items, paginate_messages_in_order,
    sort_messages_descending,
};
use crate::types::{
    AggregatedAiMessage, AggregatedAiSession, AggregatedAiSessionDetail, AiMessageRole,
    AiSessionMatchScope, AiSessionReaderContext, AiSessionReaderResult,
};
use events::{
    claude_message_text, is_claude_image_companion, message_content, resolve_claude_role,
    string_field,
};
use transcripts::{ClaudeEventLog, with_session_transcript};

pub use call_graph::read_claude_agent_call_graph;
pub use live::read_claude_live_sessions;

const DEFAULT_DETAIL_LIMIT: usize = 20;
const CLAUDE_SUMMARY_EVENT_LIMIT: usize = 60;
const SESSION_TITLE_LENGTH: usize = 80;

pub fn read_claude_sessions(context: &AiSessionReaderContext) -> AiResult<AiSessionReaderResult> {
    if !context.host.path_exists(&claude_root_directory(context)?)? {
        return Ok(create_unavailable_result(
            AiSessionProvider::Claude,
            "Claude Code directory not found",
        ));
    }
    let project_files = find_project_files(context)?;
    if project_files.is_empty() {
        let mut result = create_reader_result(AiSessionProvider::Claude, Vec::new());
        result.reason = Some("No Claude project session files matched this task".to_string());
        return Ok(result);
    }

    // TS는 로컬 파일을 한꺼번에 파싱하지만, 스레드를 파일 수만큼 띄우지 않도록 로컬도 동시성 상한을 둔다
    let concurrency = if context.host.is_remote() {
        REMOTE_SESSION_FILE_PARSE_CONCURRENCY
    } else {
        LOCAL_SESSION_FILE_PARSE_CONCURRENCY
    };
    let parsed = map_with_concurrency(&project_files, concurrency, |file_path, _| {
        parse_claude_session_from_file(file_path, context)
    });
    let sessions = parsed
        .into_iter()
        .collect::<AiResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    Ok(create_reader_result(AiSessionProvider::Claude, sessions))
}

pub fn read_claude_session_detail(
    context: &AiSessionReaderContext,
    session_id: &str,
    source_ref: Option<&str>,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> AiResult<Option<AggregatedAiSessionDetail>> {
    let source_ref = source_ref.filter(|source| !source.is_empty());
    let project_files = match source_ref {
        Some(source) => vec![source.to_string()],
        None => find_project_files(context)?,
    };
    if project_files.is_empty() {
        return Ok(None);
    }
    let request = DetailPageRequest {
        session_id,
        cursor,
        limit: limit.unwrap_or(DEFAULT_DETAIL_LIMIT),
        is_single_file: project_files.len() == 1,
        // 호출자가 짚어 준 대화 파일은 되풀이해 읽히므로 붙은 줄만 이어 읽는다. 여러 파일을 훑는 조회는 한 번씩만 읽고 지나간다
        should_append: source_ref.is_some() && !context.host.is_remote(),
    };
    let mut collection = DetailCollection {
        title: None,
        matched_path: None,
        source_ref: source_ref.map(str::to_string),
        views: Vec::new(),
    };
    for file_path in &project_files {
        let view = view_file_transcript(context, file_path, &request)?;
        collection.title = collection.title.or_else(|| view.title.clone());
        if collection.matched_path.is_none() && view.matched_path.is_some() {
            collection.matched_path = view.matched_path.clone();
            collection.source_ref = Some(file_path.clone());
        }
        collection.views.push(view);
    }
    Ok(collection.into_detail(&request))
}

/// 상세 한 페이지를 고르는 조건
struct DetailPageRequest<'a> {
    session_id: &'a str,
    cursor: Option<&'a str>,
    limit: usize,
    is_single_file: bool,
    should_append: bool,
}

/// 파일 하나에서 읽은 대화. 파일이 하나뿐이면 기억해 둔 최신순 목록으로 바로 페이지를 만든다
struct TranscriptView {
    title: Option<String>,
    matched_path: Option<String>,
    is_empty: bool,
    page: Option<Page<AggregatedAiMessage>>,
    messages: Vec<AggregatedAiMessage>,
}

fn view_file_transcript(
    context: &AiSessionReaderContext,
    file_path: &str,
    request: &DetailPageRequest<'_>,
) -> AiResult<TranscriptView> {
    let log = load_event_log(context, file_path, request.should_append)?;
    let view =
        with_session_transcript(&log, file_path, request.session_id, context, |transcript| {
            let page = request.is_single_file.then(|| {
                let newest_first = transcript
                    .newest_first_order
                    .get_or_insert_with(|| newest_first_message_order(&transcript.messages));
                paginate_messages_in_order(
                    &transcript.messages,
                    newest_first,
                    request.cursor,
                    request.limit,
                )
            });
            let messages = if request.is_single_file {
                Vec::new()
            } else {
                transcript.messages.clone()
            };
            TranscriptView {
                title: transcript.title.clone(),
                matched_path: transcript.matched_path.clone(),
                is_empty: transcript.messages.is_empty(),
                page,
                messages,
            }
        });
    Ok(view)
}

/// 파일들에서 모은 상세
struct DetailCollection {
    title: Option<String>,
    matched_path: Option<String>,
    source_ref: Option<String>,
    views: Vec<TranscriptView>,
}

impl DetailCollection {
    fn into_detail(self, request: &DetailPageRequest<'_>) -> Option<AggregatedAiSessionDetail> {
        if self.matched_path.is_none() && self.views.iter().all(|view| view.is_empty) {
            return None;
        }
        let mut views = self.views;
        let page = match views.first_mut().and_then(|view| view.page.take()) {
            Some(page) => page,
            None => {
                let merged: Vec<AggregatedAiMessage> =
                    views.into_iter().flat_map(|view| view.messages).collect();
                paginate_items(
                    &sort_messages_descending(&merged),
                    request.cursor,
                    request.limit,
                )
            }
        };
        Some(AggregatedAiSessionDetail {
            session_id: request.session_id.to_string(),
            provider: AiSessionProvider::Claude,
            title: self.title,
            matched_path: self.matched_path,
            source_ref: self.source_ref,
            messages: page.items,
            next_cursor: page.next_cursor,
        })
    }
}

fn load_event_log(
    context: &AiSessionReaderContext,
    file_path: &str,
    should_append: bool,
) -> AiResult<ClaudeEventLog> {
    if should_append {
        let appended = cached_or_parse(ParseCacheKind::Full, &context.host, file_path, || {
            read_appended_json_lines(file_path)
        })?;
        return Ok(ClaudeEventLog::Appended(appended.as_ref().clone()));
    }
    Ok(ClaudeEventLog::Whole(read_all_events(context, file_path)?))
}

fn read_all_events(context: &AiSessionReaderContext, file_path: &str) -> AiResult<Arc<Vec<Value>>> {
    cached_or_parse(ParseCacheKind::Full, &context.host, file_path, || {
        read_json_lines(&context.host, file_path)
    })
}

/// 목록에서 한 대화를 모으는 중간 상태
struct ClaudeSessionAccumulator {
    session: AggregatedAiSession,
    has_query_match: bool,
}

/// 이벤트 목록에서 처음 나온 대화 하나를 목록 항목으로 만든다. 검색어가 있으면 맞는 대화만 남긴다
fn summarize_first_session(
    events: &[Value],
    context: &AiSessionReaderContext,
    source_ref: &str,
) -> Option<AggregatedAiSession> {
    let mut sessions: Vec<ClaudeSessionAccumulator> = Vec::new();
    for event in events {
        consume_claude_list_event(&mut sessions, event, context, source_ref);
    }
    let first = sessions.into_iter().next()?;
    let has_query = context
        .query
        .as_deref()
        .is_some_and(|query| !query.is_empty());
    (!has_query || first.has_query_match).then_some(first.session)
}

/// 제목은 파일 앞, 실제 최근 대화 시각은 파일 끝에서 읽어 긴 JSONL 전체 전송을 피한다
fn parse_claude_session_from_file(
    file_path: &str,
    context: &AiSessionReaderContext,
) -> AiResult<Option<AggregatedAiSession>> {
    if context
        .query
        .as_deref()
        .is_some_and(|query| !query.is_empty())
    {
        let all_events = read_all_events(context, file_path)?;
        return Ok(summarize_first_session(&all_events, context, file_path));
    }

    let head_events = cached_or_parse(ParseCacheKind::Head, &context.host, file_path, || {
        read_json_lines_head(&context.host, file_path, CLAUDE_SUMMARY_EVENT_LIMIT)
    })?;
    let head_result = summarize_first_session(&head_events, context, file_path);
    if let Some(mut session) = head_result.filter(|session| session.first_user_prompt.is_some()) {
        let tail_events =
            read_json_lines_tail(&context.host, file_path, CLAUDE_SUMMARY_EVENT_LIMIT)?;
        if let Some(latest) = find_latest_claude_conversation_timestamp(&tail_events) {
            session.updated_at = Some(latest);
            return Ok(Some(session));
        }
    }
    let all_events = read_all_events(context, file_path)?;
    Ok(summarize_first_session(&all_events, context, file_path))
}

fn find_latest_claude_conversation_timestamp(events: &[Value]) -> Option<String> {
    events.iter().rev().find_map(|event| {
        if is_claude_image_companion(event) {
            return None;
        }
        let role = resolve_claude_role(event);
        if role != AiMessageRole::User && role != AiMessageRole::Assistant {
            return None;
        }
        let content = message_content(event);
        let has_content = !extract_plain_text(content).is_empty()
            || !extract_message_attachments(content, "").is_empty();
        if !has_content {
            return None;
        }
        to_iso_string(event.get("timestamp").unwrap_or(&Value::Null))
    })
}

fn find_project_files(context: &AiSessionReaderContext) -> AiResult<Vec<String>> {
    let projects_directory = claude_projects_directory(context)?;
    let mut files: Vec<String> = Vec::new();
    for directory_path in candidate_project_directories(context, &projects_directory) {
        let found = context
            .host
            .read_directory_files_by_suffix(&directory_path, ".jsonl")?;
        push_unique(&mut files, found);
    }
    if !files.is_empty() {
        return Ok(files);
    }
    // Claude Code는 이벤트마다 실제 cwd를 적지만, 바깥 `projects/<encoded-path>` 디렉터리 이름은 심볼릭 링크나 이전,
    // 다른 CLI 버전의 인코딩 때문에 어긋날 수 있다. 바로 찾지 못하면 프로젝트 JSONL 전체를 훑고 cwd로 판정한다
    let mut all_files = Vec::new();
    push_unique(
        &mut all_files,
        context
            .host
            .list_files_recursively_by_suffix(&projects_directory, ".jsonl")?,
    );
    Ok(all_files)
}

fn push_unique(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !target.contains(&value) {
            target.push(value);
        }
    }
}

/// worktree에 해당하는 `projects/<encoded-path>` 디렉터리들. 같은 이름은 한 번만 둔다
fn candidate_project_directories(
    context: &AiSessionReaderContext,
    projects_directory: &str,
) -> Vec<String> {
    let mut directories = Vec::new();
    let candidates = get_candidate_paths(context)
        .iter()
        .map(|candidate| {
            node_path::join(&[projects_directory, &to_claude_project_dir_name(candidate)])
        })
        .collect();
    push_unique(&mut directories, candidates);
    directories
}

fn claude_root_directory(context: &AiSessionReaderContext) -> AiResult<String> {
    Ok(node_path::join(&[
        &context.host.home_directory()?,
        ".claude",
    ]))
}

fn claude_projects_directory(context: &AiSessionReaderContext) -> AiResult<String> {
    Ok(node_path::join(&[
        &claude_root_directory(context)?,
        "projects",
    ]))
}

/// Claude Code는 프로젝트 디렉터리 이름을 만들 때 경로 구분자(/)와 밑줄(_)을 모두 하이픈(-)으로 바꾼다
fn to_claude_project_dir_name(target_path: &str) -> String {
    node_path::resolve(&[target_path]).replace(['/', '_'], "-")
}

fn consume_claude_list_event(
    sessions: &mut Vec<ClaudeSessionAccumulator>,
    event: &Value,
    context: &AiSessionReaderContext,
    source_ref: &str,
) {
    if is_claude_image_companion(event) {
        return;
    }
    let session_id = string_field(event, "sessionId").filter(|id| !id.is_empty());
    let cwd = string_field(event, "cwd").filter(|cwd| !cwd.is_empty());
    let (Some(session_id), Some(cwd)) = (session_id, cwd) else {
        return;
    };
    let Some(match_scope) = determine_match_scope(Some(cwd), context) else {
        return;
    };

    let role = resolve_claude_role(event);
    let text = claude_message_text(event);
    let prompt = list_prompt(event, &text, source_ref);
    let timestamp = event.get("timestamp").unwrap_or(&Value::Null);
    let accumulator = claude_session_entry(
        sessions,
        session_id,
        cwd,
        match_scope,
        timestamp,
        source_ref,
    );
    let session = &mut accumulator.session;
    apply_list_event(session, role, prompt, timestamp);
    let values = [
        Some(text.as_str()),
        session.title.as_deref(),
        session.first_user_prompt.as_deref(),
        session.matched_path.as_deref(),
    ];
    if matches_claude_query(context.query.as_deref(), &values) {
        accumulator.has_query_match = true;
    }
}

/// 이벤트 하나를 목록 항목에 반영한다. 첫 사용자 글이 제목이 되고, 내용이 있는 사용자·AI 줄이 최근 시각을 정한다
fn apply_list_event(
    session: &mut AggregatedAiSession,
    role: AiMessageRole,
    prompt: String,
    timestamp: &Value,
) {
    let has_content = !prompt.is_empty();
    if has_content {
        session.message_count += 1;
    }
    let normalized_timestamp = to_iso_string(timestamp);
    if normalized_timestamp.is_some()
        && has_content
        && matches!(role, AiMessageRole::User | AiMessageRole::Assistant)
    {
        session.updated_at = normalized_timestamp;
    }
    if role == AiMessageRole::User && has_content && session.first_user_prompt.is_none() {
        session.title = Some(truncate_text_to(&prompt, SESSION_TITLE_LENGTH));
        session.first_user_prompt = Some(prompt);
    }
}

/// 목록에 보일 사용자 글. 글이 없으면 첨부 이름을 쉼표로 잇는다
fn list_prompt(event: &Value, text: &str, source_ref: &str) -> String {
    if !text.is_empty() {
        return text.to_string();
    }
    extract_message_attachments(message_content(event), source_ref)
        .iter()
        .map(|attachment| attachment.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn claude_session_entry<'a>(
    sessions: &'a mut Vec<ClaudeSessionAccumulator>,
    session_id: &str,
    cwd: &str,
    match_scope: AiSessionMatchScope,
    timestamp: &Value,
    source_ref: &str,
) -> &'a mut ClaudeSessionAccumulator {
    let index = match sessions
        .iter()
        .position(|entry| entry.session.id == session_id)
    {
        Some(index) => index,
        None => {
            sessions.push(ClaudeSessionAccumulator {
                session: AggregatedAiSession {
                    id: session_id.to_string(),
                    provider: AiSessionProvider::Claude,
                    started_at: to_iso_string(timestamp),
                    updated_at: to_iso_string(timestamp),
                    matched_path: Some(cwd.to_string()),
                    match_scope,
                    title: None,
                    first_user_prompt: None,
                    message_count: 0,
                    source_ref: Some(source_ref.to_string()),
                },
                has_query_match: false,
            });
            sessions.len() - 1
        }
    };
    &mut sessions[index]
}

fn matches_claude_query(query: Option<&str>, values: &[Option<&str>]) -> bool {
    let Some(query) = query.filter(|query| !query.is_empty()) else {
        return true;
    };
    let normalized_query = query.to_lowercase();
    values
        .iter()
        .flatten()
        .any(|value| value.to_lowercase().contains(&normalized_query))
}

#[cfg(test)]
mod tests;
