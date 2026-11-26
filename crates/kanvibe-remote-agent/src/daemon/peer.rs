//! rpc 소켓에 붙은 데스크탑 연결 하나(TS `attachPeer`).
//!
//! 첫 레코드는 10초 안에 온 `hello`여야 하고, namespace·role·version·build·client id가 모두 맞아야 `ready`를 받는다.
//! 같은 client id가 이미 붙어 있으면 앞 연결에 ping을 보내 2초 동안 답을 기다린다. 답하면 새 연결을 거절하고,
//! 답이 없으면 앞 연결을 닫고 새 연결을 받는다. 그 뒤 레코드는 id마다
//! 명령, 소켓 포워딩, PTY, hook 응답으로 나눈다. 잘못된 레코드를 받으면 `error`를 보내고 연결을 닫는다.

use std::collections::HashMap;
use std::io::Read;
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::ServerShared;
use super::exec::RunningCommand;
use super::forward::SocketForward;
use super::pty::PtyPeer;
use crate::outbox::RecordOutbox;
use crate::protocol::{
    HOST_CLIENT_ALREADY_CONNECTED, HOST_PROTOCOL_VERSION, HostRecord, HostRecordDecoder,
    decode_host_bytes, filter_hook_headers, is_valid_host_identifier, record_from, record_integer,
    record_text,
};

const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// 같은 client id의 앞 연결이 ping에 답하기를 기다리는 시간
const PEER_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// `stopped`를 쓰고 닫을 때 상대가 읽지 않으면 더 기다리지 않는다
const STOPPED_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
pub(super) const MAX_ACTIVE_REQUESTS: usize = 128;
pub(super) const MAX_HOOK_BYTES: usize = 512 * 1024;

/// 원격 hook 요청에 대한 데스크탑의 답
pub(super) enum HookReply {
    Response {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    Disconnected,
}

/// 데스크탑 연결 하나. hook 소켓은 client id로 이 값을 찾아 요청을 넘긴다
pub(super) struct Peer {
    client_id: String,
    pub(super) outbox: Arc<RecordOutbox>,
    pub(super) hooks: Mutex<HashMap<String, Sender<HookReply>>>,
    /// 응답 없는 연결로 판정해 교체할 때 쓰기가 막혀 있어도 바로 끊도록 소켓을 직접 닫는다
    socket: UnixStream,
    /// 진행 중인 ping. 같은 client id로 온 여러 새 연결이 같은 답을 함께 기다린다
    probe: Mutex<Option<Arc<PeerProbe>>>,
}

/// 앞 연결에 보낸 ping 하나와 그 답. 답이 오면 true, 2초가 지나거나 연결이 끊기면 false다
struct PeerProbe {
    id: String,
    has_responded: Mutex<Option<bool>>,
    settled: Condvar,
}

impl PeerProbe {
    fn finish(&self, has_responded: bool) {
        self.has_responded
            .lock()
            .unwrap()
            .get_or_insert(has_responded);
        self.settled.notify_all();
    }

    /// 답을 기다린다. 먼저 시간이 다 된 쪽이 false로 정해 함께 기다리던 쪽도 같은 답을 받는다
    fn wait_for_response(&self) -> bool {
        let has_responded = self.has_responded.lock().unwrap();
        let (mut has_responded, _) = self
            .settled
            .wait_timeout_while(has_responded, PEER_PROBE_TIMEOUT, |has_responded| {
                has_responded.is_none()
            })
            .unwrap();
        let has_responded = *has_responded.get_or_insert(false);
        self.settled.notify_all();
        has_responded
    }
}

impl Peer {
    /// ping을 보내 아직 답하는 연결인지 본다. 이미 묻는 중이면 그 답을 함께 기다린다
    fn probe(&self) -> bool {
        if self.outbox.is_closed() {
            return false;
        }
        let probe = {
            let mut current = self.probe.lock().unwrap();
            match current.as_ref() {
                Some(probe) => Arc::clone(probe),
                None => {
                    let probe = Arc::new(PeerProbe {
                        id: uuid::Uuid::new_v4().to_string(),
                        has_responded: Mutex::new(None),
                        settled: Condvar::new(),
                    });
                    *current = Some(Arc::clone(&probe));
                    self.send(record_from(json!({ "type": "ping", "id": probe.id })));
                    probe
                }
            }
        };
        let has_responded = probe.wait_for_response();
        let mut current = self.probe.lock().unwrap();
        if current
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &probe))
        {
            *current = None;
        }
        has_responded
    }

    fn answer_probe(&self, id: &str) {
        if let Some(probe) = self.probe.lock().unwrap().as_ref()
            && probe.id == id
        {
            probe.finish(true);
        }
    }

    /// 연결이 끝났다. 답을 기다리던 새 연결은 더 기다리지 않는다
    fn end_probe(&self) {
        if let Some(probe) = self.probe.lock().unwrap().take() {
            probe.finish(false);
        }
    }

    /// 응답 없는 연결을 끊는다(TS `socket.destroy()`)
    fn destroy(&self) {
        self.outbox.abort();
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}

