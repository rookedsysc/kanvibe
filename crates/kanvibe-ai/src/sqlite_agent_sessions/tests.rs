use rusqlite::Connection;

use super::*;
use crate::test_support::{local_host, temp_home};

const SESSION_SCHEMA: &str = "CREATE TABLE session_v2 (id TEXT PRIMARY KEY,directory TEXT,title TEXT,time_created INTEGER,time_updated INTEGER);
CREATE TABLE session_message (session_id TEXT,type TEXT,data TEXT,time_created INTEGER,seq INTEGER);";
const ZCODE_SCHEMA: &str = "CREATE TABLE session (id TEXT PRIMARY KEY,directory TEXT,title TEXT,time_created INTEGER,time_updated INTEGER);
CREATE TABLE message (id TEXT PRIMARY KEY,session_id TEXT,data TEXT,time_created INTEGER,sequence INTEGER);
CREATE TABLE part (id TEXT PRIMARY KEY,message_id TEXT,session_id TEXT,data TEXT,time_created INTEGER,sequence INTEGER);";

struct AgentStore {
    _directory: tempfile::TempDir,
    home: String,
}

impl AgentStore {
    fn new() -> Self {
        let (directory, home) = temp_home();
        Self {
            _directory: directory,
            home,
        }
    }

    fn fixture(&self, relative: &str, sql: &str) -> String {
        let file_path = format!("{}/{relative}", self.home);
        std::fs::create_dir_all(node_path::dirname(&file_path)).unwrap();
        Connection::open(&file_path)
            .unwrap()
            .execute_batch(sql)
            .unwrap();
        file_path
    }

    fn context(&self) -> AiSessionReaderContext {
        let mut context =
            AiSessionReaderContext::new(local_host(&self.home), Some("/workspace/task"));
        context.repo_path = Some("/workspace".to_string());
        context
    }
}

