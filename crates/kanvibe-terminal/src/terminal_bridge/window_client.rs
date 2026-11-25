//! 데스크탑 창 하나의 터미널 화면 클라이언트(TS `ElectronTerminalClient`). Electron IPC 대신 [`TerminalWindow`]를 직접 부른다(AD-8).

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, Weak};

use super::ports::TerminalSessions;
use super::{TerminalOpenResult, TerminalWindow};
use crate::terminal_registry::{ClientClosure, TerminalAttachment, TerminalClient};

pub(super) type WindowClients = Mutex<HashMap<String, Arc<WindowTerminalClient>>>;

struct WindowClientState {
    is_open: bool,
    /// 같은 (창, task, 탭)으로 열기가 겹친 횟수. React StrictMode처럼 mount → cleanup → remount가 겹쳐도
    /// 첫 mount의 정리가 두 번째 mount가 쓰는 PTY를 끊지 않게 한다
    reference_count: u32,
    attachment: Option<TerminalAttachment>,
}

pub(super) struct WindowTerminalClient {
    window: Arc<dyn TerminalWindow>,
    task_id: String,
    tab_id: Option<String>,
    client_key: String,
    sessions: Arc<dyn TerminalSessions>,
    window_clients: Weak<WindowClients>,
    state: Mutex<WindowClientState>,
    open_result: Mutex<Option<TerminalOpenResult>>,
    open_finished: Condvar,
}

impl WindowTerminalClient {
    pub(super) fn new(
        window: Arc<dyn TerminalWindow>,
        task_id: &str,
        tab_id: Option<&str>,
        client_key: String,
        sessions: Arc<dyn TerminalSessions>,
        window_clients: Weak<WindowClients>,
    ) -> Arc<Self> {
        Arc::new(Self {
            window,
            task_id: task_id.to_string(),
            tab_id: tab_id.map(str::to_string),
            client_key,
            sessions,
            window_clients,
            state: Mutex::new(WindowClientState {
                is_open: true,
                reference_count: 1,
                attachment: None,
            }),
            open_result: Mutex::new(None),
            open_finished: Condvar::new(),
        })
    }

    pub(super) fn add_reference(&self) {
        self.state.lock().unwrap().reference_count += 1;
    }

    /// 참조를 하나 놓는다. 남은 참조가 없으면 `true`다
    pub(super) fn release_reference(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        state.reference_count = state.reference_count.saturating_sub(1);
        state.reference_count == 0
    }

    pub(super) fn drop_all_references(&self) {
        self.state.lock().unwrap().reference_count = 0;
    }

    /// 창이 닫혔거나 클라이언트가 닫혔다
    pub(super) fn is_cancelled(&self) -> bool {
        !self.is_open() || self.window.is_destroyed()
    }

    pub(super) fn finish_open(&self, result: TerminalOpenResult) {
        *self.open_result.lock().unwrap() = Some(result);
        self.open_finished.notify_all();
    }

    /// 먼저 시작한 열기의 결과를 함께 쓴다
    pub(super) fn wait_open_result(&self) -> TerminalOpenResult {
        let open_result = self.open_result.lock().unwrap();
        let open_result = self
            .open_finished
            .wait_while(open_result, |result| result.is_none())
            .unwrap();
        open_result.clone().expect("열기가 끝날 때까지 기다렸다")
    }

    /// 레지스트리 첨부를 기억한다. 그 사이 닫혔으면 레지스트리에 바로 닫힘을 알린다
    pub(super) fn bind_attachment(&self, attachment: TerminalAttachment) {
        {
            let mut state = self.state.lock().unwrap();
            if state.is_open {
                state.attachment = Some(attachment);
                return;
            }
        }
        self.sessions.client_closed(&attachment);
    }

    /// 화면 입력이나 크기 변경을 레지스트리에 넘긴다. 아직 붙지 않았거나 닫혔으면 버린다
    pub(super) fn emit_message(&self, message: &str) {
        let attachment = {
            let state = self.state.lock().unwrap();
            if !state.is_open {
                return;
            }
            state.attachment.clone()
        };
        if let Some(attachment) = attachment {
            self.sessions.client_message(&attachment, message);
        }
    }

    fn forget_from_window_clients(&self) {
        let Some(window_clients) = self.window_clients.upgrade() else {
            return;
        };
        let mut window_clients = window_clients.lock().unwrap();
        let is_registered_client = window_clients
            .get(&self.client_key)
            .is_some_and(|registered| std::ptr::eq(Arc::as_ptr(registered), self));
        if is_registered_client {
            window_clients.remove(&self.client_key);
        }
    }
}

impl TerminalClient for WindowTerminalClient {
    fn is_open(&self) -> bool {
        self.state.lock().unwrap().is_open
    }

    fn send(&self, output: &str) {
        if !self.is_open() || self.window.is_destroyed() {
            return;
        }
        self.window
            .send_terminal_output(&self.task_id, self.tab_id.as_deref(), output);
    }

    /// 창에는 쌓아 두는 송신 버퍼가 없다
    fn buffered_amount(&self) -> usize {
        0
    }

    /// 데스크탑 창은 TS 렌더러 xterm.js 자리라 터미널 질의에 답한다
    fn answers_terminal_queries(&self) -> bool {
        true
    }

    fn close(&self, closure: Option<ClientClosure>) {
        let attachment = {
            let mut state = self.state.lock().unwrap();
            if !state.is_open {
                return;
            }
            state.is_open = false;
            state.attachment.take()
        };
        self.forget_from_window_clients();
        if let Some(attachment) = attachment {
            self.sessions.client_closed(&attachment);
        }
        if !self.window.is_destroyed() {
            let reason = closure.as_ref().map(|closure| closure.reason.as_str());
            self.window
                .send_terminal_closed(&self.task_id, self.tab_id.as_deref(), reason);
        }
    }
}
