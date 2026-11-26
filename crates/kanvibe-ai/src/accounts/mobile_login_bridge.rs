//! 모바일 기기가 여는 provider 로그인 PTY(TS `mobileAiLoginBridge`). 세션은 기기와 소켓이 소유하고,
//! 계정 하나에는 로그인 터미널을 하나만 열며, OAuth 화면을 남겨 둔 기기가 CLI를 계속 점유하지 않도록 수명을 제한한다.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, Weak};

use serde::{Deserialize, Serialize};

use crate::accounts::login_terminal::{
    LoginTerminal, LoginTerminalCallbacks, LoginTerminalSpawner, build_login_terminal_request,
};
use crate::usage::context::UsageContext;
use crate::usage::types::AiUsageProvider;

const LOGIN_TIMEOUT_MS: u64 = 10 * 60 * 1000;
const MAX_LOGINS_PER_DEVICE: usize = 4;
const MAX_TOTAL_LOGINS: usize = 12;
const MIN_TERMINAL_COLUMNS: f64 = 2.0;
const MAX_TERMINAL_COLUMNS: f64 = 500.0;
const MIN_TERMINAL_ROWS: f64 = 1.0;
const MAX_TERMINAL_ROWS: f64 = 250.0;
const MAX_INPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum MobileAiLoginEvent {
    Output {
        data: String,
    },
    Exit {
        #[serde(rename = "exitCode")]
        exit_code: i32,
    },
    Error {
        error: String,
    },
}

/// 로그인을 연 모바일 소켓. 닫힌 소켓에는 사건을 보내지 않는다
pub trait MobileLoginSocket: Send + Sync {
    fn is_open(&self) -> bool;
    fn send_event(&self, event: MobileAiLoginEvent);
}

/// 지연 작업. 앱에서는 스레드 타이머가, 시험에서는 손으로 돌리는 가짜가 맡는다.
/// 돌려받은 취소 신호를 버리면 아직 돌지 않은 작업은 돌지 않는다
pub trait LoginTimerScheduler: Send + Sync {
    fn schedule(&self, delay_ms: u64, task: Box<dyn FnOnce() + Send>) -> LoginTimerCancellation;
}

/// 예약한 지연 작업의 취소 신호. 값을 버리면 짝인 수신 쪽이 끊겨 기다리던 타이머가 바로 깨어나 작업 없이 끝난다
pub struct LoginTimerCancellation {
    _cancel_on_drop: Sender<()>,
}

impl LoginTimerCancellation {
    /// 취소 신호와, 타이머가 기다릴 수신 쪽. 수신 쪽이 끊기면 취소된 것이다
    pub fn new() -> (Self, Receiver<()>) {
        let (sender, receiver) = mpsc::channel();
        (
            Self {
                _cancel_on_drop: sender,
            },
            receiver,
        )
    }
}

/// 지연 작업마다 스레드 하나가 취소 신호를 기다린다. 세션이 먼저 끝나 신호가 버려지면 그 즉시 빠져나간다(TS `clearTimeout`)
pub struct ThreadTimerScheduler;

impl LoginTimerScheduler for ThreadTimerScheduler {
    fn schedule(&self, delay_ms: u64, task: Box<dyn FnOnce() + Send>) -> LoginTimerCancellation {
        let (cancellation, cancelled) = LoginTimerCancellation::new();
        std::thread::spawn(move || {
            let waited = cancelled.recv_timeout(std::time::Duration::from_millis(delay_ms));
            if waited == Err(RecvTimeoutError::Timeout) {
                task();
            }
        });
        cancellation
    }
}

/// HTTP 상태 코드와 사용자에게 보일 문장을 함께 담은 오류
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct MobileAiLoginError {
    pub status_code: u16,
    pub message: String,
}

fn login_error(status_code: u16, message: &str) -> MobileAiLoginError {
    MobileAiLoginError {
        status_code,
        message: message.to_string(),
    }
}

pub struct MobileLoginOpenRequest {
    pub device_id: String,
    pub socket_id: String,
    pub provider: AiUsageProvider,
    pub account_root: String,
    pub columns: f64,
    pub rows: f64,
    pub socket: Arc<dyn MobileLoginSocket>,
}

struct LoginSession {
    device_id: String,
    socket_id: String,
    account_root: String,
    generation: u64,
    socket: Arc<dyn MobileLoginSocket>,
    terminal: Arc<dyn LoginTerminal>,
    /// 세션을 목록에서 떼어 내면 함께 버려져 만료 타이머를 취소한다
    _expiry: LoginTimerCancellation,
}

#[derive(Default)]
struct LoginSessions {
    /// 연 순서를 지킨다
    sessions: Vec<LoginSession>,
    next_generation: u64,
}

