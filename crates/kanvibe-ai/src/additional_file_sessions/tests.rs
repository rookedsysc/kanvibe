use rusqlite::{Connection, params};
use serde_json::json;

use super::*;
use crate::javascript::stringify;
use crate::test_support::{local_host, remote_file_system, remote_host, temp_home, write_file};

const CWD: &str = "/projects/history-fixture";
const AT: &str = "2026-11-01T10:00:00Z";
const LATER: &str = "2026-11-01T10:01:00Z";
const USER: &str = "Please explain the change";
const ANSWER: &str = "The change is complete";

fn json_lines_text(values: &Value) -> String {
    values
        .as_array()
        .unwrap()
        .iter()
        .map(stringify)
        .collect::<Vec<_>>()
        .join("\n")
}

struct HistoryHome {
    _directory: tempfile::TempDir,
    home: String,
}

impl HistoryHome {
    fn new() -> Self {
        let (directory, home) = temp_home();
        Self {
            _directory: directory,
            home,
        }
    }

    fn put(&self, relative: &str, value: &Value, is_lines: bool) -> String {
        let file = format!("{}/{relative}", self.home);
        write_file(
            &file,
            &if is_lines {
                json_lines_text(value)
            } else {
                stringify(value)
            },
        );
        file
    }

    fn context(&self) -> AiSessionReaderContext {
        let mut context = AiSessionReaderContext::new(local_host(&self.home), Some(CWD));
        context.repo_path = Some(CWD.to_string());
        context
    }
}

