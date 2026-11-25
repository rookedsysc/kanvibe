//! 미러 테스트용 가짜 협력자. 실제 tmux·zellij·Herdr·ssh는 띄우지 않는다.
//! 터미널 레지스트리는 진짜를 쓰고 PTY만 가짜로 바꾼다(TS 테스트의 `node-pty` mock과 같다).

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use kanvibe_domain::task::{KanbanTask, SessionType, TaskStatus};
use kanvibe_process::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};
use kanvibe_process::worktree::PaneLayoutSelection;
use kanvibe_terminal::herdr::{HerdrMirrorPaneOptions, HerdrPaneExtent, HerdrPaneSize};
use kanvibe_terminal::pane_mirror::MirrorPane;
use kanvibe_terminal::pty::{PtyEventCallbacks, PtyExit, PtyProcess, PtySpawnRequest, PtySpawner};
use kanvibe_terminal::remote_terminal_protocol::{RemoteTerminalGrant, RemoteTerminalGrantInput};
use kanvibe_terminal::terminal_registry::{
    HerdrClientRelay, HerdrClientRelayFactory, HerdrPaneTab, HerdrSessionError, HerdrSessions,
    HerdrTabBarPosition, LocalAttachRequest, RemoteHostConnection, RemoteHostConnections,
    TerminalAttachment, TerminalClient, TerminalEventSink, TerminalRegistry,
};
use kanvibe_terminal::terminal_tabs::{
    build_tmux_new_window_command, build_zellij_new_tab_command,
};

use crate::ports::{
    HerdrMirrorQueries, MirrorProcessSpawner, MirrorTaskStore, MobileTerminalTabs, RelayClose,
    RelayOutput, RemotePaneGrants, RemotePaneRelay, TaskTerminalAttacher,
};
use crate::process::{MirrorProcess, MirrorProcessControl};

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

/// 열기 전까지 들어온 호출을 붙잡아 두는 문
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

pub fn task(id: &str, session_type: Option<SessionType>, session_name: Option<&str>) -> KanbanTask {
    KanbanTask {
        id: id.to_string(),
        title: "task".to_string(),
        description: None,
        status: TaskStatus::Progress,
        branch_name: None,
        worktree_path: None,
        session_type,
        session_name: session_name.map(str::to_string),
        ssh_host: None,
        agent_type: None,
        project_id: None,
        base_branch: None,
        pr_url: None,
        priority: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

type Responder = dyn Fn(&str, Option<&str>) -> Option<Result<String, String>> + Send + Sync;

/// `exec_git` 가짜. 명령을 기록하고, 뒤에 등록한 응답기부터 결과를 정한다. 아무도 답하지 않으면 빈 출력으로 성공한다
#[derive(Default)]
pub struct ScriptedRunner {
    calls: Mutex<Vec<(String, Option<String>)>>,
    responders: Mutex<Vec<Arc<Responder>>>,
}

impl ScriptedRunner {
    pub fn respond(
        &self,
        responder: impl Fn(&str, Option<&str>) -> Option<Result<String, String>> + Send + Sync + 'static,
    ) {
        self.responders.lock().unwrap().push(Arc::new(responder));
    }

    /// 앞서 등록한 응답기를 모두 지우고 하나로 바꾼다(vitest `mockImplementation`)
    pub fn replace(
        &self,
        responder: impl Fn(&str, Option<&str>) -> Option<Result<String, String>> + Send + Sync + 'static,
    ) {
        *self.responders.lock().unwrap() = vec![Arc::new(responder)];
    }

    pub fn calls(&self) -> Vec<(String, Option<String>)> {
        self.calls.lock().unwrap().clone()
    }

    pub fn commands(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .map(|(command, _)| command)
            .collect()
    }

    pub fn commands_containing(&self, needle: &str) -> Vec<String> {
        self.commands()
            .into_iter()
            .filter(|command| command.contains(needle))
            .collect()
    }
}

impl ShellCommandRunner for ScriptedRunner {
    fn exec_git(
        &self,
        command: &str,
        ssh_host: Option<&str>,
        _options: &ExecGitOptions,
    ) -> Result<String, ExecGitError> {
        self.calls
            .lock()
            .unwrap()
            .push((command.to_string(), ssh_host.map(str::to_string)));
        let responders: Vec<Arc<Responder>> = self.responders.lock().unwrap().clone();
        for responder in responders.iter().rev() {
            if let Some(result) = responder(command, ssh_host) {
                return result.map_err(ExecGitError::new);
            }
        }
        Ok(String::new())
    }
}

/// 태스크 저장소 가짜. TS `findOneBy` mock처럼 어떤 ID에도 같은 태스크를 돌려준다
#[derive(Default)]
pub struct FakeTasks {
    pub task: Mutex<Option<KanbanTask>>,
    pub default_session_type: Mutex<Option<SessionType>>,
    pub connect_results: Mutex<VecDeque<Option<KanbanTask>>>,
    pub connect_calls: Mutex<Vec<(String, SessionType)>>,
    pub connect_gate: Mutex<Option<Arc<Gate>>>,
    /// 한 번만 막는 조회 문
    pub find_gate: Mutex<Option<Arc<Gate>>>,
}

impl FakeTasks {
    pub fn set(&self, task: Option<KanbanTask>) {
        *self.task.lock().unwrap() = task;
    }

    pub fn connect_calls(&self) -> Vec<(String, SessionType)> {
        self.connect_calls.lock().unwrap().clone()
    }
}

impl MirrorTaskStore for FakeTasks {
    fn find_task(&self, _task_id: &str) -> Result<Option<KanbanTask>, String> {
        let gate = self.find_gate.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.pass();
        }
        Ok(self.task.lock().unwrap().clone())
    }

    fn default_session_type(&self) -> Result<SessionType, String> {
        Ok(self
            .default_session_type
            .lock()
            .unwrap()
            .unwrap_or(SessionType::Tmux))
    }

    fn connect_terminal_session(
        &self,
        task_id: &str,
        session_type: SessionType,
    ) -> Result<Option<KanbanTask>, String> {
        self.connect_calls
            .lock()
            .unwrap()
            .push((task_id.to_string(), session_type));
        let gate = self.connect_gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.pass();
        }
        Ok(self.connect_results.lock().unwrap().pop_front().flatten())
    }
}

