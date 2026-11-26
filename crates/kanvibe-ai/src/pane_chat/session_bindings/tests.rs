use std::collections::{BTreeMap, HashMap, VecDeque};

use base64::Engine;
use serde_json::json;

use super::*;
use crate::host::SessionHost;
use crate::test_support::{ScriptedRunner, temp_home, write_file};
use crate::types::AiSessionMatchScope;

const STARTED: &str = "Thu Nov 12 19:46:51 2026";
const REMOTE: &str = "pane-chat-binding-host";

fn process_table() -> String {
    [
        format!("100 1 {STARTED} /usr/local/bin/claude claude"),
        format!("200 100 {STARTED} /bin/sh sh -c hook"),
        format!("300 200 {STARTED} /bin/bash bash hook.sh"),
        format!("101 1 {STARTED} /usr/local/bin/claude claude"),
        format!("301 101 {STARTED} /bin/bash bash hook.sh"),
    ]
    .join("\n")
}

fn session(id: &str, source_ref: &str) -> AggregatedAiSession {
    AggregatedAiSession {
        id: id.to_string(),
        provider: crate::providers::AiSessionProvider::Claude,
        started_at: None,
        updated_at: None,
        matched_path: None,
        match_scope: AiSessionMatchScope::Worktree,
        title: None,
        first_user_prompt: None,
        message_count: 0,
        source_ref: Some(source_ref.to_string()),
    }
}

/// 리더가 받은 provider, worktree, 검색어
type CatalogCall = (PaneChatProvider, Option<String>, Option<String>);

/// provider별로 정해 둔 세션 목록을 돌려주는 리더. 한 번만 쓸 목록을 앞에 쌓아 둘 수 있다
#[derive(Default)]
struct FakeCatalog {
    responses: Mutex<HashMap<PaneChatProvider, VecDeque<Vec<AggregatedAiSession>>>>,
    calls: Mutex<Vec<CatalogCall>>,
    codex_rollout: Mutex<Option<String>>,
    rollout_calls: Mutex<Vec<String>>,
}

impl FakeCatalog {
    fn set(&self, provider: PaneChatProvider, sessions: Vec<AggregatedAiSession>) {
        self.responses
            .lock()
            .unwrap()
            .insert(provider, VecDeque::from([sessions]));
    }

    fn push_front_once(&self, provider: PaneChatProvider, sessions: Vec<AggregatedAiSession>) {
        self.responses
            .lock()
            .unwrap()
            .entry(provider)
            .or_default()
            .push_front(sessions);
    }

    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl PaneChatSessionCatalog for FakeCatalog {
    fn read_sessions(
        &self,
        provider: PaneChatProvider,
        context: &AiSessionReaderContext,
    ) -> AiResult<Vec<AggregatedAiSession>> {
        self.calls.lock().unwrap().push((
            provider,
            context.worktree_path.clone(),
            context.query.clone(),
        ));
        let mut responses = self.responses.lock().unwrap();
        let queue = responses.entry(provider).or_default();
        Ok(if queue.len() > 1 {
            queue.pop_front().unwrap()
        } else {
            queue.front().cloned().unwrap_or_default()
        })
    }

    fn find_codex_main_thread_rollout(
        &self,
        _context: &AiSessionReaderContext,
        thread_id: &str,
    ) -> AiResult<Option<String>> {
        self.rollout_calls
            .lock()
            .unwrap()
            .push(thread_id.to_string());
        Ok(self.codex_rollout.lock().unwrap().clone())
    }
}

/// 명령마다 지금 정해 둔 출력을 돌려주는 실행기
fn runner_with(output: &Arc<Mutex<String>>) -> Arc<ScriptedRunner> {
    let output = output.clone();
    ScriptedRunner::new(move |command, _| {
        if command == "printf '%s' \"$HOME\"" {
            return Ok("/home/user".to_string());
        }
        Ok(output.lock().unwrap().clone())
    })
}

struct Fixture {
    catalog: Arc<FakeCatalog>,
    output: Arc<Mutex<String>>,
    runner: Arc<ScriptedRunner>,
    registry: PaneChatSessionBindings,
}

impl Fixture {
    fn new() -> Self {
        let catalog = Arc::new(FakeCatalog::default());
        catalog.set(
            PaneChatProvider::Claude,
            vec![session(
                "session-a",
                "/home/.claude/projects/a/session-a.jsonl",
            )],
        );
        let output = Arc::new(Mutex::new(process_table()));
        let runner = runner_with(&output);
        let registry = PaneChatSessionBindings::new(catalog.clone());
        Self {
            catalog,
            output,
            runner,
            registry,
        }
    }

