//! TS `terminalBridge.test.ts`처럼 레지스트리, DB, 원격 의존성, 쿨다운, hook 설치기, 이미지 전송, 보드 알림을 가짜로 바꾼다.
//! 호출 순서는 가짜들이 함께 쓰는 기록으로 비교한다(vitest `invocationCallOrder`).

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use kanvibe_domain::pane_layout::{PaneCommand, PaneLayoutType};
use kanvibe_domain::task::TaskStatus;
use tempfile::TempDir;

use super::*;

/// 가짜들이 함께 남기는 호출 순서
#[derive(Default)]
struct CallLog(Mutex<Vec<String>>);

impl CallLog {
    fn push(&self, entry: impl Into<String>) {
        self.0.lock().unwrap().push(entry.into());
    }

    fn position(&self, entry: &str) -> usize {
        let entries = self.0.lock().unwrap();
        entries
            .iter()
            .position(|recorded| recorded == entry)
            .unwrap_or_else(|| panic!("{entry} 호출이 없다: {entries:?}"))
    }

    fn count(&self, entry: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|recorded| *recorded == entry)
            .count()
    }
}

/// 원격 연결 단계 시간을 재는 가짜 시계. 가짜 협력자가 걸린 시간만큼 앞으로 민다
struct FakeClock {
    started_at: Instant,
    offset: Mutex<Duration>,
}

impl FakeClock {
    fn now(&self) -> Instant {
        self.started_at + *self.offset.lock().unwrap()
    }

    fn advance(&self, milliseconds: u64) {
        *self.offset.lock().unwrap() += Duration::from_millis(milliseconds);
    }
}

/// 호출이 들어온 것을 알리고, 테스트가 풀어 줄 때까지 붙잡는다(vitest 미해결 Promise)
#[derive(Default)]
struct Gate {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

impl Gate {
    fn enter_and_wait(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.changed.notify_all();
        let _released = self.changed.wait_while(state, |state| !state.1).unwrap();
    }

    fn wait_entered(&self) {
        let state = self.state.lock().unwrap();
        let (_state, timeout) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(5), |state| !state.0)
            .unwrap();
        assert!(!timeout.timed_out(), "붙잡힌 호출이 들어오지 않았다");
    }

    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }
}

struct FakeSessions {
    log: Arc<CallLog>,
    clock: Arc<FakeClock>,
    remote_attach_ms: Mutex<u64>,
    attached_clients: Mutex<Vec<Arc<dyn TerminalClient>>>,
    messages: Mutex<Vec<(TerminalAttachment, String)>>,
}

impl FakeSessions {
    fn attach(&self, label: &str, client: Arc<dyn TerminalClient>) -> Option<TerminalAttachment> {
        self.log.push(label);
        let mut attached_clients = self.attached_clients.lock().unwrap();
        attached_clients.push(client);
        Some(TerminalAttachment {
            terminal_key: label.to_string(),
            entry_generation: 1,
            client_id: attached_clients.len() as u64,
        })
    }
}

impl TerminalSessions for FakeSessions {
    fn attach_local_session(
        &self,
        _request: LocalAttachRequest,
        client: Arc<dyn TerminalClient>,
    ) -> Result<Option<TerminalAttachment>, String> {
        Ok(self.attach("attach_local", client))
    }

    fn attach_remote_session(
        &self,
        _request: RemoteAttachRequest,
        client: Arc<dyn TerminalClient>,
    ) -> Result<Option<TerminalAttachment>, String> {
        self.clock.advance(*self.remote_attach_ms.lock().unwrap());
        Ok(self.attach("attach_remote", client))
    }

    fn prepare_terminal_session(
        &self,
        _target: TerminalSessionPreparationTarget,
    ) -> Result<PreparedTerminalSession, String> {
        Ok(PreparedTerminalSession {
            is_prepared: true,
            skipped_reason: None,
            herdr_workspace_id: None,
            herdr_panes: None,
        })
    }

    fn focus_session(&self, task_id: &str) {
        self.log.push(format!("focus:{task_id}"));
    }

    fn client_message(&self, attachment: &TerminalAttachment, message: &str) {
        self.messages
            .lock()
            .unwrap()
            .push((attachment.clone(), message.to_string()));
    }

    fn client_closed(&self, _attachment: &TerminalAttachment) {
        self.log.push("client_closed");
    }
}

