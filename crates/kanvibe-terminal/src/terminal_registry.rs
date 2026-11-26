//! 활성 터미널 세션 레지스트리(TS `src/lib/terminal.ts`). 이 모듈이 PTY의 유일한 소유자이고 탭 조작도 여기 있는 PTY로만 한다.
//!
//! 계약(타입·메서드 시그니처)은 다른 모듈이 이미 이 모양으로 부르므로 바꾸지 않는다.
//!
//! 스레드 모델: 레지스트리 상태는 `Mutex` 하나에 있고, PTY 출력은 PTY 읽기 스레드가 항목의 출력 잠금 안에서
//! 화면과 클라이언트로 흘린다. 레지스트리 잠금을 쥔 채 PTY나 클라이언트의 `close`를 부르지 않는다.
//! 클라이언트의 `is_open`·`send`·`buffered_amount`는 잠금 안에서 불리므로 레지스트리를 다시 부르면 안 된다.

mod attach;
mod entry;
mod flight;
mod herdr_client;
mod herdr_server;
mod local_sessions;
mod preparation;
mod remote_commands;
mod spawn_local;
mod spawn_remote;
mod tabs;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, MutexGuard};

use kanvibe_domain::task::SessionType;
use kanvibe_domain::terminal_tabs::TerminalTab;
use kanvibe_process::exec::ShellCommandRunner;
use kanvibe_process::ssh_config::SshHostConfig;
use kanvibe_process::worktree::PaneLayoutSelection;

use crate::environment::TerminalHostEnvironment;
use crate::pty::PtySpawner;
use crate::screen::{ScreenReading, TerminalScreen};
use entry::{ActiveTerminals, TerminalEntry, lock};
use flight::Flight;
use herdr_client::HerdrTaskClient;
use preparation::build_session_preparation_key;
use tabs::LocalTerminalTabs;

/// 크기를 받지 못한 PTY가 쓰는 기본 크기
pub const DEFAULT_TERMINAL_COLUMNS: u16 = 120;
pub const DEFAULT_TERMINAL_ROWS: u16 = 30;

/// WebSocket 닫힘 코드와 사유
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientClosure {
    pub code: u16,
    pub reason: String,
}

/// PTY 출력을 받는 화면 하나(TS WebSocket). 데스크탑 뷰와 모바일 WebSocket이 구현한다
pub trait TerminalClient: Send + Sync {
    /// TS `ws.readyState === ws.OPEN`
    fn is_open(&self) -> bool;
    fn send(&self, data: &str);
    /// 아직 보내지 못한 바이트 수(TS `bufferedAmount`). 8 MiB를 넘으면 레지스트리가 1013으로 닫는다
    fn buffered_amount(&self) -> usize;
    /// `None`이면 사유 없이 닫는다(TS `ws.close()`)
    fn close(&self, closure: Option<ClientClosure>);
    /// 터미널 질의(DA·DSR·크기·색)에 답하는 화면인지. TS에서 답하던 쪽은 렌더러 xterm.js였으므로 데스크탑 뷰만 참이다.
    /// 참인 클라이언트가 붙어 있는 동안 레지스트리 화면이 응답을 PTY에 쓴다
    fn answers_terminal_queries(&self) -> bool {
        false
    }
}

/// 레지스트리에 붙은 클라이언트 하나. 클라이언트 쪽 메시지와 닫힘을 이 값으로 레지스트리에 알린다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalAttachment {
    pub terminal_key: String,
    /// 붙은 PTY 항목의 세대. 같은 키에 새 PTY가 생기면 옛 첨부의 닫힘은 무시한다
    pub entry_generation: u64,
    pub client_id: u64,
}

/// Herdr client가 탭 바를 그리는 위치
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HerdrTabBarPosition {
    Top,
    Bottom,
}

