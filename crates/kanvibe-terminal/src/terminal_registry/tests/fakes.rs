//! 레지스트리 테스트용 가짜 협력자. 실제 PTY·멀티플렉서·Herdr는 띄우지 않는다.

use std::collections::{BTreeMap, VecDeque};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use kanvibe_domain::task::SessionType;
use kanvibe_process::shell_environment::ShellPlatform;
use kanvibe_process::ssh_config::SshHostConfig;
use kanvibe_process::worktree::PaneLayoutSelection;
use serde_json::Value;

use super::super::*;
use crate::pty::{PtyEventCallbacks, PtyExit, PtyProcess, PtySpawnRequest, PtySpawner};
use crate::test_support::ScriptedShellRunner;

/// 열기 전까지 들어온 호출을 붙잡아 두는 문. 들어온 수를 세어 테스트가 경쟁 순서를 맞춘다
pub struct Gate {
    is_opened: Mutex<bool>,
    opened: Condvar,
    entered_count: AtomicUsize,
}

impl Gate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            is_opened: Mutex::new(false),
            opened: Condvar::new(),
            entered_count: AtomicUsize::new(0),
        })
    }

    pub fn pass(&self) {
        self.entered_count.fetch_add(1, Ordering::SeqCst);
        let mut is_opened = self.is_opened.lock().unwrap();
        while !*is_opened {
            is_opened = self.opened.wait(is_opened).unwrap();
        }
    }

    pub fn open(&self) {
        *self.is_opened.lock().unwrap() = true;
        self.opened.notify_all();
    }

    pub fn entered_count(&self) -> usize {
        self.entered_count.load(Ordering::SeqCst)
    }
}

/// 조건이 참이 될 때까지 기다린다. 5초 안에 안 되면 실패한다
pub fn wait_until(description: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "기다리던 조건이 오지 않았다: {description}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// 한 번만 막는 문. 꺼내 쓰면 비어 다음 호출은 바로 지나간다(vitest `mockImplementationOnce`)
#[derive(Default)]
pub struct OnceGate(Mutex<Option<Arc<Gate>>>);

impl OnceGate {
    pub fn install(&self) -> Arc<Gate> {
        let gate = Gate::new();
        *self.0.lock().unwrap() = Some(Arc::clone(&gate));
        gate
    }

    fn pass(&self) {
        let gate = self.0.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.pass();
        }
    }
}

pub struct FakeClient {
    is_open: AtomicBool,
    buffered_amount: AtomicUsize,
    pub sent: Mutex<Vec<String>>,
    pub closures: Mutex<Vec<Option<ClientClosure>>>,
}

impl FakeClient {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            is_open: AtomicBool::new(true),
            buffered_amount: AtomicUsize::new(0),
            sent: Mutex::new(Vec::new()),
            closures: Mutex::new(Vec::new()),
        })
    }

    pub fn set_open(&self, is_open: bool) {
        self.is_open.store(is_open, Ordering::SeqCst);
    }

    pub fn set_buffered_amount(&self, bytes: usize) {
        self.buffered_amount.store(bytes, Ordering::SeqCst);
    }

    pub fn sent(&self) -> Vec<String> {
        self.sent.lock().unwrap().clone()
    }

    pub fn closures(&self) -> Vec<Option<ClientClosure>> {
        self.closures.lock().unwrap().clone()
    }
}

impl TerminalClient for FakeClient {
    fn is_open(&self) -> bool {
        self.is_open.load(Ordering::SeqCst)
    }

    fn send(&self, data: &str) {
        self.sent.lock().unwrap().push(data.to_string());
    }

    fn buffered_amount(&self) -> usize {
        self.buffered_amount.load(Ordering::SeqCst)
    }

    /// TS mock WebSocket처럼 닫혀도 상태는 그대로 두고 호출만 기록한다
    fn close(&self, closure: Option<ClientClosure>) {
        self.closures.lock().unwrap().push(closure);
    }
}

pub fn closure(code: u16, reason: &str) -> Option<ClientClosure> {
    Some(ClientClosure {
        code,
        reason: reason.to_string(),
    })
}

/// 로컬 PTY와 daemon PTY를 함께 흉내 낸다
pub struct FakePty {
    pid: u32,
    pub process_name: Mutex<Option<String>>,
    size: Mutex<(u16, u16)>,
    pub writes: Mutex<Vec<String>>,
    pub resizes: Mutex<Vec<(u16, u16)>>,
    kill_count: AtomicUsize,
    close_session_count: AtomicUsize,
    callbacks: Mutex<Option<Arc<PtyEventCallbacks>>>,
}

