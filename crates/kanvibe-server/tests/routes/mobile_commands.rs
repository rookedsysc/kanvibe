//! TS `mobileCommands.test.mjs`.
//!
//! TS는 `runMobileCommand`를 직접 불러 돌려준 값(또는 `null`)을 봤다. Rust의 명령 처리기는 이벤트 소켓 안에 있으므로
//! 실제 `/api/mobile/events` 소켓으로 보내고 받은 결과를 본다. `null`(응답할 길이 없음)은 뒤이어 보낸 확인용 명령의
//! 결과가 다음 메시지로 오는 것으로 확인한다. 테스트 런타임은 단일 스레드라 앞 메시지의 처리가 먼저 끝난다.

use kanvibe_server::bridge::BridgeError;
use serde_json::{Value, json};

use crate::harness::*;

const SUPPORTED: [&str; 8] = [
    "delete-task",
    "remove-task-registration",
    "get-tab-close-info",
    "close-tab",
    "start-session",
    "set-pane-chat-subscription",
    "get-pane-chat-modes",
    "set-pane-chat-mode",
];

/// 응답이 없어야 하는 메시지 다음에 확인용 명령을 보내 그 결과가 곧바로 오는지 본다
pub async fn assert_ignored(client: &mut WsClient, raw: &str) {
    client.send_text(raw).await;
    client
        .send_json(&json!({ "type": "command", "requestId": "probe", "command": "rename-task" }))
        .await;
    let next = client.next_json().await;
    assert_eq!(next["requestId"], "probe", "{raw}에 응답이 왔다: {next}");
}

fn assert_code(result: &Value, code: &str) {
    assert_eq!(
        (
            &result["type"],
            &result["requestId"],
            &result["ok"],
            &result["code"]
        ),
        (
            &json!("command-result"),
            &json!("r1"),
            &json!(false),
            &json!(code)
        ),
        "{result}"
    );
}

fn assert_bridge_untouched(server: &TestServer, names: &[&str]) {
    for name in names {
        assert_eq!(server.fake.count(name), 0, "{name}");
    }
}

#[tokio::test]
async fn registration_removal_is_its_own_command_and_never_deletes_resources() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    let result = run_command(
        &mut client,
        json!({ "command": "remove-task-registration", "taskId": "task-1" }),
    )
    .await;
    assert_eq!(
        result,
        json!({ "type": "command-result", "requestId": "r1", "ok": true, "failedPeerCount": 0 })
    );
    assert_eq!(
        server.fake.calls("remove_mobile_task_registration"),
        vec![json!("task-1")]
    );
    assert_bridge_untouched(&server, &["delete_mobile_task"]);
}

#[tokio::test]
async fn damaged_worktrees_answer_the_registration_removal_hint_code() {
    let server = TestServer::start().await;
    server.fake.fails(
        "delete_mobile_task",
        BridgeError::InvalidWorktree("손상된 저장소".into()),
    );
    let mut client = server.events(GOOD).await;
    let result = run_command(
        &mut client,
        json!({ "command": "delete-task", "taskId": "task-1" }),
    )
    .await;
    assert_code(&result, "invalid-worktree");
}

#[tokio::test]
async fn removing_a_missing_registration_answers_task_not_found() {
    let server = TestServer::start().await;
    server
        .fake
        .returns("remove_mobile_task_registration", json!(false));
    let mut client = server.events(GOOD).await;
    let result = run_command(
        &mut client,
        json!({ "command": "remove-task-registration", "taskId": "missing" }),
    )
    .await;
    assert_code(&result, "task-not-found");
    assert_bridge_untouched(&server, &["delete_mobile_task"]);
}

#[tokio::test]
async fn supported_commands_are_exposed_with_the_connected_event() {
    let server = TestServer::start().await;
    let mut client = server
        .websocket("/api/mobile/events", Some(GOOD))
        .await
        .expect("이벤트 소켓");
    assert_eq!(
        client.next_json().await,
        json!({ "type": "connected", "commands": SUPPORTED })
    );
}

