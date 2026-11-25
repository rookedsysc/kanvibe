//! 테스트가 공유하는 임시 DB와 데스크탑 서비스 가짜. 가짜는 받은 호출을 문자열로 남기고 미리 정한 값을 돌려준다.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{
    AgentCallGraph, AggregatedAiSessionDetail, AggregatedAiSessionsResult,
    AiSessionAttachmentContent, LiveAiSessionsResult, RunningAgentPane,
};
use kanvibe_db::{Database, DatabaseLocation};
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
    TaskWithProject, TasksByStatus, TasksByStatusWithMeta,
};
use kanvibe_services::notification_store::{DatabaseNotificationListStorage, NotificationStore};
use kanvibe_services::project::ScanResult;
use kanvibe_services::project::ports::{
    AiSessionAttachmentRequest, AiSessionDetailRequest, AiSessionListRequest,
};
use tempfile::TempDir;

use crate::error::MobileResult;
use crate::ports::{
    AiHistoryReadError, BackgroundSyncControl, DiffFiles, PeerTaskRegistration, ProjectCatalog,
    RemoteImageUpload, SessionDependencyCheck, TaskBoard, TaskPaneAccess, TerminalTabListing,
    TerminalTabs,
};

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 테스트마다 따로 여는 임시 SQLite. 디렉터리가 살아 있는 동안만 쓴다
pub struct TestDatabase {
    pub database: Arc<Database>,
    _directory: TempDir,
}

pub fn open_test_database() -> TestDatabase {
    let directory = tempfile::tempdir().expect("임시 디렉터리");
    let location = DatabaseLocation::in_app_data_directory(directory.path(), None);
    TestDatabase {
        database: Arc::new(Database::open(&location).expect("임시 DB")),
        _directory: directory,
    }
}

pub fn notification_store(database: &Arc<Database>) -> Arc<NotificationStore> {
    Arc::new(NotificationStore::new(Arc::new(
        DatabaseNotificationListStorage::new(Arc::clone(database)),
    )))
}

pub fn task(task_id: &str) -> KanbanTask {
    KanbanTask {
        id: task_id.to_string(),
        title: task_id.to_string(),
        description: None,
        status: TaskStatus::Todo,
        branch_name: None,
        worktree_path: None,
        session_type: None,
        session_name: None,
        ssh_host: None,
        agent_type: None,
        project_id: None,
        base_branch: None,
        pr_url: None,
        priority: None,
        created_at: "2026-11-01T00:00:00.000Z".to_string(),
        updated_at: "2026-11-01T00:00:00.000Z".to_string(),
    }
}

pub fn project(project_id: &str) -> Project {
    Project {
        id: project_id.to_string(),
        name: project_id.to_string(),
        repo_path: format!("/repo/{project_id}"),
        default_branch: "main".to_string(),
        ssh_host: None,
        is_worktree: false,
        color: None,
        icon_data_url: None,
        created_at: "2026-11-01T00:00:00.000Z".to_string(),
    }
}

pub fn with_project(task: KanbanTask, project: Option<Project>) -> TaskWithProject {
    TaskWithProject { task, project }
}

pub fn dependency_status(
    is_available: bool,
    tool_name: Option<&'static str>,
) -> SessionDependencyStatus {
    SessionDependencyStatus {
        session_type: SessionType::Tmux,
        tool_name,
        ssh_host: None,
        is_remote: false,
        is_available,
        blocked_reason: None,
    }
}

/// 받은 호출 기록
#[derive(Default)]
pub struct CallLog {
    calls: Mutex<Vec<String>>,
}

impl CallLog {
    pub fn record(&self, call: String) {
        lock(&self.calls).push(call);
    }

    pub fn all(&self) -> Vec<String> {
        lock(&self.calls).clone()
    }

    pub fn count_starting_with(&self, prefix: &str) -> usize {
        lock(&self.calls)
            .iter()
            .filter(|call| call.starts_with(prefix))
            .count()
    }
}

