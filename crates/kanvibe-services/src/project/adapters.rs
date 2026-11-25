//! `ports`의 실제 구현. kanvibe-process(git·파일·hook·상태 파일)와 kanvibe-ai(세션 기록)를 그대로 부른다.
//! `HostProjectStateFiles`는 M2 서비스가 빌린 참조로 만들어지므로, 앱은 상태 저장소와 DB를 앱 수명 동안 살아 있는
//! 참조(`&'static`)로 넘겨 하나만 만든다. 그래야 git exclude 확인 캐시가 호출마다 사라지지 않는다.

use std::sync::Arc;

use kanvibe_ai::agent_call_graph::read_agent_call_graph;
use kanvibe_ai::aggregate::{aggregate_ai_sessions, get_ai_session_detail};
use kanvibe_ai::host::SessionHost;
use kanvibe_ai::live_sessions::read_live_ai_sessions;
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::running_agent_panes::RunningAgentPaneSource;
use kanvibe_ai::session_attachment::read_ai_session_attachment;
use kanvibe_ai::types::{
    AgentCallGraph, AggregatedAiSessionDetail, AggregatedAiSessionsResult,
    AiSessionAttachmentContent, AiSessionReaderContext, LiveAiSession, RunningAgentPane,
};
use kanvibe_db::Database;
use kanvibe_db::repositories::pane_layouts::find_global_pane_layout;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{SessionType, TaskStatus};
use kanvibe_process::exec::{ExecGitOptions, ShellCommandRunner};
use kanvibe_process::git_operations::{
    WorktreeEntry, get_default_branch, list_branches, list_worktrees, scan_git_repos,
    validate_git_repo,
};
use kanvibe_process::github_project_icon::GitHubProjectIconResolver;
use kanvibe_process::hooks::claude::get_claude_hooks_status;
use kanvibe_process::hooks::client_runtime::HookInstallContext;
use kanvibe_process::hooks::codex::get_codex_hooks_status;
use kanvibe_process::hooks::gemini::get_gemini_hooks_status;
use kanvibe_process::hooks::installer::{HookInstallScheduleOptions, KanvibeHooksInstaller};
use kanvibe_process::hooks::open_code::get_open_code_hooks_status;
use kanvibe_process::hooks::open_code_registry::OpenCodePluginRegistration;
use kanvibe_process::hooks::providers::{
    KanvibeHookProvider, KanvibeHookStatus, KanvibeHookStatuses,
};
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::kanvibe_project_state::KanvibeTaskSyncState;
use kanvibe_process::kanvibe_state_store::{KanvibeStateStore, ProjectRecords};
use kanvibe_process::project_color_service::ProjectColorService;
use kanvibe_process::ssh_config::SshConfigReader;
use kanvibe_process::task_state_service::TaskStateService;
use kanvibe_process::worktree::{
    PaneLayoutSelection, create_session_without_worktree, is_session_alive,
};

use super::ports::{
    AiSessionAttachmentRequest, AiSessionDetailRequest, AiSessionListRequest, AiSessionScope,
    BranchSessions, HookInstallFailure, HookInstallSuccess, ProjectHookOperations,
    ProjectIconSource, ProjectRepositoryProbe, ProjectStateFiles, TaskAiSessionReader,
};
use crate::terminal_ports::TaskTerminalSessions;

/// 로컬·원격 파일 접근과 `~/.ssh/config`로 저장소를 조회한다
pub struct HostProjectRepositoryProbe {
    files: Arc<HostFileAccess>,
    ssh_config: SshConfigReader,
}

impl HostProjectRepositoryProbe {
    pub fn new(files: Arc<HostFileAccess>) -> Self {
        let ssh_config = SshConfigReader::new(files.local_home_directory());
        Self { files, ssh_config }
    }
}

impl ProjectRepositoryProbe for HostProjectRepositoryProbe {
    fn local_home_directory(&self) -> &str {
        self.files.local_home_directory()
    }

    fn is_path_missing(&self, target_path: &str, ssh_host: Option<&str>) -> Result<bool, String> {
        self.files
            .is_path_missing(target_path, ssh_host)
            .map_err(|error| error.to_string())
    }