/// Herdr 연동 가운데 레지스트리가 쓰는 부분. `crate::herdr`의 서비스가 구현한다
pub trait HerdrSessions: Send + Sync {
    /// 공유 Space를 준비하고 workspace ID를 돌려준다(TS `ensureHerdrSession`)
    fn ensure_session(
        &self,
        session_name: &str,
        working_directory: &str,
        pane_layout: Option<&PaneLayoutSelection>,
        ssh_host: Option<&str>,
    ) -> Result<Option<String>, HerdrSessionError>;
    fn ensure_remote_machine(
        &self,
        session_name: &str,
        ssh_host: &str,
    ) -> Result<(), HerdrSessionError>;
    fn handoff_remote_server(
        &self,
        herdr_session: &str,
        ssh_host: &str,
    ) -> Result<(), HerdrSessionError>;
    fn client_socket_path(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<String, HerdrSessionError>;
    fn client_tab_bar_position(&self, ssh_host: Option<&str>) -> Option<HerdrTabBarPosition>;
    /// task Space의 pane과 그 pane이 있는 탭(TS `listHerdrTaskPaneTabIds`)
    fn list_task_pane_tab_ids(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<HerdrPaneTab>, HerdrSessionError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrPaneTab {
    pub pane_id: String,
    pub tab_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HerdrSessionError {
    /// 원격 서버가 이전 방식으로 떠 있어 사용자의 교체 승인이 필요하다(TS `HerdrServerHandoffRequiredError`)
    #[error("{message}")]
    ServerHandoffRequired { message: String },
    /// Herdr CLI 실패. `stderr`의 마지막 줄을 사용자에게 보여 준다
    #[error("{message}")]
    Command { message: String, stderr: String },
}

/// Herdr client와 서버 사이 중계(TS `createHerdrClientProxy` 결과). `crate::herdr_client_proxy`가 구현한다
pub trait HerdrClientRelay: Send + Sync {
    fn socket_path(&self) -> String;
    fn active_pane_id(&self) -> Option<String>;
    fn active_tab_id(&self) -> Option<String>;
    /// client가 마지막으로 알린 surface 크기(열, 행)
    fn surface_size(&self) -> Option<(u16, u16)>;
    fn request(&self, method: &str, params: serde_json::Value)
    -> Result<serde_json::Value, String>;
    fn close(&self) -> Result<(), String>;
}

pub trait HerdrClientRelayFactory: Send + Sync {
    fn create(
        &self,
        upstream_socket_path: &str,
        workspace_id: &str,
    ) -> Result<Arc<dyn HerdrClientRelay>, String>;
}

/// 원격 host daemon이 소유한 PTY 세션 하나(TS `listPtySessions` 항목)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteHostPtySession {
    /// `JSON.stringify([terminalKey, sessionType, sessionName])`
    pub key: String,
    pub session_id: String,
}

/// daemon이 열어 준 PTY. 창이 닫혀도 세션은 daemon에 남고, `close_session`만 세션을 끝낸다
pub trait RemoteHostPty: crate::pty::PtyProcess {
    fn session_id(&self) -> String;
    fn close_session(&self) -> Result<(), String>;
}

/// daemon이 로컬로 이어 준 Unix 소켓. 값을 버리거나 `close`하면 포워딩을 닫는다
pub trait RemoteHostSocketForward: Send + Sync {
    fn socket_path(&self) -> String;
    fn close(&self) -> Result<(), String>;
}

/// 원격 host daemon 연결(TS `HostConnection`). M4 원격 daemon이 구현하고, 없으면 순수 ssh 경로로 간다
pub trait RemoteHostConnection: Send + Sync {
    fn open_pty(
        &self,
        key: &str,
        command: &str,
        columns: u16,
        rows: u16,
        callbacks: crate::pty::PtyEventCallbacks,
    ) -> Result<Arc<dyn RemoteHostPty>, String>;
    fn list_pty_sessions(&self) -> Result<Vec<RemoteHostPtySession>, String>;
    fn close_pty_session(&self, session_id: &str) -> Result<(), String>;
    fn forward_socket(
        &self,
        remote_socket_path: &str,
    ) -> Result<Arc<dyn RemoteHostSocketForward>, String>;
}

pub trait RemoteHostConnections: Send + Sync {
    /// daemon을 쓸 수 없는 호스트(미설정·미지원)면 `Ok(None)`이다. 연결 준비에 실패하면 순수 ssh로 돌리지 않고 오류를 돌려준다(TS `getRemoteHostConnection`의 throw)
    fn connection(&self, ssh_host: &str) -> Result<Option<Arc<dyn RemoteHostConnection>>, String>;
}

/// 보드 알림 가운데 레지스트리가 내는 것(TS `boardNotifier`의 Herdr 알림)
pub trait TerminalEventSink: Send + Sync {
    fn herdr_server_handoff_required(&self, ssh_host: &str, herdr_session: &str);
    fn herdr_machine_registration_failed(&self, ssh_host: &str, herdr_session: &str, reason: &str);
}

/// 레지스트리가 쓰는 협력자. 앱이 시작할 때 한 번 만든다
pub struct TerminalRegistryDependencies {
    pub runner: Arc<dyn ShellCommandRunner>,
    pub environment: Arc<TerminalHostEnvironment>,
    pub pty_spawner: Arc<dyn PtySpawner>,
    pub herdr: Arc<dyn HerdrSessions>,
    pub herdr_relays: Arc<dyn HerdrClientRelayFactory>,
    pub remote_hosts: Arc<dyn RemoteHostConnections>,
    pub events: Arc<dyn TerminalEventSink>,
}

/// 로컬 세션 attach 요청
#[derive(Debug, Clone)]
pub struct LocalAttachRequest {
    pub task_id: String,
    pub tab_id: Option<String>,
    pub session_type: SessionType,
    pub session_name: String,
    pub working_directory: Option<String>,
    pub columns: Option<u16>,
    pub rows: Option<u16>,
    pub pane_layout: Option<PaneLayoutSelection>,
}

/// SSH 원격 세션 attach 요청
#[derive(Debug, Clone)]
pub struct RemoteAttachRequest {
    pub task_id: String,
    pub tab_id: Option<String>,
    pub ssh_host: String,
    pub ssh_config: SshHostConfig,
    pub session_type: SessionType,
    pub session_name: String,
    pub columns: Option<u16>,
    pub rows: Option<u16>,
    pub worktree_path: Option<String>,
    pub pane_layout: Option<PaneLayoutSelection>,
}

/// 화면 attach 없이 세션만 준비할 대상(TS `TerminalSessionPreparationTarget`)
#[derive(Debug, Clone)]
pub struct TerminalSessionPreparationTarget {
    pub task_id: String,
    pub session_type: SessionType,
    pub session_name: String,
    pub worktree_path: Option<String>,
    pub ssh_host: Option<String>,
    pub pane_layout: Option<PaneLayoutSelection>,
}

/// 화면 attach 없이 세션만 준비한 결과(TS `PreparedTerminalSession`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedTerminalSession {
    pub is_prepared: bool,
    /// 준비하지 않은 이유. terminal 타입처럼 미리 만들 세션이 없을 때만 있다
    pub skipped_reason: Option<String>,
    pub herdr_workspace_id: Option<String>,
    pub herdr_panes: Option<Vec<HerdrPaneTab>>,
}

/// Herdr pane을 실제 화면에 투영할 영역(열·행 단위)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HerdrSurfaceBounds {
    pub left: u16,
    pub top: u16,
    pub width: u16,
    pub height: u16,
}

/// task 기본 터미널이 붙어 있는 PTY와 세션(TS `readTaskTerminalAttachment`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTerminalAttachment {
    pub pid: u32,
    pub session_type: SessionType,
    pub session_name: String,
}

