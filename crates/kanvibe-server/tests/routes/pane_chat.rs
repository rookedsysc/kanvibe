//! TS `mobileRoutes.test.mjs`: 현재 pane 채팅 인증 및 대상 경계, 채팅 composer 첨부와 자동완성 HTTP

use base64::Engine;
use kanvibe_server::bridge::{BridgeError, PaneChatAction};
use serde_json::{Value, json};

use crate::harness::*;

const ATTACHMENTS_PATH: &str = "/api/mobile/tasks/task-1/pane-chat/attachments";
const HISTORY_ATTACHMENT_PATH: &str =
    "/api/mobile/tasks/task-1/ai-sessions/codex/session-1/attachments/image-id";

fn post(action: &str, body: &Value) -> HttpCall {
    HttpCall::new(
        "POST",
        &format!("/api/mobile/tasks/task-1/pane-chat/{action}"),
    )
    .json(body)
}

fn action(task_id: &str, token: &str, action: PaneChatAction) -> (String, String, PaneChatAction) {
    (task_id.to_string(), token.to_string(), action)
}

// parity: TS-electron-mobileRoutes-dde2c2
#[tokio::test]
async fn paired_devices_read_the_exact_requested_pane_snapshot() {
    let server = TestServer::start().await;
    let reply = server
        .get("/api/mobile/tasks/task-1/pane-chat?paneId=%253", Some(GOOD))
        .await;
    assert_eq!(reply.status, 200);
    let snapshot = &reply.json()["snapshot"];
    assert_eq!(
        (&snapshot["paneId"], &snapshot["targetToken"]),
        (&json!("%3"), &json!("opaque-token"))
    );
    assert_eq!(
        server.fake.calls("get_sequenced_pane_chat_snapshot"),
        vec![json!(["task-1", "%3", true])]
    );
}

// parity: TS-electron-mobileRoutes-a6ea62
#[tokio::test]
async fn devices_hiding_the_screen_read_the_snapshot_without_it() {
    let server = TestServer::start().await;
    server
        .get(
            "/api/mobile/tasks/task-1/pane-chat?paneId=%253&wantsScreen=false",
            Some(GOOD),
        )
        .await;
    assert_eq!(
        server.fake.calls("get_sequenced_pane_chat_snapshot"),
        vec![json!(["task-1", "%3", false])]
    );
}

// parity: TS-electron-mobileRoutes-b67b91
#[tokio::test]
async fn agent_only_reads_return_the_provider_without_a_snapshot() {
    let server = TestServer::start().await;
    server
        .fake
        .returns("get_pane_chat_agent_provider", json!("codex"));
    let reply = server
        .get(
            "/api/mobile/tasks/task-1/pane-chat?paneId=%253&detail=agent",
            Some(GOOD),
        )
        .await;
    assert_eq!(
        reply.pair(),
        (200, json!({ "success": true, "provider": "codex" }))
    );
    assert_eq!(
        server.fake.calls("get_pane_chat_agent_provider"),
        vec![json!(["task-1", "%3"])]
    );
    assert_eq!(server.fake.count("get_sequenced_pane_chat_snapshot"), 0);
}

// parity: TS-electron-mobileRoutes-1d66b4
#[tokio::test]
async fn devices_without_a_token_cannot_read_or_send() {
    let server = TestServer::start().await;
    assert_eq!(
        server
            .get("/api/mobile/tasks/task-1/pane-chat", None)
            .await
            .status,
        401
    );
    let prompt = post(
        "prompt",
        &json!({ "targetToken": "opaque-token", "text": "Hi" }),
    );
    assert_eq!(server.request(prompt).await.status, 401);
    assert_eq!(server.fake.count("get_sequenced_pane_chat_snapshot"), 0);
    assert!(server.fake.pane_chat_actions().is_empty());
}

