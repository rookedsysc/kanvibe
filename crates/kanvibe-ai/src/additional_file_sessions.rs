//! 파일 기반 provider 19개 원천(qoder, codebuddy, hermes, pi, omp, prime-agent, cursor, antigravity, rovo, copilot,
//! grok, openclaw, devin, droid, cline, kimi, muse, jcode, kiro)의 목록과 상세.
//!
//! provider 수와 파일 작업 수를 따로 제한해 저장소가 늘어도 디스크·SSH 요청이 폭증하지 않게 하고,
//! 손상된 파일 하나나 잠긴 곁 파일이 다른 기록까지 가리지 않게 한다.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};

use crate::additional_file_session_formats::{
    AdditionalFileSession, decode_additional_file_session, field, session_string,
};
use crate::error::{AiError, AiResult};
use crate::javascript::{is_truthy, js_trim, parse_json};
use crate::jsonl::read_json_lines_head;
use crate::message_text::{split_lines, truncate_text, truncate_text_to};
use crate::parse_cache::{ParseCacheKind, cached_or_parse};
use crate::providers::AiSessionProvider;
use crate::session_listing::{determine_match_scope, map_with_concurrency, paginate_items};
use crate::types::{
    AggregatedAiMessage, AggregatedAiSession, AggregatedAiSessionDetail, AiMessageRole,
    AiSessionMatchScope, AiSessionReaderContext, AiSessionReaderResult,
};

mod sidecars;
mod sources;

use sources::{FileSource, file_sources, is_within};

const PROVIDER_CONCURRENCY: usize = 3;
const FILE_CONCURRENCY: usize = 4;
const DEFAULT_DETAIL_LIMIT: usize = 20;
const SESSION_METADATA_HEAD_LINES: usize = 1;
const SESSION_TITLE_MAX_LENGTH: usize = 80;

/// 머리 한 줄만으로 작업 경로를 알 수 있어, 다른 worktree 기록은 본문을 읽기 전에 거르는 provider
const HEADER_SCOPED_PROVIDERS: [AiSessionProvider; 9] = [
    AiSessionProvider::Qoder,
    AiSessionProvider::Codebuddy,
    AiSessionProvider::Pi,
    AiSessionProvider::Omp,
    AiSessionProvider::PrimeAgent,
    AiSessionProvider::Openclaw,
    AiSessionProvider::Copilot,
    AiSessionProvider::Droid,
    AiSessionProvider::Muse,
];

/// Devin 세션 ID별 색인 행
pub(crate) type DevinIndex = Arc<HashMap<String, Value>>;

/// 한 번의 조회 안에서 같은 파일·색인을 다시 읽지 않도록 결과를 나눠 쓴다
pub(crate) struct ReadScope<'a> {
    home: String,
    context: &'a AiSessionReaderContext,
    files: Mutex<HashMap<String, AiResult<Arc<ParsedSessionFile>>>>,
    devin_indexes: Mutex<HashMap<String, AiResult<DevinIndex>>>,
    cursor_metadata_files: Mutex<Option<AiResult<Arc<Vec<String>>>>>,
}

impl<'a> ReadScope<'a> {
    fn new(home: String, context: &'a AiSessionReaderContext) -> Self {
        Self {
            home,
            context,
            files: Mutex::new(HashMap::new()),
            devin_indexes: Mutex::new(HashMap::new()),
            cursor_metadata_files: Mutex::new(None),
        }
    }

    fn sources(&self) -> Vec<FileSource> {
        file_sources(&self.home, &self.context.host)
    }

    /// 로컬 기록 파일은 링크일 수 없다
    fn read_regular_file(&self, file: &str) -> AiResult<Arc<ParsedSessionFile>> {
        if !self.context.host.is_remote() {
            let metadata =
                std::fs::symlink_metadata(file).map_err(|error| AiError::from_io(&error, file))?;
            if metadata.file_type().is_symlink() {
                return Err(AiError::message("Session file cannot be a symbolic link"));
            }
        }
        cached_or_parse(ParseCacheKind::Full, &self.context.host, file, || {
            parse_session_file(file, &self.context.host.read_text_file(file)?)
                .map(ParsedSessionFile)
        })
    }

