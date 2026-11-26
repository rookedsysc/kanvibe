//! 원격 host daemon 연결 하나(TS `HostConnection`).
//!
//! SSH 자식(또는 테스트의 소켓)의 stdin/stdout 위에서 길이 접두 레코드를 주고받는다. 요청마다 id를 붙이고
//! 응답 레코드를 id로 받는 쪽에 넘긴다. 연결이 끊기면 기다리던 모든 쪽에 `disconnected`를 알리고, 결과를 알 수
//! 없는 명령은 다시 보내지 않는다. `hook.request`는 등록한 처리기로 답하고, daemon의 `ping`에는 같은 id의 `pong`으로 답한다.

mod bootstrap;
mod command;
mod process;
mod pty_attachment;
mod socket_forward;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kanvibe_remote_agent::outbox::{OutboxError, RecordOutbox};
use kanvibe_remote_agent::protocol::{
    HOST_PROTOCOL_VERSION, HostRecord, HostRecordDecoder, record_from, record_integer, record_text,
};
use serde_json::{Value, json};

use crate::hook_tunnel::{HostHookHandler, HostHookRequest, HostHookResponse};

pub use bootstrap::{HostTransportSpawner, connect_through_loader};
pub use command::HostExecuteOptions;
pub use process::{HostProcess, HostProcessInput, HostProcessOutput, HostProcessTermination};
pub use pty_attachment::RemotePtyAttachment;
pub use socket_forward::HostSocketForward;

/// 한 연결이 동시에 기다리는 요청 수 상한
const MAX_PENDING: usize = 128;
/// 원격 쓰기의 확인 응답을 기다리는 조각 수. 연속 입력은 최대 256 KiB까지 먼저 전송한다
const REMOTE_INPUT_WINDOW_CHUNKS: usize = 8;
/// 부트스트랩과 PTY 요청 응답을 기다리는 시간
pub(crate) const START_TIMEOUT: Duration = Duration::from_secs(15);

/// 원격 프로세스와 잇는 바이트 통로. `terminate`는 SSH 자식을 끝낸다(테스트는 소켓을 닫는다)
pub struct HostTransport {
    pub writer: Box<dyn Write + Send>,
    pub reader: Box<dyn Read + Send>,
    pub terminate: Box<dyn Fn() + Send + Sync>,
    /// SSH stderr의 마지막 부분. 끊김 오류 메시지에 붙인다
    pub stderr_tail: Arc<Mutex<String>>,
}

/// daemon 연결을 시작하지 못한 단계(TS `RemoteHostConnectionError.kind`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteHostConnectionErrorKind {
    /// 원격 loader가 첫 줄을 내기 전에 ssh가 끝났다
    Ssh,
    /// daemon과 hello·ready를 주고받지 못했거나 daemon이 연결을 거절했다
    Handshake,
    /// 바이너리 업로드나 bootstrap 순서처럼 원격 실행 환경을 준비하지 못했다
    Setup,
}

/// daemon 연결 시작 실패. 호출자가 SSH 문제와 daemon 거절을 구분해 알리도록 단계와 daemon의 사유 코드를 싣는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteHostConnectionError {
    pub kind: RemoteHostConnectionErrorKind,
    pub message: String,
    /// daemon이 `error` 레코드에 실은 `code`
    pub code: Option<String>,
}

impl RemoteHostConnectionError {
    pub fn new(kind: RemoteHostConnectionErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            code: None,
        }
    }

    pub fn setup(message: impl Into<String>) -> Self {
        Self::new(RemoteHostConnectionErrorKind::Setup, message)
    }

    fn handshake(message: impl Into<String>) -> Self {
        Self::new(RemoteHostConnectionErrorKind::Handshake, message)
    }
}

impl std::fmt::Display for RemoteHostConnectionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RemoteHostConnectionError {}

impl From<RemoteHostConnectionError> for String {
    fn from(error: RemoteHostConnectionError) -> Self {
        error.message
    }
}

