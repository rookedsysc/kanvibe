//! TS `mobileRoutes.test.mjs`: 인증 경계, 기기 연결 해제, 기기 이름, 개발용 페어링 코드, 요청 본문 상한

use std::net::{IpAddr, Ipv4Addr};

use kanvibe_server::server::RouteOptions;
use serde_json::json;

use crate::harness::*;

const LAN_PEER: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));
/// 기본 본문 상한(64 KiB)을 한 바이트 넘는 본문
const OVERSIZED_BODY_BYTES: usize = 64 * 1024 + 1;

fn pair_call(body: &serde_json::Value) -> HttpCall {
    HttpCall::new("POST", "/api/mobile/pair").json(body)
}

#[tokio::test]
async fn pairing_passes_without_a_token() {
    let server = TestServer::start().await;
    let reply = server
        .request(pair_call(
            &json!({ "code": "123456", "deviceName": "iPhone" }),
        ))
        .await;
    assert_eq!(
        reply.pair(),
        (200, json!({ "success": true, "token": "paired-token" }))
    );
    assert_eq!(server.fake.count("authorize_mobile_request"), 0);
}

#[tokio::test]
async fn board_and_pane_routes_need_a_token() {
    let server = TestServer::start().await;
    assert_eq!(server.get("/api/mobile/board", None).await.status, 401);
    assert_eq!(
        server
            .get("/api/mobile/tasks/task-1/surfaces", None)
            .await
            .status,
        401
    );
    assert_eq!(
        server.fake.count("get_mobile_board") + server.fake.count("get_task_surfaces"),
        0
    );
}

#[tokio::test]
async fn unknown_paths_under_the_mobile_prefix_are_404() {
    let server = TestServer::start().await;
    let reply = server.get("/api/mobile/unknown", Some(GOOD)).await;
    assert_eq!(reply.pair(), (404, failure("Not found")));
}

#[tokio::test]
async fn non_mobile_paths_fall_through_to_hook_routing() {
    let server = TestServer::start().await;
    let reply = server.get("/api/hooks/health", None).await;
    assert_eq!(reply.pair(), (200, json!({ "success": true })));
    assert_eq!(server.fake.count("authorize_mobile_request"), 0);
}

#[tokio::test]
async fn unpairing_with_its_own_token_removes_only_that_token() {
    let server = TestServer::start().await;
    let reply = server
        .request(HttpCall::new("DELETE", "/api/mobile/device").auth(GOOD))
        .await;
    assert_eq!(reply.pair(), (200, json!({ "success": true })));
    assert_eq!(
        server.fake.calls("unpair_mobile_device_by_token"),
        vec![json!(GOOD)]
    );
}

#[tokio::test]
async fn unpairing_without_a_token_is_stopped_by_authentication() {
    let server = TestServer::start().await;
    assert_eq!(
        server
            .request(HttpCall::new("DELETE", "/api/mobile/device"))
            .await
            .status,
        401
    );
    assert_eq!(server.fake.count("unpair_mobile_device_by_token"), 0);
}

#[tokio::test]
async fn unpairing_with_an_unknown_token_is_stopped_by_authentication() {
    let server = TestServer::start().await;
    let reply = server
        .request(HttpCall::new("DELETE", "/api/mobile/device").auth("Bearer unknown"))
        .await;
    assert_eq!(reply.status, 401);
    assert_eq!(server.fake.count("unpair_mobile_device_by_token"), 0);
}

#[tokio::test]
async fn a_device_unpaired_between_authentication_and_removal_gets_401() {
    let server = TestServer::start().await;
    server
        .fake
        .returns("unpair_mobile_device_by_token", json!(false));
    let reply = server
        .request(HttpCall::new("DELETE", "/api/mobile/device").auth(GOOD))
        .await;
    assert_eq!(reply.pair(), (401, failure(UNPAIRED_ERROR)));
}

#[tokio::test]
async fn a_throwing_removal_answers_500_instead_of_hanging() {
    let server = TestServer::start().await;
    server
        .fake
        .fails("unpair_mobile_device_by_token", unexpected("DB가 닫혔다"));
    let reply = server
        .request(HttpCall::new("DELETE", "/api/mobile/device").auth(GOOD))
        .await;
    assert_eq!(reply.pair(), (500, failure("서버 오류")));
}

