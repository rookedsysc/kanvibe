use serde_json::{Value, json};

use super::*;
use crate::host::SessionHost;
use crate::javascript::stringify;
use crate::test_support::{local_host, remote_file_system, remote_host, temp_home, write_file};

fn context(host: SessionHost, worktree_path: &str) -> AiSessionReaderContext {
    let mut context = AiSessionReaderContext::new(host, Some(worktree_path));
    context.repo_path = Some(worktree_path.to_string());
    context
}

/// TS 테스트의 `writeJson`. 두 칸 들여쓰기는 JSON 해석 결과에 영향이 없어 한 줄로 쓴다
fn write_json(file_path: &str, value: &Value) {
    write_file(file_path, &format!("{}\n", stringify(value)));
}

fn roles(detail: &AggregatedAiSessionDetail) -> Vec<AiMessageRole> {
    detail.messages.iter().map(|message| message.role).collect()
}

fn role_texts(detail: &AggregatedAiSessionDetail) -> Vec<(AiMessageRole, String)> {
    detail
        .messages
        .iter()
        .map(|message| (message.role, message.full_text.clone()))
        .collect()
}

// parity: TS-aiSessions-readAiSessions-8dea2e
#[test]
fn reads_chat_recordings_and_classifies_responses_thoughts_and_tools() {
    let (_directory, home) = temp_home();
    let worktree_path = format!("{home}/repo__worktrees/task");
    let chat_file = format!("{home}/.gemini/tmp/task/chats/session-2026-01-01T00-00-gemini.json");
    let mut projects = serde_json::Map::new();
    projects.insert(worktree_path.clone(), json!("task"));
    write_json(
        &format!("{home}/.gemini/projects.json"),
        &json!({ "projects": projects }),
    );
    write_json(
        &chat_file,
        &json!({
            "sessionId": "gemini-session",
            "projectHash": "unused",
            "startTime": "2026-01-01T00:00:00.000Z",
            "lastUpdated": "2026-01-01T00:04:00.000Z",
            "kind": "main",
            "messages": [
                { "id": "user-1", "timestamp": "2026-01-01T00:01:00.000Z", "type": "user", "content": [{ "text": "make a plan" }] },
                {
                    "id": "gemini-1", "timestamp": "2026-01-01T00:02:00.000Z", "type": "gemini", "model": "gemini-2.5-pro",
                    "content": [{ "text": "Here is the plan." }],
                    "thoughts": [{ "subject": "analysis", "description": "Compare history formats." }],
                    "toolCalls": [{ "name": "read_file", "args": { "path": "x" }, "result": "file contents" }],
                },
                { "id": "info-1", "timestamp": "2026-01-01T00:03:00.000Z", "type": "info", "content": "checkpoint saved" },
            ],
        }),
    );
    let context = context(local_host(&home), &worktree_path);

    let sessions = read_gemini_sessions(&context).unwrap();
    assert_eq!(sessions.sessions.len(), 1);
    let session = &sessions.sessions[0];
    assert_eq!(session.id, "gemini-session");
    assert_eq!(session.provider, AiSessionProvider::Gemini);
    assert_eq!(session.title.as_deref(), Some("make a plan"));
    assert_eq!(
        session.updated_at.as_deref(),
        Some("2026-01-01T00:02:00.000Z")
    );
    assert_eq!(session.source_ref.as_deref(), Some(chat_file.as_str()));

    let detail =
        read_gemini_session_detail(&context, "gemini-session", Some(&chat_file), None, Some(20))
            .unwrap()
            .unwrap();
    assert_eq!(
        roles(&detail),
        [
            AiMessageRole::System,
            AiMessageRole::Assistant,
            AiMessageRole::Reasoning,
            AiMessageRole::Tool,
            AiMessageRole::User,
        ]
    );
    let tool = detail
        .messages
        .iter()
        .find(|message| message.role == AiMessageRole::Tool)
        .unwrap();
    assert!(tool.full_text.contains("read_file"));
}