/// Herdr 조회 가짜. pane 목록과 이력 줄 수를 테스트가 정한다
pub struct FakeHerdr {
    pub panes: Mutex<Vec<MirrorPane>>,
    pub list_calls: Mutex<Vec<HerdrMirrorPaneOptions>>,
    pub history_rows: Mutex<i64>,
    pub extent_failure: Mutex<Option<String>>,
    pub extent_calls: AtomicUsize,
    pub pane_size: Mutex<Option<HerdrPaneSize>>,
    pub size_calls: Mutex<Vec<String>>,
    pub is_alive: AtomicBool,
}

impl Default for FakeHerdr {
    fn default() -> Self {
        Self {
            panes: Mutex::new(vec![herdr_pane("w1:p1", "w1:t1", "claude", 0, 120)]),
            list_calls: Mutex::default(),
            history_rows: Mutex::new(0),
            extent_failure: Mutex::default(),
            extent_calls: AtomicUsize::new(0),
            pane_size: Mutex::new(Some(HerdrPaneSize {
                cols: 120,
                rows: 38,
            })),
            size_calls: Mutex::default(),
            is_alive: AtomicBool::new(true),
        }
    }
}

pub fn herdr_pane(id: &str, tab_id: &str, command: &str, left: i64, width: i64) -> MirrorPane {
    MirrorPane {
        id: id.to_string(),
        tab_id: tab_id.to_string(),
        tab_name: "work".to_string(),
        command: command.to_string(),
        left,
        top: 0,
        width,
        height: 38,
    }
}

impl FakeHerdr {
    pub fn list_calls(&self) -> Vec<HerdrMirrorPaneOptions> {
        self.list_calls.lock().unwrap().clone()
    }
}

impl HerdrMirrorQueries for FakeHerdr {
    fn list_mirror_panes(
        &self,
        _session_name: &str,
        _ssh_host: Option<&str>,
        options: &HerdrMirrorPaneOptions,
    ) -> Result<Vec<MirrorPane>, String> {
        self.list_calls.lock().unwrap().push(options.clone());
        let panes = self.panes.lock().unwrap().clone();
        // 최소 목록은 실행 명령을 읽지 않는다
        Ok(panes
            .into_iter()
            .map(|mut pane| {
                if options.is_minimal {
                    pane.command.clear();
                }
                pane
            })
            .collect())
    }

