//! 모바일 업무 계층이 부르는 데스크탑 서비스. TS가 모듈을 통째로 가져다 쓰던 자리를, 실제로 부르는 메서드만 담은
//! trait으로 받는다. 앱은 `adapters`의 실제 구현(보드·프로젝트·diff·터미널 탭·AI 서비스)을 넘기고, 테스트는 가짜를 넘긴다.

use std::collections::BTreeMap;

use kanvibe_ai::accounts::account_service::{AiAccountMutationResult, AiAccountSummary};
use kanvibe_ai::accounts::mobile_login_bridge::{MobileAiLoginError, MobileLoginOpenRequest};
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{
    AgentCallGraph, AggregatedAiSessionDetail, AggregatedAiSessionsResult,
    AiSessionAttachmentContent, LiveAiSessionsResult, RunningAgentPane,
};
use kanvibe_ai::usage::types::{AiUsageProvider, AiUsageSnapshot};
use kanvibe_domain::background_sync::BackgroundTaskSyncStatus;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskStatus};
use kanvibe_domain::task_diff_stats::{DiffFile, TaskDiffStats};
use kanvibe_domain::terminal_tabs::TerminalTab;
use kanvibe_process::remote_session_dependency::SessionDependencyStatus;
use kanvibe_process::task_target_propagation::PeerTargetResult;
use kanvibe_services::ServiceResult;
use kanvibe_services::background_task_sync::BackgroundTaskSyncRunResult;
use kanvibe_services::diff::{FileSaveResult, TaskWorkspaceFile, TaskWorkspaceFileFailure};
use kanvibe_services::kanban::{
    CreateTaskInput, LoadMoreDoneResponse, SearchableTask, SessionTypeChangeResult,
    SessionTypeChangeTargets, TaskDeletion, TaskDeletionError, TaskDeletionOptions, TaskUpdate,
    TaskWithProject, TasksByStatusWithMeta,
};
use kanvibe_services::project::ScanResult;
use kanvibe_services::project::ports::{
    AiSessionAttachmentRequest, AiSessionDetailRequest, AiSessionListRequest,
};

use crate::error::MobileResult;

/// 보드 task 조회·변경(TS `kanbanService`)
pub trait TaskBoard: Send + Sync {
    fn get_tasks_by_status(&self) -> ServiceResult<TasksByStatusWithMeta>;
    fn get_task_by_id(&self, task_id: &str) -> ServiceResult<Option<TaskWithProject>>;
    fn get_searchable_tasks(&self) -> ServiceResult<Vec<SearchableTask>>;
    fn get_more_done_tasks(
        &self,
        offset: usize,
        limit: usize,
    ) -> ServiceResult<LoadMoreDoneResponse>;
    fn create_task(&self, input: &CreateTaskInput) -> ServiceResult<KanbanTask>;
    fn update_task(&self, task_id: &str, updates: &TaskUpdate)
    -> ServiceResult<Option<KanbanTask>>;
    fn update_task_status(
        &self,
        task_id: &str,
        status: TaskStatus,
    ) -> ServiceResult<Option<KanbanTask>>;
    fn update_project_color(&self, project_id: &str, color: &str) -> ServiceResult<()>;
    fn get_task_id_by_project_and_branch(
        &self,
        project_id: &str,
        branch_name: &str,
    ) -> ServiceResult<Option<String>>;
    fn branch_from_task(
        &self,
        task_id: &str,
        project_id: &str,
        base_branch: &str,
        branch_name: &str,
        session_type: SessionType,
    ) -> ServiceResult<Option<KanbanTask>>;
    /// 사용자가 직접 요청한 PR 조회(`isAutomatic: false`)
    fn fetch_and_save_pr_url(&self, task_id: &str) -> ServiceResult<Option<String>>;
    fn change_task_session_type(
        &self,
        task_id: &str,
        session_type: SessionType,
        should_close_existing_session: bool,
    ) -> ServiceResult<Option<KanbanTask>>;
    fn is_task_session_alive(&self, task_id: &str) -> ServiceResult<bool>;
    fn get_session_type_change_targets(
        &self,
        session_type: SessionType,
    ) -> ServiceResult<SessionTypeChangeTargets>;
    fn change_all_task_session_types(
        &self,
        session_type: SessionType,
        should_close_existing_sessions: bool,
    ) -> ServiceResult<SessionTypeChangeResult>;
    fn delete_task_across_targets(
        &self,
        task_id: &str,
        options: &TaskDeletionOptions,
    ) -> Result<TaskDeletion, TaskDeletionError>;
    fn remove_task_registration(&self, task_id: &str) -> ServiceResult<bool>;
    fn delete_tasks(&self, task_ids: &[String]) -> Result<Vec<String>, TaskDeletionError>;
}