/// terminal 세션 탭의 지금 화면(TS `readLocalTerminalTabSnapshot`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTabSnapshot {
    pub pid: u32,
    pub reading: ScreenReading,
}

/// 진행 중인 PTY 생성의 결과. 기다리던 attach는 이것을 보고 같은 PTY에 붙거나 다시 시도한다
#[derive(Clone)]
enum CreationOutcome {
    Failed,
    /// 만들기가 끝났을 때 그 키의 항목. 클라이언트가 준비 중 닫혀 만들지 않았으면 `None`이다
    Settled(Option<Arc<TerminalEntry>>),
}

#[derive(Default)]
struct RegistryState {
    active_terminals: ActiveTerminals,
    local_terminal_tabs: LocalTerminalTabs,
    /// 같은 터미널로 두 요청이 겹치면 둘 다 PTY를 띄워 하나가 버려진다. 첫 요청이 만드는 동안 뒤 요청은 이것을 기다린다
    pending_creations: HashMap<String, Arc<Flight<CreationOutcome>>>,
    /// 같은 세션의 준비가 겹치면 tmux 재시도 경로의 kill-session이 먼저 만든 세션을 지운다. 진행 중인 하나를 함께 기다린다
    pending_preparations: HashMap<String, Arc<PreparationFlight>>,
    /// 원격 Herdr 서버 교체. 그 사이 다시 여는 화면은 교체가 끝난 뒤 새 서버에 붙는다
    herdr_server_handoffs: HashMap<String, Arc<Flight<()>>>,
}

pub struct TerminalRegistry {
    dependencies: TerminalRegistryDependencies,
    state: Mutex<RegistryState>,
    next_generation: AtomicU64,
    next_client_id: AtomicU64,
    /// TS `KANVIBE_DEBUG_TERMINAL=true`. 앱이 시작할 때 한 번 읽어 넘긴다
    is_debug_logging_enabled: bool,
}

/// 진행 중인 세션 준비(TS `prepareTerminalSession` 약속)
type PreparationFlight = Flight<Result<PreparedTerminalSession, String>>;

/// attach 결과. 준비 중 클라이언트가 닫히면 `Ok(None)`이다
type AttachResult = Result<Option<TerminalAttachment>, String>;

/// tmux와 zellij는 태스크당 PTY 하나를 멀티플렉서에 붙이고 탭은 멀티플렉서 안에 있다.
/// terminal 세션만 탭마다 PTY를 따로 가지므로 탭 식별자까지 키에 넣는다
fn build_terminal_key(task_id: &str, tab_id: Option<&str>) -> String {
    match tab_id.filter(|tab_id| !tab_id.is_empty()) {
        Some(tab_id) => format!("{task_id}#{tab_id}"),
        None => task_id.to_string(),
    }
}

fn closure(code: u16, reason: &str) -> Option<ClientClosure> {
    Some(ClientClosure {
        code,
        reason: reason.to_string(),
    })
}

impl TerminalRegistry {
    pub fn new(dependencies: TerminalRegistryDependencies) -> Arc<Self> {
        Self::with_debug_logging(dependencies, false)
    }

