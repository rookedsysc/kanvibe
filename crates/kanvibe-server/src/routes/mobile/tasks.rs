//! 태스크·pane 경로(TS `handleMobileRequest`의 태스크 조회부터 브랜치 목록까지)

use http::{HeaderValue, Method};
use serde_json::{Map, Value, json};

use super::{
    BAD_REQUEST_ERROR, MobileRequest, RouteError, RouteResult, SERVER_ERROR, UNPAIRED_DEVICE_ERROR,
    spread_success, success_with,
};
use crate::bridge::BridgeError;
use crate::http::body::{HttpResponse, failure_response, json_response};
use crate::routes::js_semantics::{decode_uri_component, optional_property, read_text};

pub async fn handle_task_routes(mobile: &mut MobileRequest<'_>) -> RouteResult {
    if let Some(response) = handle_task_reads(mobile).await? {
        return Ok(Some(response));
    }
    if let Some(response) = handle_surface_routes(mobile).await? {
        return Ok(Some(response));
    }
    handle_task_writes(mobile).await
}

async fn handle_task_reads(mobile: &mut MobileRequest<'_>) -> RouteResult {
    let is_get = mobile.method_is(&Method::GET);
    if let Some(ids) = mobile.url.capture("/api/mobile/tasks/*").filter(|_| is_get) {
        let task_id = decode_uri_component(&ids[0])?;
        let task = mobile
            .bridge(move |bridge| bridge.get_mobile_task(&task_id))
            .await?;
        return Ok(Some(match task {
            Some(task) => success_with("task", task),
            None => failure_response(404, "없는 태스크입니다"),
        }));
    }
    let is_post = mobile.method_is(&Method::POST);
    if let Some(ids) = mobile
        .url
        .capture("/api/mobile/tasks/*/panes/*/connection")
        .filter(|_| is_post)
    {
        let mut response = handle_pane_connection(mobile, &ids).await?;
        response.headers_mut().insert(
            http::header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        );
        return Ok(Some(response));
    }
    if let Some(ids) = mobile
        .url
        .capture("/api/mobile/tasks/*/surfaces")
        .filter(|_| is_get)
    {
        let task_id = decode_uri_component(&ids[0])?;
        let minimal = (mobile.url.param("metadata") == Some("minimal")).then(|| {
            mobile
                .url
                .param("tabId")
                .filter(|tab| !tab.is_empty())
                .map(str::to_string)
        });
        let outcome = mobile
            .bridge(move |bridge| bridge.get_task_surfaces(&task_id, minimal))
            .await;
        return Ok(Some(surface_response(
            outcome.map(|surfaces| success_with("surfaces", surfaces)),
        )));
    }
    Ok(None)
}

/// 평문 연결이면 권한을 내주지 않는다. 준비 중에 기기가 끊겼으면 401로 답한다
async fn handle_pane_connection(
    mobile: &MobileRequest<'_>,
    ids: &[String],
) -> Result<HttpResponse, RouteError> {
    if !mobile.request.peer.is_encrypted() {
        return Ok(json_response(
            200,
            &json!({ "success": true, "connection": null }),
        ));
    }
    let task_id = decode_uri_component(&ids[0])?;
    let pane_id = decode_uri_component(&ids[1])?;
    let device_id = mobile.device_id.clone();
    let prepared = mobile
        .bridge(move |bridge| bridge.prepare_mobile_pane_connection(&task_id, &pane_id, &device_id))
        .await;
    if !is_same_device(mobile).await? {
        return Ok(failure_response(401, UNPAIRED_DEVICE_ERROR));
    }
    match prepared {
        Ok(connection) => Ok(success_with("connection", connection)),
        Err(BridgeError::SurfaceUnavailable(reason)) => Ok(json_response(
            409,
            &json!({ "success": false, "reason": reason }),
        )),
        Err(error) => Err(error.into()),
    }
}

async fn is_same_device(mobile: &MobileRequest<'_>) -> Result<bool, RouteError> {
    let authorization = mobile.request.authorization().map(str::to_string);
    let current = mobile
        .bridge(move |bridge| bridge.authorize_mobile_request(authorization.as_deref()))
        .await?;
    Ok(current.as_deref() == Some(mobile.device_id.as_str()))
}

