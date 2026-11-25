//! Codex app-server JSON-RPC 클라이언트. 이미 떠 있는 계정 서버의 Unix 소켓 위 WebSocket에 붙어 요청을 보낸다.
//!
//! 연결을 위해 서버나 대화를 새로 만들지 않고 구독도 하지 않는다. 기존 TUI가 응답과 승인을 받으며,
//! 연결을 닫아도 이미 수락된 turn은 취소되지 않는다. 원격 소켓은 [`UnixSocketForwarder`]가 로컬로 잇는다(SSH 터널은 `kanvibe-process`의 몫).

use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

use crate::error::{AiError, AiResult};
use crate::javascript::{js_string, parse_json, stringify};
use crate::node_path;
use crate::types::AiSessionReaderContext;

const CONNECTION_TIMEOUT: Duration = Duration::from_millis(4_000);
const REQUEST_TIMEOUT: Duration = Duration::from_millis(10_000);
const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;

const CONNECTION_CLOSED_MESSAGE: &str = "Codex 연결이 종료되었습니다.";
const RESPONSE_TIMEOUT_MESSAGE: &str =
    "Codex 응답을 확인하지 못했습니다. 요청을 다시 보내지 않습니다.";

/// 어느 소켓으로 붙을지. TS에서 `undefined`는 계정 기본 소켓, `null`은 이 연결 방식에서 쓸 수 없음을 뜻했다
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexAppServerSocket {
    /// `<codexHome>/app-server-control/app-server-control.sock`
    AccountDefault,
    /// 대화 지정 입력을 지원하지 않는 연결 방식
    Unsupported,
    Path(String),
}

/// 원격 호스트의 Unix 소켓을 로컬 임시 소켓으로 잇는다. 터미널·프로세스 crate가 SSH `-L` 터널로 구현한다
pub trait UnixSocketForwarder: Send + Sync {
    fn forward(
        &self,
        ssh_host: &str,
        remote_socket_path: &str,
    ) -> AiResult<Box<dyn ForwardedUnixSocket>>;
}

/// 열려 있는 터널. 값을 버리면 터널과 임시 디렉터리를 정리한다
pub trait ForwardedUnixSocket {
    fn local_path(&self) -> &str;
}

/// 연결할 대상 작업과 소켓
#[derive(Clone)]
pub struct CodexAppServerTarget<'a> {
    pub context: &'a AiSessionReaderContext,
    pub socket: CodexAppServerSocket,
    pub forwarder: Option<&'a dyn UnixSocketForwarder>,
}

/// 초기화를 마친 연결. `codex_home`은 서버가 알려 준 계정 홈이 우리가 기대한 홈과 같음을 확인한 값이다
pub struct CodexAppServerConnection {
    codex_home: String,
    socket: WebSocket<UnixStream>,
    sequence: u64,
}

impl CodexAppServerConnection {
    pub fn codex_home(&self) -> &str {
        &self.codex_home
    }

    /// 요청 하나를 보내고 같은 id의 응답을 기다린다. 응답이 늦으면 다시 보내지 않고 실패로 끝낸다
    pub fn request(
        &mut self,
        method: &str,
        params: Value,
        on_write: Option<&mut dyn FnMut()>,
    ) -> AiResult<Value> {
        self.sequence += 1;
        let id = self.sequence;
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        if let Some(on_write) = on_write {
            on_write();
        }
        let message = stringify(&json!({ "id": id, "method": method, "params": params }));
        self.socket
            .send(Message::text(message))
            .map_err(|_| closed_error())?;
        self.wait_for_response(id, deadline)
    }

    fn send_notification(&mut self, method: &str) -> AiResult<()> {
        let message = stringify(&json!({ "method": method }));
        self.socket
            .send(Message::text(message))
            .map_err(|_| closed_error())
    }

    fn wait_for_response(&mut self, id: u64, deadline: Instant) -> AiResult<Value> {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(AiError::message(RESPONSE_TIMEOUT_MESSAGE));
            }
            self.socket
                .get_ref()
                .set_read_timeout(Some(remaining))
                .map_err(|_| closed_error())?;
            let text = match self.socket.read() {
                Ok(Message::Text(text)) => text.to_string(),
                Ok(Message::Binary(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
                Ok(Message::Close(_)) => return Err(closed_error()),
                Ok(_) => continue,
                Err(tungstenite::Error::Io(error)) if is_timeout(&error) => {
                    return Err(AiError::message(RESPONSE_TIMEOUT_MESSAGE));
                }
                Err(_) => return Err(closed_error()),
            };
            if let Some(result) = read_response(&text, id)? {
                return result;
            }
        }
    }
}

