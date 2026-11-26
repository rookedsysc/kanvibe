//! Mermaid 이미지 경로(TS `electron/mobileDiagramRoutes.js`)

use http::Method;

use super::{MobileRequest, RouteResult, spread_success};
use crate::bridge::BridgeError;
use crate::http::body::{failure_response, json_response};

const DIAGRAM_RENDER_PATH: &str = "/api/mobile/diagrams/render";
/// JSON 이스케이프로 64 KiB 정의가 여섯 배까지 늘 수 있어 본문 한도를 따로 둔다
const MAX_DIAGRAM_BODY_BYTES: usize = 400 * 1024;
const DIAGRAM_REQUEST_ERROR: &str = "잘못된 다이어그램 요청입니다";

pub async fn handle_diagram_request(mobile: &mut MobileRequest<'_>) -> RouteResult {
    if !mobile.is(Method::POST, DIAGRAM_RENDER_PATH) {
        return Ok(None);
    }
    let Some(body) = mobile.read_json(MAX_DIAGRAM_BODY_BYTES).await? else {
        return Ok(Some(failure_response(400, DIAGRAM_REQUEST_ERROR)));
    };
    let outcome = mobile
        .bridge(move |bridge| bridge.render_mobile_diagram(&body))
        .await;
    Ok(Some(match outcome {
        Ok(image) => json_response(200, &spread_success(serde_json::Value::Object(image))),
        Err(BridgeError::Diagram { status, message })
            if matches!(status, 400 | 422 | 429 | 500 | 503) =>
        {
            failure_response(status, &message)
        }
        Err(_) => failure_response(500, "다이어그램 이미지를 만들지 못했습니다"),
    }))
}
