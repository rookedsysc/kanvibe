//! `ports`의 실제 구현. 데스크탑 서비스에 그대로 위임하고, 모바일이 따로 정하는 것은 오류 분류뿐이다.

use std::collections::BTreeMap;
use std::sync::Arc;

use kanvibe_ai::accounts::account_service::{
    AiAccountMutationResult, AiAccountService, AiAccountSummary,
};
use kanvibe_ai::accounts::mobile_login_bridge::{
    MobileAiLoginError, MobileLoginBridge, MobileLoginOpenRequest,
};
use kanvibe_ai::accounts::usage_service::AiUsageService;
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
use kanvibe_process::exec::ExecGitOptions;
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::remote_session_dependency::SessionDependencyStatus;
use kanvibe_process::task_target_propagation::{
    HookServerUrlResolver, PeerTargetResult, TaskTargetPropagation,
};
use kanvibe_services::ServiceResult;
use kanvibe_services::background_task_sync::{BackgroundTaskSync, BackgroundTaskSyncRunResult};
use kanvibe_services::diff::{
    DiffService, FileSaveResult, TaskWorkspaceFile, TaskWorkspaceFileFailure,
};
use kanvibe_services::kanban::{
    CreateTaskInput, KanbanService, LoadMoreDoneResponse, SearchableTask, SessionTypeChangeResult,
    SessionTypeChangeTargets, TaskDeletion, TaskDeletionError, TaskDeletionOptions, TaskUpdate,
    TaskWithProject, TasksByStatusWithMeta,
};
use kanvibe_services::project::ports::{
    AiSessionAttachmentRequest, AiSessionDetailRequest, AiSessionListRequest,
};
use kanvibe_services::project::{ProjectService, ScanResult};
use kanvibe_services::session_dependency::SessionDependencyService;
use kanvibe_terminal::terminal_bridge::TerminalBridge;
use kanvibe_terminal::terminal_tab_service::TerminalTabService;

use crate::error::{MobileError, MobileResult};
use crate::ports::{
    AiAccountDirectory, AiHistoryReadError, AiLoginSessions, AiUsageSource, BackgroundSyncControl,
    DiffFiles, PeerTaskRegistration, ProjectCatalog, RemoteImageUpload, SessionDependencyCheck,
    TaskBoard, TerminalTabListing, TerminalTabs,
};

impl TaskBoard for KanbanService {
    fn get_tasks_by_status(&self) -> ServiceResult<TasksByStatusWithMeta> {
        KanbanService::get_tasks_by_status(self)
    }

    fn get_task_by_id(&self, task_id: &str) -> ServiceResult<Option<TaskWithProject>> {
        KanbanService::get_task_by_id(self, task_id)
    }

    fn get_searchable_tasks(&self) -> ServiceResult<Vec<SearchableTask>> {
        KanbanService::get_searchable_tasks(self)
    }

    fn get_more_done_tasks(
        &self,
        offset: usize,
        limit: usize,
    ) -> ServiceResult<LoadMoreDoneResponse> {
        KanbanService::get_more_done_tasks(self, offset, limit)
    }

    fn create_task(&self, input: &CreateTaskInput) -> ServiceResult<KanbanTask> {
        KanbanService::create_task(self, input)
    }

    fn update_task(
        &self,
        task_id: &str,
        updates: &TaskUpdate,
    ) -> ServiceResult<Option<KanbanTask>> {
        KanbanService::update_task(self, task_id, updates)
    }

    fn update_task_status(
        &self,
        task_id: &str,
        status: TaskStatus,
    ) -> ServiceResult<Option<KanbanTask>> {
        KanbanService::update_task_status(self, task_id, status)
    }

    fn update_project_color(&self, project_id: &str, color: &str) -> ServiceResult<()> {
        KanbanService::update_project_color(self, project_id, color)
    }

    fn get_task_id_by_project_and_branch(
        &self,
        project_id: &str,
        branch_name: &str,
    ) -> ServiceResult<Option<String>> {
        KanbanService::get_task_id_by_project_and_branch(self, project_id, branch_name)
    }

