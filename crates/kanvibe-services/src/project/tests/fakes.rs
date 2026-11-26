//! 프로젝트 서비스 테스트의 가짜 협력 객체. TS 테스트의 `mocks`(gitOperations, worktree, hook 설치기,
//! hostFileAccess 상태 파일, githubProjectIcon, aiSessions) 자리다.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{
    AgentCallGraph, AggregatedAiSessionDetail, AggregatedAiSessionsResult,
    AiSessionAttachmentContent, LiveAiSession, RunningAgentPane,
};
use kanvibe_db::repositories::projects::NewProject;
use kanvibe_db::repositories::tasks::NewKanbanTask;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskStatus};
use kanvibe_process::git_operations::WorktreeEntry;
use kanvibe_process::hooks::claude::ClaudeHooksStatus;
use kanvibe_process::hooks::codex::CodexHooksStatus;
use kanvibe_process::hooks::gemini::GeminiHooksStatus;
use kanvibe_process::hooks::open_code::{OpenCodeHooksStatus, OpenCodePluginRegistrationStatus};
use kanvibe_process::hooks::provider::ShellHookScriptsStatus;
use kanvibe_process::hooks::providers::{
    KanvibeHookProvider, KanvibeHookStatus, KanvibeHookStatuses,
};
use kanvibe_process::hooks::target_registration::HookTargetRegistrationStatus;
use kanvibe_process::kanvibe_project_state::KanvibeTaskSyncState;

use crate::board_notifier::BoardNotifier;
use crate::project::ports::{
    AiSessionAttachmentRequest, AiSessionDetailRequest, AiSessionListRequest, AiSessionScope,
    BranchSessions, HookInstallFailure, HookInstallSuccess, ProjectHookOperations,
    ProjectIconSource, ProjectRepositoryProbe, ProjectStateFiles, TaskAiSessionReader,
};
use crate::project::{ProjectService, ProjectServiceParts};
use crate::test_support::{RecordedBoardEvents, TemporaryDatabase, new_project, new_task};

type Host = Option<String>;

fn host(ssh_host: Option<&str>) -> Host {
    ssh_host.map(str::to_string)
}

/// 다른 스레드가 열 때까지 기다리게 하는 관문(TS 테스트의 풀리지 않은 Promise)
#[derive(Default)]
pub(super) struct Gate {
    is_open: Mutex<bool>,
    opened: Condvar,
}

impl Gate {
    pub fn wait(&self) {
        let mut is_open = self.is_open.lock().unwrap();
        while !*is_open {
            is_open = self.opened.wait(is_open).unwrap();
        }
    }

    pub fn open(&self) {
        *self.is_open.lock().unwrap() = true;
        self.opened.notify_all();
    }
}

/// 조건이 참이 될 때까지 기다린다(TS `vi.waitFor`)
pub(super) fn wait_until(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "조건을 기다리다 시간이 지났다");
        std::thread::sleep(Duration::from_millis(5));
    }
}

type PathCheck = dyn Fn(&str, Option<&str>) -> Result<bool, String> + Send + Sync;
type WorktreeList = dyn Fn(&str, Option<&str>) -> Vec<WorktreeEntry> + Send + Sync;
type CommandOutput = dyn Fn(&str) -> Result<String, String> + Send + Sync;

pub(super) fn worktree(path: &str, branch: &str) -> WorktreeEntry {
    WorktreeEntry {
        path: path.into(),
        branch: branch.into(),
        is_bare: false,
    }
}

pub(super) struct FakeRepositories {
    path_check: Mutex<Arc<PathCheck>>,
    worktree_list: Mutex<Arc<WorktreeList>>,
    command_output: Mutex<Arc<CommandOutput>>,
    pub is_valid_repo: Mutex<bool>,
    pub scanned_repos: Mutex<Vec<String>>,
    pub branches: Mutex<Vec<String>>,
    pub path_checks: Mutex<Vec<(String, Host)>>,
    pub worktree_lists: Mutex<Vec<(String, Host)>>,
    pub commands: Mutex<Vec<(String, Host)>>,
    pub default_branch_calls: Mutex<Vec<(String, Host)>>,
    pub branch_list_calls: Mutex<Vec<(String, Host, bool)>>,
    pub scan_calls: Mutex<Vec<(String, Host)>>,
}

