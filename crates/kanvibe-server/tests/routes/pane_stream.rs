//! TS `mobileRoutes.test.mjs`: pane 스트림 업그레이드, 키 입력 순서, 서버 쪽 스트림 종료, 연결 해제와 열린 스트림,
//! 세션이 바뀐 태스크의 스트림

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use crate::harness::*;

fn binary(value: &Value) -> Vec<u8> {
    value.to_string().into_bytes()
}

fn written_inputs(server: &TestServer) -> Vec<Value> {
    server
        .fake
        .calls("write_to_pane")
        .into_iter()
        .map(|args| args[2].clone())
        .collect()
}

/// 구독이 붙을 때까지 기다린 스트림
async fn subscribed_stream(
    server: &TestServer,
    authorization: &str,
    expected_sinks: usize,
) -> WsClient {
    let client = server.pane_stream(authorization).await;
    wait_until(|| server.fake.pane_sinks.count() == expected_sinks).await;
    client
}

// parity: TS-electron-mobileRoutes-f2401e
#[tokio::test]
async fn upgrades_outside_the_stream_paths_are_cut_without_a_reply() {
    let server = TestServer::start().await;
    let received = server.raw("GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n").await;
    assert!(
        received.is_empty(),
        "{}",
        String::from_utf8_lossy(&received)
    );
    assert_eq!(server.fake.count("authorize_mobile_request"), 0);
}

/// TS와 같은 헤더 없는 상태 줄만 쓰고 끊는다. 인증이 쿼리 검사보다 먼저다
// parity: TS-electron-mobileRoutes-4abf56
#[tokio::test]
async fn missing_task_or_pane_ids_are_cut_with_400() {
    let server = TestServer::start().await;
    let request = |authorization: &str| {
        format!(
            "GET /api/mobile/stream?taskId=task-1 HTTP/1.1\r\nHost: 127.0.0.1\r\n{authorization}Connection: Upgrade\r\nUpgrade: websocket\r\n\r\n"
        )
    };
    let received = server.raw(&request("Authorization: Bearer good\r\n")).await;
    assert_eq!(
        String::from_utf8_lossy(&received),
        "HTTP/1.1 400 Bad Request\r\n\r\n"
    );
    let unauthorized = server.raw(&request("")).await;
    assert_eq!(
        String::from_utf8_lossy(&unauthorized),
        "HTTP/1.1 401 Unauthorized\r\n\r\n"
    );
}

// parity: TS-electron-mobileRoutes-167c07
#[tokio::test]
async fn leaving_before_the_subscription_attaches_releases_it_immediately() {
    let server = TestServer::start().await;
    let gate = Gate::new();
    let held = gate.clone();
    server
        .fake
        .respond("subscribe_to_pane", move |_| Ok(held.wait()));
    let mut client = server.pane_stream(GOOD).await;
    wait_until(|| server.fake.count("subscribe_to_pane") == 1).await;
    client.close().await;
    wait_until(|| server.fake.session_watchers.count() == 0).await;
    gate.open(Value::Null);
    wait_until(|| server.fake.count("pane_unsubscribe") == 1).await;
    assert_eq!(server.fake.pane_sinks.count(), 0);
}

// parity: TS-electron-mobileRoutes-264153
#[tokio::test]
async fn the_resized_screen_is_sent_again_to_keep_the_latest_output() {
    let server = TestServer::start().await;
    server
        .fake
        .returns("viewport.resize", json!("fresh-history-through-180"));
    let mut client = server.pane_stream(GOOD).await;
    client
        .send_binary(binary(&json!({ "type": "resize", "cols": 47, "rows": 48 })))
        .await;
    assert_eq!(client.next_text().await, "fresh-history-through-180");
    assert_eq!(server.fake.calls("viewport.resize"), vec![json!([47, 48])]);
}

// parity: TS-electron-mobileRoutes-e3a447
#[tokio::test]
async fn only_binary_resize_changes_size_and_the_same_json_as_text_is_typed() {
    let server = TestServer::start().await;
    let mut client = server.pane_stream(GOOD).await;
    let message = json!({ "type": "resize", "cols": 44, "rows": 70 });
    client.send_binary(binary(&message)).await;
    client.send_text(&message.to_string()).await;
    wait_until(|| server.fake.count("write_to_pane") > 0).await;
    assert_eq!(
        server.fake.calls("claim_pane_viewport"),
        vec![json!(["task-1", "pane-1"])]
    );
    assert_eq!(server.fake.calls("viewport.resize"), vec![json!([44, 70])]);
    assert_eq!(
        server.fake.calls("write_to_pane"),
        vec![json!(["task-1", "pane-1", message.to_string()])]
    );
    client.close().await;
    wait_until(|| server.fake.count("viewport.release") == 1).await;
}