    fn set_output(&self, output: &str) {
        *self.output.lock().unwrap() = output.to_string();
    }

    fn host(&self, home: &str) -> SessionHost {
        SessionHost::local(home, BTreeMap::new(), self.runner.clone())
    }

    fn context(&self, home: &str, worktree: &str) -> PaneChatBindingContext {
        let mut reader = AiSessionReaderContext::new(self.host(home), Some(worktree));
        reader.repo_path = Some("/repo".to_string());
        PaneChatBindingContext {
            task_id: "task-a".to_string(),
            reader,
        }
    }

    fn task_context(&self) -> PaneChatBindingContext {
        self.context("/home/user", "/repo/a")
    }

    fn record(&self, metadata: Value) {
        self.registry
            .record_pane_chat_session_binding(&self.task_context(), &metadata);
    }

    fn find(&self, query: &PaneChatBindingQuery) -> Option<PaneChatSessionBinding> {
        self.registry.find_pane_chat_session_binding(query).unwrap()
    }
}

fn encode(payload: &Value) -> String {
    base64::engine::general_purpose::STANDARD.encode(payload.to_string())
}

fn metadata(provider: &str, payload: Value, hook_pid: i64) -> Value {
    json!({"provider": provider, "hookPid": hook_pid, "payloadBase64": encode(&payload)})
}

fn claude_metadata(session_id: &str) -> Value {
    metadata(
        "claude",
        json!({"session_id": session_id, "cwd": "/repo/a"}),
        300,
    )
}

fn query(provider: PaneChatProvider) -> PaneChatBindingQuery {
    PaneChatBindingQuery {
        task_id: "task-a".to_string(),
        ssh_host: None,
        provider,
        provider_pid: 100,
        process_start_time: STARTED.to_string(),
    }
}

fn session_and_source(binding: Option<PaneChatSessionBinding>) -> Option<(String, String)> {
    binding.map(|binding| (binding.session_id, binding.source_ref))
}

fn pair(session_id: &str, source_ref: &str) -> Option<(String, String)> {
    Some((session_id.to_string(), source_ref.to_string()))
}

#[test]
fn gemini_child_hook_binds_new_session_and_refuses_other_cwd() {
    let fixture = Fixture::new();
    fixture.set_output(&format!(
        "100 1 {STARTED} node node --max-old-space-size=4096 /opt/node_modules/@google/gemini-cli/bundle/gemini.js\n300 100 {STARTED} bash bash hook.sh"
    ));
    let gemini = query(PaneChatProvider::Gemini);
    let path_a = "/home/user/.gemini/tmp/project/chats/session-a.jsonl";
    fixture.record(metadata(
        "gemini",
        json!({"session_id": "gemini-a", "cwd": "/repo/a", "transcript_path": path_a}),
        300,
    ));
    let first = fixture.find(&gemini).unwrap();
    assert_eq!(
        (
            first.session_id.as_str(),
            first.source_ref.as_str(),
            first.is_pending
        ),
        ("gemini-a", path_a, true)
    );
    let path_b = "/home/user/.gemini/tmp/project/chats/session-b.jsonl";
    fixture.record(metadata(
        "gemini",
        json!({"session_id": "gemini-b", "cwd": "/repo/a", "transcript_path": path_b}),
        300,
    ));
    assert_eq!(
        session_and_source(fixture.find(&gemini)),
        pair("gemini-b", path_b)
    );
    fixture.record(metadata(
        "gemini",
        json!({"session_id": "wrong", "cwd": "/other", "transcript_path": path_b}),
        300,
    ));
    assert_eq!(fixture.find(&gemini).unwrap().session_id, "gemini-b");
}

#[test]
fn gemini_external_path_is_never_read() {
    let fixture = Fixture::new();
    fixture.catalog.set(PaneChatProvider::Gemini, Vec::new());
    fixture.set_output(&format!(
        "100 1 {STARTED} gemini gemini\n300 100 {STARTED} bash hook.sh"
    ));
    let mut context = fixture.task_context();
    context.reader.host = context.reader.host.with_ssh_host(Some(REMOTE));
    let payload = json!({"session_id": "missing", "cwd": "/repo/a", "transcript_path": "/private/secret.jsonl"});
    fixture
        .registry
        .record_pane_chat_session_binding(&context, &metadata("gemini", payload, 300));
    let remote_query = PaneChatBindingQuery {
        ssh_host: Some(REMOTE.to_string()),
        ..query(PaneChatProvider::Gemini)
    };
    assert_eq!(fixture.find(&remote_query), None);
    assert!(
        fixture
            .runner
            .commands()
            .iter()
            .all(|command| !command.contains("secret") && !command.contains("cat "))
    );
}

/// OpenCode 플러그인이 남기는 경로 기록
fn opencode_route(worktree: &str, session_id: Value, revision: Value, overrides: Value) -> Value {
    let mut route = json!({"pid": 100, "processStartTime": STARTED, "cwd": worktree, "sessionId": session_id, "revision": revision, "updatedAt": now_ms()});
    for (key, value) in overrides.as_object().unwrap() {
        route[key] = value.clone();
    }
    route
}

fn opencode_process(start: &str) -> PaneChatProcess {
    PaneChatProcess {
        pid: 100,
        process_start_time: start.to_string(),
        provider: PaneChatProvider::Opencode,
        codex_server_socket: None,
    }
}

#[test]
fn opencode_revisions_bind_to_pid_and_start_time() {
    let (_directory, root) = temp_home();
    let fixture = Fixture::new();
    let worktree = format!("{root}/repo/a");
    let context = fixture.context(&root, &worktree).reader;
    let route_path = format!("{worktree}/.kanvibe/pane-chat/opencode-100.json");
    write_file(
        &route_path,
        &opencode_route(&worktree, Value::Null, json!(1), json!({})).to_string(),
    );
    let home = fixture
        .registry
        .find_pane_chat_process_session_binding(&context, &opencode_process(STARTED))
        .unwrap();
    assert_eq!(
        (
            home.session_id.as_str(),
            home.source_ref.as_str(),
            home.is_empty,
            home.revision
        ),
        ("opencode-home:100", route_path.as_str(), true, Some(1))
    );
    for (revision, id) in ["ses_A", "ses_B", "ses_A"].into_iter().enumerate() {
        let revision = revision as u64 + 2;
        write_file(
            &route_path,
            &opencode_route(&worktree, json!(id), json!(revision), json!({})).to_string(),
        );
        assert_eq!(
            fixture
                .registry
                .find_pane_chat_process_session_binding(&context, &opencode_process(STARTED)),
            Some(PaneChatSessionBinding {
                revision: Some(revision),
                ..PaneChatSessionBinding::exact(id, id)
            })
        );
    }
    assert_eq!(
        fixture
            .registry
            .find_pane_chat_process_session_binding(&context, &opencode_process("reused PID")),
        None
    );
}

#[test]
fn small_ssh_clock_skew_is_allowed() {
    let fixture = Fixture::new();
    fixture.set_output(
        &opencode_route(
            "/repo/a",
            json!("ses_A"),
            json!(1),
            json!({"updatedAt": now_ms() + 1_000.0}),
        )
        .to_string(),
    );
    let mut context = fixture.task_context().reader;
    context.host = context.host.with_ssh_host(Some(REMOTE));
    assert_eq!(
        fixture
            .registry
            .find_pane_chat_process_session_binding(&context, &opencode_process(STARTED)),
        Some(PaneChatSessionBinding {
            revision: Some(1),
            ..PaneChatSessionBinding::exact("ses_A", "ses_A")
        })
    );
}

#[test]
fn opencode_refuses_other_runs_paths_and_stale_records() {
    let (_directory, root) = temp_home();
    let worktree = format!("{root}/repo/a");
    let invalid = [
        json!({"pid": 101}),
        json!({"processStartTime": "old incarnation"}),
        json!({"cwd": "/other"}),
        json!({"updatedAt": now_ms() - 10_000.0}),
        json!({"updatedAt": now_ms() + 10_000.0}),
        json!({"sessionId": "../../secret"}),
        json!({"revision": 0}),
    ];
    for overrides in invalid {
        let fixture = Fixture::new();
        let context = fixture.context(&root, &worktree).reader;
        let route = opencode_route(&worktree, json!("ses_A"), json!(1), overrides.clone());
        write_file(
            &format!("{worktree}/.kanvibe/pane-chat/opencode-100.json"),
            &route.to_string(),
        );
        assert_eq!(
            fixture
                .registry
                .find_pane_chat_process_session_binding(&context, &opencode_process(STARTED)),
            None,
            "{overrides}"
        );
    }
}

#[test]
fn binds_exact_ancestor_and_refuses_other_identities() {
    let fixture = Fixture::new();
    fixture.record(claude_metadata("session-a"));
    let claude = query(PaneChatProvider::Claude);
    assert_eq!(
        session_and_source(fixture.find(&claude)),
        pair("session-a", "/home/.claude/projects/a/session-a.jsonl")
    );
    assert_eq!(
        fixture.find(&PaneChatBindingQuery {
            provider_pid: 101,
            ..claude.clone()
        }),
        None
    );
    assert_eq!(
        fixture.find(&PaneChatBindingQuery {
            ssh_host: Some("remote".to_string()),
            ..claude.clone()
        }),
        None
    );
    assert_eq!(
        fixture.find(&PaneChatBindingQuery {
            task_id: "task-b".to_string(),
            ..claude.clone()
        }),
        None
    );
    assert_eq!(
        fixture.find(&PaneChatBindingQuery {
            process_start_time: "another start".to_string(),
            ..claude
        }),
        None
    );
}

#[test]
fn does_not_bind_unrelated_transcript_or_read_hook_path() {
    let fixture = Fixture::new();
    fixture.record(metadata("claude", json!({"session_id": "session-missing", "cwd": "/repo/a", "transcript_path": "/private/secret.jsonl"}), 300));
    assert_eq!(fixture.find(&query(PaneChatProvider::Claude)), None);
    let calls = fixture.catalog.calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        [(PaneChatProvider::Claude, Some("/repo/a".to_string()), None)]
    );
}