/// AI 기록 상세를 읽지 못한 이유. 캐시한 원본 파일이 지워진 경우만 새 경로를 다시 찾는다(TS `ENOENT`·`ENOTDIR`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiHistoryReadError {
    SourceMissing(String),
    Failed(String),
}

/// 프로젝트·폴더 조회와 task에 묶인 AI 세션 조회(TS `projectService`)
pub trait ProjectCatalog: Send + Sync {
    fn get_all_projects(&self) -> ServiceResult<Vec<Project>>;
    fn get_project_by_id(&self, project_id: &str) -> ServiceResult<Option<Project>>;
    fn get_project_branches(&self, project_id: &str) -> ServiceResult<Vec<String>>;
    fn get_available_hosts(&self) -> Vec<String>;
    fn list_subdirectories(&self, parent_path: &str, ssh_host: Option<&str>) -> Vec<String>;
    fn scan_and_register_projects(
        &self,
        root_path: &str,
        ssh_host: Option<&str>,
    ) -> ServiceResult<ScanResult>;
    fn delete_project(&self, project_id: &str) -> ServiceResult<bool>;
    fn get_task_ai_sessions(
        &self,
        task_id: &str,
        request: &AiSessionListRequest,
    ) -> ServiceResult<AggregatedAiSessionsResult>;
    fn get_task_ai_session_detail(
        &self,
        task_id: &str,
        request: &AiSessionDetailRequest,
    ) -> Result<Option<AggregatedAiSessionDetail>, AiHistoryReadError>;
    fn get_task_ai_session_attachment(
        &self,
        task_id: &str,
        request: &AiSessionAttachmentRequest,
    ) -> ServiceResult<Option<AiSessionAttachmentContent>>;
    fn get_task_live_ai_sessions(&self, task_id: &str) -> ServiceResult<LiveAiSessionsResult>;
    fn get_running_agent_panes(&self) -> Vec<RunningAgentPane>;
    fn get_task_agent_call_graph(
        &self,
        task_id: &str,
        provider: AiSessionProvider,
        session_id: &str,
    ) -> ServiceResult<Option<AgentCallGraph>>;
}

/// worktree 변경 파일 조회·저장(TS `diffService`, `hostFileAccess.findSymbolicLinkPath`)
pub trait DiffFiles: Send + Sync {
    fn get_git_diff_files(&self, task_id: &str) -> Vec<DiffFile>;
    fn get_task_diff_stats(&self, task_ids: &[String]) -> BTreeMap<String, TaskDiffStats>;
    fn get_original_file_content(&self, task_id: &str, file_path: &str) -> String;
    fn get_file_content(&self, task_id: &str, file_path: &str) -> String;
    fn save_file_content(&self, task_id: &str, file_path: &str, content: &str) -> FileSaveResult;
    fn read_task_workspace_file(
        &self,
        task_id: &str,
        file_path: &str,
    ) -> Result<TaskWorkspaceFile, TaskWorkspaceFileFailure>;
    /// 경로 가운데 심볼릭 링크가 하나라도 있으면 그 경로
    fn find_symbolic_link_path(
        &self,
        paths: &[String],
        ssh_host: Option<&str>,
    ) -> Result<Option<String>, String>;
}

/// 터미널 탭 목록·이름 변경·이동 결과. TS `{ ok, tabs }`에서 쓰는 값만 담는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTabListing {
    pub is_ok: bool,
    pub tabs: Vec<TerminalTab>,
}