    fn branch_from_task(
        &self,
        task_id: &str,
        project_id: &str,
        base_branch: &str,
        branch_name: &str,
        session_type: SessionType,
    ) -> ServiceResult<Option<KanbanTask>> {
        KanbanService::branch_from_task(
            self,
            task_id,
            project_id,
            base_branch,
            branch_name,
            session_type,
        )
    }

    fn fetch_and_save_pr_url(&self, task_id: &str) -> ServiceResult<Option<String>> {
        KanbanService::fetch_and_save_pr_url(self, task_id, false)
    }

    fn change_task_session_type(
        &self,
        task_id: &str,
        session_type: SessionType,
        should_close_existing_session: bool,
    ) -> ServiceResult<Option<KanbanTask>> {
        KanbanService::change_task_session_type(
            self,
            task_id,
            session_type,
            should_close_existing_session,
        )
    }

    fn is_task_session_alive(&self, task_id: &str) -> ServiceResult<bool> {
        KanbanService::is_task_session_alive(self, task_id)
    }

    fn get_session_type_change_targets(
        &self,
        session_type: SessionType,
    ) -> ServiceResult<SessionTypeChangeTargets> {
        KanbanService::get_session_type_change_targets(self, session_type)
    }

    fn change_all_task_session_types(
        &self,
        session_type: SessionType,
        should_close_existing_sessions: bool,
    ) -> ServiceResult<SessionTypeChangeResult> {
        KanbanService::change_all_task_session_types(
            self,
            session_type,
            should_close_existing_sessions,
        )
    }

    fn delete_task_across_targets(
        &self,
        task_id: &str,
        options: &TaskDeletionOptions,
    ) -> Result<TaskDeletion, TaskDeletionError> {
        KanbanService::delete_task_across_targets(self, task_id, options)
    }

    fn remove_task_registration(&self, task_id: &str) -> ServiceResult<bool> {
        KanbanService::remove_task_registration(self, task_id)
    }

    fn delete_tasks(&self, task_ids: &[String]) -> Result<Vec<String>, TaskDeletionError> {
        KanbanService::delete_tasks(self, task_ids)
    }
}

/// 기록 리더는 io 오류를 문장으로만 넘긴다. 문장 끝의 errno(ENOENT=2, ENOTDIR=20)로 지워진 원본 파일을 가린다
fn classify_history_failure(message: String) -> AiHistoryReadError {
    let is_missing_source = ["(os error 2)", "(os error 20)"]
        .iter()
        .any(|marker| message.contains(marker));
    if is_missing_source {
        AiHistoryReadError::SourceMissing(message)
    } else {
        AiHistoryReadError::Failed(message)
    }
}

impl ProjectCatalog for ProjectService {
    fn get_all_projects(&self) -> ServiceResult<Vec<Project>> {
        ProjectService::get_all_projects(self)
    }

    fn get_project_by_id(&self, project_id: &str) -> ServiceResult<Option<Project>> {
        ProjectService::get_project_by_id(self, project_id)
    }

    fn get_project_branches(&self, project_id: &str) -> ServiceResult<Vec<String>> {
        ProjectService::get_project_branches(self, project_id)
    }

    fn get_available_hosts(&self) -> Vec<String> {
        ProjectService::get_available_hosts(self)
    }

    fn list_subdirectories(&self, parent_path: &str, ssh_host: Option<&str>) -> Vec<String> {
        ProjectService::list_subdirectories(self, parent_path, ssh_host)
    }

    fn scan_and_register_projects(
        &self,
        root_path: &str,
        ssh_host: Option<&str>,
    ) -> ServiceResult<ScanResult> {
        ProjectService::scan_and_register_projects(self, root_path, ssh_host)
    }

    fn delete_project(&self, project_id: &str) -> ServiceResult<bool> {
        ProjectService::delete_project(self, project_id)
    }

    fn get_task_ai_sessions(
        &self,
        task_id: &str,
        request: &AiSessionListRequest,
    ) -> ServiceResult<AggregatedAiSessionsResult> {
        ProjectService::get_task_ai_sessions(self, task_id, request)
    }

