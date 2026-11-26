//! TS `mobileRoutes.test.mjs`: 터미널 탭 생성, 지연 최소화 계약, 오류 응답, 태스크 생성·상태·세션 타입, 브랜치 목록, pane 단축키

use kanvibe_server::bridge::BridgeError;
use serde_json::{Value, json};

use crate::harness::*;

const SHORTCUT_PATH: &str = "/api/mobile/tasks/task%20id/panes/%2519/shortcuts";

fn input_error(message: &str) -> BridgeError {
    BridgeError::TaskInput(message.to_string())
}

fn reason_failure(reason: &str) -> Value {
    json!({ "success": false, "reason": reason })
}

#[tokio::test]
async fn unauthenticated_tokens_cannot_create_tabs() {
    let server = TestServer::start().await;
    for authorization in [None, Some("Bearer unknown")] {
        let call = HttpCall::new("POST", "/api/mobile/tasks/task-1/tabs");
        let reply = server
            .request(authorization.map_or(call.clone(), |token| call.auth(token)))
            .await;
        assert_eq!(reply.status, 401);
    }
    assert_eq!(server.fake.count("create_mobile_terminal_tab"), 0);
}

#[tokio::test]
async fn tabs_are_added_to_the_authenticated_task_and_the_new_pane_returned() {
    let server = TestServer::start().await;
    let surfaces = json!({ "taskId": "task id", "sessionType": "terminal", "canResize": true, "tabs": [{ "id": "tab-2", "panes": [{ "id": "tab-2" }] }] });
    server.fake.returns(
        "create_mobile_terminal_tab",
        json!({ "surfaces": surfaces, "paneId": "tab-2" }),
    );
    let injected =
        json!({ "sshHost": "injected", "command": "injected", "worktreePath": "/injected" });
    let reply = server
        .request(
            HttpCall::new("POST", "/api/mobile/tasks/task%20id/tabs")
                .auth(GOOD)
                .json(&injected),
        )
        .await;
    assert_eq!(
        server.fake.calls("create_mobile_terminal_tab"),
        vec![json!("task id")]
    );
    assert_eq!(
        reply.pair(),
        (
            200,
            json!({ "success": true, "surfaces": surfaces, "paneId": "tab-2" })
        )
    );
}

#[tokio::test]
async fn tab_creation_without_a_session_answers_409_with_the_reason() {
    let server = TestServer::start().await;
    server.fake.fails(
        "create_mobile_terminal_tab",
        BridgeError::SurfaceUnavailable("no-session".into()),
    );
    let reply = server
        .request(HttpCall::new("POST", "/api/mobile/tasks/task-1/tabs").auth(GOOD))
        .await;
    assert_eq!(reply.pair(), (409, reason_failure("no-session")));
}

#[tokio::test]
async fn failed_tab_creation_hides_ssh_commands_and_hosts() {
    let server = TestServer::start().await;
    server.fake.fails(
        "create_mobile_terminal_tab",
        unexpected("ssh secret-host tmux new-window /secret/path"),
    );
    let reply = server
        .request(HttpCall::new("POST", "/api/mobile/tasks/task-1/tabs").auth(GOOD))
        .await;
    assert_eq!(reply.pair(), (500, failure("서버 오류")));
}

#[tokio::test]
async fn only_minimal_surface_requests_pass_the_new_option() {
    let server = TestServer::start().await;
    assert_eq!(
        server
            .get(
                "/api/mobile/tasks/task-1/surfaces?metadata=minimal",
                Some(GOOD)
            )
            .await
            .status,
        200
    );
    server
        .get(
            "/api/mobile/tasks/task-1/surfaces?metadata=minimal&tabId=w1%3At2",
            Some(GOOD),
        )
        .await;
    server
        .get(
            "/api/mobile/tasks/task-1/surfaces?metadata=unknown",
            Some(GOOD),
        )
        .await;
    server
        .get("/api/mobile/tasks/task-1/surfaces", Some(GOOD))
        .await;
    let expected = vec![
        json!(["task-1", { "metadata": "minimal" }]),
        json!(["task-1", { "metadata": "minimal", "tabId": "w1:t2" }]),
        json!(["task-1"]),
        json!(["task-1"]),
    ];
    assert_eq!(server.fake.calls("get_task_surfaces"), expected);
}

