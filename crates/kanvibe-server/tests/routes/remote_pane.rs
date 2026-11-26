//! TS `mobileRoutes.test.mjs` "원격 pane 지속 연결 경로"
//!
//! TS는 핸드셰이크 뒤 다시 인증하는 동안 받은 프레임을 메모리에 쌓았다(1 MiB·256개를 넘으면 1009). Rust는 다시 인증이
//! 끝날 때까지 소켓을 읽지 않아 프레임이 TCP 버퍼에 남는다. 관찰되는 결과(승인 뒤에만 전달, 거절이면 실행 안 함)는 같다.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use kanvibe_server::bridge::BridgeError;
use serde_json::{Value, json};

use crate::harness::*;

const CONNECTION_PATH: &str = "/api/mobile/tasks/task-1/panes/w1%3Ap1/connection";
const REMOTE_STREAM_PATH: &str = "/api/mobile/stream?taskId=task-1&paneId=w1%3Ap1";

fn grant() -> Value {
    json!({
        "version": 1, "grantId": "grant-one", "expiresAt": 2_000_000_000_000_u64,
        "direct": { "host": "b.example", "port": 19443, "path": "/stream/grant-one", "token": "B-only-capability", "certificateFingerprint": "b".repeat(64) },
    })
}

fn connection_call() -> HttpCall {
    HttpCall::new("POST", CONNECTION_PATH).auth(GOOD)
}

/// 핸드셰이크의 첫 인증만 통과시키고, 핸드셰이크 뒤의 다시 인증은 `later`가 정한다
fn authorize_once_then(server: &TestServer, later: impl Fn() -> Value + Send + Sync + 'static) {
    let calls = AtomicUsize::new(0);
    server.fake.respond("authorize_mobile_request", move |_| {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Ok(json!("device-good"));
        }
        Ok(later())
    });
}

#[tokio::test]
async fn only_authenticated_tls_requests_get_the_scoped_b_connection() {
    let server = TestServer::start().await;
    server
        .fake
        .returns("prepare_mobile_pane_connection", grant());
    let reply = server.request_tls(connection_call()).await;
    assert_eq!(
        reply.pair(),
        (200, json!({ "success": true, "connection": grant() }))
    );
    // 접속 토큰이 중간 캐시에 남지 않게 한다(TS `mobileRoutes.js`의 `Cache-Control: no-store`)
    assert_eq!(reply.header("cache-control"), Some("no-store"));
    assert_eq!(
        server.fake.calls("prepare_mobile_pane_connection"),
        vec![json!(["task-1", "w1:p1", "device-good"])]
    );
}

#[tokio::test]
async fn plaintext_connections_never_get_a_b_token() {
    let server = TestServer::start().await;
    server
        .fake
        .returns("prepare_mobile_pane_connection", grant());
    let reply = server.request(connection_call()).await;
    assert_eq!(
        reply.pair(),
        (200, json!({ "success": true, "connection": null }))
    );
    assert_eq!(server.fake.count("prepare_mobile_pane_connection"), 0);
}

#[tokio::test]
async fn devices_unpaired_while_preparing_get_no_b_capability() {
    let server = TestServer::start().await;
    let is_revoked = Arc::new(AtomicBool::new(false));
    let revoked = is_revoked.clone();
    server.fake.respond("authorize_mobile_request", move |_| {
        Ok(if revoked.load(Ordering::SeqCst) {
            Value::Null
        } else {
            json!("device-good")
        })
    });
    server
        .fake
        .respond("prepare_mobile_pane_connection", move |_| {
            is_revoked.store(true, Ordering::SeqCst);
            Ok(grant())
        });
    let reply = server.request_tls(connection_call()).await;
    assert_eq!(reply.status, 401);
    assert!(reply.json().get("connection").is_none());
}

#[tokio::test]
async fn invalid_pane_connections_answer_the_session_error() {
    let server = TestServer::start().await;
    server.fake.fails(
        "prepare_mobile_pane_connection",
        BridgeError::SurfaceUnavailable("session-not-running".into()),
    );
    let reply = server.request_tls(connection_call()).await;
    assert_eq!(
        reply.pair(),
        (
            409,
            json!({ "success": false, "reason": "session-not-running" })
        )
    );
}

#[tokio::test]
async fn devices_unpaired_during_the_relay_handshake_are_reauthorized_and_closed() {
    let server = TestServer::start().await;
    authorize_once_then(&server, || Value::Null);
    let mut client = server
        .websocket(REMOTE_STREAM_PATH, Some(GOOD))
        .await
        .expect("핸드셰이크");
    assert_eq!(client.closed().await, (4001, "device-unpaired".to_string()));
    assert_eq!(server.fake.count("authorize_mobile_request"), 2);
}

