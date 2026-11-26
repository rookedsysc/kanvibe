use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use rusqlite::{Connection, params};
use serde_json::json;

use super::*;
use crate::javascript::{iso_string_from_ms, stringify};
use crate::test_support::{ScriptedRunner, local_host, remote_host, temp_home};

fn context_for(home: &str, worktree_path: &str) -> AiSessionReaderContext {
    AiSessionReaderContext::new(local_host(home), Some(worktree_path))
}

fn database_path(home: &str) -> String {
    let directory = format!("{home}/.local/share/opencode");
    std::fs::create_dir_all(&directory).unwrap();
    format!("{directory}/opencode.db")
}

fn roles_and_texts(detail: &AggregatedAiSessionDetail) -> Vec<(AiMessageRole, String)> {
    detail
        .messages
        .iter()
        .map(|message| (message.role, message.full_text.clone()))
        .collect()
}

fn session_ids(result: &AiSessionReaderResult) -> Vec<String> {
    result
        .sessions
        .iter()
        .map(|session| session.id.clone())
        .collect()
}

const QUERY_SCOPE_SCHEMA: &str = "CREATE TABLE session (id TEXT, directory TEXT, title TEXT, time_created INTEGER, time_updated INTEGER, parent_id TEXT);
    CREATE TABLE message (id TEXT, data TEXT);
    CREATE TABLE part (session_id TEXT, message_id TEXT, data TEXT, time_created INTEGER);";

fn query_scope_fixture(home: &str) -> Connection {
    let connection = Connection::open(database_path(home)).unwrap();
    connection.execute_batch(QUERY_SCOPE_SCHEMA).unwrap();
    connection
}

fn insert_session(connection: &Connection, id: &str, directory: &str, time: i64, title: &str) {
    connection
        .execute(
            "INSERT INTO session VALUES (?, ?, ?, ?, ?, NULL)",
            params![id, directory, title, time, time],
        )
        .unwrap();
}

const SCOPED_WORKTREE: &str = "/fixtures/repo_%/task";

#[test]
fn filters_worktree_and_descendants_before_limiting_candidates() {
    let (_directory, home) = temp_home();
    let connection = query_scope_fixture(&home);
    insert_session(&connection, "target", SCOPED_WORKTREE, 1, "Session");
    insert_session(
        &connection,
        "nested",
        &format!("{SCOPED_WORKTREE}/nested"),
        2,
        "Session",
    );
    for index in 0..130 {
        insert_session(
            &connection,
            &format!("other-{index}"),
            "/other/task",
            100 + index,
            "Session",
        );
    }
    insert_session(
        &connection,
        "prefix-neighbor",
        &format!("{SCOPED_WORKTREE}-other"),
        1000,
        "Session",
    );
    insert_session(
        &connection,
        "wildcard-neighbor",
        "/fixtures/repoAX/task",
        1000,
        "Session",
    );
    let context = context_for(&home, SCOPED_WORKTREE);

    assert_eq!(
        session_ids(&read_opencode_sessions(&context).unwrap()),
        ["nested", "target"]
    );
    let now = crate::javascript::now_ms();
    let windows = LiveAiSessionWindows {
        running_window_ms: now as u64,
        recent_window_ms: 1000,
    };
    let live = read_opencode_live_sessions(&context, windows, now).unwrap();
    let live_ids: Vec<_> = live
        .iter()
        .filter_map(|row| row.session_id.clone())
        .collect();
    assert_eq!(live_ids, ["nested", "target"]);
}

#[test]
fn applies_search_before_limiting_candidates() {
    let (_directory, home) = temp_home();
    let connection = query_scope_fixture(&home);
    insert_session(&connection, "old-match", SCOPED_WORKTREE, 1, "needle");
    for index in 0..130 {
        insert_session(
            &connection,
            &format!("new-{index}"),
            SCOPED_WORKTREE,
            100 + index,
            "Session",
        );
    }
    let mut context = context_for(&home, SCOPED_WORKTREE);
    context.query = Some("needle".to_string());

    assert_eq!(
        session_ids(&read_opencode_sessions(&context).unwrap()),
        ["old-match"]
    );
}

fn seed_local_open(db_path: &str, directory: &str) {
    let connection = Connection::open(db_path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT NOT NULL, title TEXT, time_created INTEGER, time_updated INTEGER);
CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE part (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, message_id TEXT NOT NULL, data TEXT NOT NULL, time_created INTEGER NOT NULL);",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO session VALUES (?, ?, ?, ?, ?)",
            params![
                "local-open",
                directory,
                "Local OpenCode",
                1_700_000_000_000_i64,
                1_700_000_100_000_i64
            ],
        )
        .unwrap();
    let messages = [
        (
            "user-message",
            "user",
            "load local opencode history",
            1_700_000_000_000_i64,
        ),
        (
            "assistant-message",
            "assistant",
            "local opencode history loaded",
            1_700_000_010_000_i64,
        ),
    ];
    for (message_id, role, text, timestamp) in messages {
        let message_data = stringify(&json!({"role": role}));
        let part_data = stringify(&json!({"type": "text", "text": text}));
        connection
            .execute(
                "INSERT INTO message VALUES (?, ?, ?)",
                params![message_id, "local-open", message_data],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO part VALUES (?, ?, ?, ?, ?)",
                params![
                    format!("{message_id}-part"),
                    "local-open",
                    message_id,
                    part_data,
                    timestamp
                ],
            )
            .unwrap();
    }
}