// parity: TS-electron-mobileRoutes-0cf1e2
#[tokio::test]
async fn binary_wheel_goes_to_the_claimed_viewport_and_is_never_typed() {
    let server = TestServer::start().await;
    let mut client = server.pane_stream(GOOD).await;
    let wheel = json!({ "type": "scroll", "direction": "up", "column": 4, "row": 2 });
    client.send_binary(binary(&wheel)).await;
    client
        .send_binary(binary(&json!({ "type": "resize", "cols": 44, "rows": 70 })))
        .await;
    client.send_binary(binary(&wheel)).await;
    client.send_text("done").await;
    wait_until(|| server.fake.count("write_to_pane") > 0).await;
    // 크기 요청 전의 휠은 잡은 viewport가 없어 보낼 곳이 없다
    assert_eq!(server.fake.calls("viewport.scroll"), vec![wheel]);
    assert_eq!(written_inputs(&server), vec![json!("done")]);
}

// parity: TS-electron-mobileRoutes-c931d2
#[tokio::test]
async fn screens_returned_by_the_wheel_are_sent_to_that_device() {
    let server = TestServer::start().await;
    server
        .fake
        .returns("viewport.scroll", json!("copy-mode-screen"));
    let mut client = server.pane_stream(GOOD).await;
    client
        .send_binary(binary(&json!({ "type": "resize", "cols": 44, "rows": 70 })))
        .await;
    client
        .send_binary(binary(
            &json!({ "type": "scroll", "direction": "up", "column": 0, "row": 0 }),
        ))
        .await;
    assert_eq!(client.next_text().await, "copy-mode-screen");
}

// parity: TS-electron-mobileRoutes-534b19
#[tokio::test]
async fn binary_history_refresh_goes_to_the_mirror_only() {
    let server = TestServer::start().await;
    let mut client = server.pane_stream(GOOD).await;
    client
        .send_binary(binary(&json!({ "type": "refresh-history" })))
        .await;
    client.send_text("done").await;
    wait_until(|| server.fake.count("write_to_pane") > 0).await;
    assert_eq!(
        server.fake.calls("refresh_pane_history"),
        vec![json!(["task-1", "pane-1"])]
    );
    assert_eq!(server.fake.count("claim_pane_viewport"), 0);
    assert_eq!(written_inputs(&server), vec![json!("done")]);
}

// parity: TS-electron-mobileRoutes-df122f
#[tokio::test]
async fn invalid_binary_sizes_and_json_are_never_typed() {
    let server = TestServer::start().await;
    let mut client = server.pane_stream(GOOD).await;
    client.send_binary(b"broken-json".to_vec()).await;
    for cols in [json!(0), json!(-1), json!(1.5), json!(501), json!("44")] {
        client
            .send_binary(binary(
                &json!({ "type": "resize", "cols": cols, "rows": 70 }),
            ))
            .await;
    }
    client.send_text("done").await;
    wait_until(|| server.fake.count("write_to_pane") > 0).await;
    assert_eq!(server.fake.count("claim_pane_viewport"), 0);
    assert_eq!(written_inputs(&server), vec![json!("done")]);
}

// parity: TS-electron-mobileRoutes-e0f776
#[tokio::test]
async fn the_next_key_waits_for_the_previous_write() {
    let server = TestServer::start().await;
    let pending: Arc<Mutex<Vec<Arc<Gate>>>> = Arc::default();
    let completed: Arc<Mutex<Vec<String>>> = Arc::default();
    let (pending_writes, completed_writes) = (pending.clone(), completed.clone());
    server.fake.respond("write_to_pane", move |args| {
        let gate = Gate::new();
        pending_writes.lock().expect("대기 목록").push(gate.clone());
        gate.wait();
        completed_writes
            .lock()
            .expect("완료 목록")
            .push(args[2].as_str().unwrap_or_default().to_string());
        Ok(Value::Null)
    });
    let mut client = server.pane_stream(GOOD).await;
    client.send_text("l").await;
    client.send_text("s").await;
    wait_until(|| pending.lock().expect("대기 목록").len() == 1).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(server.fake.count("write_to_pane"), 1);
    pending.lock().expect("대기 목록")[0].open(Value::Null);
    wait_until(|| pending.lock().expect("대기 목록").len() == 2).await;
    pending.lock().expect("대기 목록")[1].open(Value::Null);
    wait_until(|| completed.lock().expect("완료 목록").len() == 2).await;
    assert_eq!(*completed.lock().expect("완료 목록"), vec!["l", "s"]);
}