/// 연결을 닫는 사유. `code`가 있으면 데스크탑이 사유를 구분하도록 `error` 레코드에 함께 싣는다
struct ConnectionRejection {
    message: String,
    code: Option<&'static str>,
}

impl From<String> for ConnectionRejection {
    fn from(message: String) -> Self {
        Self {
            message,
            code: None,
        }
    }
}

impl PtyPeer for Peer {
    fn client_id(&self) -> &str {
        &self.client_id
    }

    fn send(&self, record: HostRecord) -> bool {
        self.outbox.send(&record).is_ok()
    }
}

/// 연결이 연 명령과 소켓 포워딩. 둘을 합쳐 128개까지 연다
#[derive(Default)]
pub(super) struct PeerRequests {
    pub(super) commands: HashMap<String, Arc<RunningCommand>>,
    pub(super) forwards: HashMap<String, Arc<SocketForward>>,
}

impl PeerRequests {
    pub(super) fn has_room_for(&self, id: &str) -> bool {
        !self.commands.contains_key(id)
            && !self.forwards.contains_key(id)
            && self.commands.len() + self.forwards.len() < MAX_ACTIVE_REQUESTS
    }
}

struct Connection {
    shared: Arc<ServerShared>,
    outbox: Arc<RecordOutbox>,
    requests: Arc<Mutex<PeerRequests>>,
    peer: Option<Arc<Peer>>,
}

/// rpc 연결 하나를 끝날 때까지 처리한다
pub(super) fn serve_connection(shared: Arc<ServerShared>, stream: UnixStream) {
    let Ok(writer) = stream.try_clone() else {
        return;
    };
    let closer = stream.try_clone().ok();
    let outbox = RecordOutbox::start(
        Box::new(writer),
        Box::new(move || {
            if let Some(closer) = closer {
                let _ = closer.shutdown(Shutdown::Both);
            }
        }),
    );
    let mut connection = Connection {
        shared,
        outbox,
        requests: Arc::new(Mutex::new(PeerRequests::default())),
        peer: None,
    };
    connection.read_until_closed(stream);
    connection.clean_up();
}

impl Connection {
    fn read_until_closed(&mut self, mut stream: UnixStream) {
        let _ = stream.set_read_timeout(Some(HELLO_TIMEOUT));
        let mut decoder = HostRecordDecoder::new();
        let mut buffer = vec![0; 64 * 1024];
        loop {
            let count = match stream.read(&mut buffer) {
                Ok(0) => return,
                Ok(count) => count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return,
            };
            let outcome = decoder
                .push(&buffer[..count])
                .map_err(|error| ConnectionRejection::from(error.to_string()))
                .and_then(|records| {
                    records
                        .into_iter()
                        .try_for_each(|record| self.receive(&record, &stream))
                });
            if let Err(rejection) = outcome {
                let mut error = json!({ "type": "error", "error": rejection.message });
                if let Some(code) = rejection.code {
                    error["code"] = Value::from(code);
                }
                let _ = self.outbox.send(&record_from(error));
                self.outbox.close_after_flush();
                return;
            }
        }
    }

    fn receive(
        &mut self,
        record: &HostRecord,
        stream: &UnixStream,
    ) -> Result<(), ConnectionRejection> {
        let Some(peer) = self.peer.clone() else {
            return self.accept_hello(record, stream);
        };
        self.receive_request(peer, record)
            .map_err(ConnectionRejection::from)
    }