    fn read_pane_extent(
        &self,
        _session_name: &str,
        _pane_id: &str,
        _ssh_host: Option<&str>,
    ) -> Result<HerdrPaneExtent, String> {
        self.extent_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(failure) = self.extent_failure.lock().unwrap().clone() {
            return Err(failure);
        }
        Ok(HerdrPaneExtent {
            history_rows: *self.history_rows.lock().unwrap(),
            viewport_rows: 38,
        })
    }

    fn read_task_pane_size(
        &self,
        _session_name: &str,
        pane_id: &str,
        _ssh_host: Option<&str>,
    ) -> Result<Option<HerdrPaneSize>, String> {
        self.size_calls.lock().unwrap().push(pane_id.to_string());
        let is_known = self
            .panes
            .lock()
            .unwrap()
            .iter()
            .any(|pane| pane.id == pane_id);
        Ok(self.pane_size.lock().unwrap().filter(|_| is_known))
    }

    fn is_task_alive(&self, _session_name: &str, _ssh_host: Option<&str>) -> bool {
        self.is_alive.load(Ordering::SeqCst)
    }
}

pub type AttachOverride = dyn Fn(
        &KanbanTask,
        Option<&str>,
        Arc<dyn TerminalClient>,
    ) -> Result<Option<TerminalAttachment>, String>
    + Send
    + Sync;

/// attach 호출 하나: 태스크 ID, 탭 ID, 클라이언트, 열, 행
pub type AttachCall = (
    String,
    Option<String>,
    Arc<dyn TerminalClient>,
    Option<u16>,
    Option<u16>,
);

/// 세션에 붙는 길. 기본은 진짜 레지스트리의 로컬 attach를 지나고(TS `realAttachTaskTerminal`), 테스트가 갈아 끼울 수 있다
pub struct FakeAttacher {
    pub registry: Mutex<Option<Arc<TerminalRegistry>>>,
    pub calls: Mutex<Vec<AttachCall>>,
    pub override_attach: Mutex<Option<Arc<AttachOverride>>>,
}

impl FakeAttacher {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            registry: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
            override_attach: Mutex::new(None),
        })
    }

    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    pub fn set_override(
        &self,
        attach: impl Fn(
            &KanbanTask,
            Option<&str>,
            Arc<dyn TerminalClient>,
        ) -> Result<Option<TerminalAttachment>, String>
        + Send
        + Sync
        + 'static,
    ) {
        *self.override_attach.lock().unwrap() = Some(Arc::new(attach));
    }
}

impl TaskTerminalAttacher for FakeAttacher {
    fn attach_task_terminal(
        &self,
        task: &KanbanTask,
        tab_id: Option<&str>,
        client: Arc<dyn TerminalClient>,
        columns: Option<u16>,
        rows: Option<u16>,
    ) -> Result<Option<TerminalAttachment>, String> {
        self.calls.lock().unwrap().push((
            task.id.clone(),
            tab_id.map(str::to_string),
            Arc::clone(&client),
            columns,
            rows,
        ));
        let override_attach = self.override_attach.lock().unwrap().clone();
        if let Some(attach) = override_attach {
            return attach(task, tab_id, client);
        }
        let registry = self.registry.lock().unwrap().clone().expect("레지스트리");
        let request = LocalAttachRequest {
            task_id: task.id.clone(),
            tab_id: tab_id.map(str::to_string),
            session_type: task.session_type.expect("세션 종류"),
            session_name: task.session_name.clone().unwrap_or_default(),
            working_directory: task.worktree_path.clone(),
            columns,
            rows,
            pane_layout: None::<PaneLayoutSelection>,
        };
        registry.attach_local_session(request, client)
    }
}

/// 탭 바의 새 탭 만들기. 탭 서비스와 같은 명령 빌더를 지난다
pub struct FakeTabs {
    pub tasks: Arc<FakeTasks>,
    pub runner: Arc<ScriptedRunner>,
    pub registry: Mutex<Option<Arc<TerminalRegistry>>>,
}