#[test]
fn falls_back_to_python_sqlite_when_native_sqlite_cannot_open() {
    let (_directory, home) = temp_home();
    let worktree_path = format!("{home}/repo__worktrees/task");
    seed_local_open(&database_path(&home), &worktree_path);
    let context = context_for(&home, &worktree_path);
    let failing_native = |_: &str, _: &str, _: &SqlParameters| -> AiResult<Option<Vec<Value>>> {
        Err(AiError::message("native sqlite unavailable"))
    };
    let database = OpenCodeDatabase {
        context: &context,
        native: &failing_native,
    };

    let sessions = read_opencode_sessions_with(database).unwrap();
    assert!(sessions.available);
    assert_eq!(sessions.session_count, 1);
    let session = &sessions.sessions[0];
    assert_eq!(session.id, "local-open");
    assert_eq!(
        session.matched_path.as_deref(),
        Some(worktree_path.as_str())
    );
    assert_eq!(session.title.as_deref(), Some("Local OpenCode"));
    assert_eq!(
        session.first_user_prompt.as_deref(),
        Some("load local opencode history")
    );
    assert_eq!(session.updated_at, iso_string_from_ms(1_700_000_010_000.0));
    assert_eq!(session.source_ref.as_deref(), Some("local-open"));

    let detail =
        read_opencode_session_detail_with(database, "local-open", Some("local-open"), None, None)
            .unwrap()
            .unwrap();
    assert_eq!(
        roles_and_texts(&detail),
        [
            (
                AiMessageRole::Assistant,
                "local opencode history loaded".to_string()
            ),
            (
                AiMessageRole::User,
                "load local opencode history".to_string()
            ),
        ]
    );
}

const REMOTE_WORKTREE: &str = "/remote/repo__worktrees/task";

fn remote_listing_rows() -> String {
    stringify(&json!([{
        "id": "remote-open",
        "directory": REMOTE_WORKTREE,
        "title": "Remote OpenCode",
        "time_created": 1_700_000_000_000_i64,
        "time_updated": 1_700_000_010_000_i64,
        "part_count": 2,
        "first_user_part": stringify(&json!({"type": "text", "text": "fix remote chat"})),
        "matching_part_count": 0,
    }]))
}

fn remote_detail_rows() -> String {
    let row = |message_id: &str, text: &str, time: i64, role: &str| {
        json!({
            "session_id": "remote-open",
            "directory": REMOTE_WORKTREE,
            "title": "Remote OpenCode",
            "message_id": message_id,
            "part_data": stringify(&json!({"type": "text", "text": text})),
            "time_created": time,
            "message_data": stringify(&json!({"role": role})),
        })
    };
    stringify(&json!([
        row("user-message", "fix remote chat", 1_700_000_000_000, "user"),
        row(
            "assistant-message",
            "remote history loaded",
            1_700_000_010_000,
            "assistant"
        ),
    ]))
}

#[test]
fn reads_remote_opencode_sessions_and_detail_over_ssh() {
    let python_calls = std::sync::Arc::new(AtomicUsize::new(0));
    let counter = python_calls.clone();
    let runner = ScriptedRunner::new(move |command, _| {
        if command == "printf '%s' \"$HOME\"" {
            return Ok("/remote/home".to_string());
        }
        if command.contains("test -e") && command.contains("opencode.db") {
            return Ok("1".to_string());
        }
        if command.contains("python3 -c") {
            let call = counter.fetch_add(1, Ordering::SeqCst);
            return Ok(if call == 0 {
                remote_listing_rows()
            } else {
                remote_detail_rows()
            });
        }
        Err(AiError::message(format!(
            "unexpected remote command: {command}"
        )))
    });
    let context = AiSessionReaderContext::new(
        remote_host(runner.clone(), "remote-host"),
        Some(REMOTE_WORKTREE),
    );

    let sessions = read_opencode_sessions(&context).unwrap();
    assert!(sessions.available);
    assert_eq!((sessions.session_count, sessions.reason.clone()), (1, None));
    let session = &sessions.sessions[0];
    assert_eq!(session.id, "remote-open");
    assert_eq!(session.matched_path.as_deref(), Some(REMOTE_WORKTREE));
    assert_eq!(session.title.as_deref(), Some("Remote OpenCode"));
    assert_eq!(
        session.first_user_prompt.as_deref(),
        Some("fix remote chat")
    );
    assert_eq!(session.source_ref.as_deref(), Some("remote-open"));

    let detail =
        read_opencode_session_detail(&context, "remote-open", Some("remote-open"), None, Some(20))
            .unwrap()
            .unwrap();
    assert_eq!(
        roles_and_texts(&detail),
        [
            (
                AiMessageRole::Assistant,
                "remote history loaded".to_string()
            ),
            (AiMessageRole::User, "fix remote chat".to_string()),
        ]
    );
    let calls = runner.calls.lock().unwrap();
    assert!(
        calls
            .iter()
            .all(|call| call.ssh_host.as_deref() == Some("remote-host"))
    );
    let python_buffers: Vec<_> = calls
        .iter()
        .filter(|call| call.command.contains("python3 -c"))
        .map(|call| call.options.max_buffer_bytes)
        .collect();
    assert_eq!(
        python_buffers,
        [Some(64 * 1024 * 1024), Some(64 * 1024 * 1024)]
    );
    assert_eq!(python_calls.load(Ordering::SeqCst), 2);
}

