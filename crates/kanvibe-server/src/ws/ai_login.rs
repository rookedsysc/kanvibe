//! AI 계정 CLI 로그인 PTY 소켓(TS `attachMobileAiLoginStream`)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::SocketBehavior;
use super::socket::SocketHandle;
use crate::bridge::{AiLoginEvents, BridgeError};
use crate::routes::js_semantics::MobileUrl;
use crate::routes::mobile::call_bridge;
use crate::server::ServerState;

const MAX_FRAME_BYTES: usize = 64 * 1024;
/// 휴대폰이 읽지 않는 동안 쌓아 둘 로그인 출력의 상한(TS `MAX_BUFFERED_OUTPUT_BYTES`)
const MAX_BUFFERED_OUTPUT_BYTES: usize = 1024 * 1024;
const DEFAULT_SIZE: (f64, f64) = (80.0, 24.0);
const INVALID_INPUT_ERROR: &str = "터미널 입력 형식이 올바르지 않습니다";

struct AiLoginSocket {
    socket: Arc<SocketHandle>,
    state: Arc<ServerState>,
    device_id: String,
    socket_id: String,
    is_closed: AtomicBool,
    /// PTY 읽기 스레드처럼 tokio 밖에서 불려도 정리 작업을 맡길 런타임
    runtime: tokio::runtime::Handle,
    /// `ready`를 보내기 전에 PTY가 낸 이벤트. 기기는 언제나 `ready`를 먼저 받는다
    pending_events: Mutex<Option<Vec<Value>>>,
    /// 입력·크기 프레임을 받은 순서대로 PTY에 넘기는 큐. 정리하면 비워 둬 큐를 비우는 작업도 끝난다
    terminal_inputs: Mutex<Option<mpsc::UnboundedSender<LoginTerminalInput>>>,
}

enum LoginTerminalInput {
    Write(String),
    Resize(u16, u16),
}

pub fn attach_ai_login_socket(
    socket: Arc<SocketHandle>,
    device_id: &str,
    url: &MobileUrl,
    state: Arc<ServerState>,
) -> Arc<dyn SocketBehavior> {
    let (terminal_input_sender, terminal_input_receiver) = mpsc::unbounded_channel();
    let login = Arc::new(AiLoginSocket {
        socket,
        state,
        device_id: device_id.to_string(),
        socket_id: uuid::Uuid::new_v4().to_string(),
        is_closed: AtomicBool::new(false),
        runtime: tokio::runtime::Handle::current(),
        pending_events: Mutex::new(Some(Vec::new())),
        terminal_inputs: Mutex::new(Some(terminal_input_sender)),
    });
    tokio::spawn(apply_terminal_inputs_in_order(
        login.state.clone(),
        login.device_id.clone(),
        login.socket_id.clone(),
        terminal_input_receiver,
    ));
    let account_id = account_id_from_path(&url.pathname);
    let size = read_terminal_size(url);
    tokio::spawn(open_login(login.clone(), account_id, size));
    login
}

/// `/api/mobile/ai/accounts/<64자리 소문자 16진>/login`만 받는다
fn account_id_from_path(pathname: &str) -> Option<String> {
    let account_id = pathname
        .strip_prefix("/api/mobile/ai/accounts/")?
        .strip_suffix("/login")?;
    let is_hex64 = account_id.len() == 64
        && account_id
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    is_hex64.then(|| account_id.to_string())
}

fn is_valid_size(cols: f64, rows: f64) -> bool {
    let is_integer = |value: f64| value.fract() == 0.0;
    is_integer(cols)
        && is_integer(rows)
        && (2.0..=500.0).contains(&cols)
        && (1.0..=250.0).contains(&rows)
}

fn read_terminal_size(url: &MobileUrl) -> Option<(u16, u16)> {
    let read = |name: &str, fallback: f64| match url.has_param(name) {
        true => url
            .param(name)
            .and_then(|text| text.trim().parse::<f64>().ok())
            .unwrap_or(f64::NAN),
        false => fallback,
    };
    let (cols, rows) = (read("cols", DEFAULT_SIZE.0), read("rows", DEFAULT_SIZE.1));
    is_valid_size(cols, rows).then_some((cols as u16, rows as u16))
}

async fn open_login(
    login: Arc<AiLoginSocket>,
    account_id: Option<String>,
    size: Option<(u16, u16)>,
) {
    let (Some(account_id), Some(size)) = (account_id, size) else {
        login.send(json!({ "type": "error", "error": "계정 또는 터미널 크기를 확인해 주세요" }));
        login.finish(1008, "invalid-login-request");
        return;
    };
    let events: Arc<dyn AiLoginEvents> = login.clone();
    let (device_id, socket_id) = (login.device_id.clone(), login.socket_id.clone());
    let opened = call_bridge(&login.state, move |bridge| {
        bridge.open_mobile_ai_login(&device_id, &socket_id, &account_id, size, events)
    })
    .await;
    match opened {
        Ok(()) if !login.is_open() => login.cleanup(),
        Ok(()) => login.release_ready(),
        Err(error) => {
            login.send(json!({ "type": "error", "error": login_error_message(&error) }));
            login.finish(1011, "login-open-failed");
        }
    }
}