impl Default for FakeRepositories {
    fn default() -> Self {
        Self {
            path_check: Mutex::new(Arc::new(|_, _| Ok(false))),
            worktree_list: Mutex::new(Arc::new(|_, _| Vec::new())),
            command_output: Mutex::new(Arc::new(|_| Ok(String::new()))),
            is_valid_repo: Mutex::new(true),
            scanned_repos: Mutex::default(),
            branches: Mutex::new(vec!["main".into(), "develop".into()]),
            path_checks: Mutex::default(),
            worktree_lists: Mutex::default(),
            commands: Mutex::default(),
            default_branch_calls: Mutex::default(),
            branch_list_calls: Mutex::default(),
            scan_calls: Mutex::default(),
        }
    }
}

impl FakeRepositories {
    pub fn on_path_check(
        &self,
        check: impl Fn(&str, Option<&str>) -> Result<bool, String> + Send + Sync + 'static,
    ) {
        *self.path_check.lock().unwrap() = Arc::new(check);
    }

    pub fn on_worktree_list(
        &self,
        list: impl Fn(&str, Option<&str>) -> Vec<WorktreeEntry> + Send + Sync + 'static,
    ) {
        *self.worktree_list.lock().unwrap() = Arc::new(list);
    }

    pub fn returning_worktrees(&self, worktrees: Vec<WorktreeEntry>) {
        self.on_worktree_list(move |_, _| worktrees.clone());
    }

    pub fn on_command(
        &self,
        output: impl Fn(&str) -> Result<String, String> + Send + Sync + 'static,
    ) {
        *self.command_output.lock().unwrap() = Arc::new(output);
    }

    pub fn listed_repo_paths(&self) -> Vec<String> {
        let lists = self.worktree_lists.lock().unwrap();
        lists.iter().map(|(path, _)| path.clone()).collect()
    }

    pub fn checked_paths(&self) -> Vec<String> {
        let checks = self.path_checks.lock().unwrap();
        checks.iter().map(|(path, _)| path.clone()).collect()
    }

    pub fn command_texts(&self) -> Vec<String> {
        let commands = self.commands.lock().unwrap();
        commands
            .iter()
            .map(|(command, _)| command.clone())
            .collect()
    }
}

impl ProjectRepositoryProbe for FakeRepositories {
    fn local_home_directory(&self) -> &str {
        "/home/tester"
    }

    fn is_path_missing(&self, target_path: &str, ssh_host: Option<&str>) -> Result<bool, String> {
        self.path_checks
            .lock()
            .unwrap()
            .push((target_path.into(), host(ssh_host)));
        let check = Arc::clone(&self.path_check.lock().unwrap());
        check(target_path, ssh_host)
    }

    fn list_worktrees(&self, repo_path: &str, ssh_host: Option<&str>) -> Vec<WorktreeEntry> {
        self.worktree_lists
            .lock()
            .unwrap()
            .push((repo_path.into(), host(ssh_host)));
        let list = Arc::clone(&self.worktree_list.lock().unwrap());
        list(repo_path, ssh_host)
    }

    fn validate_git_repo(&self, _repo_path: &str, _ssh_host: Option<&str>) -> bool {
        *self.is_valid_repo.lock().unwrap()
    }

    fn default_branch(&self, repo_path: &str, ssh_host: Option<&str>) -> String {
        self.default_branch_calls
            .lock()
            .unwrap()
            .push((repo_path.into(), host(ssh_host)));
        "main".into()
    }

