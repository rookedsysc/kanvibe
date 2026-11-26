//! TS `taskCliRoutes.test.mjs`, `browserOriginGuard.test.mjs`, `hookTransportGuard.test.mjs`.
//!
//! TS는 경로 처리기 하나를 따로 띄워 "처리하지 않으면 418"로 넘김을 확인했다. 여기서는 실제 라우팅 순서 그대로
//! 띄우므로, 넘긴 요청은 hook 라우팅의 404이거나 다른 서비스 호출로 드러난다.

use std::net::{IpAddr, Ipv4Addr};

use kanvibe_server::bridge::BridgeError;
use kanvibe_server::http::is_loopback_hook_address;
use serde_json::{Value, json};

use crate::harness::*;

const TASK_CLI_ROUTES: [(&str, &str, u16); 5] = [
    ("/api/cli/tasks", "Create", 201),
    ("/api/cli/tasks/delete", "Delete", 200),
    ("/api/cli/tasks/register", "Register", 200),
    ("/api/cli/tasks/list", "List", 200),
    ("/api/cli/tasks/session", "Session", 200),
];

fn task_cli_call(path: &str, body: &str) -> HttpCall {
    HttpCall::new("POST", path)
        .header("Content-Type", "application/json")
        .body(body)
}

#[tokio::test]
async fn task_cli_routes_pass_authorization_and_body_and_return_the_service_result() {
    let server = TestServer::start().await;
    for (path, route, status) in TASK_CLI_ROUTES {
        let call = task_cli_call(path, r#"{"contextTaskId":"task-1"}"#).auth("Bearer token-1");
        let reply = server.request(call).await;
        let data = crate::harness::fake_defaults::default_response(
            &format!("task_cli.{route}"),
            &Value::Null,
        );
        assert_eq!(
            reply.pair(),
            (status, json!({ "success": true, "data": data })),
            "{path}"
        );
        let calls = server.fake.calls(&format!("task_cli.{route}"));
        let expected = json!([{ "authorization": "Bearer token-1", "body": { "contextTaskId": "task-1" }, "hosts": null }]);
        assert_eq!(Value::Array(calls), expected, "{path}");
    }
}

#[tokio::test]
async fn task_cli_request_errors_keep_the_service_status_and_reason() {
    let server = TestServer::start().await;
    let rejection = BridgeError::Feature {
        status: 401,
        message: "task CLI 인증에 실패했습니다.".into(),
    };
    server.fake.fails("task_cli.Delete", rejection);
    let reply = server
        .request(task_cli_call(
            "/api/cli/tasks/delete",
            r#"{"contextTaskId":"task-1"}"#,
        ))
        .await;
    assert_eq!(
        reply.pair(),
        (401, failure("task CLI 인증에 실패했습니다."))
    );
}

#[tokio::test]
async fn unexpected_task_cli_failures_answer_500_with_the_cause_message() {
    let server = TestServer::start().await;
    server.fake.fails(
        "task_cli.Create",
        unexpected("fatal: invalid reference: missing"),
    );
    let reply = server
        .request(task_cli_call(
            "/api/cli/tasks",
            r#"{"contextTaskId":"task-1"}"#,
        ))
        .await;
    assert_eq!(
        reply.pair(),
        (500, failure("fatal: invalid reference: missing"))
    );
}

#[tokio::test]
async fn unreadable_or_non_object_task_cli_bodies_are_rejected_before_the_service() {
    let server = TestServer::start().await;
    for (body, error) in [
        ("{not json", "JSON 본문을 읽을 수 없습니다"),
        ("[1,2]", "JSON 객체 본문이 필요합니다"),
    ] {
        let reply = server.request(task_cli_call("/api/cli/tasks", body)).await;
        assert_eq!(reply.pair(), (400, failure(error)), "{body}");
    }
    assert_eq!(server.fake.count("task_cli.Create"), 0);
}

#[tokio::test]
async fn other_paths_and_methods_fall_through_to_hook_routing() {
    let server = TestServer::start().await;
    let get_reply = server.get("/api/cli/tasks", None).await;
    assert_eq!(get_reply.pair(), (404, failure("Not found")));
    let status_reply = server
        .request(HttpCall::new("POST", "/api/hooks/status").body("{}"))
        .await;
    assert_eq!(status_reply.status, 200);
    assert_eq!(server.fake.count("task_cli.Create"), 0);
    assert_eq!(server.fake.count("update_hook_task_status"), 1);
}

#[tokio::test]
async fn requests_with_an_origin_header_are_rejected_with_403_before_handling() {
    let server = TestServer::start().await;
    let call = HttpCall::new("POST", "/api/hooks/status")
        .header("Origin", "https://attacker.example")
        .header("Content-Type", "text/plain")
        .body("{}");
    let reply = server.request(call).await;
    assert_eq!(
        reply.pair(),
        (403, failure("브라우저 요청은 허용하지 않습니다"))
    );
    assert_eq!(server.fake.count("update_hook_task_status"), 0);
}

#[tokio::test]
async fn requests_without_an_origin_header_pass_through_to_the_hook_route() {
    let server = TestServer::start().await;
    let reply = server
        .request(HttpCall::new("POST", "/api/hooks/status").body("{}"))
        .await;
    assert_eq!(reply.pair(), (200, json!({ "success": true })));
    assert_eq!(server.fake.count("update_hook_task_status"), 1);
}

#[tokio::test]
async fn remote_tcp_hook_requests_are_rejected_before_any_handler_runs() {
    let server = TestServer::start().await;
    let remote_peer = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));
    let routes = [
        "/api/hooks/status",
        "/api/hooks/health",
        "/api/install/hooks.sh",
        "/api/cli/tasks",
        "/api/cli/tasks/delete",
        "/api/cli/tasks/register",
    ];
    for route in routes {
        let call = HttpCall::get(route).header("X-Forwarded-For", "127.0.0.1");
        let reply = server.request_from(remote_peer, call).await;
        assert_eq!(reply.pair(), (404, failure("Not found")), "{route}");
    }
    assert_eq!(
        server.fake.count("update_hook_task_status")
            + server.fake.count("read_hook_install_script"),
        0
    );
}

#[tokio::test]
async fn local_peers_are_handled_as_hook_requests() {
    let server = TestServer::start().await;
    let reply = server.get("/api/hooks/health", None).await;
    assert_eq!(reply.pair(), (200, json!({ "success": true })));
    // 서버는 127.0.0.1에만 묶이므로 나머지 두 표기는 같은 판정 함수로 확인한다
    for address in ["127.0.0.1", "::1", "::ffff:127.0.0.1"] {
        assert!(
            is_loopback_hook_address(address.parse().expect("주소")),
            "{address}"
        );
    }
}