#[tokio::test]
async fn icon_deduplication_applies_only_on_request_and_keeps_authentication() {
    let server = TestServer::start().await;
    assert_eq!(
        server
            .get("/api/mobile/board?iconFormat=deduplicated", Some(GOOD))
            .await
            .status,
        200
    );
    server.get("/api/mobile/board", Some(GOOD)).await;
    assert_eq!(
        server.fake.calls("get_mobile_board"),
        vec![json!(true), json!(false)]
    );
    assert_eq!(
        server
            .get("/api/mobile/board?iconFormat=deduplicated", None)
            .await
            .status,
        401
    );
    assert_eq!(server.fake.count("get_mobile_board"), 2);
}

#[tokio::test]
async fn panes_without_a_session_answer_409_with_the_reason() {
    let server = TestServer::start().await;
    server.fake.fails(
        "get_task_surfaces",
        BridgeError::SurfaceUnavailable("session-missing".into()),
    );
    let reply = server
        .get("/api/mobile/tasks/task-1/surfaces", Some(GOOD))
        .await;
    assert_eq!(reply.pair(), (409, reason_failure("session-missing")));
}

#[tokio::test]
async fn other_surface_errors_hide_the_internal_message() {
    let server = TestServer::start().await;
    server.fake.fails(
        "get_task_surfaces",
        unexpected("SSH 호스트를 찾을 수 없습니다: secret-alias"),
    );
    let reply = server
        .get("/api/mobile/tasks/task-1/surfaces", Some(GOOD))
        .await;
    assert_eq!(reply.pair(), (500, failure("서버 오류")));
    assert!(!reply.body.contains("secret-alias"));
}

#[tokio::test]
async fn a_throwing_board_read_answers_500() {
    let server = TestServer::start().await;
    server
        .fake
        .fails("get_mobile_board", unexpected("DB가 닫혔다"));
    assert_eq!(
        server.get("/api/mobile/board", Some(GOOD)).await.pair(),
        (500, failure("서버 오류"))
    );
}

#[tokio::test]
async fn broken_percent_encoding_in_the_path_answers_500() {
    let server = TestServer::start().await;
    let reply = server.get("/api/mobile/tasks/%/surfaces", Some(GOOD)).await;
    assert_eq!(reply.pair(), (500, failure("서버 오류")));
}

#[tokio::test]
async fn a_throwing_authorization_lookup_answers_500() {
    let server = TestServer::start().await;
    server
        .fake
        .fails("authorize_mobile_request", unexpected("DB가 닫혔다"));
    assert_eq!(
        server.get("/api/mobile/board", None).await.pair(),
        (500, failure("서버 오류"))
    );
}

fn draft() -> Value {
    json!({ "projectId": "project-1", "branchName": "feat/from-mobile" })
}

fn create_task(body: &Value) -> HttpCall {
    HttpCall::new("POST", "/api/mobile/tasks")
        .auth(GOOD)
        .json(body)
}

#[tokio::test]
async fn task_drafts_are_passed_to_the_service_and_the_created_task_returned() {
    let server = TestServer::start().await;
    let reply = server.request(create_task(&draft())).await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.json()["task"],
        json!({ "id": "task-1", "title": "feat/from-mobile" })
    );
    assert_eq!(server.fake.calls("create_mobile_task"), vec![draft()]);
}

#[tokio::test]
async fn the_chosen_session_type_is_passed_to_task_creation() {
    let server = TestServer::start().await;
    let mut body = draft();
    body["sessionType"] = json!("tmux");
    server.request(create_task(&body)).await;
    assert_eq!(
        server.fake.calls("create_mobile_task")[0]["sessionType"],
        "tmux"
    );
}

