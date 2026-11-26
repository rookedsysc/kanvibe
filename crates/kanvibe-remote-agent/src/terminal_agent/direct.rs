//! 모바일이 직접 붙는 WSS 서버(TS `server.on("upgrade")`).
//!
//! `/stream/<grantId>`에 `Authorization: Bearer <token>`으로 업그레이드를 청한 연결만, 토큰을 상수 시간으로 비교해
//! 받는다. 틀리면 401, 업그레이드가 아닌 평범한 HTTPS 요청은 404로 바로 답하고 닫는다. 연결마다 스레드 하나가
//! 짧은 읽기 제한으로 소켓을 돌며 큐에 쌓인 출력을 쓰고 들어온 메시지를 agent에 넘긴다.

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustls::{ServerConfig, ServerConnection, StreamOwned};
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::{CloseFrame, Role, WebSocketConfig};
use tungstenite::{Message, WebSocket};

use super::Agent;
use super::attach::AttachTicket;
use super::state::{ClientChannel, WireGrant};
use super::wire::{
    REMOTE_TERMINAL_COMMAND_TIMEOUT_MS, REMOTE_TERMINAL_FRAME_BYTES, REMOTE_TERMINAL_INPUT_BYTES,
    REMOTE_TERMINAL_OWNER_LEASE_MS,
};

type TlsStream = StreamOwned<ServerConnection, TcpStream>;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(REMOTE_TERMINAL_COMMAND_TIMEOUT_MS);
const SOCKET_POLL_INTERVAL: Duration = Duration::from_millis(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const PING_INTERVAL: Duration = Duration::from_millis(REMOTE_TERMINAL_OWNER_LEASE_MS);
const MAX_HEAD_BYTES: usize = 64 * 1024;

#[derive(Default)]
struct ChannelState {
    outgoing: VecDeque<String>,
    queued_bytes: usize,
    close: Option<(u16, String)>,
    is_terminated: bool,
}

/// 직접 연결로 나갈 출력과 닫기 요청. agent 잠금 안에서 막히지 않고 넣기만 한다
#[derive(Default)]
pub(super) struct DirectChannel {
    state: Mutex<ChannelState>,
}

impl DirectChannel {
    /// 보내지 못한 출력이 8 MiB를 넘으면 느린 연결로 보고 닫는다
    pub(super) fn push_text(&self, text: &str) {
        let mut state = self.state.lock().unwrap();
        if state.close.is_some() || state.is_terminated {
            return;
        }
        if state.queued_bytes + text.len() > REMOTE_TERMINAL_FRAME_BYTES {
            state.close = Some((4000, "slow-client".into()));
            return;
        }
        state.queued_bytes += text.len();
        state.outgoing.push_back(text.to_string());
    }

    pub(super) fn request_close(&self, code: u16, reason: &str) {
        self.state
            .lock()
            .unwrap()
            .close
            .get_or_insert_with(|| (code, reason.to_string()));
    }

    /// 닫기 핸드셰이크 없이 끊는다(TS `ws.terminate()`)
    pub(super) fn terminate(&self) {
        self.state.lock().unwrap().is_terminated = true;
    }

    fn take(&self) -> (Vec<String>, Option<(u16, String)>, bool) {
        let mut state = self.state.lock().unwrap();
        state.queued_bytes = 0;
        let outgoing = state.outgoing.drain(..).collect();
        (outgoing, state.close.clone(), state.is_terminated)
    }
}

pub(super) fn spawn_listener(agent: &Arc<Agent>, listener: TcpListener, config: Arc<ServerConfig>) {
    let agent = Arc::clone(agent);
    std::thread::spawn(move || {
        while agent.is_listening.load(Ordering::SeqCst) {
            if !is_readable(listener.as_raw_fd()) {
                continue;
            }
            let Ok((stream, _)) = listener.accept() else {
                continue;
            };
            let (agent, config) = (Arc::clone(&agent), Arc::clone(&config));
            std::thread::spawn(move || serve_connection(&agent, stream, config));
        }
    });
}

fn is_readable(file_descriptor: i32) -> bool {
    let mut poll = libc::pollfd {
        fd: file_descriptor,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: 살아 있는 리스너 fd 하나만 넘긴다
    let ready = unsafe { libc::poll(&mut poll, 1, 100) };
    ready > 0 && poll.revents & libc::POLLIN != 0
}

struct RequestHead {
    path: String,
    headers: Vec<(String, String)>,
    leftover: Vec<u8>,
}

impl RequestHead {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn serve_connection(agent: &Arc<Agent>, stream: TcpStream, config: Arc<ServerConfig>) {
    let _ = stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT));
    let _ = stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT));
    let Ok(connection) = ServerConnection::new(config) else {
        return;
    };
    let mut tls = StreamOwned::new(connection, stream);
    let Some(head) = read_head(&mut tls) else {
        return;
    };
    let is_upgrade = head
        .header("upgrade")
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    let key = head.header("sec-websocket-key").map(str::to_string);
    let (true, Some(key)) = (is_upgrade, key) else {
        finish_plain(
            &mut tls,
            "HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        );
        return;
    };
    let Some(grant) = admit(agent, &head) else {
        finish_plain(
            &mut tls,
            "HTTP/1.1 401 Unauthorized\r\nConnection: close\r\n\r\n",
        );
        return;
    };
    // 101을 쓰기 전에 grant를 쥐어야, 모바일이 열림을 본 뒤 온 철회가 이 연결을 닫는다
    let attachment = register_direct(agent, grant);
    let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    if tls
        .write_all(response.as_bytes())
        .and_then(|()| tls.flush())
        .is_err()
    {
        attachment.abandon(agent);
        return;
    }
    let config = WebSocketConfig::default()
        .max_message_size(Some(REMOTE_TERMINAL_INPUT_BYTES))
        .max_frame_size(Some(REMOTE_TERMINAL_INPUT_BYTES));
    let socket = WebSocket::from_partially_read(tls, head.leftover, Role::Server, Some(config));
    run_client(agent, socket, attachment);
}

