//! 보드 서비스 테스트의 가짜 협력 객체. TS 테스트의 `mocks` 자리이며, 호출 순서를 하나의 기록에 모은다.
//! 공유 상태 파일은 실제 `KanvibeStateWriters`를 쓰고 파일 쓰기만 기록하는 저장소로 바꿔, 어느 경로에 무엇을 쓰는지 확인한다.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub(super) use crate::test_support::wait_until;

use kanvibe_db::Database;
use kanvibe_domain::task::{SessionType, TaskPriority, TaskStatus};
use kanvibe_process::exec::ExecGitError;
use kanvibe_process::host_file_access::{HostFileAccess, HostFileError, TextFileReadResult};
use kanvibe_process::kanvibe_project_state::{
    KanvibeTaskMetadataUpdate, KanvibeTaskSession, KanvibeTaskState, KanvibeTaskSyncState,
};
use kanvibe_process::kanvibe_state_store::KanvibeStateStore;
use kanvibe_process::task_target_propagation::PeerTargetResult;
use kanvibe_process::worktree::{HerdrTaskWorkspaces, ResourceCleanupOptions, WorktreeSession};

use crate::board_notifier::{BoardEvent, BoardEventSubscription, BoardNotifier};
use crate::error::{ServiceError, ServiceResult};
use crate::kanban::ports::{
    HookVerificationFailure, HookVerificationSuccess, KanvibeStateWriters, TaskHookInstallation,
    TaskPeerPropagation, TaskWorktrees,
};
use crate::kanban::{KanbanService, KanbanServiceParts, KanbanTimings};
use crate::terminal_ports::TaskTerminalSessions;
use crate::test_support::{RecordedCommand, ScriptedCommandRunner, TemporaryDatabase};

/// 모든 가짜가 함께 쓰는 호출 순서 기록
#[derive(Clone, Default)]
pub(super) struct CallLog(Arc<Mutex<Vec<String>>>);

impl CallLog {
    pub fn push(&self, entry: impl Into<String>) {
        self.0.lock().unwrap().push(entry.into());
    }

    pub fn entries(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }

    pub fn position(&self, entry: &str) -> Option<usize> {
        self.entries().iter().position(|logged| logged == entry)
    }

    pub fn count(&self, entry: &str) -> usize {
        self.entries()
            .iter()
            .filter(|logged| *logged == entry)
            .count()
    }
}

/// 테스트가 열 때까지 호출을 붙잡는 문
#[derive(Clone, Default)]
pub(super) struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    pub fn open(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }

    pub fn wait(&self) {
        let mut is_open = self.0.0.lock().unwrap();
        while !*is_open {
            is_open = self.0.1.wait(is_open).unwrap();
        }
    }
}

/// 테스트가 앞으로 돌리는 시계
#[derive(Clone)]
pub(super) struct FakeClock {
    base: Instant,
    offset: Arc<Mutex<Duration>>,
}

impl FakeClock {
    fn new() -> Self {
        Self {
            base: Instant::now(),
            offset: Arc::new(Mutex::new(Duration::ZERO)),
        }
    }

    pub fn advance(&self, duration: Duration) {
        *self.offset.lock().unwrap() += duration;
    }

    fn now(&self) -> Instant {
        self.base + *self.offset.lock().unwrap()
    }
}

type CommandHandler = dyn Fn(&RecordedCommand) -> Result<String, ExecGitError> + Send + Sync;

/// 테스트 중간에 응답을 바꿀 수 있는 실행기 응답
#[derive(Clone)]
pub(super) struct CommandResponses(Arc<Mutex<Arc<CommandHandler>>>);

impl CommandResponses {
    pub fn set(
        &self,
        handler: impl Fn(&RecordedCommand) -> Result<String, ExecGitError> + Send + Sync + 'static,
    ) {
        *self.0.lock().unwrap() = Arc::new(handler);
    }

    pub fn returning(&self, output: &str) {
        let output = output.to_string();
        self.set(move |_| Ok(output.clone()));
    }

    pub fn failing(&self, message: &str) {
        let message = message.to_string();
        self.set(move |_| Err(ExecGitError::new(message.clone())));
    }