// parity: TS-electron-mobileRoutes-a17d14
#[tokio::test]
async fn prompts_keep_authentication_and_the_target_token() {
    let server = TestServer::start().await;
    let reply = server
        .request(
            post(
                "prompt",
                &json!({ "targetToken": "opaque-token", "text": "First\nSecond" }),
            )
            .auth(GOOD),
        )
        .await;
    assert_eq!(reply.pair(), (200, json!({ "success": true })));
    let prompt = PaneChatAction::Prompt {
        text: json!("First\nSecond"),
        extra: None,
    };
    assert_eq!(
        server.fake.pane_chat_actions(),
        vec![action("task-1", "opaque-token", prompt)]
    );
}

// parity: TS-electron-mobileRoutes-d3e4f0
#[tokio::test]
async fn model_setting_changes_pass_through_and_rejections_are_400() {
    let server = TestServer::start().await;
    let change = json!({ "kind": "effort", "effort": "high" });
    let body = json!({ "targetToken": "opaque-token", "change": change });
    assert_eq!(
        server
            .request(post("model-setting", &body).auth(GOOD))
            .await
            .pair(),
        (200, json!({ "success": true }))
    );
    let expected = action(
        "task-1",
        "opaque-token",
        PaneChatAction::ModelSetting { change },
    );
    assert_eq!(server.fake.pane_chat_actions(), vec![expected]);
    server.fake.fails(
        "pane_chat.ModelSetting",
        BridgeError::PaneChatInput("지금은 이 설정을 바꿀 수 없습니다.".into()),
    );
    let rejected = server
        .request(post("model-setting", &body).auth(GOOD))
        .await;
    assert_eq!(
        rejected.pair(),
        (400, failure("지금은 이 설정을 바꿀 수 없습니다."))
    );
    assert_eq!(
        server.request(post("model-setting", &body)).await.status,
        401
    );
}

// parity: TS-electron-mobileRoutes-ec52aa
#[tokio::test]
async fn replaced_targets_conflict_and_empty_tokens_are_rejected() {
    let server = TestServer::start().await;
    server.fake.fails(
        "pane_chat.Interrupt",
        BridgeError::PaneChatTargetChanged("대상 변경".into()),
    );
    let interrupted = server
        .request(post("interrupt", &json!({ "targetToken": "stale-token" })).auth(GOOD))
        .await;
    assert_eq!(
        interrupted.pair(),
        (
            409,
            json!({ "success": false, "error": "대상 변경", "reason": "target-changed" })
        )
    );
    assert_eq!(
        server
            .request(post("prompt", &json!({ "text": "Hi" })).auth(GOOD))
            .await
            .status,
        400
    );
    assert_eq!(server.fake.count("pane_chat.Prompt"), 0);
}

fn staged_attachment() -> Value {
    json!({ "success": true, "attachment": { "id": "owned-id", "name": "file.txt", "mimeType": "text/plain", "kind": "file", "sizeBytes": 65537 } })
}

// parity: TS-electron-mobileRoutes-534ec1
#[tokio::test]
async fn only_authenticated_uploads_read_attachment_bodies_over_64_kib() {
    let server = TestServer::start().await;
    server
        .fake
        .returns("pane_chat.StageAttachment", staged_attachment());
    let data = base64::engine::general_purpose::STANDARD.encode(vec![0_u8; 65537]);
    let attachment = json!({ "name": "file.txt", "mimeType": "text/plain", "dataUrl": format!("data:text/plain;base64,{data}") });
    let body = json!({ "targetToken": "token", "attachment": attachment });
    let reply = server
        .request(
            HttpCall::new("POST", ATTACHMENTS_PATH)
                .auth(GOOD)
                .json(&body),
        )
        .await;
    assert_eq!(
        (reply.status, &reply.json()["attachment"]["id"]),
        (200, &json!("owned-id"))
    );
    let staged = action(
        "task-1",
        "token",
        PaneChatAction::StageAttachment { attachment },
    );
    assert_eq!(server.fake.pane_chat_actions(), vec![staged]);
    let unauthenticated = server
        .exchange(HttpCall::new("POST", ATTACHMENTS_PATH).json(&body))
        .await;
    if let Exchange::Reply(reply) = unauthenticated {
        assert_eq!(reply.status, 401);
    }
    assert_eq!(server.fake.pane_chat_actions().len(), 1);
}

