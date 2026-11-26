//! TS `mobileAiRoutes.test.mjs`, `mobileAiMount.test.mjs`.
//!
//! TS는 AI 경로와 로그인 소켓 처리기를 따로 띄우거나 가짜 소켓 객체로 불렀다. 여기서는 실제 서버의 공통 인증 뒤에서
//! HTTP와 WebSocket으로 같은 결과를 본다.

use kanvibe_server::bridge::BridgeError;
use serde_json::{Value, json};

use crate::harness::*;

fn login_path() -> String {
    format!("/api/mobile/ai/accounts/{ACCOUNT_ID}/login?cols=80&rows=24")
}

/// 로그인 소켓을 열고 `ready`까지 받는다
async fn open_login(server: &TestServer, authorization: &str) -> WsClient {
    let mut client = server
        .websocket(&login_path(), Some(authorization))
        .await
        .expect("로그인 소켓");
    assert_eq!(client.next_json().await, json!({ "type": "ready" }));
    client
}

fn socket_id(server: &TestServer, index: usize) -> Value {
    server.fake.calls("open_mobile_ai_login")[index][1].clone()
}

#[tokio::test]
async fn ai_routes_return_the_bridge_envelopes() {
    let server = TestServer::start().await;
    let running = server.get("/api/mobile/ai/running-panes", Some(GOOD)).await;
    assert_eq!(
        running.pair(),
        (200, json!({ "success": true, "panes": [] }))
    );
    let live = server
        .get("/api/mobile/tasks/task-1/live-ai-sessions", Some(GOOD))
        .await;
    assert_eq!(
        live.pair(),
        (
            200,
            json!({ "success": true, "liveSessions": { "taskId": "task-1", "sessions": [] } })
        )
    );
    let graph = server
        .get(
            "/api/mobile/tasks/task-1/agent-call-graph?provider=claude&sessionId=session-1",
            Some(GOOD),
        )
        .await;
    assert_eq!(
        graph.pair(),
        (200, json!({ "success": true, "graph": null }))
    );
    assert_eq!(
        server.fake.calls("get_mobile_agent_call_graph"),
        vec![json!(["task-1", "claude", "session-1"])]
    );
    let accounts = server
        .get("/api/mobile/ai/accounts", Some(GOOD))
        .await
        .json();
    assert_eq!(accounts["accounts"].as_array().map(Vec::len), Some(1));
    let usage = server.get("/api/mobile/ai/usage", Some(GOOD)).await;
    assert_eq!(
        usage.pair(),
        (
            200,
            json!({ "success": true, "usage": { "accounts": [], "fetchedAt": "now" } })
        )
    );
}

#[tokio::test]
async fn accounts_are_created_and_removed_only_through_explicit_methods() {
    let server = TestServer::start().await;
    let body = json!({ "provider": "claude", "accountName": "work" });
    let added = server
        .request(
            HttpCall::new("POST", "/api/mobile/ai/accounts")
                .auth(GOOD)
                .json(&body),
        )
        .await;
    assert_eq!(
        added.pair(),
        (201, json!({ "success": true, "accountId": ACCOUNT_ID }))
    );
    assert_eq!(server.fake.calls("add_mobile_ai_account"), vec![body]);
    let path = format!("/api/mobile/ai/accounts/{ACCOUNT_ID}");
    let removed = server
        .request(HttpCall::new("DELETE", &path).auth(GOOD))
        .await;
    assert_eq!(
        removed.pair(),
        (200, json!({ "success": true, "outcome": "ok" }))
    );
}

#[tokio::test]
async fn authentication_precedes_every_account_mutation() {
    let server = TestServer::start().await;
    let call = HttpCall::new("POST", "/api/mobile/ai/accounts")
        .auth("Bearer wrong")
        .json(&json!({}));
    assert_eq!(server.request(call).await.status, 401);
    assert_eq!(server.fake.count("add_mobile_ai_account"), 0);
}

#[tokio::test]
async fn malformed_json_is_400_and_request_errors_are_preserved() {
    let server = TestServer::start().await;
    let malformed = server
        .request(
            HttpCall::new("POST", "/api/mobile/ai/accounts")
                .auth(GOOD)
                .body("{"),
        )
        .await;
    assert_eq!(malformed.status, 400);
    server.fake.fails(
        "add_mobile_ai_account",
        BridgeError::Ai {
            status: Some(409),
            message: "중복된 계정".into(),
        },
    );
    let duplicate = server
        .request(
            HttpCall::new("POST", "/api/mobile/ai/accounts")
                .auth(GOOD)
                .json(&json!({})),
        )
        .await;
    assert_eq!(duplicate.pair(), (409, failure("중복된 계정")));
}

