use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tungstenite::Message;

use super::*;
use crate::codex_app_server::CodexAppServerSocket;
use crate::test_support::{local_host, temp_home, write_file};
use crate::types::AiSessionReaderContext;

const THREAD_ID: &str = "0190aaaa-0000-7000-8000-00000000000a";

/// TS 테스트의 WebSocketServer. Unix 소켓 위에서 받은 요청을 기록하고 대화 상태로 답한다
struct FakeCodexServer {
    _directory: tempfile::TempDir,
    home: String,
    socket: String,
    thread: Arc<Mutex<Value>>,
    methods: Arc<Mutex<Vec<Value>>>,
    codex_home: Arc<Mutex<String>>,
    should_drop_after_input: Arc<AtomicBool>,
}

impl FakeCodexServer {
    fn start() -> Self {
        let (directory, home) = temp_home();
        let socket = format!("{home}/api");
        let thread = json!({
            "id": THREAD_ID, "cwd": format!("{home}/task"), "source": "cli", "parentThreadId": null,
            "ephemeral": false, "canAcceptDirectInput": true, "preview": "",
            "path": format!("{home}/.codex/sessions/rollout-test-{THREAD_ID}.jsonl"), "status": { "type": "idle" },
        });
        let server = Self {
            _directory: directory,
            socket: socket.clone(),
            thread: Arc::new(Mutex::new(thread)),
            methods: Arc::new(Mutex::new(Vec::new())),
            codex_home: Arc::new(Mutex::new(format!("{home}/.codex"))),
            should_drop_after_input: Arc::new(AtomicBool::new(false)),
            home,
        };
        let listener = UnixListener::bind(&socket).unwrap();
        let state = server.shared_state();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let state = state.clone();
                std::thread::spawn(move || serve_connection(stream, &state));
            }
        });
        server
    }

    fn shared_state(&self) -> ServerState {
        ServerState {
            thread: self.thread.clone(),
            methods: self.methods.clone(),
            codex_home: self.codex_home.clone(),
            should_drop_after_input: self.should_drop_after_input.clone(),
        }
    }

    fn context(&self) -> AiSessionReaderContext {
        AiSessionReaderContext::new(local_host(&self.home), Some(&format!("{}/task", self.home)))
    }

    fn set_thread(&self, key: &str, value: Value) {
        self.thread.lock().unwrap()[key] = value;
    }

    fn thread_path(&self) -> String {
        self.thread.lock().unwrap()["path"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn method_names(&self) -> Vec<String> {
        self.methods
            .lock()
            .unwrap()
            .iter()
            .map(|message| message["method"].as_str().unwrap().to_string())
            .collect()
    }

    fn last_message(&self) -> Value {
        self.methods.lock().unwrap().last().cloned().unwrap()
    }
}

#[derive(Clone)]
struct ServerState {
    thread: Arc<Mutex<Value>>,
    methods: Arc<Mutex<Vec<Value>>>,
    codex_home: Arc<Mutex<String>>,
    should_drop_after_input: Arc<AtomicBool>,
}

fn serve_connection(stream: UnixStream, state: &ServerState) {
    let Ok(mut socket) = tungstenite::accept(stream) else {
        return;
    };
    while let Ok(Message::Text(text)) = socket.read() {
        let message: Value = serde_json::from_str(&text).unwrap();
        state.methods.lock().unwrap().push(message.clone());
        let method = message["method"].as_str().unwrap_or_default();
        if method == "turn/start" && state.should_drop_after_input.load(Ordering::SeqCst) {
            return;
        }
        let Some(result) = respond(method, &message["params"], state) else {
            continue;
        };
        let reply = json!({ "id": message["id"], "result": result });
        if socket.send(Message::text(reply.to_string())).is_err() {
            return;
        }
    }
}

fn respond(method: &str, params: &Value, state: &ServerState) -> Option<Value> {
    match method {
        "initialize" => Some(json!({ "codexHome": state.codex_home.lock().unwrap().clone() })),
        "thread/read" => Some(json!({ "thread": state.thread.lock().unwrap().clone() })),
        "model/list" => Some(json!({ "data": [
            { "model": "model-a", "displayName": "Model A", "hidden": false, "supportedReasoningEfforts": [{ "reasoningEffort": "high", "description": "High" }], "serviceTiers": [{ "id": "priority" }] },
            { "model": "model-b", "displayName": "Model B", "hidden": false, "supportedReasoningEfforts": [], "serviceTiers": [] },
        ], "nextCursor": null })),
        "thread/settings/update" => {
            let mut thread = state.thread.lock().unwrap();
            if let Some(model) = params
                .get("model")
                .filter(|model| crate::javascript::is_truthy(model))
            {
                thread["model"] = model.clone();
            }
            if let Some(effort) = params
                .get("effort")
                .filter(|effort| crate::javascript::is_truthy(effort))
            {
                thread["reasoningEffort"] = effort.clone();
            }
            Some(json!({}))
        }
        "turn/start" => Some(json!({ "turn": { "id": "accepted-turn", "status": "inProgress" } })),
        _ => None,
    }
}

fn target<'a>(
    context: &'a AiSessionReaderContext,
    server: &FakeCodexServer,
) -> CodexAppServerTarget<'a> {
    CodexAppServerTarget {
        context,
        socket: CodexAppServerSocket::Path(server.socket.clone()),
        forwarder: None,
    }
}