fn source(
    results: &[AiSessionReaderResult],
    provider: AiSessionProvider,
) -> &AiSessionReaderResult {
    results
        .iter()
        .find(|result| result.provider == provider)
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

// parity: TS-aiSessions-readSqliteAgentSessions-5e911f
#[test]
fn reads_opencode2_from_matching_database_names_within_task() {
    let store = AgentStore::new();
    let db = store.fixture(
        ".local/share/opencode/opencode-beta.db",
        &format!("{SESSION_SCHEMA}
INSERT INTO session_v2 VALUES ('s','/workspace/task','Beta',1760000000000,1760000002000),('foreign','/workspace/task-other','Other',1760000000000,1760000003000);
INSERT INTO session_message VALUES ('s','user','{{\"text\":[\"question\",\"second line\"]}}',1760000000000,1),('s','assistant','{{\"content\":[{{\"type\":\"text\",\"text\":\"answer\"}}]}}',1760000002000,2);"),
    );
    let context = store.context();
    let sources = read_sqlite_agent_sessions(&context).unwrap();
    let sessions = &source(&sources, AiSessionProvider::Opencode2).sessions;
    assert_eq!(sessions.len(), 1);
    assert_eq!(
        (sessions[0].id.as_str(), sessions[0].title.as_deref()),
        ("s", Some("Beta"))
    );
    assert_eq!(
        sessions[0].first_user_prompt.as_deref(),
        Some("question second line")
    );
    assert_eq!(
        (sessions[0].message_count, sessions[0].source_ref.as_deref()),
        (2, Some(db.as_str()))
    );

    let provider = AiSessionProvider::Opencode2;
    let first = read_sqlite_agent_session_detail(&context, provider, "s", Some(&db), None, Some(1))
        .unwrap();
    assert_eq!(texts(&first), ["answer"]);
    assert_eq!(first.unwrap().next_cursor.as_deref(), Some("1"));
    let second =
        read_sqlite_agent_session_detail(&context, provider, "s", Some(&db), Some("1"), Some(1))
            .unwrap();
    assert_eq!(texts(&second), ["question\nsecond line"]);
    assert_eq!(second.unwrap().next_cursor, None);
    assert_eq!(
        read_sqlite_agent_session_detail(&context, provider, "foreign", Some(&db), None, None)
            .unwrap(),
        None
    );
}

// parity: TS-aiSessions-readSqliteAgentSessions-a569f8
#[test]
fn searches_full_opencode2_conversations_and_filters_roles_before_paging() {
    let store = AgentStore::new();
    store.fixture(
        ".local/share/opencode/opencode.db",
        &format!("{SESSION_SCHEMA}
INSERT INTO session_v2 VALUES ('s','/workspace/task',NULL,1760000000000,1760000002000);
INSERT INTO session_message VALUES ('s','user','{{\"text\":\"first\"}}',1760000000000,1),('s','assistant','{{\"text\":\"needle answer\"}}',1760000002000,2);"),
    );
    let mut context = store.context();
    context.query = Some("NEEDLE".to_string());
    let sources = read_sqlite_agent_sessions(&context).unwrap();
    assert_eq!(
        sources[0]
            .sessions
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        ["s"]
    );
    context.query = Some("needle".to_string());
    context.roles = Some(vec![AiMessageRole::Assistant]);
    let detail = read_sqlite_agent_session_detail(
        &context,
        AiSessionProvider::Opencode2,
        "s",
        None,
        None,
        Some(1),
    )
    .unwrap();
    assert_eq!(texts(&detail), ["needle answer"]);
}

// parity: TS-aiSessions-readSqliteAgentSessions-b8b1b4
#[test]
fn respects_zcode_visibility_and_sequence_order() {
    let store = AgentStore::new();
    let db = store.fixture(
        ".zcode/cli/db/db.sqlite",
        &format!("{ZCODE_SCHEMA}
INSERT INTO session VALUES ('s','/workspace/task',NULL,1760000000000,1760000003000);
INSERT INTO message VALUES ('m1','s','{{\"role\":\"user\"}}',1760000003000,1),('m2','s','{{\"role\":\"assistant\"}}',1760000001000,2),('hidden','s','{{\"role\":\"assistant\",\"semantics\":{{\"transcriptVisibility\":\"hidden\"}}}}',1760000002000,3);
INSERT INTO part VALUES ('p1','m1','s','{{\"type\":\"text\",\"text\":\"first prompt\"}}',1760000003000,1),('p2','m2','s','{{\"type\":\"text\",\"text\":\"second reply\"}}',1760000001000,2),('p3','hidden','s','{{\"type\":\"text\",\"text\":\"secret needle\"}}',1760000002000,3);"),
    );
    let mut context = store.context();
    let sources = read_sqlite_agent_sessions(&context).unwrap();
    let sessions = &source(&sources, AiSessionProvider::Zcode).sessions;
    assert_eq!(sessions.len(), 1);
    assert_eq!(
        (sessions[0].id.as_str(), sessions[0].message_count),
        ("s", 2)
    );
    assert_eq!(
        sessions[0].first_user_prompt.as_deref(),
        Some("first prompt")
    );
    let detail = read_sqlite_agent_session_detail(
        &context,
        AiSessionProvider::Zcode,
        "s",
        Some(&db),
        None,
        None,
    )
    .unwrap();
    assert_eq!(texts(&detail), ["second reply", "first prompt"]);
    context.query = Some("secret needle".to_string());
    let search = read_sqlite_agent_sessions(&context).unwrap();
    assert!(
        source(&search, AiSessionProvider::Zcode)
            .sessions
            .is_empty()
    );
}

// parity: TS-aiSessions-readSqliteAgentSessions-723d30
#[test]
fn ignores_arbitrary_source_ref_even_when_schema_matches() {
    let store = AgentStore::new();
    let db = store.fixture(
        "outside.db",
        &format!("{SESSION_SCHEMA}INSERT INTO session_v2 VALUES ('s','/workspace/task','private',1760000000000,1760000000000);"),
    );
    let context = store.context();
    for provider in [AiSessionProvider::Opencode2, AiSessionProvider::Zcode] {
        assert_eq!(
            read_sqlite_agent_session_detail(&context, provider, "s", Some(&db), None, None)
                .unwrap(),
            None
        );
    }
}

// parity: TS-aiSessions-readSqliteAgentSessions-3308de
#[test]
fn keeps_readable_database_when_another_install_is_broken() {
    let store = AgentStore::new();
    store.fixture(
        ".local/share/opencode/opencode.db",
        "CREATE TABLE session_v2 (id TEXT,directory TEXT,time_created INTEGER,time_updated INTEGER);",
    );
    store.fixture(
        ".local/share/opencode/opencode-beta.db",
        &format!(
            "{SESSION_SCHEMA}
INSERT INTO session_v2 VALUES ('healthy','/workspace/task','Healthy',1760000000000,1760000002000);
INSERT INTO session_message VALUES ('healthy','user','{{\"text\":\"hello\"}}',1760000000000,1);"
        ),
    );
    let sources = read_sqlite_agent_sessions(&store.context()).unwrap();
    assert!(sources[0].available);
    assert_eq!(
        sources[0]
            .sessions
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        ["healthy"]
    );
    assert!(
        sources[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("schema is unreadable")
    );
}

// parity: TS-aiSessions-readSqliteAgentSessions-04c49b
#[test]
fn reads_legacy_zcode_schema_and_keeps_tool_and_reasoning_roles() {
    let store = AgentStore::new();
    store.fixture(
        ".zcode/cli/db/db.sqlite",
        &format!("{}
INSERT INTO session VALUES ('s','/workspace/task','Tools',1760000000000,1760000003000);
INSERT INTO message VALUES ('m','s','{{\"role\":\"assistant\"}}',1760000001000);
INSERT INTO part VALUES ('p','m','s','{{\"type\":\"reasoning\",\"text\":\"thinking\"}}',1760000001000),('t','m','s','{{\"type\":\"tool\",\"tool\":\"read\",\"state\":{{\"output\":\"file body\"}}}}',1760000002000);",
            ZCODE_SCHEMA.replace(",sequence INTEGER", "")),
    );
    let mut context = store.context();
    context.roles = Some(vec![AiMessageRole::Tool]);
    let detail =
        read_sqlite_agent_session_detail(&context, AiSessionProvider::Zcode, "s", None, None, None)
            .unwrap()
            .unwrap();
    assert_eq!(detail.messages.len(), 1);
    assert_eq!(
        (
            detail.messages[0].role,
            detail.messages[0].full_text.as_str()
        ),
        (AiMessageRole::Tool, "read: file body")
    );
}

// parity: TS-aiSessions-readSqliteAgentSessions-109878
#[test]
fn reports_absent_stores_without_creating_databases() {
    let store = AgentStore::new();
    let sources = read_sqlite_agent_sessions(&store.context()).unwrap();
    let summary: Vec<_> = sources
        .iter()
        .map(|source| (source.provider, source.available))
        .collect();
    assert_eq!(
        summary,
        [
            (AiSessionProvider::Opencode2, false),
            (AiSessionProvider::Zcode, false)
        ]
    );
    assert!(!std::path::Path::new(&format!("{}/.zcode", store.home)).exists());
}

/// 로컬 rusqlite 이식이 원본 Python 스크립트와 같은 행을 돌려주는지 실제 python3로 대조한다
#[test]
fn local_port_matches_python_script_output() {
    let store = AgentStore::new();
    let db = store.fixture(
        ".zcode/cli/db/db.sqlite",
        &format!("{ZCODE_SCHEMA}
INSERT INTO session VALUES ('s','/workspace/task','Title',1760000000000,1760000003000),('n','/workspace/task/nested',NULL,1760000000000,1760000004000);
INSERT INTO message VALUES ('m1','s','{{\"role\":\"user\"}}',1760000003000,1),('m2','s','{{\"role\":\"assistant\"}}',1760000001000,NULL);
INSERT INTO part VALUES ('p1','m1','s','{{\"type\":\"text\",\"text\":\"한글 prompt\"}}',1760000003000,1),('p2','m2','s','{{\"type\":\"text\",\"text\":\"reply\"}}',1760000001000,NULL);"),
    );
    for session_id in [None, Some("s")] {
        let request = AgentDatabaseRequest {
            db_path: &db,
            provider: AiSessionProvider::Zcode,
            worktree: "/workspace/task".to_string(),
            separator: "/",
            query: String::new(),
            session_id,
        };
        let local = local_query::read_local_database(&request).unwrap();
        let mut payload = json!({"dbPath": db, "provider": "zcode", "worktree": "/workspace/task", "separator": "/", "query": ""});
        if let Some(session_id) = session_id {
            payload["sessionId"] = json!(session_id);
        }
        let limits = crate::sqlite_rows::LocalPythonLimits {
            max_output_bytes: SQLITE_READ_MAX_BYTES,
        };
        let output = crate::sqlite_rows::run_local_python(
            SQLITE_AGENT_READ_SCRIPT,
            &encode_base64_json(&payload),
            limits,
        )
        .unwrap();
        let python = parse_read_result(&output).unwrap();
        assert_eq!(local.sessions, python.sessions);
        assert_eq!(local.messages, python.messages);
        assert!(local.is_schema_available && python.is_schema_available);
    }
}

#[test]
fn remote_reads_send_the_python_script_with_base64_request() {
    let runner = crate::test_support::ScriptedRunner::new(|command, _| {
        if command == "printf '%s' \"$HOME\"" {
            return Ok("/remote/home".to_string());
        }
        if command.starts_with("test -e") {
            return Ok(if command.contains(".zcode") { "1" } else { "" }.to_string());
        }
        Ok("{\"schemaAvailable\":true,\"sessions\":[{\"id\":\"z\",\"directory\":\"/workspace/task\",\"title\":null,\"time_created\":1760000000000,\"time_updated\":1760000000000,\"message_count\":0,\"first_user_data\":null}],\"messages\":[]}".to_string())
    });
    let host = crate::test_support::remote_host(runner.clone(), "agent-host");
    let context = AiSessionReaderContext::new(host, Some("/workspace/task"));
    let sources = read_sqlite_agent_sessions(&context).unwrap();
    assert_eq!(
        source(&sources, AiSessionProvider::Zcode).sessions[0].id,
        "z"
    );
    let python_call = runner
        .commands()
        .into_iter()
        .find(|command| command.starts_with("python3 -c"))
        .unwrap();
    assert!(python_call.starts_with(&format!(
        "python3 -c {}",
        quote_shell_argument(SQLITE_AGENT_READ_SCRIPT)
    )));
    let payload = encode_base64_json(&json!({
        "dbPath": "/remote/home/.zcode/cli/db/db.sqlite",
        "provider": "zcode",
        "worktree": "/workspace/task",
        "separator": "/",
        "query": "",
    }));
    assert!(python_call.ends_with(&quote_shell_argument(&payload)));
}
