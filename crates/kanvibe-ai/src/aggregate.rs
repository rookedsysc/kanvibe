//! 모든 provider 리더를 함께 돌려 이 worktree의 세션을 모으고, 최근 순으로 정렬해 페이지로 나눈다.
//! 상세 조회는 provider에 맞는 리더로 보낸다.

use crate::additional_file_sessions::{
    read_additional_file_session_detail, read_additional_file_sessions,
};
use crate::claude_sessions::{read_claude_session_detail, read_claude_sessions};
use crate::codex_sessions::{read_codex_session_detail, read_codex_sessions};
use crate::error::AiResult;
use crate::gemini_sessions::{read_gemini_session_detail, read_gemini_sessions};
use crate::opencode_sessions::{read_opencode_session_detail, read_opencode_sessions};
use crate::providers::AiSessionProvider;
use crate::session_listing::{
    create_unavailable_result, paginate_items, sort_sessions_descending, to_source_status,
};
use crate::sqlite_agent_sessions::{read_sqlite_agent_session_detail, read_sqlite_agent_sessions};
use crate::types::{
    AggregatedAiSessionDetail, AggregatedAiSessionsResult, AiSessionReaderContext,
    AiSessionReaderResult,
};

type PrimaryReader = fn(&AiSessionReaderContext) -> AiResult<AiSessionReaderResult>;

/// 실패해도 다른 원천을 지키는 주 리더. 순서가 원천 목록 순서다
const PRIMARY_READERS: [(AiSessionProvider, PrimaryReader); 4] = [
    (AiSessionProvider::Claude, read_claude_sessions),
    (AiSessionProvider::Codex, read_codex_sessions),
    (AiSessionProvider::Opencode, read_opencode_sessions),
    (AiSessionProvider::Gemini, read_gemini_sessions),
];

/// 리더들이 돌려준 결과. 주 리더는 실패를 그대로 담고, 파일·SQLite 리더는 이미 성공한 결과만 담는다
pub(crate) struct ReaderOutcomes {
    pub primary: Vec<(AiSessionProvider, AiResult<AiSessionReaderResult>)>,
    pub additional: Vec<AiSessionReaderResult>,
    pub sqlite: Vec<AiSessionReaderResult>,
}

/// TS `aggregateAiSessions`. 파일·SQLite 리더의 실패는 TS의 `Promise.all`처럼 전체 실패로 올린다
pub fn aggregate_ai_sessions(
    context: &AiSessionReaderContext,
) -> AiResult<AggregatedAiSessionsResult> {
    let (primary, additional, sqlite) = std::thread::scope(|scope| {
        let primary_handles: Vec<_> = PRIMARY_READERS
            .iter()
            .map(|(provider, reader)| (*provider, scope.spawn(move || reader(context))))
            .collect();
        let additional = scope.spawn(|| read_additional_file_sessions(context));
        let sqlite = scope.spawn(|| read_sqlite_agent_sessions(context));
        let primary: Vec<_> = primary_handles
            .into_iter()
            .map(|(provider, handle)| (provider, handle.join().expect("세션 리더 스레드")))
            .collect();
        (
            primary,
            additional.join().expect("파일 리더 스레드"),
            sqlite.join().expect("SQLite 리더 스레드"),
        )
    });
    Ok(combine_reader_outcomes(
        context,
        ReaderOutcomes {
            primary,
            additional: additional?,
            sqlite: sqlite?,
        },
    ))
}

pub(crate) fn combine_reader_outcomes(
    context: &AiSessionReaderContext,
    outcomes: ReaderOutcomes,
) -> AggregatedAiSessionsResult {
    let readers: Vec<AiSessionReaderResult> = outcomes
        .primary
        .into_iter()
        .map(|(provider, outcome)| {
            outcome.unwrap_or_else(|error| unavailable_reader(context, provider, &error))
        })
        .chain(outcomes.additional)
        .chain(outcomes.sqlite)
        .collect();
    let mut sessions: Vec<_> = readers
        .iter()
        .flat_map(|reader| reader.sessions.iter().cloned())
        .collect();
    sort_sessions_descending(&mut sessions);
    let page = match context.limit.filter(|limit| *limit > 0) {
        Some(limit) => paginate_items(&sessions, context.cursor.as_deref(), limit),
        None => crate::session_listing::Page {
            items: sessions,
            next_cursor: None,
        },
    };
    AggregatedAiSessionsResult {
        is_remote: context.host.is_remote(),
        target_path: context.worktree_path.clone(),
        repo_path: context.repo_path.clone(),
        sessions: page.items,
        sources: readers.iter().map(to_source_status).collect(),
        next_cursor: page.next_cursor,
    }
}

/// 원격 연결이 끊긴 실패와 기록이 깨진 실패를 다른 이유로 알린다
fn unavailable_reader(
    context: &AiSessionReaderContext,
    provider: AiSessionProvider,
    error: &crate::error::AiError,
) -> AiSessionReaderResult {
    match context.ssh_host() {
        Some(ssh_host) if error.is_connection_failure() => create_unavailable_result(
            provider,
            format!("SSH connection to {ssh_host} is unavailable"),
        ),
        _ => create_unavailable_result(provider, "Session storage could not be read"),
    }
}

/// TS `getAiSessionDetail`
pub fn get_ai_session_detail(
    context: &AiSessionReaderContext,
    provider: AiSessionProvider,
    session_id: &str,
    source_ref: Option<&str>,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> AiResult<Option<AggregatedAiSessionDetail>> {
    match provider {
        AiSessionProvider::Claude => {
            read_claude_session_detail(context, session_id, source_ref, cursor, limit)
        }
        AiSessionProvider::Codex => {
            read_codex_session_detail(context, session_id, source_ref, cursor, limit)
        }
        AiSessionProvider::Opencode => {
            read_opencode_session_detail(context, session_id, source_ref, cursor, limit)
        }
        AiSessionProvider::Gemini => {
            read_gemini_session_detail(context, session_id, source_ref, cursor, limit)
        }
        AiSessionProvider::Opencode2 | AiSessionProvider::Zcode => {
            read_sqlite_agent_session_detail(
                context, provider, session_id, source_ref, cursor, limit,
            )
        }
        _ => read_additional_file_session_detail(
            context, provider, session_id, source_ref, cursor, limit,
        ),
    }
}

#[cfg(test)]
mod tests;