#[tokio::test]
async fn keys_received_while_reauthorizing_reach_the_relay_only_after_approval() {
    let server = TestServer::start().await;
    let gate = Gate::new();
    let held = gate.clone();
    authorize_once_then(&server, move || held.wait());
    server
        .fake
        .returns("open_remote_mobile_pane_stream", json!(true));
    let mut client = server
        .websocket(REMOTE_STREAM_PATH, Some(GOOD))
        .await
        .expect("핸드셰이크");
    client.send_text("pending-input").await;
    wait_until(|| server.fake.count("authorize_mobile_request") == 2).await;
    let untouched = [
        "open_remote_mobile_pane_stream",
        "subscribe_to_pane",
        "relay.write",
    ];
    assert!(untouched.iter().all(|name| server.fake.count(name) == 0));
    gate.open(json!("device-good"));
    wait_until(|| server.fake.count("relay.write") == 1).await;
    assert_eq!(
        server.fake.calls("relay.write"),
        vec![json!(["pending-input", false])]
    );
    client.close().await;
}

#[tokio::test]
async fn commands_of_a_device_unpaired_while_reauthorizing_never_run() {
    let server = TestServer::start().await;
    let gate = Gate::new();
    let held = gate.clone();
    authorize_once_then(&server, move || held.wait());
    let mut client = server
        .websocket("/api/mobile/events", Some(GOOD))
        .await
        .expect("핸드셰이크");
    client.send_json(&json!({ "type": "command", "requestId": "blocked", "command": "start-session", "taskId": "task-1" })).await;
    wait_until(|| server.fake.count("authorize_mobile_request") == 2).await;
    assert_eq!(server.fake.count("get_task_surfaces"), 0);
    gate.open(Value::Null);
    assert_eq!(client.closed().await, (4001, "device-unpaired".to_string()));
    assert_eq!(server.fake.count("get_task_surfaces"), 0);
}

#[tokio::test]
async fn the_b_relay_forwards_input_and_size_frames_without_local_command_paths() {
    let server = TestServer::start().await;
    server
        .fake
        .returns("open_remote_mobile_pane_stream", json!(true));
    let mut client = server
        .websocket(
            &format!("{REMOTE_STREAM_PATH}&grantId=grant-one"),
            Some(GOOD),
        )
        .await
        .expect("핸드셰이크");
    wait_until(|| server.fake.count("open_remote_mobile_pane_stream") == 1).await;
    assert_eq!(
        server.fake.calls("open_remote_mobile_pane_stream"),
        vec![json!(["task-1", "w1:p1", "device-good", "grant-one"])]
    );
    let on_chunk = server.fake.relay_sinks.lock().expect("중계 콜백")[0]
        .0
        .clone();
    on_chunk("B terminal output".to_string());
    assert_eq!(client.next_text().await, "B terminal output");
    let resize = json!({ "type": "resize", "cols": 100, "rows": 30 }).to_string();
    // 휠도 B의 Herdr 제어 연결이 처리하므로 그대로 넘긴다
    let wheel =
        json!({ "type": "scroll", "direction": "down", "column": 0, "row": 29 }).to_string();
    client
        .send_binary(
            json!({ "type": "refresh-history" })
                .to_string()
                .into_bytes(),
        )
        .await;
    client.send_text("ls\n").await;
    client.send_binary(resize.clone().into_bytes()).await;
    client.send_binary(wheel.clone().into_bytes()).await;
    wait_until(|| server.fake.count("relay.write") == 3).await;
    assert_eq!(
        server.fake.calls("relay.write"),
        vec![
            json!(["ls\n", false]),
            json!([resize, true]),
            json!([wheel, true])
        ]
    );
    let local_paths = [
        "refresh_pane_history",
        "write_to_pane",
        "claim_pane_viewport",
        "subscribe_to_pane",
    ];
    assert!(local_paths.iter().all(|name| server.fake.count(name) == 0));
    client.close().await;
    wait_until(|| server.fake.count("relay.close") == 1).await;
}

#[tokio::test]
async fn a_relay_that_arrives_after_the_device_left_is_closed_at_once() {
    let server = TestServer::start().await;
    let gate = Gate::new();
    let held = gate.clone();
    server
        .fake
        .respond("open_remote_mobile_pane_stream", move |_| Ok(held.wait()));
    let mut client = server
        .websocket(REMOTE_STREAM_PATH, Some(GOOD))
        .await
        .expect("핸드셰이크");
    wait_until(|| server.fake.count("open_remote_mobile_pane_stream") == 1).await;
    client.close().await;
    wait_until(|| server.fake.session_watchers.count() == 0).await;
    gate.open(json!(true));
    wait_until(|| server.fake.count("relay.close") == 1).await;
    assert_eq!(server.fake.count("relay.write"), 0);
}
