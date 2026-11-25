use kanvibe_ai::types::{AiSessionMatchScope, AiSessionSourceStatus};

use super::*;

fn session(
    id: &str,
    provider: AiSessionProvider,
    title: Option<&str>,
    prompt: Option<&str>,
) -> AggregatedAiSession {
    AggregatedAiSession {
        id: id.into(),
        provider,
        started_at: None,
        updated_at: Some("2026-01-01T00:03:00.000Z".into()),
        matched_path: Some("/repo".into()),
        match_scope: AiSessionMatchScope::Worktree,
        title: title.map(str::to_string),
        first_user_prompt: prompt.map(str::to_string),
        message_count: 2,
        source_ref: Some(format!("{id}.jsonl")),
    }
}

fn page(
    sessions: Vec<AggregatedAiSession>,
    next_cursor: Option<&str>,
) -> AggregatedAiSessionsResult {
    AggregatedAiSessionsResult {
        sessions,
        next_cursor: next_cursor.map(str::to_string),
        ..AggregatedAiSessionsResult::default()
    }
}

fn detail(
    session_id: &str,
    texts: &[(AiMessageRole, &str, &str)],
    next_cursor: Option<&str>,
) -> AggregatedAiSessionDetail {
    AggregatedAiSessionDetail {
        session_id: session_id.into(),
        provider: AiSessionProvider::Claude,
        title: None,
        matched_path: None,
        source_ref: None,
        messages: texts
            .iter()
            .map(|(role, timestamp, text)| AggregatedAiMessage::plain(*role, Some(timestamp), text))
            .collect(),
        next_cursor: next_cursor.map(str::to_string),
    }
}

fn titles(state: &ChatHistoryState) -> Vec<String> {
    state
        .filtered_sessions()
        .into_iter()
        .map(session_title)
        .collect()
}

/// 첫 쪽을 받은 상태
fn loaded(sessions: Vec<AggregatedAiSession>, next_cursor: Option<&str>) -> ChatHistoryState {
    let mut state = ChatHistoryState::default();
    let request = state.begin_history_load("");
    state.finish_history_load(&request, Ok(page(sessions, next_cursor)));
    state
}

fn select_first(state: &mut ChatHistoryState) -> DetailRequest {
    let key = session_key(state.filtered_sessions()[0]);
    state.select_session(key).expect("detail request")
}

fn message_texts(state: &ChatHistoryState) -> Vec<String> {
    transcript_rows(state.messages(), state.selected_key.as_deref())
        .into_iter()
        .map(|row| row.message.text.clone())
        .collect()
}

// parity: TS-renderer-TaskDetailRoute-2006cd
#[test]
fn chat_view_reads_nothing_until_opened_and_then_asks_for_the_newest_first_page() {
    let mut state = ChatHistoryState::default();
    assert!(state.history.is_none());
    assert_eq!(state.history_count_label(), None);
    let request = state.begin_history_load("");
    assert_eq!(
        request.list,
        AiSessionListRequest {
            query: None,
            cursor: None,
            limit: Some(20)
        }
    );
    assert!(state.is_history_loading);
}

// parity: TS-renderer-TaskDetailRoute-f60797
#[test]
fn history_shows_the_first_prompt_only_when_it_differs_from_the_title() {
    let same = session(
        "a",
        AiSessionProvider::Claude,
        Some("같은 문장"),
        Some("같은 문장"),
    );
    let different = session(
        "b",
        AiSessionProvider::Claude,
        Some("요약 제목"),
        Some("첫 입력"),
    );
    let untitled = session("c", AiSessionProvider::Claude, None, Some("첫 입력만"));
    assert_eq!(session_preview(&same), None);
    assert_eq!(session_preview(&different).as_deref(), Some("첫 입력"));
    assert_eq!(session_title(&untitled), "첫 입력만");
    assert_eq!(session_preview(&untitled), None);
}

// parity: TS-renderer-TaskDetailRoute-dbf4bf
#[test]
fn session_list_appends_the_next_page_after_the_newest_one() {
    let mut state = loaded(
        vec![session(
            "newest",
            AiSessionProvider::Claude,
            Some("Newest chat"),
            None,
        )],
        Some("20"),
    );
    let more = state.begin_more_sessions().expect("next page");
    assert_eq!(
        more.list,
        AiSessionListRequest {
            query: None,
            cursor: Some("20".into()),
            limit: Some(20)
        }
    );
    state.finish_history_load(
        &more,
        Ok(page(
            vec![session(
                "older",
                AiSessionProvider::Codex,
                Some("Older chat"),
                None,
            )],
            None,
        )),
    );
    assert_eq!(titles(&state), ["Newest chat", "Older chat"]);
    assert!(state.begin_more_sessions().is_none());
}