    fn list_worktrees(&self, repo_path: &str, ssh_host: Option<&str>) -> Vec<WorktreeEntry> {
        list_worktrees(&self.files, repo_path, ssh_host)
    }

    fn validate_git_repo(&self, repo_path: &str, ssh_host: Option<&str>) -> bool {
        validate_git_repo(&self.files, repo_path, ssh_host)
    }

    fn default_branch(&self, repo_path: &str, ssh_host: Option<&str>) -> String {
        get_default_branch(&self.files, repo_path, ssh_host)
    }

    fn list_branches(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
        should_refresh: bool,
    ) -> Result<Vec<String>, String> {
        list_branches(&self.files, repo_path, ssh_host, should_refresh)
            .map_err(|error| error.message)
    }

    fn scan_git_repos(&self, root_path: &str, ssh_host: Option<&str>) -> Vec<String> {
        scan_git_repos(&self.files, root_path, ssh_host)
    }

    fn exec_git(&self, command: &str, ssh_host: Option<&str>) -> Result<String, String> {
        self.files
            .runner()
            .exec_git(command, ssh_host, &ExecGitOptions::default())
            .map_err(|error| error.message)
    }

    fn available_hosts(&self) -> Vec<String> {
        self.ssh_config.available_hosts()
    }
}

/// worktree 세션 함수와 터미널 계층으로 브랜치 세션을 다룬다
pub struct HostBranchSessions {
    files: Arc<HostFileAccess>,
    terminal: Arc<dyn TaskTerminalSessions>,
    database: Arc<Database>,
}

impl HostBranchSessions {
    pub fn new(
        files: Arc<HostFileAccess>,
        terminal: Arc<dyn TaskTerminalSessions>,
        database: Arc<Database>,
    ) -> Self {
        Self {
            files,
            terminal,
            database,
        }
    }

    /// 프로젝트 id 없이 만드는 세션이라 전역 레이아웃을 쓴다(TS `getEffectivePaneLayout(undefined)`)
    fn global_pane_layout(&self) -> Result<Option<PaneLayoutSelection>, String> {
        let layout = find_global_pane_layout(&self.database.connection())
            .map_err(|error| error.to_string())?;
        Ok(layout.map(|layout| PaneLayoutSelection {
            layout_type: layout.layout_type,
            panes: layout.panes,
        }))
    }
}

