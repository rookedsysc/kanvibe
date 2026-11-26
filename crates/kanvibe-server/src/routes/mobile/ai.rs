//! AI 경로(TS `electron/mobileAiRoutes.js` `handleMobileAiRequest`). 인증은 앞단에서 이미 거쳤다.

use http::Method;
use serde_json::{Value, json};

use super::{MobileRequest, RouteError, RouteResult, spread_success, success_with};
use crate::bridge::BridgeError;
use crate::http::body::{HttpResponse, failure_response, json_response};
use crate::routes::js_semantics::decode_uri_component;

const ACCOUNT_LIST_PATH: &str = "/api/mobile/ai/accounts";
const REQUEST_FORMAT_ERROR: &str = "요청 형식이 올바르지 않습니다";

/// 처리한 경로가 아니면 `None`을 돌려 기본 경로가 이어 본다
pub async fn handle_ai_request(mobile: &mut MobileRequest<'_>) -> RouteResult {
    match dispatch_ai_request(mobile).await {
        Ok(response) => Ok(response),
        Err(RouteError::Abort) => Err(RouteError::Abort),
        Err(RouteError::Unexpected) => Ok(Some(failure_response(400, REQUEST_FORMAT_ERROR))),
        Err(RouteError::Bridge(error)) => Ok(Some(ai_failure_response(&error))),
    }
}

/// TS `responseError`: 알려진 상태 코드만 메시지를 내보낸다
fn ai_failure_response(error: &BridgeError) -> HttpResponse {
    let passthrough = match error {
        BridgeError::Ai {
            status: Some(status),
            message,
        } => Some((*status, message.as_str())),
        BridgeError::Feature { status, message } => Some((*status, message.as_str())),
        _ => None,
    };
    match passthrough {
        Some((status, message)) if matches!(status, 400 | 403 | 404 | 409 | 429) => {
            failure_response(status, message)
        }
        _ => failure_response(500, "AI 기능 요청을 처리하지 못했습니다"),
    }
}

async fn dispatch_ai_request(mobile: &mut MobileRequest<'_>) -> RouteResult {
    if mobile.is(Method::GET, "/api/mobile/ai/running-panes") {
        let panes = mobile
            .bridge(|bridge| bridge.get_mobile_running_agent_panes())
            .await?;
        return Ok(Some(success_with("panes", panes)));
    }
    let is_get = mobile.method_is(&Method::GET);
    if let Some(ids) = mobile
        .url
        .capture("/api/mobile/tasks/*/live-ai-sessions")
        .filter(|_| is_get)
    {
        let task_id = decode_uri_component(&ids[0])?;
        let sessions = mobile
            .bridge(move |bridge| bridge.get_mobile_live_ai_sessions(&task_id))
            .await?;
        return Ok(Some(success_with("liveSessions", sessions)));
    }
    if let Some(ids) = mobile
        .url
        .capture("/api/mobile/tasks/*/agent-call-graph")
        .filter(|_| is_get)
    {
        return handle_call_graph(mobile, &ids[0]).await.map(Some);
    }
    handle_account_routes(mobile).await
}

async fn handle_call_graph(
    mobile: &MobileRequest<'_>,
    raw_task_id: &str,
) -> Result<HttpResponse, RouteError> {
    let provider = mobile
        .url
        .param("provider")
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let session_id = mobile
        .url
        .param("sessionId")
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let (Some(provider), Some(session_id)) = (provider, session_id) else {
        return Ok(failure_response(400, "AI 제공자와 세션을 선택해 주세요"));
    };
    let task_id = decode_uri_component(raw_task_id)?;
    let graph = mobile
        .bridge(move |bridge| bridge.get_mobile_agent_call_graph(&task_id, &provider, &session_id))
        .await?;
    Ok(success_with("graph", graph))
}

async fn handle_account_routes(mobile: &mut MobileRequest<'_>) -> RouteResult {
    if mobile.is(Method::GET, ACCOUNT_LIST_PATH) {
        let accounts = mobile
            .bridge(|bridge| bridge.list_mobile_ai_accounts())
            .await?;
        return Ok(Some(success_with("accounts", accounts)));
    }
    if mobile.is(Method::POST, ACCOUNT_LIST_PATH) {
        let body = mobile
            .read_default_json()
            .await?
            .ok_or(RouteError::Unexpected)?;
        let added = mobile
            .bridge(move |bridge| bridge.add_mobile_ai_account(&body))
            .await?;
        return Ok(Some(json_response(
            201,
            &spread_success(Value::Object(added)),
        )));
    }
    let is_delete = mobile.method_is(&Method::DELETE);
    if let Some(ids) = mobile
        .url
        .capture("/api/mobile/ai/accounts/*")
        .filter(|_| is_delete)
    {
        let account_id = decode_uri_component(&ids[0])?;
        let removed = mobile
            .bridge(move |bridge| bridge.remove_mobile_ai_account(&account_id))
            .await?;
        return Ok(Some(json_response(
            200,
            &spread_success(Value::Object(removed)),
        )));
    }
    if mobile.is(Method::GET, "/api/mobile/ai/usage") {
        let usage = mobile
            .bridge(|bridge| bridge.get_mobile_ai_usage_snapshot())
            .await?;
        return Ok(Some(json_response(
            200,
            &json!({ "success": true, "usage": usage }),
        )));
    }
    Ok(None)
}