    fn receive_request(&mut self, peer: Arc<Peer>, record: &HostRecord) -> Result<(), String> {
        let id = record_text(record, "id")
            .filter(|id| is_valid_host_identifier(id))
            .ok_or("Invalid host request id")?;
        let kind = record_text(record, "type").unwrap_or_default();
        match kind {
            kind if kind.starts_with("pty.") => self.receive_pty(&peer, record, id),
            "exec.open" => super::exec::open_command(&self.outbox, &self.requests, record, id),
            "exec.stdin" => super::exec::write_stdin(&self.requests, record, id),
            "exec.cancel" => {
                super::exec::cancel(&self.requests, id);
                Ok(())
            }
            "socket.open" => {
                super::forward::open_forward(&self.outbox, &self.requests, record, id);
                Ok(())
            }
            "socket.data" | "socket.end" | "socket.close" => {
                super::forward::receive(&self.requests, record, kind, id)
            }
            "hook.response" => answer_hook(&peer, record, id),
            "pong" => {
                peer.answer_probe(id);
                Ok(())
            }
            "status" => {
                let mut status = self.shared.identity.clone();
                status.insert("type".into(), Value::from("status"));
                status.insert("id".into(), Value::from(id));
                peer.send(status);
                Ok(())
            }
            "stop" => {
                peer.send(record_from(json!({ "type": "stopped", "id": id })));
                // 주 스레드가 곧바로 close()로 큐를 버리므로, TS처럼 stopped가 소켓에 쓰인 뒤에 멈춘다
                self.outbox.wait_until_written(STOPPED_WRITE_TIMEOUT);
                self.shared.request_stop();
                Ok(())
            }
            _ => Err("Unknown host operation".into()),
        }
    }

    fn accept_hello(
        &mut self,
        record: &HostRecord,
        stream: &UnixStream,
    ) -> Result<(), ConnectionRejection> {
        let client_id = validated_hello_client_id(record, &self.shared)?;
        let previous = self.shared.peers.lock().unwrap().get(client_id).cloned();
        let has_previous_responded = previous.as_ref().is_some_and(|previous| previous.probe());
        if has_disconnected(stream) {
            return Ok(());
        }
        let peer = Arc::new(Peer {
            client_id: client_id.to_string(),
            outbox: Arc::clone(&self.outbox),
            hooks: Mutex::new(HashMap::new()),
            socket: stream.try_clone().map_err(|error| error.to_string())?,
            probe: Mutex::new(None),
        });
        let stale_peer = {
            let mut peers = self.shared.peers.lock().unwrap();
            let current = peers.get(client_id);
            // 같은 ping을 함께 기다린 다른 새 연결이 먼저 자리를 차지했을 수 있다
            let is_claimed = current.is_some_and(|current| {
                previous
                    .as_ref()
                    .is_none_or(|previous| !Arc::ptr_eq(current, previous))
                    || (has_previous_responded && !current.outbox.is_closed())
            });
            if is_claimed {
                return Err(ConnectionRejection {
                    message: "A desktop with this client ID is already connected".into(),
                    code: Some(HOST_CLIENT_ALREADY_CONNECTED),
                });
            }
            let stale_peer = current
                .filter(|current| !current.outbox.is_closed())
                .cloned();
            peers.insert(client_id.to_string(), Arc::clone(&peer));
            stale_peer
        };
        let mut ready = self.shared.identity.clone();
        if let Some(stale_peer) = stale_peer {
            stale_peer.destroy();
            ready.insert("replacedStalePeer".into(), Value::Bool(true));
        }
        let _ = stream.set_read_timeout(None);
        self.peer = Some(Arc::clone(&peer));
        peer.send(ready);
        Ok(())
    }

    fn receive_pty(&self, peer: &Arc<Peer>, record: &HostRecord, id: &str) -> Result<(), String> {
        let pty_peer: Arc<dyn PtyPeer> = Arc::clone(peer) as Arc<dyn PtyPeer>;
        if let Err(error) = self.shared.ptys.receive(&pty_peer, record) {
            let detach = record_from(json!({ "type": "pty.detach", "id": id }));
            let _ = self.shared.ptys.receive(&pty_peer, &detach);
            peer.send(record_from(
                json!({ "type": "pty.error", "id": id, "error": error }),
            ));
        }
        Ok(())
    }

