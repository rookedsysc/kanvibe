//! 프로젝트 서비스가 바깥 계층에 맡기는 일. 서비스는 이 trait만 받아, 앱은 `adapters`의 실제 구현을,
//! 테스트는 호출을 기록하는 가짜를 넘긴다. TS 테스트가 `vi.mock`으로 바꾸던 모듈 경계와 같다.

use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{
    AgentCallGraph, AggregatedAiSessionDetail, AggregatedAiSessionsResult, AiMessageRole,
    AiSessionAttachmentContent, LiveAiSession, RunningAgentPane,
};
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{SessionType, TaskStatus};
use kanvibe_process::git_operations::WorktreeEntry;
use kanvibe_process::hooks::providers::{
    KanvibeHookProvider, KanvibeHookStatus, KanvibeHookStatuses,
};
use kanvibe_process::kanvibe_project_state::KanvibeTaskSyncState;

/// 저장소 경로·git 조회(TS `gitOperations`, `hostFileAccess.isPathMissing`, `sshConfig.getAvailableHosts`)
pub trait ProjectRepositoryProbe: Send + Sync {
    /// `~` 경로를 로컬에서 펼칠 때 쓰는 홈 디렉터리
    fn local_home_directory(&self) -> &str;
    /// 경로가 없으면 `true`. 연결 실패처럼 판정할 수 없으면 오류다
    fn is_path_missing(&self, target_path: &str, ssh_host: Option<&str>) -> Result<bool, String>;
    /// git worktree 목록(bare 포함). 조회에 실패하면 빈 목록이다
    fn list_worktrees(&self, repo_path: &str, ssh_host: Option<&str>) -> Vec<WorktreeEntry>;
    fn validate_git_repo(&self, repo_path: &str, ssh_host: Option<&str>) -> bool;
    fn default_branch(&self, repo_path: &str, ssh_host: Option<&str>) -> String;
    fn list_branches(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
        should_refresh: bool,
    ) -> Result<Vec<String>, String>;
    fn scan_git_repos(&self, root_path: &str, ssh_host: Option<&str>) -> Vec<String>;
    /// `execGit`. 앞뒤 공백을 뺀 stdout을 돌려준다
    fn exec_git(&self, command: &str, ssh_host: Option<&str>) -> Result<String, String>;
    /// `~/.ssh/config`에 적힌 호스트 별칭
    fn available_hosts(&self) -> Vec<String>;
}