    fn list_branches(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
        should_refresh: bool,
    ) -> Result<Vec<String>, String> {
        self.branch_list_calls.lock().unwrap().push((
            repo_path.into(),
            host(ssh_host),
            should_refresh,
        ));
        Ok(self.branches.lock().unwrap().clone())
    }

    fn scan_git_repos(&self, root_path: &str, ssh_host: Option<&str>) -> Vec<String> {
        self.scan_calls
            .lock()
            .unwrap()
            .push((root_path.into(), host(ssh_host)));
        self.scanned_repos.lock().unwrap().clone()
    }

    fn exec_git(&self, command: &str, ssh_host: Option<&str>) -> Result<String, String> {
        self.commands
            .lock()
            .unwrap()
            .push((command.into(), host(ssh_host)));
        let output = Arc::clone(&self.command_output.lock().unwrap());
        output(command)
    }

    fn available_hosts(&self) -> Vec<String> {
        Vec::new()
    }
}

type SessionAlive = dyn Fn(SessionType, &str) -> bool + Send + Sync;

/// 세션 생성 호출 하나(projectPath, branchName, sessionType, sshHost, workingDir)
pub(super) type SessionCreation = (String, String, SessionType, Host, Option<String>);

pub(super) struct FakeSessions {
    alive: Mutex<Arc<SessionAlive>>,
    pub created_session: Mutex<Result<String, String>>,
    pub creations: Mutex<Vec<SessionCreation>>,
    pub detached: Mutex<Vec<(String, String)>>,
}

impl Default for FakeSessions {
    fn default() -> Self {
        Self {
            alive: Mutex::new(Arc::new(|_, _| false)),
            created_session: Mutex::new(Err("세션 생성 결과를 정하지 않았다".into())),
            creations: Mutex::default(),
            detached: Mutex::default(),
        }
    }
}

impl FakeSessions {
    pub fn on_alive(&self, alive: impl Fn(SessionType, &str) -> bool + Send + Sync + 'static) {
        *self.alive.lock().unwrap() = Arc::new(alive);
    }
}

impl BranchSessions for FakeSessions {
    fn is_session_alive(
        &self,
        session_type: SessionType,
        session_name: &str,
        _: Option<&str>,
    ) -> bool {
        let alive = Arc::clone(&self.alive.lock().unwrap());
        alive(session_type, session_name)
    }

    fn create_session_without_worktree(
        &self,
        project_path: &str,
        branch_name: &str,
        session_type: SessionType,
        ssh_host: Option<&str>,
        working_directory: Option<&str>,
    ) -> Result<String, String> {
        self.creations.lock().unwrap().push((
            project_path.into(),
            branch_name.into(),
            session_type,
            host(ssh_host),
            working_directory.map(str::to_string),
        ));
        self.created_session.lock().unwrap().clone()
    }

    fn detach_session(&self, task_id: &str, trigger_label: &str) {
        self.detached
            .lock()
            .unwrap()
            .push((task_id.into(), trigger_label.into()));
    }
}

fn scripts_status(is_installed: bool) -> ShellHookScriptsStatus {
    ShellHookScriptsStatus {
        has_task_id_binding: is_installed,
        has_status_mappings: is_installed,
        has_status_json_persistence: is_installed,
        has_target_fanout: is_installed,
        has_bounded_notify_timeout: is_installed,
        has_parallel_target_fanout: is_installed,
        has_expected_hook_server_url: is_installed,
        has_reachable_hook_server: is_installed,
        bound_task_id: None,
        configured_hook_server_url: None,
        expected_hook_server_url: None,
        has_registered_hook_target: is_installed,
        registered_hook_target_url: None,
    }
}

