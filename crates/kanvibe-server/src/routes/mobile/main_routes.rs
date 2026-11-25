//! 기본 모바일 경로(TS `handleMobileRequest`의 기능 경로 뒤 부분). 순서를 TS와 같게 둔다.

use http::Method;
use serde_json::{Value, json};

use super::{
    BAD_REQUEST_ERROR, MobileRequest, RouteError, RouteResult, UNPAIRED_DEVICE_ERROR, ai_history,
    pane_chat, success_with, tasks,
};
use crate::http::body::{HttpResponse, failure_response, json_response, not_found_response};
use crate::routes::js_semantics::{decode_uri_component, optional_property, property, read_text};

const NOTIFICATIONS_PATH: &str = "/api/mobile/notifications";

pub async fn handle_main_routes(
    mobile: &mut MobileRequest<'_>,
) -> Result<HttpResponse, RouteError> {
    if let Some(response) = handle_device_and_board(mobile).await? {
        return Ok(response);
    }
    if let Some(response) = handle_notification_routes(mobile).await? {
        return Ok(response);
    }
    if let Some(response) = pane_chat::handle_pane_chat_route(mobile).await? {
        return Ok(response);
    }
    if let Some(response) = ai_history::handle_ai_history_routes(mobile).await? {
        return Ok(response);
    }
    if let Some(response) = tasks::handle_task_routes(mobile).await? {
        return Ok(response);
    }
    Ok(not_found_response())
}

async fn handle_device_and_board(mobile: &mut MobileRequest<'_>) -> RouteResult {
    if mobile.is(Method::DELETE, "/api/mobile/device") {
        let authorization = mobile.request.authorization().map(str::to_string);
        let is_removed = mobile
            .bridge(move |bridge| bridge.unpair_mobile_device_by_token(authorization.as_deref()))
            .await?;
        return Ok(Some(registration_response(is_removed)));
    }
    if mobile.is(Method::GET, "/api/mobile/board") {
        let is_deduplicated = mobile.url.param("iconFormat") == Some("deduplicated");
        let board = mobile
            .bridge(move |bridge| bridge.get_mobile_board(is_deduplicated))
            .await?;
        return Ok(Some(success_with("board", board)));
    }
    if mobile.is(Method::GET, "/api/mobile/directories") {
        let path = mobile.url.param("path").unwrap_or("~").to_string();
        let ssh_host = mobile
            .url
            .has_param("sshHost")
            .then(|| mobile.url.param("sshHost").unwrap_or("").to_string());
        let directories = mobile
            .bridge(move |bridge| match ssh_host {
                Some(ssh_host) => bridge.get_mobile_remote_directories(&path, &ssh_host),
                None => bridge.get_mobile_directories(&path),
            })
            .await?;
        return Ok(Some(success_with("directories", directories)));
    }
    if mobile.is(Method::POST, "/api/mobile/projects/scan") {
        return handle_project_scan(mobile).await.map(Some);
    }
    Ok(None)
}

async fn handle_project_scan(mobile: &mut MobileRequest<'_>) -> Result<HttpResponse, RouteError> {
    let Some(body) = mobile.read_default_json().await? else {
        return Ok(failure_response(400, BAD_REQUEST_ERROR));
    };
    let Some(root_path) = read_text(optional_property(&body, "rootPath")) else {
        return Ok(failure_response(400, "폴더 경로가 필요합니다"));
    };
    let ssh_host = property(&body, "sshHost")
        .map_err(|_| RouteError::Unexpected)?
        .cloned();
    let result = mobile
        .bridge(move |bridge| match ssh_host {
            Some(ssh_host) => bridge.scan_mobile_remote_projects(&root_path, &ssh_host),
            None => bridge.scan_mobile_projects(&root_path),
        })
        .await?;
    Ok(success_with("result", result))
}

fn registration_response(is_registered: bool) -> HttpResponse {
    if is_registered {
        json_response(200, &json!({ "success": true }))
    } else {
        failure_response(401, UNPAIRED_DEVICE_ERROR)
    }
}

async fn handle_notification_routes(mobile: &mut MobileRequest<'_>) -> RouteResult {
    let is_subscription_method =
        mobile.method_is(&Method::PUT) || mobile.method_is(&Method::DELETE);
    if is_subscription_method && mobile.url.pathname == NOTIFICATIONS_PATH {
        return handle_notification_subscription(mobile).await.map(Some);
    }
    if mobile.is(Method::GET, NOTIFICATIONS_PATH) {
        let notifications = mobile
            .bridge(|bridge| bridge.get_mobile_notifications())
            .await?;
        return Ok(Some(success_with("notifications", notifications)));
    }
    if mobile.url.pathname == "/api/mobile/notification-settings"
        && let Some(response) = handle_notification_settings(mobile).await?
    {
        return Ok(Some(response));
    }
    if mobile.is(Method::POST, "/api/mobile/notifications/read-all") {
        mobile
            .bridge(|bridge| bridge.mark_all_mobile_notifications_read())
            .await?;
        return Ok(Some(json_response(200, &json!({ "success": true }))));
    }
    handle_notification_read_routes(mobile).await
}