/// 브랜치 세션 이름 정하기·생존 확인과 터미널 끊기(TS `worktree`, `terminal.detachSession`)
pub trait BranchSessions: Send + Sync {
    fn is_session_alive(
        &self,
        session_type: SessionType,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> bool;
    /// worktree 없이 기존 디렉터리에서 쓸 세션 이름. zellij는 레이아웃 파일도 남긴다
    fn create_session_without_worktree(
        &self,
        project_path: &str,
        branch_name: &str,
        session_type: SessionType,
        ssh_host: Option<&str>,
        working_directory: Option<&str>,
    ) -> Result<String, String>;
    fn detach_session(&self, task_id: &str, trigger_label: &str);
}

/// 예약 설치가 끝난 뒤 부르는 callback
pub type HookInstallSuccess = Box<dyn FnOnce() + Send>;
pub type HookInstallFailure = Box<dyn FnOnce(&str) + Send>;

/// hook 설치·상태 조회(TS `kanvibeHooksInstaller`, `*HooksSetup.get*HooksStatus`)
pub trait ProjectHookOperations: Send + Sync {
    /// 현재 설치를 확인하고 복구했으면 `true`
    fn ensure_hooks_current(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<bool, String>;
    fn hooks_statuses(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<KanvibeHookStatuses, String>;
    fn install_hooks(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), String>;
    fn install_hook_provider(
        &self,
        target_path: &str,
        task_id: &str,
        provider: KanvibeHookProvider,
        ssh_host: Option<&str>,
    ) -> Result<(), String>;
    /// 원격 설치를 백그라운드로 예약한다. 결과는 callback으로만 알린다
    fn schedule_hooks_install(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
        on_success: HookInstallSuccess,
        on_failure: HookInstallFailure,
    );
    fn provider_status(
        &self,
        provider: KanvibeHookProvider,
        target_path: &str,
        task_id: Option<&str>,
        ssh_host: Option<&str>,
    ) -> Result<KanvibeHookStatus, String>;
}

/// 저장소의 `.kanvibe/` 공유 파일(TS `kanvibeTaskStateService`, `kanvibeProjectColorService`)
pub trait ProjectStateFiles: Send + Sync {
    fn read_task_status(
        &self,
        repo_path: Option<&str>,
        ssh_host: Option<&str>,
    ) -> Result<Option<TaskStatus>, String>;
    fn read_task_sync_state(
        &self,
        repo_path: Option<&str>,
        ssh_host: Option<&str>,
        project_repo_path: &str,
    ) -> Result<KanvibeTaskSyncState, String>;
    /// 실패는 로그만 남긴다
    fn persist_task_state(
        &self,
        repo_path: Option<&str>,
        task_id: &str,
        status: TaskStatus,
        ssh_host: Option<&str>,
    );
    fn read_shared_project_color(&self, repo_path: &str, ssh_host: Option<&str>) -> Option<String>;
    fn persist_project_color(&self, project: &Project);
    /// 공유 색상을 worktree에 퍼뜨리고 DB에 반영할 색상을 돌려준다
    fn sync_project_color(&self, project: &Project) -> Option<String>;
}

/// GitHub owner 아이콘(TS `githubProjectIcon.resolveProjectIconDataUrl`)
pub trait ProjectIconSource: Send + Sync {
    fn resolve_project_icon_data_url(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
    ) -> Option<String>;
}

/// task 하나가 보는 AI 세션 기록의 범위
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiSessionScope {
    pub worktree_path: String,
    pub repo_path: String,
    pub ssh_host: Option<String>,
}

/// 세션 상세 조회 조건
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiSessionDetailRequest {
    pub provider: AiSessionProvider,
    pub session_id: String,
    pub source_ref: Option<String>,
    pub cursor: Option<String>,
    pub limit: usize,
    pub query: Option<String>,
    pub roles: Option<Vec<AiMessageRole>>,
}

/// 세션 목록 조회 조건
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AiSessionListRequest {
    pub query: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

/// 첨부 하나를 가리키는 값
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiSessionAttachmentRequest {
    pub provider: AiSessionProvider,
    pub session_id: String,
    pub attachment_id: String,
    pub source_ref: Option<String>,
}

/// AI 세션 기록 읽기(TS `aiSessions/*`)
pub trait TaskAiSessionReader: Send + Sync {
    fn aggregate_sessions(
        &self,
        scope: &AiSessionScope,
        request: &AiSessionListRequest,
    ) -> Result<AggregatedAiSessionsResult, String>;
    fn session_detail(
        &self,
        scope: &AiSessionScope,
        request: &AiSessionDetailRequest,
    ) -> Result<Option<AggregatedAiSessionDetail>, String>;
    fn session_attachment(
        &self,
        scope: &AiSessionScope,
        request: &AiSessionAttachmentRequest,
    ) -> Result<Option<AiSessionAttachmentContent>, String>;
    fn live_sessions(&self, scope: &AiSessionScope) -> Vec<LiveAiSession>;
    fn running_agent_panes(&self) -> Vec<RunningAgentPane>;
    fn agent_call_graph(
        &self,
        scope: &AiSessionScope,
        provider: AiSessionProvider,
        session_id: &str,
    ) -> AgentCallGraph;
}

/// 상세 조회의 기본 메시지 수(TS `getTaskAiSessionDetail`의 `limit = 20`)
pub const DEFAULT_AI_SESSION_DETAIL_LIMIT: usize = 20;
