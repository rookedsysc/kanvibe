use serde_json::json;

use super::*;
use crate::error::AiError;
use crate::test_support::{local_host, remote_file_system, remote_host, temp_home, write_file};
use crate::types::{AggregatedAiSession, AiSessionMatchScope, AiSessionSourceStatus};

fn session(
    id: &str,
    provider: AiSessionProvider,
    started_at: &str,
    updated_at: &str,
) -> AggregatedAiSession {
    AggregatedAiSession {
        id: id.to_string(),
        provider,
        started_at: Some(started_at.to_string()),
        updated_at: Some(updated_at.to_string()),
        matched_path: Some("/repo".to_string()),
        match_scope: AiSessionMatchScope::Worktree,
        title: Some(id.to_string()),
        first_user_prompt: Some(id.to_string()),
        message_count: 1,
        source_ref: None,
    }
}

fn reader(
    provider: AiSessionProvider,
    sessions: Vec<AggregatedAiSession>,
    reason: Option<&str>,
) -> AiSessionReaderResult {
    AiSessionReaderResult {
        provider,
        available: true,
        session_count: sessions.len() as u64,
        reason: reason.map(str::to_string),
        sessions,
    }
}

fn empty(provider: AiSessionProvider) -> AiResult<AiSessionReaderResult> {
    Ok(reader(provider, Vec::new(), None))
}

fn outcomes(primary: Vec<(AiSessionProvider, AiResult<AiSessionReaderResult>)>) -> ReaderOutcomes {
    ReaderOutcomes {
        primary,
        additional: Vec::new(),
        sqlite: Vec::new(),
    }
}

fn local_context(worktree: &str) -> AiSessionReaderContext {
    let mut context = AiSessionReaderContext::new(local_host("/unused-home"), Some(worktree));
    context.repo_path = Some(worktree.to_string());
    context
}

fn status(
    provider: AiSessionProvider,
    available: bool,
    count: u64,
    reason: Option<&str>,
) -> AiSessionSourceStatus {
    AiSessionSourceStatus {
        provider,
        available,
        session_count: count,
        reason: reason.map(str::to_string),
    }
}

fn ids(result: &AggregatedAiSessionsResult) -> Vec<&str> {
    result
        .sessions
        .iter()
        .map(|session| session.id.as_str())
        .collect()
}

#[test]
fn merges_readers_and_sorts_by_updated_time() {
    let claude = session(
        "claude-1",
        AiSessionProvider::Claude,
        "2026-03-10T10:00:00.000Z",
        "2026-03-10T10:10:00.000Z",
    );
    let codex = session(
        "codex-1",
        AiSessionProvider::Codex,
        "2026-03-11T10:00:00.000Z",
        "2026-03-11T10:10:00.000Z",
    );
    let result = combine_reader_outcomes(
        &local_context("/repo/worktree"),
        outcomes(vec![
            (
                AiSessionProvider::Claude,
                Ok(reader(AiSessionProvider::Claude, vec![claude], None)),
            ),
            (
                AiSessionProvider::Codex,
                Ok(reader(AiSessionProvider::Codex, vec![codex], None)),
            ),
            (
                AiSessionProvider::Opencode,
                empty(AiSessionProvider::Opencode),
            ),
            (
                AiSessionProvider::Gemini,
                Ok(reader(
                    AiSessionProvider::Gemini,
                    Vec::new(),
                    Some("No data"),
                )),
            ),
        ]),
    );
    assert_eq!(ids(&result), ["codex-1", "claude-1"]);
    assert_eq!(
        result.sources,
        [
            status(AiSessionProvider::Claude, true, 1, None),
            status(AiSessionProvider::Codex, true, 1, None),
            status(AiSessionProvider::Opencode, true, 0, None),
            status(AiSessionProvider::Gemini, true, 0, Some("No data")),
        ]
    );
}

#[test]
fn keeps_other_sources_when_remote_provider_loses_ssh() {
    let runner = remote_file_system(&[]);
    let mut context =
        AiSessionReaderContext::new(remote_host(runner, "remote-host"), Some("/repo/worktree"));
    context.repo_path = Some("/repo".to_string());
    let claude = session(
        "claude-1",
        AiSessionProvider::Claude,
        "2026-03-10T10:00:00.000Z",
        "2026-03-10T10:10:00.000Z",
    );
    let connection_reset = AiError::Command {
        message: "remote-host 원격 명령 실패: Connection reset by 100.73.171.123 port 22"
            .to_string(),
        is_connection_failure: true,
    };
    let unavailable = |provider, reason: &str| create_unavailable_result(provider, reason);
    let result = combine_reader_outcomes(
        &context,
        outcomes(vec![
            (
                AiSessionProvider::Claude,
                Ok(reader(AiSessionProvider::Claude, vec![claude], None)),
            ),
            (AiSessionProvider::Codex, Err(connection_reset)),
            (
                AiSessionProvider::Opencode,
                Ok(unavailable(
                    AiSessionProvider::Opencode,
                    "Remote OpenCode session reading is not available yet",
                )),
            ),
            (
                AiSessionProvider::Gemini,
                Ok(unavailable(
                    AiSessionProvider::Gemini,
                    "Gemini CLI directory not found",
                )),
            ),
        ]),
    );
    assert_eq!(ids(&result), ["claude-1"]);
    assert!(result.is_remote);
    assert_eq!(
        result.sources,
        [
            status(AiSessionProvider::Claude, true, 1, None),
            status(
                AiSessionProvider::Codex,
                false,
                0,
                Some("SSH connection to remote-host is unavailable")
            ),
            status(
                AiSessionProvider::Opencode,
                false,
                0,
                Some("Remote OpenCode session reading is not available yet")
            ),
            status(
                AiSessionProvider::Gemini,
                false,
                0,
                Some("Gemini CLI directory not found")
            ),
        ]
    );
}