fn input_request<'a>(
    context: &'a AiSessionReaderContext,
    server: &FakeCodexServer,
) -> CodexThreadInput<'a> {
    CodexThreadInput {
        target: target(context, server),
        thread_id: THREAD_ID.to_string(),
        source_ref: server.thread_path(),
        input: vec![
            CodexUserInput::Text {
                text: "첫 줄\n\nSecond line".to_string(),
            },
            CodexUserInput::LocalImage {
                path: format!("{}/image.png", server.home),
            },
        ],
        client_user_message_id: "local-operation-id".to_string(),
    }
}

fn settings_target<'a>(
    context: &'a AiSessionReaderContext,
    server: &FakeCodexServer,
) -> CodexThreadSettingsTarget<'a> {
    CodexThreadSettingsTarget {
        target: target(context, server),
        thread_id: THREAD_ID.to_string(),
        source_ref: server.thread_path(),
    }
}

/// 입력을 보내며 `before_send`·`on_write`가 불린 횟수를 함께 돌려준다
fn send_counting(
    request: &CodexThreadInput<'_>,
    before_send_result: AiResult<()>,
) -> (AiResult<()>, usize, usize) {
    let before_send_calls = std::cell::Cell::new(0);
    let mut write_calls = 0;
    let result = send_codex_thread_input(
        request,
        || {
            before_send_calls.set(before_send_calls.get() + 1);
            before_send_result
        },
        || write_calls += 1,
    );
    (result, before_send_calls.get(), write_calls)
}

#[test]
fn reads_real_model_list_and_changes_only_this_thread_model() {
    let server = FakeCodexServer::start();
    server.set_thread("model", json!("model-a"));
    server.set_thread("reasoningEffort", json!("high"));
    let context = server.context();
    let settings_target = settings_target(&context, &server);
    let settings = read_codex_thread_settings(&settings_target).unwrap();
    assert_eq!(settings.model.as_deref(), Some("model-a"));
    assert_eq!(settings.effort.as_deref(), Some("high"));
    assert_eq!(
        settings
            .models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["model-a", "model-b"]
    );

    let mut before_write_calls = 0;
    let change = PaneChatModelSettingChange::Model {
        model: "model-b".to_string(),
    };
    update_codex_thread_settings(&settings_target, &change, || {
        before_write_calls += 1;
        Ok(())
    })
    .unwrap();
    let last = server.last_message();
    assert_eq!(last["method"], "thread/settings/update");
    assert_eq!(
        last["params"],
        json!({ "threadId": THREAD_ID, "model": "model-b" })
    );
    assert_eq!(before_write_calls, 1);
    assert_eq!(
        read_codex_thread_settings(&settings_target)
            .unwrap()
            .model
            .as_deref(),
        Some("model-b")
    );
    let forbidden = regex::Regex::new(r"turn/|thread/(start|resume|subscribe)").unwrap();
    assert!(
        !server
            .method_names()
            .iter()
            .any(|method| forbidden.is_match(method))
    );
}