    fn get_task_ai_session_detail(
        &self,
        task_id: &str,
        request: &AiSessionDetailRequest,
    ) -> Result<Option<AggregatedAiSessionDetail>, AiHistoryReadError> {
        ProjectService::get_task_ai_session_detail(self, task_id, request)
            .map_err(|error| classify_history_failure(error.message))
    }

    fn get_task_ai_session_attachment(
        &self,
        task_id: &str,
        request: &AiSessionAttachmentRequest,
    ) -> ServiceResult<Option<AiSessionAttachmentContent>> {
        ProjectService::get_task_ai_session_attachment(self, task_id, request)
    }

    fn get_task_live_ai_sessions(&self, task_id: &str) -> ServiceResult<LiveAiSessionsResult> {
        ProjectService::get_task_live_ai_sessions(self, task_id)
    }

    fn get_running_agent_panes(&self) -> Vec<RunningAgentPane> {
        ProjectService::get_running_agent_panes(self)
    }

    fn get_task_agent_call_graph(
        &self,
        task_id: &str,
        provider: AiSessionProvider,
        session_id: &str,
    ) -> ServiceResult<Option<AgentCallGraph>> {
        ProjectService::get_task_agent_call_graph(self, task_id, provider, session_id)
    }
}

/// diff 서비스와 심볼릭 링크 확인에 쓰는 파일 접근 문맥을 묶는다
pub struct DiffFileAccess {
    pub diff: DiffService,
    pub files: Arc<HostFileAccess>,
}

impl DiffFiles for DiffFileAccess {
    fn get_git_diff_files(&self, task_id: &str) -> Vec<DiffFile> {
        self.diff.get_git_diff_files(task_id)
    }

    fn get_task_diff_stats(&self, task_ids: &[String]) -> BTreeMap<String, TaskDiffStats> {
        self.diff.get_task_diff_stats(task_ids)
    }

    fn get_original_file_content(&self, task_id: &str, file_path: &str) -> String {
        self.diff.get_original_file_content(task_id, file_path)
    }

    fn get_file_content(&self, task_id: &str, file_path: &str) -> String {
        self.diff.get_file_content(task_id, file_path)
    }

    fn save_file_content(&self, task_id: &str, file_path: &str, content: &str) -> FileSaveResult {
        self.diff.save_file_content(task_id, file_path, content)
    }

    fn read_task_workspace_file(
        &self,
        task_id: &str,
        file_path: &str,
    ) -> Result<TaskWorkspaceFile, TaskWorkspaceFileFailure> {
        self.diff.read_task_workspace_file(task_id, file_path)
    }

    fn find_symbolic_link_path(
        &self,
        paths: &[String],
        ssh_host: Option<&str>,
    ) -> Result<Option<String>, String> {
        self.files
            .find_symbolic_link_path(paths, ssh_host, &ExecGitOptions::default())
            .map_err(|error| error.to_string())
    }
}

impl TerminalTabs for TerminalTabService {
    fn list_terminal_tabs(&self, task_id: &str) -> MobileResult<TerminalTabListing> {
        let listing =
            TerminalTabService::list_terminal_tabs(self, task_id).map_err(MobileError::internal)?;
        Ok(TerminalTabListing {
            is_ok: listing.is_ok,
            tabs: listing.tabs,
        })
    }

    fn rename_terminal_tab(&self, task_id: &str, tab_id: &str, name: &str) -> MobileResult<bool> {
        TerminalTabService::rename_terminal_tab(self, task_id, tab_id, name)
            .map(|result| result.is_ok)
            .map_err(MobileError::internal)
    }

    fn move_terminal_tab(&self, task_id: &str, tab_id: &str, index: i64) -> MobileResult<bool> {
        TerminalTabService::move_terminal_tab(self, task_id, tab_id, index)
            .map(|result| result.is_ok)
            .map_err(MobileError::internal)
    }
}