impl MobileTerminalTabs for FakeTabs {
    fn create_terminal_tab(&self, task_id: &str) -> Result<(), String> {
        let task = self
            .tasks
            .task
            .lock()
            .unwrap()
            .clone()
            .ok_or("작업에 연결된 세션이 없습니다.")?;
        let session_name = task.session_name.clone().unwrap_or_default();
        let worktree = task.worktree_path.as_deref();
        let options = ExecGitOptions::default();
        let command = match task.session_type {
            Some(SessionType::Terminal) => {
                let registry = self.registry.lock().unwrap().clone().expect("레지스트리");
                registry.create_local_terminal_tab(task_id);
                return Ok(());
            }
            Some(SessionType::Zellij) => build_zellij_new_tab_command(&session_name, worktree),
            _ => build_tmux_new_window_command(&session_name, worktree),
        };
        self.runner
            .exec_git(&command, task.ssh_host.as_deref(), &options)
            .map(|_| ())
            .map_err(|error| error.message)
    }
}

type StdinLineHook = dyn Fn(&FakeChild, &str) + Send + Sync;

/// 띄운 미러 프로세스 하나. 테스트가 출력을 흘리고 끝낼 수 있다
pub struct FakeChild {
    pub command: String,
    output: Mutex<Option<Sender<Option<Vec<u8>>>>>,
    pub stdin_lines: Mutex<Vec<String>>,
    stdin_buffer: Mutex<String>,
    pub is_stdin_ended: AtomicBool,
    pub terminate_count: AtomicUsize,
    exited: Mutex<bool>,
    exit_signal: Condvar,
    /// stdin 한 줄을 받았을 때. Herdr control의 크기 응답을 흉내 낸다
    on_stdin_line: Mutex<Option<Arc<StdinLineHook>>>,
    /// stdin이 닫히면 끝난다(Herdr control의 `terminal.release`)
    should_exit_on_stdin_end: AtomicBool,
}

impl FakeChild {
    pub fn emit(&self, text: &str) {
        if let Some(sender) = self.output.lock().unwrap().as_ref() {
            let _ = sender.send(Some(text.as_bytes().to_vec()));
        }
    }

    /// 출력 읽기가 실패하고 끝난다(TS `child.emit("error")`)
    pub fn fail(&self) {
        if let Some(sender) = self.output.lock().unwrap().as_ref() {
            let _ = sender.send(None);
        }
        self.exit();
    }

    /// 출력을 닫고 끝낸다(TS `child.emit("exit")`)
    pub fn exit(&self) {
        *self.exited.lock().unwrap() = true;
        self.exit_signal.notify_all();
        self.output.lock().unwrap().take();
    }

    pub fn terminate_count(&self) -> usize {
        self.terminate_count.load(Ordering::SeqCst)
    }

    pub fn stdin_lines(&self) -> Vec<String> {
        self.stdin_lines.lock().unwrap().clone()
    }

    fn accept_stdin(&self, bytes: &[u8]) {
        let mut buffer = self.stdin_buffer.lock().unwrap();
        buffer.push_str(&String::from_utf8_lossy(bytes));
        while let Some(index) = buffer.find('\n') {
            let line: String = buffer.drain(..=index).collect();
            let line = line.trim_end_matches('\n').to_string();
            self.stdin_lines.lock().unwrap().push(line.clone());
            let hook = self.on_stdin_line.lock().unwrap().clone();
            if let Some(hook) = hook {
                hook(self, &line);
            }
        }
    }
}

struct ChannelReader {
    receiver: Receiver<Option<Vec<u8>>>,
    pending: Vec<u8>,
}

impl Read for ChannelReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.pending.is_empty() {
            match self.receiver.recv() {
                Ok(Some(bytes)) => self.pending = bytes,
                Ok(None) => return Err(std::io::Error::other("boom")),
                Err(_) => return Ok(0),
            }
        }
        let count = self.pending.len().min(buffer.len());
        buffer[..count].copy_from_slice(&self.pending[..count]);
        self.pending.drain(..count);
        Ok(count)
    }
}

struct FakeStdin(Arc<FakeChild>);