impl FakePty {
    pub fn new(pid: u32, columns: u16, rows: u16) -> Arc<Self> {
        Arc::new(Self {
            pid,
            process_name: Mutex::new(Some("zsh".to_string())),
            size: Mutex::new((columns, rows)),
            writes: Mutex::new(Vec::new()),
            resizes: Mutex::new(Vec::new()),
            kill_count: AtomicUsize::new(0),
            close_session_count: AtomicUsize::new(0),
            callbacks: Mutex::new(None),
        })
    }

    fn attach_callbacks(&self, callbacks: PtyEventCallbacks) {
        *self.callbacks.lock().unwrap() = Some(Arc::new(callbacks));
    }

    fn callbacks(&self) -> Arc<PtyEventCallbacks> {
        self.callbacks
            .lock()
            .unwrap()
            .clone()
            .expect("콜백이 등록돼 있어야 한다")
    }

    pub fn emit_output(&self, data: &str) {
        (self.callbacks().on_output)(data);
    }

    pub fn emit_exit(&self, exit_code: i32, is_detached: bool) {
        (self.callbacks().on_exit)(PtyExit {
            exit_code,
            signal: None,
            is_detached,
        });
    }

    pub fn kill_count(&self) -> usize {
        self.kill_count.load(Ordering::SeqCst)
    }

    pub fn close_session_count(&self) -> usize {
        self.close_session_count.load(Ordering::SeqCst)
    }

    pub fn resizes(&self) -> Vec<(u16, u16)> {
        self.resizes.lock().unwrap().clone()
    }

    pub fn writes(&self) -> Vec<String> {
        self.writes.lock().unwrap().clone()
    }
}

impl PtyProcess for FakePty {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn foreground_process_name(&self) -> Option<String> {
        self.process_name.lock().unwrap().clone()
    }

    fn columns(&self) -> u16 {
        self.size.lock().unwrap().0
    }

    fn rows(&self) -> u16 {
        self.size.lock().unwrap().1
    }

    fn write(&self, data: &str) -> Result<(), String> {
        self.writes.lock().unwrap().push(data.to_string());
        Ok(())
    }

    fn resize(&self, columns: u16, rows: u16) -> Result<(), String> {
        self.resizes.lock().unwrap().push((columns, rows));
        *self.size.lock().unwrap() = (columns, rows);
        Ok(())
    }