// parity: TS-electron-mobileRoutes-2ed80b
#[tokio::test]
async fn attachment_removal_and_search_use_the_same_target_token() {
    let server = TestServer::start().await;
    let removal = HttpCall::new("DELETE", ATTACHMENTS_PATH)
        .auth(GOOD)
        .json(&json!({ "targetToken": "token", "id": "owned-id" }));
    assert_eq!(server.request(removal).await.status, 200);
    assert_eq!(
        server.fake.pane_chat_actions(),
        vec![action(
            "task-1",
            "token",
            PaneChatAction::RemoveAttachment {
                id: json!("owned-id")
            }
        )]
    );
    let items = json!([{ "id": "skill:review", "label": "review", "description": "Review", "insertText": "$review" }]);
    server.fake.returns(
        "get_pane_chat_suggestions",
        json!({ "targetToken": "token", "items": items, "truncated": false }),
    );
    let reply = server.get("/api/mobile/tasks/task-1/pane-chat/suggestions?targetToken=token&kind=skill&query=review", Some(GOOD)).await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        (
            &reply.json()["targetToken"],
            &reply.json()["items"][0]["insertText"]
        ),
        (&json!("token"), &json!("$review"))
    );
    assert_eq!(
        server.fake.calls("get_pane_chat_suggestions"),
        vec![json!(["task-1", "token", "skill", "review", null])]
    );
}

// parity: TS-electron-mobileRoutes-995f5b
#[tokio::test]
async fn full_list_requests_pass_the_limit_as_a_number() {
    let server = TestServer::start().await;
    let reply = server.get("/api/mobile/tasks/task-1/pane-chat/suggestions?targetToken=token&kind=skill&query=&limit=512", Some(GOOD)).await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        server.fake.calls("get_pane_chat_suggestions"),
        vec![json!(["task-1", "token", "skill", "", 512.0])]
    );
}

// parity: TS-electron-mobileRoutes-fe148f
#[tokio::test]
async fn attachment_ids_and_the_dedupe_id_travel_with_the_prompt() {
    let server = TestServer::start().await;
    let body = json!({ "targetToken": "token", "text": "caption", "attachmentIds": ["owned-id"], "requestId": "once" });
    server.request(post("prompt", &body).auth(GOOD)).await;
    let prompt = PaneChatAction::Prompt {
        text: json!("caption"),
        extra: Some((json!(["owned-id"]), json!("once"))),
    };
    assert_eq!(
        server.fake.pane_chat_actions(),
        vec![action("task-1", "token", prompt)]
    );
}

// parity: TS-electron-mobileRoutes-dfbcc3
#[tokio::test]
async fn history_attachments_need_authentication_and_missing_files_are_404() {
    let server = TestServer::start().await;
    let image = json!({ "id": "image-id", "name": "image.png", "mimeType": "image/png", "dataUrl": "data:image/png;base64,aGVsbG8=" });
    server
        .fake
        .returns("get_mobile_ai_session_attachment", image.clone());
    assert_eq!(server.get(HISTORY_ATTACHMENT_PATH, None).await.status, 401);
    assert_eq!(server.fake.count("get_mobile_ai_session_attachment"), 0);
    assert_eq!(
        server.get(HISTORY_ATTACHMENT_PATH, Some(GOOD)).await.json()["attachment"],
        image
    );
    let args = json!(["task-1", "codex", "session-1", "image-id", null]);
    assert_eq!(
        server.fake.calls("get_mobile_ai_session_attachment"),
        vec![args]
    );
    server
        .fake
        .returns("get_mobile_ai_session_attachment", Value::Null);
    assert_eq!(
        server.get(HISTORY_ATTACHMENT_PATH, Some(GOOD)).await.status,
        404
    );
}

