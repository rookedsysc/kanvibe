//! 아무것도 보내지 않는 연결은 시간 제한 뒤 닫힌다(TS가 기대던 Node `headersTimeout`·`keepAliveTimeout`).
//! 시간 제한은 짧게 주입하고, 서버가 닫지 않으면 `read_until_closed`의 5초 대기가 실패한다.

use std::time::Duration;

use kanvibe_server::http::ConnectionTimeouts;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::harness::http_client::read_until_closed;
use crate::harness::*;

const SHORT_TIMEOUTS: ConnectionTimeouts = ConnectionTimeouts {
    tls_handshake: Duration::from_millis(200),
    request_head: Duration::from_millis(200),
};

#[tokio::test]
async fn a_plaintext_connection_that_never_sends_a_request_head_is_closed() {
    let server = TestServer::start_with_connection_timeouts(SHORT_TIMEOUTS).await;
    let mut stream = TcpStream::connect(server.plaintext_address())
        .await
        .expect("평문 포트에 연결");
    stream
        .write_all(b"GET /api/hooks/health HTTP/1.1\r\n")
        .await
        .expect("머리 일부");
    assert!(read_until_closed(&mut stream).await.is_empty());
}

#[tokio::test]
async fn a_tls_connection_that_never_finishes_the_handshake_is_closed() {
    let server = TestServer::start_with_connection_timeouts(SHORT_TIMEOUTS).await;
    let address = server.running.tls_address.expect("TLS 주소");
    let mut stream = TcpStream::connect(address).await.expect("TLS 포트에 연결");
    assert!(read_until_closed(&mut stream).await.is_empty());
}

#[tokio::test]
async fn an_idle_keep_alive_connection_is_closed_after_its_response() {
    let server = TestServer::start_with_connection_timeouts(SHORT_TIMEOUTS).await;
    let mut stream = TcpStream::connect(server.plaintext_address())
        .await
        .expect("평문 포트에 연결");
    stream
        .write_all(b"GET /api/hooks/health HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .expect("요청 쓰기");
    let received = read_until_closed(&mut stream).await;
    assert!(String::from_utf8_lossy(&received).starts_with("HTTP/1.1 200"));
}