const LITERAL_WORKTREE: &str = "/kanvibe-opencode-fixture/repo/task";

fn literal_rows(matching_id: Option<&str>) -> Vec<Value> {
    let rows = [
        (
            "literal-percent",
            "Percent",
            1_700_000_000_000_i64,
            "deployment reached 100% complete",
        ),
        (
            "literal-underscore",
            "Underscore",
            1_700_000_001_000,
            "rename user_name field",
        ),
        (
            "plain-text",
            "Plain",
            1_700_000_002_000,
            "database migration rollback",
        ),
    ];
    rows.iter()
        .map(|(id, title, time, text)| {
            json!({
                "id": id,
                "directory": LITERAL_WORKTREE,
                "title": title,
                "time_created": time,
                "time_updated": time,
                "part_count": 1,
                "first_user_part": stringify(&json!({"type": "text", "text": text})),
                "matching_part_count": if matching_id == Some(*id) { 1 } else { 0 },
            })
        })
        .collect()
}

#[test]
fn treats_body_search_wildcards_as_literal_text() {
    let (_directory, home) = temp_home();
    let calls: Mutex<Vec<(String, SqlParameters)>> = Mutex::new(Vec::new());
    let fake_native =
        |_: &str, sql: &str, parameters: &SqlParameters| -> AiResult<Option<Vec<Value>>> {
            calls
                .lock()
                .unwrap()
                .push((sql.to_string(), parameters.clone()));
            let matching_id = match parameters.get("query").and_then(Value::as_str) {
                Some("\\%") => Some("literal-percent"),
                Some("\\_") => Some("literal-underscore"),
                Some("database migration") => Some("plain-text"),
                _ => None,
            };
            Ok(Some(literal_rows(matching_id)))
        };
    let search = |query: &str| {
        let mut context = context_for(&home, LITERAL_WORKTREE);
        context.query = Some(query.to_string());
        let database = OpenCodeDatabase {
            context: &context,
            native: &fake_native,
        };
        session_ids(&read_opencode_sessions_with(database).unwrap())
    };
    let last_parameters = || calls.lock().unwrap().last().unwrap().1.clone();

    assert_eq!(search("%"), ["literal-percent"]);
    assert_eq!(last_parameters()["query"], json!("\\%"));
    assert_eq!(last_parameters()["worktreePath"], json!(LITERAL_WORKTREE));
    assert_eq!(search("_"), ["literal-underscore"]);
    assert_eq!(last_parameters()["query"], json!("\\_"));
    assert_eq!(search("database migration"), ["plain-text"]);
    assert_eq!(last_parameters()["query"], json!("database migration"));
    let calls = calls.lock().unwrap();
    assert!(
        calls
            .iter()
            .filter(|(_, parameters)| parameters.contains_key("query"))
            .all(|(sql, _)| sql.contains("ESCAPE"))
    );
}

#[test]
fn call_graph_nests_children_and_ends_idle_sessions() {
    let (_directory, home) = temp_home();
    let connection = query_scope_fixture(&home);
    let insert = |id: &str, parent: &str, created: i64, updated: i64| {
        connection
            .execute(
                "INSERT INTO session VALUES (?, ?, ?, ?, ?, ?)",
                params![
                    id,
                    SCOPED_WORKTREE,
                    format!("task {id}"),
                    created,
                    updated,
                    parent
                ],
            )
            .unwrap();
    };
    insert("child", "root", 1_700_000_000_000, 1_700_000_001_000);
    insert("grandchild", "child", 1_700_000_002_000, 1_700_000_009_500);
    let context = context_for(&home, SCOPED_WORKTREE);
    let windows = LiveAiSessionWindows {
        running_window_ms: 1_000,
        recent_window_ms: 10_000,
    };
    let roots =
        read_opencode_agent_call_graph(&context, "root", windows, 1_700_000_010_000.0).unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].task.as_deref(), Some("task child"));
    assert_eq!(
        roots[0].ended_at.as_deref(),
        Some("2023-11-14T22:13:21.000Z")
    );
    assert_eq!(roots[0].children[0].id, "grandchild");
    assert_eq!(roots[0].children[0].ended_at, None);
}