// parity: TS-renderer-TaskDetailRoute-739335
#[test]
fn provider_filter_hiding_the_whole_page_still_allows_the_next_page() {
    let mut state = loaded(
        vec![session(
            "claude",
            AiSessionProvider::Claude,
            Some("Claude newest"),
            None,
        )],
        Some("20"),
    );
    state.toggle_provider(AiSessionProvider::Codex);
    assert!(titles(&state).is_empty());
    assert_eq!(state.history_count_label().as_deref(), Some("0/1"));
    let more = state
        .begin_more_sessions()
        .expect("next page stays available");
    state.finish_history_load(
        &more,
        Ok(page(
            vec![session(
                "codex",
                AiSessionProvider::Codex,
                Some("Codex older"),
                None,
            )],
            None,
        )),
    );
    assert_eq!(titles(&state), ["Codex older"]);
}

// parity: TS-renderer-TaskDetailRoute-dcc0e8
#[test]
fn additional_agents_get_their_own_filter_count_and_conversation() {
    let mut state = ChatHistoryState::default();
    let request = state.begin_history_load("");
    let mut result = page(
        vec![session(
            "hermes-1",
            AiSessionProvider::Hermes,
            Some("Hermes answer"),
            None,
        )],
        None,
    );
    result.sources.push(AiSessionSourceStatus {
        provider: AiSessionProvider::Cursor,
        available: true,
        session_count: 0,
        reason: None,
    });
    state.finish_history_load(&request, Ok(result));
    let filters = state.provider_filters();
    assert!(filters.contains(&(AiSessionProvider::Hermes, 1)));
    assert!(filters.contains(&(AiSessionProvider::Cursor, 0)));
    assert!(
        !filters
            .iter()
            .any(|(provider, _)| *provider == AiSessionProvider::Droid)
    );
    state.toggle_provider(AiSessionProvider::Hermes);
    assert_eq!(titles(&state), ["Hermes answer"]);
}

// parity: TS-renderer-TaskDetailRoute-1369f7
#[test]
fn sessions_of_every_provider_share_one_list() {
    let state = loaded(
        vec![
            session(
                "claude-1",
                AiSessionProvider::Claude,
                Some("Claude answer"),
                None,
            ),
            session(
                "codex-1",
                AiSessionProvider::Codex,
                Some("Codex answer"),
                None,
            ),
            session(
                "opencode-1",
                AiSessionProvider::Opencode,
                Some("OpenCode answer"),
                None,
            ),
            session(
                "gemini-1",
                AiSessionProvider::Gemini,
                Some("Gemini answer"),
                None,
            ),
        ],
        None,
    );
    let providers: Vec<AiSessionProvider> = state
        .filtered_sessions()
        .iter()
        .map(|session| session.provider)
        .collect();
    assert_eq!(
        providers,
        [
            AiSessionProvider::Claude,
            AiSessionProvider::Codex,
            AiSessionProvider::Opencode,
            AiSessionProvider::Gemini
        ]
    );
    let shown: Vec<(AiSessionProvider, usize)> = state.provider_filters();
    for provider in providers {
        assert!(shown.contains(&(provider, 1)));
    }
}

// parity: TS-renderer-TaskDetailRoute-cd1318
#[test]
fn provider_filters_combine_with_or() {
    let mut state = loaded(
        vec![
            session(
                "gemini-1",
                AiSessionProvider::Gemini,
                Some("Gemini answer"),
                None,
            ),
            session(
                "claude-1",
                AiSessionProvider::Claude,
                Some("Claude answer"),
                None,
            ),
            session(
                "opencode-1",
                AiSessionProvider::Opencode,
                Some("OpenCode answer"),
                None,
            ),
        ],
        None,
    );
    state.toggle_provider(AiSessionProvider::Claude);
    assert_eq!(titles(&state), ["Claude answer"]);
    state.toggle_provider(AiSessionProvider::Gemini);
    assert_eq!(titles(&state), ["Gemini answer", "Claude answer"]);
    state.toggle_provider(AiSessionProvider::Claude);
    state.toggle_provider(AiSessionProvider::Gemini);
    assert_eq!(titles(&state).len(), 3);
}

