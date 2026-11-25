//! SSH 연결과 따로 원격 PTY를 소유하는 레지스트리(TS `remoteHostPty.ts`).
//!
//! 세션은 daemon이 갖고, 데스크탑 연결은 세션에 붙었다 떨어지는 attachment일 뿐이다. 같은 데스크탑(client id)이
//! 같은 key로 다시 열면 같은 세션과 PID에 다시 붙고, 붙는 순간 지금 화면을 받은 뒤 이후 출력을 차례로 받는다.
//! 입력은 커널이 받아 간 조각마다 `pty.input-ready`로 알리고, 출력은 attachment가 ACK하지 않은 양이 1 MiB를
//! 넘으면 그 attachment만 떼어 낸다. 다른 데스크탑의 세션은 열거나 닫을 수 없다.
//!
//! 잠금 순서: 레지스트리 상태 → 세션 출력. 세션 출력 잠금을 쥔 채 레지스트리 상태를 잠그지 않는다.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::pty_launcher::{LaunchedPty, PtyControl, PtyLauncher, PtyWaiter};
use super::screen::DaemonScreen;
use crate::protocol::{
    HOST_BUFFER_BYTES, HOST_CHUNK_BYTES, HOST_RECORD_BYTES, HostRecord, decode_host_bytes,
    encode_host_bytes, record_from, record_integer, record_text,
};

const MAX_SESSIONS: usize = 64;
const MAX_OWNER_SESSIONS: usize = 16;
const MAX_SESSION_ATTACHMENTS: usize = 16;
const OUTPUT_WINDOW_BYTES: usize = 1024 * 1024;
/// 프로세스가 끝난 뒤 남은 출력을 비우기를 기다리는 상한. 손주 프로세스가 PTY를 쥐고 있어도 끝을 알린다
const EXIT_FLUSH_LIMIT: Duration = Duration::from_secs(2);

/// PTY 레코드를 받는 데스크탑 연결 하나. 같은 연결인지는 `Arc` 주소로 가린다
pub trait PtyPeer: Send + Sync {
    fn client_id(&self) -> &str;
    fn send(&self, record: HostRecord) -> bool;
}

type PeerKey = usize;

