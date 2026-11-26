//! TS `mobileRoutes.test.mjs`: 상태 알림 경로, 알림함 경로, 알림 상태 설정

use kanvibe_server::bridge::{BridgeError, FeatureCall};
use serde_json::{Value, json};

use crate::harness::*;

fn subscription() -> Value {
    json!({ "pushToken": "fcm-token", "platform": "ios", "connectionId": "connection-1" })
}

fn subscribe(body: &Value) -> HttpCall {
    HttpCall::new("PUT", "/api/mobile/notifications")
        .auth(GOOD)
        .json(body)
}

#[tokio::test]
async fn notification_registration_and_task_lookup_need_pairing() {
    let server = TestServer::start().await;
    let register = server
        .request(HttpCall::new("PUT", "/api/mobile/notifications").json(&subscription()))
        .await;
    let unregister = server
        .request(HttpCall::new("DELETE", "/api/mobile/notifications"))
        .await;
    let task = server.get("/api/mobile/tasks/task-1", None).await;
    assert_eq!(
        [register.status, unregister.status, task.status],
        [401, 401, 401]
    );
    assert_eq!(
        server.fake.count("update_mobile_notification_subscription"),
        0
    );
}

#[tokio::test]
async fn push_tokens_are_registered_and_removed_only_for_the_authenticated_device() {
    let server = TestServer::start().await;
    assert_eq!(server.request(subscribe(&subscription())).await.status, 200);
    assert_eq!(
        server.fake.calls("update_mobile_notification_subscription"),
        vec![json!([GOOD, subscription()])]
    );
    assert_eq!(
        server
            .request(HttpCall::new("DELETE", "/api/mobile/notifications").auth(GOOD))
            .await
            .status,
        200
    );
    assert_eq!(
        server.fake.calls("delete_mobile_notification_subscription"),
        vec![json!(GOOD)]
    );
}

fn with_field(name: &str, value: Value) -> Value {
    let mut body = subscription();
    body[name] = value;
    body
}

#[tokio::test]
async fn malformed_subscriptions_are_not_stored() {
    let server = TestServer::start().await;
    let cases = [
        Value::Null,
        with_field("pushToken", json!("")),
        with_field("platform", json!("web")),
        with_field("connectionId", json!("")),
        with_field("pushToken", json!("a".repeat(4097))),
        with_field("connectionId", json!("🚀".repeat(33))),
    ];
    for body in cases {
        assert_eq!(server.request(subscribe(&body)).await.status, 400, "{body}");
    }
    assert_eq!(
        server.fake.count("update_mobile_notification_subscription"),
        0
    );
}

#[tokio::test]
async fn a_device_unpaired_after_authentication_cannot_register() {
    let server = TestServer::start().await;
    server
        .fake
        .returns("update_mobile_notification_subscription", json!(false));
    assert_eq!(server.request(subscribe(&subscription())).await.status, 401);
}

#[tokio::test]
async fn tasks_outside_the_first_board_page_are_readable_and_missing_ones_are_404() {
    let server = TestServer::start().await;
    let reply = server.get("/api/mobile/tasks/task-1", Some(GOOD)).await;
    let task = json!({ "id": "task-1", "title": "알림 태스크", "projectName": "kanvibe" });
    assert_eq!(
        reply.pair(),
        (200, json!({ "success": true, "task": task }))
    );
    server.fake.returns("get_mobile_task", Value::Null);
    assert_eq!(
        server
            .get("/api/mobile/tasks/missing", Some(GOOD))
            .await
            .status,
        404
    );
}

#[tokio::test]
async fn pane_chat_pushes_reach_only_the_subscriber_and_closing_clears_the_subscription() {
    let server = TestServer::start().await;
    let mut subscriber = server.events(GOOD).await;
    let mut bystander = server.events(GOOD).await;
    let pane = json!({ "taskId": "task-1", "paneId": "%3", "detail": "chat", "wantsScreen": true, "cursor": { "epoch": "epoch-a", "seq": 4 } });
    let fields =
        json!({ "requestId": "sub-1", "command": "set-pane-chat-subscription", "panes": [pane] });
    let result = run_command(&mut subscriber, fields).await;
    let freshness = json!([{ "taskId": "task-1", "paneId": "%3", "freshness": "behind" }]);
    assert_eq!(
        result,
        json!({ "type": "command-result", "requestId": "sub-1", "ok": true, "panes": freshness })
    );
    let call = server.fake.calls("set_pane_chat_subscriptions")[0].clone();
    assert_eq!(call[1], json!([pane]));
    let send = server.fake.pane_chat_senders.lock().expect("구독 목록")[0].clone();
    let pushed = json!({ "type": "pane-chat-rows", "taskId": "task-1", "paneId": "%3", "epoch": "epoch-a", "rows": [{ "seq": 5, "message": { "role": "assistant", "fullText": "새 답변" } }] });
    send(pushed.clone());
    assert_eq!(subscriber.next_json().await, pushed);
    server
        .fake
        .broadcast_event(json!({ "type": "board-updated" }));
    assert_eq!(
        bystander.next_json().await,
        json!({ "type": "board-updated" })
    );
    subscriber.close().await;
    wait_until(|| server.fake.count("clear_pane_chat_subscriptions") == 1).await;
    assert_eq!(
        server.fake.calls("clear_pane_chat_subscriptions"),
        vec![call[0].clone()]
    );
    send(pushed);
}