struct Fixture {
    provider: AiSessionProvider,
    file: &'static str,
    value: Value,
    is_lines: bool,
    sidecars: Vec<(&'static str, Value, bool)>,
}

fn fixture(
    provider: AiSessionProvider,
    file: &'static str,
    value: Value,
    is_lines: bool,
) -> Fixture {
    Fixture {
        provider,
        file,
        value,
        is_lines,
        sidecars: Vec::new(),
    }
}

fn with_sidecars(mut fixture: Fixture, sidecars: Vec<(&'static str, Value, bool)>) -> Fixture {
    fixture.sidecars = sidecars;
    fixture
}

fn graph_fixture(provider: AiSessionProvider, file: &'static str) -> Fixture {
    let value = json!([
        {"type": "session", "id": "p", "cwd": CWD, "timestamp": AT},
        {"type": "message", "message": {"role": "user", "content": USER}, "timestamp": AT},
        {"type": "message", "message": {"role": "assistant", "content": ANSWER}, "timestamp": LATER},
    ]);
    fixture(provider, file, value, true)
}

fn hermes_fixture() -> Fixture {
    let value = json!({"session_id": "h", "cwd": CWD, "session_start": AT, "messages": [
        {"role": "user", "content": USER, "timestamp": AT}, {"role": "assistant", "content": ANSWER, "timestamp": LATER}]});
    fixture(
        AiSessionProvider::Hermes,
        ".hermes/sessions/session_h.json",
        value,
        false,
    )
}

fn devin_fixture() -> Fixture {
    let value = json!({"session_id": "de", "working_directory": CWD, "steps": [
        {"source": "user", "message": USER, "timestamp": AT}, {"source": "agent", "message": {"content": ANSWER}, "timestamp": LATER}]});
    fixture(
        AiSessionProvider::Devin,
        ".local/share/devin/cli/transcripts/de.json",
        value,
        false,
    )
}

fn kimi_fixture() -> Fixture {
    with_sidecars(
        fixture(
            AiSessionProvider::Kimi,
            ".kimi-code/sessions/work/session_ki/state.json",
            json!({"title": "Kimi session", "createdAt": AT}),
            false,
        ),
        vec![
            (
                ".kimi-code/session_index.jsonl",
                json!([{"sessionId": "session_ki", "workDir": CWD}]),
                true,
            ),
            (
                ".kimi-code/sessions/work/session_ki/agents/main/wire.jsonl",
                json!([
                {"type": "context.append_message", "message": {"role": "user", "origin": {"kind": "user"}, "content": USER}},
                {"type": "context.append_loop_event", "event": {"type": "content.part", "part": {"type": "text", "text": ANSWER}}},
                {"type": "context.append_loop_event", "event": {"type": "step.end"}}]),
                true,
            ),
        ],
    )
}

fn kiro_fixture() -> Fixture {
    with_sidecars(
        fixture(
            AiSessionProvider::Kiro,
            ".kiro/sessions/cli/00000000-0000-0000-0000-000000000001.json",
            json!({"cwd": CWD, "created_at": AT}),
            false,
        ),
        vec![(
            ".kiro/sessions/cli/00000000-0000-0000-0000-000000000001.jsonl",
            json!([
            {"kind": "Prompt", "data": {"meta": {"timestamp": AT}, "content": [{"kind": "text", "data": USER}]}},
            {"kind": "AssistantMessage", "data": {"content": [{"kind": "text", "data": ANSWER}]}}]),
            true,
        )],
    )
}

fn antigravity_fixture() -> Fixture {
    with_sidecars(
        fixture(
            AiSessionProvider::Antigravity,
            ".gemini/antigravity-cli/brain/ag/.system_generated/logs/transcript.jsonl",
            json!([
            {"source": "USER_EXPLICIT", "type": "USER_INPUT", "content": format!("<USER_REQUEST>{USER}</USER_REQUEST>"), "created_at": AT},
            {"source": "MODEL", "type": "PLANNER_RESPONSE", "content": ANSWER, "created_at": LATER}]),
            true,
        ),
        vec![(
            ".gemini/antigravity-cli/history.jsonl",
            json!([{"conversationId": "ag", "workspace": CWD, "timestamp": AT}]),
            true,
        )],
    )
}

fn fixtures() -> Vec<Fixture> {
    use AiSessionProvider as Provider;
    vec![
        fixture(
            Provider::Qoder,
            ".qoder/projects/task/q.jsonl",
            json!([
            {"type": "workspace-directories", "directories": [CWD]},
            {"type": "user", "sessionId": "q", "timestamp": AT, "message": {"role": "user", "content": USER}},
            {"type": "assistant", "sessionId": "q", "timestamp": LATER, "message": {"role": "assistant", "content": ANSWER}}]),
            true,
        ),
        fixture(
            Provider::Codebuddy,
            ".codebuddy/projects/task/cb.jsonl",
            json!([
            {"type": "message", "sessionId": "cb", "cwd": CWD, "role": "user", "content": USER, "timestamp": AT},
            {"type": "message", "sessionId": "cb", "role": "assistant", "content": [{"type": "output_text", "text": ANSWER}], "timestamp": LATER}]),
            true,
        ),
        hermes_fixture(),
        graph_fixture(Provider::Pi, ".pi/agent/sessions/p.jsonl"),
        graph_fixture(Provider::Omp, ".omp/agent/sessions/p.jsonl"),
        graph_fixture(Provider::PrimeAgent, ".prime/agent/sessions/p.jsonl"),
        graph_fixture(Provider::Openclaw, ".openclaw/agents/main/sessions/p.jsonl"),
        with_sidecars(
            fixture(
                Provider::Cursor,
                ".cursor/projects/task/agent-transcripts/cu/cu.jsonl",
                json!([
                {"role": "user", "message": {"content": USER}, "timestamp": AT},
                {"role": "assistant", "message": {"content": ANSWER}, "timestamp": LATER}]),
                true,
            ),
            vec![(
                ".cursor/chats/hash/cu/meta.json",
                json!({"agentId": "cu", "cwd": CWD, "title": "Cursor session"}),
                false,
            )],
        ),
        antigravity_fixture(),
        with_sidecars(
            fixture(
                Provider::Rovo,
                ".rovodev/sessions/r/metadata.json",
                json!({"workspace_path": CWD, "created_at": AT}),
                false,
            ),
            vec![(
                ".rovodev/sessions/r/session_context.json",
                json!({"messages": [
                {"role": "user", "content": USER, "timestamp": AT}, {"role": "assistant", "content": ANSWER, "timestamp": LATER}]}),
                false,
            )],
        ),
        fixture(
            Provider::Copilot,
            ".copilot/session-state/co/events.jsonl",
            json!([
            {"type": "session.start", "data": {"sessionId": "co", "context": {"cwd": CWD}, "startTime": AT}},
            {"type": "user.message", "data": {"content": USER}, "timestamp": AT},
            {"type": "assistant.message", "data": {"content": ANSWER}, "timestamp": LATER}]),
            true,
        ),
        with_sidecars(
            fixture(
                Provider::Grok,
                ".grok/sessions/gr/summary.json",
                json!({"info": {"id": "gr", "cwd": CWD}, "created_at": AT}),
                false,
            ),
            vec![(
                ".grok/sessions/gr/chat_history.jsonl",
                json!([
                {"type": "user", "content": USER, "timestamp": AT}, {"type": "assistant", "content": ANSWER, "timestamp": LATER}]),
                true,
            )],
        ),
        devin_fixture(),
        fixture(
            Provider::Droid,
            ".factory/sessions/dr.jsonl",
            json!([
            {"type": "session_start", "id": "dr", "cwd": CWD, "timestamp": AT},
            {"type": "message", "role": "user", "content": USER, "timestamp": AT},
            {"type": "message", "role": "assistant", "message": {"content": ANSWER}, "timestamp": LATER}]),
            true,
        ),
        with_sidecars(
            fixture(
                Provider::Cline,
                ".cline/data/sessions/cl/cl.json",
                json!({"session_id": "cl", "cwd": CWD, "started_at": AT}),
                false,
            ),
            vec![(
                ".cline/data/sessions/cl/cl.messages.json",
                json!({"messages": [
                {"role": "user", "content": USER, "ts": AT}, {"role": "assistant", "content": ANSWER, "ts": LATER}]}),
                false,
            )],
        ),
        kimi_fixture(),
        fixture(
            Provider::Muse,
            ".local/share/muse/sessions/mu/session.jsonl",
            json!([
            {"payload_type": "runtime.session.metadata", "payload": {"record": {"workspace_root": CWD}}, "recorded_at": 1_793_527_200_000_000_i64},
            {"payload_type": "runtime.user_intent.accepted", "payload": {"refill_blocks": [{"text": USER}]}, "recorded_at": 1_793_527_200_000_000_i64},
            {"payload_type": "runtime.session", "payload": {"event": {"kind": "assistant_message_committed", "text": ANSWER}}, "recorded_at": 1_793_527_260_000_000_i64}]),
            true,
        ),
        fixture(
            Provider::Jcode,
            ".jcode/sessions/session_j.json",
            json!({"id": "j", "working_dir": CWD, "messages": [
            {"role": "user", "content": USER, "timestamp": AT}, {"role": "assistant", "content": ANSWER, "timestamp": LATER}]}),
            false,
        ),
        kiro_fixture(),
    ]
}

fn detail(
    context: &AiSessionReaderContext,
    provider: AiSessionProvider,
    session_id: &str,
    source_ref: Option<&str>,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Option<AggregatedAiSessionDetail> {
    read_additional_file_session_detail(context, provider, session_id, source_ref, cursor, limit)
        .unwrap()
}

fn texts(detail: &Option<AggregatedAiSessionDetail>) -> Vec<String> {
    detail
        .as_ref()
        .unwrap()
        .messages
        .iter()
        .map(|message| message.full_text.clone())
        .collect()
}

fn source_of(
    results: &[AiSessionReaderResult],
    provider: AiSessionProvider,
) -> &AiSessionReaderResult {
    results
        .iter()
        .find(|result| result.provider == provider)
        .unwrap()
}

// parity: TS-aiSessions-readAdditionalFileSessions-6b95c3
#[test]
fn keeps_existing_hermes_detail_when_no_messages_match() {
    for has_source_ref in [true, false] {
        let history = HistoryHome::new();
        let hermes = hermes_fixture();
        let file = history.put(hermes.file, &hermes.value, false);
        let context = history.context();
        let source_ref = has_source_ref.then_some(file.as_str());
        let existing = detail(
            &context,
            AiSessionProvider::Hermes,
            "h",
            source_ref,
            None,
            None,
        );
        assert_eq!(texts(&existing), [ANSWER, USER]);
        let mut empty_search = history.context();
        empty_search.query = Some("zqx-no-message-match".to_string());
        empty_search.roles = Some(vec![AiMessageRole::Assistant]);
        let empty = detail(
            &empty_search,
            AiSessionProvider::Hermes,
            "h",
            source_ref,
            Some("20"),
            Some(1),
        )
        .unwrap();
        assert_eq!(
            (empty.session_id.as_str(), empty.provider),
            ("h", AiSessionProvider::Hermes)
        );
        assert_eq!(empty.matched_path.as_deref(), Some(CWD));
        assert_eq!(empty.source_ref.as_deref(), Some(file.as_str()));
        assert!(empty.messages.is_empty() && empty.next_cursor.is_none());
        assert_eq!(empty.title, existing.unwrap().title);
    }
}

// parity: TS-aiSessions-readAdditionalFileSessions-3880bb
#[test]
fn keeps_list_query_and_guards_for_empty_detail_searches() {
    let history = HistoryHome::new();
    let hermes = hermes_fixture();
    let file = history.put(hermes.file, &hermes.value, false);
    let mut context = history.context();
    context.query = Some("zqx-no-message-match".to_string());
    let listing = read_additional_file_sessions(&context).unwrap();
    let hermes_listing = source_of(&listing, AiSessionProvider::Hermes);
    assert!(
        hermes_listing.available
            && hermes_listing.sessions.is_empty()
            && hermes_listing.session_count == 0
    );
    assert_eq!(
        detail(
            &context,
            AiSessionProvider::Hermes,
            "missing",
            Some(&file),
            None,
            None
        ),
        None
    );
    let mut unrelated = context.clone();
    unrelated.worktree_path = Some("/unrelated-worktree".to_string());
    assert_eq!(
        detail(
            &unrelated,
            AiSessionProvider::Hermes,
            "h",
            Some(&file),
            None,
            None
        ),
        None
    );
    let outside = history.put("session_outside.json", &hermes.value, false);
    assert_eq!(
        detail(
            &context,
            AiSessionProvider::Hermes,
            "h",
            Some(&outside),
            None,
            None
        ),
        None
    );
}

// parity: TS-aiSessions-readAdditionalFileSessions-c09b73
#[test]
fn filters_query_and_roles_before_reverse_paging() {
    let history = HistoryHome::new();
    let file = history.put(
        ".hermes/sessions/session_filtered.json",
        &json!({"session_id": "filtered", "cwd": CWD, "messages": [
        {"role": "user", "content": "needle user"},
        {"role": "assistant", "content": "needle first assistant"},
        {"role": "tool", "content": "needle tool"},
        {"role": "assistant", "content": "needle second assistant"},
        {"role": "assistant", "content": "unrelated assistant"}]}),
        false,
    );
    let mut context = history.context();
    context.query = Some("NeEdLe".to_string());
    context.roles = Some(vec![AiMessageRole::Assistant]);
    let first = detail(
        &context,
        AiSessionProvider::Hermes,
        "filtered",
        Some(&file),
        None,
        Some(1),
    );
    assert_eq!(texts(&first), ["needle second assistant"]);
    let cursor = first.unwrap().next_cursor;
    assert_eq!(cursor.as_deref(), Some("1"));
    let second = detail(
        &context,
        AiSessionProvider::Hermes,
        "filtered",
        Some(&file),
        cursor.as_deref(),
        Some(1),
    );
    assert_eq!(texts(&second), ["needle first assistant"]);
    assert_eq!(second.unwrap().next_cursor, None);
    context.query = Some("needle tool".to_string());
    let excluded = detail(
        &context,
        AiSessionProvider::Hermes,
        "filtered",
        Some(&file),
        None,
        None,
    )
    .unwrap();
    assert!(excluded.messages.is_empty() && excluded.next_cursor.is_none());
}

fn assert_fixture_round_trip(fixture: &Fixture) {
    let history = HistoryHome::new();
    let file = history.put(fixture.file, &fixture.value, fixture.is_lines);
    for (name, value, is_lines) in &fixture.sidecars {
        history.put(name, value, *is_lines);
    }
    let context = history.context();
    let results = read_additional_file_sessions(&context).unwrap();
    let sessions = &source_of(&results, fixture.provider).sessions;
    assert_eq!(sessions.len(), 1, "{}", fixture.provider);
    let session = &sessions[0];
    assert_eq!(session.matched_path.as_deref(), Some(CWD));
    assert_eq!(session.first_user_prompt.as_deref(), Some(USER));
    let first = detail(
        &context,
        session.provider,
        &session.id,
        Some(&file),
        None,
        Some(1),
    );
    assert_eq!(first.as_ref().unwrap().messages.len(), 1);
    assert_eq!(first.as_ref().unwrap().next_cursor.as_deref(), Some("1"));
    let second = detail(
        &context,
        session.provider,
        &session.id,
        Some(&file),
        Some("1"),
        Some(1),
    );
    let mut pair = vec![texts(&first)[0].clone(), texts(&second)[0].clone()];
    pair.sort();
    let mut expected = vec![ANSWER.to_string(), USER.to_string()];
    expected.sort();
    assert_eq!(pair, expected);
    let mut filtered = history.context();
    filtered.roles = Some(vec![AiMessageRole::User]);
    filtered.query = Some("explain".to_string());
    assert_eq!(
        texts(&detail(
            &filtered,
            session.provider,
            &session.id,
            Some(&file),
            None,
            None
        )),
        [USER]
    );
    assert_eq!(
        detail(
            &context,
            session.provider,
            "wrong-id",
            Some(&file),
            None,
            None
        ),
        None
    );
    let mut other = history.context();
    other.worktree_path = Some("/other".to_string());
    assert_eq!(
        detail(
            &other,
            session.provider,
            &session.id,
            Some(&file),
            None,
            None
        ),
        None
    );
}

// parity: TS-aiSessions-readAdditionalFileSessions-3eb4e9
#[test]
fn reads_each_provider_list_content_pagination_and_isolation() {
    for fixture in fixtures() {
        assert_fixture_round_trip(&fixture);
    }
}

// parity: TS-aiSessions-readAdditionalFileSessions-6cc914
#[test]
fn rejects_outside_refs_and_isolates_broken_providers() {
    let history = HistoryHome::new();
    history.put(".hermes/sessions/session_broken.json", &Value::Null, false);
    let outside = history.put("outside.json", &json!({"cwd": CWD, "session_id": "secret", "messages": [{"role": "user", "content": "secret"}]}), false);
    let context = history.context();
    assert_eq!(
        detail(
            &context,
            AiSessionProvider::Hermes,
            "secret",
            Some(&outside),
            None,
            None
        ),
        None
    );
    let results = read_additional_file_sessions(&context).unwrap();
    assert_eq!(results.len(), 19);
    assert!(
        source_of(&results, AiSessionProvider::Hermes)
            .sessions
            .is_empty()
    );
}

// parity: TS-aiSessions-readAdditionalFileSessions-7bf35a
#[test]
fn keeps_readable_files_beside_malformed_ones_and_rejects_symlinks() {
    let history = HistoryHome::new();
    let hermes = hermes_fixture();
    history.put(hermes.file, &hermes.value, false);
    write_file(
        &format!("{}/.hermes/sessions/session_bad.json", history.home),
        "{broken",
    );
    let outside = history.put("outside.json", &json!({"cwd": CWD, "session_id": "secret", "messages": [{"role": "user", "content": "private"}]}), false);
    let link = format!("{}/.hermes/sessions/session_link.json", history.home);
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let context = history.context();
    let results = read_additional_file_sessions(&context).unwrap();
    let hermes_result = source_of(&results, AiSessionProvider::Hermes);
    assert_eq!(
        hermes_result
            .sessions
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        ["h"]
    );
    assert!(
        hermes_result
            .reason
            .as_deref()
            .unwrap()
            .contains("could not be read")
    );
    assert_eq!(
        detail(
            &context,
            AiSessionProvider::Hermes,
            "secret",
            Some(&link),
            None,
            None
        ),
        None
    );
}

// parity: TS-aiSessions-readAdditionalFileSessions-a2df24
#[test]
fn classifies_omp_transcripts_in_the_pi_directory_once() {
    let history = HistoryHome::new();
    history.put(
        ".pi/agent/sessions/omp.jsonl",
        &json!([
        {"type": "session", "id": "omp-id", "cwd": CWD}, {"type": "title", "v": 1, "title": "OMP"},
        {"type": "message", "message": {"role": "user", "content": USER}}]),
        true,
    );
    let results = read_additional_file_sessions(&history.context()).unwrap();
    assert!(
        source_of(&results, AiSessionProvider::Pi)
            .sessions
            .is_empty()
    );
    let omp_ids: Vec<_> = source_of(&results, AiSessionProvider::Omp)
        .sessions
        .iter()
        .map(|session| session.id.clone())
        .collect();
    assert_eq!(omp_ids, ["omp-id"]);
}

// parity: TS-aiSessions-readAdditionalFileSessions-7c7770
#[test]
fn reads_devin_cwd_and_visibility_from_sibling_database() {
    let history = HistoryHome::new();
    let mut value = devin_fixture().value;
    value.as_object_mut().unwrap().remove("working_directory");
    history.put(".local/share/devin/cli/transcripts/de.json", &value, false);
    let db = format!("{}/.local/share/devin/cli/sessions.db", history.home);
    let connection = Connection::open(&db).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE sessions (id TEXT, working_directory TEXT, title TEXT, hidden INTEGER)",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO sessions VALUES (?,?,?,?)",
            params!["de", CWD, "DB title", 0],
        )
        .unwrap();
    let context = history.context();
    let first = read_additional_file_sessions(&context).unwrap();
    let devin = &source_of(&first, AiSessionProvider::Devin).sessions;
    assert_eq!(devin[0].title.as_deref(), Some("DB title"));
    assert_eq!(devin[0].matched_path.as_deref(), Some(CWD));
    connection
        .execute("UPDATE sessions SET hidden=1", [])
        .unwrap();
    let hidden = read_additional_file_sessions(&context).unwrap();
    assert!(
        source_of(&hidden, AiSessionProvider::Devin)
            .sessions
            .is_empty()
    );
}

// parity: TS-aiSessions-readAdditionalFileSessions-be830b
#[test]
fn does_not_guess_antigravity_workspace_on_conflict() {
    let history = HistoryHome::new();
    let antigravity = antigravity_fixture();
    history.put(antigravity.file, &antigravity.value, true);
    history.put(".gemini/antigravity-cli/history.jsonl", &json!([
        {"conversationId": "ag", "workspace": CWD, "timestamp": AT}, {"conversationId": "ag", "workspace": "/other", "timestamp": AT}]), true);
    let results = read_additional_file_sessions(&history.context()).unwrap();
    assert!(
        source_of(&results, AiSessionProvider::Antigravity)
            .sessions
            .is_empty()
    );
}

// parity: TS-aiSessions-readAdditionalFileSessions-707579
#[test]
fn excludes_other_worktree_graph_transcript_before_reading_body() {
    let file = "/remote/home/.pi/agent/sessions/other.jsonl";
    let body = json_lines_text(&json!([
        {"type": "session", "id": "other", "cwd": "/other"},
        {"type": "message", "message": {"role": "user", "content": "large body".repeat(1_000)}}]));
    let runner = remote_file_system(&[(file, body.as_str())]);
    let context =
        AiSessionReaderContext::new(remote_host(runner.clone(), "history-host"), Some(CWD));
    let results = read_additional_file_sessions(&context).unwrap();
    assert!(
        source_of(&results, AiSessionProvider::Pi)
            .sessions
            .is_empty()
    );
    let full_reads: Vec<_> = runner
        .commands()
        .into_iter()
        .filter(|command| command.contains("cat '") && command.contains(file))
        .collect();
    assert!(full_reads.is_empty(), "{full_reads:?}");
}

// parity: TS-aiSessions-readAdditionalFileSessions-5e664c
#[test]
fn pages_kimi_turns_newest_first_without_timestamps() {
    let history = HistoryHome::new();
    let kimi = kimi_fixture();
    let file = history.put(kimi.file, &kimi.value, false);
    history.put(
        ".kimi-code/session_index.jsonl",
        &json!([{"sessionId": "session_ki", "workDir": CWD}]),
        true,
    );
    let records: Vec<Value> = ["first", "second"]
        .iter()
        .flat_map(|turn| {
            vec![
                json!({"type": "context.append_message", "message": {"role": "user", "origin": {"kind": "user"}, "content": format!("{turn} user")}}),
                json!({"type": "context.append_loop_event", "event": {"type": "content.part", "part": {"type": "text", "text": format!("{turn} assistant")}}}),
                json!({"type": "context.append_loop_event", "event": {"type": "step.end"}}),
            ]
        })
        .collect();
    history.put(
        ".kimi-code/sessions/work/session_ki/agents/main/wire.jsonl",
        &Value::Array(records),
        true,
    );
    let context = history.context();
    let first = detail(
        &context,
        AiSessionProvider::Kimi,
        "session_ki",
        Some(&file),
        None,
        Some(2),
    );
    let cursor = first.as_ref().unwrap().next_cursor.clone();
    let second = detail(
        &context,
        AiSessionProvider::Kimi,
        "session_ki",
        Some(&file),
        cursor.as_deref(),
        Some(2),
    );
    assert_eq!(texts(&first), ["second assistant", "second user"]);
    assert_eq!(texts(&second), ["first assistant", "first user"]);
    assert_eq!(second.unwrap().next_cursor, None);
}

// parity: TS-aiSessions-readAdditionalFileSessions-ed820a
#[test]
fn keeps_kiro_interleaving_across_pages() {
    let history = HistoryHome::new();
    let kiro = kiro_fixture();
    let file = history.put(kiro.file, &kiro.value, false);
    history.put(&format!("{}l", kiro.file), &json!([
        {"kind": "Prompt", "data": {"meta": {"timestamp": AT}, "content": [{"kind": "text", "data": "first user"}]}},
        {"kind": "AssistantMessage", "data": {"content": [{"kind": "text", "data": "first assistant"}]}},
        {"kind": "ToolResults", "data": {"content": [{"kind": "text", "data": "tool result"}]}},
        {"kind": "Prompt", "data": {"meta": {"timestamp": LATER}, "content": [{"kind": "text", "data": "second user"}]}},
        {"kind": "AssistantMessage", "data": {"content": [{"kind": "text", "data": "second assistant"}]}}]), true);
    let context = history.context();
    let session_id = "00000000-0000-0000-0000-000000000001";
    let first = detail(
        &context,
        AiSessionProvider::Kiro,
        session_id,
        Some(&file),
        None,
        Some(3),
    );
    let cursor = first.as_ref().unwrap().next_cursor.clone();
    let second = detail(
        &context,
        AiSessionProvider::Kiro,
        session_id,
        Some(&file),
        cursor.as_deref(),
        Some(3),
    );
    assert_eq!(
        texts(&first),
        ["second assistant", "second user", "tool result"]
    );
    assert_eq!(texts(&second), ["first assistant", "first user"]);
}
