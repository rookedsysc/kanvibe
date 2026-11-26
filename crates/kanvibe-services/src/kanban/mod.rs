//! 보드 task 서비스(TS `src/desktop/main/services/kanbanService.ts`).
//! task 조회·생성·수정·삭제와 worktree·터미널 세션 연결, PR 조회, 활성 task pull을 맡는다.
//! DB는 직접 쓰고, worktree·hook·공유 상태 파일·다른 client 전파·터미널은 `ports`의 trait로 받는다.

mod board_queries;
mod deletion;
mod github_cli;
mod hook_preparation;
pub mod ports;
mod pull_requests;
mod records;
mod sessions;
mod task_editing;
mod task_pulls;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kanvibe_db::Database;
use kanvibe_domain::notifications::{
    BackgroundSyncFailurePayload, BackgroundSyncPulledTaskPayload, TaskPrMergedDetectedPayload,
};
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskPriority, TaskStatus};
use kanvibe_process::host_file_access::HostFileAccess;
use serde::Serialize;

pub use self::deletion::TaskDeletionError;
pub use kanvibe_process::task_cli::service::{TaskDeletion, TaskDeletionOptions};

use self::ports::{KanvibeStateFiles, TaskHookInstallation, TaskPeerPropagation, TaskWorktrees};
use self::pull_requests::PullRequestLookups;
use crate::board_notifier::BoardNotifier;
use crate::terminal_ports::TaskTerminalSessions;

/// Done 컬럼이 처음과 더보기마다 불러오는 개수
pub const DONE_PAGE_SIZE: usize = 20;

/// 시각을 고정해야 하는 테스트가 바꿔 끼우는 시계
pub type ServiceClock = dyn Fn() -> Instant + Send + Sync;

/// 기다림 시간. 앱은 기본값을 쓰고 테스트만 줄인다
#[derive(Debug, Clone, Copy)]
pub struct KanbanTimings {
    /// 터미널 연결 전에 hook 파일 설치를 기다리는 최대 시간. 넘으면 설치는 뒤에서 계속하고 세션을 만든다
    pub terminal_hook_install_pre_attach_wait: Duration,
}

impl Default for KanbanTimings {
    fn default() -> Self {
        Self {
            terminal_hook_install_pre_attach_wait: Duration::from_millis(1_500),
        }
    }
}

/// 보드 서비스가 쓰는 협력 객체
pub struct KanbanServiceParts {
    pub database: Arc<Database>,
    /// git·gh 명령 실행과 원격 HOME 해석
    pub files: Arc<HostFileAccess>,
    pub board: Arc<BoardNotifier>,
    pub terminal: Arc<dyn TaskTerminalSessions>,
    pub worktrees: Arc<dyn TaskWorktrees>,
    pub hooks: Arc<dyn TaskHookInstallation>,
    pub state_files: Arc<dyn KanvibeStateFiles>,
    pub peers: Arc<dyn TaskPeerPropagation>,
    pub clock: Arc<ServiceClock>,
    pub timings: KanbanTimings,
}

pub struct KanbanService {
    database: Arc<Database>,
    files: Arc<HostFileAccess>,
    board: Arc<BoardNotifier>,
    terminal: Arc<dyn TaskTerminalSessions>,
    worktrees: Arc<dyn TaskWorktrees>,
    hooks: Arc<dyn TaskHookInstallation>,
    state_files: Arc<dyn KanvibeStateFiles>,
    peers: Arc<dyn TaskPeerPropagation>,
    clock: Arc<ServiceClock>,
    timings: KanbanTimings,
    pr_lookups: PullRequestLookups,
    /// 같은 task의 pull 실패를 성공 전까지 한 번만 알리려고 기억한다
    notified_pull_failure_keys: Mutex<HashSet<String>>,
}

impl KanbanService {
    pub fn new(parts: KanbanServiceParts) -> Self {
        Self {
            database: parts.database,
            files: parts.files,
            board: parts.board,
            terminal: parts.terminal,
            worktrees: parts.worktrees,
            hooks: parts.hooks,
            state_files: parts.state_files,
            peers: parts.peers,
            clock: parts.clock,
            timings: parts.timings,
            pr_lookups: PullRequestLookups::default(),
            notified_pull_failure_keys: Mutex::new(HashSet::new()),
        }
    }

    fn now(&self) -> Instant {
        (self.clock)()
    }
}

/// 상태별 task 묶음. Done은 첫 페이지만 담는다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct TasksByStatus {
    pub todo: Vec<KanbanTask>,
    pub progress: Vec<KanbanTask>,
    pub pending: Vec<KanbanTask>,
    pub review: Vec<KanbanTask>,
    pub done: Vec<KanbanTask>,
}

impl TasksByStatus {
    fn column_mut(&mut self, status: TaskStatus) -> &mut Vec<KanbanTask> {
        match status {
            TaskStatus::Todo => &mut self.todo,
            TaskStatus::Progress => &mut self.progress,
            TaskStatus::Pending => &mut self.pending,
            TaskStatus::Review => &mut self.review,
            TaskStatus::Done => &mut self.done,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksByStatusWithMeta {
    pub tasks: TasksByStatus,
    pub done_total: usize,
    pub done_limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadMoreDoneResponse {
    pub tasks: Vec<KanbanTask>,
    pub done_total: usize,
}

/// task와 연결된 프로젝트(TS `relations: { project: true }`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskWithProject {
    #[serde(flatten)]
    pub task: KanbanTask,
    pub project: Option<Project>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchableTask {
    pub id: String,
    pub title: String,
    pub branch_name: Option<String>,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub ssh_host: Option<String>,
    pub status: TaskStatus,
    pub updated_at: String,
}

/// 새 task 입력(TS `CreateTaskInput`). 빈 문자열은 값이 없는 것으로 본다
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreateTaskInput {
    pub title: String,
    pub description: Option<String>,
    pub branch_name: Option<String>,
    pub base_branch: Option<String>,
    pub session_type: Option<SessionType>,
    pub ssh_host: Option<String>,
    pub project_id: Option<String>,
    pub priority: Option<TaskPriority>,
}

/// task 부분 수정. 바깥 `None`은 그 필드를 건드리지 않는다는 뜻이다
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskUpdate {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub priority: Option<Option<TaskPriority>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTypeChangeTargets {
    pub task_count: usize,
    pub live_session_count: usize,
    /// 새 세션 도구를 확인해야 하는 호스트. `None`은 로컬이다
    pub ssh_hosts: Vec<Option<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTypeChangeResult {
    pub changed_count: usize,
    pub failed_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveTaskPullRequestSyncResult {
    pub updated_task_ids: Vec<String>,
    pub merge_event_keys: Vec<String>,
    pub merged_pull_requests: Vec<TaskPrMergedDetectedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failures: Option<Vec<BackgroundSyncFailurePayload>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveTaskPullSyncResult {
    pub pulled_tasks: Vec<BackgroundSyncPulledTaskPayload>,
}