#[tokio::test]
async fn unknown_commands_answer_unsupported_with_the_request_id() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    for name in ["rename-task", "toString", "constructor", "__proto__", ""] {
        let result = run_command(&mut client, json!({ "command": name, "taskId": "task-1" })).await;
        assert_code(&result, "unsupported-command");
    }
    assert_bridge_untouched(&server, &["delete_mobile_task"]);
}

#[tokio::test]
async fn malformed_task_and_pane_fields_answer_invalid_command_without_bridge_calls() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    let cases = [
        json!({ "command": "delete-task" }),
        json!({ "command": "delete-task", "taskId": 7 }),
        json!({ "command": "delete-task", "taskId": "" }),
        json!({ "command": "delete-task", "taskId": "task-1", "isForced": "yes" }),
        json!({ "command": "remove-task-registration" }),
        json!({ "command": "remove-task-registration", "taskId": "" }),
        json!({ "command": "close-tab", "taskId": "task-1" }),
        json!({ "command": "close-tab", "taskId": "task-1", "paneId": "" }),
        json!({ "command": "get-tab-close-info", "taskId": "task-1" }),
    ];
    for fields in cases {
        assert_code(&run_command(&mut client, fields).await, "invalid-command");
    }
    assert_bridge_untouched(
        &server,
        &[
            "delete_mobile_task",
            "close_mobile_pane",
            "get_mobile_pane_close_info",
        ],
    );
}

#[tokio::test]
async fn messages_without_a_valid_request_id_or_command_type_get_no_answer() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    let cases = [
        json!({ "type": "command", "command": "delete-task", "taskId": "task-1" }),
        json!({ "type": "command", "requestId": "", "command": "delete-task", "taskId": "task-1" }),
        json!({ "type": "command", "requestId": 1, "command": "delete-task", "taskId": "task-1" }),
        json!({ "type": "command", "requestId": "a".repeat(65), "command": "delete-task", "taskId": "task-1" }),
        json!({ "type": "ping", "requestId": "r1", "command": "delete-task", "taskId": "task-1" }),
    ];
    for message in cases {
        assert_ignored(&mut client, &message.to_string()).await;
    }
    assert_bridge_untouched(&server, &["delete_mobile_task"]);
}

#[tokio::test]
async fn messages_that_are_not_json_objects_are_ignored() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    for raw in ["not json", "null", "[]", "7", "\"text\""] {
        assert_ignored(&mut client, raw).await;
    }
}

#[tokio::test]
async fn messages_over_1024_bytes_are_ignored_without_parsing() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    let oversized = json!({ "type": "command", "requestId": "r1", "command": "delete-task", "taskId": "t".repeat(1024) });
    assert_ignored(&mut client, &oversized.to_string()).await;
    assert_bridge_untouched(&server, &["delete_mobile_task"]);
}

#[tokio::test]
async fn pane_chat_subscription_passes_the_sending_socket_and_returns_freshness() {
    let server = TestServer::start().await;
    server.fake.returns(
        "set_pane_chat_subscriptions",
        json!([{ "taskId": "task-1", "paneId": "%1", "freshness": "current" }]),
    );
    let mut client = server.events(GOOD).await;
    let chat = json!({ "taskId": "task-1", "paneId": "%1", "detail": "chat", "wantsScreen": true, "cursor": { "epoch": "epoch-a", "seq": 0 } });
    let panes = json!([chat, { "taskId": "task-1", "paneId": "%2", "detail": "agent" }]);
    let result = run_command(
        &mut client,
        json!({ "command": "set-pane-chat-subscription", "panes": panes }),
    )
    .await;
    let expected = json!({ "type": "command-result", "requestId": "r1", "ok": true, "panes": [{ "taskId": "task-1", "paneId": "%1", "freshness": "current" }] });
    assert_eq!(result, expected);
    let call = &server.fake.calls("set_pane_chat_subscriptions")[0];
    let normalized = json!([chat, { "taskId": "task-1", "paneId": "%2", "detail": "agent", "wantsScreen": false, "cursor": null }]);
    assert!(call[0].is_u64(), "구독자는 보낸 소켓이다");
    assert_eq!(call[1], normalized);
}