// parity: TS-aiSessions-readAiSessions-d0d649
#[test]
fn uses_project_root_file_when_projects_json_has_no_mapping() {
    let (_directory, home) = temp_home();
    let worktree_path = format!("{home}/repo__worktrees/task");
    let project_directory = format!("{home}/.gemini/tmp/project-with-root-file");
    let chat_file = format!("{project_directory}/chats/session-2026-01-01T00-20-gemini.json");
    write_file(
        &format!("{project_directory}/.project_root"),
        &worktree_path,
    );
    write_json(
        &chat_file,
        &json!({
            "sessionId": "gemini-root-file-session",
            "startTime": "2026-01-01T00:20:00.000Z",
            "lastUpdated": "2026-01-01T00:22:00.000Z",
            "messages": [
                { "id": "user-1", "timestamp": "2026-01-01T00:21:00.000Z", "type": "user", "content": [{ "text": "load project root gemini history" }] },
                { "id": "assistant-1", "timestamp": "2026-01-01T00:22:00.000Z", "type": "gemini", "content": [{ "text": "project root history loaded" }] },
            ],
        }),
    );
    let context = context(local_host(&home), &worktree_path);

    let sessions = read_gemini_sessions(&context).unwrap();
    assert_eq!(sessions.sessions.len(), 1);
    let session = &sessions.sessions[0];
    assert_eq!(session.id, "gemini-root-file-session");
    assert_eq!(
        session.matched_path.as_deref(),
        Some(worktree_path.as_str())
    );
    assert_eq!(
        session.first_user_prompt.as_deref(),
        Some("load project root gemini history")
    );
    assert_eq!(session.source_ref.as_deref(), Some(chat_file.as_str()));

    let detail = read_gemini_session_detail(
        &context,
        "gemini-root-file-session",
        Some(&chat_file),
        None,
        None,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        role_texts(&detail),
        [
            (
                AiMessageRole::Assistant,
                "project root history loaded".to_string()
            ),
            (
                AiMessageRole::User,
                "load project root gemini history".to_string()
            ),
        ]
    );
}

// parity: TS-aiSessions-readAiSessions-f00005
#[test]
fn reads_remote_worktree_sessions_and_detail_over_ssh() {
    let worktree_path = "/remote/repo__worktrees/task";
    let chat_file = "/remote/home/.gemini/tmp/remote-repo/chats/remote-gemini.json";
    let mut projects = serde_json::Map::new();
    projects.insert(worktree_path.to_string(), json!("remote-repo"));
    let projects_content = stringify(&json!({ "projects": projects }));
    let chat_content = stringify(&json!({
        "sessionId": "remote-gemini",
        "startTime": "2026-01-01T00:00:00.000Z",
        "lastUpdated": "2026-01-01T00:02:00.000Z",
        "messages": [
            { "id": "user-1", "timestamp": "2026-01-01T00:01:00.000Z", "type": "user", "content": [{ "text": "load repo gemini history" }] },
            { "id": "assistant-1", "timestamp": "2026-01-01T00:02:00.000Z", "type": "gemini", "content": [{ "text": "Gemini history loaded remotely." }] },
        ],
    }));
    let runner = remote_file_system(&[
        ("/remote/home/.gemini/projects.json", &projects_content),
        (chat_file, &chat_content),
    ]);
    let mut remote_context = context(remote_host(runner.clone(), "remote-host"), worktree_path);
    remote_context.repo_path = Some("/remote/repo".to_string());

    let sessions = read_gemini_sessions(&remote_context).unwrap();
    assert_eq!((sessions.available, sessions.session_count), (true, 1));
    let session = &sessions.sessions[0];
    assert_eq!(session.id, "remote-gemini");
    assert_eq!(session.matched_path.as_deref(), Some(worktree_path));
    assert_eq!(session.match_scope, AiSessionMatchScope::Worktree);
    assert_eq!(
        session.first_user_prompt.as_deref(),
        Some("load repo gemini history")
    );
    assert_eq!(session.source_ref.as_deref(), Some(chat_file));

    let detail = read_gemini_session_detail(
        &remote_context,
        "remote-gemini",
        Some(chat_file),
        None,
        Some(20),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        role_texts(&detail),
        [
            (
                AiMessageRole::Assistant,
                "Gemini history loaded remotely.".to_string()
            ),
            (AiMessageRole::User, "load repo gemini history".to_string()),
        ]
    );
    let calls = runner.calls.lock().unwrap();
    assert!(!calls.is_empty());
    assert!(
        calls
            .iter()
            .all(|call| call.ssh_host.as_deref() == Some("remote-host"))
    );
}