/// `hello`에 싣는 데스크탑 신원
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostIdentity {
    pub namespace: String,
    pub client_id: String,
    pub build_hash: String,
}

type RecordReceiver = Arc<dyn Fn(&HostRecord) + Send + Sync>;

pub struct HostConnection {
    pub instance_id: String,
    pub remote_pid: i64,
    /// daemon이 응답 없는 같은 client id의 앞 연결을 닫고 이 연결을 받았다
    pub replaced_stale_peer: bool,
    outbox: Arc<RecordOutbox>,
    receivers: Mutex<HashMap<String, RecordReceiver>>,
    hook_handler: Mutex<Option<HostHookHandler>>,
    is_closed: AtomicBool,
    close_callbacks: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
    terminate: Box<dyn Fn() + Send + Sync>,
}

impl HostConnection {
    /// bridge가 붙은 통로에서 `hello`를 보내고 `ready`를 확인한 뒤 연결을 돌려준다
    pub fn attach(
        transport: HostTransport,
        identity: &HostIdentity,
    ) -> Result<Arc<Self>, RemoteHostConnectionError> {
        let HostTransport {
            writer,
            reader,
            terminate,
            stderr_tail,
        } = transport;
        let terminate: Arc<dyn Fn() + Send + Sync> = Arc::from(terminate);
        let outbox_terminate = Arc::clone(&terminate);
        let outbox = RecordOutbox::start(writer, Box::new(move || outbox_terminate()));
        let (first_sender, first_record) = channel();
        let (connection_sender, connection_receiver) = channel::<Option<Arc<HostConnection>>>();
        std::thread::spawn(move || {
            read_records(reader, first_sender, connection_receiver, stderr_tail)
        });
        let hello = json!({
            "type": "hello", "version": HOST_PROTOCOL_VERSION, "namespace": identity.namespace,
            "clientId": identity.client_id, "role": "desktop", "buildHash": identity.build_hash,
        });
        let handshake = outbox
            .send(&record_from(hello))
            .map_err(|error| RemoteHostConnectionError::handshake(error.to_string()))
            .and_then(|()| receive_ready(&first_record, identity));
        let ready = match handshake {
            Ok(ready) => ready,
            Err(error) => {
                let _ = connection_sender.send(None);
                outbox.abort();
                terminate();
                return Err(error);
            }
        };
        let connection = Arc::new(Self {
            instance_id: record_text(&ready, "instanceId")
                .unwrap_or_default()
                .to_string(),
            remote_pid: record_integer(&ready, "pid").unwrap_or_default(),
            replaced_stale_peer: ready.get("replacedStalePeer") == Some(&Value::Bool(true)),
            outbox,
            receivers: Mutex::new(HashMap::new()),
            hook_handler: Mutex::new(None),
            is_closed: AtomicBool::new(false),
            close_callbacks: Mutex::new(Vec::new()),
            terminate: Box::new(move || terminate()),
        });
        let _ = connection_sender.send(Some(Arc::clone(&connection)));
        Ok(connection)
    }