fn is_timeout(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

fn closed_error() -> AiError {
    AiError::message(CONNECTION_CLOSED_MESSAGE)
}

/// 이 요청의 응답이면 결과를, 알림이나 다른 요청의 응답이면 `None`을 돌려준다
fn read_response(text: &str, id: u64) -> AiResult<Option<AiResult<Value>>> {
    let message =
        parse_json(text).ok_or_else(|| AiError::message("Codex 응답을 읽지 못했습니다."))?;
    let is_notification = message
        .get("method")
        .is_some_and(crate::javascript::is_truthy);
    let is_this_request = message.get("id").and_then(Value::as_f64) == Some(id as f64);
    if is_notification || !is_this_request {
        return Ok(None);
    }
    if let Some(error) = message
        .get("error")
        .filter(|error| crate::javascript::is_truthy(error))
    {
        let code = error
            .get("code")
            .map_or_else(|| "undefined".to_string(), js_string);
        return Ok(Some(Err(AiError::message(format!(
            "Codex 요청이 거절되었습니다 ({code})."
        )))));
    }
    Ok(Some(Ok(message
        .get("result")
        .cloned()
        .unwrap_or(Value::Null))))
}

fn connect(socket_path: &str, codex_home: &str) -> AiResult<CodexAppServerConnection> {
    let stream =
        UnixStream::connect(socket_path).map_err(|error| AiError::from_io(&error, socket_path))?;
    stream
        .set_read_timeout(Some(CONNECTION_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(CONNECTION_TIMEOUT)))
        .map_err(|error| AiError::from_io(&error, socket_path))?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_PAYLOAD_BYTES))
        .max_frame_size(Some(MAX_PAYLOAD_BYTES));
    let (socket, _) =
        tungstenite::client::client_with_config("ws://localhost/", stream, Some(config))
            .map_err(|error| AiError::message(format!("Codex에 연결되지 않았습니다. ({error})")))?;
    Ok(CodexAppServerConnection {
        codex_home: codex_home.to_string(),
        socket,
        sequence: 0,
    })
}

/// 연결을 열고 계정 홈을 확인한 뒤 `initialized`를 알리고 작업을 돌린다
fn with_socket<T>(
    socket_path: &str,
    codex_home: &str,
    operation: impl FnOnce(&mut CodexAppServerConnection) -> AiResult<T>,
) -> AiResult<T> {
    let mut connection = connect(socket_path, codex_home)?;
    let params = json!({
        "clientInfo": { "name": "kanvibe_pane_chat", "version": "1" },
        "capabilities": { "experimentalApi": true },
    });
    let initialized = connection.request("initialize", params, None)?;
    let server_home = initialized.get("codexHome").and_then(Value::as_str);
    if server_home.map(node_path::normalize).as_deref() != Some(codex_home) {
        return Err(AiError::message(
            "현재 CLI의 Codex 계정을 확인하지 못했습니다.",
        ));
    }
    connection.send_notification("initialized")?;
    operation(&mut connection)
}

fn with_host_socket<T>(
    target: &CodexAppServerTarget<'_>,
    socket_path: &str,
    codex_home: &str,
    operation: impl FnOnce(&mut CodexAppServerConnection) -> AiResult<T>,
) -> AiResult<T> {
    let Some(ssh_host) = target.context.ssh_host() else {
        let resolved = std::fs::canonicalize(socket_path)
            .map_err(|error| AiError::from_io(&error, socket_path))?;
        return with_socket(&resolved.to_string_lossy(), codex_home, operation);
    };
    let forwarder = target
        .forwarder
        .ok_or_else(|| AiError::message("CLI의 SSH 호스트를 찾지 못했습니다."))?;
    let tunnel = forwarder.forward(ssh_host, socket_path)?;
    with_socket(tunnel.local_path(), codex_home, operation)
}

/// 기존 계정의 서버에 연결해 전달된 작업을 수행한다. 연결을 위해 서버나 대화를 새로 만들지 않는다
pub fn with_codex_app_server<T>(
    target: &CodexAppServerTarget<'_>,
    operation: impl FnOnce(&mut CodexAppServerConnection) -> AiResult<T>,
) -> AiResult<T> {
    if target.socket == CodexAppServerSocket::Unsupported {
        return Err(AiError::message(
            "이 Codex 연결 방식에서는 대화 지정 입력을 지원하지 않습니다.",
        ));
    }
    let home_directory = target.context.host.home_directory()?;
    let codex_home = node_path::normalize(&node_path::join(&[&home_directory, ".codex"]));
    let socket_path = match &target.socket {
        CodexAppServerSocket::Unsupported | CodexAppServerSocket::AccountDefault => {
            node_path::join(&[&codex_home, "app-server-control", "app-server-control.sock"])
        }
        CodexAppServerSocket::Path(path) => path.clone(),
    };
    if !socket_path.starts_with('/') || socket_path.contains(':') {
        return Err(AiError::message("Codex 소켓 경로가 올바르지 않습니다."));
    }
    with_host_socket(target, &socket_path, &codex_home, operation)
}