    fn kill(&self) -> Result<(), String> {
        self.kill_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

impl RemoteHostPty for FakePty {
    fn session_id(&self) -> String {
        "daemon-issued".to_string()
    }

    fn close_session(&self) -> Result<(), String> {
        self.close_session_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Default)]
pub struct FakePtySpawner {
    spawned: Mutex<Vec<(PtySpawnRequest, Arc<FakePty>)>>,
    pub should_fail_next: AtomicBool,
    pub spawn_gate: OnceGate,
}

impl FakePtySpawner {
    pub fn count(&self) -> usize {
        self.spawned.lock().unwrap().len()
    }

    pub fn request(&self, index: usize) -> PtySpawnRequest {
        self.spawned.lock().unwrap()[index].0.clone()
    }

    pub fn pty(&self, index: usize) -> Arc<FakePty> {
        Arc::clone(&self.spawned.lock().unwrap()[index].1)
    }
}

impl PtySpawner for FakePtySpawner {
    fn spawn(
        &self,
        request: PtySpawnRequest,
        callbacks: PtyEventCallbacks,
    ) -> Result<Arc<dyn PtyProcess>, String> {
        self.spawn_gate.pass();
        if self.should_fail_next.swap(false, Ordering::SeqCst) {
            return Err("spawn failed".to_string());
        }
        let mut spawned = self.spawned.lock().unwrap();
        let pty = FakePty::new(1000 + spawned.len() as u32, request.columns, request.rows);
        pty.attach_callbacks(callbacks);
        spawned.push((request, Arc::clone(&pty)));
        Ok(pty)
    }
}

/// `ensure_session` 호출 하나: 세션 이름, 작업 디렉터리, 레이아웃, 호스트
pub type EnsureSessionCall = (String, String, Option<PaneLayoutSelection>, Option<String>);

pub struct FakeHerdr {
    pub ensure_session_calls: Mutex<Vec<EnsureSessionCall>>,
    pub ensure_session_gate: OnceGate,
    /// task 키마다 공유 Space ID. 없는 키나 독립 서버는 `w1`이다
    pub workspace_ids: Mutex<BTreeMap<String, String>>,
    pub remote_machine_calls: Mutex<Vec<(String, String)>>,
    pub remote_machine_results: Mutex<VecDeque<Result<(), HerdrSessionError>>>,
    pub remote_machine_gate: OnceGate,
    pub handoff_calls: Mutex<Vec<(String, String)>>,
    pub handoff_gate: OnceGate,
    pub handoff_result: Mutex<Result<(), HerdrSessionError>>,
    pub tab_bar_position: Mutex<Option<HerdrTabBarPosition>>,
    pub pane_tabs: Mutex<Vec<HerdrPaneTab>>,
}

impl Default for FakeHerdr {
    fn default() -> Self {
        Self {
            ensure_session_calls: Mutex::default(),
            ensure_session_gate: OnceGate::default(),
            workspace_ids: Mutex::new(BTreeMap::from([
                ("task-a".to_string(), "w1".to_string()),
                ("task-b".to_string(), "w2".to_string()),
            ])),
            remote_machine_calls: Mutex::default(),
            remote_machine_results: Mutex::default(),
            remote_machine_gate: OnceGate::default(),
            handoff_calls: Mutex::default(),
            handoff_gate: OnceGate::default(),
            handoff_result: Mutex::new(Ok(())),
            tab_bar_position: Mutex::new(Some(HerdrTabBarPosition::Top)),
            pane_tabs: Mutex::default(),
        }
    }
}

impl FakeHerdr {
    pub fn remote_machine_calls(&self) -> Vec<(String, String)> {
        self.remote_machine_calls.lock().unwrap().clone()
    }
}

impl HerdrSessions for FakeHerdr {
    fn ensure_session(
        &self,
        session_name: &str,
        working_directory: &str,
        pane_layout: Option<&PaneLayoutSelection>,
        ssh_host: Option<&str>,
    ) -> Result<Option<String>, HerdrSessionError> {
        self.ensure_session_calls.lock().unwrap().push((
            session_name.to_string(),
            working_directory.to_string(),
            pane_layout.cloned(),
            ssh_host.map(str::to_string),
        ));
        self.ensure_session_gate.pass();
        let workspace_key = session_name
            .strip_prefix("kanvibe:")
            .unwrap_or(session_name);
        let workspace_ids = self.workspace_ids.lock().unwrap();
        Ok(Some(
            workspace_ids
                .get(workspace_key)
                .cloned()
                .unwrap_or_else(|| "w1".to_string()),
        ))
    }

    fn ensure_remote_machine(
        &self,
        session_name: &str,
        ssh_host: &str,
    ) -> Result<(), HerdrSessionError> {
        self.remote_machine_calls
            .lock()
            .unwrap()
            .push((session_name.to_string(), ssh_host.to_string()));
        self.remote_machine_gate.pass();
        self.remote_machine_results
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(()))
    }

    fn handoff_remote_server(
        &self,
        herdr_session: &str,
        ssh_host: &str,
    ) -> Result<(), HerdrSessionError> {
        self.handoff_calls
            .lock()
            .unwrap()
            .push((herdr_session.to_string(), ssh_host.to_string()));
        self.handoff_gate.pass();
        self.handoff_result.lock().unwrap().clone()
    }

    fn client_socket_path(
        &self,
        _session_name: &str,
        _ssh_host: Option<&str>,
    ) -> Result<String, HerdrSessionError> {
        Ok("/config/herdr/sessions/kanvibe/herdr.sock".to_string())
    }

    fn client_tab_bar_position(&self, _ssh_host: Option<&str>) -> Option<HerdrTabBarPosition> {
        *self.tab_bar_position.lock().unwrap()
    }

    fn list_task_pane_tab_ids(
        &self,
        _session_name: &str,
        _ssh_host: Option<&str>,
    ) -> Result<Vec<HerdrPaneTab>, HerdrSessionError> {
        Ok(self.pane_tabs.lock().unwrap().clone())
    }
}

pub struct FakeRelay {
    socket_path: String,
    pub active_tab_id: Mutex<Option<String>>,
    pub active_pane_id: Mutex<Option<String>>,
    pub surface_size: Mutex<Option<(u16, u16)>>,
    pub requests: Mutex<Vec<(String, Value)>>,
    close_count: AtomicUsize,
}

impl FakeRelay {
    pub fn new(socket_path: &str) -> Arc<Self> {
        Arc::new(Self {
            socket_path: socket_path.to_string(),
            active_tab_id: Mutex::new(Some("w1:t1".to_string())),
            active_pane_id: Mutex::new(None),
            surface_size: Mutex::new(None),
            requests: Mutex::new(Vec::new()),
            close_count: AtomicUsize::new(0),
        })
    }

