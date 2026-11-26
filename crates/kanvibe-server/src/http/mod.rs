//! 연결 하나를 받아 HTTP 요청이나 WebSocket upgrade로 넘기는 계층.
//!
//! Node는 upgrade 요청에 헤더 없는 상태 줄(`HTTP/1.1 401 Unauthorized\r\n\r\n`)만 쓰고 소켓을 버리거나,
//! 아무것도 쓰지 않고 끊었다. hyper로는 그 바이트를 만들 수 없어 첫 요청 머리를 직접 읽고,
//! upgrade면 [`crate::ws`]가 소켓을 통째로 맡고 아니면 읽은 바이트를 되돌려 hyper에 넘긴다.

pub mod body;
mod connection;
pub mod rewind;

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::{Arc, RwLock};

use http::{HeaderMap, Method};

pub use body::{ConnectionAbort, HttpResponse, RequestBody};
pub use connection::{ConnectionTimeouts, IoStream, UpgradeHead, serve_connection};

/// upgrade 처리부가 머리 뒤에 이미 받은 바이트를 WebSocket 첫 프레임으로 다시 읽게 한다
pub fn rewind_stream(
    prefix: bytes::Bytes,
    stream: Box<dyn IoStream>,
) -> rewind::Rewind<Box<dyn IoStream>> {
    rewind::Rewind::new(prefix, stream)
}

/// 요청이 들어온 리스너
#[derive(Debug, Clone)]
pub enum ListenerKind {
    /// hook 포트. 모바일 경로 다음에 루프백에서 온 hook 경로를 싣는다
    Plain,
    /// hook 포트 + 10. 모바일 경로만 싣는다
    Tls,
    /// SSH 호스트별 터널 소켓. hook 경로만 싣고, 요청은 그 호스트들의 task만 건드릴 수 있다.
    /// 같은 원격을 가리키는 별칭이 나중에 붙을 수 있어 목록은 터널 서비스와 함께 쓴다
    Tunnel(Arc<RwLock<HashSet<String>>>),
}

/// 연결의 상대와 전송 방식
#[derive(Debug, Clone)]
pub struct PeerContext {
    pub listener: ListenerKind,
    /// TCP 상대 주소. Unix 소켓이면 없다
    pub remote_address: Option<IpAddr>,
}

impl PeerContext {
    /// TS `request.socket.encrypted === true`
    pub fn is_encrypted(&self) -> bool {
        matches!(self.listener, ListenerKind::Tls)
    }

    /// TS `LOOPBACK_ADDRESSES.has(socket.remoteAddress)`: `127.0.0.1`, `::1`, `::ffff:127.0.0.1`만 루프백이다
    pub fn is_loopback(&self) -> bool {
        self.remote_address.is_some_and(is_loopback_hook_address)
    }

    /// 요청이 들어온 순간의 터널 호스트 목록
    pub fn tunnel_hosts(&self) -> Option<HashSet<String>> {
        match &self.listener {
            ListenerKind::Tunnel(hosts) => Some(
                hosts
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone(),
            ),
            _ => None,
        }
    }
}

pub fn is_loopback_hook_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => address.octets() == [127, 0, 0, 1],
        IpAddr::V6(address) => {
            address.is_loopback()
                || address
                    .to_ipv4_mapped()
                    .is_some_and(|v4| v4.octets() == [127, 0, 0, 1])
        }
    }
}

/// 라우트가 보는 요청 하나
pub struct HttpRequest {
    pub method: Method,
    /// TS `request.url`: 요청 줄의 대상 문자열 그대로(쿼리 포함)
    pub raw_url: String,
    pub headers: HeaderMap,
    pub body: RequestBody,
    pub peer: PeerContext,
}

impl HttpRequest {
    /// 같은 이름 헤더가 여러 개면 Node처럼 첫 값을 쓴다
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    pub fn authorization(&self) -> Option<&str> {
        self.header("authorization")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_three_node_loopback_spellings_count_as_loopback() {
        assert!(is_loopback_hook_address("127.0.0.1".parse().unwrap()));
        assert!(is_loopback_hook_address("::1".parse().unwrap()));
        assert!(is_loopback_hook_address(
            "::ffff:127.0.0.1".parse().unwrap()
        ));
        assert!(!is_loopback_hook_address("127.0.0.2".parse().unwrap()));
        assert!(!is_loopback_hook_address("192.168.0.10".parse().unwrap()));
    }
}