#[tokio::test]
async fn task_creation_without_a_token_is_stopped_by_authentication() {
    let server = TestServer::start().await;
    let reply = server
        .request(HttpCall::new("POST", "/api/mobile/tasks").json(&draft()))
        .await;
    assert_eq!(reply.status, 401);
    assert_eq!(server.fake.count("create_mobile_task"), 0);
}

#[tokio::test]
async fn task_creation_without_project_or_branch_is_400() {
    let server = TestServer::start().await;
    let blank_branch = json!({ "projectId": "project-1", "branchName": "   " });
    for body in [
        json!({ "branchName": "feat/x" }),
        json!({ "projectId": "project-1" }),
        blank_branch,
    ] {
        assert_eq!(
            server.request(create_task(&body)).await.status,
            400,
            "{body}"
        );
    }
    assert_eq!(server.fake.count("create_mobile_task"), 0);
}

/// TS는 문자열을 `JSON.stringify`해 보냈다. 읽히는 JSON이지만 객체가 아니라 두 칸을 찾지 못한다
#[tokio::test]
async fn non_object_task_bodies_are_400() {
    let server = TestServer::start().await;
    assert_eq!(
        server
            .request(create_task(&json!("깨진 본문")))
            .await
            .status,
        400
    );
    assert_eq!(server.fake.count("create_mobile_task"), 0);
}

#[tokio::test]
async fn invalid_value_errors_return_their_reason() {
    let server = TestServer::start().await;
    server.fake.fails(
        "create_mobile_task",
        input_error("브랜치 이름에 쓸 수 없는 문자가 있습니다"),
    );
    let reply = server.request(create_task(&draft())).await;
    assert_eq!(
        reply.pair(),
        (400, failure("브랜치 이름에 쓸 수 없는 문자가 있습니다"))
    );
}

#[tokio::test]
async fn other_task_creation_errors_hide_the_internal_message() {
    let server = TestServer::start().await;
    server.fake.fails(
        "create_mobile_task",
        unexpected("git -C /home/me/repo worktree add 실패"),
    );
    assert_eq!(
        server.request(create_task(&draft())).await.pair(),
        (500, failure("서버 오류"))
    );
}

fn move_task(body: &Value) -> HttpCall {
    HttpCall::new("PATCH", "/api/mobile/tasks/task-1/status").json(body)
}

#[tokio::test]
async fn known_statuses_move_the_task() {
    let server = TestServer::start().await;
    let reply = server
        .request(move_task(&json!({ "status": "review" })).auth(GOOD))
        .await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.json()["task"],
        json!({ "id": "task-1", "status": "review" })
    );
    assert_eq!(
        server.fake.calls("update_mobile_task_status"),
        vec![json!(["task-1", "review"])]
    );
}

#[tokio::test]
async fn moving_without_a_token_is_stopped_by_authentication() {
    let server = TestServer::start().await;
    assert_eq!(
        server
            .request(move_task(&json!({ "status": "review" })))
            .await
            .status,
        401
    );
    assert_eq!(server.fake.count("update_mobile_task_status"), 0);
}

#[tokio::test]
async fn moving_without_a_status_is_400() {
    let server = TestServer::start().await;
    assert_eq!(
        server
            .request(move_task(&json!({})).auth(GOOD))
            .await
            .status,
        400
    );
    assert_eq!(server.fake.count("update_mobile_task_status"), 0);
}

#[tokio::test]
async fn tasks_deleted_before_the_move_are_404() {
    let server = TestServer::start().await;
    server
        .fake
        .returns("update_mobile_task_status", Value::Null);
    assert_eq!(
        server
            .request(move_task(&json!({ "status": "done" })).auth(GOOD))
            .await
            .status,
        404
    );
}

#[tokio::test]
async fn unknown_status_errors_return_their_reason() {
    let server = TestServer::start().await;
    server.fake.fails(
        "update_mobile_task_status",
        input_error("상태 값이 올바르지 않습니다"),
    );
    let reply = server
        .request(move_task(&json!({ "status": "archived" })).auth(GOOD))
        .await;
    assert_eq!(reply.pair(), (400, failure("상태 값이 올바르지 않습니다")));
}

