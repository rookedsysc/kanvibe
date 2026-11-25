//! rpc 소켓에 붙은 데스크탑 연결 하나(TS `attachPeer`).
//!
//! 첫 레코드는 10초 안에 온 `hello`여야 하고, namespace·role·version·build·client id가 모두 맞아야 `ready`를 받는다.
//! 같은 client id가 이미 붙어 있으면 앞 연결을 바꾸지 않고 새 연결을 거절한다. 그 뒤 레코드는 id마다
//! 명령, 소켓 포워딩, PTY, hook 응답으로 나눈다. 잘못된 레코드를 받으면 `error`를 보내고 연결을 닫는다.

use std::collections::HashMap;
use std::io::Read;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::ServerShared;
use super::exec::RunningCommand;
use super::forward::SocketForward;
use super::pty::PtyPeer;
use crate::outbox::RecordOutbox;
use crate::protocol::{
    HOST_PROTOCOL_VERSION, HostRecord, HostRecordDecoder, decode_host_bytes, filter_hook_headers,
    is_valid_host_identifier, record_from, record_integer, record_text,
};

const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
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
                .map_err(|error| error.to_string())
                .and_then(|records| {
                    records
                        .into_iter()
                        .try_for_each(|record| self.receive(&record, &stream))
                });
            if let Err(error) = outcome {
                let _ = self
                    .outbox
                    .send(&record_from(json!({ "type": "error", "error": error })));
                self.outbox.close_after_flush();
                return;
            }
        }
    }

    fn receive(&mut self, record: &HostRecord, stream: &UnixStream) -> Result<(), String> {
        let Some(peer) = self.peer.clone() else {
            return self.accept_hello(record, stream);
        };
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
            "status" => {
                let mut status = self.shared.identity.clone();
                status.insert("type".into(), Value::from("status"));
                status.insert("id".into(), Value::from(id));
                peer.send(status);
                Ok(())
            }
            "stop" => {
                peer.send(record_from(json!({ "type": "stopped", "id": id })));
                self.shared.request_stop();
                Ok(())
            }
            _ => Err("Unknown host operation".into()),
        }
    }

    fn accept_hello(&mut self, record: &HostRecord, stream: &UnixStream) -> Result<(), String> {
        let client_id = record_text(record, "clientId").filter(|id| is_valid_host_identifier(id));
        let is_matching = record_text(record, "type") == Some("hello")
            && record_integer(record, "version") == Some(HOST_PROTOCOL_VERSION as i64)
            && record_text(record, "namespace") == Some(self.shared.namespace.as_str())
            && record_text(record, "role") == Some("desktop")
            && record_text(record, "buildHash") == Some(self.shared.build_hash.as_str());
        let mismatch =
            || "Host handshake namespace, role, version, build or client mismatch".to_string();
        let client_id = client_id.filter(|_| is_matching).ok_or_else(mismatch)?;
        let peer = Arc::new(Peer {
            client_id: client_id.to_string(),
            outbox: Arc::clone(&self.outbox),
            hooks: Mutex::new(HashMap::new()),
        });
        {
            let mut peers = self.shared.peers.lock().unwrap();
            if peers.contains_key(client_id) {
                return Err(mismatch());
            }
            peers.insert(client_id.to_string(), Arc::clone(&peer));
        }
        let _ = stream.set_read_timeout(None);
        self.peer = Some(Arc::clone(&peer));
        peer.send(self.shared.identity.clone());
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
