//! TS `mobileFeatureRoutes.test.mjs`, `mobileDiagramRoutes.test.mjs`

use kanvibe_server::bridge::{BridgeError, FeatureCall};
use serde_json::{Value, json};

use crate::harness::*;

#[tokio::test]
async fn search_returns_every_searchable_task_from_the_provider() {
    let server = TestServer::start().await;
    let reply = server.get("/api/mobile/tasks/search", Some(GOOD)).await;
    assert_eq!(
        reply.pair(),
        (
            200,
            json!({ "success": true, "tasks": [{ "id": "task-1", "title": "searchable" }] })
        )
    );
    assert_eq!(server.fake.feature_calls(), vec![FeatureCall::SearchTasks]);
}

#[tokio::test]
async fn feature_providers_are_not_called_before_authorization() {
    let server = TestServer::start().await;
    let reply = server
        .get("/api/mobile/tasks/search", Some("Bearer bad"))
        .await;
    assert_eq!(reply.status, 401);
    assert!(server.fake.feature_calls().is_empty());
}

#[tokio::test]
async fn done_pagination_is_validated_before_delegation() {
    let server = TestServer::start().await;
    assert_eq!(
        server
            .get("/api/mobile/tasks/done?offset=20&limit=20", Some(GOOD))
            .await
            .status,
        200
    );
    assert_eq!(
        server.fake.feature_calls(),
        vec![FeatureCall::DoneTasks {
            offset: 20,
            limit: 20
        }]
    );
    assert_eq!(
        server
            .get("/api/mobile/tasks/done?offset=-1", Some(GOOD))
            .await
            .status,
        400
    );
    assert_eq!(server.fake.feature_calls().len(), 1);
}

#[tokio::test]
async fn task_details_update_only_through_the_explicit_endpoint() {
    let server = TestServer::start().await;
    let body = json!({ "description": "updated", "priority": null });
    let reply = server
        .request(
            HttpCall::new("PATCH", "/api/mobile/tasks/task-1/details")
                .auth(GOOD)
                .json(&body),
        )
        .await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        server.fake.feature_calls(),
        vec![FeatureCall::UpdateTaskDetails {
            task_id: "task-1".into(),
            body
        }]
    );
}

#[tokio::test]
async fn unexpected_feature_failures_answer_a_generic_error() {
    let server = TestServer::start().await;
    server
        .fake
        .fails("feature.SearchTasks", unexpected("secret command"));
    let reply = server.get("/api/mobile/tasks/search", Some(GOOD)).await;
    assert_eq!(reply.pair(), (500, failure("서버 오류")));
}

fn diagram_input() -> Value {
    json!({ "definition": "flowchart LR\nA-->B", "theme": "dark", "width": 640 })
}

fn render(body: &str, authorization: &str) -> HttpCall {
    HttpCall::new("POST", "/api/mobile/diagrams/render")
        .auth(authorization)
        .body(body)
}

#[tokio::test]
async fn diagram_route_authorizes_before_touching_the_renderer() {
    let server = TestServer::start().await;
    let reply = server
        .request(render(&diagram_input().to_string(), "Bearer bad"))
        .await;
    assert_eq!(reply.status, 401);
    assert_eq!(server.fake.count("render_mobile_diagram"), 0);
}

#[tokio::test]
async fn diagram_route_returns_only_the_image_envelope() {
    let server = TestServer::start().await;
    let reply = server
        .request(render(&diagram_input().to_string(), GOOD))
        .await;
    let image = json!({ "success": true, "imageBase64": "jpeg", "width": 640, "height": 120, "mimeType": "image/jpeg" });
    assert_eq!(reply.pair(), (200, image));
    assert_eq!(
        server.fake.calls("render_mobile_diagram"),
        vec![diagram_input()]
    );
}

#[tokio::test]
async fn malformed_diagram_json_is_rejected_before_rendering() {
    let server = TestServer::start().await;
    assert_eq!(server.request(render("{", GOOD)).await.status, 400);
    assert_eq!(server.fake.count("render_mobile_diagram"), 0);
}

#[tokio::test]
async fn safe_renderer_error_statuses_are_preserved() {
    let server = TestServer::start().await;
    for status in [400, 422, 429, 500, 503] {
        server.fake.fails(
            "render_mobile_diagram",
            BridgeError::Diagram {
                status,
                message: "safe message".into(),
            },
        );
        let reply = server
            .request(render(&diagram_input().to_string(), GOOD))
            .await;
        assert_eq!(reply.pair(), (status, failure("safe message")), "{status}");
    }
}

#[tokio::test]
async fn unexpected_renderer_errors_hide_paths_and_source() {
    let server = TestServer::start().await;
    server.fake.fails(
        "render_mobile_diagram",
        unexpected("/private/path graph secret"),
    );
    let reply = server
        .request(render(&diagram_input().to_string(), GOOD))
        .await;
    assert_eq!(
        reply.pair(),
        (500, failure("다이어그램 이미지를 만들지 못했습니다"))
    );
}