// parity: TS-electron-mobileRoutes-0800b8
#[tokio::test]
async fn a_failed_write_does_not_block_the_next_key() {
    let server = TestServer::start().await;
    server
        .fake
        .respond("write_to_pane", |args| match args[2].as_str() {
            Some("l") => Err(unexpected("send-keys 실패")),
            _ => Ok(Value::Null),
        });
    let mut client = server.pane_stream(GOOD).await;
    client.send_text("l").await;
    client.send_text("s").await;
    wait_until(|| server.fake.count("write_to_pane") == 2).await;
    assert_eq!(written_inputs(&server), vec![json!("l"), json!("s")]);
}

// parity: TS-electron-mobileRoutes-d54737
#[tokio::test]
async fn a_dying_server_stream_closes_with_the_subscription_failure_code() {
    let server = TestServer::start().await;
    let mut client = subscribed_stream(&server, GOOD, 1).await;
    server.fake.end_pane_streams();
    assert_eq!(client.closed().await, (4000, "stream-failed".to_string()));
}

// parity: TS-electron-mobileRoutes-bd961c
#[tokio::test]
async fn streams_of_an_unpaired_device_close_with_the_unpair_code() {
    let server = TestServer::start().await;
    let mut client = subscribed_stream(&server, GOOD, 1).await;
    server.fake.notify_unpaired("device-good");
    assert_eq!(client.closed().await, (4001, "device-unpaired".to_string()));
}

// parity: TS-electron-mobileRoutes-c73707
#[tokio::test]
async fn another_device_watching_the_same_pane_keeps_streaming() {
    let server = TestServer::start().await;
    let mut removed = subscribed_stream(&server, GOOD, 1).await;
    let mut kept = subscribed_stream(&server, SECOND, 2).await;
    server.fake.notify_unpaired("device-good");
    assert_eq!(removed.closed().await.0, 4001);
    server.fake.emit_pane_chunk("남은 기기가 볼 출력");
    assert_eq!(kept.next_text().await, "남은 기기가 볼 출력");
}

// parity: TS-electron-mobileRoutes-7b7e1c
#[tokio::test]
async fn an_unpair_signal_for_an_unknown_device_closes_nothing() {
    let server = TestServer::start().await;
    let mut client = subscribed_stream(&server, GOOD, 1).await;
    server.fake.notify_unpaired("device-unknown");
    server.fake.emit_pane_chunk("still-open");
    assert_eq!(client.next_text().await, "still-open");
}

/// TS는 서버 소켓의 `close` 스파이로 확인했다. Rust 소켓 지도는 밖에서 볼 수 없어, 닫힌 소켓의 정리(구독·세션 감시 해제)가
/// 끝난 뒤에 온 해제 신호가 같은 기기의 살아 있는 소켓을 그대로 해제 코드로 닫는 것까지만 확인한다
// parity: TS-electron-mobileRoutes-7960f9
#[tokio::test]
async fn normally_closed_sockets_leave_the_device_map() {
    let server = TestServer::start().await;
    let mut closed = subscribed_stream(&server, GOOD, 1).await;
    closed.close().await;
    wait_until(|| server.fake.session_watchers.count() == 0 && server.fake.pane_sinks.count() == 0)
        .await;
    let mut current = subscribed_stream(&server, GOOD, 1).await;
    server.fake.notify_unpaired("device-good");
    assert_eq!(
        current.closed().await,
        (4001, "device-unpaired".to_string())
    );
}

// parity: TS-electron-mobileRoutes-eccd4f
#[tokio::test]
async fn a_changed_task_session_closes_the_stream_with_the_session_changed_code() {
    let server = TestServer::start().await;
    let mut client = subscribed_stream(&server, GOOD, 1).await;
    server.fake.notify_task_session_changed("task-1");
    assert_eq!(client.closed().await, (4002, "session-changed".to_string()));
}

// parity: TS-electron-mobileRoutes-fddfe7
#[tokio::test]
async fn other_task_session_changes_are_ignored_and_closed_streams_stop_watching() {
    let server = TestServer::start().await;
    let mut client = subscribed_stream(&server, GOOD, 1).await;
    server.fake.notify_task_session_changed("task-2");
    server.fake.emit_pane_chunk("still-open");
    assert_eq!(client.next_text().await, "still-open");
    client.close().await;
    wait_until(|| server.fake.session_watchers.count() == 0).await;
}