#[tokio::test]
async fn a_wrong_pairing_code_is_answered_only_after_the_failure_delay() {
    let server = TestServer::start().await;
    server.fake.returns("pair_mobile_device", json!(null));
    let started = std::time::Instant::now();
    let reply = server
        .request(pair_call(&json!({ "code": "000000" })))
        .await;
    assert_eq!(reply.status, 401);
    assert!(started.elapsed() >= kanvibe_mobile::pairing::PAIRING_CODE_FAILURE_DELAY);
}

#[tokio::test]
async fn long_device_names_are_truncated() {
    let server = TestServer::start().await;
    server
        .request(pair_call(
            &json!({ "code": "123456", "deviceName": "가".repeat(500) }),
        ))
        .await;
    assert_eq!(
        server.fake.calls("pair_mobile_device")[0][1],
        json!("가".repeat(64))
    );
}

#[tokio::test]
async fn missing_device_names_use_the_default_name() {
    let server = TestServer::start().await;
    server
        .request(pair_call(&json!({ "code": "123456" })))
        .await;
    assert_eq!(
        server.fake.calls("pair_mobile_device")[0][1],
        json!("모바일 기기")
    );
}

#[tokio::test]
async fn dev_runs_issue_a_pairing_code_to_this_machine() {
    let server = TestServer::start_with(RouteOptions {
        is_dev_pairing_route_enabled: true,
    })
    .await;
    let reply = server
        .request(HttpCall::new("POST", "/api/mobile/dev/pairing-code"))
        .await;
    assert_eq!(
        reply.pair(),
        (
            200,
            json!({ "success": true, "code": "654321", "expiresAt": 1_700_000_000_000_u64 })
        )
    );
    assert_eq!(server.fake.count("start_mobile_pairing"), 1);
}

#[tokio::test]
async fn built_apps_answer_the_dev_route_like_an_unknown_path() {
    let server = TestServer::start().await;
    let unknown = server
        .request(HttpCall::new("POST", "/api/mobile/unknown"))
        .await;
    let dev = server
        .request(HttpCall::new("POST", "/api/mobile/dev/pairing-code"))
        .await;
    assert_eq!(dev, unknown);
    assert_eq!(server.fake.count("start_mobile_pairing"), 0);
}

#[tokio::test]
async fn other_machines_get_the_unknown_path_answer_even_in_dev_runs() {
    let server = TestServer::start_with(RouteOptions {
        is_dev_pairing_route_enabled: true,
    })
    .await;
    let dev = server
        .request_from(
            LAN_PEER,
            HttpCall::new("POST", "/api/mobile/dev/pairing-code"),
        )
        .await;
    let unknown = server
        .request_from(LAN_PEER, HttpCall::new("POST", "/api/mobile/unknown"))
        .await;
    assert_eq!(dev, unknown);
    assert_eq!(server.fake.count("start_mobile_pairing"), 0);
}

/// TS `readJsonBody`는 상한을 넘으면 `request.destroy()`로 소켓을 끊는다. 상대가 보는 결과는 응답 없는 끊김이다
#[tokio::test]
async fn bodies_over_the_limit_cut_the_socket_without_buffering() {
    let server = TestServer::start().await;
    let call = HttpCall::new("PUT", "/api/mobile/notifications")
        .auth(GOOD)
        .body(vec![b'a'; OVERSIZED_BODY_BYTES]);
    assert_eq!(server.exchange(call).await, Exchange::ClosedWithoutResponse);
    assert_eq!(
        server.fake.count("update_mobile_notification_subscription"),
        0
    );
}

/// TS는 끊은 소켓의 가짜 응답 객체에 400을 썼지만 실제 연결에서는 그 응답이 나가지 못한다. 관찰되는 결과는 끊김이다
#[tokio::test]
async fn oversized_pairing_bodies_are_rejected_before_pairing() {
    let server = TestServer::start().await;
    let call = HttpCall::new("POST", "/api/mobile/pair").body(vec![b'a'; OVERSIZED_BODY_BYTES]);
    assert_eq!(server.exchange(call).await, Exchange::ClosedWithoutResponse);
    assert_eq!(server.fake.count("pair_mobile_device"), 0);
}