pub fn empty_board() -> TasksByStatusWithMeta {
    TasksByStatusWithMeta {
        tasks: TasksByStatus::default(),
        done_total: 0,
        done_limit: 20,
    }
}

#[derive(Default)]
pub struct FakeTaskBoard {
    pub calls: CallLog,
    pub tasks: Mutex<HashMap<String, TaskWithProject>>,
    pub board: Mutex<Option<TasksByStatusWithMeta>>,
    pub searchable_tasks: Mutex<Vec<SearchableTask>>,
    pub created_task: Mutex<Option<KanbanTask>>,
    pub create_inputs: Mutex<Vec<CreateTaskInput>>,
    pub updated_task: Mutex<Option<KanbanTask>>,
    pub status_result: Mutex<Option<Option<KanbanTask>>>,
    pub branch_result: Mutex<Option<KanbanTask>>,
    pub task_id_by_branch: Mutex<Option<String>>,
    pub session_change_result: Mutex<Option<KanbanTask>>,
    pub change_targets: Mutex<Option<SessionTypeChangeTargets>>,
    pub deletion: Mutex<Option<TaskDeletion>>,
    pub deletion_error: Mutex<Option<TaskDeletionError>>,
    pub is_registration_removed: Mutex<bool>,
    pub deleted_task_ids: Mutex<Vec<String>>,
}

impl FakeTaskBoard {
    pub fn put_task(&self, task: TaskWithProject) {
        lock(&self.tasks).insert(task.task.id.clone(), task);
    }
}

impl TaskBoard for FakeTaskBoard {
    fn get_tasks_by_status(&self) -> ServiceResult<TasksByStatusWithMeta> {
        Ok(lock(&self.board).clone().unwrap_or_else(empty_board))
    }

    fn get_task_by_id(&self, task_id: &str) -> ServiceResult<Option<TaskWithProject>> {
        self.calls.record(format!("get_task_by_id {task_id}"));
        Ok(lock(&self.tasks).get(task_id).cloned())
    }

    fn get_searchable_tasks(&self) -> ServiceResult<Vec<SearchableTask>> {
        Ok(lock(&self.searchable_tasks).clone())
    }

    fn get_more_done_tasks(
        &self,
        offset: usize,
        limit: usize,
    ) -> ServiceResult<LoadMoreDoneResponse> {
        self.calls
            .record(format!("get_more_done_tasks {offset} {limit}"));
        Ok(LoadMoreDoneResponse {
            tasks: Vec::new(),
            done_total: 0,
        })
    }

    fn create_task(&self, input: &CreateTaskInput) -> ServiceResult<KanbanTask> {
        self.calls.record("create_task".to_string());
        lock(&self.create_inputs).push(input.clone());
        Ok(lock(&self.created_task)
            .clone()
            .unwrap_or_else(|| task("created-task")))
    }

    fn update_task(
        &self,
        task_id: &str,
        updates: &TaskUpdate,
    ) -> ServiceResult<Option<KanbanTask>> {
        self.calls
            .record(format!("update_task {task_id} {updates:?}"));
        Ok(lock(&self.updated_task).clone())
    }

    fn update_task_status(
        &self,
        task_id: &str,
        status: TaskStatus,
    ) -> ServiceResult<Option<KanbanTask>> {
        self.calls
            .record(format!("update_task_status {task_id} {}", status.as_str()));
        Ok(lock(&self.status_result)
            .clone()
            .unwrap_or_else(|| Some(task(task_id))))
    }

    fn update_project_color(&self, project_id: &str, color: &str) -> ServiceResult<()> {
        self.calls
            .record(format!("update_project_color {project_id} {color}"));
        Ok(())
    }

    fn get_task_id_by_project_and_branch(&self, _: &str, _: &str) -> ServiceResult<Option<String>> {
        Ok(lock(&self.task_id_by_branch).clone())
    }

