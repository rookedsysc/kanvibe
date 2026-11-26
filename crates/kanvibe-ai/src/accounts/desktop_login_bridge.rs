//! 데스크톱 창 안에서 provider 로그인 CLI를 보여 주는 PTY 세션(TS `aiAccountLoginBridge`).
//! 세션은 창과 계정 루트로 묶이고, 창이 사라지면 그 창이 띄운 로그인 프로세스도 남기지 않는다.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, Weak};

use serde::{Deserialize, Serialize};

use crate::accounts::login_terminal::{
    LoginTerminal, LoginTerminalCallbacks, LoginTerminalSpawner, build_login_terminal_request,
};
use crate::usage::context::{UsageContext, WarningSink};
use crate::usage::types::AiUsageProvider;

/// 로그인 화면을 그리는 창. TS의 `webContents`와 `kanvibe:ai-login-data`·`-exit` 알림에 해당한다
pub trait LoginWindow: Send + Sync {
    fn id(&self) -> u64;
    fn is_destroyed(&self) -> bool;
    fn send_output(&self, account_root: &str, data: &str);
    fn send_exit(&self, account_root: &str, exit_code: i32);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiAccountLoginOpenResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl AiAccountLoginOpenResult {
    fn opened() -> Self {
        Self {
            ok: true,
            error: None,
        }
    }

    fn refused(error: Option<String>) -> Self {
        Self { ok: false, error }
    }
}

struct LoginSession {
    generation: u64,
    terminal: Arc<dyn LoginTerminal>,
}

#[derive(Default)]
struct LoginSessions {
    by_key: BTreeMap<String, LoginSession>,
    next_generation: u64,
}

pub struct DesktopLoginBridge {
    context: UsageContext,
    spawner: Arc<dyn LoginTerminalSpawner>,
    sessions: Arc<Mutex<LoginSessions>>,
    warn: WarningSink,
}

fn build_session_key(window_id: u64, account_root: &str) -> String {
    format!("{window_id}:{account_root}")
}

/// 지금도 같은 세션이 그 자리를 차지하고 있는지. 닫히고 다시 열린 뒤 늦게 온 콜백을 걸러 낸다
fn is_current(sessions: &Mutex<LoginSessions>, key: &str, generation: u64) -> bool {
    let sessions = sessions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    sessions
        .by_key
        .get(key)
        .is_some_and(|session| session.generation == generation)
}

fn create_callbacks(
    sessions: Weak<Mutex<LoginSessions>>,
    window: Arc<dyn LoginWindow>,
    key: String,
    generation: u64,
    account_root: String,
) -> LoginTerminalCallbacks {
    let output_sessions = sessions.clone();
    let output_window = window.clone();
    let output_key = key.clone();
    let output_root = account_root.clone();
    LoginTerminalCallbacks {
        on_output: Box::new(move |data| {
            let Some(sessions) = output_sessions.upgrade() else {
                return;
            };
            if is_current(&sessions, &output_key, generation) && !output_window.is_destroyed() {
                output_window.send_output(&output_root, data);
            }
        }),
        on_exit: Box::new(move |exit_code| {
            let Some(sessions) = sessions.upgrade() else {
                return;
            };
            let mut registry = sessions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !registry
                .by_key
                .get(&key)
                .is_some_and(|session| session.generation == generation)
            {
                return;
            }
            registry.by_key.remove(&key);
            drop(registry);
            if !window.is_destroyed() {
                window.send_exit(&account_root, exit_code);
            }
        }),
    }
}

impl DesktopLoginBridge {
    pub fn new(
        context: UsageContext,
        spawner: Arc<dyn LoginTerminalSpawner>,
        warn: WarningSink,
    ) -> Self {
        Self {
            context,
            spawner,
            sessions: Arc::new(Mutex::new(LoginSessions::default())),
            warn,
        }
    }

    fn lock_sessions(&self) -> std::sync::MutexGuard<'_, LoginSessions> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn terminal(&self, window_id: u64, account_root: &str) -> Option<Arc<dyn LoginTerminal>> {
        self.lock_sessions()
            .by_key
            .get(&build_session_key(window_id, account_root))
            .map(|session| session.terminal.clone())
    }

    /// 같은 창·계정의 로그인이 이미 떠 있으면 그 세션을 그대로 쓴다. 등록 잠금을 쥔 채 띄우므로 겹친 호출도 PTY는 하나다
    pub fn open_ai_account_login(
        &self,
        window: Arc<dyn LoginWindow>,
        provider: AiUsageProvider,
        account_root: &str,
        columns: u16,
        rows: u16,
    ) -> AiAccountLoginOpenResult {
        let key = build_session_key(window.id(), account_root);
        let mut sessions = self.lock_sessions();
        if sessions.by_key.contains_key(&key) {
            return AiAccountLoginOpenResult::opened();
        }
        if window.is_destroyed() {
            return AiAccountLoginOpenResult::refused(None);
        }
        sessions.next_generation += 1;
        let generation = sessions.next_generation;
        let request =
            build_login_terminal_request(&self.context, provider, account_root, &[], columns, rows);
        let callbacks = create_callbacks(
            Arc::downgrade(&self.sessions),
            window,
            key.clone(),
            generation,
            account_root.to_string(),
        );
        match self.spawner.spawn(request, callbacks) {
            Ok(terminal) => {
                sessions.by_key.insert(
                    key,
                    LoginSession {
                        generation,
                        terminal,
                    },
                );
                AiAccountLoginOpenResult::opened()
            }
            Err(error) => AiAccountLoginOpenResult::refused(Some(error)),
        }
    }

    /// 네이티브 fd가 onExit보다 먼저 닫힐 수 있어, 늦게 온 입력이 앱을 멈추지 않도록 실패는 경고로만 남긴다
    pub fn write_ai_account_login(&self, window_id: u64, account_root: &str, data: &str) {
        if let Some(Err(error)) = self
            .terminal(window_id, account_root)
            .map(|terminal| terminal.write(data))
        {
            (self.warn)(&format!("[ai-login] PTY write failed: {error}"));
        }
    }

    pub fn resize_ai_account_login(
        &self,
        window_id: u64,
        account_root: &str,
        columns: u16,
        rows: u16,
    ) {
        if let Some(Err(error)) = self
            .terminal(window_id, account_root)
            .map(|terminal| terminal.resize(columns, rows))
        {
            (self.warn)(&format!("[ai-login] PTY resize failed: {error}"));
        }
    }

    pub fn close_ai_account_login(&self, window_id: u64, account_root: &str) {
        let removed = self
            .lock_sessions()
            .by_key
            .remove(&build_session_key(window_id, account_root));
        if let Some(Err(error)) = removed.map(|session| session.terminal.kill()) {
            (self.warn)(&format!("[ai-login] PTY close failed: {error}"));
        }
    }

    /// 창이 사라지면 그 창이 띄운 로그인 프로세스도 남겨 두지 않는다
    pub fn close_window_ai_account_logins(&self, window_id: u64) {
        let prefix = format!("{window_id}:");
        let account_roots: Vec<String> = self
            .lock_sessions()
            .by_key
            .keys()
            .filter_map(|key| key.strip_prefix(&prefix).map(str::to_string))
            .collect();
        for account_root in account_roots {
            self.close_ai_account_login(window_id, &account_root);
        }
    }
}

#[cfg(test)]
mod tests;