    pub fn requests(&self) -> Vec<(String, Value)> {
        self.requests.lock().unwrap().clone()
    }

    pub fn close_count(&self) -> usize {
        self.close_count.load(Ordering::SeqCst)
    }
}

impl HerdrClientRelay for FakeRelay {
    fn socket_path(&self) -> String {
        self.socket_path.clone()
    }

    fn active_pane_id(&self) -> Option<String> {
        self.active_pane_id.lock().unwrap().clone()
    }

    fn active_tab_id(&self) -> Option<String> {
        self.active_tab_id.lock().unwrap().clone()
    }

    fn surface_size(&self) -> Option<(u16, u16)> {
        *self.surface_size.lock().unwrap()
    }

    fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        self.requests
            .lock()
            .unwrap()
            .push((method.to_string(), params));
        Ok(Value::Null)
    }

    fn close(&self) -> Result<(), String> {
        self.close_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// 넣어 둔 중계를 차례로 돌려주고, 없으면 새로 만든다
#[derive(Default)]
pub struct FakeRelayFactory {
    pub queued: Mutex<VecDeque<Arc<FakeRelay>>>,
    pub created: Mutex<Vec<(String, String)>>,
    pub create_gate: OnceGate,
}

impl FakeRelayFactory {
    pub fn queue(&self, relay: &Arc<FakeRelay>) {
        self.queued.lock().unwrap().push_back(Arc::clone(relay));
    }

    pub fn created(&self) -> Vec<(String, String)> {
        self.created.lock().unwrap().clone()
    }
}

impl HerdrClientRelayFactory for FakeRelayFactory {
    fn create(
        &self,
        upstream_socket_path: &str,
        workspace_id: &str,
    ) -> Result<Arc<dyn HerdrClientRelay>, String> {
        self.created
            .lock()
            .unwrap()
            .push((upstream_socket_path.to_string(), workspace_id.to_string()));
        self.create_gate.pass();
        let queued = self.queued.lock().unwrap().pop_front();
        Ok(queued.unwrap_or_else(|| FakeRelay::new("/tmp/proxy/client")))
    }
}

/// daemon 연결. 넣어 둔 PTY를 차례로 열어 준다
#[derive(Default)]
pub struct FakeConnection {
    pub queued_ptys: Mutex<VecDeque<Arc<FakePty>>>,
    pub open_pty_calls: Mutex<Vec<(String, String, u16, u16)>>,
    pub sessions: Mutex<Vec<RemoteHostPtySession>>,
    pub closed_session_ids: Mutex<Vec<String>>,
    pub close_failure: Mutex<Option<String>>,
}

impl FakeConnection {
    pub fn queue_pty(&self) -> Arc<FakePty> {
        let pty = FakePty::new(4321, 80, 24);
        *pty.process_name.lock().unwrap() = Some("remote-shell".to_string());
        self.queued_ptys.lock().unwrap().push_back(Arc::clone(&pty));
        pty
    }

    pub fn open_pty_calls(&self) -> Vec<(String, String, u16, u16)> {
        self.open_pty_calls.lock().unwrap().clone()
    }

    pub fn closed_session_ids(&self) -> Vec<String> {
        self.closed_session_ids.lock().unwrap().clone()
    }
}

impl RemoteHostConnection for FakeConnection {
    fn open_pty(
        &self,
        key: &str,
        command: &str,
        columns: u16,
        rows: u16,
        callbacks: PtyEventCallbacks,
    ) -> Result<Arc<dyn RemoteHostPty>, String> {
        self.open_pty_calls.lock().unwrap().push((
            key.to_string(),
            command.to_string(),
            columns,
            rows,
        ));
        let pty = self
            .queued_ptys
            .lock()
            .unwrap()
            .pop_front()
            .expect("열 daemon PTY를 넣어 둬야 한다");
        pty.attach_callbacks(callbacks);
        Ok(pty)
    }

    fn list_pty_sessions(&self) -> Result<Vec<RemoteHostPtySession>, String> {
        Ok(self.sessions.lock().unwrap().clone())
    }

    fn close_pty_session(&self, session_id: &str) -> Result<(), String> {
        if let Some(failure) = self.close_failure.lock().unwrap().take() {
            return Err(failure);
        }
        self.closed_session_ids
            .lock()
            .unwrap()
            .push(session_id.to_string());
        Ok(())
    }

    fn forward_socket(
        &self,
        _remote_socket_path: &str,
    ) -> Result<Arc<dyn RemoteHostSocketForward>, String> {
        Err("forward_socket은 이 테스트에서 쓰지 않는다".to_string())
    }
}

#[derive(Default)]
pub struct FakeRemoteHosts {
    pub connection: Mutex<Option<Arc<FakeConnection>>>,
}

impl RemoteHostConnections for FakeRemoteHosts {
    fn connection(&self, _ssh_host: &str) -> Option<Arc<dyn RemoteHostConnection>> {
        let connection = self.connection.lock().unwrap().clone()?;
        Some(connection)
    }
}

#[derive(Default)]
pub struct FakeEvents {
    pub handoff_required: Mutex<Vec<(String, String)>>,
    pub registration_failed: Mutex<Vec<(String, String, String)>>,
}

impl FakeEvents {
    pub fn registration_failed(&self) -> Vec<(String, String, String)> {
        self.registration_failed.lock().unwrap().clone()
    }

    pub fn handoff_required(&self) -> Vec<(String, String)> {
        self.handoff_required.lock().unwrap().clone()
    }
}

impl TerminalEventSink for FakeEvents {
    fn herdr_server_handoff_required(&self, ssh_host: &str, herdr_session: &str) {
        self.handoff_required
            .lock()
            .unwrap()
            .push((ssh_host.to_string(), herdr_session.to_string()));
    }

    fn herdr_machine_registration_failed(&self, ssh_host: &str, herdr_session: &str, reason: &str) {
        self.registration_failed.lock().unwrap().push((
            ssh_host.to_string(),
            herdr_session.to_string(),
            reason.to_string(),
        ));
    }
}

pub struct Harness {
    pub registry: Arc<TerminalRegistry>,
    pub runner: Arc<ScriptedShellRunner>,
    pub ptys: Arc<FakePtySpawner>,
    pub herdr: Arc<FakeHerdr>,
    pub relays: Arc<FakeRelayFactory>,
    pub remote_hosts: Arc<FakeRemoteHosts>,
    pub events: Arc<FakeEvents>,
    pub environment: Arc<TerminalHostEnvironment>,
    pub temp_directory: tempfile::TempDir,
}

impl Harness {
    pub fn new() -> Self {
        Self::with_parent_variables(&[])
    }

    /// 서버 런타임 값이 섞인 부모 환경에서 만든다. `extra`가 기본값을 덮는다
    pub fn with_parent_variables(extra: &[(&str, &str)]) -> Self {
        let temp_directory = tempfile::tempdir().unwrap();
        let mut parent: BTreeMap<String, String> = [
            ("HOME", "/home/local-user"),
            ("SHELL", "/bin/zsh"),
            ("PATH", "/usr/bin:/bin"),
            ("PORT", "9736"),
            ("HOST", "0.0.0.0"),
            ("NODE_ENV", "production"),
            ("KANVIBE_APP_DATA_DIR", "/tmp/kanvibe-app-data"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
        parent.insert(
            "TMPDIR".to_string(),
            temp_directory.path().to_string_lossy().into_owned(),
        );
        for (key, value) in extra {
            parent.insert(key.to_string(), value.to_string());
        }
        let environment = Arc::new(TerminalHostEnvironment::from_parent_environment(
            &parent,
            ShellPlatform::Other,
            "/fallback",
        ));
        Self::build(environment, temp_directory)
    }

    fn build(environment: Arc<TerminalHostEnvironment>, temp_directory: tempfile::TempDir) -> Self {
        let runner = ScriptedShellRunner::new();
        let ptys = Arc::new(FakePtySpawner::default());
        let herdr = Arc::new(FakeHerdr::default());
        let relays = Arc::new(FakeRelayFactory::default());
        let remote_hosts = Arc::new(FakeRemoteHosts::default());
        let events = Arc::new(FakeEvents::default());
        let registry = TerminalRegistry::new(TerminalRegistryDependencies {
            runner: runner.clone(),
            environment: Arc::clone(&environment),
            pty_spawner: ptys.clone(),
            herdr: herdr.clone(),
            herdr_relays: relays.clone(),
            remote_hosts: remote_hosts.clone(),
            events: events.clone(),
        });
        Self {
            registry,
            runner,
            ptys,
            herdr,
            relays,
            remote_hosts,
            events,
            environment,
            temp_directory,
        }
    }

    /// daemon 연결을 쓰는 호스트로 만든다
    pub fn with_daemon_connection(&self) -> Arc<FakeConnection> {
        let connection = Arc::new(FakeConnection::default());
        *self.remote_hosts.connection.lock().unwrap() = Some(Arc::clone(&connection));
        connection
    }

    pub fn attach_local(
        &self,
        request: LocalAttachRequest,
        client: &Arc<FakeClient>,
    ) -> AttachResult {
        self.registry.attach_local_session(request, client.clone())
    }

    pub fn attach_remote(
        &self,
        request: RemoteAttachRequest,
        client: &Arc<FakeClient>,
    ) -> AttachResult {
        self.registry.attach_remote_session(request, client.clone())
    }

    /// 다른 스레드에서 attach를 시작한다
    pub fn attach_in_background(
        &self,
        request: AttachTarget,
        client: &Arc<FakeClient>,
    ) -> std::thread::JoinHandle<AttachResult> {
        let registry = Arc::clone(&self.registry);
        let client: Arc<dyn TerminalClient> = client.clone();
        std::thread::spawn(move || match request {
            AttachTarget::Local(request) => registry.attach_local_session(request, client),
            AttachTarget::Remote(request) => registry.attach_remote_session(request, client),
        })
    }

    pub fn commands(&self) -> Vec<String> {
        self.runner.commands()
    }

    pub fn command_containing(&self, needle: &str) -> Option<String> {
        self.commands()
            .into_iter()
            .find(|command| command.contains(needle))
    }
}

pub enum AttachTarget {
    Local(LocalAttachRequest),
    Remote(RemoteAttachRequest),
}

pub fn local_request(
    task_id: &str,
    tab_id: Option<&str>,
    session_type: SessionType,
    session_name: &str,
    working_directory: Option<&str>,
) -> LocalAttachRequest {
    LocalAttachRequest {
        task_id: task_id.to_string(),
        tab_id: tab_id.map(str::to_string),
        session_type,
        session_name: session_name.to_string(),
        working_directory: working_directory.map(str::to_string),
        columns: None,
        rows: None,
        pane_layout: None,
    }
}

pub fn ssh_config(
    host: &str,
    hostname: &str,
    port: i64,
    username: &str,
    private_key_path: &str,
) -> SshHostConfig {
    SshHostConfig {
        host: host.to_string(),
        hostname: hostname.to_string(),
        port,
        username: username.to_string(),
        private_key_path: private_key_path.to_string(),
    }
}

pub fn remote_request(
    task_id: &str,
    tab_id: Option<&str>,
    ssh_host: &str,
    session_type: SessionType,
    session_name: &str,
    worktree_path: Option<&str>,
) -> RemoteAttachRequest {
    RemoteAttachRequest {
        task_id: task_id.to_string(),
        tab_id: tab_id.map(str::to_string),
        ssh_host: ssh_host.to_string(),
        ssh_config: ssh_config(ssh_host, "example.com", 2202, "tester", "/tmp/test-key"),
        session_type,
        session_name: session_name.to_string(),
        columns: Some(120),
        rows: Some(30),
        worktree_path: worktree_path.map(str::to_string),
        pane_layout: None,
    }
}

/// 실제 `/bin/sh -n`으로 POSIX 문법을 확인한다. 명령을 실행하지 않는다
pub fn assert_valid_posix_syntax(command: &str) {
    let result = Command::new("/bin/sh")
        .args(["-n", "-c", command])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{command}",
        String::from_utf8_lossy(&result.stderr)
    );
}

/// 원격 명령은 `sh -lc '<payload>'`로 감싸 전달되므로 실제 셸로 풀어 원문을 본다
pub fn extract_remote_shell_payload(remote_shell_command: &str) -> String {
    let script = format!("set -- {remote_shell_command}; printf '%s' \"$3\"");
    let result = Command::new("/bin/sh")
        .args(["-c", &script])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let payload = String::from_utf8(result.stdout).unwrap();
    assert!(!payload.is_empty());
    payload
}
