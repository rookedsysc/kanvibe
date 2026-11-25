//! hook·task CLI 경로(TS `hookServer.js` `handleHookRequest`, `taskCliRoutes.js`).
//!
//! 이 경로들은 `request.url` 전체가 같아야 맞는다. 쿼리 문자열이 붙거나 메서드가 다르면 404다.

use std::collections::HashSet;

use http::Method;
use serde_json::{Map, Value, json};

use crate::http::HttpRequest;
use crate::http::body::{
    ConnectionAbort, HttpResponse, MAX_BODY_BYTES, json_response, not_found_response,
    shell_script_response,
};
use crate::server::ServerState;

pub const HOOK_HEALTH_PATH: &str = "/api/hooks/health";
pub const HOOK_INSTALL_SCRIPT_PATH: &str = "/api/install/hooks.sh";
const HOOK_STATUS_PATH: &str = "/api/hooks/status";
const HOOK_INSTALL_TOKEN_HEADER: &str = "x-kanvibe-install-token";

/// task CLI 경로 하나와 성공 상태 코드
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskCliRoute {
    Create,
    Delete,
    Register,
    List,
    Session,
}

impl TaskCliRoute {
    fn from_url(raw_url: &str) -> Option<Self> {
        match raw_url {
            "/api/cli/tasks" => Some(Self::Create),
            "/api/cli/tasks/delete" => Some(Self::Delete),
            "/api/cli/tasks/register" => Some(Self::Register),
            "/api/cli/tasks/list" => Some(Self::List),
            "/api/cli/tasks/session" => Some(Self::Session),
            _ => None,
        }
    }

    fn success_status(self) -> u16 {
        if self == Self::Create { 201 } else { 200 }
    }
}

/// task CLI 서비스의 실패. `Request`는 상태 코드와 메시지를 그대로, `Internal`은 500과 원문 메시지로 답한다
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskCliFailure {
    Request { status: u16, message: String },
    Internal(String),
}

/// hook 경로가 부르는 서비스(hookService, taskCliService, hookInstallBundle)
pub trait HookBackend: Send + Sync {
    /// 응답 상태 코드와 본문. 거절 본문에는 TS처럼 숫자 `status` 칸이 함께 실린다
    fn update_hook_task_status(
        &self,
        body: &Value,
        authorization: Option<&str>,
        source_ssh_hosts: Option<&HashSet<String>>,
    ) -> (u16, Value);
    fn run_task_cli(
        &self,
        route: TaskCliRoute,
        authorization: Option<&str>,
        body: &Map<String, Value>,
        source_ssh_hosts: Option<&HashSet<String>>,
    ) -> Result<Value, TaskCliFailure>;
    /// 토큰이 맞을 때만 설치 스크립트
    fn read_hook_install_script(&self, token: Option<&str>) -> Option<String>;
}

pub async fn handle_hook_request(
    mut request: HttpRequest,
    state: &ServerState,
) -> Result<HttpResponse, ConnectionAbort> {
    if request.method == Method::POST
        && let Some(route) = TaskCliRoute::from_url(&request.raw_url)
    {
        return handle_task_cli_request(&mut request, route, state).await;
    }
    let is_get = request.method == Method::GET;
    if is_get && request.raw_url == HOOK_HEALTH_PATH {
        return Ok(json_response(200, &json!({ "success": true })));
    }
    if is_get && request.raw_url == HOOK_INSTALL_SCRIPT_PATH {
        let token = request
            .header(HOOK_INSTALL_TOKEN_HEADER)
            .map(str::to_string);
        let hooks = state.hooks.clone();
        let script =
            tokio::task::spawn_blocking(move || hooks.read_hook_install_script(token.as_deref()))
                .await
                .ok()
                .flatten();
        return Ok(script.map_or_else(not_found_response, shell_script_response));
    }
    if request.method == Method::POST && request.raw_url == HOOK_STATUS_PATH {
        return handle_hook_status_request(&mut request, state).await;
    }
    Ok(not_found_response())
}

/// 본문을 읽지 못하면(JSON 오류) 서비스를 부르지 않고 500과 파서 메시지로 답한다
async fn handle_hook_status_request(
    request: &mut HttpRequest,
    state: &ServerState,
) -> Result<HttpResponse, ConnectionAbort> {
    let body = match request.body.read_json(MAX_BODY_BYTES).await? {
        Ok(body) => body,
        Err(message) => {
            return Ok(json_response(
                500,
                &json!({ "success": false, "error": message }),
            ));
        }
    };
    let hooks = state.hooks.clone();
    let authorization = request.authorization().map(str::to_string);
    let hosts = request.peer.tunnel_hosts();
    let outcome = tokio::task::spawn_blocking(move || {
        hooks.update_hook_task_status(&body, authorization.as_deref(), hosts.as_ref())
    })
    .await;
    Ok(match outcome {
        Ok((status, payload)) => json_response(status, &payload),
        Err(_) => json_response(500, &json!({ "success": false, "error": "서버 오류" })),
    })
}

async fn handle_task_cli_request(
    request: &mut HttpRequest,
    route: TaskCliRoute,
    state: &ServerState,
) -> Result<HttpResponse, ConnectionAbort> {
    let body = match request.body.read_json(MAX_BODY_BYTES).await? {
        Ok(Value::Object(body)) => body,
        Ok(_) => return Ok(task_cli_failure(400, "JSON 객체 본문이 필요합니다")),
        Err(_) => return Ok(task_cli_failure(400, "JSON 본문을 읽을 수 없습니다")),
    };
    let hooks = state.hooks.clone();
    let authorization = request.authorization().map(str::to_string);
    let hosts = request.peer.tunnel_hosts();
    let outcome = tokio::task::spawn_blocking(move || {
        hooks.run_task_cli(route, authorization.as_deref(), &body, hosts.as_ref())
    })
    .await
    .unwrap_or_else(|_| Err(TaskCliFailure::Internal("서버 오류".to_string())));
    Ok(match outcome {
        Ok(data) => json_response(
            route.success_status(),
            &json!({ "success": true, "data": data }),
        ),
        Err(TaskCliFailure::Request { status, message }) => task_cli_failure(status, &message),
        Err(TaskCliFailure::Internal(message)) => task_cli_failure(500, &message),
    })
}

fn task_cli_failure(status: u16, message: &str) -> HttpResponse {
    json_response(status, &json!({ "success": false, "error": message }))
}
