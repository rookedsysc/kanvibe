//! 데스크탑 창·모바일과 터미널 세션 레지스트리 사이의 브리지(TS `desktop/main/terminalBridge.ts`).
//!
//! Electron IPC와 WebContents 대신 GPUI 뷰가 이 서비스를 같은 프로세스에서 부른다(AD-8). 창 하나는 [`TerminalWindow`]로 받는다.
//! 데스크탑 화면과 모바일이 같은 [`TerminalBridge::attach_task_terminal`]을 지나야 pane 레이아웃, 원격 의존성 확인,
//! hook 확인 순서가 한쪽에서만 달라지지 않는다.

mod callback_client;
mod ports;
#[cfg(test)]
mod tests;
mod window_client;

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use kanvibe_ai::javascript::js_trim;
use kanvibe_domain::task::{KanbanTask, SessionType};
use kanvibe_process::exec::ExecGitError;
use kanvibe_process::hooks::client_runtime::HookInstallError;
use kanvibe_process::ssh_config::{SshConfigReader, SshHostConfig};
use kanvibe_process::worktree::PaneLayoutSelection;

pub use self::callback_client::CallbackTerminalClient;
pub use self::ports::{
    RemoteConnectionPhases, RemoteImageTransfer, RemoteSessionDependencyCheck,
    SshTransportCooldowns, TaskHookInstallFailure, TerminalBridgeEvents, TerminalHookCheckFailure,
    TerminalHookVerifier, TerminalSessions, TerminalTaskStore,
};
use self::window_client::{WindowClients, WindowTerminalClient};
use crate::terminal_registry::{
    ClientClosure, LocalAttachRequest, PreparedTerminalSession, RemoteAttachRequest,
    TerminalAttachment, TerminalClient, TerminalSessionPreparationTarget,
};

const TERMINAL_CONNECTION_FAILED: &str = "터미널 연결 실패";
const TASK_WITHOUT_SESSION: &str = "작업에 연결된 세션이 없습니다.";
const NOT_REMOTE_SESSION: &str = "원격 세션이 아닙니다.";
const POLICY_VIOLATION_CLOSE_CODE: u16 = 1008;
const INTERNAL_ERROR_CLOSE_CODE: u16 = 1011;

/// 브리지 실패. SSH 전송 쿨다운이면 다시 시도할 수 있는 시각을 함께 싣는다
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct TerminalBridgeError {
    pub message: String,
    pub retry_after_ms: Option<NonZeroU32>,
}

impl TerminalBridgeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retry_after_ms: None,
        }
    }
}

impl From<String> for TerminalBridgeError {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<ExecGitError> for TerminalBridgeError {
    fn from(error: ExecGitError) -> Self {
        Self {
            retry_after_ms: error.retry_after_ms,
            message: error.message,
        }
    }
}

impl From<HookInstallError> for TerminalBridgeError {
    fn from(error: HookInstallError) -> Self {
        let retry_after_ms = match &error {
            HookInstallError::Exec(exec_error) => exec_error.retry_after_ms,
            HookInstallError::Files(_) | HookInstallError::Message(_) => None,
        };
        Self {
            message: error.to_string(),
            retry_after_ms,
        }
    }
}

/// TS `{ ok, error?, retryAfterMs? }`
pub type TerminalOpenResult = Result<(), TerminalBridgeError>;

/// 터미널을 그리는 데스크탑 창(TS `WebContents`). GPUI 창 뷰가 구현한다
pub trait TerminalWindow: Send + Sync {
    fn window_id(&self) -> u64;
    fn is_destroyed(&self) -> bool;
    /// TS `kanvibe:terminal-data`
    fn send_terminal_output(&self, task_id: &str, tab_id: Option<&str>, output: &str);
    /// TS `kanvibe:terminal-close`
    fn send_terminal_closed(&self, task_id: &str, tab_id: Option<&str>, reason: Option<&str>);
}

/// 브리지가 쓰는 협력자. 앱이 시작할 때 한 번 만든다
pub struct TerminalBridgeDependencies {
    pub sessions: Arc<dyn TerminalSessions>,
    pub store: Arc<dyn TerminalTaskStore>,
    pub ssh_hosts: Arc<SshConfigReader>,
    pub session_dependencies: Arc<dyn RemoteSessionDependencyCheck>,
    pub transport_cooldowns: Arc<dyn SshTransportCooldowns>,
    pub hooks: Arc<dyn TerminalHookVerifier>,
    pub image_transfer: Arc<dyn RemoteImageTransfer>,
    pub events: Arc<dyn TerminalBridgeEvents>,
    /// 원격 연결 단계 시간을 재는 시계
    pub clock: Arc<dyn Fn() -> Instant + Send + Sync>,
}

pub struct TerminalBridge {
    dependencies: Arc<TerminalBridgeDependencies>,
    window_clients: Arc<WindowClients>,
}

/// attach할 task 세션과 화면 크기
struct TaskAttachTarget<'a> {
    task: &'a KanbanTask,
    tab_id: Option<&'a str>,
    session_type: SessionType,
    session_name: &'a str,
    columns: Option<u16>,
    rows: Option<u16>,
    pane_layout: Option<PaneLayoutSelection>,
}