    pub fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::SeqCst)
    }

    /// 레코드를 보낸다. 닫혔거나 쓰기 큐가 8 MiB를 넘으면 실패한다
    pub fn send(&self, record: Value) -> Result<(), String> {
        if self.is_closed() {
            return Err(OutboxError::Closed.to_string());
        }
        self.outbox
            .send(&record_from(record))
            .map_err(|error| error.to_string())
    }

    /// id로 오는 레코드를 받을 쪽을 등록한다. 기다리는 요청이 128개를 넘으면 거절한다
    pub(crate) fn subscribe(&self, id: &str, receiver: RecordReceiver) -> Result<(), String> {
        let mut receivers = self.receivers.lock().unwrap();
        if receivers.len() >= MAX_PENDING || self.is_closed() {
            return Err("Host request capacity exceeded".into());
        }
        receivers.insert(id.to_string(), receiver);
        Ok(())
    }

    pub(crate) fn unsubscribe(&self, id: &str) {
        self.receivers.lock().unwrap().remove(id);
    }

    fn receive(self: &Arc<Self>, record: HostRecord) {
        if record_text(&record, "type") == Some("ping")
            && let Some(id) = record_text(&record, "id")
        {
            if self.send(json!({ "type": "pong", "id": id })).is_err() {
                self.close();
            }
            return;
        }
        if record_text(&record, "type") == Some("hook.request") {
            let connection = Arc::clone(self);
            std::thread::spawn(move || connection.answer_hook(&record));
            return;
        }
        let receiver = record_text(&record, "id")
            .and_then(|id| self.receivers.lock().unwrap().get(id).cloned());
        if let Some(receiver) = receiver {
            receiver(&record);
        }
    }

    pub fn set_hook_handler(&self, handler: HostHookHandler) {
        *self.hook_handler.lock().unwrap() = Some(handler);
    }

    fn answer_hook(&self, record: &HostRecord) {
        let id = record_text(record, "id").unwrap_or_default();
        let handler = self.hook_handler.lock().unwrap().clone();
        let response = match (handler, parse_hook_request(record)) {
            (Some(handler), Some(request)) => handler(request).ok(),
            _ => None,
        };
        let response = response.unwrap_or(HostHookResponse {
            status: 503,
            headers: Vec::new(),
            body: Vec::new(),
        });
        let headers: serde_json::Map<String, Value> = response
            .headers
            .into_iter()
            .map(|(key, value)| (key, Value::from(value)))
            .collect();
        let reply = json!({
            "type": "hook.response", "id": id, "status": response.status, "headers": headers,
            "body": kanvibe_remote_agent::protocol::encode_host_bytes(&response.body),
        });
        if self.send(reply).is_err() && !self.is_closed() {
            self.close();
        }
    }

    /// 닫힐 때 한 번 부를 일을 등록한다. 이미 닫혔으면 바로 부른다
    pub fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
        let mut callbacks = self.close_callbacks.lock().unwrap();
        if self.is_closed() {
            drop(callbacks);
            callback();
            return;
        }
        callbacks.push(callback);
    }

    /// 연결을 닫는다. 기다리던 요청은 결과를 모르는 채 실패하고 다시 보내지 않는다
    pub fn close(&self) {
        if self.is_closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let receivers: Vec<(String, RecordReceiver)> =
            self.receivers.lock().unwrap().drain().collect();
        for (id, receiver) in receivers {
            receiver(&record_from(json!({ "type": "disconnected", "id": id })));
        }
        self.outbox.close_after_flush();
        (self.terminate)();
        let callbacks = std::mem::take(&mut *self.close_callbacks.lock().unwrap());
        for callback in callbacks {
            callback();
        }
    }
}

fn parse_hook_request(record: &HostRecord) -> Option<HostHookRequest> {
    let headers = match record.get("headers") {
        Some(Value::Object(headers)) => headers
            .iter()
            .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string())))
            .collect(),
        _ => Vec::new(),
    };
    let body = kanvibe_remote_agent::protocol::decode_host_bytes(
        record.get("body"),
        kanvibe_remote_agent::protocol::HOST_RECORD_BYTES / 2,
    )
    .ok()?;
    Some(HostHookRequest {
        method: record_text(record, "method")?.to_string(),
        path: record_text(record, "path")?.to_string(),
        headers,
        body,
    })
}