#[tokio::test]
async fn an_empty_subscription_list_is_passed_through_to_clear_everything() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    let result = run_command(
        &mut client,
        json!({ "command": "set-pane-chat-subscription", "panes": [] }),
    )
    .await;
    assert_eq!(
        result,
        json!({ "type": "command-result", "requestId": "r1", "ok": true, "panes": [] })
    );
    assert_eq!(
        server.fake.calls("set_pane_chat_subscriptions")[0][1],
        json!([])
    );
}

#[tokio::test]
async fn malformed_pane_chat_subscriptions_are_rejected_without_changing_subscriptions() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    let too_many: Vec<Value> = (0..5)
        .map(|index| json!({ "taskId": "t", "paneId": format!("%{index}"), "detail": "agent" }))
        .collect();
    let cases = [
        json!("task-1"),
        Value::Array(too_many),
        json!([{ "taskId": "t", "paneId": "%1", "detail": "all" }]),
        json!([{ "taskId": "t", "paneId": "", "detail": "chat" }]),
        json!([{ "taskId": "t", "paneId": "%1", "detail": "chat", "cursor": { "epoch": "e", "seq": -1 } }]),
        json!([{ "taskId": "t", "paneId": "%1", "detail": "chat", "wantsScreen": "yes" }]),
    ];
    for panes in cases {
        let result = run_command(
            &mut client,
            json!({ "command": "set-pane-chat-subscription", "panes": panes }),
        )
        .await;
        assert_code(&result, "invalid-command");
    }
    assert_bridge_untouched(&server, &["set_pane_chat_subscriptions"]);
}

#[tokio::test]
async fn pane_chat_modes_are_returned_per_pane() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    let result = run_command(
        &mut client,
        json!({ "command": "get-pane-chat-modes", "taskId": "task-1" }),
    )
    .await;
    assert_eq!(
        result,
        json!({ "type": "command-result", "requestId": "r1", "ok": true, "modes": { "%1": true } })
    );
    assert_eq!(
        server.fake.calls("get_pane_chat_modes"),
        vec![json!("task-1")]
    );
}

#[tokio::test]
async fn saving_a_pane_chat_mode_passes_the_values_through_and_answers_empty_success() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    let fields = json!({ "command": "set-pane-chat-mode", "taskId": "task-1", "paneId": "%1", "isEnabled": false });
    let result = run_command(&mut client, fields).await;
    assert_eq!(
        result,
        json!({ "type": "command-result", "requestId": "r1", "ok": true })
    );
    assert_eq!(
        server.fake.calls("set_pane_chat_mode"),
        vec![json!(["task-1", "%1", false])]
    );
}

#[tokio::test]
async fn malformed_pane_chat_mode_commands_answer_invalid_command_without_bridge_calls() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    let cases = [
        json!({ "command": "get-pane-chat-modes" }),
        json!({ "command": "get-pane-chat-modes", "taskId": "" }),
        json!({ "command": "set-pane-chat-mode", "paneId": "%1", "isEnabled": true }),
        json!({ "command": "set-pane-chat-mode", "taskId": "task-1", "isEnabled": true }),
        json!({ "command": "set-pane-chat-mode", "taskId": "task-1", "paneId": "%1" }),
        json!({ "command": "set-pane-chat-mode", "taskId": "task-1", "paneId": "%1", "isEnabled": "true" }),
    ];
    for fields in cases {
        assert_code(&run_command(&mut client, fields).await, "invalid-command");
    }
    assert_bridge_untouched(&server, &["get_pane_chat_modes", "set_pane_chat_mode"]);
}

/// TS의 Buffer 입력은 소켓의 텍스트 프레임이다(이진 프레임은 명령으로 읽지 않는다)
#[tokio::test]
async fn a_64_character_request_id_is_processed() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    let request_id = "r".repeat(64);
    client.send_json(&json!({ "type": "command", "requestId": request_id, "command": "delete-task", "taskId": "task-1" })).await;
    let result = client.next_json().await;
    assert_eq!(
        result,
        json!({ "type": "command-result", "requestId": request_id, "ok": true, "failedPeerCount": 0 })
    );
}