#[test]
fn keeps_reader_filtered_sessions_matched_by_body_text() {
    let mut context = local_context("/repo");
    context.query = Some("database migration".to_string());
    let body_match = session(
        "claude-body-match",
        AiSessionProvider::Claude,
        "2026-03-10T10:00:00.000Z",
        "2026-03-10T10:10:00.000Z",
    );
    let result = combine_reader_outcomes(
        &context,
        outcomes(vec![
            (
                AiSessionProvider::Claude,
                Ok(reader(AiSessionProvider::Claude, vec![body_match], None)),
            ),
            (AiSessionProvider::Codex, empty(AiSessionProvider::Codex)),
            (
                AiSessionProvider::Opencode,
                empty(AiSessionProvider::Opencode),
            ),
            (AiSessionProvider::Gemini, empty(AiSessionProvider::Gemini)),
        ]),
    );
    assert_eq!(ids(&result), ["claude-body-match"]);
}

#[test]
fn paginates_after_sorting_newest_first() {
    let read_page = |cursor: Option<String>| {
        let mut context = local_context("/repo");
        context.cursor = cursor;
        context.limit = Some(2);
        let claude = vec![
            session(
                "oldest",
                AiSessionProvider::Claude,
                "2026-03-10T10:00:00.000Z",
                "2026-03-10T10:00:00.000Z",
            ),
            session(
                "newest",
                AiSessionProvider::Claude,
                "2026-03-12T10:00:00.000Z",
                "2026-03-12T10:00:00.000Z",
            ),
        ];
        let codex = vec![session(
            "middle",
            AiSessionProvider::Codex,
            "2026-03-11T10:00:00.000Z",
            "2026-03-11T10:00:00.000Z",
        )];
        combine_reader_outcomes(
            &context,
            outcomes(vec![
                (
                    AiSessionProvider::Claude,
                    Ok(reader(AiSessionProvider::Claude, claude, None)),
                ),
                (
                    AiSessionProvider::Codex,
                    Ok(reader(AiSessionProvider::Codex, codex, None)),
                ),
                (
                    AiSessionProvider::Opencode,
                    empty(AiSessionProvider::Opencode),
                ),
                (AiSessionProvider::Gemini, empty(AiSessionProvider::Gemini)),
            ]),
        )
    };
    let first_page = read_page(None);
    assert_eq!(ids(&first_page), ["newest", "middle"]);
    assert_eq!(first_page.next_cursor.as_deref(), Some("2"));
    let second_page = read_page(first_page.next_cursor.clone());
    assert_eq!(ids(&second_page), ["oldest"]);
    assert_eq!(second_page.next_cursor, None);
}

#[test]
fn isolates_a_damaged_local_provider() {
    let result = combine_reader_outcomes(
        &local_context("/repo"),
        outcomes(vec![
            (
                AiSessionProvider::Claude,
                Err(AiError::message("broken transcript")),
            ),
            (AiSessionProvider::Codex, empty(AiSessionProvider::Codex)),
            (
                AiSessionProvider::Opencode,
                empty(AiSessionProvider::Opencode),
            ),
            (AiSessionProvider::Gemini, empty(AiSessionProvider::Gemini)),
        ]),
    );
    let claude = result
        .sources
        .iter()
        .find(|source| source.provider == AiSessionProvider::Claude)
        .unwrap();
    assert!(!claude.available);
    assert_eq!(
        claude.reason.as_deref(),
        Some("Session storage could not be read")
    );
    assert_eq!(
        result
            .sources
            .iter()
            .filter(|source| source.available)
            .count(),
        3
    );
}

#[test]
fn reports_all_providers_and_opens_an_added_provider() {
    let (_directory, home) = temp_home();
    write_file(
        &format!("{home}/.hermes/sessions/session_fixture.json"),
        &crate::javascript::stringify(&json!({
            "session_id": "hermes-integration",
            "cwd": "/worktree/integration",
            "messages": [{"role": "user", "content": "Question"}, {"role": "assistant", "content": "Answer"}]
        })),
    );
    let context = |worktree: &str| {
        let mut context = AiSessionReaderContext::new(local_host(&home), Some(worktree));
        context.repo_path = Some(worktree.to_string());
        context
    };
    let result = aggregate_ai_sessions(&context("/worktree/integration")).unwrap();
    let providers: std::collections::BTreeSet<_> = result
        .sources
        .iter()
        .map(|source| source.provider)
        .collect();
    assert_eq!(providers, AiSessionProvider::ALL.into_iter().collect());
    assert_eq!(result.sources.len(), 25);
    assert_eq!(ids(&result), ["hermes-integration"]);
    let source_ref = result.sessions[0].source_ref.as_deref();
    let detail = get_ai_session_detail(
        &context("/worktree/integration"),
        AiSessionProvider::Hermes,
        "hermes-integration",
        source_ref,
        None,
        None,
    )
    .unwrap()
    .unwrap();
    let texts: Vec<_> = detail
        .messages
        .iter()
        .map(|message| message.full_text.as_str())
        .collect();
    assert_eq!(texts, ["Answer", "Question"]);
    let foreign = get_ai_session_detail(
        &context("/foreign"),
        AiSessionProvider::Hermes,
        "hermes-integration",
        source_ref,
        None,
        None,
    )
    .unwrap();
    assert!(foreign.is_none());
}