#[tokio::test]
async fn internal_failures_are_hidden_and_unknown_routes_are_not_dispatched() {
    let server = TestServer::start().await;
    server.fake.fails(
        "list_mobile_ai_accounts",
        unexpected("secret filesystem path"),
    );
    let listed = server.get("/api/mobile/ai/accounts", Some(GOOD)).await;
    assert_eq!(
        listed.pair(),
        (500, failure("AI 기능 요청을 처리하지 못했습니다"))
    );
    let patched = server
        .request(
            HttpCall::new("PATCH", "/api/mobile/ai/accounts")
                .auth(GOOD)
                .json(&json!({})),
        )
        .await;
    assert_eq!(patched.status, 404);
    assert_eq!(
        server.get("/api/mobile/unknown", Some(GOOD)).await.status,
        404
    );
}

#[tokio::test]
async fn missing_graph_parameters_and_malformed_identifiers_are_rejected_before_readers() {
    let server = TestServer::start().await;
    assert_eq!(
        server
            .get("/api/mobile/tasks/task-1/agent-call-graph", Some(GOOD))
            .await
            .status,
        400
    );
    assert_eq!(
        server
            .get("/api/mobile/tasks/%ZZ/live-ai-sessions", Some(GOOD))
            .await
            .status,
        400
    );
    assert_eq!(server.fake.count("get_mobile_agent_call_graph"), 0);
    assert_eq!(server.fake.count("get_mobile_live_ai_sessions"), 0);
}

#[tokio::test]
async fn login_stream_routes_output_input_resize_and_close_to_its_owner() {
    let server = TestServer::start().await;
    let mut client = open_login(&server, GOOD).await;
    let opened = &server.fake.calls("open_mobile_ai_login")[0];
    assert_eq!(
        [&opened[0], &opened[2], &opened[3], &opened[4]],
        [
            &json!("device-good"),
            &json!(ACCOUNT_ID),
            &json!(80),
            &json!(24)
        ]
    );
    let socket_id = socket_id(&server, 0);
    assert!(socket_id.is_string());
    server
        .fake
        .ai_login(0)
        .on_event(json!({ "type": "output", "data": "OAuth code:" }));
    assert_eq!(
        client.next_json().await,
        json!({ "type": "output", "data": "OAuth code:" })
    );
    client
        .send_json(&json!({ "type": "input", "data": "123\r" }))
        .await;
    client
        .send_json(&json!({ "type": "resize", "cols": 90, "rows": 30 }))
        .await;
    wait_until(|| server.fake.count("resize_mobile_ai_login") == 1).await;
    assert_eq!(
        server.fake.calls("write_mobile_ai_login"),
        vec![json!(["device-good", socket_id, "123\r"])]
    );
    assert_eq!(
        server.fake.calls("resize_mobile_ai_login"),
        vec![json!(["device-good", socket_id, 90, 30])]
    );
    client.close().await;
    wait_until(|| server.fake.count("close_mobile_ai_login") == 1).await;
    assert_eq!(
        server.fake.calls("close_mobile_ai_login"),
        vec![json!(["device-good", socket_id])]
    );
}

#[tokio::test]
async fn ready_is_not_sent_after_a_disconnect_while_the_account_lookup_is_pending() {
    let server = TestServer::start().await;
    let gate = Gate::new();
    let held = gate.clone();
    server
        .fake
        .respond("open_mobile_ai_login", move |_| Ok(held.wait()));
    let mut client = server
        .websocket(&login_path(), Some(GOOD))
        .await
        .expect("로그인 소켓");
    wait_until(|| server.fake.count("open_mobile_ai_login") == 1).await;
    client.close().await;
    wait_until(|| !server.fake.ai_login(0).is_open()).await;
    gate.open(Value::Null);
    wait_until(|| server.fake.count("close_mobile_ai_login") == 1).await;
    assert!(!server.fake.ai_login(0).is_open());
}