    /// 진단 로그를 켠다. 앱이 `KANVIBE_DEBUG_TERMINAL=true`를 한 번 읽어 넘긴다(라이브러리는 환경 변수를 읽지 않는다)
    pub fn with_debug_logging(
        dependencies: TerminalRegistryDependencies,
        is_debug_logging_enabled: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            dependencies,
            state: Mutex::new(RegistryState::default()),
            next_generation: AtomicU64::new(1),
            next_client_id: AtomicU64::new(1),
            is_debug_logging_enabled,
        })
    }

    fn lock_state(&self) -> MutexGuard<'_, RegistryState> {
        lock(&self.state)
    }

    fn default_tab_name(&self) -> String {
        self.dependencies.environment.default_tab_name()
    }

    fn active_entry(&self, terminal_key: &str) -> Option<Arc<TerminalEntry>> {
        self.lock_state()
            .active_terminals
            .get(terminal_key)
            .cloned()
    }

    /// 진단 로그. 켜져 있을 때만 남긴다
    fn debug_log(&self, message: &str, payload: serde_json::Value) {
        if self.is_debug_logging_enabled {
            eprintln!("[터미널-진단] {message} {payload}");
        }
    }

    pub fn list_local_terminal_tabs(&self, task_id: &str) -> Vec<TerminalTab> {
        let default_tab_name = self.default_tab_name();
        self.lock_state()
            .local_terminal_tabs
            .list(task_id, &default_tab_name)
    }

    /// 앱 메모리에서 사라진 원격 탭을 daemon의 현재 소유 세션 목록으로 복원한다
    pub fn list_remote_terminal_tabs(
        &self,
        task_id: &str,
        ssh_host: &str,
        session_name: &str,
    ) -> Result<Vec<TerminalTab>, String> {
        if self.lock_state().local_terminal_tabs.contains(task_id) {
            return Ok(self.list_local_terminal_tabs(task_id));
        }
        let Some(connection) = self.dependencies.remote_hosts.connection(ssh_host)? else {
            return Ok(self.list_local_terminal_tabs(task_id));
        };
        let sessions = connection.list_pty_sessions()?;
        let default_tab_name = self.default_tab_name();
        {
            let mut state = self.lock_state();
            let restored = tabs::restore_tabs_from_remote_sessions(
                task_id,
                session_name,
                &sessions,
                &default_tab_name,
            );
            if let Some(restored) =
                restored.filter(|_| !state.local_terminal_tabs.contains(task_id))
            {
                state.local_terminal_tabs.insert(task_id, restored);
            }
        }
        Ok(self.list_local_terminal_tabs(task_id))
    }

    /// 남은 탭 수를 돌려준다. 조회·닫기 실패를 성공으로 보고하지 않는다
    pub fn close_remote_terminal_tab(
        &self,
        task_id: &str,
        tab_id: &str,
        ssh_host: &str,
        session_name: &str,
    ) -> Result<usize, String> {
        self.list_remote_terminal_tabs(task_id, ssh_host, session_name)?;
        let terminal_key = build_terminal_key(task_id, Some(tab_id));
        let remote_pty = self
            .active_entry(&terminal_key)
            .and_then(|entry| entry.remote_pty.clone());
        if let Some(connection) = self.dependencies.remote_hosts.connection(ssh_host)? {
            match remote_pty {
                Some(remote_pty) => connection.close_pty_session(&remote_pty.session_id())?,
                None => close_unattached_remote_session(
                    connection.as_ref(),
                    &terminal_key,
                    session_name,
                )?,
            }
            self.destroy_terminal(&terminal_key, Some("remote-close-acknowledged"), false);
        }
        Ok(self.close_local_terminal_tab(task_id, tab_id))
    }

    pub fn local_terminal_tab_process(&self, task_id: &str, tab_id: &str) -> Option<String> {
        let entry = self.active_entry(&build_terminal_key(task_id, Some(tab_id)))?;
        entry
            .pty
            .foreground_process_name()
            .filter(|name| !name.is_empty())
    }

    /// 새 탭을 목록 끝에 추가하고 활성으로 만든다. PTY는 렌더러가 붙을 때 생긴다
    pub fn create_local_terminal_tab(&self, task_id: &str) -> TerminalTab {
        let default_tab_name = self.default_tab_name();
        self.lock_state()
            .local_terminal_tabs
            .create(task_id, &default_tab_name)
    }

    /// 탭을 닫고 남은 탭 수를 반환한다. 활성 탭을 닫으면 그 자리를 이어받는 탭이 활성이 된다
    pub fn close_local_terminal_tab(&self, task_id: &str, tab_id: &str) -> usize {
        let default_tab_name = self.default_tab_name();
        {
            let mut state = self.lock_state();
            let tab_state = state
                .local_terminal_tabs
                .state_mut(task_id, &default_tab_name);
            if !tab_state.tabs.iter().any(|tab| tab.id == tab_id) {
                return tab_state.tabs.len();
            }
        }
        self.destroy_terminal(
            &build_terminal_key(task_id, Some(tab_id)),
            Some("close-terminal-tab"),
            true,
        );
        self.lock_state()
            .local_terminal_tabs
            .remove_tab(task_id, tab_id)
            .unwrap_or(0)
    }

    pub fn rename_local_terminal_tab(&self, task_id: &str, tab_id: &str, name: &str) {
        let default_tab_name = self.default_tab_name();
        self.lock_state()
            .local_terminal_tabs
            .rename(task_id, tab_id, name, &default_tab_name);
    }

    pub fn select_local_terminal_tab(&self, task_id: &str, tab_id: &str) {
        let default_tab_name = self.default_tab_name();
        self.lock_state()
            .local_terminal_tabs
            .select(task_id, tab_id, &default_tab_name);
    }

    pub fn move_local_terminal_tab(&self, task_id: &str, tab_id: &str, target_index: usize) {
        let default_tab_name = self.default_tab_name();
        self.lock_state().local_terminal_tabs.move_tab(
            task_id,
            tab_id,
            target_index,
            &default_tab_name,
        );
    }

    /// 앱 종료 경로. 원격 PTY는 daemon 소유이므로 detach하고, 로컬 PTY만 종료한다
    pub fn kill_all_terminal_sessions(&self) {
        let terminal_keys = self.lock_state().active_terminals.keys();
        for terminal_key in terminal_keys {
            self.destroy_terminal(&terminal_key, Some("app-quit"), false);
        }
        self.lock_state().local_terminal_tabs.clear();
    }

    /// 태스크의 Herdr client가 붙은 항목들. 넣은 순서대로다
    fn herdr_client_entries(&self, task_id: &str) -> Vec<Arc<TerminalEntry>> {
        self.lock_state()
            .active_terminals
            .iter()
            .filter(|(_, entry)| entry.task_id == task_id && entry.herdr_client.is_some())
            .map(|(_, entry)| Arc::clone(entry))
            .collect()
    }

    fn first_herdr_client_entry(&self, task_id: &str) -> Option<Arc<TerminalEntry>> {
        self.herdr_client_entries(task_id).into_iter().next()
    }

    pub fn herdr_task_active_pane_id(&self, task_id: &str) -> Option<String> {
        let entry = self.first_herdr_client_entry(task_id)?;
        entry.herdr_client.as_ref()?.relay.active_pane_id()
    }

    /// Herdr pane을 실제 화면에 투영할 영역. 탭 바 설정이 바뀌었거나 크기가 맞지 않으면 `None`이다
    pub fn herdr_task_surface_bounds(&self, task_id: &str) -> Option<HerdrSurfaceBounds> {
        let entry = self.first_herdr_client_entry(task_id)?;
        let client = entry.herdr_client.as_ref()?;
        let tab_bar_position = client.tab_bar_position?;
        if self
            .dependencies
            .herdr
            .client_tab_bar_position(client.ssh_host.as_deref())
            != Some(tab_bar_position)
        {
            return None;
        }
        let (surface_columns, surface_rows) = client.relay.surface_size()?;
        let left = i32::from(entry.pty.columns()) - i32::from(surface_columns);
        let tab_rows = i32::from(entry.pty.rows()) - i32::from(surface_rows);
        if left < 0 || (tab_rows != 0 && tab_rows != HERDR_TAB_BAR_ROWS) {
            return None;
        }
        Some(HerdrSurfaceBounds {
            left: left as u16,
            top: if tab_bar_position == HerdrTabBarPosition::Top {
                tab_rows as u16
            } else {
                0
            },
            width: surface_columns,
            height: surface_rows,
        })
    }

    pub fn herdr_task_active_tab_id(&self, task_id: &str) -> Option<String> {
        let entry = self.first_herdr_client_entry(task_id)?;
        entry.herdr_client.as_ref()?.relay.active_tab_id()
    }

    pub fn focus_herdr_task_pane(&self, task_id: &str, pane_id: &str) -> Result<(), String> {
        let entries = self.herdr_client_entries(task_id);
        if entries.is_empty() {
            return Err("Herdr task 클라이언트가 연결되지 않았습니다.".to_string());
        }
        let params = serde_json::json!({ "pane_id": pane_id });
        request_herdr_focus(
            &entries,
            pane_id,
            "다른 Space의 pane을 선택할 수 없습니다.",
            "pane.focus",
            params,
        )
    }

    pub fn focus_herdr_task_tab(&self, task_id: &str, tab_id: &str) -> Result<(), String> {
        let entries = self.herdr_client_entries(task_id);
        let params = serde_json::json!({ "tab_id": tab_id });
        request_herdr_focus(
            &entries,
            tab_id,
            "다른 Space의 탭을 선택할 수 없습니다.",
            "tab.focus",
            params,
        )
    }

    /// 같은 터미널의 PTY가 있거나 만들어지는 중이면 공유한다. 준비 중 클라이언트가 닫히면 `Ok(None)`이다
    pub fn attach_local_session(
        self: &Arc<Self>,
        request: LocalAttachRequest,
        client: Arc<dyn TerminalClient>,
    ) -> Result<Option<TerminalAttachment>, String> {
        let terminal_key = build_terminal_key(&request.task_id, request.tab_id.as_deref());
        let preparation_key = build_session_preparation_key(None, &request.session_name);
        self.attach_or_spawn(&terminal_key, &preparation_key, &client, || {
            self.spawn_local_session(&request, &client)
        })
    }

    pub fn attach_remote_session(
        self: &Arc<Self>,
        request: RemoteAttachRequest,
        client: Arc<dyn TerminalClient>,
    ) -> Result<Option<TerminalAttachment>, String> {
        let terminal_key = build_terminal_key(&request.task_id, request.tab_id.as_deref());
        let preparation_key =
            build_session_preparation_key(Some(&request.ssh_host), &request.session_name);
        self.attach_or_spawn(&terminal_key, &preparation_key, &client, || {
            self.spawn_remote_session(&request, &client)
        })
    }

    /// 사용자가 승인한 원격 Herdr 서버 교체. 끝날 때까지 기다린다
    pub fn replace_herdr_remote_server(self: &Arc<Self>, ssh_host: &str, herdr_session: &str) {
        self.run_herdr_server_replacement(ssh_host, herdr_session);
    }

    /// task의 멀티플렉서 세션을 화면 attach와 같은 부트스트랩으로 만든다. 이미 있으면 그대로 둔다
    pub fn prepare_terminal_session(
        &self,
        target: TerminalSessionPreparationTarget,
    ) -> Result<PreparedTerminalSession, String> {
        self.prepare_terminal_session_once(&target)
    }

    /// 렌더러 입력 포커스는 뷰가 처리한다. 호스트 tmux 클라이언트 전환은 하지 않는다
    pub fn focus_session(&self, task_id: &str) {
        let _ = task_id;
    }

    /// 태스크에 딸린 터미널을 모두 종료한다. 탭이 여러 개인 terminal 세션도 함께 정리한다
    pub fn detach_session(&self, task_id: &str, trigger_label: Option<&str>) {
        let task_terminal_keys: Vec<String> = self
            .lock_state()
            .active_terminals
            .iter()
            .filter(|(_, entry)| entry.task_id == task_id)
            .map(|(terminal_key, _)| terminal_key.clone())
            .collect();
        if task_terminal_keys.is_empty() {
            self.debug_log(
                "detachSession 호출 (entry 없음)",
                serde_json::json!({ "taskId": task_id, "triggerLabel": trigger_label }),
            );
        }
        for terminal_key in task_terminal_keys {
            self.destroy_terminal(&terminal_key, trigger_label, false);
        }
        self.lock_state().local_terminal_tabs.remove_task(task_id);
    }

    pub fn active_terminal_count(&self) -> usize {
        self.lock_state().active_terminals.len()
    }

    /// 클라이언트가 보낸 메시지. `\x01{"type":"resize",...}`는 크기 변경이고 나머지는 PTY 입력이다
    pub fn client_message(&self, attachment: &TerminalAttachment, data: &str) {
        if let Some(entry) = self.attached_entry(attachment) {
            handle_terminal_message(&entry, data);
        }
    }

    /// 클라이언트가 닫혔다. 마지막 클라이언트가 떠나면 멀티플렉서 세션의 PTY만 끊는다
    pub fn client_closed(&self, attachment: &TerminalAttachment) {
        self.release_terminal_client(attachment, "ws-close");
    }

    /// 떠 있는 terminal 세션 탭에 붙이고 붙인 순간까지의 화면(ANSI)을 돌려준다. PTY가 없으면 `None`이다.
    /// 화면 직렬화와 클라이언트 등록을 출력과 같은 잠금 안에서 해, 받는 쪽은 돌려받은 화면 뒤에 이어지는 출력만 받는다.
    /// 스크롤백 전체 직렬화는 오래 걸릴 수 있어 레지스트리 잠금은 쥐지 않는다. 그 사이 항목이 바뀌었으면 등록을 되돌린다
    pub fn attach_client_to_terminal_tab(
        &self,
        task_id: &str,
        tab_id: &str,
        client: Arc<dyn TerminalClient>,
    ) -> Option<(TerminalAttachment, String)> {
        let terminal_key = build_terminal_key(task_id, Some(tab_id));
        let entry = self.active_entry(&terminal_key)?;
        let (screen_until_now, client_id) = {
            let mut sink = lock(&entry.output.sink);
            let screen_until_now = sink.screen.as_ref()?.serialize();
            if !client.is_open() {
                return None;
            }
            let client_id = sink.register(&client, || self.allocate_client_id());
            (screen_until_now, client_id)
        };
        let is_still_current = self
            .active_entry(&terminal_key)
            .is_some_and(|current| Arc::ptr_eq(&current, &entry));
        if !is_still_current {
            lock(&entry.output.sink)
                .clients
                .retain(|registered| registered.client_id != client_id);
            return None;
        }
        let attachment = TerminalAttachment {
            terminal_key,
            entry_generation: entry.generation(),
            client_id,
        };
        Some((attachment, screen_until_now))
    }

    /// 데스크탑 터미널 뷰가 그릴 화면을 읽는다. 출력 반영과 같은 잠금 안에서 읽으므로 반쯤 반영된 화면은 보이지 않는다.
    /// 화면이 없으면(모바일만 붙은 멀티플렉서 세션) `None`이다
    pub fn read_terminal_screen<R>(
        &self,
        task_id: &str,
        tab_id: Option<&str>,
        read: impl FnOnce(&TerminalScreen) -> R,
    ) -> Option<R> {
        let entry = self.active_entry(&build_terminal_key(task_id, tab_id))?;
        let sink = lock(&entry.output.sink);
        sink.screen.as_ref().map(read)
    }

    /// 화면의 보기 상태(스크롤백 위치, 셀 픽셀 크기)를 바꾸거나, 시간 제한이 지난 동기화 출력을 반영한다.
    /// 그 밖의 화면 내용은 바꾸지 않는다
    pub fn update_terminal_screen_view<R>(
        &self,
        task_id: &str,
        tab_id: Option<&str>,
        update: impl FnOnce(&mut TerminalScreen) -> R,
    ) -> Option<R> {
        let entry = self.active_entry(&build_terminal_key(task_id, tab_id))?;
        let mut sink = lock(&entry.output.sink);
        sink.screen.as_mut().map(update)
    }

    pub fn read_task_terminal_attachment(&self, task_id: &str) -> Option<TaskTerminalAttachment> {
        let entry = self.active_entry(&build_terminal_key(task_id, None))?;
        Some(TaskTerminalAttachment {
            pid: entry.pty.pid(),
            session_type: entry.session_type,
            session_name: entry.session_name.clone(),
        })
    }

    pub fn read_local_terminal_tab_snapshot(
        &self,
        task_id: &str,
        tab_id: &str,
    ) -> Option<TerminalTabSnapshot> {
        let entry = self.active_entry(&build_terminal_key(task_id, Some(tab_id)))?;
        if entry.session_type != SessionType::Terminal {
            return None;
        }
        let reading = lock(&entry.output.sink).screen.as_ref()?.read();
        Some(TerminalTabSnapshot {
            pid: entry.pty.pid(),
            reading,
        })
    }

    /// terminal 세션 탭에 입력을 그대로 넣는다. 클라이언트 메시지 경로를 지나지 않아 크기 변경 명령으로 읽히지 않는다
    pub fn write_terminal_tab_input(&self, task_id: &str, tab_id: &str, input: &str) -> bool {
        let Some(entry) = self.active_entry(&build_terminal_key(task_id, Some(tab_id))) else {
            return false;
        };
        let _ = entry.pty.write(input);
        true
    }

    /// PTY가 아직 없으면 새로 만들 때 쓸 기본 크기다
    pub fn terminal_tab_size(&self, task_id: &str, tab_id: &str) -> (u16, u16) {
        match self.active_entry(&build_terminal_key(task_id, Some(tab_id))) {
            Some(entry) => (entry.pty.columns(), entry.pty.rows()),
            None => (DEFAULT_TERMINAL_COLUMNS, DEFAULT_TERMINAL_ROWS),
        }
    }

    pub fn resize_terminal_tab(
        &self,
        task_id: &str,
        tab_id: &str,
        columns: u16,
        rows: u16,
    ) -> Result<(), String> {
        let entry = self
            .active_entry(&build_terminal_key(task_id, Some(tab_id)))
            .filter(|entry| entry.session_type == SessionType::Terminal)
            .ok_or_else(|| "터미널 세션이 실행 중이지 않습니다.".to_string())?;
        claim_mobile_viewport(&entry, columns, rows)
    }

    pub fn release_terminal_tab_viewport(
        &self,
        task_id: &str,
        tab_id: &str,
        columns: u16,
        rows: u16,
    ) {
        if let Some(entry) = self.active_entry(&build_terminal_key(task_id, Some(tab_id))) {
            release_mobile_viewport(&entry, columns, rows);
        }
    }

    fn active_zellij_task_entry(&self, task_id: &str) -> Option<Arc<TerminalEntry>> {
        self.active_entry(&build_terminal_key(task_id, None))
            .filter(|entry| entry.session_type == SessionType::Zellij)
    }

    pub fn task_terminal_size(&self, task_id: &str) -> Option<(u16, u16)> {
        let entry = self.active_zellij_task_entry(task_id)?;
        Some((entry.pty.columns(), entry.pty.rows()))
    }

    pub fn resize_task_terminal_viewport(
        &self,
        task_id: &str,
        columns: u16,
        rows: u16,
    ) -> Result<(), String> {
        let entry = self
            .active_zellij_task_entry(task_id)
            .ok_or_else(|| ZELLIJ_NOT_RUNNING_MESSAGE.to_string())?;
        claim_mobile_viewport(&entry, columns, rows)
    }

    /// 데스크탑에 붙은 Zellij 클라이언트에 입력을 넣는다. 모바일 휠을 Zellij가 실제 마우스 입력으로 받게 할 때 쓴다
    pub fn write_task_terminal_input(&self, task_id: &str, input: &str) -> Result<(), String> {
        let entry = self
            .active_zellij_task_entry(task_id)
            .ok_or_else(|| ZELLIJ_NOT_RUNNING_MESSAGE.to_string())?;
        entry.pty.write(input)
    }

    pub fn release_task_terminal_viewport(&self, task_id: &str, columns: u16, rows: u16) {
        if let Some(entry) = self.active_zellij_task_entry(task_id) {
            release_mobile_viewport(&entry, columns, rows);
        }
    }
}