const JSONL_WORKTREE: &str = "/workspace/gemini-jsonl";

fn jsonl_fixture(home: &str) -> String {
    let directory = format!("{home}/.gemini/tmp/project/chats");
    write_file(
        &format!("{home}/.gemini/tmp/project/.project_root"),
        JSONL_WORKTREE,
    );
    let events = [
        json!({ "sessionId": "jsonl-session", "startTime": "2026-11-01T00:00:00Z" }),
        json!({ "type": "user", "timestamp": "2026-11-01T00:01:00Z", "content": "Initial question" }),
        json!({ "type": "gemini", "timestamp": "2026-11-01T00:02:00Z", "content": "Unique answer" }),
        json!({ "$set": { "lastUpdated": "2026-11-01T00:03:00Z" } }),
    ];
    let file = format!("{directory}/session.jsonl");
    write_file(&file, &crate::test_support::json_lines(&events));
    file
}

// parity: TS-aiSessions-geminiJsonl-3dd6cd
#[test]
fn discovers_jsonl_chats_and_maps_project_root() {
    let (_directory, home) = temp_home();
    jsonl_fixture(&home);
    let result = read_gemini_sessions(&context(local_host(&home), JSONL_WORKTREE)).unwrap();
    assert_eq!(result.sessions.len(), 1);
    let session = &result.sessions[0];
    assert_eq!(session.id, "jsonl-session");
    assert_eq!(session.title.as_deref(), Some("Initial question"));
    assert_eq!(session.message_count, 2);
    assert_eq!(session.matched_path.as_deref(), Some(JSONL_WORKTREE));
}

// parity: TS-aiSessions-geminiJsonl-732f5f
#[test]
fn finds_body_matches_and_pages_jsonl_messages_newest_first() {
    let (_directory, home) = temp_home();
    jsonl_fixture(&home);
    let base = context(local_host(&home), JSONL_WORKTREE);
    let mut query_context = base.clone();
    query_context.query = Some("Unique answer".to_string());
    assert_eq!(
        read_gemini_sessions(&query_context).unwrap().sessions.len(),
        1
    );

    let first = read_gemini_session_detail(&base, "jsonl-session", None, None, Some(1))
        .unwrap()
        .unwrap();
    let texts = |detail: &AggregatedAiSessionDetail| {
        detail
            .messages
            .iter()
            .map(|message| message.full_text.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(texts(&first), ["Unique answer"]);
    let older = read_gemini_session_detail(
        &base,
        "jsonl-session",
        None,
        first.next_cursor.as_deref(),
        Some(1),
    )
    .unwrap()
    .unwrap();
    assert_eq!(texts(&older), ["Initial question"]);
    let foreign = context(local_host(&home), "/foreign");
    assert_eq!(
        read_gemini_session_detail(&foreign, "jsonl-session", None, None, None).unwrap(),
        None
    );
}

#[test]
fn live_sessions_map_project_hashes_through_projects_json() {
    let (_directory, home) = temp_home();
    let worktree_path = format!("{home}/task");
    let chat_file = format!("{home}/.gemini/tmp/hash-1/chats/session-a.json");
    let mut projects = serde_json::Map::new();
    projects.insert(worktree_path.clone(), json!("hash-1"));
    write_json(
        &format!("{home}/.gemini/projects.json"),
        &json!({ "projects": projects }),
    );
    write_json(&chat_file, &json!({ "sessionId": "a", "messages": [] }));
    let windows = LiveAiSessionWindows {
        running_window_ms: 90_000,
        recent_window_ms: 6 * 60 * 60 * 1000,
    };
    let now_ms = crate::host::local_mtime_ms(&chat_file).unwrap() + 1_000.0;
    let snapshots =
        read_gemini_live_sessions(&context(local_host(&home), &worktree_path), windows, now_ms)
            .unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].session_id.as_deref(), Some("session-a"));
    assert!(snapshots[0].running_subtasks.is_empty());
    let foreign =
        read_gemini_live_sessions(&context(local_host(&home), "/elsewhere"), windows, now_ms)
            .unwrap();
    assert!(foreign.is_empty());
}
