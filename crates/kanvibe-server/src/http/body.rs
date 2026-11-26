//! 요청 본문 읽기와 JSON 응답 쓰기(TS `electron/httpBody.js`).
//!
//! 본문 한도를 넘으면 Node는 `request.destroy()`로 소켓을 끊고 응답을 쓰지 않는다. 같은 결과를 내려고
//! 한도 초과는 응답 대신 [`ConnectionAbort`]로 올리고, 연결 처리부가 이것을 받으면 응답 없이 연결을 닫는다.

use bytes::{Bytes, BytesMut};
use http::{HeaderValue, Response, StatusCode, header};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use serde_json::Value;

/// 기본 본문 한도. `POST /api/mobile/pair`가 인증 앞단에 있어 끝없는 본문으로 힙을 채우지 못하게 한다
pub const MAX_BODY_BYTES: usize = 64 * 1024;

pub type HttpResponse = Response<Full<Bytes>>;

/// 응답 없이 연결을 끊어야 한다(본문 한도 초과, 본문 읽기 실패)
#[derive(Debug, thiserror::Error)]
#[error("요청 본문이 너무 큽니다")]
pub struct ConnectionAbort;

/// 본문은 처음 읽을 때 한 번만 받는다. 읽지 않은 본문은 hyper가 버린다
pub struct RequestBody {
    incoming: Option<Incoming>,
}

impl RequestBody {
    pub fn new(incoming: Incoming) -> Self {
        Self {
            incoming: Some(incoming),
        }
    }

    pub fn empty() -> Self {
        Self { incoming: None }
    }

    /// 한도까지 본문 바이트를 모은다. 넘으면 연결을 끊는다
    pub async fn read_bytes(&mut self, max_bytes: usize) -> Result<Bytes, ConnectionAbort> {
        let Some(mut incoming) = self.incoming.take() else {
            return Ok(Bytes::new());
        };
        let mut collected = BytesMut::new();
        while let Some(frame) = incoming.frame().await {
            let frame = frame.map_err(|_| ConnectionAbort)?;
            let Ok(data) = frame.into_data() else {
                continue;
            };
            if collected.len() + data.len() > max_bytes {
                return Err(ConnectionAbort);
            }
            collected.extend_from_slice(&data);
        }
        Ok(collected.freeze())
    }

    /// TS `readJsonBody`. 빈 본문은 `{}`이고, JSON이 아니면 안쪽 `Err`에 파서 메시지를 싣는다
    pub async fn read_json(
        &mut self,
        max_bytes: usize,
    ) -> Result<Result<Value, String>, ConnectionAbort> {
        let bytes = self.read_bytes(max_bytes).await?;
        Ok(parse_json_body(&bytes))
    }
}

/// Node가 청크를 UTF-8로 이어 붙인 뒤 `JSON.parse`하던 것과 같게 깨진 바이트는 대체 문자로 읽는다
pub fn parse_json_body(bytes: &[u8]) -> Result<Value, String> {
    if bytes.is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    let text = String::from_utf8_lossy(bytes);
    serde_json::from_str(&text).map_err(|error| format!("Unexpected JSON input: {error}"))
}

/// TS `writeJson`: `Content-Type: application/json`(charset 없음), 본문 `JSON.stringify(payload)`
pub fn json_response(status: u16, payload: &Value) -> HttpResponse {
    let body = serde_json::to_string(payload).unwrap_or_else(|_| "null".to_string());
    let mut response = Response::new(Full::new(Bytes::from(body)));
    *response.status_mut() =
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

/// `{success:false, error}`
pub fn failure_response(status: u16, error: &str) -> HttpResponse {
    json_response(
        status,
        &serde_json::json!({ "success": false, "error": error }),
    )
}

pub fn not_found_response() -> HttpResponse {
    failure_response(404, "Not found")
}

/// hook 설치 스크립트 응답
pub fn shell_script_response(script: String) -> HttpResponse {
    let mut response = Response::new(Full::new(Bytes::from(script)));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_body_reads_as_empty_object_and_invalid_json_keeps_the_parser_message() {
        assert_eq!(parse_json_body(b""), Ok(serde_json::json!({})));
        assert_eq!(parse_json_body(b"null"), Ok(Value::Null));
        assert!(
            parse_json_body(b"{")
                .unwrap_err()
                .starts_with("Unexpected JSON input")
        );
    }

    #[test]
    fn json_response_uses_charsetless_content_type() {
        let response = failure_response(403, "브라우저 요청은 허용하지 않습니다");
        assert_eq!(response.status(), 403);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    }
}