#[tokio::test]
async fn invalid_oversized_and_binary_frames_close_without_running_terminal_input() {
    let server = TestServer::start().await;
    let invalid_resize = json!({ "type": "resize", "cols": -1, "rows": 0 }).to_string();
    let frames = [
        "{".to_string(),
        json!({ "type": "run-shell", "command": "bad" }).to_string(),
        invalid_resize,
        "x".repeat(65537),
    ];
    let rejection = json!({ "type": "error", "error": "터미널 입력 형식이 올바르지 않습니다" });
    for frame in frames {
        let mut client = open_login(&server, GOOD).await;
        client.send_text(&frame).await;
        assert_eq!(client.next_json().await, rejection);
        assert_eq!(
            client.closed().await,
            (1008, "invalid-login-input".to_string())
        );
    }
    let mut binary = open_login(&server, GOOD).await;
    binary
        .send_binary(br#"{"type":"input","data":"bad"}"#.to_vec())
        .await;
    assert_eq!(binary.next_json().await, rejection);
    assert_eq!(
        binary.closed().await,
        (1008, "invalid-login-input".to_string())
    );
    assert_eq!(server.fake.count("write_mobile_ai_login"), 0);
}

/// TS의 출력 적체(`bufferedAmount` > 2 MiB → 1009) 갈래는 Rust 보내기 줄에 적체 값이 없어 옮기지 못했다
#[tokio::test]
async fn login_exit_closes_the_socket_and_cleans_the_process_immediately() {
    let server = TestServer::start().await;
    let mut client = open_login(&server, GOOD).await;
    server
        .fake
        .ai_login(0)
        .on_event(json!({ "type": "exit", "exitCode": 0 }));
    assert_eq!(
        client.next_json().await,
        json!({ "type": "exit", "exitCode": 0 })
    );
    assert_eq!(client.closed().await, (1000, "login-finished".to_string()));
    wait_until(|| server.fake.count("close_mobile_ai_login") == 1).await;
}

#[tokio::test]
async fn login_over_a_real_websocket_streams_both_ways_and_releases_on_disconnect() {
    let server = TestServer::start().await;
    let mut client = open_login(&server, GOOD).await;
    server
        .fake
        .ai_login(0)
        .on_event(json!({ "type": "output", "data": "paste verification code" }));
    assert_eq!(
        client.next_json().await,
        json!({ "type": "output", "data": "paste verification code" })
    );
    client
        .send_json(&json!({ "type": "input", "data": "test-code\r" }))
        .await;
    wait_until(|| server.fake.count("write_mobile_ai_login") == 1).await;
    assert_eq!(
        server.fake.calls("write_mobile_ai_login")[0],
        json!(["device-good", socket_id(&server, 0), "test-code\r"])
    );
    client.close().await;
    wait_until(|| server.fake.count("close_mobile_ai_login") == 1).await;
    assert_eq!(server.websocket(&login_path(), None).await.err(), Some(401));
    assert_eq!(server.fake.count("open_mobile_ai_login"), 1);
}

#[tokio::test]
async fn ai_http_and_login_socket_are_protected_before_provider_calls() {
    let server = TestServer::start().await;
    assert_eq!(
        server
            .get("/api/mobile/tasks/task/live-ai-sessions", None)
            .await
            .status,
        401
    );
    assert_eq!(server.fake.count("get_mobile_live_ai_sessions"), 0);
    assert_eq!(
        server
            .websocket(&login_path(), Some("Bearer bad"))
            .await
            .err(),
        Some(401)
    );
    assert_eq!(server.fake.count("open_mobile_ai_login"), 0);
}

#[tokio::test]
async fn login_socket_uses_the_authenticated_device_and_owns_cleanup() {
    let server = TestServer::start().await;
    let mut client = open_login(&server, SECOND).await;
    let opened = &server.fake.calls("open_mobile_ai_login")[0];
    assert_eq!(
        (&opened[0], &opened[2], &opened[3], &opened[4]),
        (
            &json!("device-second"),
            &json!(ACCOUNT_ID),
            &json!(80),
            &json!(24)
        )
    );
    client
        .send_json(&json!({ "type": "input", "data": "login\r" }))
        .await;
    client
        .send_json(&json!({ "type": "resize", "cols": 90, "rows": 30 }))
        .await;
    wait_until(|| server.fake.count("resize_mobile_ai_login") == 1).await;
    let socket_id = socket_id(&server, 0);
    assert_eq!(
        server.fake.calls("write_mobile_ai_login"),
        vec![json!(["device-second", socket_id, "login\r"])]
    );
    assert_eq!(
        server.fake.calls("resize_mobile_ai_login"),
        vec![json!(["device-second", socket_id, 90, 30])]
    );
    client.close().await;
    wait_until(|| {
        server.fake.calls("close_mobile_ai_login") == vec![json!(["device-second", socket_id])]
    })
    .await;
}

#[tokio::test]
async fn unpairing_closes_only_that_device_while_shutdown_closes_every_login() {
    let server = TestServer::start().await;
    let mut first = open_login(&server, GOOD).await;
    let mut second = open_login(&server, SECOND).await;
    server.fake.notify_unpaired("device-good");
    assert_eq!(first.closed().await.0, 4001);
    assert_eq!(
        server.fake.calls("close_mobile_ai_logins_for_device"),
        vec![json!("device-good")]
    );
    server.running.shutdown();
    assert_eq!(second.closed().await.0, 1001);
    wait_until(|| server.fake.count("close_all_mobile_ai_logins") == 1).await;
}