    fn branch_from_task(
        &self,
        task_id: &str,
        project_id: &str,
        base_branch: &str,
        branch_name: &str,
        session_type: SessionType,
    ) -> ServiceResult<Option<KanbanTask>> {
        self.calls.record(format!(
            "branch_from_task {task_id} {project_id} {base_branch} {branch_name} {}",
            session_type.as_str()
        ));
        Ok(lock(&self.branch_result).clone())
    }

    fn fetch_and_save_pr_url(&self, task_id: &str) -> ServiceResult<Option<String>> {
        self.calls
            .record(format!("fetch_and_save_pr_url {task_id}"));
        Ok(None)
    }

    fn change_task_session_type(
        &self,
        task_id: &str,
        session_type: SessionType,
        should_close_existing_session: bool,
    ) -> ServiceResult<Option<KanbanTask>> {
        self.calls.record(format!(
            "change_task_session_type {task_id} {} {should_close_existing_session}",
            session_type.as_str()
        ));
        Ok(lock(&self.session_change_result).clone())
    }

    fn is_task_session_alive(&self, _: &str) -> ServiceResult<bool> {
        Ok(false)
    }

    fn get_session_type_change_targets(
        &self,
        _: SessionType,
    ) -> ServiceResult<SessionTypeChangeTargets> {
        Ok(lock(&self.change_targets)
            .clone()
            .unwrap_or(SessionTypeChangeTargets {
                task_count: 0,
                live_session_count: 0,
                ssh_hosts: Vec::new(),
            }))
    }

    fn change_all_task_session_types(
        &self,
        session_type: SessionType,
        should_close: bool,
    ) -> ServiceResult<SessionTypeChangeResult> {
        self.calls.record(format!(
            "change_all_task_session_types {} {should_close}",
            session_type.as_str()
        ));
        Ok(SessionTypeChangeResult {
            changed_count: 0,
            failed_count: 0,
        })
    }

    fn delete_task_across_targets(
        &self,
        task_id: &str,
        options: &TaskDeletionOptions,
    ) -> Result<TaskDeletion, TaskDeletionError> {
        self.calls.record(format!(
            "delete_task_across_targets {task_id} reject_dirty={} propagate={}",
            options.should_reject_dirty_worktree, options.should_propagate_to_peers
        ));
        if let Some(error) = lock(&self.deletion_error).clone() {
            return Err(error);
        }
        Ok(lock(&self.deletion).clone().unwrap_or(TaskDeletion {
            is_deleted: false,
            peer_results: Vec::new(),
        }))
    }

    fn remove_task_registration(&self, task_id: &str) -> ServiceResult<bool> {
        self.calls
            .record(format!("remove_task_registration {task_id}"));
        Ok(*lock(&self.is_registration_removed))
    }

    fn delete_tasks(&self, task_ids: &[String]) -> Result<Vec<String>, TaskDeletionError> {
        self.calls.record(format!("delete_tasks {task_ids:?}"));
        Ok(lock(&self.deleted_task_ids).clone())
    }
}

#[derive(Default)]
pub struct FakeProjectCatalog {
    pub calls: CallLog,
    pub projects: Mutex<Vec<Project>>,
    pub branches: Mutex<Vec<String>>,
    pub hosts: Mutex<Vec<String>>,
    pub subdirectories: Mutex<Vec<String>>,
    pub scan_result: Mutex<ScanResult>,
    pub ai_sessions: Mutex<VecDeque<AggregatedAiSessionsResult>>,
    pub ai_details: Mutex<VecDeque<Result<Option<AggregatedAiSessionDetail>, AiHistoryReadError>>>,
    pub detail_requests: Mutex<Vec<(String, AiSessionDetailRequest)>>,
    pub attachment: Mutex<Option<ServiceResult<Option<AiSessionAttachmentContent>>>>,
    pub attachment_requests: Mutex<Vec<(String, AiSessionAttachmentRequest)>>,
    pub live_sessions: Mutex<Option<LiveAiSessionsResult>>,
    pub running_panes: Mutex<Vec<RunningAgentPane>>,
    pub call_graph: Mutex<Option<AgentCallGraph>>,
}

