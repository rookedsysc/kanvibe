//! pane chat 경로(TS `handlePaneChatRequest`). 메서드와 동작이 맞지 않는 조합은 404로 흘러간다.

use http::Method;
use serde_json::{Value, json};

use super::{
    BAD_REQUEST_ERROR, MobileRequest, RouteError, RouteResult, SERVER_ERROR, spread_success,
    success_with,
};
use crate::bridge::{BridgeError, PaneChatAction, PaneChatSuggestionQuery};
use crate::http::body::{HttpResponse, MAX_BODY_BYTES, failure_response, json_response};
use crate::routes::js_semantics::{decode_uri_component, match_path, optional_property};

/// 10 MiB 파일의 base64와 JSON 메타데이터를 받는다. 다른 요청의 64 KiB 한도는 그대로다
const MAX_PANE_CHAT_ATTACHMENT_BODY_BYTES: usize = 14 * 1024 * 1024;
const TOKEN_REQUIRED_ERROR: &str = "CLI 대상 토큰이 필요합니다";
const PANE_CHAT_ACTIONS: [&str; 7] = [
    "prompt",
    "interrupt",
    "model-picker",
    "model-setting",
    "key",
    "attachments",
    "suggestions",
];

/// `…/pane-chat`이면 `Some(None)`, `…/pane-chat/<동작>`이면 `Some(Some(동작))`
fn match_pane_chat_path(pathname: &str) -> Option<(String, Option<&'static str>)> {
    if let Some(ids) = match_path(pathname, "/api/mobile/tasks/*/pane-chat") {
        return Some((ids[0].clone(), None));
    }
    PANE_CHAT_ACTIONS.iter().find_map(|action| {
        let pattern = format!("/api/mobile/tasks/*/pane-chat/{action}");
        match_path(pathname, &pattern).map(|ids| (ids[0].clone(), Some(*action)))
    })
}

fn is_supported_combination(method: &Method, action: Option<&str>) -> bool {
    match action {
        None => method == Method::GET,
        Some("suggestions") => method == Method::GET,
        Some("attachments") => method == Method::POST || method == Method::DELETE,
        Some(_) => method == Method::POST,
    }
}

pub async fn handle_pane_chat_route(mobile: &mut MobileRequest<'_>) -> RouteResult {
    let Some((raw_task_id, action)) = match_pane_chat_path(&mobile.url.pathname) else {
        return Ok(None);
    };
    if !is_supported_combination(&mobile.request.method, action) {
        return Ok(None);
    }
    let task_id = decode_uri_component(&raw_task_id)?;
    let outcome = if mobile.method_is(&Method::GET) {
        read_pane_chat(mobile, task_id, action).await
    } else {
        run_pane_chat_action(mobile, task_id, action.unwrap_or_default()).await
    };
    Ok(Some(match outcome {
        Ok(response) => response,
        Err(RouteError::Abort) => return Err(RouteError::Abort),
        Err(RouteError::Bridge(error)) => pane_chat_failure_response(&error),
        Err(RouteError::Unexpected) => failure_response(500, SERVER_ERROR),
    }))
}

fn pane_chat_failure_response(error: &BridgeError) -> HttpResponse {
    match error {
        BridgeError::PaneChatInput(message) => failure_response(400, message),
        BridgeError::PaneChatTargetChanged(message) => json_response(
            409,
            &json!({ "success": false, "error": message, "reason": "target-changed" }),
        ),
        BridgeError::PaneChatDeliveryUncertain(message) => json_response(
            409,
            &json!({ "success": false, "error": message, "reason": "delivery-uncertain" }),
        ),
        _ => failure_response(500, SERVER_ERROR),
    }
}