impl SessionDependencyCheck for SessionDependencyService {
    fn get_session_dependency_status(
        &self,
        session_type: SessionType,
        ssh_host: Option<&str>,
    ) -> ServiceResult<SessionDependencyStatus> {
        SessionDependencyService::get_session_dependency_status(self, session_type, ssh_host)
    }
}

impl BackgroundSyncControl for BackgroundTaskSync {
    fn run_background_task_sync_now(&self) -> ServiceResult<BackgroundTaskSyncRunResult> {
        BackgroundTaskSync::run_background_task_sync_now(self)
    }

    fn get_background_task_sync_status(&self) -> BackgroundTaskSyncStatus {
        BackgroundTaskSync::get_background_task_sync_status(self)
    }

    fn start_background_task_sync_now(&self) -> BackgroundTaskSyncStatus {
        BackgroundTaskSync::start_background_task_sync_now(self)
    }
}

/// kanvibe-process의 전파 명령으로 다른 데스크탑에 task 등록을 알린다
pub struct ProcessPeerTaskRegistration {
    pub files: Arc<HostFileAccess>,
    pub hook_server_url: Arc<HookServerUrlResolver>,
}

impl PeerTaskRegistration for ProcessPeerTaskRegistration {
    fn propagate_task_registration_to_peers(
        &self,
        project_repo_path: &str,
        branch_name: &str,
        ssh_host: Option<&str>,
    ) -> Vec<PeerTargetResult> {
        TaskTargetPropagation::new(&self.files, self.hook_server_url.as_ref())
            .propagate_task_registration_to_peers(project_repo_path, branch_name, ssh_host)
    }
}

impl RemoteImageUpload for TerminalBridge {
    fn paste_image_to_remote_terminal(
        &self,
        task_id: &str,
        image_data_url: &str,
    ) -> Result<String, String> {
        TerminalBridge::paste_image_to_remote_terminal(self, task_id, image_data_url)
    }
}

impl AiAccountDirectory for AiAccountService {
    fn list_ai_accounts(&self) -> Result<Vec<AiAccountSummary>, String> {
        AiAccountService::list_ai_accounts(self).map_err(|error| error.to_string())
    }

    fn add_ai_account(
        &self,
        provider: AiUsageProvider,
        account_name: &str,
    ) -> Result<AiAccountMutationResult, String> {
        AiAccountService::add_ai_account(self, provider, account_name)
            .map_err(|error| error.to_string())
    }

    fn remove_ai_account(
        &self,
        provider: AiUsageProvider,
        account_root: &str,
    ) -> Result<AiAccountMutationResult, String> {
        AiAccountService::remove_ai_account(self, provider, account_root)
            .map_err(|error| error.to_string())
    }
}

impl AiUsageSource for AiUsageService {
    fn get_ai_usage_snapshot(&self) -> Result<AiUsageSnapshot, String> {
        AiUsageService::get_ai_usage_snapshot(self, false)
            .map(|snapshot| snapshot.as_ref().clone())
            .map_err(|error| error.to_string())
    }
}

impl AiLoginSessions for MobileLoginBridge {
    fn open(&self, request: MobileLoginOpenRequest) -> Result<(), MobileAiLoginError> {
        self.open_mobile_ai_login_session(request)
    }

    fn write(
        &self,
        device_id: &str,
        socket_id: &str,
        data: &str,
    ) -> Result<(), MobileAiLoginError> {
        self.write_mobile_ai_login(device_id, socket_id, data)
    }

    fn resize(
        &self,
        device_id: &str,
        socket_id: &str,
        columns: f64,
        rows: f64,
    ) -> Result<(), MobileAiLoginError> {
        self.resize_mobile_ai_login(device_id, socket_id, columns, rows)
    }

    fn close(&self, device_id: &str, socket_id: &str) {
        self.close_mobile_ai_login(device_id, socket_id);
    }

    fn close_for_device(&self, device_id: &str) {
        self.close_mobile_ai_logins_for_device(device_id);
    }

    fn close_for_account(&self, account_root: &str) {
        self.close_mobile_ai_logins_for_account(account_root);
    }

    fn close_all(&self) {
        self.close_all_mobile_ai_logins();
    }
}
