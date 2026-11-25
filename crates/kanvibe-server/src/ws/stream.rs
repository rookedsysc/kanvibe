//! `/api/mobile/stream` 소켓: pane 하나를 비추고 입력·크기·휠을 그 pane에 넘긴다(TS `openPaneStream`).
//!
//! 텍스트 프레임은 키 입력이고, 이진 프레임(256바이트 이하 JSON)은 제어 요청이다. 키 입력 하나가 세션 조회와 명령 실행을
//! 지나므로 겹치면 `ls`가 `sl`로 들어갈 수 있어, 소켓 하나의 메시지는 작업 하나가 받은 순서대로 처리한다.
//! 구독이 붙기 전에 온 메시지는 줄에서 기다린다.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use serde_json::Value;
use tokio::sync::mpsc;

use super::SocketBehavior;
use super::socket::SocketHandle;
use crate::bridge::{BridgeError, PaneViewport, RemotePaneRelay, RemotePaneTarget, Subscription};
use crate::routes::mobile::call_bridge;
use crate::server::ServerState;

const STREAM_FAILED_CLOSE_CODE: u16 = 4000;
const TASK_SESSION_CHANGED_CLOSE_CODE: u16 = 4002;
const MAX_CONTROL_MESSAGE_BYTES: usize = 256;

pub struct PaneStreamTarget {
    pub task_id: String,
    pub pane_id: String,
    pub device_id: String,
    pub grant_id: Option<String>,
}

struct PaneStream {
    socket: Arc<SocketHandle>,
    state: Arc<ServerState>,
    target: PaneStreamTarget,
    is_closed: AtomicBool,
    queue: Mutex<Option<mpsc::UnboundedSender<(Bytes, bool)>>>,
    relay: Mutex<Option<Arc<dyn RemotePaneRelay>>>,
    subscription: Mutex<Option<Subscription>>,
    session_watch: Mutex<Option<Subscription>>,
}

pub fn open_pane_stream(
    socket: Arc<SocketHandle>,
    target: PaneStreamTarget,
    state: Arc<ServerState>,
) -> Arc<dyn SocketBehavior> {
    let (sender, receiver) = mpsc::unbounded_channel();
    let stream = Arc::new(PaneStream {
        socket,
        state,
        target,
        is_closed: AtomicBool::new(false),
        queue: Mutex::new(Some(sender)),
        relay: Mutex::new(None),
        subscription: Mutex::new(None),
        session_watch: Mutex::new(None),
    });
    watch_session_change(&stream);
    tokio::spawn(run_stream(stream.clone(), receiver));
    stream
}

/// 데스크탑이 세션을 바꾸면 기기는 4002를 받고 새 세션의 pane을 다시 받는다
fn watch_session_change(stream: &Arc<PaneStream>) {
    let socket = stream.socket.clone();
    let on_changed =
        Arc::new(move || socket.close(TASK_SESSION_CHANGED_CLOSE_CODE, "session-changed"));
    let watch = stream
        .state
        .bridge
        .watch_task_session_change(&stream.target.task_id, on_changed);
    *lock(&stream.session_watch) = Some(watch);
}

impl SocketBehavior for PaneStream {
    fn on_message(&self, data: Bytes, is_binary: bool) {
        if let Some(queue) = lock(&self.queue).as_ref() {
            let _ = queue.send((data, is_binary));
        }
    }

    /// 구독이 붙기 전에 기기가 나가도 파이프를 걸어 둔 채 남지 않게 곧바로 되돌린다
    fn on_close(&self) {
        self.is_closed.store(true, Ordering::SeqCst);
        drop(lock(&self.session_watch).take());
        if let Some(relay) = lock(&self.relay).take() {
            relay.close();
        }
        drop(lock(&self.subscription).take());
        drop(lock(&self.queue).take());
    }
}

impl PaneStream {
    fn is_open(&self) -> bool {
        !self.is_closed.load(Ordering::SeqCst) && self.socket.is_open()
    }
}

async fn run_stream(stream: Arc<PaneStream>, mut receiver: mpsc::UnboundedReceiver<(Bytes, bool)>) {
    subscribe(&stream).await;
    let mut viewport: Option<Arc<dyn PaneViewport>> = None;
    while let Some((data, is_binary)) = receiver.recv().await {
        if !stream.is_open() {
            continue;
        }
        if let Err(true) = handle_message(&stream, &mut viewport, data, is_binary).await {
            stream
                .socket
                .close(STREAM_FAILED_CLOSE_CODE, "resize-failed");
        }
    }
    if let Some(viewport) = viewport {
        let _ = tokio::task::spawn_blocking(move || viewport.release()).await;
    }
}

/// 원격 Herdr면 B의 agent로 중계하고, 아니면 이 데스크탑의 미러를 구독한다
async fn subscribe(stream: &Arc<PaneStream>) {
    match open_remote_relay(stream).await {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => return close_for_failure(stream, &error),
    }
    let output_socket = stream.socket.clone();
    let end_socket = stream.socket.clone();
    let on_chunk = Arc::new(move |chunk: String| output_socket.send_text(chunk));
    let on_end = Arc::new(move || end_socket.close(STREAM_FAILED_CLOSE_CODE, "stream-failed"));
    let task_id = stream.target.task_id.clone();
    let pane_id = stream.target.pane_id.clone();
    let outcome = call_bridge(&stream.state, move |bridge| {
        bridge.subscribe_to_pane(&task_id, &pane_id, on_chunk, on_end)
    })
    .await;
    match outcome {
        Ok(subscription) if stream.is_closed.load(Ordering::SeqCst) => subscription.stop(),
        Ok(subscription) => *lock(&stream.subscription) = Some(subscription),
        Err(error) => close_for_failure(stream, &error),
    }
}