async fn read_pane_chat(
    mobile: &MobileRequest<'_>,
    task_id: String,
    action: Option<&str>,
) -> Result<HttpResponse, RouteError> {
    let url = &mobile.url;
    if action == Some("suggestions") {
        let Some(target_token) = url.param("targetToken").filter(|token| !token.is_empty()) else {
            return Ok(failure_response(400, TOKEN_REQUIRED_ERROR));
        };
        let query = PaneChatSuggestionQuery {
            target_token: target_token.to_string(),
            kind: url.param("kind").map(str::to_string),
            query: url.param("query").unwrap_or("").to_string(),
            limit: url
                .has_param("limit")
                .then(|| js_number(url.param("limit").unwrap_or(""))),
        };
        let result = mobile
            .bridge(move |bridge| bridge.get_pane_chat_suggestions(&task_id, &query))
            .await?;
        return Ok(json_response(200, &spread_success(Value::Object(result))));
    }
    let pane_id = url.param("paneId").map(str::to_string);
    if url.param("detail") == Some("agent")
        && let Some(pane_id) = pane_id.clone().filter(|id| !id.is_empty())
    {
        let provider = mobile
            .bridge(move |bridge| bridge.get_pane_chat_agent_provider(&task_id, &pane_id))
            .await?;
        return Ok(success_with("provider", provider));
    }
    let should_include_screen = url.param("wantsScreen") != Some("false");
    let snapshot = mobile
        .bridge(move |bridge| {
            bridge.get_sequenced_pane_chat_snapshot(
                &task_id,
                pane_id.as_deref(),
                should_include_screen,
            )
        })
        .await?;
    Ok(success_with("snapshot", snapshot))
}

/// JS `Number(text)`: 앞뒤 공백을 지운 십진·16진 표기, 빈 문자열은 0, 그 밖은 NaN
fn js_number(text: &str) -> f64 {
    let trimmed = kanvibe_ai::javascript::js_trim(text);
    if trimmed.is_empty() {
        return 0.0;
    }
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        return u64::from_str_radix(hex, 16).map_or(f64::NAN, |value| value as f64);
    }
    let unsigned = trimmed.trim_start_matches(['+', '-']);
    if unsigned == "Infinity" {
        return if trimmed.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    // Rust는 `inf`·`NaN`도 숫자로 읽지만 JS는 지수 표기 말고는 글자를 받지 않는다
    let has_foreign_letter = trimmed
        .chars()
        .any(|character| character.is_alphabetic() && !matches!(character, 'e' | 'E'));
    if has_foreign_letter {
        return f64::NAN;
    }
    trimmed.parse::<f64>().unwrap_or(f64::NAN)
}

async fn run_pane_chat_action(
    mobile: &mut MobileRequest<'_>,
    task_id: String,
    action: &str,
) -> Result<HttpResponse, RouteError> {
    let is_attachment_upload = action == "attachments" && mobile.method_is(&Method::POST);
    let limit = if is_attachment_upload {
        MAX_PANE_CHAT_ATTACHMENT_BODY_BYTES
    } else {
        MAX_BODY_BYTES
    };
    let Some(body) = mobile.read_json(limit).await? else {
        return Ok(failure_response(400, BAD_REQUEST_ERROR));
    };
    let target_token = match optional_property(&body, "targetToken") {
        Some(Value::String(token)) if !token.is_empty() && body.is_object() => token.clone(),
        _ => return Ok(failure_response(400, TOKEN_REQUIRED_ERROR)),
    };
    let field = |name: &str| {
        optional_property(&body, name)
            .cloned()
            .unwrap_or(Value::Null)
    };
    let pane_chat_action = match action {
        "attachments" if is_attachment_upload => PaneChatAction::StageAttachment {
            attachment: field("attachment"),
        },
        "attachments" => PaneChatAction::RemoveAttachment { id: field("id") },
        "prompt" => PaneChatAction::Prompt {
            text: field("text"),
            attachment_ids: optional_property(&body, "attachmentIds").cloned(),
            request_id: optional_property(&body, "requestId").cloned(),
        },
        "interrupt" => PaneChatAction::Interrupt,
        "model-picker" => PaneChatAction::ModelPicker,
        "model-setting" => PaneChatAction::ModelSetting {
            change: field("change"),
        },
        _ => PaneChatAction::Key { key: field("key") },
    };
    let result = mobile
        .bridge(move |bridge| {
            bridge.run_pane_chat_action(&task_id, &target_token, pane_chat_action)
        })
        .await?;
    Ok(json_response(200, &result))
}