fn change_session_type(body: &Value) -> HttpCall {
    HttpCall::new("PATCH", "/api/mobile/tasks/task-1/session-type").json(body)
}

#[tokio::test]
async fn session_type_and_close_flag_are_passed_and_the_changed_task_returned() {
    let server = TestServer::start().await;
    let body = json!({ "sessionType": "zellij", "shouldCloseExistingSession": true });
    let reply = server.request(change_session_type(&body).auth(GOOD)).await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.json()["task"],
        json!({ "id": "task-1", "sessionType": "zellij" })
    );
    assert_eq!(
        server.fake.calls("change_mobile_task_session_type"),
        vec![json!(["task-1", "zellij", true])]
    );
}

#[tokio::test]
async fn a_non_true_close_flag_keeps_the_existing_session() {
    let server = TestServer::start().await;
    let body = json!({ "sessionType": "zellij", "shouldCloseExistingSession": "yes" });
    server.request(change_session_type(&body).auth(GOOD)).await;
    assert_eq!(
        server.fake.calls("change_mobile_task_session_type"),
        vec![json!(["task-1", "zellij", false])]
    );
}

#[tokio::test]
async fn session_type_changes_need_a_token() {
    let server = TestServer::start().await;
    assert_eq!(
        server
            .request(change_session_type(&json!({ "sessionType": "zellij" })))
            .await
            .status,
        401
    );
    assert_eq!(server.fake.count("change_mobile_task_session_type"), 0);
}

#[tokio::test]
async fn session_type_changes_without_a_type_are_400() {
    let server = TestServer::start().await;
    let reply = server
        .request(change_session_type(&json!({ "shouldCloseExistingSession": true })).auth(GOOD))
        .await;
    assert_eq!(reply.status, 400);
    assert_eq!(server.fake.count("change_mobile_task_session_type"), 0);
}

#[tokio::test]
async fn rejected_session_types_return_400_with_the_reason() {
    let server = TestServer::start().await;
    server.fake.fails(
        "change_mobile_task_session_type",
        input_error("세션 타입 값이 올바르지 않습니다"),
    );
    let reply = server
        .request(change_session_type(&json!({ "sessionType": "screen" })).auth(GOOD))
        .await;
    assert_eq!(
        reply.pair(),
        (400, failure("세션 타입 값이 올바르지 않습니다"))
    );
}

#[tokio::test]
async fn a_missing_session_tool_answers_409_with_the_install_hint() {
    let server = TestServer::start().await;
    let message = "zellij이(가) 설치되어 있지 않습니다. 데스크탑에서 설치한 뒤 다시 시도해 주세요.";
    server.fake.fails(
        "change_mobile_task_session_type",
        BridgeError::SessionDependencyMissing(message.into()),
    );
    let reply = server
        .request(change_session_type(&json!({ "sessionType": "zellij" })).auth(GOOD))
        .await;
    assert_eq!(reply.status, 409);
    assert!(
        reply.json()["error"]
            .as_str()
            .is_some_and(|error| error.contains("데스크탑에서 설치"))
    );
}

#[tokio::test]
async fn session_type_changes_for_missing_tasks_are_404() {
    let server = TestServer::start().await;
    server
        .fake
        .returns("change_mobile_task_session_type", Value::Null);
    let reply = server
        .request(change_session_type(&json!({ "sessionType": "zellij" })).auth(GOOD))
        .await;
    assert_eq!(reply.status, 404);
    assert_eq!(
        server.fake.calls("change_mobile_task_session_type"),
        vec![json!(["task-1", "zellij", false])]
    );
}

#[tokio::test]
async fn session_liveness_is_reported() {
    let server = TestServer::start().await;
    server
        .fake
        .returns("is_mobile_task_session_alive", json!(false));
    let reply = server
        .get("/api/mobile/tasks/task-1/session-alive", Some(GOOD))
        .await;
    assert_eq!(
        reply.pair(),
        (200, json!({ "success": true, "isAlive": false }))
    );
    assert_eq!(
        server.fake.calls("is_mobile_task_session_alive"),
        vec![json!("task-1")]
    );
}