    /// 파일 하나의 해석 결과. 비어 있으면 `null`이고, JSONL은 해석되는 줄의 배열이다
    pub(crate) fn read_parsed_file(&self, file: &str) -> AiResult<Arc<ParsedSessionFile>> {
        if let Some(known) = self.files.lock().expect("파일 결과").get(file) {
            return known.clone();
        }
        let result = self.read_regular_file(file);
        self.files
            .lock()
            .expect("파일 결과")
            .insert(file.to_string(), result.clone());
        result
    }

    /// 있으면 읽고, 없으면 `null`
    pub(crate) fn optional_file(&self, file: &str) -> AiResult<Arc<ParsedSessionFile>> {
        if self.context.host.path_exists(file)? {
            return self.read_parsed_file(file);
        }
        Ok(Arc::new(ParsedSessionFile(Value::Null)))
    }
}

/// 해석한 기록 파일. 파싱 캐시에 다른 리더의 결과와 섞이지 않도록 따로 감싼다
pub(crate) struct ParsedSessionFile(pub(crate) Value);

fn parse_session_file(file: &str, content: &str) -> AiResult<Value> {
    if js_trim(content).is_empty() {
        return Ok(Value::Null);
    }
    if file.ends_with(".jsonl") {
        let lines = split_lines(content)
            .filter_map(parse_json)
            .filter(|value| !value.is_null())
            .collect();
        return Ok(Value::Array(lines));
    }
    serde_json::from_str(content).map_err(|error| AiError::message(error.to_string()))
}

fn allowed_source(file: &str, source: &FileSource, context: &AiSessionReaderContext) -> bool {
    if !file.ends_with(source.suffix) || source.accepts.is_some_and(|accepts| !accepts(file)) {
        return false;
    }
    source
        .roots
        .iter()
        .filter(|root| is_within(file, root))
        .any(|root| {
            if context.host.is_remote() {
                return true;
            }
            // 삭제 중인 파일은 다음 목록 조회에서 다시 발견하므로 상세 후보에서 뺀다
            match (std::fs::canonicalize(file), std::fs::canonicalize(root)) {
                (Ok(real_file), Ok(real_root)) => {
                    is_within(&real_file.to_string_lossy(), &real_root.to_string_lossy())
                }
                _ => false,
            }
        })
}

fn discover_source(source: &FileSource, context: &AiSessionReaderContext) -> AiResult<Vec<String>> {
    let mut unique: Vec<String> = Vec::new();
    for root in &source.roots {
        for file in context
            .host
            .list_files_recursively_by_suffix(root, source.suffix)?
        {
            if !unique.contains(&file) {
                unique.push(file);
            }
        }
    }
    unique.retain(|file| source.accepts.is_none_or(|accepts| accepts(file)));
    if source.provider != AiSessionProvider::Antigravity {
        return Ok(unique);
    }
    // 전체 기록이 있으면 짧은 미리보기 파일을 중복 세션으로 올리지 않는다
    let full_transcripts: Vec<String> = unique.clone();
    Ok(unique
        .into_iter()
        .filter(|file| {
            let Some(stem) = file.strip_suffix("transcript.jsonl") else {
                return true;
            };
            !file.ends_with("/transcript.jsonl")
                || !full_transcripts.contains(&format!("{stem}transcript_full.jsonl"))
        })
        .collect())
}

/// 19개 원천의 목록. 한 원천의 실패는 그 원천 결과에만 적고 나머지는 계속 읽는다
pub fn read_additional_file_sessions(
    context: &AiSessionReaderContext,
) -> AiResult<Vec<AiSessionReaderResult>> {
    let home = context.host.home_directory()?;
    let scope = ReadScope::new(home, context);
    let sources = scope.sources();
    Ok(map_with_concurrency(
        &sources,
        PROVIDER_CONCURRENCY,
        |source, _| {
            read_source_sessions(&scope, source).unwrap_or_else(|_| {
                unavailable_result(source.provider, "Session storage could not be read")
            })
        },
    ))
}