// parity: TS-renderer-TaskDetailRoute-06254d
#[test]
fn search_reloads_history_with_the_typed_query() {
    let mut state = loaded(
        vec![session(
            "a",
            AiSessionProvider::Claude,
            Some("Initial chat"),
            None,
        )],
        None,
    );
    let search = state.begin_history_load("  filtered topic ");
    assert_eq!(search.list.query.as_deref(), Some("filtered topic"));
    state.finish_history_load(
        &search,
        Ok(page(
            vec![session(
                "b",
                AiSessionProvider::Claude,
                Some("Filtered chat"),
                None,
            )],
            None,
        )),
    );
    assert_eq!(titles(&state), ["Filtered chat"]);
    assert_eq!(state.applied_query.as_deref(), Some("filtered topic"));
}

// parity: TS-renderer-TaskDetailRoute-d09b39
#[test]
fn stale_session_page_after_a_new_search_is_dropped() {
    let mut state = loaded(
        vec![session(
            "initial",
            AiSessionProvider::Claude,
            Some("Initial chat"),
            None,
        )],
        Some("20"),
    );
    let stale_append = state.begin_more_sessions().expect("append");
    let search = state.begin_history_load("filtered topic");
    state.finish_history_load(
        &search,
        Ok(page(
            vec![session(
                "filtered",
                AiSessionProvider::Claude,
                Some("Filtered chat"),
                None,
            )],
            None,
        )),
    );
    state.finish_history_load(
        &stale_append,
        Ok(page(
            vec![session(
                "stale",
                AiSessionProvider::Claude,
                Some("Stale chat"),
                None,
            )],
            None,
        )),
    );
    assert_eq!(titles(&state), ["Filtered chat"]);
    assert!(!state.is_history_loading);
}

// parity: TS-renderer-TaskDetailRoute-249203
#[test]
fn detail_request_carries_the_chosen_role_filters() {
    let mut state = loaded(
        vec![session(
            "claude-1",
            AiSessionProvider::Claude,
            Some("Claude answer"),
            None,
        )],
        None,
    );
    let first = select_first(&mut state);
    assert_eq!(first.query.roles, None);
    assert_eq!(first.query.limit, 40);
    let system = state.toggle_role(AiMessageRole::System).expect("reload");
    assert_eq!(system.query.roles, Some(vec![AiMessageRole::System]));
    let both = state.toggle_role(AiMessageRole::Developer).expect("reload");
    assert_eq!(
        both.query.roles,
        Some(vec![AiMessageRole::System, AiMessageRole::Developer])
    );
    assert_eq!(
        ROLE_FILTERS[..3],
        [
            AiMessageRole::User,
            AiMessageRole::Assistant,
            AiMessageRole::System
        ]
    );
}

// parity: TS-renderer-TaskDetailRoute-9d503e
#[test]
fn detail_shows_the_newest_page_and_appends_older_messages() {
    let mut state = loaded(
        vec![session(
            "claude-1",
            AiSessionProvider::Claude,
            Some("chat"),
            None,
        )],
        None,
    );
    let first = select_first(&mut state);
    state.finish_detail_load(
        &first,
        Ok(Some(detail(
            "claude-1",
            &[(
                AiMessageRole::Assistant,
                "2026-01-01T00:04:00.000Z",
                "Newest answer",
            )],
            Some("40"),
        ))),
    );
    assert!(state.has_older_messages());
    let older = state.begin_older_messages().expect("older page");
    assert_eq!(older.query.cursor.as_deref(), Some("40"));
    assert!(
        state.begin_older_messages().is_none(),
        "한 번에 한 쪽만 읽는다"
    );
    state.finish_detail_load(
        &older,
        Ok(Some(detail(
            "claude-1",
            &[(
                AiMessageRole::User,
                "2026-01-01T00:01:00.000Z",
                "Older prompt",
            )],
            None,
        ))),
    );
    assert_eq!(message_texts(&state), ["Older prompt", "Newest answer"]);
    assert!(!state.has_older_messages());
}

// parity: TS-renderer-TaskDetailRoute-ab48b7
#[test]
fn stale_older_page_after_a_role_change_is_dropped() {
    let mut state = loaded(
        vec![session(
            "claude-1",
            AiSessionProvider::Claude,
            Some("chat"),
            None,
        )],
        None,
    );
    let first = select_first(&mut state);
    state.finish_detail_load(
        &first,
        Ok(Some(detail(
            "claude-1",
            &[(
                AiMessageRole::Assistant,
                "2026-01-01T00:04:00.000Z",
                "Current assistant answer",
            )],
            Some("40"),
        ))),
    );
    let stale_older = state.begin_older_messages().expect("older");
    let filtered = state.toggle_role(AiMessageRole::User).expect("filtered");
    state.finish_detail_load(
        &filtered,
        Ok(Some(detail(
            "claude-1",
            &[(
                AiMessageRole::User,
                "2026-01-01T00:01:00.000Z",
                "Filtered user prompt",
            )],
            None,
        ))),
    );
    state.finish_detail_load(
        &stale_older,
        Ok(Some(detail(
            "claude-1",
            &[(
                AiMessageRole::Assistant,
                "2026-01-01T00:00:00.000Z",
                "Stale older answer",
            )],
            None,
        ))),
    );
    assert_eq!(message_texts(&state), ["Filtered user prompt"]);
}