// parity: TS-electron-mobileRoutes-541405
#[tokio::test]
async fn only_paired_devices_read_workspace_files_and_the_path_is_decoded() {
    let server = TestServer::start().await;
    let file = json!({ "path": "docs/새 계획.md", "content": "# plan" });
    server.fake.returns("get_mobile_task_file", file.clone());
    let encoded: String =
        url::form_urlencoded::byte_serialize("/work/task/docs/새 계획.md".as_bytes()).collect();
    let path = format!("/api/mobile/tasks/task-1/workspace-file?path={encoded}");
    assert_eq!(server.get(&path, None).await.status, 401);
    assert_eq!(server.fake.count("get_mobile_task_file"), 0);
    assert_eq!(
        server.get(&path, Some(GOOD)).await.pair(),
        (200, json!({ "success": true, "file": file }))
    );
    assert_eq!(
        server.fake.calls("get_mobile_task_file"),
        vec![json!(["task-1", "/work/task/docs/새 계획.md"])]
    );
}

// parity: TS-electron-mobileRoutes-4bbd8d
#[tokio::test]
async fn missing_or_empty_file_paths_are_400_without_reading() {
    let server = TestServer::start().await;
    let missing = server
        .get("/api/mobile/tasks/task-1/workspace-file", Some(GOOD))
        .await;
    assert_eq!(missing.pair(), (400, failure("파일 경로가 필요합니다")));
    assert_eq!(
        server
            .get("/api/mobile/tasks/task-1/workspace-file?path=", Some(GOOD))
            .await
            .status,
        400
    );
    assert_eq!(server.fake.count("get_mobile_task_file"), 0);
}

// parity: TS-electron-mobileRoutes-e2a9ac
#[tokio::test]
async fn file_open_failures_are_told_apart_by_status_and_reason() {
    let server = TestServer::start().await;
    let cases = [
        (
            "outside-worktree",
            400,
            "작업 폴더 밖의 파일은 열 수 없습니다",
        ),
        ("not-found", 404, "파일을 찾을 수 없습니다"),
        ("too-large", 413, "파일이 너무 커서 열 수 없습니다"),
        ("unreadable", 500, "파일을 읽을 수 없습니다"),
    ];
    for (reason, status, error) in cases {
        server.fake.fails(
            "get_mobile_task_file",
            BridgeError::TaskWorkspaceFile(reason.into()),
        );
        let reply = server
            .get(
                "/api/mobile/tasks/task-1/workspace-file?path=a.md",
                Some(GOOD),
            )
            .await;
        assert_eq!(
            reply.pair(),
            (
                status,
                json!({ "success": false, "error": error, "reason": reason })
            )
        );
    }
}

// parity: TS-electron-mobileRoutes-9652dc
#[tokio::test]
async fn unknown_file_read_failures_hide_the_internal_message() {
    let server = TestServer::start().await;
    server
        .fake
        .fails("get_mobile_task_file", unexpected("private host path"));
    let reply = server
        .get(
            "/api/mobile/tasks/task-1/workspace-file?path=a.md",
            Some(GOOD),
        )
        .await;
    assert_eq!(reply.pair(), (500, failure("서버 오류")));
}

// parity: TS-electron-mobileRoutes-92cab0
#[tokio::test]
async fn uncertain_partial_deliveries_are_stated_without_inviting_a_resend() {
    let server = TestServer::start().await;
    server.fake.fails(
        "pane_chat.Prompt",
        BridgeError::PaneChatDeliveryUncertain("전송 결과 불확실".into()),
    );
    let body = json!({ "targetToken": "token", "text": "caption", "requestId": "once" });
    let reply = server.request(post("prompt", &body).auth(GOOD)).await;
    assert_eq!(
        reply.pair(),
        (
            409,
            json!({ "success": false, "error": "전송 결과 불확실", "reason": "delivery-uncertain" })
        )
    );
}
