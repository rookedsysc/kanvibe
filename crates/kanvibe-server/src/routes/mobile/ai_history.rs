//! AI 대화 기록·첨부·작업 파일 경로(TS `handleAiHistoryRequest`, `handleTaskFileRequest`, 첨부 경로)

use http::Method;

use super::{MobileRequest, RouteError, RouteResult, success_with};
use crate::bridge::{AiHistoryQuery, BridgeError};
use crate::http::body::{HttpResponse, failure_response, json_response};
use crate::routes::js_semantics::{
    decode_all, decode_uri_component, is_ascii_digits, safe_integer,
};

const AI_MESSAGE_ROLES: [&str; 7] = [
    "user",
    "assistant",
    "tool",
    "system",
    "developer",
    "reasoning",
    "unknown",
];
const AI_HISTORY_PAGE_LIMIT: u64 = 100;

pub async fn handle_ai_history_routes(mobile: &mut MobileRequest<'_>) -> RouteResult {
    if !mobile.method_is(&Method::GET) {
        return Ok(None);
    }
    if let Some(ids) = mobile
        .url
        .capture("/api/mobile/tasks/*/ai-sessions/*/*/attachments/*")
    {
        return handle_attachment(mobile, decode_all(&ids)?).await.map(Some);
    }
    if let Some(ids) = mobile.url.capture("/api/mobile/tasks/*/workspace-file") {
        let task_id = decode_uri_component(&ids[0])?;
        return handle_task_file(mobile, task_id).await.map(Some);
    }
    if let Some(ids) = mobile.url.capture("/api/mobile/tasks/*/ai-sessions") {
        return handle_history(mobile, ids).await.map(Some);
    }
    if let Some(ids) = mobile.url.capture("/api/mobile/tasks/*/ai-sessions/*/*") {
        return handle_history(mobile, ids).await.map(Some);
    }
    Ok(None)
}

async fn handle_attachment(
    mobile: &MobileRequest<'_>,
    ids: Vec<String>,
) -> Result<HttpResponse, RouteError> {
    let [task_id, provider, session_id, attachment_id]: [String; 4] =
        ids.try_into().map_err(|_| RouteError::Unexpected)?;
    let checked_provider = provider.clone();
    if !mobile
        .bridge(move |bridge| Ok(bridge.is_mobile_ai_session_provider(&checked_provider)))
        .await?
    {
        return Ok(failure_response(400, "지원하지 않는 CLI입니다"));
    }
    let source_ref = mobile.url.param("sourceRef").map(str::to_string);
    let attachment = mobile
        .bridge(move |bridge| {
            bridge.get_mobile_ai_session_attachment(
                &task_id,
                &provider,
                &session_id,
                &attachment_id,
                source_ref.as_deref(),
            )
        })
        .await?;
    Ok(match attachment {
        Some(attachment) => success_with("attachment", attachment),
        None => failure_response(404, "첨부 파일을 찾을 수 없습니다"),
    })
}

/// 파일을 열지 못한 사유별 응답. 화면이 사유에 맞는 안내를 고르도록 `reason`도 싣는다
fn task_file_failure(reason: &str) -> Option<(u16, &'static str)> {
    match reason {
        "outside-worktree" => Some((400, "작업 폴더 밖의 파일은 열 수 없습니다")),
        "not-found" => Some((404, "파일을 찾을 수 없습니다")),
        "too-large" => Some((413, "파일이 너무 커서 열 수 없습니다")),
        "unreadable" => Some((500, "파일을 읽을 수 없습니다")),
        _ => None,
    }
}

async fn handle_task_file(
    mobile: &MobileRequest<'_>,
    task_id: String,
) -> Result<HttpResponse, RouteError> {
    let Some(file_path) = mobile
        .url
        .param("path")
        .filter(|path| !path.is_empty())
        .map(str::to_string)
    else {
        return Ok(failure_response(400, "파일 경로가 필요합니다"));
    };
    match mobile
        .bridge(move |bridge| bridge.get_mobile_task_file(&task_id, &file_path))
        .await
    {
        Ok(file) => Ok(success_with("file", file)),
        Err(BridgeError::TaskWorkspaceFile(reason)) => {
            let (status, error) = task_file_failure(&reason).ok_or(RouteError::Unexpected)?;
            Ok(json_response(
                status,
                &serde_json::json!({ "success": false, "error": error, "reason": reason }),
            ))
        }
        Err(error) => Err(error.into()),
    }
}

fn read_history_query(mobile: &MobileRequest<'_>) -> Option<AiHistoryQuery> {
    let url = &mobile.url;
    let limit_text = url.param("limit").unwrap_or("20");
    let limit = is_ascii_digits(limit_text)
        .then(|| limit_text.parse::<f64>().ok())
        .flatten()
        .filter(|limit| (1.0..=AI_HISTORY_PAGE_LIMIT as f64).contains(limit))?
        as u64;
    let cursor = url.param("cursor").map(str::to_string);
    if cursor
        .as_deref()
        .is_some_and(|cursor| safe_integer(cursor).is_none())
    {
        return None;
    }
    let roles: Option<Vec<String>> = url.has_param("roles").then(|| {
        url.param("roles")
            .unwrap_or("")
            .split(',')
            .map(str::to_string)
            .collect()
    });
    if roles.as_ref().is_some_and(|roles| {
        roles
            .iter()
            .any(|role| !AI_MESSAGE_ROLES.contains(&role.as_str()))
    }) {
        return None;
    }
    let query = url
        .param("query")
        .map(kanvibe_ai::javascript::js_trim)
        .filter(|query| !query.is_empty());
    Some(AiHistoryQuery {
        query: query.map(str::to_string),
        cursor,
        limit,
        roles,
    })
}

async fn handle_history(
    mobile: &MobileRequest<'_>,
    raw_ids: Vec<String>,
) -> Result<HttpResponse, RouteError> {
    let Ok(mut ids) = decode_all(&raw_ids) else {
        return Ok(failure_response(400, "잘못된 식별자입니다"));
    };
    let task_id = ids.remove(0);
    let session = (ids.len() == 2).then(|| (ids.remove(0), ids.remove(0)));
    let provider_check = session.as_ref().map(|(provider, _)| provider.clone());
    let is_provider_valid = match provider_check {
        Some(provider) => {
            mobile
                .bridge(move |bridge| Ok(bridge.is_mobile_ai_session_provider(&provider)))
                .await?
        }
        None => true,
    };
    let query = read_history_query(mobile)
        .filter(|_| is_provider_valid && !mobile.url.has_param("sourceRef"));
    let Some(query) = query else {
        return Ok(failure_response(400, "조회 값이 올바르지 않습니다"));
    };
    let checked_task_id = task_id.clone();
    if mobile
        .bridge(move |bridge| bridge.get_mobile_task(&checked_task_id))
        .await?
        .is_none()
    {
        return Ok(failure_response(404, "없는 태스크입니다"));
    }
    let Some((provider, session_id)) = session else {
        let history = mobile
            .bridge(move |bridge| bridge.get_mobile_ai_sessions(&task_id, &query))
            .await?;
        return Ok(success_with("history", history));
    };
    let detail = mobile
        .bridge(move |bridge| {
            bridge.get_mobile_ai_session_detail(&task_id, &provider, &session_id, &query)
        })
        .await?;
    Ok(match detail {
        Some(detail) => success_with("detail", detail),
        None => failure_response(404, "없는 대화입니다"),
    })
}