fn open_code_status(is_installed: bool) -> OpenCodeHooksStatus {
    OpenCodeHooksStatus {
        is_installed,
        has_plugin: is_installed,
        has_task_id_binding: is_installed,
        has_status_endpoint: is_installed,
        has_event_mappings: is_installed,
        has_status_json_persistence: is_installed,
        has_bounded_notify_timeout: is_installed,
        has_target_fanout: is_installed,
        has_parallel_target_fanout: is_installed,
        has_main_session_guard: is_installed,
        has_duplicate_progress_guard: is_installed,
        has_pane_chat_binding: is_installed,
        has_expected_hook_server_url: is_installed,
        has_reachable_hook_server: is_installed,
        bound_task_id: None,
        target_path: String::new(),
        plugin_path: String::new(),
        configured_hook_server_url: None,
        expected_hook_server_url: None,
        registration: OpenCodePluginRegistrationStatus::default(),
        target_registration: HookTargetRegistrationStatus {
            has_registered_hook_target: is_installed,
            registered_hook_target_url: None,
        },
    }
}

/// 모든 확인 항목이 `is_installed`인 provider 상태(TS mock `{ installed: true|false }`)
pub(super) fn hook_status(provider: KanvibeHookProvider, is_installed: bool) -> KanvibeHookStatus {
    let scripts = scripts_status(is_installed);
    match provider {
        KanvibeHookProvider::Claude => KanvibeHookStatus::Claude(ClaudeHooksStatus {
            is_installed,
            has_prompt_hook: is_installed,
            has_stop_hook: is_installed,
            has_question_hook: is_installed,
            has_settings_entry: is_installed,
            scripts,
        }),
        KanvibeHookProvider::Gemini => KanvibeHookStatus::Gemini(GeminiHooksStatus {
            is_installed,
            has_prompt_hook: is_installed,
            has_stop_hook: is_installed,
            has_settings_entry: is_installed,
            scripts,
        }),
        KanvibeHookProvider::Codex => KanvibeHookStatus::Codex(CodexHooksStatus {
            is_installed,
            has_prompt_hook: is_installed,
            has_permission_hook: is_installed,
            has_pre_tool_hook: is_installed,
            has_stop_hook: is_installed,
            has_hooks_file: is_installed,
            has_hook_entries: is_installed,
            has_config_entry: is_installed,
            scripts,
        }),
        KanvibeHookProvider::OpenCode => {
            KanvibeHookStatus::OpenCode(open_code_status(is_installed))
        }
    }
}

/// hook 호출 하나(대상 경로, taskId, provider, sshHost)
pub(super) type HookCall = (String, String, Option<KanvibeHookProvider>, Host);

/// provider 상태 조회 하나(provider, 대상 경로, taskId, sshHost)
pub(super) type StatusCall = (KanvibeHookProvider, String, Option<String>, Host);

#[derive(Default)]
pub(super) struct FakeHooks {
    pub ensure_calls: Mutex<Vec<HookCall>>,
    pub install_calls: Mutex<Vec<HookCall>>,
    pub provider_install_calls: Mutex<Vec<HookCall>>,
    pub scheduled_installs: Mutex<Vec<HookCall>>,
    pub status_calls: Mutex<Vec<StatusCall>>,
    pub statuses_calls: Mutex<Vec<HookCall>>,
    pub is_installed: Mutex<bool>,
    pub status_error: Mutex<Option<String>>,
}