/// 터미널 탭 조작(TS `terminalTabService`). 성공 여부만 돌려주고, 실패 문구는 모바일이 정한다
pub trait TerminalTabs: Send + Sync {
    fn list_terminal_tabs(&self, task_id: &str) -> MobileResult<TerminalTabListing>;
    fn rename_terminal_tab(&self, task_id: &str, tab_id: &str, name: &str) -> MobileResult<bool>;
    fn move_terminal_tab(&self, task_id: &str, tab_id: &str, index: i64) -> MobileResult<bool>;
}

/// 세션 도구 설치 여부(TS `remoteSessionDependency.getSessionDependencyStatus`)
pub trait SessionDependencyCheck: Send + Sync {
    fn get_session_dependency_status(
        &self,
        session_type: SessionType,
        ssh_host: Option<&str>,
    ) -> ServiceResult<SessionDependencyStatus>;
}

/// 백그라운드 동기화(TS `backgroundTaskSyncService`)
pub trait BackgroundSyncControl: Send + Sync {
    fn run_background_task_sync_now(&self) -> ServiceResult<BackgroundTaskSyncRunResult>;
    fn get_background_task_sync_status(&self) -> BackgroundTaskSyncStatus;
    fn start_background_task_sync_now(&self) -> BackgroundTaskSyncStatus;
}

/// 같은 프로젝트를 다루는 다른 데스크탑에 task 등록만 전파한다(TS `propagateTaskRegistrationToPeers`)
pub trait PeerTaskRegistration: Send + Sync {
    fn propagate_task_registration_to_peers(
        &self,
        project_repo_path: &str,
        branch_name: &str,
        ssh_host: Option<&str>,
    ) -> Vec<PeerTargetResult>;
}

/// pane 미러 계층이 맡는 pane 확인과 입력(TS `getTaskSurfaces`·`writeToPane`). 실패는 이름 없는 오류로 올린다
pub trait TaskPaneAccess: Send + Sync {
    fn has_task_pane(&self, task_id: &str, pane_id: &str) -> MobileResult<bool>;
    fn write_to_pane(&self, task_id: &str, pane_id: &str, input: &str) -> MobileResult<()>;
}

/// 원격 터미널로 이미지 파일을 올린다(TS `terminalBridge.pasteImageToRemoteTerminal`). 성공하면 원격 경로
pub trait RemoteImageUpload: Send + Sync {
    fn paste_image_to_remote_terminal(
        &self,
        task_id: &str,
        image_data_url: &str,
    ) -> Result<String, String>;
}

/// AI 계정 목록·추가·삭제(TS `aiAccountService`)
pub trait AiAccountDirectory: Send + Sync {
    fn list_ai_accounts(&self) -> Result<Vec<AiAccountSummary>, String>;
    fn add_ai_account(
        &self,
        provider: AiUsageProvider,
        account_name: &str,
    ) -> Result<AiAccountMutationResult, String>;
    fn remove_ai_account(
        &self,
        provider: AiUsageProvider,
        account_root: &str,
    ) -> Result<AiAccountMutationResult, String>;
}

/// AI 사용량 조회(TS `aiUsageService.getAiUsageSnapshot()`, 최근 결과를 재사용하지 않는 호출)
pub trait AiUsageSource: Send + Sync {
    fn get_ai_usage_snapshot(&self) -> Result<AiUsageSnapshot, String>;
}

/// 모바일 기기가 여는 provider 로그인 PTY(TS `mobileAiLoginBridge`)
pub trait AiLoginSessions: Send + Sync {
    fn open(&self, request: MobileLoginOpenRequest) -> Result<(), MobileAiLoginError>;
    fn write(&self, device_id: &str, socket_id: &str, data: &str)
    -> Result<(), MobileAiLoginError>;
    fn resize(
        &self,
        device_id: &str,
        socket_id: &str,
        columns: f64,
        rows: f64,
    ) -> Result<(), MobileAiLoginError>;
    fn close(&self, device_id: &str, socket_id: &str);
    fn close_for_device(&self, device_id: &str);
    fn close_for_account(&self, account_root: &str);
    fn close_all(&self);
}
