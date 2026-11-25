use rusqlite::{Connection, params};
use serde_json::json;

use super::*;
use crate::javascript::stringify;
use crate::opencode_sessions::{read_opencode_session_detail, read_opencode_sessions};
use crate::test_support::{local_host_with, temp_home, write_file};

const CWD: &str = "/projects/legacy-task";

struct LegacyStore {
    _directory: tempfile::TempDir,
    home: String,
}

impl LegacyStore {
    fn new() -> Self {
        let (directory, home) = temp_home();
        Self {
            _directory: directory,
            home,
        }
    }

    fn put(&self, relative: &str, value: &Value) -> String {
        let file = format!("{}/.local/share/opencode/storage/{relative}", self.home);
        write_file(&file, &stringify(value));
        file
    }

    fn session(&self, id: &str, directory: &str) -> String {
        self.put(
            &format!("session/project/{id}.json"),
            &json!({"id": id, "directory": directory, "title": "Legacy session", "time": {"created": 1_793_527_200_000_i64, "updated": 1_793_527_260_000_i64}}),
        )
    }

    fn message(
        &self,
        id: &str,
        message_id: &str,
        role: &str,
        content: Option<Value>,
        created: i64,
    ) -> String {
        let mut value =
            json!({"id": message_id, "sessionID": id, "role": role, "time": {"created": created}});
        if let Some(content) = content {
            value["content"] = content;
        }
        self.put(&format!("message/{id}/{message_id}.json"), &value)
    }

    fn context(&self, worktree_path: &str) -> AiSessionReaderContext {
        AiSessionReaderContext::new(
            local_host_with(&self.home, &[("XDG_DATA_HOME", "")]),
            Some(worktree_path),
        )
    }
}

fn texts(detail: Option<AggregatedAiSessionDetail>) -> Vec<String> {
    detail
        .unwrap()
        .messages
        .into_iter()
        .map(|message| message.full_text)
        .collect()
}

// parity: TS-aiSessions-legacyOpenCodeSessions-56ddd0
#[test]
fn reads_inline_messages_in_creation_order_with_filters() {
    let store = LegacyStore::new();
    let file = store.session("ses_legacy", CWD);
    store.message(
        "ses_legacy",
        "a",
        "assistant",
        Some(json!([{"type": "text", "text": "finished change"}])),
        1_793_527_260_000,
    );
    store.message(
        "ses_legacy",
        "z",
        "user",
        Some(json!([{"type": "text", "text": "please change this"}])),
        1_793_527_200_000,
    );
    store.session("ses_other", "/other");
    let context = store.context(CWD);

    let listed = read_opencode_sessions(&context).unwrap();
    assert!(listed.available);
    assert_eq!(
        listed
            .sessions
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["ses_legacy"]
    );
    assert_eq!(
        listed.sessions[0].source_ref.as_deref(),
        Some(file.as_str())
    );
    assert_eq!(
        listed.sessions[0].first_user_prompt.as_deref(),
        Some("please change this")
    );
    assert_eq!(listed.sessions[0].message_count, 2);
    let first = read_opencode_session_detail(&context, "ses_legacy", Some(&file), None, Some(1))
        .unwrap()
        .unwrap();
    assert_eq!(first.next_cursor.as_deref(), Some("1"));
    assert_eq!(texts(Some(first)), ["finished change"]);
    assert_eq!(
        texts(
            read_opencode_session_detail(&context, "ses_legacy", Some(&file), Some("1"), Some(1))
                .unwrap()
        ),
        ["please change this"]
    );
    let mut filtered = store.context(CWD);
    filtered.roles = Some(vec![AiMessageRole::User]);
    filtered.query = Some("please".to_string());
    assert_eq!(
        texts(
            read_opencode_session_detail(&filtered, "ses_legacy", Some(&file), None, None).unwrap()
        ),
        ["please change this"]
    );
    let other = store.context("/other");
    assert_eq!(
        read_opencode_session_detail(&other, "ses_legacy", Some(&file), None, None).unwrap(),
        None
    );
}