#[test]
fn waits_for_first_transcript_flush() {
    let fixture = Fixture::new();
    fixture
        .catalog
        .push_front_once(PaneChatProvider::Claude, Vec::new());
    fixture.record(claude_metadata("session-a"));
    let claude = query(PaneChatProvider::Claude);
    assert_eq!(fixture.find(&claude), None);
    assert_eq!(fixture.find(&claude).unwrap().session_id, "session-a");
}

#[test]
fn two_claude_processes_keep_their_own_sessions() {
    let fixture = Fixture::new();
    fixture.catalog.set(
        PaneChatProvider::Claude,
        vec![
            session("session-a", "/safe/a.jsonl"),
            session("session-b", "/safe/b.jsonl"),
        ],
    );
    fixture.record(claude_metadata("session-a"));
    fixture.record(metadata(
        "claude",
        json!({"session_id": "session-b", "cwd": "/repo/a"}),
        301,
    ));
    let claude = query(PaneChatProvider::Claude);
    assert_eq!(fixture.find(&claude).unwrap().session_id, "session-a");
    assert_eq!(
        fixture
            .find(&PaneChatBindingQuery {
                provider_pid: 101,
                ..claude
            })
            .unwrap()
            .session_id,
        "session-b"
    );
}

#[test]
fn ignores_codex_child_hooks_and_malformed_metadata() {
    let fixture = Fixture::new();
    fixture.record(metadata(
        "codex",
        json!({"session_id": "child", "cwd": "/repo/a", "agent_id": "worker"}),
        300,
    ));
    fixture.record(json!({"provider": "claude", "hookPid": 300, "payloadBase64": "bad-json"}));
    fixture.record(Value::Null);
    assert!(fixture.runner.commands().is_empty());
    assert_eq!(fixture.find(&query(PaneChatProvider::Claude)), None);
}