impl BranchSessions for HostBranchSessions {
    fn is_session_alive(
        &self,
        session_type: SessionType,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> bool {
        let herdr: &dyn kanvibe_process::worktree::HerdrTaskWorkspaces = self.terminal.as_ref();
        is_session_alive(
            self.files.runner(),
            herdr,
            session_type,
            session_name,
            ssh_host,
        )
    }

    fn create_session_without_worktree(
        &self,
        project_path: &str,
        branch_name: &str,
        session_type: SessionType,
        ssh_host: Option<&str>,
        working_directory: Option<&str>,
    ) -> Result<String, String> {
        let pane_layout = || self.global_pane_layout();
        Ok(create_session_without_worktree(
            &self.files,
            project_path,
            branch_name,
            session_type,
            ssh_host,
            working_directory,
            &pane_layout,
        ))
    }

    fn detach_session(&self, task_id: &str, trigger_label: &str) {
        self.terminal.detach_session(task_id, trigger_label);
    }
}

/// hook 설치기와 provider별 상태 함수
pub struct InstallerProjectHookOperations {
    installer: Arc<KanvibeHooksInstaller>,
    context: HookInstallContext,
    open_code_registration: Arc<dyn OpenCodePluginRegistration>,
}

impl InstallerProjectHookOperations {
    pub fn new(
        installer: Arc<KanvibeHooksInstaller>,
        context: HookInstallContext,
        open_code_registration: Arc<dyn OpenCodePluginRegistration>,
    ) -> Self {
        Self {
            installer,
            context,
            open_code_registration,
        }
    }
}

impl ProjectHookOperations for InstallerProjectHookOperations {
    fn ensure_hooks_current(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<bool, String> {
        self.installer
            .ensure_hooks_current(target_path, task_id, ssh_host)
            .map_err(|error| error.to_string())
    }

    fn hooks_statuses(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<KanvibeHookStatuses, String> {
        self.installer
            .hooks_statuses(target_path, task_id, ssh_host)
            .map_err(|error| error.to_string())
    }

    fn install_hooks(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), String> {
        self.installer
            .install_hooks(target_path, task_id, ssh_host)
            .map_err(|error| error.to_string())
    }

    fn install_hook_provider(
        &self,
        target_path: &str,
        task_id: &str,
        provider: KanvibeHookProvider,
        ssh_host: Option<&str>,
    ) -> Result<(), String> {
        self.installer
            .install_hook_provider(target_path, task_id, provider, ssh_host)
            .map_err(|error| error.to_string())
    }

    fn schedule_hooks_install(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
        on_success: HookInstallSuccess,
        on_failure: HookInstallFailure,
    ) {
        let options = HookInstallScheduleOptions {
            on_success: Some(on_success),
            on_failure: Some(Box::new(move |error| on_failure(&error.to_string()))),
            ..HookInstallScheduleOptions::default()
        };
        self.installer
            .schedule_hooks_install(target_path, task_id, ssh_host, options);
    }

    fn provider_status(
        &self,
        provider: KanvibeHookProvider,
        target_path: &str,
        task_id: Option<&str>,
        ssh_host: Option<&str>,
    ) -> Result<KanvibeHookStatus, String> {
        let context = &self.context;
        let status = match provider {
            KanvibeHookProvider::Claude => {
                get_claude_hooks_status(context, target_path, task_id, ssh_host, None)
                    .map(KanvibeHookStatus::Claude)
            }
            KanvibeHookProvider::Gemini => {
                get_gemini_hooks_status(context, target_path, task_id, ssh_host, None)
                    .map(KanvibeHookStatus::Gemini)
            }
            KanvibeHookProvider::Codex => {
                get_codex_hooks_status(context, target_path, task_id, ssh_host, None)
                    .map(KanvibeHookStatus::Codex)
            }
            KanvibeHookProvider::OpenCode => get_open_code_hooks_status(
                context,
                self.open_code_registration.as_ref(),
                target_path,
                task_id,
                ssh_host,
                None,
            )
            .map(KanvibeHookStatus::OpenCode),
        };
        status.map_err(|error| error.to_string())
    }
}

/// M2 task 상태·프로젝트 색상 서비스로 `.kanvibe/` 파일을 다룬다
pub struct HostProjectStateFiles<'a> {
    task_state: TaskStateService<'a>,
    project_color: ProjectColorService<'a>,
}

impl<'a> HostProjectStateFiles<'a> {
    pub fn new(state_store: &'a dyn KanvibeStateStore, records: &'a dyn ProjectRecords) -> Self {
        Self {
            task_state: TaskStateService::new(state_store, records),
            project_color: ProjectColorService::new(state_store, records),
        }
    }
}

impl ProjectStateFiles for HostProjectStateFiles<'_> {
    fn read_task_status(
        &self,
        repo_path: Option<&str>,
        ssh_host: Option<&str>,
    ) -> Result<Option<TaskStatus>, String> {
        self.task_state
            .read_persisted_task_status_at_path(repo_path, ssh_host)
            .map_err(|error| error.to_string())
    }

    fn read_task_sync_state(
        &self,
        repo_path: Option<&str>,
        ssh_host: Option<&str>,
        project_repo_path: &str,
    ) -> Result<KanvibeTaskSyncState, String> {
        self.task_state
            .read_persisted_task_sync_state_at_path(repo_path, ssh_host, Some(project_repo_path))
            .map_err(|error| error.to_string())
    }

    fn persist_task_state(
        &self,
        repo_path: Option<&str>,
        task_id: &str,
        status: TaskStatus,
        ssh_host: Option<&str>,
    ) {
        self.task_state
            .persist_task_state_at_path(repo_path, task_id, status, ssh_host);
    }

    fn read_shared_project_color(&self, repo_path: &str, ssh_host: Option<&str>) -> Option<String> {
        self.project_color
            .read_shared_project_color(repo_path, ssh_host)
    }

