//! 리스너별 라우팅 순서(TS `hookServer.js`의 세 요청 처리기).
//!
//! 평문 포트: Origin 거절 → 모바일 경로 → 루프백이 아니면 404 → hook 경로.
//! TLS 포트: Origin 거절 → 모바일 경로 → 그 밖은 404. 터널 소켓: Origin 거절 → hook 경로(터널 호스트 범위).

pub mod hooks;
pub mod js_semantics;
pub mod mobile;

use crate::http::body::{ConnectionAbort, HttpResponse, failure_response, not_found_response};
use crate::http::{HttpRequest, ListenerKind};
use crate::server::ServerState;

pub async fn route_http(
    request: HttpRequest,
    state: &ServerState,
) -> Result<HttpResponse, ConnectionAbort> {
    if request.headers.contains_key(http::header::ORIGIN) {
        return Ok(failure_response(403, "브라우저 요청은 허용하지 않습니다"));
    }
    match request.peer.listener.clone() {
        ListenerKind::Plain => route_plaintext(request, state).await,
        ListenerKind::Tls => {
            let response = mobile::handle_mobile_request(request, state).await?;
            Ok(response.unwrap_or_else(not_found_response))
        }
        ListenerKind::Tunnel(_) => hooks::handle_hook_request(request, state).await,
    }
}

async fn route_plaintext(
    request: HttpRequest,
    state: &ServerState,
) -> Result<HttpResponse, ConnectionAbort> {
    let raw_url = request.raw_url.clone();
    let remote_address = request.peer.remote_address;
    let is_loopback = request.peer.is_loopback();
    let response = if raw_url.starts_with(mobile::MOBILE_PATH_PREFIX) {
        mobile::handle_mobile_request(request, state)
            .await?
            .unwrap_or_else(not_found_response)
    } else if !is_loopback {
        not_found_response()
    } else {
        hooks::handle_hook_request(request, state).await?
    };
    state.record_plaintext_response(remote_address, &raw_url, response.status().as_u16());
    Ok(response)
}