#[tokio::test]
async fn paired_devices_get_the_branch_list() {
    let server = TestServer::start().await;
    let reply = server
        .get("/api/mobile/projects/project-1/branches", Some(GOOD))
        .await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.json()["branches"], json!(["main", "dev"]));
    assert_eq!(
        server.fake.calls("get_mobile_project_branches"),
        vec![json!("project-1")]
    );
}

#[tokio::test]
async fn branch_lists_need_a_token() {
    let server = TestServer::start().await;
    assert_eq!(
        server
            .get("/api/mobile/projects/project-1/branches", None)
            .await
            .status,
        401
    );
    assert_eq!(server.fake.count("get_mobile_project_branches"), 0);
}

#[tokio::test]
async fn a_throwing_branch_read_answers_500() {
    let server = TestServer::start().await;
    server
        .fake
        .fails("get_mobile_project_branches", unexpected("git 실패"));
    assert_eq!(
        server
            .get("/api/mobile/projects/project-1/branches", Some(GOOD))
            .await
            .status,
        500
    );
}

fn shortcut(body: &Value) -> HttpCall {
    HttpCall::new("POST", SHORTCUT_PATH).auth(GOOD).json(body)
}

#[tokio::test]
async fn unauthenticated_shortcuts_do_not_run() {
    let server = TestServer::start().await;
    let reply = server
        .request(HttpCall::new("POST", SHORTCUT_PATH).json(&json!({ "action": "split-right" })))
        .await;
    assert_eq!(reply.status, 401);
    assert_eq!(server.fake.count("execute_mobile_terminal_shortcut"), 0);
}

#[tokio::test]
async fn only_the_path_target_and_action_reach_the_service() {
    let server = TestServer::start().await;
    let reply = server
        .request(shortcut(
            &json!({ "action": "split-right", "command": "injected", "sshHost": "injected" }),
        ))
        .await;
    assert_eq!(
        server.fake.calls("execute_mobile_terminal_shortcut"),
        vec![json!(["task id", "%19", "split-right"])]
    );
    assert_eq!(
        reply.pair(),
        (
            200,
            json!({ "success": true, "surfaces": { "tabs": [] }, "screen": "screen" })
        )
    );
}

#[tokio::test]
async fn bodies_without_an_action_are_400() {
    let server = TestServer::start().await;
    for body in [
        Value::Null,
        json!({}),
        json!({ "action": 1 }),
        json!({ "action": "" }),
    ] {
        assert_eq!(server.request(shortcut(&body)).await.status, 400, "{body}");
    }
    assert_eq!(server.fake.count("execute_mobile_terminal_shortcut"), 0);
}

#[tokio::test]
async fn actions_rejected_by_the_service_are_400() {
    let server = TestServer::start().await;
    server.fake.fails(
        "execute_mobile_terminal_shortcut",
        input_error("지원하지 않는 단축키입니다"),
    );
    assert_eq!(
        server
            .request(shortcut(&json!({ "action": "unknown" })))
            .await
            .status,
        400
    );
}

#[tokio::test]
async fn panes_of_another_session_are_409() {
    let server = TestServer::start().await;
    server.fake.fails(
        "execute_mobile_terminal_shortcut",
        BridgeError::SurfaceUnavailable("session-not-running".into()),
    );
    let reply = server
        .request(shortcut(&json!({ "action": "split-right" })))
        .await;
    assert_eq!(reply.pair(), (409, reason_failure("session-not-running")));
}

#[tokio::test]
async fn failed_cli_commands_are_not_exposed() {
    let server = TestServer::start().await;
    server.fake.fails(
        "execute_mobile_terminal_shortcut",
        unexpected("ssh secret-host secret-path"),
    );
    let reply = server
        .request(shortcut(&json!({ "action": "split-right" })))
        .await;
    assert_eq!(reply.pair(), (500, failure("서버 오류")));
}
