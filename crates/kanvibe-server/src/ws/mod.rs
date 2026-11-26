//! 모바일 WebSocket(TS `attachMobileStreamServer`의 upgrade 처리기).
//!
//! 인증을 통과하지 못한 요청에는 핸드셰이크를 아예 열지 않는다. 스트림 경로가 아니면 아무것도 쓰지 않고 끊고,
//! 인증 실패와 빠진 쿼리는 헤더 없는 상태 줄만 쓴 뒤 끊는다. Origin은 보지 않는다.

mod ai_login;
mod commands;
mod events;
pub mod socket;
mod stream;

use std::sync::Arc;

use base64::Engine;
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::frame::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::{Role, WebSocketConfig};

use self::socket::{Outgoing, SocketHandle};
use crate::http::{IoStream, PeerContext, UpgradeHead};
use crate::routes::js_semantics::{MobileUrl, match_path};
use crate::routes::mobile::call_bridge;
use crate::server::ServerState;

pub const STREAM_PATH: &str = "/api/mobile/stream";
pub const EVENTS_PATH: &str = "/api/mobile/events";
pub const DEVICE_UNPAIRED_CLOSE_CODE: u16 = 4001;
const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// 닫기 프레임을 보낸 뒤 상대의 닫기를 기다리는 상한(TS `ws`의 closeTimeout)
const CLOSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// 소켓이 받은 데이터 프레임을 처리하는 쪽
pub trait SocketBehavior: Send + Sync {
    fn on_message(&self, data: Bytes, is_binary: bool);
    fn on_close(&self);
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SocketRoute {
    Events,
    Stream { task_id: String, pane_id: String },
    AiLogin,
}

fn resolve_route(url: &MobileUrl) -> Option<Result<SocketRoute, ()>> {
    if url.pathname == EVENTS_PATH {
        return Some(Ok(SocketRoute::Events));
    }
    if match_path(&url.pathname, "/api/mobile/ai/accounts/*/login").is_some() {
        return Some(Ok(SocketRoute::AiLogin));
    }
    if url.pathname != STREAM_PATH {
        return None;
    }
    let task_id = url.param("taskId").filter(|value| !value.is_empty());
    let pane_id = url.param("paneId").filter(|value| !value.is_empty());
    Some(match (task_id, pane_id) {
        (Some(task_id), Some(pane_id)) => Ok(SocketRoute::Stream {
            task_id: task_id.into(),
            pane_id: pane_id.into(),
        }),
        _ => Err(()),
    })
}

pub async fn handle_upgrade(
    mut stream: Box<dyn IoStream>,
    head: UpgradeHead,
    _peer: PeerContext,
    state: Arc<ServerState>,
) {
    let Some(url) = MobileUrl::parse(&head.raw_url) else {
        return;
    };
    let Some(route) = resolve_route(&url) else {
        return;
    };
    let authorization = head
        .headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let checked = authorization.clone();
    let device_id = call_bridge(&state, move |bridge| {
        bridge.authorize_mobile_request(checked.as_deref())
    })
    .await;
    let Ok(Some(device_id)) = device_id else {
        let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\n\r\n").await;
        return;
    };
    let Ok(route) = route else {
        let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
        return;
    };
    let Some(accept) = handshake_accept(&head) else {
        let _ = stream
            .write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Type: text/html\r\nContent-Length: 11\r\n\r\nBad Request")
            .await;
        return;
    };
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    if stream.write_all(response.as_bytes()).await.is_err() {
        return;
    }
    let io = crate::http::rewind_stream(head.trailing.clone(), stream);
    let websocket =
        WebSocketStream::from_raw_socket(io, Role::Server, Some(WebSocketConfig::default())).await;
    let session = SocketSession {
        device_id,
        authorization,
        url,
        route,
    };
    run_socket(websocket, session, state).await;
}

/// `Sec-WebSocket-Key`가 있고 버전이 13이며 GET일 때만 수락 키를 만든다
fn handshake_accept(head: &UpgradeHead) -> Option<String> {
    let is_websocket = head
        .headers
        .get(http::header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    let version = head
        .headers
        .get("sec-websocket-version")
        .and_then(|value| value.to_str().ok());
    let key = head
        .headers
        .get("sec-websocket-key")
        .and_then(|value| value.to_str().ok())?;
    if head.method != "GET" || !is_websocket || version != Some("13") {
        return None;
    }
    let mut hasher = Sha1::new();
    hasher.update(key.trim().as_bytes());
    hasher.update(WEBSOCKET_GUID.as_bytes());
    Some(base64::engine::general_purpose::STANDARD.encode(hasher.finalize()))
}

struct SocketSession {
    device_id: String,
    authorization: Option<String>,
    url: MobileUrl,
    route: SocketRoute,
}

async fn run_socket<S>(
    websocket: WebSocketStream<S>,
    session: SocketSession,
    state: Arc<ServerState>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (sink, mut incoming) = websocket.split();
    let (sender, receiver) = mpsc::unbounded_channel();
    let (terminate_sender, mut terminated) = watch::channel(false);
    let socket = Arc::new(SocketHandle::new(state.sockets.next_socket_id(), sender));
    state.sockets.register(&session.device_id, socket.clone());
    tokio::spawn(write_outgoing(
        sink,
        receiver,
        socket.clone(),
        terminate_sender,
    ));
    let behavior = attach_behavior(&session, socket.clone(), &state).await;
    loop {
        let message = tokio::select! {
            message = incoming.next() => message,
            _ = terminated.changed() => None,
        };
        match message {
            Some(Ok(Message::Text(text))) => {
                dispatch(&behavior, Bytes::from(text.as_str().to_owned()), false)
            }
            Some(Ok(Message::Binary(data))) => dispatch(&behavior, data, true),
            Some(Ok(Message::Pong(_))) => socket.mark_alive(),
            Some(Ok(Message::Ping(_) | Message::Frame(_))) => {}
            Some(Ok(Message::Close(_)) | Err(_)) | None => break,
        }
    }
    socket.mark_ended();
    state.sockets.forget(&session.device_id, socket.id);
    if let Some(behavior) = behavior {
        behavior.on_close();
    }
    socket.terminate();
}

fn dispatch(behavior: &Option<Arc<dyn SocketBehavior>>, data: Bytes, is_binary: bool) {
    if let Some(behavior) = behavior {
        behavior.on_message(data, is_binary);
    }
}

/// 핸드셰이크 뒤 인증을 한 번 더 본다. 기기 조회 사이에 끊긴 기기의 소켓을 놓치지 않으려는 것이다
async fn attach_behavior(
    session: &SocketSession,
    socket: Arc<SocketHandle>,
    state: &Arc<ServerState>,
) -> Option<Arc<dyn SocketBehavior>> {
    let authorization = session.authorization.clone();
    let current = call_bridge(state, move |bridge| {
        bridge.authorize_mobile_request(authorization.as_deref())
    })
    .await;
    if current.ok().flatten().as_deref() != Some(session.device_id.as_str()) {
        socket.close(DEVICE_UNPAIRED_CLOSE_CODE, "device-unpaired");
        return None;
    }
    let behavior: Arc<dyn SocketBehavior> = match &session.route {
        SocketRoute::Events => events::attach_events_socket(socket, state.clone()),
        SocketRoute::Stream { task_id, pane_id } => {
            let target = stream::PaneStreamTarget {
                task_id: task_id.clone(),
                pane_id: pane_id.clone(),
                device_id: session.device_id.clone(),
                grant_id: session.url.param("grantId").map(str::to_string),
            };
            stream::open_pane_stream(socket, target, state.clone())
        }
        SocketRoute::AiLogin => ai_login::attach_ai_login_socket(
            socket,
            &session.device_id,
            &session.url,
            state.clone(),
        ),
    };
    Some(behavior)
}

async fn write_outgoing<W>(
    mut sink: W,
    mut receiver: mpsc::UnboundedReceiver<Outgoing>,
    socket: Arc<SocketHandle>,
    terminate: watch::Sender<bool>,
) where
    W: futures_util::Sink<Message> + Unpin,
{
    while let Some(outgoing) = receiver.recv().await {
        let mut text_bytes = 0;
        let message = match outgoing {
            Outgoing::Text(text) => {
                text_bytes = text.len();
                Message::text(text)
            }
            Outgoing::Ping => Message::Ping(Bytes::new()),
            Outgoing::Close(code, reason) => {
                schedule_close_timeout(terminate.clone());
                Message::Close(Some(CloseFrame {
                    code: code.into(),
                    reason: reason.into(),
                }))
            }
            Outgoing::Terminate => break,
        };
        let is_sent = sink.send(message).await.is_ok();
        socket.mark_written(text_bytes);
        if !is_sent {
            break;
        }
    }
    let _ = terminate.send(true);
}

fn schedule_close_timeout(terminate: watch::Sender<bool>) {
    tokio::spawn(async move {
        tokio::time::sleep(CLOSE_TIMEOUT).await;
        let _ = terminate.send(true);
    });
}
