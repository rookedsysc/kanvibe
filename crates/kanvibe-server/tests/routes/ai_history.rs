//! TS `mobileRoutes.test.mjs`: 모바일 AI History, 실제 Codex fixture 리더, 프로젝트 폴더 등록

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use kanvibe_ai::codex_sessions::{read_codex_session_detail, read_codex_sessions};
use kanvibe_ai::host::{SessionHost, UnavailableCommandRunner};
use kanvibe_ai::types::{AiMessageRole, AiSessionReaderContext};
use kanvibe_server::bridge::BridgeError;
use serde_json::{Value, json};

use crate::harness::*;

const DETAIL_PATH: &str = "/api/mobile/tasks/task-1/ai-sessions/hermes/session-1";

#[tokio::test]
async fn history_list_and_detail_need_authentication() {
    let server = TestServer::start().await;
    for path in ["/api/mobile/tasks/task-1/ai-sessions", DETAIL_PATH] {
        assert_eq!(server.get(path, None).await.status, 401);
    }
    assert_eq!(server.fake.count("get_mobile_ai_sessions"), 0);
}

#[tokio::test]
async fn history_list_passes_search_and_page_values() {
    let server = TestServer::start().await;
    let reply = server
        .get(
            "/api/mobile/tasks/task-1/ai-sessions?query=fix&cursor=20&limit=10",
            Some(GOOD),
        )
        .await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.json()["history"]["sessions"][0],
        json!({ "id": "session-1", "provider": "hermes" })
    );
    let query = json!({ "query": "fix", "cursor": "20", "limit": 10, "roles": null });
    assert_eq!(
        server.fake.calls("get_mobile_ai_sessions"),
        vec![json!(["task-1", query])]
    );
}

#[tokio::test]
async fn history_detail_passes_the_role_filter_and_returns_the_conversation() {
    let server = TestServer::start().await;
    let reply = server
        .get(&format!("{DETAIL_PATH}?roles=user,assistant"), Some(GOOD))
        .await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.json()["detail"]["messages"][0]["fullText"],
        "기록 본문"
    );
    let query =
        json!({ "query": null, "cursor": null, "limit": 20, "roles": ["user", "assistant"] });
    assert_eq!(
        server.fake.calls("get_mobile_ai_session_detail"),
        vec![json!(["task-1", "hermes", "session-1", query])]
    );
}

#[tokio::test]
async fn invalid_history_query_values_are_rejected() {
    let server = TestServer::start().await;
    for query in [
        "limit=0",
        "limit=101",
        "limit=2x",
        "cursor=-1",
        "cursor=garbage",
        "roles=invalid",
        "sourceRef=/private/file",
    ] {
        assert_eq!(
            server
                .get(&format!("{DETAIL_PATH}?{query}"), Some(GOOD))
                .await
                .status,
            400,
            "{query}"
        );
    }
    assert_eq!(server.fake.count("get_mobile_ai_session_detail"), 0);
}

#[tokio::test]
async fn unknown_agents_are_400_and_missing_tasks_or_sessions_are_404() {
    let server = TestServer::start().await;
    assert_eq!(
        server
            .get("/api/mobile/tasks/task-1/ai-sessions/unknown/s", Some(GOOD))
            .await
            .status,
        400
    );
    server.fake.respond("get_mobile_task", |task_id| {
        Ok(if task_id == "deleted" {
            Value::Null
        } else {
            json!({ "id": task_id })
        })
    });
    assert_eq!(
        server
            .get("/api/mobile/tasks/deleted/ai-sessions", Some(GOOD))
            .await
            .status,
        404
    );
    server.fake.respond("get_mobile_ai_session_detail", |args| {
        Ok(if args[2] == "deleted" {
            Value::Null
        } else {
            json!({})
        })
    });
    assert_eq!(
        server
            .get(
                "/api/mobile/tasks/task-1/ai-sessions/hermes/deleted",
                Some(GOOD)
            )
            .await
            .status,
        404
    );
}

const FIXTURE_WORKTREE: &str = "/qa/chat/repo";