impl LoginSessions {
    fn position(&self, device_id: &str, socket_id: &str) -> Option<usize> {
        self.sessions
            .iter()
            .position(|session| session.device_id == device_id && session.socket_id == socket_id)
    }

    fn position_of_generation(&self, generation: u64) -> Option<usize> {
        self.sessions
            .iter()
            .position(|session| session.generation == generation)
    }
}

pub struct MobileLoginBridge {
    context: UsageContext,
    spawner: Arc<dyn LoginTerminalSpawner>,
    timers: Arc<dyn LoginTimerScheduler>,
    sessions: Arc<Mutex<LoginSessions>>,
}

fn lock(sessions: &Mutex<LoginSessions>) -> std::sync::MutexGuard<'_, LoginSessions> {
    sessions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 목록에서 떼어 낸 세션의 PTY를 죽인다. 이미 종료된 PTY여도 다른 기기의 정리는 계속돼야 해서 실패는 넘긴다
fn kill_session_terminal(session: &LoginSession) {
    let _ = session.terminal.kill();
}

fn is_valid_terminal_size(columns: f64, rows: f64) -> bool {
    let is_integer = |value: f64| value.is_finite() && value.fract() == 0.0;
    is_integer(columns)
        && (MIN_TERMINAL_COLUMNS..=MAX_TERMINAL_COLUMNS).contains(&columns)
        && is_integer(rows)
        && (MIN_TERMINAL_ROWS..=MAX_TERMINAL_ROWS).contains(&rows)
}

fn validate_terminal_size(columns: f64, rows: f64) -> Result<(), MobileAiLoginError> {
    if is_valid_terminal_size(columns, rows) {
        Ok(())
    } else {
        Err(login_error(400, "터미널 크기가 올바르지 않습니다"))
    }
}

/// 출력은 같은 세션이 살아 있고 소켓이 열려 있을 때만, 종료는 세션을 정리한 뒤 소켓이 열려 있을 때 알린다
fn create_callbacks(
    sessions: Weak<Mutex<LoginSessions>>,
    socket: Arc<dyn MobileLoginSocket>,
    generation: u64,
) -> LoginTerminalCallbacks {
    let output_sessions = sessions.clone();
    let output_socket = socket.clone();
    LoginTerminalCallbacks {
        on_output: Box::new(move |data| {
            let Some(sessions) = output_sessions.upgrade() else {
                return;
            };
            let is_current = lock(&sessions).position_of_generation(generation).is_some();
            if is_current && output_socket.is_open() {
                output_socket.send_event(MobileAiLoginEvent::Output {
                    data: data.to_string(),
                });
            }
        }),
        on_exit: Box::new(move |exit_code| {
            let Some(sessions) = sessions.upgrade() else {
                return;
            };
            let mut registry = lock(&sessions);
            let Some(index) = registry.position_of_generation(generation) else {
                return;
            };
            registry.sessions.remove(index);
            drop(registry);
            if socket.is_open() {
                socket.send_event(MobileAiLoginEvent::Exit { exit_code });
            }
        }),
    }
}

impl MobileLoginBridge {
    pub fn new(
        context: UsageContext,
        spawner: Arc<dyn LoginTerminalSpawner>,
        timers: Arc<dyn LoginTimerScheduler>,
    ) -> Self {
        Self {
            context,
            spawner,
            timers,
            sessions: Arc::new(Mutex::new(LoginSessions::default())),
        }
    }

    fn check_capacity(
        registry: &LoginSessions,
        request: &MobileLoginOpenRequest,
    ) -> Result<(), MobileAiLoginError> {
        let is_taken = registry
            .position(&request.device_id, &request.socket_id)
            .is_some()
            || registry
                .sessions
                .iter()
                .any(|session| session.account_root == request.account_root);
        if is_taken {
            return Err(login_error(
                409,
                "이 계정의 로그인 터미널이 이미 열려 있습니다",
            ));
        }
        let device_count = registry
            .sessions
            .iter()
            .filter(|session| session.device_id == request.device_id)
            .count();
        if registry.sessions.len() >= MAX_TOTAL_LOGINS || device_count >= MAX_LOGINS_PER_DEVICE {
            return Err(login_error(
                429,
                "열려 있는 로그인 터미널을 먼저 닫아 주세요",
            ));
        }
        Ok(())
    }