// parity: TS-renderer-TaskDetailRoute-ecb9e8
#[test]
fn detail_messages_read_in_time_order() {
    let mut state = loaded(
        vec![session(
            "claude-1",
            AiSessionProvider::Claude,
            Some("chat"),
            None,
        )],
        None,
    );
    let first = select_first(&mut state);
    assert!(state.is_detail_loading());
    state.finish_detail_load(
        &first,
        Ok(Some(detail(
            "claude-1",
            &[
                (
                    AiMessageRole::Assistant,
                    "2026-01-01T00:03:00.000Z",
                    "AI reply",
                ),
                (
                    AiMessageRole::System,
                    "2026-01-01T00:02:00.000Z",
                    "System prompt",
                ),
                (
                    AiMessageRole::User,
                    "2026-01-01T00:01:00.000Z",
                    "User prompt",
                ),
            ],
            None,
        ))),
    );
    assert!(!state.is_detail_loading());
    assert_eq!(
        message_texts(&state),
        ["User prompt", "System prompt", "AI reply"]
    );
}

// parity: TS-renderer-TaskDetailRoute-97e340
#[test]
fn going_back_to_the_history_keeps_the_list_and_allows_reselecting() {
    let mut state = loaded(
        vec![
            session(
                "claude-1",
                AiSessionProvider::Claude,
                Some("First chat"),
                None,
            ),
            session(
                "codex-1",
                AiSessionProvider::Codex,
                Some("Second chat"),
                None,
            ),
        ],
        None,
    );
    select_first(&mut state);
    state.clear_selection();
    assert!(state.selected_session().is_none());
    assert_eq!(titles(&state), ["First chat", "Second chat"]);
    let again = select_first(&mut state);
    assert_eq!(again.query.session_id, "claude-1");
}

// parity: TS-renderer-TaskDetailRoute-098157
#[test]
fn older_pages_keep_existing_row_keys_and_tell_duplicates_apart() {
    let newest = detail(
        "claude-1",
        &[
            (
                AiMessageRole::Tool,
                "2026-01-01T00:03:00.000Z",
                "same output",
            ),
            (
                AiMessageRole::Tool,
                "2026-01-01T00:03:00.000Z",
                "same output",
            ),
        ],
        Some("40"),
    );
    let before: Vec<String> = transcript_rows(&newest.messages, Some("claude:claude-1:"))
        .into_iter()
        .map(|row| row.key)
        .collect();
    assert_ne!(before[0], before[1]);
    let mut appended = newest.messages.clone();
    appended.push(AggregatedAiMessage::plain(
        AiMessageRole::User,
        Some("2026-01-01T00:01:00.000Z"),
        "older",
    ));
    let after: Vec<String> = transcript_rows(&appended, Some("claude:claude-1:"))
        .into_iter()
        .map(|row| row.key)
        .collect();
    assert_eq!(after[1..], before[..]);
}

#[test]
fn hiding_the_selected_session_with_a_provider_filter_clears_the_selection() {
    let mut state = loaded(
        vec![session(
            "claude-1",
            AiSessionProvider::Claude,
            Some("chat"),
            None,
        )],
        None,
    );
    select_first(&mut state);
    state.toggle_provider(AiSessionProvider::Codex);
    assert!(state.selected_key.is_none());
}

#[test]
fn failed_detail_is_reported_for_the_selected_session_and_retry_reloads() {
    let mut state = loaded(
        vec![session(
            "claude-1",
            AiSessionProvider::Claude,
            Some("chat"),
            None,
        )],
        None,
    );
    let first = select_first(&mut state);
    state.finish_detail_load(&first, Err(()));
    assert!(state.has_detail_error());
    assert!(!state.is_detail_loading());
    let retry = state.begin_detail_load().expect("retry");
    assert_eq!(retry.query.session_id, "claude-1");
    assert!(state.is_detail_loading());
}