fn unavailable_result(provider: AiSessionProvider, reason: &str) -> AiSessionReaderResult {
    AiSessionReaderResult {
        provider,
        available: false,
        session_count: 0,
        reason: Some(reason.to_string()),
        sessions: Vec::new(),
    }
}

fn read_source_sessions(
    scope: &ReadScope<'_>,
    source: &FileSource,
) -> AiResult<AiSessionReaderResult> {
    let context = scope.context;
    let mut is_available = false;
    for root in &source.roots {
        is_available |= context.host.path_exists(root)?;
    }
    if !is_available {
        return Ok(unavailable_result(
            source.provider,
            "Session directory not found",
        ));
    }
    let files = discover_source(source, context)?;
    let broken_files = AtomicUsize::new(0);
    let parsed = map_with_concurrency(&files, FILE_CONCURRENCY, |file, _| {
        if !allowed_source(file, source, context) {
            return None;
        }
        read_session(scope, source.provider, file).unwrap_or_else(|_| {
            broken_files.fetch_add(1, AtomicOrdering::SeqCst);
            None
        })
    });
    let sessions: Vec<AggregatedAiSession> = parsed
        .into_iter()
        .flatten()
        .map(|(session, file)| to_summary(session, source.provider, file))
        .collect();
    let broken_count = broken_files.load(AtomicOrdering::SeqCst);
    Ok(AiSessionReaderResult {
        provider: source.provider,
        available: true,
        session_count: sessions.len() as u64,
        reason: (broken_count > 0)
            .then(|| format!("{broken_count} session files could not be read")),
        sessions,
    })
}