    /// 휴대폰 브라우저는 PC의 loopback OAuth callback에 도달하지 못하므로 Codex는 공식 기기 인증(`--device-auth`)을 쓴다
    pub fn open_mobile_ai_login_session(
        &self,
        request: MobileLoginOpenRequest,
    ) -> Result<(), MobileAiLoginError> {
        validate_terminal_size(request.columns, request.rows)?;
        let mut registry = lock(&self.sessions);
        Self::check_capacity(&registry, &request)?;
        if !request.socket.is_open() {
            return Ok(());
        }
        registry.next_generation += 1;
        let generation = registry.next_generation;
        let expiry = self.schedule_expiry(generation);
        let extra_args: &[&str] = if request.provider == AiUsageProvider::Codex {
            &["--device-auth"]
        } else {
            &[]
        };
        let terminal_request = build_login_terminal_request(
            &self.context,
            request.provider,
            &request.account_root,
            extra_args,
            request.columns as u16,
            request.rows as u16,
        );
        let callbacks = create_callbacks(
            Arc::downgrade(&self.sessions),
            request.socket.clone(),
            generation,
        );
        let terminal = self
            .spawner
            .spawn(terminal_request, callbacks)
            .map_err(|_| {
                login_error(
                    500,
                    "로그인 터미널을 열지 못했습니다. PC의 CLI 설치를 확인해 주세요",
                )
            })?;
        registry.sessions.push(LoginSession {
            device_id: request.device_id,
            socket_id: request.socket_id,
            account_root: request.account_root,
            generation,
            socket: request.socket,
            terminal,
            _expiry: expiry,
        });
        Ok(())
    }

    fn schedule_expiry(&self, generation: u64) -> LoginTimerCancellation {
        let sessions = Arc::downgrade(&self.sessions);
        self.timers.schedule(
            LOGIN_TIMEOUT_MS,
            Box::new(move || {
                let Some(sessions) = sessions.upgrade() else {
                    return;
                };
                let mut registry = lock(&sessions);
                let Some(index) = registry.position_of_generation(generation) else {
                    return;
                };
                let session = registry.sessions.remove(index);
                drop(registry);
                kill_session_terminal(&session);
                if session.socket.is_open() {
                    session.socket.send_event(MobileAiLoginEvent::Error {
                        error: "로그인 시간이 만료되었습니다. 다시 열어 주세요".to_string(),
                    });
                }
            }),
        )
    }

    fn terminal(&self, device_id: &str, socket_id: &str) -> Option<Arc<dyn LoginTerminal>> {
        let registry = lock(&self.sessions);
        let index = registry.position(device_id, socket_id)?;
        Some(registry.sessions[index].terminal.clone())
    }

    pub fn write_mobile_ai_login(
        &self,
        device_id: &str,
        socket_id: &str,
        data: &str,
    ) -> Result<(), MobileAiLoginError> {
        if data.len() > MAX_INPUT_BYTES {
            return Err(login_error(400, "터미널 입력이 올바르지 않습니다"));
        }
        if let Some(terminal) = self.terminal(device_id, socket_id) {
            let _ = terminal.write(data);
        }
        Ok(())
    }

    pub fn resize_mobile_ai_login(
        &self,
        device_id: &str,
        socket_id: &str,
        columns: f64,
        rows: f64,
    ) -> Result<(), MobileAiLoginError> {
        validate_terminal_size(columns, rows)?;
        if let Some(terminal) = self.terminal(device_id, socket_id) {
            let _ = terminal.resize(columns as u16, rows as u16);
        }
        Ok(())
    }

    /// 조건에 맞는 세션을 모두 떼어 내 PTY를 죽이고, 떼어 낸 세션을 돌려준다
    fn finish_matching(&self, is_match: impl Fn(&LoginSession) -> bool) -> Vec<LoginSession> {
        let mut registry = lock(&self.sessions);
        let (finished, remaining): (Vec<LoginSession>, Vec<LoginSession>) = registry
            .sessions
            .drain(..)
            .partition(|session| is_match(session));
        registry.sessions = remaining;
        drop(registry);
        for session in &finished {
            kill_session_terminal(session);
        }
        finished
    }

    pub fn close_mobile_ai_login(&self, device_id: &str, socket_id: &str) {
        self.finish_matching(|session| {
            session.device_id == device_id && session.socket_id == socket_id
        });
    }

    pub fn close_mobile_ai_logins_for_device(&self, device_id: &str) {
        self.finish_matching(|session| session.device_id == device_id);
    }

    /// 계정을 지우기 전에 그 계정을 붙든 로그인 터미널을 모두 닫고 이유를 알린다
    pub fn close_mobile_ai_logins_for_account(&self, account_root: &str) {
        for session in self.finish_matching(|session| session.account_root == account_root) {
            if session.socket.is_open() {
                session.socket.send_event(MobileAiLoginEvent::Error {
                    error: "계정 삭제를 위해 로그인 터미널을 닫았습니다".to_string(),
                });
            }
        }
    }

    pub fn close_all_mobile_ai_logins(&self) {
        self.finish_matching(|_| true);
    }
}

#[cfg(test)]
mod tests;