const ZELLIJ_NOT_RUNNING_MESSAGE: &str = "Zellij 터미널 세션이 실행 중이지 않습니다.";
const HERDR_TAB_BAR_ROWS: i32 = 1;

/// attach가 끊긴 탭도 daemon이 소유한 세션을 키로 찾아 닫는다
fn close_unattached_remote_session(
    connection: &dyn RemoteHostConnection,
    terminal_key: &str,
    session_name: &str,
) -> Result<(), String> {
    let pty_key = build_remote_pty_key(terminal_key, SessionType::Terminal, session_name);
    let sessions = connection.list_pty_sessions()?;
    match sessions.iter().find(|session| session.key == pty_key) {
        Some(session) => connection.close_pty_session(&session.session_id),
        None => Ok(()),
    }
}

/// daemon PTY 세션 키. TS `JSON.stringify([terminalKey, sessionType, sessionName])`
fn build_remote_pty_key(
    terminal_key: &str,
    session_type: SessionType,
    session_name: &str,
) -> String {
    serde_json::json!([terminal_key, session_type.as_str(), session_name]).to_string()
}

/// 모든 Herdr client가 같은 Space의 대상만 받도록 먼저 확인하고, 확인이 끝나면 모두에게 보낸다(TS `Promise.all`)
fn request_herdr_focus(
    entries: &[Arc<TerminalEntry>],
    target_id: &str,
    other_space_message: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<(), String> {
    let clients: Vec<&HerdrTaskClient> = entries
        .iter()
        .filter_map(|entry| entry.herdr_client.as_ref())
        .collect();
    let is_other_space = clients
        .iter()
        .any(|client| !target_id.starts_with(&format!("{}:", client.workspace_id)));
    if is_other_space {
        return Err(other_space_message.to_string());
    }
    let results: Vec<Result<serde_json::Value, String>> = clients
        .iter()
        .map(|client| client.relay.request(method, params.clone()))
        .collect();
    results
        .into_iter()
        .try_for_each(|result| result.map(|_| ()))
}

/// 크기 변경 메시지는 데스크탑 크기를 기억하고, 모바일이 화면을 쥐고 있지 않을 때만 PTY에 반영한다.
/// `\x01` 뒤가 JSON이 아니면(또는 JS에서 속성을 읽다 실패하는 `null`이면) 그대로 PTY 입력이다
fn handle_terminal_message(entry: &TerminalEntry, data: &str) {
    let Some(payload) = data.strip_prefix('\u{1}') else {
        let _ = entry.pty.write(data);
        return;
    };
    let parsed = kanvibe_ai::javascript::parse_json(payload).filter(|parsed| !parsed.is_null());
    let Some(parsed) = parsed else {
        let _ = entry.pty.write(data);
        return;
    };
    let Some((columns, rows)) = read_resize_message(&parsed) else {
        return;
    };
    let should_resize = {
        let mut viewport = lock(&entry.viewport);
        viewport.desktop_size = Some((columns, rows));
        viewport.mobile_size.is_none()
    };
    if should_resize {
        let _ = entry.resize(columns, rows);
    }
}

/// `{ type: "resize", cols, rows }`이고 크기가 둘 다 0이 아닌 숫자일 때의 크기
fn read_resize_message(parsed: &serde_json::Value) -> Option<(u16, u16)> {
    if parsed.get("type").and_then(serde_json::Value::as_str) != Some("resize") {
        return None;
    }
    let read_size = |key: &str| {
        let size = parsed.get(key)?.as_f64()?;
        (size != 0.0 && !size.is_nan()).then_some(size as u16)
    };
    Some((read_size("cols")?, read_size("rows")?))
}

/// 모바일이 화면 크기를 쥔다. 데스크탑 크기 변경은 기억만 하고 반영하지 않는다
fn claim_mobile_viewport(entry: &TerminalEntry, columns: u16, rows: u16) -> Result<(), String> {
    lock(&entry.viewport).mobile_size = Some((columns, rows));
    entry.resize(columns, rows)
}

/// 모바일이 떠나면 마지막 데스크탑 크기로, 그동안 데스크탑 변경이 없었으면 받은 크기로 돌린다
fn release_mobile_viewport(entry: &TerminalEntry, columns: u16, rows: u16) {
    let (desktop_columns, desktop_rows) = {
        let mut viewport = lock(&entry.viewport);
        viewport.mobile_size = None;
        viewport.desktop_size.unwrap_or((columns, rows))
    };
    let _ = entry.resize(desktop_columns, desktop_rows);
}