#[tokio::test]
async fn events_socket_sends_connection_board_and_task_events_and_ignores_other_unpairs() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    let event = json!({ "type": "task-status-changed", "eventId": "event-1", "taskId": "task-1", "title": "kanvibe", "body": "review", "status": "review", "createdAt": "2026-10-23T00:00:00Z" });
    server
        .fake
        .broadcast_event(json!({ "type": "board-updated" }));
    server.fake.broadcast_event(event.clone());
    assert_eq!(client.next_json().await, json!({ "type": "board-updated" }));
    assert_eq!(client.next_json().await, event);
    assert_eq!(server.fake.count("subscribe_to_pane"), 0);
    server.fake.notify_unpaired("device-second");
    server.fake.broadcast_event(json!({ "type": "still-open" }));
    assert_eq!(client.next_json().await, json!({ "type": "still-open" }));
    server.fake.notify_unpaired("device-good");
    assert_eq!(client.closed().await, (4001, "device-unpaired".to_string()));
}

#[tokio::test]
async fn events_socket_rejects_unauthenticated_handshakes() {
    let server = TestServer::start().await;
    assert_eq!(
        server.websocket("/api/mobile/events", None).await.err(),
        Some(401)
    );
}

#[tokio::test]
async fn shutdown_closes_event_subscriptions_and_open_event_sockets() {
    let server = TestServer::start().await;
    let mut client = server.events(GOOD).await;
    server.running.shutdown();
    assert_eq!(client.closed().await, (1001, "server-shutdown".to_string()));
    assert_eq!(server.fake.event_listeners.count(), 0);
    assert_eq!(server.fake.notification_listeners.count(), 0);
}

#[tokio::test]
async fn stored_notifications_are_returned() {
    let server = TestServer::start().await;
    let reply = server.get("/api/mobile/notifications", Some(GOOD)).await;
    assert_eq!(
        reply.pair(),
        (
            200,
            json!({ "success": true, "notifications": [inbox_notification()] })
        )
    );
}

#[tokio::test]
async fn no_notifications_return_an_empty_list() {
    let server = TestServer::start().await;
    server.fake.returns("get_mobile_notifications", json!([]));
    let reply = server.get("/api/mobile/notifications", Some(GOOD)).await;
    assert_eq!(
        reply.pair(),
        (200, json!({ "success": true, "notifications": [] }))
    );
}

#[tokio::test]
async fn bodyless_read_requests_decode_the_path_id_and_return_the_changed_notification() {
    let server = TestServer::start().await;
    let reply = server
        .request(HttpCall::new("PATCH", "/api/mobile/notifications/notice%201/read").auth(GOOD))
        .await;
    let mut read = inbox_notification();
    read["isRead"] = json!(true);
    assert_eq!(
        reply.pair(),
        (200, json!({ "success": true, "notification": read }))
    );
    assert_eq!(
        server.fake.calls("mark_mobile_notification_read"),
        vec![json!("notice 1")]
    );
}

#[tokio::test]
async fn reading_a_missing_notification_is_404() {
    let server = TestServer::start().await;
    server
        .fake
        .returns("mark_mobile_notification_read", Value::Null);
    let reply = server
        .request(HttpCall::new("PATCH", "/api/mobile/notifications/missing/read").auth(GOOD))
        .await;
    assert_eq!(reply.pair(), (404, failure("없는 알림입니다")));
}

#[tokio::test]
async fn bodyless_read_all_requests_succeed() {
    let server = TestServer::start().await;
    let reply = server
        .request(HttpCall::new("POST", "/api/mobile/notifications/read-all").auth(GOOD))
        .await;
    assert_eq!(reply.pair(), (200, json!({ "success": true })));
    assert_eq!(server.fake.count("mark_all_mobile_notifications_read"), 1);
}