fn peer_key(peer: &Arc<dyn PtyPeer>) -> PeerKey {
    Arc::as_ptr(peer) as *const () as usize
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionPhase {
    Running,
    Closing,
    Exited,
}

struct AttachmentState {
    token: u64,
    peer: Arc<dyn PtyPeer>,
    id: String,
    pending_output: usize,
    is_ready: bool,
}

struct SessionOutput {
    screen: DaemonScreen,
    attachments: Vec<AttachmentState>,
}

struct InputChunk {
    token: u64,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct InputQueue {
    chunks: VecDeque<InputChunk>,
    queued_bytes: usize,
    is_writing: bool,
    is_stopped: bool,
}

struct PtySession {
    id: String,
    key: String,
    owner: String,
    pid: u32,
    size: Mutex<(u16, u16)>,
    control: Box<dyn PtyControl>,
    phase: Mutex<SessionPhase>,
    output: Mutex<SessionOutput>,
    input: Mutex<InputQueue>,
    input_changed: Condvar,
    reader_finished: Mutex<bool>,
    reader_changed: Condvar,
}

/// 레지스트리가 attachment를 찾는 자리. token이 세션 출력의 attachment와 같아야 살아 있다
struct Route {
    session: Arc<PtySession>,
    token: u64,
}

#[derive(Default)]
struct RegistryState {
    sessions: HashMap<String, Arc<PtySession>>,
    routes: HashMap<(PeerKey, String), Route>,
}

pub struct PtyRegistry {
    launcher: Arc<dyn PtyLauncher>,
    state: Mutex<RegistryState>,
    next_token: AtomicU64,
}

fn dimensions(record: &HostRecord) -> Result<(u16, u16), String> {
    let columns = record_integer(record, "cols").filter(|value| (1..=1000).contains(value));
    let rows = record_integer(record, "rows").filter(|value| (1..=500).contains(value));
    match (columns, rows) {
        (Some(columns), Some(rows)) => Ok((columns as u16, rows as u16)),
        _ => Err("Invalid PTY size".into()),
    }
}

impl PtyRegistry {
    pub fn new(launcher: Arc<dyn PtyLauncher>) -> Arc<Self> {
        Arc::new(Self {
            launcher,
            state: Mutex::new(RegistryState::default()),
            next_token: AtomicU64::new(1),
        })
    }

    /// `pty.*` 레코드 하나를 처리한다. 오류면 호출자가 `pty.error`로 알리고 attachment를 뗀다
    pub fn receive(
        self: &Arc<Self>,
        peer: &Arc<dyn PtyPeer>,
        record: &HostRecord,
    ) -> Result<(), String> {
        let id = record_text(record, "id").unwrap_or_default().to_string();
        let kind = record_text(record, "type").unwrap_or_default();
        match kind {
            "pty.list" => self.list(peer, &id),
            "pty.close" if record.contains_key("sessionId") => {
                self.close_by_session(peer, record, &id)
            }
            "pty.open" => self.open(peer, record, &id),
            _ => self.receive_attached(peer, record, kind, &id),
        }
    }

    fn list(&self, peer: &Arc<dyn PtyPeer>, id: &str) -> Result<(), String> {
        let state = self.state.lock().unwrap();
        let sessions: Vec<Value> = state
            .sessions
            .values()
            .filter(|session| session.owner == peer.client_id() && session.phase() == SessionPhase::Running)
            .map(|session| json!({ "sessionId": session.id, "key": session.key, "pid": session.pid }))
            .collect();
        drop(state);
        peer.send(record_from(
            json!({ "type": "pty.sessions", "id": id, "sessions": sessions }),
        ));
        Ok(())
    }

    fn close_by_session(
        &self,
        peer: &Arc<dyn PtyPeer>,
        record: &HostRecord,
        id: &str,
    ) -> Result<(), String> {
        let session_id = record.get("sessionId").map(value_text).unwrap_or_default();
        let session = self.owned_session(peer, &session_id)?;
        session.control.kill();
        peer.send(record_from(
            json!({ "type": "pty.closed", "id": id, "sessionId": session.id }),
        ));
        Ok(())
    }

    fn owned_session(
        &self,
        peer: &Arc<dyn PtyPeer>,
        session_id: &str,
    ) -> Result<Arc<PtySession>, String> {
        let state = self.state.lock().unwrap();
        state
            .sessions
            .get(session_id)
            .filter(|session| session.owner == peer.client_id())
            .cloned()
            .ok_or_else(|| "Unknown or unowned PTY session".to_string())
    }

    fn receive_attached(
        &self,
        peer: &Arc<dyn PtyPeer>,
        record: &HostRecord,
        kind: &str,
        id: &str,
    ) -> Result<(), String> {
        let Some((session, token)) = self.route(peer, id) else {
            return Ok(());
        };
        match kind {
            "pty.input" => {
                let bytes = decode_host_bytes(record.get("data"), HOST_CHUNK_BYTES)
                    .map_err(|error| error.to_string())?;
                session.queue_input(token, bytes)
            }
            "pty.resize" => {
                let (columns, rows) = dimensions(record)?;
                session.control.resize(columns, rows)?;
                *session.size.lock().unwrap() = (columns, rows);
                session.output.lock().unwrap().screen.resize(columns, rows);
                Ok(())
            }
            "pty.ack" => session.acknowledge(token, record_integer(record, "bytes")),
            "pty.detach" => {
                self.remove_attachment(&session, token, peer, id);
                Ok(())
            }
            "pty.close" => {
                session.control.kill();
                Ok(())
            }
            _ => Err("Unknown PTY operation".into()),
        }
    }

    fn route(&self, peer: &Arc<dyn PtyPeer>, id: &str) -> Option<(Arc<PtySession>, u64)> {
        let state = self.state.lock().unwrap();
        let route = state.routes.get(&(peer_key(peer), id.to_string()))?;
        Some((Arc::clone(&route.session), route.token))
    }

    /// 데스크탑 연결이 끊겼다. 세션은 남기고 그 연결의 attachment만 뗀다
    pub fn detach(&self, peer: &Arc<dyn PtyPeer>) {
        let key = peer_key(peer);
        let removed: Vec<(String, Route)> = {
            let mut state = self.state.lock().unwrap();
            let ids: Vec<String> = state
                .routes
                .keys()
                .filter(|(owner, _)| *owner == key)
                .map(|(_, id)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| {
                    state
                        .routes
                        .remove(&(key, id.clone()))
                        .map(|route| (id, route))
                })
                .collect()
        };
        for (_, route) in removed {
            route.session.drop_attachment(route.token);
        }
    }

    /// daemon이 멈춘다. 모든 PTY를 끝낸다
    pub fn close(&self) {
        let sessions: Vec<Arc<PtySession>> = {
            let mut state = self.state.lock().unwrap();
            state.routes.clear();
            state.sessions.drain().map(|(_, session)| session).collect()
        };
        for session in sessions {
            *session.phase.lock().unwrap() = SessionPhase::Exited;
            session.stop_input();
            session.control.kill();
        }
    }

    fn remove_attachment(
        &self,
        session: &Arc<PtySession>,
        token: u64,
        peer: &Arc<dyn PtyPeer>,
        id: &str,
    ) {
        self.state
            .lock()
            .unwrap()
            .routes
            .remove(&(peer_key(peer), id.to_string()));
        session.drop_attachment(token);
    }

    fn remove_routes(&self, removed: &[(PeerKey, String, u64)]) {
        let mut state = self.state.lock().unwrap();
        for (key, id, token) in removed {
            let route_key = (*key, id.clone());
            if state
                .routes
                .get(&route_key)
                .is_some_and(|route| route.token == *token)
            {
                state.routes.remove(&route_key);
            }
        }
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

mod session;
#[cfg(test)]
mod tests;