async fn handle_notification_settings(mobile: &mut MobileRequest<'_>) -> RouteResult {
    if mobile.method_is(&Method::GET) {
        let settings = mobile
            .bridge(|bridge| bridge.get_mobile_notification_settings())
            .await?;
        return Ok(Some(success_with("settings", settings)));
    }
    if !mobile.method_is(&Method::PATCH) {
        return Ok(None);
    }
    let Some(body) = mobile.read_default_json().await? else {
        return Ok(Some(failure_response(400, BAD_REQUEST_ERROR)));
    };
    let settings = mobile
        .bridge(move |bridge| bridge.update_mobile_notification_settings(&body))
        .await?;
    Ok(Some(success_with("settings", settings)))
}

async fn handle_notification_read_routes(mobile: &mut MobileRequest<'_>) -> RouteResult {
    let is_patch = mobile.method_is(&Method::PATCH);
    if let Some(ids) = mobile
        .url
        .capture("/api/mobile/notifications/*/read")
        .filter(|_| is_patch)
    {
        let notification_id = decode_uri_component(&ids[0])?;
        let notification = mobile
            .bridge(move |bridge| bridge.mark_mobile_notification_read(&notification_id))
            .await?;
        return Ok(Some(match notification {
            Some(notification) => success_with("notification", notification),
            None => failure_response(404, "없는 알림입니다"),
        }));
    }
    let is_post = mobile.method_is(&Method::POST);
    if let Some(ids) = mobile
        .url
        .capture("/api/mobile/tasks/*/notifications/read")
        .filter(|_| is_post)
    {
        // 기능 경로가 같은 경로를 먼저 맡아 여기에는 닿지 않는다. TS 처리기 모양을 남겨 순서를 그대로 둔다
        let task_id = decode_uri_component(&ids[0])?;
        let count = mobile
            .bridge(move |bridge| {
                bridge.run_mobile_feature(crate::bridge::FeatureCall::MarkTaskNotificationsRead {
                    task_id,
                })
            })
            .await?;
        let count = count.get("updatedCount").cloned().unwrap_or(Value::Null);
        return Ok(Some(json_response(
            200,
            &json!({ "success": true, "count": count }),
        )));
    }
    Ok(None)
}

/// 푸시 토큰은 인증된 기기 한 곳에만 저장하고 네트워크에서 온 나머지 필드는 버린다
async fn handle_notification_subscription(
    mobile: &mut MobileRequest<'_>,
) -> Result<HttpResponse, RouteError> {
    let authorization = mobile.request.authorization().map(str::to_string);
    if mobile.method_is(&Method::DELETE) {
        let is_registered = mobile
            .bridge(move |bridge| {
                bridge.delete_mobile_notification_subscription(authorization.as_deref())
            })
            .await?;
        return Ok(registration_response(is_registered));
    }
    let Some(body) = mobile.read_default_json().await? else {
        return Ok(failure_response(400, BAD_REQUEST_ERROR));
    };
    let Some(subscription) = read_push_subscription(&body) else {
        return Ok(failure_response(400, "알림 구독 값이 올바르지 않습니다"));
    };
    let is_registered = mobile
        .bridge(move |bridge| {
            bridge.update_mobile_notification_subscription(authorization.as_deref(), subscription)
        })
        .await?;
    Ok(registration_response(is_registered))
}

/// `pushToken`은 공백 없는 1~4096자, `platform`은 android·ios, `connectionId`는 공백 없는 1~128자이고 128바이트 이하
fn read_push_subscription(body: &Value) -> Option<Value> {
    let push_token = optional_property(body, "pushToken")?.as_str()?;
    let platform = optional_property(body, "platform")?.as_str()?;
    let connection_id = optional_property(body, "connectionId")?.as_str()?;
    let is_valid = is_compact_text(push_token, 4096)
        && matches!(platform, "android" | "ios")
        && is_compact_text(connection_id, 128)
        && connection_id.len() <= 128;
    is_valid.then(
        || json!({ "pushToken": push_token, "platform": platform, "connectionId": connection_id }),
    )
}

/// `/^\S{1,max}$/` — 길이는 UTF-16 단위, `\S`는 JS 공백이 아닌 문자
fn is_compact_text(text: &str, max_length: usize) -> bool {
    let length = kanvibe_ai::javascript::utf16_length(text);
    (1..=max_length).contains(&length)
        && !text.chars().any(kanvibe_ai::javascript::is_js_whitespace)
}