    fn persist_project_color(&self, project: &Project) {
        self.project_color
            .persist_project_color_to_kanvibe_state(project);
    }

    fn sync_project_color(&self, project: &Project) -> Option<String> {
        self.project_color
            .sync_project_color_with_kanvibe_state(project)
    }
}

/// GitHub owner 아이콘을 origin remote에서 찾는다
pub struct GitHubProjectIconSource {
    resolver: GitHubProjectIconResolver,
    runner: Arc<dyn ShellCommandRunner>,
}

impl GitHubProjectIconSource {
    pub fn new(resolver: GitHubProjectIconResolver, runner: Arc<dyn ShellCommandRunner>) -> Self {
        Self { resolver, runner }
    }
}

impl ProjectIconSource for GitHubProjectIconSource {
    fn resolve_project_icon_data_url(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
    ) -> Option<String> {
        self.resolver
            .resolve_project_icon_data_url(self.runner.as_ref(), repo_path, ssh_host)
    }
}

/// 현재 시각(ms). 실행중 판정 창의 기준이다
pub type EpochMillisClock = dyn Fn() -> f64 + Send + Sync;

/// kanvibe-ai 세션 리더
pub struct LocalTaskAiSessionReader {
    host: SessionHost,
    running_panes: Arc<dyn RunningAgentPaneSource>,
    clock: Arc<EpochMillisClock>,
}

impl LocalTaskAiSessionReader {
    pub fn new(
        host: SessionHost,
        running_panes: Arc<dyn RunningAgentPaneSource>,
        clock: Arc<EpochMillisClock>,
    ) -> Self {
        Self {
            host,
            running_panes,
            clock,
        }
    }

    fn reader_context(&self, scope: &AiSessionScope) -> AiSessionReaderContext {
        let host = self.host.with_ssh_host(scope.ssh_host.as_deref());
        let mut context = AiSessionReaderContext::new(host, Some(&scope.worktree_path));
        context.repo_path = Some(scope.repo_path.clone());
        context
    }
}

impl TaskAiSessionReader for LocalTaskAiSessionReader {
    fn aggregate_sessions(
        &self,
        scope: &AiSessionScope,
        request: &AiSessionListRequest,
    ) -> Result<AggregatedAiSessionsResult, String> {
        let mut context = self.reader_context(scope);
        context.query = request.query.clone();
        context.cursor = request.cursor.clone();
        context.limit = request.limit;
        aggregate_ai_sessions(&context).map_err(|error| error.to_string())
    }

    fn session_detail(
        &self,
        scope: &AiSessionScope,
        request: &AiSessionDetailRequest,
    ) -> Result<Option<AggregatedAiSessionDetail>, String> {
        let mut context = self.reader_context(scope);
        context.query = request.query.clone();
        context.roles = request.roles.clone();
        get_ai_session_detail(
            &context,
            request.provider,
            &request.session_id,
            request.source_ref.as_deref(),
            request.cursor.as_deref(),
            Some(request.limit),
        )
        .map_err(|error| error.to_string())
    }

    fn session_attachment(
        &self,
        scope: &AiSessionScope,
        request: &AiSessionAttachmentRequest,
    ) -> Result<Option<AiSessionAttachmentContent>, String> {
        read_ai_session_attachment(
            &self.reader_context(scope),
            request.provider,
            &request.session_id,
            &request.attachment_id,
            request.source_ref.as_deref(),
        )
        .map_err(|error| error.to_string())
    }

    fn live_sessions(&self, scope: &AiSessionScope) -> Vec<LiveAiSession> {
        let panes = self
            .running_panes
            .list_running_agent_panes(scope.ssh_host.as_deref());
        read_live_ai_sessions(&self.reader_context(scope), &panes, (self.clock)())
    }

    fn running_agent_panes(&self) -> Vec<RunningAgentPane> {
        self.running_panes.list_running_agent_panes(None)
    }

    fn agent_call_graph(
        &self,
        scope: &AiSessionScope,
        provider: AiSessionProvider,
        session_id: &str,
    ) -> AgentCallGraph {
        read_agent_call_graph(
            &self.reader_context(scope),
            provider,
            session_id,
            (self.clock)(),
        )
    }
}