async fn open_remote_relay(stream: &Arc<PaneStream>) -> Result<bool, BridgeError> {
    let target = RemotePaneTarget {
        task_id: stream.target.task_id.clone(),
        pane_id: stream.target.pane_id.clone(),
        device_id: stream.target.device_id.clone(),
        grant_id: stream.target.grant_id.clone(),
    };
    let output_socket = stream.socket.clone();
    let close_socket = stream.socket.clone();
    let on_chunk = Arc::new(move |chunk: String| output_socket.send_text(chunk));
    let on_close = Arc::new(move |code: u16, reason: String| close_socket.close(code, &reason));
    let relay = call_bridge(&stream.state, move |bridge| {
        bridge.open_remote_mobile_pane_stream(target, on_chunk, on_close)
    })
    .await?;
    let Some(relay) = relay else {
        return Ok(false);
    };
    let relay: Arc<dyn RemotePaneRelay> = Arc::from(relay);
    if stream.is_closed.load(Ordering::SeqCst) {
        relay.close();
    } else {
        *lock(&stream.relay) = Some(relay);
    }
    Ok(true)
}

fn close_for_failure(stream: &PaneStream, error: &BridgeError) {
    let reason = match error {
        BridgeError::SurfaceUnavailable(reason) => reason.as_str(),
        _ => "stream-failed",
    };
    stream.socket.close(STREAM_FAILED_CLOSE_CODE, reason);
}

/// 이진 프레임은 입력이 아니라 제어 요청이다. 읽을 수 없으면 버린다
fn parse_control_message(data: &[u8]) -> Option<Value> {
    if data.len() > MAX_CONTROL_MESSAGE_BYTES {
        return None;
    }
    serde_json::from_str(&String::from_utf8_lossy(data)).ok()
}

fn read_resize(control: &Value) -> Option<(u16, u16)> {
    if control.get("type").and_then(Value::as_str) != Some("resize") {
        return None;
    }
    let dimension = |name: &str| {
        let value = control.get(name)?.as_f64()?;
        (value.fract() == 0.0 && (1.0..=500.0).contains(&value)).then_some(value as u16)
    };
    Some((dimension("cols")?, dimension("rows")?))
}

/// 실패하면 크기 요청이었는지를 돌려준다. 크기 요청 실패만 소켓을 닫는다
async fn handle_message(
    stream: &Arc<PaneStream>,
    viewport: &mut Option<Arc<dyn PaneViewport>>,
    data: Bytes,
    is_binary: bool,
) -> Result<(), bool> {
    let control = if is_binary {
        parse_control_message(&data)
    } else {
        None
    };
    let control_type = control
        .as_ref()
        .and_then(|control| control.get("type"))
        .and_then(Value::as_str);
    let relay = lock(&stream.relay).clone();
    if control_type == Some("refresh-history") {
        // 이력 갱신은 이 데스크탑의 미러만 아는 요청이라 B의 중계로 넘기지 않는다
        if relay.is_none() {
            let (task_id, pane_id) = stream_ids(stream);
            call_bridge(&stream.state, move |bridge| {
                bridge.refresh_pane_history(&task_id, &pane_id)
            })
            .await
            .map_err(|_| false)?;
        }
        return Ok(());
    }
    if let Some(relay) = relay {
        relay.write(&data, is_binary);
        return Ok(());
    }
    if control_type == Some("scroll") {
        return scroll_viewport(stream, viewport.clone(), control.unwrap_or_default()).await;
    }
    if !is_binary {
        let (task_id, pane_id) = stream_ids(stream);
        let input = String::from_utf8_lossy(&data).into_owned();
        return call_bridge(&stream.state, move |bridge| {
            bridge.write_to_pane(&task_id, &pane_id, &input)
        })
        .await
        .map_err(|_| false);
    }
    let Some((cols, rows)) = control.as_ref().and_then(read_resize) else {
        return Ok(());
    };
    resize_viewport(stream, viewport, cols, rows)
        .await
        .map_err(|_| true)
}

fn stream_ids(stream: &PaneStream) -> (String, String) {
    (stream.target.task_id.clone(), stream.target.pane_id.clone())
}

async fn scroll_viewport(
    stream: &PaneStream,
    viewport: Option<Arc<dyn PaneViewport>>,
    control: Value,
) -> Result<(), bool> {
    let Some(viewport) = viewport else {
        return Ok(());
    };
    let screen = tokio::task::spawn_blocking(move || viewport.scroll(&control))
        .await
        .map_err(|_| false)?
        .map_err(|_| false)?;
    if let Some(screen) = screen.filter(|_| stream.is_open()) {
        stream.socket.send_text(screen);
    }
    Ok(())
}

/// 첫 크기 요청이 viewport를 잡고, 바뀐 크기의 첫 화면이 있으면 이 기기에 보낸다
async fn resize_viewport(
    stream: &PaneStream,
    viewport: &mut Option<Arc<dyn PaneViewport>>,
    cols: u16,
    rows: u16,
) -> Result<(), BridgeError> {
    if viewport.is_none() {
        let (task_id, pane_id) = stream_ids(stream);
        let claimed = call_bridge(&stream.state, move |bridge| {
            bridge.claim_pane_viewport(&task_id, &pane_id)
        })
        .await?;
        *viewport = Some(Arc::from(claimed));
    }
    let Some(claimed) = viewport.clone() else {
        return Ok(());
    };
    let snapshot = tokio::task::spawn_blocking(move || claimed.resize(cols, rows))
        .await
        .map_err(|error| BridgeError::Internal(error.to_string()))??;
    if let Some(snapshot) = snapshot.filter(|_| stream.is_open()) {
        stream.socket.send_text(snapshot);
    }
    Ok(())
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