    fn respond(&self, call: &RecordedCommand) -> Result<String, ExecGitError> {
        let handler = Arc::clone(&self.0.lock().unwrap());
        handler(call)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WorktreeCreation {
    pub project_path: String,
    pub branch_name: String,
    pub base_branch: String,
    pub session_type: SessionType,
    pub ssh_host: Option<String>,
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SessionCreation {
    pub project_path: String,
    pub branch_name: String,
    pub session_type: SessionType,
    pub ssh_host: Option<String>,
    pub working_directory: String,
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WorktreeRemoval {
    pub project_path: String,
    pub branch_name: String,
    pub ssh_host: Option<String>,
    pub options: ResourceCleanupOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SessionRemoval {
    pub session_type: SessionType,
    pub session_name: String,
    pub ssh_host: Option<String>,
    pub should_throw_on_error: bool,
}

type SessionNameHandler = dyn Fn(&SessionCreation, usize) -> ServiceResult<String> + Send + Sync;

pub(super) struct FakeWorktrees {
    log: CallLog,
    pub created_session: Mutex<WorktreeSession>,
    pub worktree_creations: Mutex<Vec<WorktreeCreation>>,
    pub session_creations: Mutex<Vec<SessionCreation>>,
    pub session_name: Mutex<Arc<SessionNameHandler>>,
    pub worktree_removals: Mutex<Vec<WorktreeRemoval>>,
    pub session_removals: Mutex<Vec<SessionRemoval>>,
    pub session_removal_error: Mutex<Option<String>>,
    pub alive_session_names: Mutex<Vec<String>>,
    pub alive_checks: AtomicUsize,
}

impl FakeWorktrees {
    fn new(log: CallLog) -> Self {
        Self {
            log,
            created_session: Mutex::new(WorktreeSession {
                worktree_path: "/workspace/repo-worktrees/task-1".into(),
                session_name: "task-1".into(),
            }),
            worktree_creations: Mutex::default(),
            session_creations: Mutex::default(),
            session_name: Mutex::new(Arc::new(|_, _| Ok("repo-feature-remote".into()))),
            worktree_removals: Mutex::default(),
            session_removals: Mutex::default(),
            session_removal_error: Mutex::default(),
            alive_session_names: Mutex::default(),
            alive_checks: AtomicUsize::new(0),
        }
    }

    pub fn set_session_name(
        &self,
        handler: impl Fn(&SessionCreation, usize) -> ServiceResult<String> + Send + Sync + 'static,
    ) {
        *self.session_name.lock().unwrap() = Arc::new(handler);
    }
}

impl TaskWorktrees for FakeWorktrees {
    fn create_worktree_with_session(
        &self,
        project_path: &str,
        branch_name: &str,
        base_branch: &str,
        session_type: SessionType,
        ssh_host: Option<&str>,
        project_id: Option<&str>,
    ) -> ServiceResult<WorktreeSession> {
        self.log.push("create-worktree");
        self.worktree_creations
            .lock()
            .unwrap()
            .push(WorktreeCreation {
                project_path: project_path.into(),
                branch_name: branch_name.into(),
                base_branch: base_branch.into(),
                session_type,
                ssh_host: ssh_host.map(str::to_string),
                project_id: project_id.map(str::to_string),
            });
        Ok(self.created_session.lock().unwrap().clone())
    }

    fn create_session_without_worktree(
        &self,
        project_path: &str,
        branch_name: &str,
        session_type: SessionType,
        ssh_host: Option<&str>,
        working_directory: &str,
        project_id: Option<&str>,
    ) -> ServiceResult<String> {
        self.log.push("create-session");
        let creation = SessionCreation {
            project_path: project_path.into(),
            branch_name: branch_name.into(),
            session_type,
            ssh_host: ssh_host.map(str::to_string),
            working_directory: working_directory.into(),
            project_id: project_id.map(str::to_string),
        };
        let call_number = {
            let mut creations = self.session_creations.lock().unwrap();
            creations.push(creation.clone());
            creations.len()
        };
        let handler = Arc::clone(&self.session_name.lock().unwrap());
        handler(&creation, call_number)
    }

    fn remove_worktree_and_branch(
        &self,
        project_path: &str,
        branch_name: &str,
        ssh_host: Option<&str>,
        options: &ResourceCleanupOptions,
    ) -> ServiceResult<()> {
        self.log.push("remove-worktree");
        self.worktree_removals
            .lock()
            .unwrap()
            .push(WorktreeRemoval {
                project_path: project_path.into(),
                branch_name: branch_name.into(),
                ssh_host: ssh_host.map(str::to_string),
                options: options.clone(),
            });
        Ok(())
    }

    fn remove_session_only(
        &self,
        session_type: SessionType,
        session_name: &str,
        ssh_host: Option<&str>,
        should_throw_on_error: bool,
    ) -> ServiceResult<()> {
        self.log.push("remove-session");
        self.session_removals.lock().unwrap().push(SessionRemoval {
            session_type,
            session_name: session_name.into(),
            ssh_host: ssh_host.map(str::to_string),
            should_throw_on_error,
        });
        match self.session_removal_error.lock().unwrap().clone() {
            Some(message) => Err(ServiceError::new(message)),
            None => Ok(()),
        }
    }

    fn is_session_alive(&self, _: SessionType, session_name: &str, _: Option<&str>) -> bool {
        self.alive_checks.fetch_add(1, Ordering::SeqCst);
        self.alive_session_names
            .lock()
            .unwrap()
            .iter()
            .any(|name| name == session_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HookCall {
    pub target_path: String,
    pub task_id: String,
    pub ssh_host: Option<String>,
}

pub(super) struct FakeHooks {
    log: CallLog,
    pub install_gate: Mutex<Option<Gate>>,
    pub install_error: Mutex<Option<String>>,
    pub file_installs: Mutex<Vec<HookCall>>,
    pub verifications: Mutex<Vec<HookCall>>,
}

impl FakeHooks {
    fn new(log: CallLog) -> Self {
        Self {
            log,
            install_gate: Mutex::default(),
            install_error: Mutex::default(),
            file_installs: Mutex::default(),
            verifications: Mutex::default(),
        }
    }

    /// 설치가 테스트가 문을 열 때까지 끝나지 않게 한다
    pub fn hold_installs(&self) -> Gate {
        let gate = Gate::default();
        *self.install_gate.lock().unwrap() = Some(gate.clone());
        gate
    }
}

fn hook_call(target_path: &str, task_id: &str, ssh_host: Option<&str>) -> HookCall {
    HookCall {
        target_path: target_path.into(),
        task_id: task_id.into(),
        ssh_host: ssh_host.map(str::to_string),
    }
}

impl TaskHookInstallation for FakeHooks {
    fn install_hook_files(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), String> {
        self.log.push("install-files");
        self.file_installs
            .lock()
            .unwrap()
            .push(hook_call(target_path, task_id, ssh_host));
        let gate = self.install_gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.wait();
        }
        match self.install_error.lock().unwrap().clone() {
            Some(message) => Err(message),
            None => Ok(()),
        }
    }

    fn schedule_hooks_verification(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
        _: HookVerificationSuccess,
        _: HookVerificationFailure,
    ) {
        self.log.push("schedule-verification");
        self.verifications
            .lock()
            .unwrap()
            .push(hook_call(target_path, task_id, ssh_host));
    }
}

/// 실제 `KanvibeStateWriters`가 부르는 저장소 쓰기 하나
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StateWrite {
    pub kind: &'static str,
    pub repo_path: String,
    pub ssh_host: Option<String>,
    pub content: String,
}

type ProjectColorObserver = dyn Fn(&str) + Send + Sync;

/// `.kanvibe/` 파일 대신 쓰기를 기록하는 저장소. 읽기는 모두 빈 결과다
pub(super) struct RecordingStateStore {
    log: CallLog,
    pub writes: Mutex<Vec<StateWrite>>,
    pub status_gate: Mutex<Option<Gate>>,
    pub metadata_gate: Mutex<Option<Gate>>,
    pub project_color_observer: Mutex<Option<Arc<ProjectColorObserver>>>,
}

impl RecordingStateStore {
    fn new(log: CallLog) -> Self {
        Self {
            log,
            writes: Mutex::default(),
            status_gate: Mutex::default(),
            metadata_gate: Mutex::default(),
            project_color_observer: Mutex::default(),
        }
    }

    fn record(&self, kind: &'static str, repo_path: &str, ssh_host: Option<&str>, content: String) {
        self.log.push(format!("state:{kind}"));
        self.writes.lock().unwrap().push(StateWrite {
            kind,
            repo_path: repo_path.into(),
            ssh_host: ssh_host.map(str::to_string),
            content,
        });
    }

    pub fn writes_of(&self, kind: &str) -> Vec<StateWrite> {
        self.writes
            .lock()
            .unwrap()
            .iter()
            .filter(|write| write.kind == kind)
            .cloned()
            .collect()
    }
}

fn wait_for(gate: &Mutex<Option<Gate>>) {
    let gate = gate.lock().unwrap().clone();
    if let Some(gate) = gate {
        gate.wait();
    }
}

impl KanvibeStateStore for RecordingStateStore {
    fn read_project_color(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<Option<String>, HostFileError> {
        Ok(None)
    }

    fn write_project_color(
        &self,
        repo_path: &str,
        project_color: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError> {
        let observer = self.project_color_observer.lock().unwrap().clone();
        if let Some(observer) = observer {
            observer(repo_path);
        }
        self.record("project-color", repo_path, ssh_host, project_color.into());
        Ok(())
    }

    fn write_project_color_if_absent(
        &self,
        _: &str,
        _: &str,
        _: Option<&str>,
    ) -> Result<(), HostFileError> {
        Ok(())
    }

    fn write_project_priority(
        &self,
        repo_path: &str,
        priority: Option<TaskPriority>,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError> {
        let content = format!("{:?}", priority.map(TaskPriority::as_str));
        self.record("project-priority", repo_path, ssh_host, content);
        Ok(())
    }

    fn add_ai_tool_patterns_to_git_exclude(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<(), HostFileError> {
        Ok(())
    }

    fn read_task_state(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<Option<KanvibeTaskState>, HostFileError> {
        Ok(None)
    }

    fn read_task_sync_state(
        &self,
        _: &str,
        _: Option<&str>,
        _: Option<&str>,
    ) -> Result<KanvibeTaskSyncState, HostFileError> {
        Ok(KanvibeTaskSyncState::default())
    }

    fn write_task_status(
        &self,
        repo_path: &str,
        status: TaskStatus,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError> {
        self.record("status", repo_path, ssh_host, status.as_str().into());
        wait_for(&self.status_gate);
        Ok(())
    }

    fn write_task_metadata(
        &self,
        repo_path: &str,
        metadata: &KanvibeTaskMetadataUpdate,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError> {
        let content = format!(
            "description={:?} priority={:?}",
            metadata.description,
            metadata
                .priority
                .map(|priority| priority.map(TaskPriority::as_str))
        );
        self.record("metadata", repo_path, ssh_host, content);
        wait_for(&self.metadata_gate);
        Ok(())
    }

    fn write_task_session(
        &self,
        repo_path: &str,
        session: &KanvibeTaskSession,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError> {
        let content = format!("{}:{}", session.session_type.as_str(), session.session_name);
        self.record("session", repo_path, ssh_host, content);
        Ok(())
    }

    fn read_text_files(
        &self,
        _: &[String],
        _: Option<&str>,
    ) -> Result<BTreeMap<String, TextFileReadResult>, HostFileError> {
        Ok(BTreeMap::new())
    }
}

pub(super) struct FakePeers {
    log: CallLog,
    pub results: Mutex<Vec<PeerTargetResult>>,
    pub calls: Mutex<Vec<(String, Option<String>)>>,
}

impl TaskPeerPropagation for FakePeers {
    fn propagate_task_deletion_to_peers(
        &self,
        worktree_path: &str,
        ssh_host: Option<&str>,
    ) -> Vec<PeerTargetResult> {
        self.log.push("propagate");
        self.calls
            .lock()
            .unwrap()
            .push((worktree_path.into(), ssh_host.map(str::to_string)));
        self.results.lock().unwrap().clone()
    }
}

pub(super) struct FakeTerminal {
    log: CallLog,
    pub detached: Mutex<Vec<(String, String)>>,
}

impl HerdrTaskWorkspaces for FakeTerminal {
    fn close_task_workspace(&self, _: &str, _: Option<&str>) -> Result<(), String> {
        Ok(())
    }

    fn is_task_alive(&self, _: &str, _: Option<&str>) -> bool {
        false
    }
}

impl TaskTerminalSessions for FakeTerminal {
    fn detach_session(&self, task_id: &str, trigger_label: &str) {
        self.log.push("detach");
        self.detached
            .lock()
            .unwrap()
            .push((task_id.into(), trigger_label.into()));
    }

    fn replace_herdr_remote_server(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
}

fn board_event_name(event: &BoardEvent) -> String {
    let json = serde_json::to_value(event).expect("보드 이벤트는 직렬화된다");
    format!("board:{}", json["type"].as_str().unwrap_or_default())
}

/// 응답을 테스트 중간에 바꿀 수 있는 기록 실행기
fn scripted_runner() -> (Arc<ScriptedCommandRunner>, CommandResponses) {
    let responses = CommandResponses(Arc::new(Mutex::new(Arc::new(|_| Ok(String::new())))));
    let runner_responses = responses.clone();
    let runner = Arc::new(ScriptedCommandRunner::new(move |call| {
        runner_responses.respond(call)
    }));
    (runner, responses)
}

/// 실제 공유 파일 기록기와 그 쓰기를 기록하는 저장소.
/// 앱처럼 기록기는 앱 수명 동안 살아 있어야 해서 테스트에서는 저장소와 DB 참조를 일부러 놓아 둔다
fn recording_state_writers(
    log: &CallLog,
    database: &Arc<Database>,
) -> (&'static RecordingStateStore, KanvibeStateWriters<'static>) {
    let state: &'static RecordingStateStore =
        Box::leak(Box::new(RecordingStateStore::new(log.clone())));
    let records: &'static Arc<Database> = Box::leak(Box::new(Arc::clone(database)));
    let records: &'static Database = records;
    (state, KanvibeStateWriters::new(state, records))
}

type RecordedBoard = (
    Arc<BoardNotifier>,
    Arc<Mutex<Vec<BoardEvent>>>,
    BoardEventSubscription,
);

/// 보드 이벤트를 모으고 호출 순서 기록에도 남긴다
fn recorded_board(log: &CallLog) -> RecordedBoard {
    let board = BoardNotifier::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let (event_sink, event_log) = (Arc::clone(&events), log.clone());
    let subscription = board.subscribe(move |event| {
        event_log.push(board_event_name(event));
        event_sink.lock().unwrap().push(event.clone());
    });
    (board, events, subscription)
}

/// 보드 서비스와 모든 가짜를 묶은 테스트 환경
pub(super) struct KanbanFixture {
    pub db: TemporaryDatabase,
    pub service: Arc<KanbanService>,
    pub log: CallLog,
    pub runner: Arc<ScriptedCommandRunner>,
    pub responses: CommandResponses,
    pub worktrees: Arc<FakeWorktrees>,
    pub hooks: Arc<FakeHooks>,
    pub state: &'static RecordingStateStore,
    pub peers: Arc<FakePeers>,
    pub terminal: Arc<FakeTerminal>,
    pub events: Arc<Mutex<Vec<BoardEvent>>>,
    pub clock: FakeClock,
    _board_subscription: BoardEventSubscription,
}

impl KanbanFixture {
    pub fn new() -> Self {
        Self::with_timings(KanbanTimings::default())
    }

    pub fn with_timings(timings: KanbanTimings) -> Self {
        let db = TemporaryDatabase::open();
        let log = CallLog::default();
        let (runner, responses) = scripted_runner();
        let home = db.directory.path().join("home");
        let files = Arc::new(HostFileAccess::new(runner.clone(), home.to_string_lossy()));
        let worktrees = Arc::new(FakeWorktrees::new(log.clone()));
        let hooks = Arc::new(FakeHooks::new(log.clone()));
        let (state, state_writers) = recording_state_writers(&log, &db.database);
        let peers = Arc::new(FakePeers {
            log: log.clone(),
            results: Mutex::default(),
            calls: Mutex::default(),
        });
        let terminal = Arc::new(FakeTerminal {
            log: log.clone(),
            detached: Mutex::default(),
        });
        let (board, events, board_subscription) = recorded_board(&log);
        let clock = FakeClock::new();
        let service_clock = clock.clone();
        let service = Arc::new(KanbanService::new(KanbanServiceParts {
            database: Arc::clone(&db.database),
            files,
            board,
            terminal: terminal.clone(),
            worktrees: worktrees.clone(),
            hooks: hooks.clone(),
            state_files: Arc::new(state_writers),
            peers: peers.clone(),
            clock: Arc::new(move || service_clock.now()),
            timings,
        }));
        Self {
            db,
            service,
            log,
            runner,
            responses,
            worktrees,
            hooks,
            state,
            peers,
            terminal,
            events,
            clock,
            _board_subscription: board_subscription,
        }
    }

    pub fn events(&self) -> Vec<BoardEvent> {
        self.events.lock().unwrap().clone()
    }

    pub fn board_update_count(&self) -> usize {
        self.log.count("board:board-updated")
    }

    pub fn database(&self) -> &Database {
        &self.db.database
    }
}