fn fixture_context(home: &str) -> AiSessionReaderContext {
    let host = SessionHost::local(home, BTreeMap::new(), Arc::new(UnavailableCommandRunner));
    let mut context = AiSessionReaderContext::new(host, Some(FIXTURE_WORKTREE));
    context.repo_path = Some(FIXTURE_WORKTREE.to_string());
    context
}

fn write_codex_fixture(home: &Path) -> String {
    let directory = home.join(".codex").join("sessions");
    std::fs::create_dir_all(&directory).expect("세션 폴더");
    let lines = [
        json!({ "type": "session_meta", "payload": { "id": "wire-session", "cwd": FIXTURE_WORKTREE } }),
        json!({ "type": "response_item", "timestamp": "2026-11-02T01:00:00Z", "payload": { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "모바일 계약 질문" }] } }),
        json!({ "type": "response_item", "timestamp": "2026-11-02T01:00:01Z", "payload": { "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": "실제 fixture 응답" }] } }),
    ];
    let source_ref = directory.join("fixture.jsonl");
    let content: String = lines.iter().map(|line| format!("{line}\n")).collect();
    std::fs::write(&source_ref, content).expect("fixture 쓰기");
    source_ref.to_string_lossy().into_owned()
}

fn without_source_ref(mut value: Value) -> Value {
    if let Some(fields) = value.as_object_mut() {
        fields.remove("sourceRef");
    }
    value
}

/// 서비스 자리에 실제 Codex 리더를 끼운다(TS `bridge.getMobileAiSessions.mockImplementation`)
fn install_codex_readers(server: &TestServer, home: String, source_ref: String) {
    let list_home = home.clone();
    server.fake.respond("get_mobile_ai_sessions", move |_| {
        let result = read_codex_sessions(&fixture_context(&list_home)).map_err(|error| unexpected(&error.to_string()))?;
        let sessions: Vec<Value> = result.sessions.iter().map(|session| without_source_ref(json!(session))).collect();
        Ok(json!({ "sessions": sessions, "sources": [{ "provider": result.provider, "available": result.available }], "nextCursor": null }))
    });
    server
        .fake
        .respond("get_mobile_ai_session_detail", move |args| {
            let mut context = fixture_context(&home);
            let roles = args[3]["roles"].as_array().map(|roles| {
                roles
                    .iter()
                    .filter_map(|role| AiMessageRole::parse(role.as_str()?))
                    .collect()
            });
            context.roles = roles;
            let limit = args[3]["limit"].as_u64().map(|limit| limit as usize);
            let session_id = args[2].as_str().unwrap_or_default();
            let detail = read_codex_session_detail(
                &context,
                session_id,
                Some(&source_ref),
                args[3]["cursor"].as_str(),
                limit,
            )
            .map_err(|error| BridgeError::Internal(error.to_string()))?;
            Ok(detail.map_or(Value::Null, |detail| without_source_ref(json!(detail))))
        });
}