fn receive_ready(
    first_record: &std::sync::mpsc::Receiver<Result<HostRecord, String>>,
    identity: &HostIdentity,
) -> Result<HostRecord, RemoteHostConnectionError> {
    let ready = match first_record.recv_timeout(START_TIMEOUT) {
        Ok(Ok(record)) => record,
        Ok(Err(error)) => return Err(RemoteHostConnectionError::handshake(error)),
        Err(RecvTimeoutError::Timeout) => {
            return Err(RemoteHostConnectionError::handshake(
                "Host handshake timed out",
            ));
        }
        Err(RecvTimeoutError::Disconnected) => {
            return Err(RemoteHostConnectionError::handshake(
                "SSH host disconnected during startup",
            ));
        }
    };
    if record_text(&ready, "type") == Some("error") {
        let message = record_text(&ready, "error").unwrap_or("Host daemon rejected the handshake");
        return Err(RemoteHostConnectionError {
            code: record_text(&ready, "code").map(str::to_string),
            ..RemoteHostConnectionError::handshake(message)
        });
    }
    validate_ready(&ready, identity).map_err(RemoteHostConnectionError::handshake)?;
    Ok(ready)
}

/// `ready`가 이 데스크탑과 맞는지 항목마다 따로 확인해 어느 값이 어긋났는지 알린다
fn validate_ready(ready: &HostRecord, identity: &HostIdentity) -> Result<(), String> {
    let kind = record_text(ready, "type").unwrap_or_default();
    if kind != "ready" {
        return Err(format!("Expected host ready record, received {kind}"));
    }
    if record_integer(ready, "version") != Some(HOST_PROTOCOL_VERSION as i64) {
        return Err("Host ready version mismatch".into());
    }
    if record_text(ready, "namespace") != Some(identity.namespace.as_str()) {
        return Err("Host ready namespace mismatch".into());
    }
    if record_text(ready, "buildHash") != Some(identity.build_hash.as_str()) {
        return Err("Host ready buildHash mismatch".into());
    }
    if record_text(ready, "instanceId").is_none() {
        return Err("Invalid host ready instanceId".into());
    }
    if record_integer(ready, "pid").is_none() {
        return Err("Invalid host ready pid".into());
    }
    let Some(capabilities) = ready.get("capabilities").and_then(Value::as_array) else {
        return Err("Invalid host ready capabilities".into());
    };
    let missing_capabilities: Vec<&str> = ["exec", "socket", "hooks", "pty"]
        .into_iter()
        .filter(|capability| !capabilities.contains(&Value::from(*capability)))
        .collect();
    if !missing_capabilities.is_empty() {
        return Err(format!(
            "Host ready capabilities missing: {}",
            missing_capabilities.join(", ")
        ));
    }
    Ok(())
}

/// 레코드를 읽는다. 첫 레코드는 핸드셰이크 쪽으로 보내고, 연결이 만들어지면 그 뒤 레코드를 연결에 넘긴다
fn read_records(
    mut reader: Box<dyn Read + Send>,
    first_sender: std::sync::mpsc::Sender<Result<HostRecord, String>>,
    connection_receiver: std::sync::mpsc::Receiver<Option<Arc<HostConnection>>>,
    stderr_tail: Arc<Mutex<String>>,
) {
    let mut decoder = HostRecordDecoder::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut connection: Option<Arc<HostConnection>> = None;
    let mut first_sender = Some(first_sender);
    loop {
        let records = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => decoder.push(&buffer[..count]),
        };
        let Ok(records) = records else {
            break;
        };
        for record in records {
            if let Some(sender) = first_sender.take() {
                let _ = sender.send(Ok(record));
                match connection_receiver.recv() {
                    Ok(Some(attached)) => connection = Some(attached),
                    _ => return,
                }
                continue;
            }
            if let Some(connection) = &connection {
                connection.receive(record);
            }
        }
    }
    if let Some(sender) = first_sender {
        let tail = stderr_tail.lock().unwrap().trim().to_string();
        let suffix = if tail.is_empty() {
            String::new()
        } else {
            format!(": {tail}")
        };
        let _ = sender.send(Err(format!("SSH host disconnected during startup{suffix}")));
    }
    if let Some(connection) = connection {
        connection.close();
    }
}