impl Write for FakeStdin {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.accept_stdin(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for FakeStdin {
    fn drop(&mut self) {
        self.0.is_stdin_ended.store(true, Ordering::SeqCst);
        if self.0.should_exit_on_stdin_end.load(Ordering::SeqCst) {
            self.0.exit();
        }
    }
}

struct FakeControl(Arc<FakeChild>);

impl MirrorProcessControl for FakeControl {
    fn terminate(&self) {
        self.0.terminate_count.fetch_add(1, Ordering::SeqCst);
    }

    fn wait_for_exit(&self) {
        let mut exited = self.0.exited.lock().unwrap();
        while !*exited {
            exited = self.0.exit_signal.wait(exited).unwrap();
        }
    }
}

/// 미러 프로세스 실행 가짜. Herdr control은 기본으로 요청 크기의 전체 프레임을 곧바로 돌려준다
#[derive(Default)]
pub struct FakeProcesses {
    children: Mutex<Vec<Arc<FakeChild>>>,
    pub spawn_failure: Mutex<Option<String>>,
    pub should_withhold_controller_ack: AtomicBool,
}

pub fn frame_record(text: &str, is_full: bool) -> String {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.encode(text);
    format!(
        "{}\n",
        serde_json::json!({ "type": "terminal.frame", "bytes": bytes, "full": is_full })
    )
}

pub fn sized_frame_record(columns: i64, rows: i64) -> String {
    format!(
        "{}\n",
        serde_json::json!({ "type": "terminal.frame", "bytes": "", "full": true, "width": columns, "height": rows })
    )
}

fn control_size(command: &str) -> Option<(i64, i64)> {
    let after_cols = command.split("'--cols' '").nth(1)?;
    let columns = after_cols.split('\'').next()?.parse().ok()?;
    let after_rows = command.split("'--rows' '").nth(1)?;
    let rows = after_rows.split('\'').next()?.parse().ok()?;
    Some((columns, rows))
}

fn acknowledge_resize(child: &FakeChild, line: &str) {
    let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    if record["type"] == "terminal.resize" {
        let columns = record["cols"].as_i64().unwrap_or_default();
        let rows = record["rows"].as_i64().unwrap_or_default();
        child.emit(&sized_frame_record(columns, rows));
    }
}

impl FakeProcesses {
    pub fn children(&self) -> Vec<Arc<FakeChild>> {
        self.children.lock().unwrap().clone()
    }

    pub fn count(&self) -> usize {
        self.children.lock().unwrap().len()
    }

    pub fn child(&self, index: usize) -> Arc<FakeChild> {
        Arc::clone(&self.children.lock().unwrap()[index])
    }

    pub fn matching(&self, needle: &str) -> Vec<Arc<FakeChild>> {
        self.children()
            .into_iter()
            .filter(|child| child.command.contains(needle))
            .collect()
    }

    pub fn commands_containing(&self, needle: &str) -> Vec<String> {
        self.matching(needle)
            .into_iter()
            .map(|child| child.command.clone())
            .collect()
    }

    fn create_child(&self, command: &str) -> (Arc<FakeChild>, Receiver<Option<Vec<u8>>>) {
        let (sender, receiver) = channel();
        let is_controller = command.contains("'session' 'control'");
        let child = Arc::new(FakeChild {
            command: command.to_string(),
            output: Mutex::new(Some(sender)),
            stdin_lines: Mutex::default(),
            stdin_buffer: Mutex::default(),
            is_stdin_ended: AtomicBool::new(false),
            terminate_count: AtomicUsize::new(0),
            exited: Mutex::new(false),
            exit_signal: Condvar::new(),
            on_stdin_line: Mutex::new(None),
            should_exit_on_stdin_end: AtomicBool::new(is_controller),
        });
        if is_controller && !self.should_withhold_controller_ack.load(Ordering::SeqCst) {
            *child.on_stdin_line.lock().unwrap() = Some(Arc::new(acknowledge_resize));
            if let Some((columns, rows)) = control_size(command) {
                child.emit(&sized_frame_record(columns, rows));
            }
        }
        (child, receiver)
    }
}

impl MirrorProcessSpawner for FakeProcesses {
    fn spawn(&self, command: &str, _ssh_host: Option<&str>) -> Result<MirrorProcess, String> {
        if let Some(failure) = self.spawn_failure.lock().unwrap().clone() {
            return Err(failure);
        }
        let (child, receiver) = self.create_child(command);
        self.children.lock().unwrap().push(Arc::clone(&child));
        Ok(MirrorProcess {
            stdout: Box::new(ChannelReader {
                receiver,
                pending: Vec::new(),
            }),
            stdin: Some(Box::new(FakeStdin(Arc::clone(&child)))),
            control: Arc::new(FakeControl(child)),
        })
    }
}

/// 원격 직접 연결 권한 가짜
#[derive(Default)]
pub struct FakeGrants {
    pub prepared: Mutex<Vec<RemoteTerminalGrantInput>>,
    pub prepare_result: Mutex<Option<RemoteTerminalGrant>>,
    pub prepare_gate: Mutex<Option<Arc<Gate>>>,
    pub attached: Mutex<Vec<(String, String, String, String)>>,
    pub revoked_grants: Mutex<Vec<String>>,
    pub revoked_devices: Mutex<Vec<String>>,
    pub revoked_tasks: Mutex<Vec<String>>,
    pub stop_count: AtomicUsize,
}

pub struct FakeRelay;

impl RemotePaneRelay for FakeRelay {
    fn write(&self, _input: &[u8], _is_binary: bool) {}

    fn close(&self) {}
}

impl RemotePaneGrants for FakeGrants {
    fn prepare_grant(
        &self,
        input: &RemoteTerminalGrantInput,
    ) -> Result<Option<RemoteTerminalGrant>, String> {
        self.prepared.lock().unwrap().push(input.clone());
        let gate = self.prepare_gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.pass();
        }
        Ok(self.prepare_result.lock().unwrap().clone())
    }

    fn attach_relay(
        &self,
        grant_id: &str,
        device_id: &str,
        task_id: &str,
        pane_id: &str,
        _on_output: RelayOutput,
        _on_close: RelayClose,
    ) -> Result<Box<dyn RemotePaneRelay>, String> {
        self.attached.lock().unwrap().push((
            grant_id.to_string(),
            device_id.to_string(),
            task_id.to_string(),
            pane_id.to_string(),
        ));
        Ok(Box::new(FakeRelay))
    }

    fn revoke_grant(&self, grant_id: &str) {
        self.revoked_grants
            .lock()
            .unwrap()
            .push(grant_id.to_string());
    }

    fn revoke_device(&self, device_id: &str) {
        self.revoked_devices
            .lock()
            .unwrap()
            .push(device_id.to_string());
    }

    fn revoke_task(&self, task_id: &str) {
        self.revoked_tasks.lock().unwrap().push(task_id.to_string());
    }

    fn stop_agents(&self) {
        self.stop_count.fetch_add(1, Ordering::SeqCst);
    }
}

pub type ResizeHook = dyn Fn(u16, u16) + Send + Sync;

/// 가짜 PTY. 크기·입력·종료를 기록하고 테스트가 출력과 종료를 흘린다
pub struct FakePty {
    pid: u32,
    size: Mutex<(u16, u16)>,
    pub writes: Mutex<Vec<String>>,
    pub resizes: Mutex<Vec<(u16, u16)>>,
    pub kill_count: AtomicUsize,
    callbacks: Mutex<Option<Arc<PtyEventCallbacks>>>,
    /// 크기가 바뀔 때 테스트가 알아야 하는 세션 상태(zellij 탭 크기)를 함께 고친다
    pub on_resize: Mutex<Option<Arc<ResizeHook>>>,
}

impl FakePty {
    pub fn emit_output(&self, data: &str) {
        let callbacks = self.callbacks.lock().unwrap().clone().expect("콜백");
        (callbacks.on_output)(data);
    }