fn finish_plain(tls: &mut TlsStream, response: &str) {
    let _ = tls.write_all(response.as_bytes());
    tls.conn.send_close_notify();
    let _ = tls.flush();
}

fn read_head(tls: &mut TlsStream) -> Option<RequestHead> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let count = tls.read(&mut chunk).ok().filter(|count| *count > 0)?;
        buffer.extend_from_slice(&chunk[..count]);
        let mut slots = [httparse::EMPTY_HEADER; 64];
        let mut request = httparse::Request::new(&mut slots);
        match request.parse(&buffer).ok()? {
            httparse::Status::Complete(length) => {
                let headers = request
                    .headers
                    .iter()
                    .filter_map(|entry| {
                        Some((
                            entry.name.to_string(),
                            std::str::from_utf8(entry.value).ok()?.to_string(),
                        ))
                    })
                    .collect();
                let path = request.path?.to_string();
                return Some(RequestHead {
                    path,
                    headers,
                    leftover: buffer[length..].to_vec(),
                });
            }
            httparse::Status::Partial if buffer.len() > MAX_HEAD_BYTES => return None,
            httparse::Status::Partial => {}
        }
    }
}

/// 경로의 grant가 입장 시간 안에 있고, 토큰이 맞고, 다른 연결이 아직 입력을 쓰지 않았으면 받는다
fn admit(agent: &Agent, head: &RequestHead) -> Option<Arc<WireGrant>> {
    let grant_id = head.path.strip_prefix("/stream/")?;
    let is_valid_id = !grant_id.is_empty()
        && grant_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    let token = head
        .header("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    let state = agent.lock();
    let slot = state.grants.get(grant_id).filter(|_| is_valid_id)?;
    let is_in_use = slot
        .active
        .and_then(|active| state.clients.get(&active))
        .is_some_and(|client| client.has_written);
    let is_admitted = slot.admission_until > Instant::now()
        && is_same_secret(slot.wire.token.as_bytes(), token.as_bytes())
        && !is_in_use;
    is_admitted.then(|| Arc::clone(&slot.wire))
}

/// 길이가 같으면 모든 바이트를 끝까지 비교해 걸린 시간으로 토큰을 알아낼 수 없게 한다
fn is_same_secret(expected: &[u8], supplied: &[u8]) -> bool {
    expected.len() == supplied.len()
        && expected
            .iter()
            .zip(supplied)
            .fold(0u8, |difference, (left, right)| difference | (left ^ right))
            == 0
}

/// 직접 연결 하나와 그 연결이 쥔 grant
struct DirectAttachment {
    channel: Arc<DirectChannel>,
    client_id: u64,
    ticket: Result<AttachTicket, String>,
}

impl DirectAttachment {
    fn abandon(self, agent: &Agent) {
        agent.forget_direct(self.client_id);
    }
}

fn register_direct(agent: &Agent, grant: Arc<WireGrant>) -> DirectAttachment {
    let channel = Arc::new(DirectChannel::default());
    let grant_id = grant.grant_id.clone();
    let mut state = agent.lock();
    let client_id = state.add_client(grant, ClientChannel::Direct(Arc::clone(&channel)));
    let ticket = agent.begin_attach(&mut state, &grant_id, client_id);
    DirectAttachment {
        channel,
        client_id,
        ticket,
    }
}

fn run_client(agent: &Arc<Agent>, mut socket: WebSocket<TlsStream>, attachment: DirectAttachment) {
    let DirectAttachment {
        channel,
        client_id,
        ticket,
    } = attachment;
    let attaching = Arc::clone(agent);
    std::thread::spawn(move || {
        let result = ticket.and_then(|ticket| attaching.complete_attach(ticket, client_id));
        attaching.finish_direct_attach(client_id, result);
    });
    let _ = socket
        .get_ref()
        .sock
        .set_read_timeout(Some(SOCKET_POLL_INTERVAL));
    let _ = socket.get_ref().sock.set_write_timeout(Some(WRITE_TIMEOUT));
    pump_socket(agent, &mut socket, &channel, client_id);
    agent.forget_direct(client_id);
}

struct PumpState {
    is_alive: bool,
    next_ping: Instant,
    closing_since: Option<Instant>,
}

/// 소켓이 닫힐 때까지 출력을 쓰고 입력을 읽는다
fn pump_socket(
    agent: &Agent,
    socket: &mut WebSocket<TlsStream>,
    channel: &DirectChannel,
    client_id: u64,
) {
    let mut pump = PumpState {
        is_alive: true,
        next_ping: Instant::now() + PING_INTERVAL,
        closing_since: None,
    };
    loop {
        if !flush_outgoing(agent, socket, channel, client_id, &mut pump)
            || !keep_alive(socket, &mut pump)
        {
            return;
        }
        match socket.read() {
            Ok(Message::Text(text)) if pump.closing_since.is_none() => {
                agent.receive_direct(client_id, text.as_bytes().to_vec(), false)
            }
            Ok(Message::Binary(bytes)) if pump.closing_since.is_none() => {
                agent.receive_direct(client_id, bytes.to_vec(), true)
            }
            Ok(Message::Pong(_)) => pump.is_alive = true,
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return,
        }
    }
}

fn flush_outgoing(
    agent: &Agent,
    socket: &mut WebSocket<TlsStream>,
    channel: &DirectChannel,
    client_id: u64,
    pump: &mut PumpState,
) -> bool {
    let (outgoing, close, is_terminated) = channel.take();
    if is_terminated {
        return false;
    }
    for text in &outgoing {
        if socket.write(Message::text(text.as_str())).is_err() {
            return false;
        }
    }
    if !outgoing.is_empty() && socket.flush().is_err() {
        return false;
    }
    if let (Some((code, reason)), None) = (close, pump.closing_since) {
        agent.close_client(&mut agent.lock(), client_id, code, &reason);
        let frame = CloseFrame {
            code: CloseCode::from(code),
            reason: reason.into(),
        };
        let _ = socket.close(Some(frame));
        let _ = socket.flush();
        pump.closing_since = Some(Instant::now());
    }
    pump.closing_since
        .is_none_or(|since| since.elapsed() < HANDSHAKE_TIMEOUT)
}

/// 30초마다 ping을 보내고, 앞 ping에 pong이 없었으면 끊는다
fn keep_alive(socket: &mut WebSocket<TlsStream>, pump: &mut PumpState) -> bool {
    if Instant::now() < pump.next_ping {
        return true;
    }
    if !pump.is_alive {
        return false;
    }
    pump.is_alive = false;
    pump.next_ping = Instant::now() + PING_INTERVAL;
    socket.send(Message::Ping(Default::default())).is_ok()
}

impl Agent {
    /// 직접 연결의 소켓이 닫혔다(TS `ws.on("close")`)
    fn forget_direct(&self, client_id: u64) {
        let mut state = self.lock();
        if let Some(client) = state.clients.get_mut(&client_id) {
            client.is_closed = true;
        }
        self.detach(&mut state, client_id);
        state.clients.remove(&client_id);
    }
}