/// blocking 작업을 하나씩 끝까지 기다려, 이웃한 두 프레임이 다른 스레드에서 앞질러 PTY에 들어가지 않게 한다
async fn apply_terminal_inputs_in_order(
    state: Arc<ServerState>,
    device_id: String,
    socket_id: String,
    mut receiver: mpsc::UnboundedReceiver<LoginTerminalInput>,
) {
    while let Some(input) = receiver.recv().await {
        let bridge = state.bridge.clone();
        let (device_id, socket_id) = (device_id.clone(), socket_id.clone());
        let _ = tokio::task::spawn_blocking(move || match input {
            LoginTerminalInput::Write(data) => {
                bridge.write_mobile_ai_login(&device_id, &socket_id, &data)
            }
            LoginTerminalInput::Resize(cols, rows) => {
                bridge.resize_mobile_ai_login(&device_id, &socket_id, cols, rows)
            }
        })
        .await;
    }
}

/// TS `responseError(error).error`
fn login_error_message(error: &BridgeError) -> String {
    match error {
        BridgeError::Ai {
            status: Some(400 | 403 | 404 | 409 | 429),
            message,
        }
        | BridgeError::Feature {
            status: 400 | 403 | 404 | 409 | 429,
            message,
        } => message.clone(),
        _ => "AI 기능 요청을 처리하지 못했습니다".to_string(),
    }
}

impl AiLoginSocket {
    fn is_open(&self) -> bool {
        !self.is_closed.load(Ordering::SeqCst) && self.socket.is_open()
    }

    fn send(&self, event: Value) {
        if !self.is_open() {
            return;
        }
        if self.socket.buffered_amount() > MAX_BUFFERED_OUTPUT_BYTES {
            self.finish(1009, "login-output-overflow");
            return;
        }
        self.socket.send_json(&event);
    }

    fn cleanup(&self) {
        if self.is_closed.swap(true, Ordering::SeqCst) {
            return;
        }
        drop(
            self.terminal_inputs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take(),
        );
        let bridge = self.state.bridge.clone();
        let (device_id, socket_id) = (self.device_id.clone(), self.socket_id.clone());
        self.runtime
            .spawn_blocking(move || bridge.close_mobile_ai_login(&device_id, &socket_id));
    }

    fn finish(&self, code: u16, reason: &str) {
        self.cleanup();
        self.socket.close(code, reason);
    }

    /// `ready`를 보내고, 그 전에 쌓인 PTY 이벤트를 순서대로 흘려보낸다
    fn release_ready(&self) {
        self.send(json!({ "type": "ready" }));
        let pending = self
            .pending_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        for event in pending.unwrap_or_default() {
            self.deliver(event);
        }
    }

    fn deliver(&self, event: Value) {
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_string);
        self.send(event);
        match event_type.as_deref() {
            Some("exit") => self.finish(1000, "login-finished"),
            Some("error") => self.finish(1011, "login-failed"),
            _ => {}
        }
    }

    fn queue_terminal_input(&self, input: LoginTerminalInput) {
        let terminal_inputs = self
            .terminal_inputs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(terminal_inputs) = terminal_inputs.as_ref() {
            let _ = terminal_inputs.send(input);
        }
    }

    fn reject_input(&self) {
        self.send(json!({ "type": "error", "error": INVALID_INPUT_ERROR }));
        self.finish(1008, "invalid-login-input");
    }
}

impl AiLoginEvents for AiLoginSocket {
    fn is_open(&self) -> bool {
        AiLoginSocket::is_open(self)
    }

    fn on_event(&self, event: Value) {
        let mut pending = self
            .pending_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(pending) = pending.as_mut() {
            pending.push(event);
            return;
        }
        drop(pending);
        self.deliver(event);
    }
}

impl SocketBehavior for AiLoginSocket {
    fn on_message(&self, data: Bytes, is_binary: bool) {
        if !self.is_open() {
            return;
        }
        if is_binary || data.len() > MAX_FRAME_BYTES {
            return self.reject_input();
        }
        let command: Value = serde_json::from_slice(&data).unwrap_or(Value::Null);
        match command.get("type").and_then(Value::as_str) {
            Some("input") if command.get("data").is_some_and(Value::is_string) => {
                let data = command["data"].as_str().unwrap_or_default().to_string();
                self.queue_terminal_input(LoginTerminalInput::Write(data));
            }
            Some("resize") if read_resize(&command).is_some() => {
                let (cols, rows) = read_resize(&command).unwrap_or_default();
                self.queue_terminal_input(LoginTerminalInput::Resize(cols, rows));
            }
            _ => self.reject_input(),
        }
    }

    fn on_close(&self) {
        self.cleanup();
    }
}

fn read_resize(command: &Value) -> Option<(u16, u16)> {
    let cols = command.get("cols")?.as_f64()?;
    let rows = command.get("rows")?.as_f64()?;
    is_valid_size(cols, rows).then_some((cols as u16, rows as u16))
}