#[test]
fn unlisted_values_and_foreign_threads_send_no_setting_change() {
    let server = FakeCodexServer::start();
    server.set_thread("model", json!("model-a"));
    let context = server.context();
    let settings_target = settings_target(&context, &server);
    let unlisted = PaneChatModelSettingChange::Model {
        model: "unlisted".to_string(),
    };
    assert!(update_codex_thread_settings(&settings_target, &unlisted, || Ok(())).is_err());
    server.set_thread("cwd", json!("/foreign"));
    let effort = PaneChatModelSettingChange::Effort {
        effort: "high".to_string(),
    };
    assert!(update_codex_thread_settings(&settings_target, &effort, || Ok(())).is_err());
    assert!(
        !server
            .method_names()
            .iter()
            .any(|method| method == "thread/settings/update")
    );
}

#[test]
fn sends_exact_text_and_images_once_without_resuming_or_subscribing() {
    let server = FakeCodexServer::start();
    let context = server.context();
    let request = input_request(&context, &server);
    let (result, before_send_calls, write_calls) = send_counting(&request, Ok(()));
    result.unwrap();
    assert_eq!(
        server.method_names(),
        ["initialize", "initialized", "thread/read", "turn/start"]
    );
    assert_eq!(
        server.last_message()["params"],
        json!({
            "threadId": THREAD_ID,
            "input": [
                { "type": "text", "text": "첫 줄\n\nSecond line" },
                { "type": "localImage", "path": format!("{}/image.png", server.home) },
            ],
            "clientUserMessageId": "local-operation-id",
        })
    );
    assert_eq!((before_send_calls, write_calls), (1, 1));
}

#[test]
fn refuses_foreign_child_unavailable_or_busy_targets_before_writing() {
    let changes = [
        ("id", json!("0190bbbb-0000-7000-8000-00000000000b")),
        ("cwd", json!("/another-task")),
        ("source", json!({ "subAgent": {} })),
        ("parentThreadId", json!("parent")),
        ("ephemeral", json!(true)),
        ("canAcceptDirectInput", json!(false)),
        ("path", json!("/another-account/record.jsonl")),
        ("status", json!({ "type": "notLoaded" })),
        ("status", json!({ "type": "systemError" })),
    ];
    for (key, value) in changes {
        let server = FakeCodexServer::start();
        let context = server.context();
        let request = input_request(&context, &server);
        server.set_thread(key, value.clone());
        let (result, _, write_calls) = send_counting(&request, Ok(()));
        assert!(result.is_err(), "{key}: {value}");
        assert!(
            !server
                .method_names()
                .iter()
                .any(|method| method == "turn/start")
        );
        assert_eq!(write_calls, 0);
    }
}

#[test]
fn active_thread_receives_one_more_input_on_the_same_thread() {
    let server = FakeCodexServer::start();
    server.set_thread("status", json!({ "type": "active", "activeFlags": [] }));
    let context = server.context();
    let request = input_request(&context, &server);
    let (result, _, write_calls) = send_counting(&request, Ok(()));
    result.unwrap();
    let last = server.last_message();
    assert_eq!(last["method"], "turn/start");
    assert_eq!(last["params"]["threadId"], THREAD_ID);
    assert_eq!(
        last["params"]["input"],
        serde_json::to_value(&request.input).unwrap()
    );
    assert_eq!(write_calls, 1);
}

#[test]
fn changed_target_after_metadata_is_refused() {
    let server = FakeCodexServer::start();
    let context = server.context();
    let request = input_request(&context, &server);
    let (result, _, write_calls) = send_counting(
        &request,
        Err(crate::error::AiError::message("target changed")),
    );
    assert_eq!(result.unwrap_err().to_string(), "target changed");
    assert!(
        !server
            .method_names()
            .iter()
            .any(|method| method == "turn/start")
    );
    assert_eq!(write_calls, 0);
}