impl ProjectHookOperations for FakeHooks {
    fn ensure_hooks_current(
        &self,
        target: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<bool, String> {
        self.ensure_calls.lock().unwrap().push((
            target.into(),
            task_id.into(),
            None,
            host(ssh_host),
        ));
        Ok(false)
    }

    fn hooks_statuses(
        &self,
        target: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<KanvibeHookStatuses, String> {
        self.statuses_calls.lock().unwrap().push((
            target.into(),
            task_id.into(),
            None,
            host(ssh_host),
        ));
        Ok(KanvibeHookStatuses {
            claude: hook_status(KanvibeHookProvider::Claude, false),
            gemini: hook_status(KanvibeHookProvider::Gemini, true),
            codex: hook_status(KanvibeHookProvider::Codex, false),
            open_code: hook_status(KanvibeHookProvider::OpenCode, true),
        })
    }

    fn install_hooks(
        &self,
        target: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), String> {
        self.install_calls.lock().unwrap().push((
            target.into(),
            task_id.into(),
            None,
            host(ssh_host),
        ));
        Ok(())
    }

    fn install_hook_provider(
        &self,
        target: &str,
        task_id: &str,
        provider: KanvibeHookProvider,
        ssh_host: Option<&str>,
    ) -> Result<(), String> {
        self.provider_install_calls.lock().unwrap().push((
            target.into(),
            task_id.into(),
            Some(provider),
            host(ssh_host),
        ));
        Ok(())
    }

    fn schedule_hooks_install(
        &self,
        target: &str,
        task_id: &str,
        ssh_host: Option<&str>,
        _on_success: HookInstallSuccess,
        _on_failure: HookInstallFailure,
    ) {
        self.scheduled_installs.lock().unwrap().push((
            target.into(),
            task_id.into(),
            None,
            host(ssh_host),
        ));
    }

    fn provider_status(
        &self,
        provider: KanvibeHookProvider,
        target: &str,
        task_id: Option<&str>,
        ssh_host: Option<&str>,
    ) -> Result<KanvibeHookStatus, String> {
        self.status_calls.lock().unwrap().push((
            provider,
            target.into(),
            task_id.map(str::to_string),
            host(ssh_host),
        ));
        if let Some(error) = self.status_error.lock().unwrap().take() {
            return Err(error);
        }
        Ok(hook_status(provider, *self.is_installed.lock().unwrap()))
    }
}

/// `.kanvibe/` 파일 쓰기 하나(경로, taskId, 상태, sshHost)
pub(super) type PersistedStatus = (String, String, TaskStatus, Host);

#[derive(Default)]
pub(super) struct FakeStateFiles {
    /// 경로별 `status.json` 상태
    pub statuses: Mutex<HashMap<String, TaskStatus>>,
    /// 경로별 task.json·project.json 값. 상태는 `statuses`에서 채운다
    pub sync_states: Mutex<HashMap<String, KanvibeTaskSyncState>>,
    pub shared_colors: Mutex<HashMap<String, String>>,
    /// 다음 상태 읽기 한 번을 이 오류로 실패시킨다
    pub next_read_error: Mutex<Option<String>>,
    pub persisted: Mutex<Vec<PersistedStatus>>,
    pub sync_state_reads: Mutex<Vec<(Option<String>, Host, String)>>,
    pub persisted_colors: Mutex<Vec<String>>,
}

impl FakeStateFiles {
    pub fn persisted_paths(&self) -> Vec<String> {
        let persisted = self.persisted.lock().unwrap();
        persisted.iter().map(|(path, ..)| path.clone()).collect()
    }

    fn take_read_error(&self) -> Result<(), String> {
        match self.next_read_error.lock().unwrap().take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl ProjectStateFiles for FakeStateFiles {
    fn read_task_status(
        &self,
        repo_path: Option<&str>,
        _: Option<&str>,
    ) -> Result<Option<TaskStatus>, String> {
        self.take_read_error()?;
        let statuses = self.statuses.lock().unwrap();
        Ok(repo_path.and_then(|path| statuses.get(path).copied()))
    }

    fn read_task_sync_state(
        &self,
        repo_path: Option<&str>,
        ssh_host: Option<&str>,
        project_repo_path: &str,
    ) -> Result<KanvibeTaskSyncState, String> {
        self.sync_state_reads.lock().unwrap().push((
            repo_path.map(str::to_string),
            host(ssh_host),
            project_repo_path.into(),
        ));
        self.take_read_error()?;
        let Some(path) = repo_path else {
            return Ok(KanvibeTaskSyncState::default());
        };
        let mut state = self
            .sync_states
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .unwrap_or_default();
        state.status = self.statuses.lock().unwrap().get(path).copied();
        Ok(state)
    }

    fn persist_task_state(
        &self,
        repo_path: Option<&str>,
        task_id: &str,
        status: TaskStatus,
        ssh_host: Option<&str>,
    ) {
        let Some(path) = repo_path else {
            return;
        };
        self.persisted
            .lock()
            .unwrap()
            .push((path.into(), task_id.into(), status, host(ssh_host)));
        self.statuses.lock().unwrap().insert(path.into(), status);
    }

    fn read_shared_project_color(&self, repo_path: &str, _: Option<&str>) -> Option<String> {
        self.shared_colors.lock().unwrap().get(repo_path).cloned()
    }

    fn persist_project_color(&self, project: &Project) {
        self.persisted_colors
            .lock()
            .unwrap()
            .push(project.id.clone());
    }

    fn sync_project_color(&self, project: &Project) -> Option<String> {
        let shared = self
            .shared_colors
            .lock()
            .unwrap()
            .get(&project.repo_path)
            .cloned()?;
        (project.color.as_deref() != Some(shared.as_str())).then_some(shared)
    }
}

type IconLookup = dyn Fn(&str) -> Option<String> + Send + Sync;

pub(super) struct FakeIcons {
    lookup: Mutex<Arc<IconLookup>>,
    pub lookups: Mutex<Vec<String>>,
}

impl Default for FakeIcons {
    fn default() -> Self {
        Self {
            lookup: Mutex::new(Arc::new(|_| None)),
            lookups: Mutex::default(),
        }
    }
}

impl FakeIcons {
    pub fn on_lookup(&self, lookup: impl Fn(&str) -> Option<String> + Send + Sync + 'static) {
        *self.lookup.lock().unwrap() = Arc::new(lookup);
    }
}

impl ProjectIconSource for FakeIcons {
    fn resolve_project_icon_data_url(&self, repo_path: &str, _: Option<&str>) -> Option<String> {
        self.lookups.lock().unwrap().push(repo_path.into());
        let lookup = Arc::clone(&self.lookup.lock().unwrap());
        lookup(repo_path)
    }
}

#[derive(Default)]
pub(super) struct FakeAiSessions {
    pub aggregate_calls: Mutex<Vec<(AiSessionScope, AiSessionListRequest)>>,
}

impl TaskAiSessionReader for FakeAiSessions {
    fn aggregate_sessions(
        &self,
        scope: &AiSessionScope,
        request: &AiSessionListRequest,
    ) -> Result<AggregatedAiSessionsResult, String> {
        self.aggregate_calls
            .lock()
            .unwrap()
            .push((scope.clone(), request.clone()));
        Ok(AggregatedAiSessionsResult {
            is_remote: scope.ssh_host.is_some(),
            target_path: Some(scope.worktree_path.clone()),
            repo_path: Some(scope.repo_path.clone()),
            sessions: Vec::new(),
            sources: Vec::new(),
            next_cursor: None,
        })
    }

    fn session_detail(
        &self,
        _: &AiSessionScope,
        _: &AiSessionDetailRequest,
    ) -> Result<Option<AggregatedAiSessionDetail>, String> {
        Ok(None)
    }

    fn session_attachment(
        &self,
        _: &AiSessionScope,
        _: &AiSessionAttachmentRequest,
    ) -> Result<Option<AiSessionAttachmentContent>, String> {
        Ok(None)
    }

    fn live_sessions(&self, _: &AiSessionScope) -> Vec<LiveAiSession> {
        Vec::new()
    }

    fn running_agent_panes(&self) -> Vec<RunningAgentPane> {
        Vec::new()
    }

    fn agent_call_graph(
        &self,
        _: &AiSessionScope,
        provider: AiSessionProvider,
        session_id: &str,
    ) -> AgentCallGraph {
        AgentCallGraph {
            provider,
            session_id: session_id.into(),
            started_at: None,
            read_at: String::new(),
            roots: Vec::new(),
        }
    }
}

/// 서비스와 모든 가짜, 임시 DB를 묶은 테스트 환경
pub(super) struct Harness {
    pub db: TemporaryDatabase,
    pub board: Arc<BoardNotifier>,
    pub events: RecordedBoardEvents,
    pub repositories: Arc<FakeRepositories>,
    pub sessions: Arc<FakeSessions>,
    pub hooks: Arc<FakeHooks>,
    pub state_files: Arc<FakeStateFiles>,
    pub icons: Arc<FakeIcons>,
    pub ai_sessions: Arc<FakeAiSessions>,
    pub service: Arc<ProjectService>,
}

impl Harness {
    pub fn new() -> Self {
        let db = TemporaryDatabase::open();
        let board = BoardNotifier::new();
        let events = RecordedBoardEvents::subscribe(&board);
        let repositories = Arc::new(FakeRepositories::default());
        let sessions = Arc::new(FakeSessions::default());
        let hooks = Arc::new(FakeHooks::default());
        let state_files = Arc::new(FakeStateFiles::default());
        let icons = Arc::new(FakeIcons::default());
        let ai_sessions = Arc::new(FakeAiSessions::default());
        let service = ProjectService::new(ProjectServiceParts {
            database: Arc::clone(&db.database),
            board: Arc::clone(&board),
            repositories: repositories.clone(),
            sessions: sessions.clone(),
            hooks: hooks.clone(),
            state_files: state_files.clone(),
            icons: icons.clone(),
            ai_sessions: ai_sessions.clone(),
        });
        Self {
            db,
            board,
            events,
            repositories,
            sessions,
            hooks,
            state_files,
            icons,
            ai_sessions,
            service,
        }
    }

    /// 아이콘이 이미 있는 프로젝트. 아이콘 backfill이 끼지 않는다
    pub fn project(&self, name: &str, repo_path: &str, ssh_host: Option<&str>) -> Project {
        self.db.insert_project(NewProject {
            ssh_host: host(ssh_host),
            icon_data_url: Some("data:existing".into()),
            ..new_project(name, repo_path)
        })
    }

    pub fn task(&self, configure: impl FnOnce(&mut NewKanbanTask)) -> KanbanTask {
        let mut task = new_task("task", TaskStatus::Todo);
        configure(&mut task);
        self.db.insert_task(task)
    }

    /// 프로젝트 기본 브랜치 task
    pub fn root_task(&self, project: &Project, worktree_path: Option<&str>) -> KanbanTask {
        self.task(|task| {
            task.title = project.default_branch.clone();
            task.branch_name = Some(project.default_branch.clone());
            task.base_branch = Some(project.default_branch.clone());
            task.project_id = Some(project.id.clone());
            task.worktree_path = worktree_path.map(str::to_string);
            task.ssh_host = project.ssh_host.clone();
        })
    }

    pub fn tasks(&self) -> Vec<KanbanTask> {
        kanvibe_db::repositories::tasks::list_tasks(&self.db.database.connection()).unwrap()
    }

    pub fn find_task(&self, task_id: &str) -> Option<KanbanTask> {
        kanvibe_db::repositories::tasks::find_task(&self.db.database.connection(), task_id).unwrap()
    }

    pub fn branch_task(&self, branch_name: &str) -> Option<KanbanTask> {
        self.tasks()
            .into_iter()
            .find(|task| task.branch_name.as_deref() == Some(branch_name))
    }

    /// TS 테스트의 `getDefaultSessionType` mock 값
    pub fn use_default_session_type(&self, session_type: SessionType) {
        let settings = kanvibe_db::app_settings_service::AppSettingsService::new(
            &self.db.database,
            kanvibe_domain::platform::ShortcutPlatform::current(),
        );
        settings.set_default_session_type(session_type).unwrap();
    }

    /// 아이콘이 없는 프로젝트
    pub fn project_without_icon(&self, name: &str, repo_path: &str) -> Project {
        self.db.insert_project(new_project(name, repo_path))
    }

    pub fn projects(&self) -> Vec<Project> {
        self.service.get_all_projects().unwrap()
    }
}