#[derive(Default)]
struct FakeStore {
    tasks: Mutex<HashMap<String, KanbanTask>>,
    project_repo_paths: Mutex<HashMap<String, String>>,
    project_lookups: Mutex<Vec<String>>,
    pane_layout: Mutex<Option<PaneLayoutSelection>>,
    next_task_failure: Mutex<Option<String>>,
    task_gate: Mutex<Option<Arc<Gate>>>,
}

impl TerminalTaskStore for FakeStore {
    fn find_task(&self, task_id: &str) -> Result<Option<KanbanTask>, String> {
        let gate = self.task_gate.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.enter_and_wait();
        }
        if let Some(message) = self.next_task_failure.lock().unwrap().take() {
            return Err(message);
        }
        Ok(self.tasks.lock().unwrap().get(task_id).cloned())
    }

    fn find_project_repo_path(&self, project_id: &str) -> Result<Option<String>, String> {
        self.project_lookups
            .lock()
            .unwrap()
            .push(project_id.to_string());
        Ok(self
            .project_repo_paths
            .lock()
            .unwrap()
            .get(project_id)
            .cloned())
    }

    fn effective_pane_layout(
        &self,
        _project_id: Option<&str>,
    ) -> Result<Option<PaneLayoutSelection>, String> {
        Ok(self.pane_layout.lock().unwrap().clone())
    }
}

struct FakeDependencyCheck {
    log: Arc<CallLog>,
    clock: Arc<FakeClock>,
    elapsed_ms: Mutex<u64>,
    next_failure: Mutex<Option<ExecGitError>>,
}

impl RemoteSessionDependencyCheck for FakeDependencyCheck {
    fn ensure_remote_session_dependency(
        &self,
        _session_type: SessionType,
        _ssh_host: &str,
    ) -> Result<(), ExecGitError> {
        self.log.push("dependency");
        self.clock.advance(*self.elapsed_ms.lock().unwrap());
        self.next_failure.lock().unwrap().take().map_or(Ok(()), Err)
    }
}

struct FakeCooldowns {
    log: Arc<CallLog>,
}

impl SshTransportCooldowns for FakeCooldowns {
    fn clear_transport_cooldown(&self, ssh_host: &str) {
        self.log.push(format!("cooldown:{ssh_host}"));
    }
}

type HookCall = (String, String, Option<String>);

struct FakeHooks {
    log: Arc<CallLog>,
    clock: Arc<FakeClock>,
    elapsed_ms: Mutex<u64>,
    calls: Mutex<Vec<HookCall>>,
    next_failure: Mutex<Option<TerminalBridgeError>>,
    gate: Mutex<Option<Arc<Gate>>>,
}

impl TerminalHookVerifier for FakeHooks {
    fn ensure_hooks_current(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<bool, TerminalBridgeError> {
        self.log.push("hooks");
        let call = (
            target_path.to_string(),
            task_id.to_string(),
            ssh_host.map(str::to_string),
        );
        self.calls.lock().unwrap().push(call);
        let gate = self.gate.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.enter_and_wait();
        }
        self.clock.advance(*self.elapsed_ms.lock().unwrap());
        self.next_failure
            .lock()
            .unwrap()
            .take()
            .map_or(Ok(false), Err)
    }
}

#[derive(Default)]
struct FakeImageTransfer {
    transfers: Mutex<Vec<(SshHostConfig, Vec<u8>)>>,
    transfer_result: Mutex<Option<Result<String, String>>>,
}

impl RemoteImageTransfer for FakeImageTransfer {
    fn decode_data_url(&self, _image_data_url: &str) -> Result<Vec<u8>, String> {
        Ok(b"hello".to_vec())
    }

    fn transfer_image(
        &self,
        ssh_config: &SshHostConfig,
        image_bytes: &[u8],
    ) -> Result<String, String> {
        self.transfers
            .lock()
            .unwrap()
            .push((ssh_config.clone(), image_bytes.to_vec()));
        self.transfer_result.lock().unwrap().clone().unwrap()
    }
}

#[derive(Default)]
struct FakeEvents {
    install_failures: Mutex<Vec<TaskHookInstallFailure>>,
    check_failures: Mutex<Vec<TerminalHookCheckFailure>>,
    phases: Mutex<Vec<RemoteConnectionPhases>>,
}