    pub fn emit_exit(&self) {
        let callbacks = self.callbacks.lock().unwrap().clone().expect("콜백");
        (callbacks.on_exit)(PtyExit {
            exit_code: 0,
            signal: None,
            is_detached: false,
        });
    }

    pub fn resizes(&self) -> Vec<(u16, u16)> {
        self.resizes.lock().unwrap().clone()
    }

    pub fn writes(&self) -> Vec<String> {
        self.writes.lock().unwrap().clone()
    }

    pub fn size(&self) -> (u16, u16) {
        *self.size.lock().unwrap()
    }
}

impl PtyProcess for FakePty {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn foreground_process_name(&self) -> Option<String> {
        Some("zsh".to_string())
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
        let hook = self.on_resize.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook(columns, rows);
        }
        Ok(())
    }

    fn kill(&self) -> Result<(), String> {
        self.kill_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Default)]
pub struct FakePtySpawner {
    spawned: Mutex<Vec<Arc<FakePty>>>,
}

impl FakePtySpawner {
    pub fn count(&self) -> usize {
        self.spawned.lock().unwrap().len()
    }

    pub fn pty(&self, index: usize) -> Arc<FakePty> {
        Arc::clone(&self.spawned.lock().unwrap()[index])
    }
}

impl PtySpawner for FakePtySpawner {
    fn spawn(
        &self,
        request: PtySpawnRequest,
        callbacks: PtyEventCallbacks,
    ) -> Result<Arc<dyn PtyProcess>, String> {
        let mut spawned = self.spawned.lock().unwrap();
        let pty = Arc::new(FakePty {
            pid: 4000 + spawned.len() as u32,
            size: Mutex::new((request.columns, request.rows)),
            writes: Mutex::default(),
            resizes: Mutex::default(),
            kill_count: AtomicUsize::new(0),
            callbacks: Mutex::new(Some(Arc::new(callbacks))),
            on_resize: Mutex::new(None),
        });
        spawned.push(Arc::clone(&pty));
        Ok(pty)
    }
}