#[test]
fn refuses_other_provider_or_outside_cwd() {
    let fixture = Fixture::new();
    fixture.record(metadata(
        "codex",
        json!({"session_id": "session-a", "cwd": "/repo/a"}),
        300,
    ));
    fixture.record(metadata(
        "claude",
        json!({"session_id": "session-a", "cwd": "/another/project"}),
        300,
    ));
    assert_eq!(fixture.find(&query(PaneChatProvider::Claude)), None);
}

#[test]
fn shared_codex_daemon_is_never_the_owner() {
    let fixture = Fixture::new();
    fixture.set_output(&format!("100 1 {STARTED} /usr/local/bin/codex codex app-server --managed-daemon\n300 100 {STARTED} /bin/bash bash hook.sh"));
    fixture.catalog.set(
        PaneChatProvider::Codex,
        vec![session("session-a", "/safe/a.jsonl")],
    );
    fixture.record(metadata(
        "codex",
        json!({"session_id": "session-a", "cwd": "/repo/a"}),
        300,
    ));
    assert_eq!(fixture.find(&query(PaneChatProvider::Codex)), None);
}

#[test]
fn fences_a_to_b_to_a_without_querying_b() {
    let fixture = Fixture::new();
    fixture.catalog.set(
        PaneChatProvider::Claude,
        vec![
            session("session-a", "/safe/a.jsonl"),
            session("session-b", "/safe/b.jsonl"),
        ],
    );
    let claude = query(PaneChatProvider::Claude);
    fixture.record(claude_metadata("session-a"));
    let first = fixture.find(&claude).unwrap();
    fixture.record(claude_metadata("session-a"));
    assert_eq!(fixture.find(&claude).unwrap().revision, first.revision);
    fixture.record(claude_metadata("session-b"));
    fixture.record(claude_metadata("session-a"));
    let returned = fixture.find(&claude).unwrap();
    assert_eq!(returned.session_id, first.session_id);
    assert_ne!(returned.revision, first.revision);
}