#[test]
fn lost_response_stays_uncertain_and_is_never_resent() {
    let server = FakeCodexServer::start();
    server.should_drop_after_input.store(true, Ordering::SeqCst);
    let context = server.context();
    let request = input_request(&context, &server);
    let (result, _, write_calls) = send_counting(&request, Ok(()));
    assert!(result.is_err());
    assert_eq!(
        server
            .method_names()
            .iter()
            .filter(|method| *method == "turn/start")
            .count(),
        1
    );
    assert_eq!(write_calls, 1);
}

#[test]
fn account_home_mismatch_and_unsupported_socket_have_no_fallback() {
    let server = FakeCodexServer::start();
    *server.codex_home.lock().unwrap() = "/other-account/.codex".to_string();
    let context = server.context();
    let mut request = input_request(&context, &server);
    let (result, _, first_writes) = send_counting(&request, Ok(()));
    assert!(result.is_err());
    assert_eq!(server.method_names(), ["initialize"]);
    server.methods.lock().unwrap().clear();
    request.target.socket = CodexAppServerSocket::Unsupported;
    let (result, _, second_writes) = send_counting(&request, Ok(()));
    assert!(result.is_err());
    assert!(server.method_names().is_empty());
    assert_eq!(first_writes + second_writes, 0);
}

#[test]
fn reads_only_the_exact_thread_without_subscribing() {
    let server = FakeCodexServer::start();
    let context = server.context();
    assert_eq!(
        read_codex_empty_thread(&target(&context, &server), THREAD_ID),
        Some(server.thread_path())
    );
    assert_eq!(
        server.method_names(),
        ["initialize", "initialized", "thread/read"]
    );
    assert_eq!(
        server.last_message()["params"],
        json!({ "threadId": THREAD_ID, "includeTurns": false })
    );
}

#[test]
fn rejects_metadata_that_cannot_prove_an_empty_main_thread() {
    let overrides = [
        ("id", json!("0190bbbb-0000-7000-8000-00000000000b")),
        ("cwd", json!("/other-task")),
        ("parentThreadId", json!("parent")),
        ("source", json!({ "subAgent": { "spawn": {} } })),
        ("source", json!("unknown")),
        ("ephemeral", json!(true)),
        ("canAcceptDirectInput", json!(false)),
        ("preview", json!("Existing conversation")),
        ("path", json!("/outside-account/rollout.jsonl")),
    ];
    for (key, value) in overrides {
        let server = FakeCodexServer::start();
        server.set_thread(key, value.clone());
        let context = server.context();
        assert_eq!(
            read_codex_empty_thread(&target(&context, &server), THREAD_ID),
            None,
            "{key}: {value}"
        );
    }
}

#[test]
fn materialized_record_is_not_treated_as_empty() {
    let server = FakeCodexServer::start();
    write_file(&server.thread_path(), "untrusted or duplicate metadata");
    let context = server.context();
    assert_eq!(
        read_codex_empty_thread(&target(&context, &server), THREAD_ID),
        None
    );
}

#[test]
fn other_provider_home_and_unsupported_socket_are_rejected() {
    let server = FakeCodexServer::start();
    *server.codex_home.lock().unwrap() = "/other-account/.codex".to_string();
    let context = server.context();
    assert_eq!(
        read_codex_empty_thread(&target(&context, &server), THREAD_ID),
        None
    );
    server.methods.lock().unwrap().clear();
    let unsupported = CodexAppServerTarget {
        context: &context,
        socket: CodexAppServerSocket::Unsupported,
        forwarder: None,
    };
    assert_eq!(read_codex_empty_thread(&unsupported, THREAD_ID), None);
    assert_eq!(
        read_codex_empty_thread(&target(&context, &server), &THREAD_ID[..10]),
        None
    );
    assert!(server.method_names().is_empty());
}