/// 레지스트리가 요구하지만 미러 테스트가 쓰지 않는 Herdr·원격 협력자
pub struct UnusedTerminalPeers;

impl HerdrSessions for UnusedTerminalPeers {
    fn ensure_session(
        &self,
        _session_name: &str,
        _working_directory: &str,
        _pane_layout: Option<&PaneLayoutSelection>,
        _ssh_host: Option<&str>,
    ) -> Result<Option<String>, HerdrSessionError> {
        Ok(None)
    }

    fn ensure_remote_machine(
        &self,
        _session_name: &str,
        _ssh_host: &str,
    ) -> Result<(), HerdrSessionError> {
        Ok(())
    }

    fn handoff_remote_server(
        &self,
        _herdr_session: &str,
        _ssh_host: &str,
    ) -> Result<(), HerdrSessionError> {
        Ok(())
    }

    fn client_socket_path(
        &self,
        _session_name: &str,
        _ssh_host: Option<&str>,
    ) -> Result<String, HerdrSessionError> {
        Ok(String::new())
    }

    fn client_tab_bar_position(&self, _ssh_host: Option<&str>) -> Option<HerdrTabBarPosition> {
        None
    }

    fn list_task_pane_tab_ids(
        &self,
        _session_name: &str,
        _ssh_host: Option<&str>,
    ) -> Result<Vec<HerdrPaneTab>, HerdrSessionError> {
        Ok(Vec::new())
    }
}

impl HerdrClientRelayFactory for UnusedTerminalPeers {
    fn create(
        &self,
        _upstream_socket_path: &str,
        _workspace_id: &str,
    ) -> Result<Arc<dyn HerdrClientRelay>, String> {
        Err("미러 테스트는 Herdr client를 띄우지 않는다".to_string())
    }
}

impl RemoteHostConnections for UnusedTerminalPeers {
    fn connection(&self, _ssh_host: &str) -> Option<Arc<dyn RemoteHostConnection>> {
        None
    }
}

impl TerminalEventSink for UnusedTerminalPeers {
    fn herdr_server_handoff_required(&self, _ssh_host: &str, _herdr_session: &str) {}

    fn herdr_machine_registration_failed(
        &self,
        _ssh_host: &str,
        _herdr_session: &str,
        _reason: &str,
    ) {
    }
}

/// 붙여 두기만 하는 데스크탑 클라이언트
#[derive(Default)]
pub struct RecordingClient {
    pub sent: Mutex<Vec<String>>,
    pub is_closed: AtomicBool,
}

impl TerminalClient for RecordingClient {
    fn is_open(&self) -> bool {
        !self.is_closed.load(Ordering::SeqCst)
    }

    fn send(&self, data: &str) {
        self.sent.lock().unwrap().push(data.to_string());
    }

    fn buffered_amount(&self) -> usize {
        0
    }

    fn close(&self, _closure: Option<kanvibe_terminal::terminal_registry::ClientClosure>) {
        self.is_closed.store(true, Ordering::SeqCst);
    }
}