#[tokio::test]
async fn real_codex_fixture_readers_flow_through_the_http_contract() {
    let server = TestServer::start().await;
    let home_directory = tempfile::tempdir().expect("임시 홈");
    let home = home_directory.path().canonicalize().expect("홈 경로");
    let source_ref = write_codex_fixture(&home);
    install_codex_readers(&server, home.to_string_lossy().into_owned(), source_ref);
    let list = server
        .get("/api/mobile/tasks/task-1/ai-sessions", Some(GOOD))
        .await;
    assert_eq!(list.status, 200);
    let first = &list.json()["history"]["sessions"][0];
    assert_eq!(
        (&first["id"], &first["provider"], &first["title"]),
        (
            &json!("wire-session"),
            &json!("codex"),
            &json!("모바일 계약 질문")
        )
    );
    assert!(first.get("sourceRef").is_none());
    let detail = server
        .get(
            "/api/mobile/tasks/task-1/ai-sessions/codex/wire-session?roles=assistant",
            Some(GOOD),
        )
        .await;
    assert_eq!(detail.status, 200);
    let texts: Vec<Value> = detail.json()["detail"]["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|message| message["fullText"].clone())
        .collect();
    assert_eq!(texts, vec![json!("실제 fixture 응답")]);
}

fn scan(body: &Value) -> HttpCall {
    HttpCall::new("POST", "/api/mobile/projects/scan").json(body)
}

#[tokio::test]
async fn directory_listing_starts_at_home_and_decodes_the_path() {
    let server = TestServer::start().await;
    let reply = server.get("/api/mobile/directories", Some(GOOD)).await;
    assert_eq!(
        reply.pair(),
        (
            200,
            json!({ "success": true, "directories": ["Documents", "Projects"] })
        )
    );
    server
        .get(
            "/api/mobile/directories?path=%2Fworkspace%2Fspace%20repo",
            Some(GOOD),
        )
        .await;
    assert_eq!(
        server.fake.calls("get_mobile_directories"),
        vec![json!("~"), json!("/workspace/space repo")]
    );
}

#[tokio::test]
async fn directory_listing_and_project_scan_need_authentication() {
    let server = TestServer::start().await;
    assert_eq!(
        server.get("/api/mobile/directories", None).await.status,
        401
    );
    assert_eq!(
        server
            .request(scan(&json!({ "rootPath": "/workspace" })))
            .await
            .status,
        401
    );
    assert_eq!(
        server.fake.count("get_mobile_directories") + server.fake.count("scan_mobile_projects"),
        0
    );
}

#[tokio::test]
async fn only_the_chosen_folder_is_scanned_and_the_result_returned() {
    let server = TestServer::start().await;
    let reply = server
        .request(scan(&json!({ "rootPath": "~/Projects" })).auth(GOOD))
        .await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.json()["result"]["registered"], json!(["kanvibe"]));
    assert_eq!(
        server.fake.calls("scan_mobile_projects"),
        vec![json!("~/Projects")]
    );
}

#[tokio::test]
async fn scan_bodies_without_a_root_path_are_rejected() {
    let server = TestServer::start().await;
    let bodies = [
        json!({}),
        json!({ "rootPath": "" }),
        json!({ "rootPath": "   " }),
        json!({ "rootPath": 42 }),
        Value::Null,
        json!([]),
    ];
    for body in bodies {
        assert_eq!(
            server.request(scan(&body).auth(GOOD)).await.status,
            400,
            "{body}"
        );
    }
    assert_eq!(server.fake.count("scan_mobile_projects"), 0);
}

#[tokio::test]
async fn malformed_scan_json_is_rejected_before_the_service() {
    let server = TestServer::start().await;
    let reply = server
        .request(
            HttpCall::new("POST", "/api/mobile/projects/scan")
                .auth(GOOD)
                .body("{"),
        )
        .await;
    assert_eq!(reply.status, 400);
    assert_eq!(server.fake.count("scan_mobile_projects"), 0);
}

#[tokio::test]
async fn already_registered_folders_show_the_skipped_result() {
    let server = TestServer::start().await;
    let result = json!({ "registered": [], "skipped": ["/workspace/kanvibe"], "errors": [], "worktreeTasks": [], "registeredWorktrees": [], "hooksSetup": [] });
    server.fake.returns("scan_mobile_projects", result);
    let reply = server
        .request(scan(&json!({ "rootPath": "/workspace" })).auth(GOOD))
        .await;
    assert_eq!(
        reply.json()["result"]["skipped"],
        json!(["/workspace/kanvibe"])
    );
}

#[tokio::test]
async fn missing_folders_answer_the_service_safe_message() {
    let server = TestServer::start().await;
    server.fake.fails(
        "get_mobile_directories",
        BridgeError::TaskInput("폴더를 찾을 수 없습니다".into()),
    );
    let reply = server
        .get("/api/mobile/directories?path=%2Fmissing", Some(GOOD))
        .await;
    assert_eq!(reply.pair(), (400, failure("폴더를 찾을 수 없습니다")));
}

#[tokio::test]
async fn internal_scan_errors_are_hidden() {
    let server = TestServer::start().await;
    server
        .fake
        .fails("scan_mobile_projects", unexpected("secret filesystem path"));
    let reply = server
        .request(scan(&json!({ "rootPath": "/workspace" })).auth(GOOD))
        .await;
    assert_eq!(reply.pair(), (500, failure("서버 오류")));
}
