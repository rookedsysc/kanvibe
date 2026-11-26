//! 모든 경로 테스트가 쓰는 서버 띄우기와 클라이언트.
//!
//! TS 테스트가 `http.createServer` + `attachMobileStreamServer`에 가짜 `bridge`를 넣던 것처럼, 실제 `start_server`를
//! 127.0.0.1:0(평문과 TLS)에 띄우고 가짜 서비스를 두 경계(`MobileBridge`, `HookBackend`)에 함께 넣는다.

pub mod fake_bridge;
pub mod fake_defaults;
pub mod fake_mobile_bridge;
pub mod http_client;
pub mod recorder;
pub mod tls_client;
pub mod ws_client;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use kanvibe_server::http::ConnectionTimeouts;
use kanvibe_server::server::{
    RouteOptions, RunningServer, ServerConfig, ServerState, start_server,
};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpSocket, TcpStream};

pub use fake_bridge::FakeBridge;
pub use fake_defaults::{ACCOUNT_ID, inbox_notification};
pub use http_client::{Exchange, HttpCall, HttpReply};
pub use recorder::{Gate, unexpected};
pub use ws_client::WsClient;

/// 페어링된 두 기기의 토큰(TS `DEVICE_IDS`)
pub const GOOD: &str = "Bearer good";
pub const SECOND: &str = "Bearer second";
pub const UNPAIRED_ERROR: &str = "연결되지 않은 기기입니다";

pub struct TestServer {
    pub fake: Arc<FakeBridge>,
    pub running: RunningServer,
    tls_fingerprint: String,
    _directory: tempfile::TempDir,
}

impl TestServer {
    pub async fn start() -> Self {
        Self::start_with(RouteOptions::default()).await
    }

    pub async fn start_with(options: RouteOptions) -> Self {
        Self::start_configured(options, ConnectionTimeouts::default()).await
    }

    pub async fn start_with_connection_timeouts(timeouts: ConnectionTimeouts) -> Self {
        Self::start_configured(RouteOptions::default(), timeouts).await
    }

    async fn start_configured(options: RouteOptions, timeouts: ConnectionTimeouts) -> Self {
        let directory = tempfile::tempdir().expect("임시 폴더");
        let tls_directory = directory.path().join("mobile-tls");
        let tls_fingerprint = tls_client::write_test_identity(&tls_directory);
        let fake = FakeBridge::new();
        let mut state = ServerState::new(fake.clone(), fake.clone(), options);
        state.connection_timeouts = timeouts;
        let running = start_server(&server_config(tls_directory), state)
            .await
            .expect("서버 시작");
        assert!(running.tls_address.is_some(), "TLS 리스너가 열려야 한다");
        Self {
            fake,
            running,
            tls_fingerprint,
            _directory: directory,
        }
    }

    pub fn plaintext_address(&self) -> SocketAddr {
        self.running.plaintext_address
    }

    pub async fn exchange(&self, call: HttpCall) -> Exchange {
        let stream = TcpStream::connect(self.plaintext_address())
            .await
            .expect("평문 포트에 연결");
        http_client::exchange(stream, &call).await
    }

    pub async fn request(&self, call: HttpCall) -> HttpReply {
        self.exchange(call).await.reply()
    }

    /// TS `request(path, { headers: { authorization } })`
    pub async fn get(&self, path: &str, authorization: Option<&str>) -> HttpReply {
        let call = HttpCall::get(path);
        self.request(authorization.map_or(call.clone(), |token| call.auth(token)))
            .await
    }

    pub async fn request_tls(&self, call: HttpCall) -> HttpReply {
        let address = self.running.tls_address.expect("TLS 주소");
        let stream = tls_client::connect_tls(address, &self.tls_fingerprint).await;
        http_client::exchange(stream, &call).await.reply()
    }

    /// 다른 출발 주소(예: 127.0.0.2)에서 보낸 요청. 서버는 127.0.0.1에만 묶여 있고, 리눅스는 127/8 전체를 lo로 받는다
    pub async fn request_from(&self, source: IpAddr, call: HttpCall) -> HttpReply {
        let socket = TcpSocket::new_v4().expect("소켓");
        socket.bind(SocketAddr::new(source, 0)).expect("출발 주소");
        let stream = socket
            .connect(self.plaintext_address())
            .await
            .expect("평문 포트에 연결");
        http_client::exchange(stream, &call).await.reply()
    }

    /// 요청 바이트를 그대로 쓰고 서버가 끊을 때까지 받은 바이트
    pub async fn raw(&self, request: &str) -> Vec<u8> {
        let mut stream = TcpStream::connect(self.plaintext_address())
            .await
            .expect("평문 포트에 연결");
        stream
            .write_all(request.as_bytes())
            .await
            .expect("요청 쓰기");
        http_client::read_until_closed(&mut stream).await
    }

    pub async fn websocket(
        &self,
        path: &str,
        authorization: Option<&str>,
    ) -> Result<WsClient, u16> {
        ws_client::connect_websocket(self.plaintext_address(), path, authorization).await
    }

    /// `/api/mobile/events`에 붙어 연결 확인(connected)까지 받은 소켓
    pub async fn events(&self, authorization: &str) -> WsClient {
        let mut client = self
            .websocket("/api/mobile/events", Some(authorization))
            .await
            .expect("이벤트 소켓");
        let connected = client.next_json().await;
        assert_eq!(connected["type"], "connected");
        client
    }

    /// TS `openPaneStreamClient`: task-1의 pane-1 스트림
    pub async fn pane_stream(&self, authorization: &str) -> WsClient {
        let path = "/api/mobile/stream?taskId=task-1&paneId=pane-1";
        self.websocket(path, Some(authorization))
            .await
            .expect("pane 스트림")
    }
}

fn server_config(tls_directory: std::path::PathBuf) -> ServerConfig {
    let loopback: SocketAddr = "127.0.0.1:0".parse().expect("주소");
    ServerConfig {
        plaintext_address: loopback,
        tls_address: loopback,
        tls_directory,
    }
}

/// 서비스 쪽에서 일어나는 일을 기다린다. 5초 안에 참이 되지 않으면 실패한다
pub async fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "조건이 시간 안에 만족되지 않았다"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// `{ success: false, error }`
pub fn failure(error: &str) -> Value {
    json!({ "success": false, "error": error })
}

/// 명령 한 건을 보내고 그 결과를 받는다(명령은 한 번에 하나씩 보낸다)
pub async fn run_command(client: &mut WsClient, fields: Value) -> Value {
    let mut message = json!({ "type": "command", "requestId": "r1" });
    merge(&mut message, fields);
    client.send_json(&message).await;
    client.next_json().await
}

/// 객체 필드를 덮어쓴다(TS `{ ...base, ...fields }`)
pub fn merge(base: &mut Value, fields: Value) {
    if let (Value::Object(base), Value::Object(fields)) = (base, fields) {
        base.extend(fields);
    }
}