const LINUX_DOMAIN: &str = "linux:machine:pid:[1]";

fn linux_probe() -> String {
    format!(
        "100 (claude (native)) S {} 12345 0\n{LINUX_DOMAIN}\n",
        vec!["0"; 18].join(" ")
    )
}

fn descriptor(overrides: Value) -> Value {
    let mut record = json!({"pid": 100, "sessionId": "session-a", "cwd": "/repo/a", "kind": "interactive", "procStart": "12345", "pidDomain": LINUX_DOMAIN});
    for (key, value) in overrides.as_object().unwrap() {
        record[key] = value.clone();
    }
    record
}

/// Claude 프로세스 기록을 임시 홈에 둔 장면
struct ProcessFixture {
    fixture: Fixture,
    _directory: tempfile::TempDir,
    home: String,
}

impl ProcessFixture {
    fn new(record: &Value) -> Self {
        let (directory, home) = temp_home();
        let fixture = Fixture::new();
        fixture.set_output(&linux_probe());
        fixture.catalog.set(
            PaneChatProvider::Claude,
            vec![session("session-a", "/safe/session-a.jsonl")],
        );
        *fixture.catalog.codex_rollout.lock().unwrap() = Some("/safe/codex-a.jsonl".to_string());
        let process_fixture = Self {
            fixture,
            _directory: directory,
            home,
        };
        process_fixture.write_record(record);
        process_fixture
    }

    fn write_record(&self, record: &Value) {
        write_file(
            &format!("{}/.claude/sessions/100.json", self.home),
            &record.to_string(),
        );
    }

    fn transcript(&self, session_id: &str) -> String {
        format!("{}/.claude/projects/-repo-a/{session_id}.jsonl", self.home)
    }

    fn context(&self) -> AiSessionReaderContext {
        self.fixture.context(&self.home, "/repo/a").reader
    }

    fn find(&self, provider: PaneChatProvider) -> Option<PaneChatSessionBinding> {
        let process = PaneChatProcess {
            pid: 100,
            process_start_time: STARTED.to_string(),
            provider,
            codex_server_socket: None,
        };
        self.fixture
            .registry
            .find_pane_chat_process_session_binding(&self.context(), &process)
    }
}

#[test]
fn binds_idle_claude_live_pid_record() {
    let scene = ProcessFixture::new(&descriptor(json!({})));
    write_file(&scene.transcript("session-a"), "");
    assert_eq!(
        scene.find(PaneChatProvider::Claude),
        Some(PaneChatSessionBinding::exact(
            "session-a",
            &scene.transcript("session-a")
        ))
    );
    assert!(scene.fixture.runner.commands().iter().all(|command| {
        !["lsof", "send-", "statusline"]
            .iter()
            .any(|word| command.contains(word))
    }));
}