impl TerminalBridgeEvents for FakeEvents {
    fn task_hook_install_failed(&self, failure: &TaskHookInstallFailure) {
        self.install_failures.lock().unwrap().push(failure.clone());
    }

    fn terminal_hook_check_failed(&self, failure: &TerminalHookCheckFailure) {
        self.check_failures.lock().unwrap().push(failure.clone());
    }

    fn remote_connection_phases(&self, phases: &RemoteConnectionPhases) {
        self.phases.lock().unwrap().push(phases.clone());
    }
}

struct FakeWindow {
    window_id: u64,
}

impl TerminalWindow for FakeWindow {
    fn window_id(&self) -> u64 {
        self.window_id
    }

    fn is_destroyed(&self) -> bool {
        false
    }

    fn send_terminal_output(&self, _task_id: &str, _tab_id: Option<&str>, _output: &str) {}

    fn send_terminal_closed(&self, _task_id: &str, _tab_id: Option<&str>, _reason: Option<&str>) {}
}

struct Harness {
    bridge: Arc<TerminalBridge>,
    log: Arc<CallLog>,
    sessions: Arc<FakeSessions>,
    store: Arc<FakeStore>,
    dependency_check: Arc<FakeDependencyCheck>,
    hooks: Arc<FakeHooks>,
    image_transfer: Arc<FakeImageTransfer>,
    events: Arc<FakeEvents>,
    _home: TempDir,
}

fn harness() -> Harness {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".ssh")).unwrap();
    std::fs::write(
        home.path().join(".ssh/config"),
        "Host app-prod\n  HostName example.com\n  Port 2202\n  User tester\n  IdentityFile /tmp/test-key\n",
    )
    .unwrap();
    let log = Arc::new(CallLog::default());
    let clock = Arc::new(FakeClock {
        started_at: Instant::now(),
        offset: Mutex::new(Duration::ZERO),
    });
    let sessions = Arc::new(FakeSessions {
        log: log.clone(),
        clock: clock.clone(),
        remote_attach_ms: Mutex::new(0),
        attached_clients: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });
    let store = Arc::new(FakeStore::default());
    store
        .tasks
        .lock()
        .unwrap()
        .insert("task-1".to_string(), remote_task());
    let dependency_check = Arc::new(FakeDependencyCheck {
        log: log.clone(),
        clock: clock.clone(),
        elapsed_ms: Mutex::new(0),
        next_failure: Mutex::new(None),
    });
    let hooks = Arc::new(FakeHooks {
        log: log.clone(),
        clock: clock.clone(),
        elapsed_ms: Mutex::new(0),
        calls: Mutex::new(Vec::new()),
        next_failure: Mutex::new(None),
        gate: Mutex::new(None),
    });
    let image_transfer = Arc::new(FakeImageTransfer::default());
    let events = Arc::new(FakeEvents::default());
    let bridge = TerminalBridge::new(TerminalBridgeDependencies {
        sessions: sessions.clone(),
        store: store.clone(),
        ssh_hosts: Arc::new(SshConfigReader::new(home.path().to_str().unwrap())),
        session_dependencies: dependency_check.clone(),
        transport_cooldowns: Arc::new(FakeCooldowns { log: log.clone() }),
        hooks: hooks.clone(),
        image_transfer: image_transfer.clone(),
        events: events.clone(),
        clock: Arc::new(move || clock.now()),
    });
    Harness {
        bridge: Arc::new(bridge),
        log,
        sessions,
        store,
        dependency_check,
        hooks,
        image_transfer,
        events,
        _home: home,
    }
}