impl ProjectCatalog for FakeProjectCatalog {
    fn get_all_projects(&self) -> ServiceResult<Vec<Project>> {
        Ok(lock(&self.projects).clone())
    }

    fn get_project_by_id(&self, project_id: &str) -> ServiceResult<Option<Project>> {
        let projects = lock(&self.projects);
        Ok(projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned())
    }

    fn get_project_branches(&self, project_id: &str) -> ServiceResult<Vec<String>> {
        self.calls
            .record(format!("get_project_branches {project_id}"));
        Ok(lock(&self.branches).clone())
    }

    fn get_available_hosts(&self) -> Vec<String> {
        lock(&self.hosts).clone()
    }

    fn list_subdirectories(&self, parent_path: &str, ssh_host: Option<&str>) -> Vec<String> {
        self.calls
            .record(format!("list_subdirectories {parent_path} {ssh_host:?}"));
        lock(&self.subdirectories).clone()
    }

    fn scan_and_register_projects(
        &self,
        root_path: &str,
        ssh_host: Option<&str>,
    ) -> ServiceResult<ScanResult> {
        self.calls.record(format!(
            "scan_and_register_projects {root_path} {ssh_host:?}"
        ));
        Ok(lock(&self.scan_result).clone())
    }

    fn delete_project(&self, project_id: &str) -> ServiceResult<bool> {
        self.calls.record(format!("delete_project {project_id}"));
        Ok(true)
    }

    fn get_task_ai_sessions(
        &self,
        task_id: &str,
        request: &AiSessionListRequest,
    ) -> ServiceResult<AggregatedAiSessionsResult> {
        self.calls.record(format!(
            "get_task_ai_sessions {task_id} {:?}",
            request.limit
        ));
        Ok(lock(&self.ai_sessions).pop_front().unwrap_or_default())
    }

    fn get_task_ai_session_detail(
        &self,
        task_id: &str,
        request: &AiSessionDetailRequest,
    ) -> Result<Option<AggregatedAiSessionDetail>, AiHistoryReadError> {
        lock(&self.detail_requests).push((task_id.to_string(), request.clone()));
        lock(&self.ai_details).pop_front().unwrap_or(Ok(None))
    }

    fn get_task_ai_session_attachment(
        &self,
        task_id: &str,
        request: &AiSessionAttachmentRequest,
    ) -> ServiceResult<Option<AiSessionAttachmentContent>> {
        lock(&self.attachment_requests).push((task_id.to_string(), request.clone()));
        lock(&self.attachment).clone().unwrap_or(Ok(None))
    }

    fn get_task_live_ai_sessions(&self, task_id: &str) -> ServiceResult<LiveAiSessionsResult> {
        self.calls
            .record(format!("get_task_live_ai_sessions {task_id}"));
        Ok(lock(&self.live_sessions)
            .clone()
            .unwrap_or(LiveAiSessionsResult {
                task_id: task_id.to_string(),
                is_remote: false,
                sessions: Vec::new(),
            }))
    }

    fn get_running_agent_panes(&self) -> Vec<RunningAgentPane> {
        lock(&self.running_panes).clone()
    }

    fn get_task_agent_call_graph(
        &self,
        task_id: &str,
        provider: AiSessionProvider,
        session_id: &str,
    ) -> ServiceResult<Option<AgentCallGraph>> {
        self.calls.record(format!(
            "get_task_agent_call_graph {task_id} {} {session_id}",
            provider.id()
        ));
        Ok(lock(&self.call_graph).clone())
    }
}

#[derive(Default)]
pub struct FakeDiffFiles {
    pub calls: CallLog,
    pub diff_files: Mutex<Vec<DiffFile>>,
    pub symbolic_link: Mutex<Option<String>>,
    pub save_result: Mutex<Option<FileSaveResult>>,
    pub workspace_file: Mutex<Option<Result<TaskWorkspaceFile, TaskWorkspaceFileFailure>>>,
}

impl DiffFiles for FakeDiffFiles {
    fn get_git_diff_files(&self, _: &str) -> Vec<DiffFile> {
        lock(&self.diff_files).clone()
    }