/// 세션이 없거나 꺼진 것은 화면이 안내할 상태라 409와 사유로 답하고, 그 밖의 실패는 원문을 감춘다
fn surface_response(outcome: Result<HttpResponse, BridgeError>) -> HttpResponse {
    match outcome {
        Ok(response) => response,
        Err(BridgeError::SurfaceUnavailable(reason)) => {
            json_response(409, &json!({ "success": false, "reason": reason }))
        }
        Err(_) => failure_response(500, SERVER_ERROR),
    }
}

/// TS `writeTaskWriteFailure`: 입력 오류만 메시지를 내보낸다
fn task_write_failure(error: &BridgeError) -> HttpResponse {
    match error {
        BridgeError::TaskInput(message) => failure_response(400, message),
        _ => failure_response(500, SERVER_ERROR),
    }
}

async fn handle_surface_routes(mobile: &mut MobileRequest<'_>) -> RouteResult {
    let is_post = mobile.method_is(&Method::POST);
    if let Some(ids) = mobile
        .url
        .capture("/api/mobile/tasks/*/panes/*/shortcuts")
        .filter(|_| is_post)
    {
        let task_id = decode_uri_component(&ids[0])?;
        let pane_id = decode_uri_component(&ids[1])?;
        return handle_pane_shortcut(mobile, task_id, pane_id)
            .await
            .map(Some);
    }
    if let Some(ids) = mobile
        .url
        .capture("/api/mobile/tasks/*/tabs")
        .filter(|_| is_post)
    {
        let task_id = decode_uri_component(&ids[0])?;
        let outcome = mobile
            .bridge(move |bridge| bridge.create_mobile_terminal_tab(&task_id))
            .await;
        let response =
            outcome.map(|created| json_response(200, &spread_success(Value::Object(created))));
        return Ok(Some(surface_response(response)));
    }
    if mobile.is(Method::POST, "/api/mobile/tasks") {
        return handle_create_task(mobile).await.map(Some);
    }
    Ok(None)
}

async fn handle_pane_shortcut(
    mobile: &mut MobileRequest<'_>,
    task_id: String,
    pane_id: String,
) -> Result<HttpResponse, RouteError> {
    let Some(body) = mobile.read_default_json().await? else {
        return Ok(failure_response(400, BAD_REQUEST_ERROR));
    };
    let Some(action) = optional_property(&body, "action")
        .and_then(Value::as_str)
        .filter(|action| !action.is_empty())
    else {
        return Ok(failure_response(400, "단축키 동작이 필요합니다"));
    };
    let action = action.to_string();
    let outcome = mobile
        .bridge(move |bridge| bridge.execute_mobile_terminal_shortcut(&task_id, &pane_id, &action))
        .await;
    Ok(match outcome {
        Ok(result) => json_response(200, &spread_success(Value::Object(result))),
        Err(BridgeError::SurfaceUnavailable(reason)) => {
            json_response(409, &json!({ "success": false, "reason": reason }))
        }
        Err(error) => task_write_failure(&error),
    })
}

/// 프로젝트와 브랜치 이름이 모두 있어야 만든다. 데스크탑 생성 화면이 제출을 막는 두 칸과 같다
async fn handle_create_task(mobile: &mut MobileRequest<'_>) -> Result<HttpResponse, RouteError> {
    let Some(body) = mobile.read_default_json().await? else {
        return Ok(failure_response(400, BAD_REQUEST_ERROR));
    };
    reject_null_body(&body)?;
    let text = |name: &str| read_text(optional_property(&body, name));
    let (Some(project_id), Some(branch_name)) = (text("projectId"), text("branchName")) else {
        return Ok(failure_response(400, "프로젝트와 브랜치 이름이 필요합니다"));
    };
    let mut draft = Map::new();
    draft.insert("projectId".into(), project_id.into());
    draft.insert("branchName".into(), branch_name.into());
    for name in ["baseBranch", "description", "priority"] {
        if let Some(value) = text(name) {
            draft.insert(name.into(), value.into());
        }
    }
    if let Some(session_type) = optional_property(&body, "sessionType") {
        draft.insert("sessionType".into(), session_type.clone());
    }
    let draft = Value::Object(draft);
    let outcome = mobile
        .bridge(move |bridge| bridge.create_mobile_task(&draft))
        .await;
    Ok(match outcome {
        Ok(task) => success_with("task", task),
        Err(error) => task_write_failure(&error),
    })
}