fn task(
    id: &str,
    ssh_host: Option<&str>,
    session_type: SessionType,
    session_name: &str,
) -> KanbanTask {
    KanbanTask {
        id: id.to_string(),
        title: "existing task".to_string(),
        description: None,
        status: TaskStatus::Todo,
        branch_name: None,
        worktree_path: None,
        session_type: Some(session_type),
        session_name: Some(session_name.to_string()),
        ssh_host: ssh_host.map(str::to_string),
        agent_type: None,
        project_id: None,
        base_branch: None,
        pr_url: None,
        priority: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn remote_task() -> KanbanTask {
    KanbanTask {
        worktree_path: Some("/remote/repo".to_string()),
        ..task("task-1", Some("app-prod"), SessionType::Tmux, "existing")
    }
}

fn app_prod_config() -> SshHostConfig {
    SshHostConfig {
        host: "app-prod".to_string(),
        hostname: "example.com".to_string(),
        port: 2202,
        username: "tester".to_string(),
        private_key_path: "/tmp/test-key".to_string(),
    }
}

fn layout_with_initial_command() -> PaneLayoutSelection {
    PaneLayoutSelection {
        layout_type: PaneLayoutType::Single,
        panes: vec![PaneCommand {
            position: 0,
            command: "codex".to_string(),
        }],
    }
}

fn callback_client() -> Arc<dyn TerminalClient> {
    Arc::new(CallbackTerminalClient::new(|_| {}))
}

fn window(window_id: u64) -> Arc<dyn TerminalWindow> {
    Arc::new(FakeWindow { window_id })
}

fn wait_until(description: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "기다리다 시간이 지났다: {description}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn retry_hint(milliseconds: u32) -> Option<NonZeroU32> {
    NonZeroU32::new(milliseconds)
}

fn open_in_background(
    harness: &Harness,
    window_id: u64,
    task_id: &str,
) -> mpsc::Receiver<TerminalOpenResult> {
    let (sender, receiver) = mpsc::channel();
    let bridge = harness.bridge.clone();
    let task_id = task_id.to_string();
    std::thread::spawn(move || {
        let _ = sender.send(bridge.open_terminal(window(window_id), &task_id, None, 100, 30));
    });
    receiver
}

// parity: TS-main-terminalBridge-b28e73
#[test]
fn paste_into_a_local_session_fails_without_transferring() {
    let harness = harness();
    let local_task = task("task-1", None, SessionType::Tmux, "existing");
    harness
        .store
        .tasks
        .lock()
        .unwrap()
        .insert("task-1".to_string(), local_task);

    let result = harness
        .bridge
        .paste_image_to_remote_terminal("task-1", "data:image/png;base64,aGVsbG8=");

    assert_eq!(result, Err("원격 세션이 아닙니다.".to_string()));
    assert!(harness.image_transfer.transfers.lock().unwrap().is_empty());
}

// parity: TS-main-terminalBridge-d8880c
#[test]
fn paste_fails_when_the_task_ssh_host_has_no_parsed_config() {
    let harness = harness();
    let unknown_host_task = task(
        "task-1",
        Some("unknown-host"),
        SessionType::Tmux,
        "existing",
    );
    harness
        .store
        .tasks
        .lock()
        .unwrap()
        .insert("task-1".to_string(), unknown_host_task);

    let result = harness
        .bridge
        .paste_image_to_remote_terminal("task-1", "data:image/png;base64,aGVsbG8=");

    assert_eq!(
        result,
        Err("SSH 호스트를 찾을 수 없습니다: unknown-host".to_string())
    );
}

// parity: TS-main-terminalBridge-a53a40
#[test]
fn paste_transfers_the_decoded_image_and_returns_the_remote_path() {
    let harness = harness();
    *harness.image_transfer.transfer_result.lock().unwrap() =
        Some(Ok("/tmp/kanvibe-paste-fixed-uuid.png".to_string()));

    let result = harness
        .bridge
        .paste_image_to_remote_terminal("task-1", "data:image/png;base64,aGVsbG8=");

    assert_eq!(result, Ok("/tmp/kanvibe-paste-fixed-uuid.png".to_string()));
    assert_eq!(
        *harness.image_transfer.transfers.lock().unwrap(),
        vec![(app_prod_config(), b"hello".to_vec())]
    );
}

// parity: TS-main-terminalBridge-670892
#[test]
fn paste_returns_the_scp_failure_message() {
    let harness = harness();
    *harness.image_transfer.transfer_result.lock().unwrap() =
        Some(Err("scp 전송 실패 (exit 1): Permission denied".to_string()));

    let result = harness
        .bridge
        .paste_image_to_remote_terminal("task-1", "data:image/png;base64,aGVsbG8=");

    assert_eq!(
        result,
        Err("scp 전송 실패 (exit 1): Permission denied".to_string())
    );
}

fn hold_task_lookup(harness: &Harness) -> Arc<Gate> {
    let local_task = task("task", None, SessionType::Terminal, "session");
    harness
        .store
        .tasks
        .lock()
        .unwrap()
        .insert("task".to_string(), local_task);
    let gate = Arc::new(Gate::default());
    *harness.store.task_gate.lock().unwrap() = Some(gate.clone());
    gate
}

// parity: TS-main-terminalBridge-8b13cb
#[test]
fn overlapping_opens_share_one_attach_and_balance_references() {
    let harness = harness();
    let gate = hold_task_lookup(&harness);
    let first = open_in_background(&harness, 100, "task");
    gate.wait_entered();
    let second = open_in_background(&harness, 100, "task");

    gate.release();

    assert_eq!(first.recv_timeout(Duration::from_secs(5)).unwrap(), Ok(()));
    assert_eq!(second.recv_timeout(Duration::from_secs(5)).unwrap(), Ok(()));
    assert_eq!(harness.log.count("attach_local"), 1);
    let client = harness.sessions.attached_clients.lock().unwrap()[0].clone();
    harness.bridge.close_terminal(100, "task", None);
    assert!(client.is_open());
    harness.bridge.close_window_terminals(100);
    assert!(!client.is_open());
}

// parity: TS-main-terminalBridge-e472c9
#[test]
fn closing_or_destroying_the_window_while_open_is_pending_cancels_before_attach() {
    for is_window_destroyed in [false, true] {
        let harness = harness();
        let gate = hold_task_lookup(&harness);
        let pending = open_in_background(&harness, 100, "task");
        gate.wait_entered();

        if is_window_destroyed {
            harness.bridge.close_window_terminals(100);
        } else {
            harness.bridge.close_terminal(100, "task", None);
        }
        gate.release();

        let result = pending.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(result, Err(TerminalBridgeError::new("터미널 연결 실패")));
        assert_eq!(harness.log.count("attach_local"), 0);
    }
}

// parity: TS-main-terminalBridge-430518
#[test]
fn failed_preparation_allows_a_retry() {
    let harness = harness();
    let local_task = task("task", None, SessionType::Terminal, "session");
    harness
        .store
        .tasks
        .lock()
        .unwrap()
        .insert("task".to_string(), local_task);
    *harness.store.next_task_failure.lock().unwrap() = Some("database unavailable".to_string());

    let failed = harness
        .bridge
        .open_terminal(window(100), "task", None, 100, 30);
    let retried = harness
        .bridge
        .open_terminal(window(100), "task", None, 100, 30);

    assert_eq!(
        failed,
        Err(TerminalBridgeError::new("database unavailable"))
    );
    assert_eq!(retried, Ok(()));
    harness.bridge.close_window_terminals(100);
}

// parity: TS-main-terminalBridge-f869d8
#[test]
fn remaining_ssh_cooldown_is_forwarded_as_a_retry_hint() {
    let harness = harness();
    *harness.dependency_check.next_failure.lock().unwrap() = Some(ExecGitError {
        retry_after_ms: retry_hint(3200),
        ..ExecGitError::new("SSH transport cooldown")
    });

    let result = harness
        .bridge
        .open_terminal(window(100), "task-1", None, 100, 30);

    assert_eq!(
        result,
        Err(TerminalBridgeError {
            message: "SSH transport cooldown".to_string(),
            retry_after_ms: retry_hint(3200),
        })
    );
}

// parity: TS-main-terminalBridge-ef696f
#[test]
fn failure_without_a_valid_retry_hint_is_not_retried() {
    let harness = harness();
    *harness.dependency_check.next_failure.lock().unwrap() =
        Some(ExecGitError::new("permanent failure"));

    let result = harness
        .bridge
        .open_terminal(window(100), "task-1", None, 100, 30);

    assert_eq!(result, Err(TerminalBridgeError::new("permanent failure")));
}

// parity: TS-main-terminalBridge-0eb0f0
#[test]
fn shared_attach_checks_remote_hooks_in_the_background_after_attaching() {
    let harness = harness();

    harness
        .bridge
        .attach_task_terminal(&remote_task(), None, callback_client(), None, None)
        .unwrap();

    wait_until("hook을 확인했다", || harness.log.count("hooks") == 1);
    assert_eq!(
        *harness.hooks.calls.lock().unwrap(),
        vec![(
            "/remote/repo".to_string(),
            "task-1".to_string(),
            Some("app-prod".to_string())
        )]
    );
    assert!(harness.log.position("attach_remote") < harness.log.position("hooks"));
}

// parity: TS-main-terminalBridge-16a1bd
#[test]
fn local_attach_also_checks_hooks_in_the_background_to_replace_tokenless_hooks() {
    let harness = harness();
    let local_task = KanbanTask {
        ssh_host: None,
        worktree_path: Some("/local/repo__worktrees/feat".to_string()),
        ..remote_task()
    };

    harness
        .bridge
        .attach_task_terminal(&local_task, None, callback_client(), None, None)
        .unwrap();

    wait_until("hook을 확인했다", || harness.log.count("hooks") == 1);
    assert_eq!(
        *harness.hooks.calls.lock().unwrap(),
        vec![(
            "/local/repo__worktrees/feat".to_string(),
            "task-1".to_string(),
            None
        )]
    );
    assert!(harness.log.position("attach_local") < harness.log.position("hooks"));
}

// parity: TS-main-terminalBridge-ff3e98
#[test]
fn remote_attach_clears_the_ssh_cooldown_before_any_remote_command() {
    let harness = harness();

    harness
        .bridge
        .attach_task_terminal(&remote_task(), None, callback_client(), None, None)
        .unwrap();

    assert!(harness.log.position("cooldown:app-prod") < harness.log.position("dependency"));
}

// parity: TS-main-terminalBridge-464da5
#[test]
fn default_branch_session_without_worktree_checks_the_project_path_hooks() {
    let harness = harness();
    harness
        .store
        .project_repo_paths
        .lock()
        .unwrap()
        .insert("project-1".to_string(), "/remote/root".to_string());
    let default_branch_task = KanbanTask {
        worktree_path: None,
        project_id: Some("project-1".to_string()),
        ..remote_task()
    };

    harness
        .bridge
        .attach_task_terminal(&default_branch_task, None, callback_client(), None, None)
        .unwrap();

    wait_until("hook을 확인했다", || harness.log.count("hooks") == 1);
    assert_eq!(
        *harness.store.project_lookups.lock().unwrap(),
        vec!["project-1"]
    );
    assert_eq!(harness.hooks.calls.lock().unwrap()[0].0, "/remote/root");
}

// parity: TS-main-terminalBridge-15eba4
#[test]
fn background_hook_failure_is_reported_but_keeps_the_terminal_open_and_retries_next_time() {
    let harness = harness();
    *harness.hooks.next_failure.lock().unwrap() =
        Some(HookInstallError::Message("hooks write denied".to_string()).into());
    let client = callback_client();

    let attached =
        harness
            .bridge
            .attach_task_terminal(&remote_task(), None, client.clone(), None, None);

    assert!(attached.is_ok());
    wait_until("실패를 알렸다", || {
        !harness.events.install_failures.lock().unwrap().is_empty()
    });
    assert!(client.is_open());
    assert_eq!(
        *harness.events.install_failures.lock().unwrap(),
        vec![TaskHookInstallFailure {
            task_id: "task-1".to_string(),
            task_title: "existing task".to_string(),
            error: "hooks write denied".to_string(),
        }]
    );
    assert_eq!(
        *harness.events.check_failures.lock().unwrap(),
        vec![TerminalHookCheckFailure {
            task_id: "task-1".to_string(),
            task_title: "existing task".to_string(),
            target_path: Some("/remote/repo".to_string()),
            ssh_host: Some("app-prod".to_string()),
            error: "hooks write denied".to_string(),
        }]
    );
    assert_eq!(harness.log.count("attach_remote"), 1);
    harness
        .bridge
        .attach_task_terminal(&remote_task(), None, client, None, None)
        .unwrap();
    assert_eq!(harness.log.count("attach_remote"), 2);
}

// parity: TS-main-terminalBridge-6be605
#[test]
fn transient_ssh_reconnect_wait_is_not_reported_as_a_hook_install_failure() {
    let harness = harness();
    let cooldown = HookInstallError::Exec(ExecGitError {
        retry_after_ms: retry_hint(3200),
        ..ExecGitError::new("SSH transport cooldown")
    });
    *harness.hooks.next_failure.lock().unwrap() = Some(cooldown.into());

    let result = harness
        .bridge
        .open_terminal(window(101), "task-1", None, 100, 30);

    assert_eq!(result, Ok(()));
    wait_until("hook 확인이 끝났다", || {
        !harness.events.check_failures.lock().unwrap().is_empty()
    });
    std::thread::sleep(Duration::from_millis(50));
    assert!(harness.events.install_failures.lock().unwrap().is_empty());
}

// parity: TS-main-terminalBridge-69cd97
#[test]
fn attach_and_terminal_input_work_while_the_hook_check_is_delayed() {
    let harness = harness();
    let gate = Arc::new(Gate::default());
    *harness.hooks.gate.lock().unwrap() = Some(gate.clone());

    let opened = open_in_background(&harness, 102, "task-1");

    assert_eq!(
        opened.recv_timeout(Duration::from_millis(200)).unwrap(),
        Ok(())
    );
    assert_eq!(harness.log.count("attach_remote"), 1);
    harness.bridge.write_terminal(102, "task-1", None, "pwd\r");
    let messages = harness.sessions.messages.lock().unwrap().clone();
    assert_eq!(
        messages
            .iter()
            .map(|(_, message)| message.as_str())
            .collect::<Vec<_>>(),
        vec!["pwd\r"]
    );
    gate.release();
}

// parity: TS-main-terminalBridge-f6fa84
#[test]
fn initial_pane_commands_enter_the_attach_path_only_after_hooks_are_ready() {
    for session_type in [SessionType::Tmux, SessionType::Herdr] {
        let harness = harness();
        let gate = Arc::new(Gate::default());
        *harness.hooks.gate.lock().unwrap() = Some(gate.clone());
        *harness.store.pane_layout.lock().unwrap() = Some(layout_with_initial_command());
        let bridge = harness.bridge.clone();
        let target_task = KanbanTask {
            session_type: Some(session_type),
            ..remote_task()
        };
        let pending = std::thread::spawn(move || {
            bridge.attach_task_terminal(&target_task, None, callback_client(), None, None)
        });

        gate.wait_entered();
        assert_eq!(harness.log.count("attach_remote"), 0);
        gate.release();

        pending.join().unwrap().unwrap();
        assert_eq!(harness.log.count("attach_remote"), 1);
    }
}

// parity: TS-main-terminalBridge-6c68fe
#[test]
fn remote_connection_logs_dependency_hook_and_attach_phase_durations() {
    let harness = harness();
    *harness.store.pane_layout.lock().unwrap() = Some(layout_with_initial_command());
    *harness.dependency_check.elapsed_ms.lock().unwrap() = 300;
    *harness.hooks.elapsed_ms.lock().unwrap() = 4_000;
    *harness.sessions.remote_attach_ms.lock().unwrap() = 1_200;

    harness
        .bridge
        .attach_task_terminal(&remote_task(), None, callback_client(), None, None)
        .unwrap();

    assert_eq!(
        *harness.events.phases.lock().unwrap(),
        vec![RemoteConnectionPhases {
            task_id: "task-1".to_string(),
            ssh_host: "app-prod".to_string(),
            session_type: SessionType::Tmux,
            total_ms: 5_500,
            dependency_check_ms: 300,
            hooks_before_attach_ms: Some(4_000),
            session_attach_ms: 1_200,
        }]
    );
}

// parity: TS-main-terminalBridge-8de94b
#[test]
fn hook_preparation_failure_is_reported_and_the_terminal_still_attaches() {
    let harness = harness();
    *harness.store.pane_layout.lock().unwrap() = Some(layout_with_initial_command());
    *harness.hooks.next_failure.lock().unwrap() =
        Some(HookInstallError::Message("hook unavailable".to_string()).into());

    let attached =
        harness
            .bridge
            .attach_task_terminal(&remote_task(), None, callback_client(), None, None);

    assert!(attached.is_ok());
    assert_eq!(harness.log.count("attach_remote"), 1);
    assert!(harness.log.position("hooks") < harness.log.position("attach_remote"));
    assert_eq!(harness.log.count("hooks"), 1);
    assert_eq!(
        *harness.events.install_failures.lock().unwrap(),
        vec![TaskHookInstallFailure {
            task_id: "task-1".to_string(),
            task_title: "existing task".to_string(),
            error: "hook unavailable".to_string(),
        }]
    );
}

#[test]
fn callback_client_sends_the_screen_before_output_held_during_attach() {
    let chunks = Arc::new(Mutex::new(Vec::new()));
    let recorded_chunks = chunks.clone();
    let client = CallbackTerminalClient::new(move |chunk| {
        recorded_chunks.lock().unwrap().push(chunk.to_string());
    });

    client.hold_output();
    client.send("held-1");
    client.send("held-2");
    client.release_held_output("screen");
    client.send("live");

    assert_eq!(
        *chunks.lock().unwrap(),
        ["screen", "held-1", "held-2", "live"]
    );
}