#[test]
fn allows_new_empty_session_before_first_flush() {
    let scene = ProcessFixture::new(&descriptor(json!({})));
    scene
        .fixture
        .catalog
        .set(PaneChatProvider::Claude, Vec::new());
    assert_eq!(
        scene.find(PaneChatProvider::Claude),
        Some(PaneChatSessionBinding {
            is_pending: true,
            ..PaneChatSessionBinding::exact("session-a", &scene.transcript("session-a"))
        })
    );
    // 그 대화 ID 이름의 기록이 어디에도 없으면 모든 프로젝트 기록을 여는 리더 조회 없이 대기 중으로 잇는다
    assert_eq!(scene.fixture.catalog.call_count(), 0);

    // 프로젝트 디렉터리 이름이 어긋난 곳에 같은 이름의 기록이 있으면 리더가 허용한 기록으로만 잇는다(검색어 없이 한 번)
    write_file(
        &format!(
            "{}/.claude/projects/-moved-repo-a/session-a.jsonl",
            scene.home
        ),
        "",
    );
    scene.fixture.catalog.set(
        PaneChatProvider::Claude,
        vec![session("session-a", "/safe/session-a.jsonl")],
    );
    assert_eq!(
        scene.find(PaneChatProvider::Claude),
        Some(PaneChatSessionBinding::exact(
            "session-a",
            "/safe/session-a.jsonl"
        ))
    );
    let calls = scene.fixture.catalog.calls.lock().unwrap().clone();
    assert!(calls.iter().all(|(_, _, query)| query.is_none()));
    assert_eq!(calls.len(), 1);
}

#[test]
fn rejects_foreign_or_stale_process_records() {
    let invalid = [
        json!({"pid": 101}),
        json!({"procStart": "old-start"}),
        json!({"pidDomain": "linux:other:pid:[1]"}),
        json!({"cwd": "/different/repo"}),
        json!({"kind": "background"}),
        json!({"sessionId": "../unsafe"}),
        json!({"procStart": null}),
    ];
    for overrides in invalid {
        let scene = ProcessFixture::new(&descriptor(overrides.clone()));
        assert_eq!(scene.find(PaneChatProvider::Claude), None, "{overrides}");
        assert_eq!(scene.fixture.catalog.call_count(), 0);
    }
}

#[test]
fn validates_utc_start_marker_on_macos() {
    let scene = ProcessFixture::new(&descriptor(
        json!({"procStart": STARTED, "pidDomain": "darwin"}),
    ));
    scene.fixture.set_output(&format!("{STARTED}\ndarwin\n"));
    assert_eq!(
        scene.find(PaneChatProvider::Claude).unwrap().session_id,
        "session-a"
    );
    let calls = scene.fixture.runner.calls.lock().unwrap().clone();
    let probe = calls
        .iter()
        .find(|call| call.command.contains("LC_ALL=C TZ=UTC ps -o lstart="))
        .unwrap();
    assert!(probe.command.contains("uname -s"));
    assert_eq!(probe.ssh_host, None);
}

#[test]
fn rereads_process_session_after_clear_or_resume() {
    let scene = ProcessFixture::new(&descriptor(json!({})));
    write_file(&scene.transcript("session-a"), "");
    write_file(&scene.transcript("session-b"), "");
    assert_eq!(
        scene.find(PaneChatProvider::Claude).unwrap().session_id,
        "session-a"
    );
    scene.write_record(&descriptor(json!({"sessionId": "session-b"})));
    scene.fixture.catalog.set(
        PaneChatProvider::Claude,
        vec![session("session-b", "/safe/session-b.jsonl")],
    );
    assert_eq!(
        scene.find(PaneChatProvider::Claude).unwrap().session_id,
        "session-b"
    );
}

#[test]
fn unavailable_or_malformed_record_stays_unavailable() {
    let scene = ProcessFixture::new(&descriptor(json!({})));
    std::fs::remove_file(format!("{}/.claude/sessions/100.json", scene.home)).unwrap();
    assert_eq!(scene.find(PaneChatProvider::Claude), None);
    write_file(
        &format!("{}/.claude/sessions/100.json", scene.home),
        "not json",
    );
    assert_eq!(scene.find(PaneChatProvider::Claude), None);
    assert_eq!(scene.find(PaneChatProvider::Codex), None);
}

#[test]
fn reported_binding_resolves_only_allowed_ids() {
    let scene = ProcessFixture::new(&descriptor(json!({})));
    let registry = &scene.fixture.registry;
    let context = scene.context();
    assert_eq!(
        registry
            .find_pane_chat_reported_session_binding(&context, PaneChatProvider::Codex, "codex-a")
            .unwrap(),
        Some(PaneChatSessionBinding::exact(
            "codex-a",
            "/safe/codex-a.jsonl"
        ))
    );
    assert_eq!(
        registry
            .find_pane_chat_reported_session_binding(&context, PaneChatProvider::Claude, "missing")
            .unwrap(),
        None
    );
    assert_eq!(
        registry
            .find_pane_chat_reported_session_binding(
                &context,
                PaneChatProvider::Claude,
                "../unsafe"
            )
            .unwrap(),
        None
    );
    assert_eq!(
        scene.fixture.catalog.rollout_calls.lock().unwrap().clone(),
        ["codex-a"]
    );
    assert!(
        scene
            .fixture
            .catalog
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|(provider, _, _)| *provider != PaneChatProvider::Codex)
    );
}