impl TerminalBridge {
    pub fn new(dependencies: TerminalBridgeDependencies) -> Self {
        Self {
            dependencies: Arc::new(dependencies),
            window_clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 클립보드 이미지를 원격 세션에 scp로 보내고 원격 경로를 돌려준다
    pub fn paste_image_to_remote_terminal(
        &self,
        task_id: &str,
        image_data_url: &str,
    ) -> Result<String, String> {
        let ssh_config = self
            .resolve_remote_ssh_config(task_id)
            .map_err(|error| error.message)??;
        let image_transfer = &self.dependencies.image_transfer;
        let image_bytes = image_transfer.decode_data_url(image_data_url)?;
        image_transfer.transfer_image(&ssh_config, &image_bytes)
    }

    /// 태스크의 세션에 클라이언트를 붙인다. 세션이 없으면 데스크탑이 처음 열 때와 똑같이 만든다.
    /// 준비 중 클라이언트가 닫히면 `Ok(None)`이다. 원격 접속 정보를 찾지 못하면 클라이언트를 닫고 실패한다
    pub fn attach_task_terminal(
        &self,
        task: &KanbanTask,
        tab_id: Option<&str>,
        client: Arc<dyn TerminalClient>,
        columns: Option<u16>,
        rows: Option<u16>,
    ) -> Result<Option<TerminalAttachment>, TerminalBridgeError> {
        let (session_type, session_name) = task_session(task)?;
        let target = TaskAttachTarget {
            task,
            tab_id,
            session_type,
            session_name,
            columns,
            rows,
            pane_layout: self.load_pane_layout(task, session_type)?,
        };
        if !client.is_open() {
            return Ok(None);
        }
        match non_empty(task.ssh_host.as_deref()) {
            Some(ssh_host) => self.attach_remote_task_terminal(target, ssh_host, client),
            None => self.attach_local_task_terminal(target, client),
        }
    }

    /// 화면 attach 없이 태스크의 세션만 만든다. 이미 있으면 그대로 둔다.
    /// attach와 같은 pane 레이아웃, 원격 의존성 확인, 초기 pane 명령 전 hook 확인을 거쳐야 화면이 나중에 같은 세션을 연다
    pub fn prepare_task_terminal(
        &self,
        task: &KanbanTask,
    ) -> Result<PreparedTerminalSession, TerminalBridgeError> {
        let (session_type, session_name) = task_session(task)?;
        let pane_layout = self.load_pane_layout(task, session_type)?;
        let ssh_host = non_empty(task.ssh_host.as_deref());
        if let Some(ssh_host) = ssh_host.filter(|_| session_type != SessionType::Terminal) {
            let dependencies = &self.dependencies;
            dependencies
                .transport_cooldowns
                .clear_transport_cooldown(ssh_host);
            dependencies
                .session_dependencies
                .ensure_remote_session_dependency(session_type, ssh_host)?;
            if has_initial_pane_commands(pane_layout.as_ref()) {
                ensure_terminal_hooks(dependencies, task)?;
            }
        }
        let target = TerminalSessionPreparationTarget {
            task_id: task.id.clone(),
            session_type,
            session_name: session_name.to_string(),
            worktree_path: task.worktree_path.clone(),
            ssh_host: task.ssh_host.clone(),
            pane_layout,
        };
        Ok(self
            .dependencies
            .sessions
            .prepare_terminal_session(target)?)
    }

    /// 창의 터미널을 연다. 같은 (창, task, 탭)의 열기가 진행 중이면 그 결과를 함께 쓰고 크기만 다시 알린다
    pub fn open_terminal(
        &self,
        window: Arc<dyn TerminalWindow>,
        task_id: &str,
        tab_id: Option<&str>,
        columns: u16,
        rows: u16,
    ) -> TerminalOpenResult {
        let client_key = build_client_key(window.window_id(), task_id, tab_id);
        let (client, is_existing) = {
            let mut window_clients = self.window_clients.lock().unwrap();
            match window_clients.get(&client_key) {
                Some(existing) => {
                    existing.add_reference();
                    (existing.clone(), true)
                }
                None => {
                    let client = WindowTerminalClient::new(
                        window,
                        task_id,
                        tab_id,
                        client_key.clone(),
                        self.dependencies.sessions.clone(),
                        Arc::downgrade(&self.window_clients),
                    );
                    window_clients.insert(client_key, client.clone());
                    (client, false)
                }
            }
        };
        if is_existing {
            let result = client.wait_open_result();
            client.emit_message(&build_resize_message(columns, rows));
            return result;
        }
        let result = self.initialize_terminal(&client, task_id, tab_id, columns, rows);
        client.finish_open(result.clone());
        result
    }

    pub fn write_terminal(&self, window_id: u64, task_id: &str, tab_id: Option<&str>, input: &str) {
        if let Some(client) = self.find_window_client(window_id, task_id, tab_id) {
            client.emit_message(input);
        }
    }

    pub fn resize_terminal(
        &self,
        window_id: u64,
        task_id: &str,
        tab_id: Option<&str>,
        columns: u16,
        rows: u16,
    ) {
        if let Some(client) = self.find_window_client(window_id, task_id, tab_id) {
            client.emit_message(&build_resize_message(columns, rows));
        }
    }

    pub fn focus_terminal(&self, task_id: &str) {
        self.dependencies.sessions.focus_session(task_id);
    }

    /// 참조를 하나 놓고, 남은 참조가 없을 때만 닫는다
    pub fn close_terminal(&self, window_id: u64, task_id: &str, tab_id: Option<&str>) {
        let Some(client) = self.find_window_client(window_id, task_id, tab_id) else {
            return;
        };
        if client.release_reference() {
            client.close(None);
        }
    }

    /// 창 자체가 사라지는 경로라 참조 수와 무관하게 그 창의 클라이언트를 모두 닫는다
    pub fn close_window_terminals(&self, window_id: u64) {
        let window_prefix = format!("{window_id}:");
        let window_clients: Vec<Arc<WindowTerminalClient>> = self
            .window_clients
            .lock()
            .unwrap()
            .iter()
            .filter(|(client_key, _)| client_key.starts_with(&window_prefix))
            .map(|(_, client)| client.clone())
            .collect();
        for client in window_clients {
            client.drop_all_references();
            client.close(None);
        }
    }

    fn find_window_client(
        &self,
        window_id: u64,
        task_id: &str,
        tab_id: Option<&str>,
    ) -> Option<Arc<WindowTerminalClient>> {
        let client_key = build_client_key(window_id, task_id, tab_id);
        self.window_clients
            .lock()
            .unwrap()
            .get(&client_key)
            .cloned()
    }

    fn initialize_terminal(
        &self,
        client: &Arc<WindowTerminalClient>,
        task_id: &str,
        tab_id: Option<&str>,
        columns: u16,
        rows: u16,
    ) -> TerminalOpenResult {
        let attached = self.attach_window_client(client, task_id, tab_id, columns, rows);
        attached.unwrap_or_else(|error| {
            client.close(Some(ClientClosure {
                code: INTERNAL_ERROR_CLOSE_CODE,
                reason: TERMINAL_CONNECTION_FAILED.to_string(),
            }));
            Err(error)
        })
    }

    /// 바깥 `Err`는 예외로 끝난 준비이고, 안쪽 결과는 취소나 세션 없음처럼 정해진 실패다
    fn attach_window_client(
        &self,
        client: &Arc<WindowTerminalClient>,
        task_id: &str,
        tab_id: Option<&str>,
        columns: u16,
        rows: u16,
    ) -> Result<TerminalOpenResult, TerminalBridgeError> {
        if client.is_cancelled() {
            return Ok(cancel_open(client));
        }
        let task = self.dependencies.store.find_task(task_id)?;
        if client.is_cancelled() {
            return Ok(cancel_open(client));
        }
        let Some(task) = task.filter(|task| task_session(task).is_ok()) else {
            client.close(None);
            return Ok(Err(TerminalBridgeError::new(TASK_WITHOUT_SESSION)));
        };
        let session_client: Arc<dyn TerminalClient> = client.clone();
        let attachment =
            self.attach_task_terminal(&task, tab_id, session_client, Some(columns), Some(rows))?;
        if let Some(attachment) = attachment {
            client.bind_attachment(attachment);
        }
        if client.is_cancelled() {
            return Ok(cancel_open(client));
        }
        Ok(Ok(()))
    }

    fn load_pane_layout(
        &self,
        task: &KanbanTask,
        session_type: SessionType,
    ) -> Result<Option<PaneLayoutSelection>, TerminalBridgeError> {
        if !matches!(session_type, SessionType::Tmux | SessionType::Herdr) {
            return Ok(None);
        }
        self.dependencies
            .store
            .effective_pane_layout(task.project_id.as_deref())
            .map_err(TerminalBridgeError::from)
    }

    fn attach_local_task_terminal(
        &self,
        target: TaskAttachTarget<'_>,
        client: Arc<dyn TerminalClient>,
    ) -> Result<Option<TerminalAttachment>, TerminalBridgeError> {
        let request = LocalAttachRequest {
            task_id: target.task.id.clone(),
            tab_id: target.tab_id.map(str::to_string),
            session_type: target.session_type,
            session_name: target.session_name.to_string(),
            working_directory: target.task.worktree_path.clone(),
            columns: target.columns,
            rows: target.rows,
            pane_layout: target.pane_layout,
        };
        let attachment = self
            .dependencies
            .sessions
            .attach_local_session(request, client.clone())?;
        if client.is_open() {
            // 상태 hook은 task 토큰이 있어야 받아지므로, 토큰을 보내지 않는 이전 버전 hook을 로컬 task도 접속할 때 갱신한다
            self.check_hooks_in_background(target.task);
        }
        Ok(attachment)
    }

    fn attach_remote_task_terminal(
        &self,
        target: TaskAttachTarget<'_>,
        ssh_host: &str,
        client: Arc<dyn TerminalClient>,
    ) -> Result<Option<TerminalAttachment>, TerminalBridgeError> {
        let dependencies = &self.dependencies;
        let connection_started_at = (dependencies.clock)();
        let dependency_check_ms = self.check_remote_dependency(target.session_type, ssh_host)?;
        let Some(ssh_config) = self.resolve_attach_ssh_config(&target.task.id, &client)? else {
            return Ok(None);
        };
        // 초기 pane 명령은 hook을 필요로 할 수 있으므로 자동 실행 순서를 유지한다
        let is_hook_required_before_attach = has_initial_pane_commands(target.pane_layout.as_ref());
        let hooks_before_attach_ms =
            is_hook_required_before_attach.then(|| self.check_hooks_before_attach(target.task));
        if hooks_before_attach_ms.is_some() && !client.is_open() {
            return Ok(None);
        }
        let task = target.task;
        let session_type = target.session_type;
        let request = build_remote_attach_request(target, ssh_host, ssh_config);
        let (attachment, session_attach_ms) = self.measure_phase(|| {
            dependencies
                .sessions
                .attach_remote_session(request, client.clone())
        });
        let attachment = attachment?;
        dependencies
            .events
            .remote_connection_phases(&RemoteConnectionPhases {
                task_id: task.id.clone(),
                ssh_host: ssh_host.to_string(),
                session_type,
                total_ms: elapsed_ms_since(connection_started_at, (dependencies.clock)()),
                dependency_check_ms,
                hooks_before_attach_ms,
                session_attach_ms,
            });
        if !is_hook_required_before_attach && client.is_open() {
            // 실패 알림은 ensure_terminal_hooks가 처리한다. 열린 터미널의 수명과 분리한다
            self.check_hooks_in_background(task);
        }
        Ok(attachment)
    }

    /// 백그라운드 실패로 걸린 SSH 쿨다운을 원격 명령보다 먼저 풀고 세션 도구를 확인한다. 걸린 시간(ms)을 돌려준다
    fn check_remote_dependency(
        &self,
        session_type: SessionType,
        ssh_host: &str,
    ) -> Result<u64, TerminalBridgeError> {
        let dependencies = &self.dependencies;
        dependencies
            .transport_cooldowns
            .clear_transport_cooldown(ssh_host);
        let (dependency_check, elapsed_ms) = self.measure_phase(|| {
            dependencies
                .session_dependencies
                .ensure_remote_session_dependency(session_type, ssh_host)
        });
        dependency_check?;
        Ok(elapsed_ms)
    }

    /// attach 직전 SSH 접속 정보를 다시 찾는다. 그 사이 클라이언트가 닫혔으면 `None`이고, 찾지 못하면 클라이언트를 닫는다
    fn resolve_attach_ssh_config(
        &self,
        task_id: &str,
        client: &Arc<dyn TerminalClient>,
    ) -> Result<Option<SshHostConfig>, TerminalBridgeError> {
        if !client.is_open() {
            return Ok(None);
        }
        let resolved = self.resolve_remote_ssh_config(task_id)?;
        if !client.is_open() {
            return Ok(None);
        }
        match resolved {
            Ok(ssh_config) => Ok(Some(ssh_config)),
            Err(message) => {
                client.close(Some(ClientClosure {
                    code: POLICY_VIOLATION_CLOSE_CODE,
                    reason: message.clone(),
                }));
                Err(TerminalBridgeError::new(message))
            }
        }
    }

    /// taskId로 원격 세션 여부와 매칭되는 SSH 접속 정보를 함께 조회한다.
    /// 바깥 `Err`는 조회 자체의 실패이고, 안쪽 `Err`는 사용자에게 보일 판정 문구다
    fn resolve_remote_ssh_config(
        &self,
        task_id: &str,
    ) -> Result<Result<SshHostConfig, String>, TerminalBridgeError> {
        let task = self.dependencies.store.find_task(task_id)?;
        let Some(ssh_host) = task
            .as_ref()
            .and_then(|task| non_empty(task.ssh_host.as_deref()))
        else {
            return Ok(Err(NOT_REMOTE_SESSION.to_string()));
        };
        let ssh_config = self
            .dependencies
            .ssh_hosts
            .parse_ssh_config()
            .into_iter()
            .find(|config| config.host == ssh_host);
        Ok(ssh_config.ok_or_else(|| format!("SSH 호스트를 찾을 수 없습니다: {ssh_host}")))
    }

    /// hook은 부가 기능이라 준비에 실패해도 터미널 연결을 막지 않는다. 실패 알림은 ensure_terminal_hooks가 처리한다.
    /// 걸린 시간(ms)을 돌려준다
    fn check_hooks_before_attach(&self, task: &KanbanTask) -> u64 {
        let (_, elapsed_ms) =
            self.measure_phase(|| ensure_terminal_hooks(&self.dependencies, task));
        elapsed_ms
    }

    fn measure_phase<T>(&self, run_phase: impl FnOnce() -> T) -> (T, u64) {
        let phase_started_at = (self.dependencies.clock)();
        let outcome = run_phase();
        let elapsed_ms = elapsed_ms_since(phase_started_at, (self.dependencies.clock)());
        (outcome, elapsed_ms)
    }

    /// hook 확인을 열린 터미널과 떼어 따로 돌린다. 실패 알림은 ensure_terminal_hooks가 처리한다
    fn check_hooks_in_background(&self, task: &KanbanTask) {
        let dependencies = self.dependencies.clone();
        let task = task.clone();
        std::thread::spawn(move || {
            let _ = ensure_terminal_hooks(&dependencies, &task);
        });
    }
}

/// hook을 확인한다. 실패하면 로그를 남기고, 일시적인 SSH 재연결 대기가 아닐 때만 보드에 알린 뒤 실패를 돌려준다
fn ensure_terminal_hooks(
    dependencies: &TerminalBridgeDependencies,
    task: &KanbanTask,
) -> Result<(), TerminalBridgeError> {
    let mut hook_target_path = non_empty(task.worktree_path.as_deref()).map(str::to_string);
    let Err(error) = verify_task_hooks(dependencies, task, &mut hook_target_path) else {
        return Ok(());
    };
    dependencies
        .events
        .terminal_hook_check_failed(&TerminalHookCheckFailure {
            task_id: task.id.clone(),
            task_title: task.title.clone(),
            target_path: hook_target_path,
            ssh_host: task.ssh_host.clone(),
            error: error.message.clone(),
        });
    if error.retry_after_ms.is_none() {
        dependencies
            .events
            .task_hook_install_failed(&TaskHookInstallFailure {
                task_id: task.id.clone(),
                task_title: task.title.clone(),
                error: error.message.clone(),
            });
    }
    Err(error)
}

/// worktree 경로가 없는 기본 브랜치 세션은 프로젝트 경로의 hook을 본다
fn verify_task_hooks(
    dependencies: &TerminalBridgeDependencies,
    task: &KanbanTask,
    hook_target_path: &mut Option<String>,
) -> Result<(), TerminalBridgeError> {
    if hook_target_path.is_none()
        && let Some(project_id) = non_empty(task.project_id.as_deref())
    {
        let repo_path = dependencies.store.find_project_repo_path(project_id)?;
        *hook_target_path = repo_path.filter(|path| !path.is_empty());
    }
    if let Some(target_path) = hook_target_path.as_deref() {
        dependencies
            .hooks
            .ensure_hooks_current(target_path, &task.id, task.ssh_host.as_deref())?;
    }
    Ok(())
}

fn build_remote_attach_request(
    target: TaskAttachTarget<'_>,
    ssh_host: &str,
    ssh_config: SshHostConfig,
) -> RemoteAttachRequest {
    RemoteAttachRequest {
        task_id: target.task.id.clone(),
        tab_id: target.tab_id.map(str::to_string),
        ssh_host: ssh_host.to_string(),
        ssh_config,
        session_type: target.session_type,
        session_name: target.session_name.to_string(),
        columns: target.columns,
        rows: target.rows,
        worktree_path: target.task.worktree_path.clone(),
        pane_layout: target.pane_layout,
    }
}

/// 세션 종류와 이름이 모두 있어야 붙을 수 있다
fn task_session(task: &KanbanTask) -> Result<(SessionType, &str), TerminalBridgeError> {
    match (task.session_type, non_empty(task.session_name.as_deref())) {
        (Some(session_type), Some(session_name)) => Ok((session_type, session_name)),
        _ => Err(TerminalBridgeError::new(TASK_WITHOUT_SESSION)),
    }
}

fn has_initial_pane_commands(pane_layout: Option<&PaneLayoutSelection>) -> bool {
    pane_layout.is_some_and(|layout| {
        layout
            .panes
            .iter()
            .any(|pane| !js_trim(&pane.command).is_empty())
    })
}

fn cancel_open(client: &WindowTerminalClient) -> TerminalOpenResult {
    client.close(None);
    Err(TerminalBridgeError::new(TERMINAL_CONNECTION_FAILED))
}

/// 멀티플렉서 세션은 탭 없이, terminal 세션은 탭마다 키가 갈린다.
/// 그래야 tmux·zellij는 창 하나가 PTY 하나를 공유하고, terminal 세션만 탭마다 스트림이 갈린다
fn build_client_key(window_id: u64, task_id: &str, tab_id: Option<&str>) -> String {
    format!("{window_id}:{task_id}:{}", tab_id.unwrap_or_default())
}

fn build_resize_message(columns: u16, rows: u16) -> String {
    format!("\x01{{\"type\":\"resize\",\"cols\":{columns},\"rows\":{rows}}}")
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|text| !text.is_empty())
}

fn elapsed_ms_since(started_at: Instant, now: Instant) -> u64 {
    now.saturating_duration_since(started_at).as_millis() as u64
}