    fn get_task_diff_stats(&self, task_ids: &[String]) -> BTreeMap<String, TaskDiffStats> {
        self.calls
            .record(format!("get_task_diff_stats {task_ids:?}"));
        BTreeMap::new()
    }

    fn get_original_file_content(&self, _: &str, file_path: &str) -> String {
        self.calls
            .record(format!("get_original_file_content {file_path}"));
        String::new()
    }

    fn get_file_content(&self, _: &str, file_path: &str) -> String {
        self.calls.record(format!("get_file_content {file_path}"));
        String::new()
    }

    fn save_file_content(&self, task_id: &str, file_path: &str, content: &str) -> FileSaveResult {
        self.calls
            .record(format!("save_file_content {task_id} {file_path} {content}"));
        lock(&self.save_result).clone().unwrap_or(FileSaveResult {
            success: true,
            error: None,
        })
    }

    fn read_task_workspace_file(
        &self,
        task_id: &str,
        file_path: &str,
    ) -> Result<TaskWorkspaceFile, TaskWorkspaceFileFailure> {
        self.calls
            .record(format!("read_task_workspace_file {task_id} {file_path}"));
        lock(&self.workspace_file)
            .clone()
            .unwrap_or(Err(TaskWorkspaceFileFailure::NotFound))
    }

    fn find_symbolic_link_path(
        &self,
        _: &[String],
        _: Option<&str>,
    ) -> Result<Option<String>, String> {
        Ok(lock(&self.symbolic_link).clone())
    }
}

#[derive(Default)]
pub struct FakeTerminalTabs {
    pub calls: CallLog,
    pub tabs: Mutex<Vec<TerminalTab>>,
}

pub fn terminal_tab(tab_id: &str, name: &str) -> TerminalTab {
    TerminalTab {
        id: tab_id.to_string(),
        native_index: 0,
        name: name.to_string(),
        is_active: false,
    }
}

impl TerminalTabs for FakeTerminalTabs {
    fn list_terminal_tabs(&self, _: &str) -> MobileResult<TerminalTabListing> {
        Ok(TerminalTabListing {
            is_ok: true,
            tabs: lock(&self.tabs).clone(),
        })
    }

    fn rename_terminal_tab(&self, task_id: &str, tab_id: &str, name: &str) -> MobileResult<bool> {
        self.calls
            .record(format!("rename_terminal_tab {task_id} {tab_id} {name}"));
        Ok(true)
    }

    fn move_terminal_tab(&self, task_id: &str, tab_id: &str, index: i64) -> MobileResult<bool> {
        self.calls
            .record(format!("move_terminal_tab {task_id} {tab_id} {index}"));
        Ok(true)
    }
}

/// 호스트별 설치 여부. 정하지 않은 호스트는 설치되어 있다고 본다
#[derive(Default)]
pub struct FakeSessionDependencies {
    pub calls: CallLog,
    pub status: Mutex<Option<SessionDependencyStatus>>,
}

impl SessionDependencyCheck for FakeSessionDependencies {
    fn get_session_dependency_status(
        &self,
        session_type: SessionType,
        ssh_host: Option<&str>,
    ) -> ServiceResult<SessionDependencyStatus> {
        self.calls.record(format!(
            "get_session_dependency_status {} {ssh_host:?}",
            session_type.as_str()
        ));
        Ok(lock(&self.status)
            .clone()
            .unwrap_or_else(|| dependency_status(true, Some("tmux"))))
    }
}

#[derive(Default)]
pub struct FakeBackgroundSync;

impl BackgroundSyncControl for FakeBackgroundSync {
    fn run_background_task_sync_now(&self) -> ServiceResult<BackgroundTaskSyncRunResult> {
        Ok(BackgroundTaskSyncRunResult {
            review_needed: false,
            board_updated: false,
        })
    }

    fn get_background_task_sync_status(&self) -> BackgroundTaskSyncStatus {
        serde_json::from_value(serde_json::json!({
            "phase": "idle", "isRunning": false, "completedStages": 0,
            "reviewNeeded": false, "boardUpdated": false, "error": null
        }))
        .expect("동기화 상태")
    }

