//! 연결 하나의 수명. 첫 요청 머리를 읽어 upgrade인지 가른 뒤 WebSocket 처리부나 hyper로 넘긴다.

use std::sync::Arc;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};

use super::rewind::Rewind;
use super::{HttpRequest, ListenerKind, PeerContext, RequestBody};
use crate::server::ServerState;

/// 요청 머리의 상한. 넘으면 Node처럼 연결을 끊는다
const MAX_HEAD_BYTES: usize = 64 * 1024;
const MAX_HEADERS: usize = 100;

/// 아무것도 보내지 않는 연결이 task와 fd를 붙잡지 않게 닫는 시간 제한.
/// TS 서버가 기대던 Node `http.Server` 기본값(`headersTimeout` 60초)에 맞춘다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionTimeouts {
    /// TLS 핸드셰이크를 마칠 때까지
    pub tls_handshake: Duration,
    /// 요청 머리를 다 받을 때까지. keep-alive 연결이 다음 요청 머리를 기다리는 시간도 같은 제한을 받는다
    pub request_head: Duration,
}

impl Default for ConnectionTimeouts {
    fn default() -> Self {
        Self {
            tls_handshake: Duration::from_secs(10),
            request_head: Duration::from_secs(60),
        }
    }
}

pub trait IoStream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> IoStream for T {}

/// 처음 받은 요청 머리. upgrade 처리부가 경로·헤더를 다시 쓰고, 나머지 바이트는 WebSocket 첫 프레임일 수 있다
pub struct UpgradeHead {
    pub method: String,
    pub raw_url: String,
    pub headers: http::HeaderMap,
    /// 머리 뒤에 이미 받아 둔 바이트
    pub trailing: Bytes,
}

pub async fn serve_connection<S: IoStream>(
    mut stream: S,
    peer: PeerContext,
    state: Arc<ServerState>,
) {
    let timeouts = state.connection_timeouts;
    let Ok(Some(buffered)) =
        tokio::time::timeout(timeouts.request_head, read_request_head(&mut stream)).await
    else {
        return;
    };
    let is_tunnel = matches!(peer.listener, ListenerKind::Tunnel(_));
    if !is_tunnel && let Some(head) = parse_upgrade_head(&buffered) {
        crate::ws::handle_upgrade(Box::new(stream), head, peer, state).await;
        return;
    }
    let io = TokioIo::new(Rewind::new(buffered.freeze(), stream));
    let service = service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
        let state = state.clone();
        let peer = peer.clone();
        async move {
            let (parts, incoming) = request.into_parts();
            let request = HttpRequest {
                raw_url: parts
                    .uri
                    .path_and_query()
                    .map(|target| target.as_str().to_string())
                    .unwrap_or_else(|| parts.uri.to_string()),
                method: parts.method,
                headers: parts.headers,
                body: RequestBody::new(incoming),
                peer,
            };
            crate::routes::route_http(request, &state).await
        }
    });
    let _ = http1::Builder::new()
        .keep_alive(true)
        .timer(TokioTimer::new())
        .header_read_timeout(timeouts.request_head)
        .serve_connection(io, service)
        .await;
}

/// 첫 요청의 머리 끝(`\r\n\r\n`)까지 읽는다. 상대가 아무것도 보내지 않고 닫았거나 머리가 너무 크면 `None`
async fn read_request_head<S: IoStream>(stream: &mut S) -> Option<BytesMut> {
    let mut buffered = BytesMut::with_capacity(4096);
    loop {
        if find_head_end(&buffered).is_some() {
            return Some(buffered);
        }
        if buffered.len() > MAX_HEAD_BYTES {
            return None;
        }
        let read = stream.read_buf(&mut buffered).await.ok()?;
        if read == 0 {
            return (!buffered.is_empty()).then_some(buffered);
        }
    }
}

fn find_head_end(buffered: &[u8]) -> Option<usize> {
    buffered
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
}

/// Node `http.Server`가 `upgrade` 이벤트를 내는 요청(`Connection: upgrade`와 `Upgrade` 헤더)이면 머리를 돌려준다
fn parse_upgrade_head(buffered: &[u8]) -> Option<UpgradeHead> {
    let head_end = find_head_end(buffered)?;
    let mut header_slots = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut request = httparse::Request::new(&mut header_slots);
    if !request.parse(&buffered[..head_end]).ok()?.is_complete() {
        return None;
    }
    let mut headers = http::HeaderMap::new();
    for header in request.headers.iter() {
        let name = http::HeaderName::from_bytes(header.name.as_bytes()).ok()?;
        let value = http::HeaderValue::from_bytes(header.value).ok()?;
        headers.append(name, value);
    }
    let has_upgrade_token = headers
        .get_all(http::header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|token| token.trim().eq_ignore_ascii_case("upgrade"));
    if !has_upgrade_token || !headers.contains_key(http::header::UPGRADE) {
        return None;
    }
    Some(UpgradeHead {
        method: request.method?.to_string(),
        raw_url: request.path?.to_string(),
        headers,
        trailing: Bytes::copy_from_slice(&buffered[head_end..]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_heads_need_both_connection_token_and_upgrade_header() {
        let upgrade = b"GET /api/mobile/events HTTP/1.1\r\nHost: x\r\nConnection: keep-alive, Upgrade\r\nUpgrade: websocket\r\n\r\nrest";
        let head = parse_upgrade_head(upgrade).expect("upgrade");
        assert_eq!(head.raw_url, "/api/mobile/events");
        assert_eq!(&head.trailing[..], b"rest");
        let plain = b"GET /api/mobile/events HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\n\r\n";
        assert!(parse_upgrade_head(plain).is_none());
    }
}
