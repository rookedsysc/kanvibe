//! TS `mobileRoutes.test.mjs` "상태 WebSocket 명령"

use kanvibe_server::bridge::BridgeError;
use serde_json::{Value, json};

use crate::harness::*;
use crate::mobile_commands::assert_ignored;

fn surface_unavailable(reason: &str) -> BridgeError {
    BridgeError::SurfaceUnavailable(reason.to_string())
}

// parity: TS-electron-mobileRoutes-b38356
#[tokio::test]
async fn start_session_returns_the_prepared_panes_to_the_requester() {
    let server = TestServer::start().await;
    let surfaces = json!({ "taskId": "task-1", "sessionType": "herdr", "canResize": true, "tabs": [{ "id": "w1:t1", "panes": [{ "id": "w1:p1", "command": "codex" }] }] });
    server.fake.returns("get_task_surfaces", surfaces.clone());
    let mut client = server.events(GOOD).await;
    let result = run_command(
        &mut client,
        json!({ "command": "start-session", "taskId": "task-1" }),
    )
    .await;
    assert_eq!(
        result,
        json!({ "type": "command-result", "requestId": "r1", "ok": true, "surfaces": surfaces })
    );
}

// parity: TS-electron-mobileRoutes-d1d5e8
#[tokio::test]
async fn start_session_rejects_malformed_task_ids() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    for task_id in [json!(""), Value::Null, json!(1)] {
        let result = run_command(
            &mut client,
            json!({ "command": "start-session", "taskId": task_id }),
        )
        .await;
        assert_eq!(
            (&result["ok"], &result["code"]),
            (&json!(false), &json!("invalid-command"))
        );
    }
    assert_eq!(server.fake.count("get_task_surfaces"), 0);
}

// parity: TS-electron-mobileRoutes-061372
#[tokio::test]
async fn start_session_keeps_the_failure_reason_as_the_code() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    for reason in [
        "herdr-missing",
        "remote-unreachable",
        "session-not-running",
        "no-session",
    ] {
        server
            .fake
            .fails("get_task_surfaces", surface_unavailable(reason));
        let result = run_command(
            &mut client,
            json!({ "command": "start-session", "taskId": "task-1" }),
        )
        .await;
        assert_eq!(
            (&result["ok"], &result["code"], &result["error"]),
            (
                &json!(false),
                &json!(reason),
                &json!("세션을 시작하지 못했습니다")
            )
        );
    }
}

// parity: TS-electron-mobileRoutes-20226a
#[tokio::test]
async fn connected_message_lists_the_supported_commands() {
    let server = TestServer::start().await;
    let mut client = server
        .websocket("/api/mobile/events", Some(GOOD))
        .await
        .expect("이벤트 소켓");
    let commands = [
        "delete-task",
        "remove-task-registration",
        "get-tab-close-info",
        "close-tab",
        "start-session",
        "set-pane-chat-subscription",
        "get-pane-chat-modes",
        "set-pane-chat-mode",
    ];
    assert_eq!(
        client.next_json().await,
        json!({ "type": "connected", "commands": commands })
    );
}

// parity: TS-electron-mobileRoutes-3e45ec
#[tokio::test]
async fn delete_results_go_only_to_the_requesting_socket() {
    let server = TestServer::start().await;
    let mut requester = server.events(GOOD).await;
    let mut bystander = server.events(SECOND).await;
    let result = run_command(
        &mut requester,
        json!({ "command": "delete-task", "taskId": "task-1", "isForced": false }),
    )
    .await;
    server
        .fake
        .broadcast_event(json!({ "type": "board-updated" }));
    assert_eq!(
        result,
        json!({ "type": "command-result", "requestId": "r1", "ok": true, "failedPeerCount": 0 })
    );
    assert_eq!(
        server.fake.calls("delete_mobile_task"),
        vec![json!(["task-1", false])]
    );
    assert_eq!(
        bystander.next_json().await,
        json!({ "type": "board-updated" })
    );
}

/// 두 명령은 서로 기다리지 않아 서비스 호출 순서가 정해지지 않는다. 그래서 강제 여부로 실패를 고르고 호출은 집합으로 본다
// parity: TS-electron-mobileRoutes-b2212a
#[tokio::test]
async fn dirty_worktrees_answer_dirty_worktree_and_forced_deletion_passes_is_forced() {
    let server = TestServer::start().await;
    server
        .fake
        .respond("delete_mobile_task", |args| match args[1].as_bool() {
            Some(true) => Ok(json!(0)),
            _ => Err(BridgeError::DirtyWorktree("dirty".into())),
        });
    let mut client = server.events(GOOD).await;
    client.send_json(&json!({ "type": "command", "requestId": "first", "command": "delete-task", "taskId": "task-1" })).await;
    client.send_json(&json!({ "type": "command", "requestId": "forced", "command": "delete-task", "taskId": "task-1", "isForced": true })).await;
    let mut results = [client.next_json().await, client.next_json().await];
    results.sort_by_key(|result| result["requestId"].as_str().map(str::to_string));
    assert_eq!(
        (&results[0]["requestId"], &results[0]["code"]),
        (&json!("first"), &json!("dirty-worktree"))
    );
    assert_eq!(
        (&results[1]["requestId"], &results[1]["ok"]),
        (&json!("forced"), &json!(true))
    );
    let mut calls = server.fake.calls("delete_mobile_task");
    calls.sort_by_key(Value::to_string);
    assert_eq!(
        calls,
        vec![json!(["task-1", false]), json!(["task-1", true])]
    );
}