    fn start_background_task_sync_now(&self) -> BackgroundTaskSyncStatus {
        self.get_background_task_sync_status()
    }
}

#[derive(Default)]
pub struct FakePeers {
    pub calls: CallLog,
    pub results: Mutex<Vec<PeerTargetResult>>,
}

impl PeerTaskRegistration for FakePeers {
    fn propagate_task_registration_to_peers(
        &self,
        project_repo_path: &str,
        branch_name: &str,
        ssh_host: Option<&str>,
    ) -> Vec<PeerTargetResult> {
        self.calls.record(format!(
            "propagate_task_registration_to_peers {project_repo_path} {branch_name} {ssh_host:?}"
        ));
        lock(&self.results).clone()
    }
}

#[derive(Default)]
pub struct FakePanes {
    pub calls: CallLog,
    pub pane_ids: Mutex<Vec<String>>,
}

impl TaskPaneAccess for FakePanes {
    fn has_task_pane(&self, _: &str, pane_id: &str) -> MobileResult<bool> {
        Ok(lock(&self.pane_ids).iter().any(|id| id == pane_id))
    }

    fn write_to_pane(&self, task_id: &str, pane_id: &str, input: &str) -> MobileResult<()> {
        self.calls
            .record(format!("write_to_pane {task_id} {pane_id} {input}"));
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeImages {
    pub calls: CallLog,
    pub remote_path: Mutex<Option<String>>,
}

impl RemoteImageUpload for FakeImages {
    fn paste_image_to_remote_terminal(&self, task_id: &str, _: &str) -> Result<String, String> {
        self.calls
            .record(format!("paste_image_to_remote_terminal {task_id}"));
        lock(&self.remote_path)
            .clone()
            .ok_or_else(|| "전송 실패".to_string())
    }
}

/// 조건이 참이 될 때까지 짧게 기다린다. 5초 안에 되지 않으면 테스트를 실패시킨다
pub fn wait_until(description: &str, condition: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "기다린 조건이 오지 않았다: {description}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// 가짜 중계 서버가 받은 요청 하나
#[derive(Debug, Clone)]
pub struct RelayRequest {
    pub headers: Vec<(String, String)>,
    pub body: serde_json::Value,
}

impl RelayRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

pub type RelayResponder = Arc<dyn Fn(&RelayRequest) -> u16 + Send + Sync>;

/// 127.0.0.1의 빈 포트에서 듣는 가짜 푸시 중계 서버. 응답 상태는 `responder`가 정하고, 그 안에서 기다리면 멈춘 중계가 된다
pub struct FakeRelay {
    pub url: String,
    pub requests: Arc<Mutex<Vec<RelayRequest>>>,
}

impl FakeRelay {
    pub fn received(&self) -> Vec<RelayRequest> {
        lock(&self.requests).clone()
    }
}

pub fn start_fake_relay(responder: RelayResponder) -> FakeRelay {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("가짜 중계 포트");
    let url = format!("http://{}/notifications", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&requests);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let recorded = Arc::clone(&recorded);
            let responder = Arc::clone(&responder);
            std::thread::spawn(move || serve_relay_request(stream, &recorded, &responder));
        }
    });
    FakeRelay { url, requests }
}

fn serve_relay_request(
    stream: std::net::TcpStream,
    recorded: &Mutex<Vec<RelayRequest>>,
    responder: &RelayResponder,
) {
    use std::io::{BufRead, BufReader, Read, Write};
    let mut reader = BufReader::new(stream.try_clone().expect("소켓 복제"));
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap_or_default();
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.trim_end().split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    let content_length: usize = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).unwrap_or_default();
    let request = RelayRequest {
        headers,
        body: serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
    };
    lock(recorded).push(request.clone());
    let status = responder(&request);
    let mut stream = stream;
    let _ = write!(
        stream,
        "HTTP/1.1 {status} Relay\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    );
}