#[test]
fn node_base64_decoding_is_lenient() {
    assert_eq!(decode_node_base64("eyJhIjoxfQ"), b"{\"a\":1}");
    assert_eq!(decode_node_base64("eyJhIjoxfQ=="), b"{\"a\":1}");
    assert!(serde_json::from_slice::<Value>(&decode_node_base64("bad-json")).is_err());
}

#[test]
fn stored_hook_binding_announces_its_task_once_and_ignored_hooks_stay_silent() {
    let fixture = Fixture::new();
    let announced: Arc<Mutex<Vec<String>>> = Arc::default();
    let sink = announced.clone();
    fixture
        .registry
        .on_binding_recorded(Arc::new(move |task_id: &str| {
            sink.lock().unwrap().push(task_id.to_string())
        }));
    fixture.set_output(&format!(
        "100 1 {STARTED} node node --max-old-space-size=4096 /opt/node_modules/@google/gemini-cli/bundle/gemini.js\n300 100 {STARTED} bash hook.sh"
    ));
    let path = "/home/user/.gemini/tmp/project/chats/session-a.jsonl";
    fixture.record(metadata(
        "gemini",
        json!({"session_id": "gemini-a", "cwd": "/repo/a", "transcript_path": path}),
        300,
    ));
    assert_eq!(*announced.lock().unwrap(), vec!["task-a".to_string()]);

    // 다른 작업 경로의 hook과 CLI 조상을 찾지 못한 hook은 바인딩을 남기지 않으므로 알리지도 않는다
    fixture.record(metadata(
        "gemini",
        json!({"session_id": "wrong", "cwd": "/other", "transcript_path": path}),
        300,
    ));
    fixture.record(metadata(
        "gemini",
        json!({"session_id": "orphan", "cwd": "/repo/a", "transcript_path": path}),
        999,
    ));
    assert_eq!(announced.lock().unwrap().len(), 1);
}

/// 같은 작업 폴더의 두 pane에서 Claude가 따로 돈다. 한쪽은 기록이 있는 대화, 다른 쪽은 첫 입력 전이라 기록이 없는 새 대화다.
/// 새 대화는 같은 폴더의 다른 기록을 빌려 쓰지 않고 자기 대화 ID의 예정 경로에 대기 중으로 묶인다
#[test]
fn two_panes_in_the_same_cwd_keep_their_own_claude_conversations() {
    let scene = ProcessFixture::new(&descriptor(json!({})));
    write_file(&scene.transcript("session-a"), "");
    let second_record = descriptor(json!({"pid": 200, "sessionId": "session-new"}));
    write_file(
        &format!("{}/.claude/sessions/200.json", scene.home),
        &second_record.to_string(),
    );
    let find_pid = |pid: i64| {
        let process = PaneChatProcess {
            pid,
            process_start_time: STARTED.to_string(),
            provider: PaneChatProvider::Claude,
            codex_server_socket: None,
        };
        scene
            .fixture
            .registry
            .find_pane_chat_process_session_binding(&scene.context(), &process)
    };
    assert_eq!(
        find_pid(100),
        Some(PaneChatSessionBinding::exact(
            "session-a",
            &scene.transcript("session-a")
        ))
    );
    let second = find_pid(200).unwrap();
    assert_eq!(second.session_id, "session-new");
    assert_eq!(second.source_ref, scene.transcript("session-new"));
    assert!(second.is_pending);

    // 두 번째 pane이 `/clear`로 대화를 바꿔도 첫 pane의 대화는 그대로다
    write_file(
        &format!("{}/.claude/sessions/200.json", scene.home),
        &descriptor(json!({"pid": 200, "sessionId": "session-next"})).to_string(),
    );
    assert_eq!(find_pid(200).unwrap().session_id, "session-next");
    assert_eq!(find_pid(100).unwrap().session_id, "session-a");
}