/// 대화 하나. 메시지 검색 결과가 비어도 세션 식별과 작업 경로 검증은 유지한다
pub fn read_additional_file_session_detail(
    context: &AiSessionReaderContext,
    provider: AiSessionProvider,
    session_id: &str,
    source_ref: Option<&str>,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> AiResult<Option<AggregatedAiSessionDetail>> {
    let home = context.host.home_directory()?;
    let Some(source) = file_sources(&home, &context.host)
        .into_iter()
        .find(|source| source.provider == provider)
    else {
        return Ok(None);
    };
    let source_ref = source_ref.filter(|reference| !reference.is_empty());
    if source_ref.is_some_and(|reference| !allowed_source(reference, &source, context)) {
        return Ok(None);
    }
    let files = match source_ref {
        Some(reference) => vec![reference.to_string()],
        None => discover_source(&source, context)?,
    };
    let mut unfiltered_context = context.clone();
    unfiltered_context.query = None;
    let scope = ReadScope::new(home, &unfiltered_context);
    for file in files {
        if !allowed_source(&file, &source, context) {
            continue;
        }
        // 한 후보의 부분 기록으로 상세 조회 전체를 실패시키지 않고 다음 후보를 본다
        let Ok(Some((session, file))) = read_session(&scope, provider, &file) else {
            continue;
        };
        if session.id != session_id {
            continue;
        }
        return Ok(Some(to_detail(
            session, provider, file, context, cursor, limit,
        )));
    }
    Ok(None)
}

fn to_detail(
    session: AdditionalFileSession,
    provider: AiSessionProvider,
    file: String,
    context: &AiSessionReaderContext,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> AggregatedAiSessionDetail {
    let query = context
        .query
        .as_deref()
        .filter(|query| !query.is_empty())
        .map(str::to_lowercase);
    let messages: Vec<AggregatedAiMessage> = session
        .messages
        .iter()
        .rev()
        .filter(|message| {
            context
                .roles
                .as_ref()
                .is_none_or(|roles| roles.is_empty() || roles.contains(&message.role))
        })
        .filter(|message| {
            query
                .as_deref()
                .is_none_or(|query| message.full_text.to_lowercase().contains(query))
        })
        .cloned()
        .collect();
    // 일부 Agent는 assistant·tool 시각을 저장하지 않으므로 로그의 기록 순서를 지킨다
    let page = paginate_items(&messages, cursor, limit.unwrap_or(DEFAULT_DETAIL_LIMIT));
    let first_user_text = session
        .messages
        .iter()
        .find(|message| message.role == AiMessageRole::User)
        .map(|message| message.text.clone());
    AggregatedAiSessionDetail {
        session_id: session.id,
        provider,
        title: session.title.or(first_user_text),
        matched_path: session.cwd,
        source_ref: Some(file),
        messages: page.items,
        next_cursor: page.next_cursor,
    }
}

fn match_scope(cwd: Option<&str>, scope: &ReadScope<'_>) -> Option<AiSessionMatchScope> {
    determine_match_scope(cwd, scope.context)
}

/// 머리줄이나 곁 메타데이터로 이미 다른 worktree임을 알면 본문을 읽지 않는다
fn is_excluded_before_body(
    scope: &ReadScope<'_>,
    provider: AiSessionProvider,
    file: &str,
) -> AiResult<bool> {
    let host = &scope.context.host;
    if HEADER_SCOPED_PROVIDERS.contains(&provider) {
        let head = cached_or_parse(ParseCacheKind::Head, host, file, || {
            read_json_lines_head(host, file, SESSION_METADATA_HEAD_LINES)
        })?;
        let cwd = decode_additional_file_session(
            provider,
            file,
            &Value::Array(head.as_ref().clone()),
            &Map::new(),
        )
        .cwd;
        if cwd.is_some() && match_scope(cwd.as_deref(), scope).is_none() {
            return Ok(true);
        }
    }
    if provider == AiSessionProvider::Cursor {
        let metadata = sidecars::read_cursor_metadata(scope, file)?;
        let cwd = session_string(field(&metadata, "cwd"));
        if cwd.is_some() && match_scope(cwd.as_deref(), scope).is_none() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn read_session(
    scope: &ReadScope<'_>,
    provider: AiSessionProvider,
    file: &str,
) -> AiResult<Option<(AdditionalFileSession, String)>> {
    if is_excluded_before_body(scope, provider, file)? {
        return Ok(None);
    }
    let parsed = scope.read_parsed_file(file)?;
    let raw = &parsed.0;
    if !is_truthy(raw) {
        return Ok(None);
    }
    let is_graph_provider = matches!(provider, AiSessionProvider::Pi | AiSessionProvider::Omp);
    if is_graph_provider && !sources::matches_graph_writer(provider, file, raw, scope) {
        return Ok(None);
    }
    let sidecar = sidecars::read_sidecar(scope, provider, file, raw)?;
    if provider == AiSessionProvider::Devin && is_truthy(field(&sidecar, "hidden")) {
        return Ok(None);
    }
    let session = decode_additional_file_session(provider, file, raw, &sidecar);
    let is_visible = match_scope(session.cwd.as_deref(), scope).is_some()
        && matches_listing_query(&session, scope.context.query.as_deref());
    Ok(is_visible.then(|| (session, file.to_string())))
}

/// 검색어가 세션 ID·제목·작업 경로·메시지 전문 중 하나에 들어 있는지
fn matches_listing_query(session: &AdditionalFileSession, query: Option<&str>) -> bool {
    let Some(query) = query.filter(|query| !query.is_empty()) else {
        return true;
    };
    let query = query.to_lowercase();
    let fields = [
        Some(session.id.as_str()),
        session.title.as_deref(),
        session.cwd.as_deref(),
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
        .any(|text| text.to_lowercase().contains(&query))
}

fn to_summary(
    session: AdditionalFileSession,
    provider: AiSessionProvider,
    file: String,
) -> AggregatedAiSession {
    let first_user_prompt = session
        .messages
        .iter()
        .find(|message| message.role == AiMessageRole::User)
        .map(|message| message.full_text.clone())
        .filter(|prompt| !prompt.is_empty());
    let title = session
        .title
        .as_deref()
        .or(first_user_prompt.as_deref())
        .map(|title| truncate_text_to(title, SESSION_TITLE_MAX_LENGTH));
    AggregatedAiSession {
        id: session.id,
        provider,
        started_at: session.started_at,
        updated_at: session.updated_at,
        matched_path: session.cwd,
        match_scope: AiSessionMatchScope::Worktree,
        title,
        first_user_prompt: first_user_prompt.map(|prompt| truncate_text(&prompt)),
        message_count: session.messages.len() as u64,
        source_ref: Some(file),
    }
}

#[cfg(test)]
mod tests;