    /// 연결이 끝났다. 연 명령과 포워딩을 닫고, 기다리던 hook 요청에는 다시 보내지 않는다고 답한다
    fn clean_up(&mut self) {
        let requests = std::mem::take(&mut *self.requests.lock().unwrap());
        for command in requests.commands.values() {
            command.kill();
        }
        for forward in requests.forwards.values() {
            forward.destroy();
        }
        if let Some(peer) = self.peer.take() {
            peer.end_probe();
            {
                let mut peers = self.shared.peers.lock().unwrap();
                if peers
                    .get(&peer.client_id)
                    .is_some_and(|current| Arc::ptr_eq(current, &peer))
                {
                    peers.remove(&peer.client_id);
                }
            }
            let pty_peer: Arc<dyn PtyPeer> = Arc::clone(&peer) as Arc<dyn PtyPeer>;
            self.shared.ptys.detach(&pty_peer);
            for (_, reply) in peer.hooks.lock().unwrap().drain() {
                let _ = reply.send(HookReply::Disconnected);
            }
        }
        self.outbox.close_after_flush();
    }
}

/// hello가 이 daemon과 맞는지 항목마다 따로 확인해 어느 값이 어긋났는지 알린다
fn validated_hello_client_id<'record>(
    record: &'record HostRecord,
    shared: &ServerShared,
) -> Result<&'record str, String> {
    if record_text(record, "type") != Some("hello") {
        return Err("Expected a single host hello record".into());
    }
    if record_integer(record, "version") != Some(HOST_PROTOCOL_VERSION as i64) {
        return Err("Host hello version mismatch".into());
    }
    if record_text(record, "namespace") != Some(shared.namespace.as_str()) {
        return Err("Host hello namespace mismatch".into());
    }
    if record_text(record, "role") != Some("desktop") {
        return Err("Host hello role mismatch".into());
    }
    if record_text(record, "buildHash") != Some(shared.build_hash.as_str()) {
        return Err("Host hello buildHash mismatch".into());
    }
    record_text(record, "clientId")
        .filter(|id| is_valid_host_identifier(id))
        .ok_or_else(|| "Invalid host hello clientId".to_string())
}

/// 앞 연결의 답을 기다리는 사이 새 연결이 끊겼는지 본다. 읽을 데이터가 남아 있거나 아직 없으면 열린 연결이다
fn has_disconnected(stream: &UnixStream) -> bool {
    let mut byte = 0u8;
    // SAFETY: 살아 있는 소켓 fd와 1바이트 버퍼만 넘기고, MSG_PEEK이라 데이터를 소비하지 않는다
    let received = unsafe {
        libc::recv(
            stream.as_raw_fd(),
            (&mut byte as *mut u8).cast(),
            1,
            libc::MSG_PEEK | libc::MSG_DONTWAIT,
        )
    };
    received == 0
        || (received < 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::WouldBlock)
}

fn answer_hook(peer: &Arc<Peer>, record: &HostRecord, id: &str) -> Result<(), String> {
    let Some(reply) = peer.hooks.lock().unwrap().remove(id) else {
        return Ok(());
    };
    let status = record_integer(record, "status")
        .filter(|status| (100..=599).contains(status))
        .ok_or("Invalid hook status")?;
    let body =
        decode_host_bytes(record.get("body"), MAX_HOOK_BYTES).map_err(|error| error.to_string())?;
    let Some(Value::Object(headers)) = record.get("headers") else {
        return Err("Invalid hook headers".into());
    };
    let pairs = headers
        .iter()
        .filter_map(|(key, value)| value.as_str().map(|value| (key.as_str(), value)));
    let headers = filter_hook_headers(pairs)
        .into_iter()
        .filter_map(|(key, value)| value.as_str().map(|value| (key, value.to_string())))
        .collect();
    let _ = reply.send(HookReply::Response {
        status: status as u16,
        headers,
        body,
    });
    Ok(())
}