// parity: TS-aiSessions-legacyOpenCodeSessions-05bade
#[test]
fn reads_part_files_without_generated_summaries() {
    let store = LegacyStore::new();
    let file = store.session("ses_parts", CWD);
    store.message("ses_parts", "msg_user", "user", None, 1_793_527_200_000);
    store.put(
        "part/msg_user/part_a.json",
        &json!({"id": "part_a", "messageID": "msg_user", "sessionID": "ses_parts", "type": "text", "text": "real prompt"}),
    );
    store.put(
        "message/ses_parts/msg_summary.json",
        &json!({"id": "msg_summary", "role": "assistant", "summary": {"body": "generated summary"}, "time": {"created": 1_793_527_260_000_i64}}),
    );
    let context = store.context(CWD);
    assert_eq!(
        texts(
            read_opencode_session_detail(&context, "ses_parts", Some(&file), None, None).unwrap()
        ),
        ["real prompt"]
    );
}

// parity: TS-aiSessions-legacyOpenCodeSessions-65ef77
#[test]
fn rejects_outside_refs_malformed_files_and_escaping_ids() {
    let store = LegacyStore::new();
    let file = store.session("ses_safe", CWD);
    store.message(
        "ses_safe",
        "msg",
        "user",
        Some(json!("safe")),
        1_793_527_200_000,
    );
    let outside = format!("{}/outside.json", store.home);
    write_file(
        &outside,
        &stringify(&json!({"id": "ses_safe", "directory": CWD})),
    );
    let context = store.context(CWD);

    assert_eq!(
        read_opencode_session_detail(&context, "ses_safe", Some(&outside), None, None).unwrap(),
        None
    );
    assert_eq!(
        read_opencode_session_detail(&context, "../../outside", Some(&file), None, None).unwrap(),
        None
    );
    store.put("session/project/broken.json", &Value::Null);
    assert_eq!(read_opencode_sessions(&context).unwrap().sessions.len(), 1);
}

fn seed_shared_session(db_path: &str) {
    let connection = Connection::open(db_path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE session(id TEXT,directory TEXT,title TEXT,time_created INTEGER,time_updated INTEGER); CREATE TABLE message(id TEXT,session_id TEXT,data TEXT); CREATE TABLE part(id TEXT,message_id TEXT,session_id TEXT,data TEXT,time_created INTEGER);",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO session VALUES(?,?,?,?,?)",
            params![
                "ses_shared",
                CWD,
                "SQLite session",
                1_793_527_200_000_i64,
                1_793_527_260_000_i64
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO message VALUES(?,?,?)",
            params!["msg_sql", "ses_shared", stringify(&json!({"role": "user"}))],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO part VALUES(?,?,?,?,?)",
            params![
                "prt",
                "msg_sql",
                "ses_shared",
                stringify(&json!({"type": "text", "text": "current prompt"})),
                1_793_527_260_000_i64
            ],
        )
        .unwrap();
}

// parity: TS-aiSessions-legacyOpenCodeSessions-eec154
#[test]
fn prefers_sqlite_over_stale_legacy_file_with_same_id() {
    let store = LegacyStore::new();
    let file = store.session("ses_shared", CWD);
    store.message(
        "ses_shared",
        "legacy",
        "user",
        Some(json!("stale prompt")),
        1_793_527_200_000,
    );
    let db_path = format!("{}/.local/share/opencode/opencode.db", store.home);
    seed_shared_session(&db_path);
    let context = store.context(CWD);

    let listed = read_opencode_sessions(&context).unwrap();
    assert_eq!(listed.sessions.len(), 1);
    assert_eq!(
        listed.sessions[0].first_user_prompt.as_deref(),
        Some("current prompt")
    );
    let mut stale_search = store.context(CWD);
    stale_search.query = Some("stale prompt".to_string());
    assert!(
        read_opencode_sessions(&stale_search)
            .unwrap()
            .sessions
            .is_empty()
    );
    assert_eq!(
        texts(
            read_opencode_session_detail(&context, "ses_shared", Some(&file), None, None).unwrap()
        ),
        ["current prompt"]
    );
    let writer = Connection::open(&db_path).unwrap();
    writer
        .execute_batch("DELETE FROM part; DELETE FROM message;")
        .unwrap();
    drop(writer);
    let emptied = read_opencode_session_detail(&context, "ses_shared", Some(&file), None, None)
        .unwrap()
        .unwrap();
    assert!(emptied.messages.is_empty());
}