#[tokio::test]
async fn task_notification_reads_decode_the_task_id_and_return_the_updated_count() {
    let server = TestServer::start().await;
    let reply = server
        .request(HttpCall::new("POST", "/api/mobile/tasks/task%201/notifications/read").auth(GOOD))
        .await;
    assert_eq!(
        reply.pair(),
        (200, json!({ "success": true, "updatedCount": 2 }))
    );
    assert_eq!(
        server.fake.feature_calls(),
        vec![FeatureCall::MarkTaskNotificationsRead {
            task_id: "task 1".into()
        }]
    );
}

const INBOX_ROUTES: [(&str, &str, &str); 4] = [
    (
        "GET",
        "/api/mobile/notifications",
        "get_mobile_notifications",
    ),
    (
        "PATCH",
        "/api/mobile/notifications/notice-1/read",
        "mark_mobile_notification_read",
    ),
    (
        "POST",
        "/api/mobile/notifications/read-all",
        "mark_all_mobile_notifications_read",
    ),
    (
        "POST",
        "/api/mobile/tasks/task-1/notifications/read",
        "feature.MarkTaskNotificationsRead",
    ),
];

#[tokio::test]
async fn inbox_routes_do_not_call_services_before_authentication() {
    let server = TestServer::start().await;
    for (method, path, service) in INBOX_ROUTES {
        let reply = server.request(HttpCall::new(method, path)).await;
        assert_eq!(
            reply.pair(),
            (401, failure(UNPAIRED_ERROR)),
            "{method} {path}"
        );
        assert_eq!(server.fake.count(service), 0, "{service}");
    }
}

#[tokio::test]
async fn inbox_service_errors_are_hidden_behind_500() {
    let server = TestServer::start().await;
    for (method, path, service) in INBOX_ROUTES {
        server
            .fake
            .fails(service, unexpected("private settings path"));
        let reply = server.request(HttpCall::new(method, path).auth(GOOD)).await;
        assert_eq!(reply.pair(), (500, failure("서버 오류")), "{method} {path}");
    }
}

#[tokio::test]
async fn notification_store_changes_send_only_an_invalidation_to_every_device() {
    let server = TestServer::start().await;
    let mut first = server.events(GOOD).await;
    let mut second = server.events(SECOND).await;
    for listener in server.fake.notification_listeners.snapshot() {
        listener();
    }
    assert_eq!(
        first.next_json().await,
        json!({ "type": "notifications-changed" })
    );
    assert_eq!(
        second.next_json().await,
        json!({ "type": "notifications-changed" })
    );
}

#[tokio::test]
async fn notification_settings_are_readable_by_the_paired_device() {
    let server = TestServer::start().await;
    let reply = server
        .get("/api/mobile/notification-settings", Some(GOOD))
        .await;
    let settings = json!({ "isEnabled": true, "enabledStatuses": ["review"] });
    assert_eq!(
        reply.pair(),
        (200, json!({ "success": true, "settings": settings }))
    );
}

#[tokio::test]
async fn notification_settings_cannot_be_read_or_changed_without_authentication() {
    let server = TestServer::start().await;
    for method in ["GET", "PATCH"] {
        assert_eq!(
            server
                .request(HttpCall::new(method, "/api/mobile/notification-settings"))
                .await
                .status,
            401
        );
    }
    assert_eq!(
        server.fake.count("get_mobile_notification_settings")
            + server.fake.count("update_mobile_notification_settings"),
        0
    );
}

#[tokio::test]
async fn notification_settings_are_saved_and_the_stored_value_returned() {
    let server = TestServer::start().await;
    let settings = json!({ "isEnabled": false, "enabledStatuses": [] });
    let reply = server
        .request(
            HttpCall::new("PATCH", "/api/mobile/notification-settings")
                .auth(GOOD)
                .json(&settings),
        )
        .await;
    assert_eq!(
        reply.pair(),
        (200, json!({ "success": true, "settings": settings }))
    );
    assert_eq!(
        server.fake.calls("update_mobile_notification_settings"),
        vec![settings]
    );
}

#[tokio::test]
async fn invalid_settings_and_json_are_400() {
    let server = TestServer::start().await;
    server.fake.fails(
        "update_mobile_notification_settings",
        BridgeError::TaskInput("잘못된 상태".into()),
    );
    let invalid_status = HttpCall::new("PATCH", "/api/mobile/notification-settings")
        .auth(GOOD)
        .body(r#"{"isEnabled":true}"#);
    assert_eq!(server.request(invalid_status).await.status, 400);
    let invalid_json = HttpCall::new("PATCH", "/api/mobile/notification-settings")
        .auth(GOOD)
        .body("invalid");
    assert_eq!(server.request(invalid_json).await.status, 400);
}