// parity: TS-electron-mobileRoutes-8fd06b
#[tokio::test]
async fn deleting_a_missing_task_answers_task_not_found() {
    let server = TestServer::start().await;
    server.fake.returns("delete_mobile_task", Value::Null);
    let mut client = server.events(GOOD).await;
    let result = run_command(
        &mut client,
        json!({ "command": "delete-task", "taskId": "missing" }),
    )
    .await;
    assert_eq!(
        (&result["ok"], &result["code"]),
        (&json!(false), &json!("task-not-found"))
    );
}

// parity: TS-electron-mobileRoutes-316a27
#[tokio::test]
async fn close_tab_passes_only_the_named_pane_and_returns_the_remaining_surfaces() {
    let server = TestServer::start().await;
    let surfaces =
        json!({ "taskId": "task-1", "sessionType": "tmux", "canResize": true, "tabs": [] });
    server.fake.returns("close_mobile_pane", surfaces.clone());
    let mut client = server.events(GOOD).await;
    let result = run_command(
        &mut client,
        json!({ "command": "close-tab", "taskId": "task-1", "paneId": "%20" }),
    )
    .await;
    assert_eq!(
        result,
        json!({ "type": "command-result", "requestId": "r1", "ok": true, "surfaces": surfaces })
    );
    assert_eq!(
        server.fake.calls("close_mobile_pane"),
        vec![json!(["task-1", "%20"])]
    );
}

// parity: TS-electron-mobileRoutes-531c7a
#[tokio::test]
async fn close_tab_errors_map_to_their_codes() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    let cases = [
        (BridgeError::LastPane("LastPaneError".into()), "last-tab"),
        (
            BridgeError::PaneNotFound("PaneNotFoundError".into()),
            "pane-not-found",
        ),
        (surface_unavailable("no-session"), "session-unavailable"),
    ];
    for (error, code) in cases {
        server.fake.fails("close_mobile_pane", error);
        let result = run_command(
            &mut client,
            json!({ "command": "close-tab", "taskId": "task-1", "paneId": "%20" }),
        )
        .await;
        assert_eq!(
            (&result["ok"], &result["code"]),
            (&json!(false), &json!(code))
        );
    }
}

// parity: TS-electron-mobileRoutes-d51eda
#[tokio::test]
async fn unexpected_command_errors_hide_the_internal_reason() {
    let server = TestServer::start().await;
    server.fake.fails(
        "close_mobile_pane",
        unexpected("tmux: /private/path failed"),
    );
    let mut client = server.events(GOOD).await;
    let result = run_command(
        &mut client,
        json!({ "command": "close-tab", "taskId": "task-1", "paneId": "%20" }),
    )
    .await;
    assert_eq!(
        (&result["ok"], &result["code"]),
        (&json!(false), &json!("server-error"))
    );
    assert!(!result.to_string().contains("/private/path"));
}

// parity: TS-electron-mobileRoutes-59d594
#[tokio::test]
async fn tab_close_info_returns_running_process_names() {
    let server = TestServer::start().await;
    server.fake.returns(
        "get_mobile_pane_close_info",
        json!({ "processNames": ["vim"] }),
    );
    let mut client = server.events(GOOD).await;
    let result = run_command(
        &mut client,
        json!({ "command": "get-tab-close-info", "taskId": "task-1", "paneId": "%20" }),
    )
    .await;
    assert_eq!(
        result,
        json!({ "type": "command-result", "requestId": "r1", "ok": true, "processNames": ["vim"] })
    );
}

// parity: TS-electron-mobileRoutes-dbdf4b
#[tokio::test]
async fn unknown_commands_are_answered_and_request_id_less_messages_keep_the_socket_open() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    assert_ignored(
        &mut client,
        &json!({ "type": "command", "command": "delete-task", "taskId": "task-1" }).to_string(),
    )
    .await;
    let unknown = run_command(
        &mut client,
        json!({ "command": "rename-task", "taskId": "task-1" }),
    )
    .await;
    assert_eq!(
        (&unknown["ok"], &unknown["code"]),
        (&json!(false), &json!("unsupported-command"))
    );
    assert_eq!(server.fake.count("delete_mobile_task"), 0);
}

// parity: TS-electron-mobileRoutes-454f70
#[tokio::test]
async fn binary_frames_are_not_read_as_commands() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    let command = json!({ "type": "command", "requestId": "bin", "command": "delete-task", "taskId": "task-1" });
    client.send_binary(command.to_string().into_bytes()).await;
    let after = run_command(
        &mut client,
        json!({ "requestId": "after", "command": "rename-task", "taskId": "task-1" }),
    )
    .await;
    assert_eq!(after["requestId"], "after");
    assert_eq!(server.fake.count("delete_mobile_task"), 0);
}