async fn handle_task_writes(mobile: &mut MobileRequest<'_>) -> RouteResult {
    let is_patch = mobile.method_is(&Method::PATCH);
    if let Some(ids) = mobile
        .url
        .capture("/api/mobile/tasks/*/status")
        .filter(|_| is_patch)
    {
        let task_id = decode_uri_component(&ids[0])?;
        return handle_task_status(mobile, task_id).await.map(Some);
    }
    if let Some(ids) = mobile
        .url
        .capture("/api/mobile/tasks/*/session-type")
        .filter(|_| is_patch)
    {
        let task_id = decode_uri_component(&ids[0])?;
        return handle_session_type(mobile, task_id).await.map(Some);
    }
    let is_get = mobile.method_is(&Method::GET);
    if let Some(ids) = mobile
        .url
        .capture("/api/mobile/tasks/*/session-alive")
        .filter(|_| is_get)
    {
        let task_id = decode_uri_component(&ids[0])?;
        let is_alive = mobile
            .bridge(move |bridge| bridge.is_mobile_task_session_alive(&task_id))
            .await?;
        return Ok(Some(json_response(
            200,
            &json!({ "success": true, "isAlive": is_alive }),
        )));
    }
    if let Some(ids) = mobile
        .url
        .capture("/api/mobile/projects/*/branches")
        .filter(|_| is_get)
    {
        let project_id = decode_uri_component(&ids[0])?;
        let branches = mobile
            .bridge(move |bridge| bridge.get_mobile_project_branches(&project_id))
            .await?;
        return Ok(Some(success_with("branches", branches)));
    }
    Ok(None)
}

async fn handle_task_status(
    mobile: &mut MobileRequest<'_>,
    task_id: String,
) -> Result<HttpResponse, RouteError> {
    let Some(body) = mobile.read_default_json().await? else {
        return Ok(failure_response(400, BAD_REQUEST_ERROR));
    };
    reject_null_body(&body)?;
    let Some(status) = read_text(optional_property(&body, "status")) else {
        return Ok(failure_response(400, "상태가 필요합니다"));
    };
    let outcome = mobile
        .bridge(move |bridge| bridge.update_mobile_task_status(&task_id, &status))
        .await;
    Ok(match outcome {
        Ok(Some(task)) => success_with("task", task),
        Ok(None) => failure_response(404, "없는 태스크입니다"),
        Err(error) => task_write_failure(&error),
    })
}

/// 기존 세션은 기기가 명시적으로 `true`를 보낼 때만 닫는다
async fn handle_session_type(
    mobile: &mut MobileRequest<'_>,
    task_id: String,
) -> Result<HttpResponse, RouteError> {
    let Some(body) = mobile.read_default_json().await? else {
        return Ok(failure_response(400, BAD_REQUEST_ERROR));
    };
    reject_null_body(&body)?;
    let Some(session_type) = read_text(optional_property(&body, "sessionType")) else {
        return Ok(failure_response(400, "세션 타입이 필요합니다"));
    };
    let should_close =
        optional_property(&body, "shouldCloseExistingSession") == Some(&Value::Bool(true));
    let outcome = mobile
        .bridge(move |bridge| {
            bridge.change_mobile_task_session_type(&task_id, &session_type, should_close)
        })
        .await;
    Ok(match outcome {
        Ok(Some(task)) => success_with("task", task),
        Ok(None) => failure_response(404, "없는 태스크입니다"),
        Err(BridgeError::SessionDependencyMissing(message)) => failure_response(409, &message),
        Err(error) => task_write_failure(&error),
    })
}

/// JSON 본문이 `null`이면 TS는 `body.<칸>`을 읽다가 TypeError로 전역 500이 됐다
fn reject_null_body(body: &Value) -> Result<(), RouteError> {
    if body.is_null() {
        Err(RouteError::Unexpected)
    } else {
        Ok(())
    }
}
