//! 주기적인 3단계 sync: 등록 프로젝트 worktree 발견 → PR 상태 → 활성 task `git pull`(TS `backgroundTaskSyncService.ts`).
//! 수동 sync와 예약 sync는 진행 중인 사이클 하나를 함께 기다리고, 고비용 단계는 직렬로 돈다.
//! 설정 변경(켜기·끄기·주기)은 TS가 appSettings 콜백으로 받던 것을 이 서비스의 setter가 저장과 함께 바로 반영한다.

mod scheduler;
mod stages;
#[cfg(test)]
mod tests;
pub mod timer;

pub use self::stages::ServiceBackgroundSyncStages;

use std::collections::HashSet;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use kanvibe_db::Database;
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_domain::background_sync::{BackgroundTaskSyncPhase, BackgroundTaskSyncStatus};
use kanvibe_domain::notifications::{
    BackgroundSyncFailurePayload, BackgroundSyncOperation, BackgroundSyncPulledTaskPayload,
    BackgroundSyncRegisteredWorktreePayload, BackgroundSyncReviewPayload, PulledTaskStatus,
    TaskPrMergedDetectedPayload,
};
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_process::exec::run_with_background_ssh_commands;
use kanvibe_process::task_state_service::status_sync::TaskStatusSyncResult;
use serde::Serialize;

use self::scheduler::SchedulerState;
use self::timer::SyncTimer;
use crate::board_notifier::BoardNotifier;
use crate::error::{ServiceError, ServiceResult};

/// 등록 프로젝트 worktree sync 결과 중 이 서비스가 쓰는 부분
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeSyncOutcome {
    pub registered_worktrees: Vec<BackgroundSyncRegisteredWorktreePayload>,
    pub errors: Vec<String>,
    pub has_changed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PullRequestSyncOutcome {
    pub updated_task_ids: Vec<String>,
    pub merged_pull_requests: Vec<TaskPrMergedDetectedPayload>,
    pub failures: Vec<BackgroundSyncFailurePayload>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskPullSyncOutcome {
    pub pulled_tasks: Vec<BackgroundSyncPulledTaskPayload>,
}

/// sync 사이클의 단계들. 실패는 사용자에게 보일 이유 문구로 돌려준다
pub trait BackgroundSyncStages: Send + Sync {
    /// TS `kanvibeTaskStateService.syncTaskStatusesFromFiles`
    fn sync_task_statuses_from_files(&self) -> Result<TaskStatusSyncResult, String>;
    /// TS `projectService.syncRegisteredProjectWorktrees()`
    fn sync_registered_project_worktrees(&self) -> Result<WorktreeSyncOutcome, String>;
    /// TS `kanbanService.syncActiveTaskPullRequests(emittedMergeEventKeys)`. 이미 알린 merge는 다시 알리지 않도록 키를 이어서 쓴다
    fn sync_active_task_pull_requests(
        &self,
        emitted_merge_event_keys: &mut HashSet<String>,
    ) -> Result<PullRequestSyncOutcome, String>;
    /// TS `kanbanService.syncActiveTaskPulls`
    fn sync_active_task_pulls(&self) -> Result<TaskPullSyncOutcome, String>;
}

/// 켜짐 여부와 주기 저장소. 앱은 `app_settings`를 쓴다
pub trait BackgroundSyncSettings: Send + Sync {
    fn is_enabled(&self) -> Result<bool, String>;
    fn interval_ms(&self) -> Result<i64, String>;
    fn save_enabled(&self, is_enabled: bool) -> Result<(), String>;
    fn save_interval_ms(&self, interval_ms: i64) -> Result<(), String>;
}

pub struct DatabaseBackgroundSyncSettings {
    pub database: Arc<Database>,
}

impl DatabaseBackgroundSyncSettings {
    fn settings(&self) -> AppSettingsService<'_> {
        AppSettingsService::new(&self.database, ShortcutPlatform::current())
    }
}

impl BackgroundSyncSettings for DatabaseBackgroundSyncSettings {
    fn is_enabled(&self) -> Result<bool, String> {
        self.settings()
            .get_background_sync_enabled()
            .map_err(|error| error.to_string())
    }

    fn interval_ms(&self) -> Result<i64, String> {
        self.settings()
            .get_background_sync_interval_ms()
            .map_err(|error| error.to_string())
    }

    fn save_enabled(&self, is_enabled: bool) -> Result<(), String> {
        self.settings()
            .set_background_sync_enabled(is_enabled)
            .map_err(|error| error.to_string())
    }

    fn save_interval_ms(&self, interval_ms: i64) -> Result<(), String> {
        self.settings()
            .set_background_sync_interval_ms(interval_ms)
            .map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundTaskSyncRunResult {
    pub review_needed: bool,
    pub board_updated: bool,
}

type CycleOutcome = Result<BackgroundTaskSyncRunResult, String>;

/// 진행 중인 사이클 하나. 먼저 시작한 호출이 돌리고 나머지는 결과를 기다린다
#[derive(Default)]
struct InFlightCycle {
    outcome: Mutex<Option<CycleOutcome>>,
    is_done: Condvar,
}

impl InFlightCycle {
    fn finish(&self, outcome: CycleOutcome) {
        *self.outcome.lock().unwrap() = Some(outcome);
        self.is_done.notify_all();
    }

    fn wait(&self) -> CycleOutcome {
        let guard = self.outcome.lock().unwrap();
        let guard = self
            .is_done
            .wait_while(guard, |outcome| outcome.is_none())
            .unwrap();
        guard.clone().expect("기다림이 끝나면 결과가 있다")
    }
}

struct SyncShared {
    stages: Arc<dyn BackgroundSyncStages>,
    settings: Arc<dyn BackgroundSyncSettings>,
    board: Arc<BoardNotifier>,
    timer: Arc<dyn SyncTimer>,
    status: Mutex<BackgroundTaskSyncStatus>,
    active_cycle: Mutex<Option<Arc<InFlightCycle>>>,
    emitted_merge_event_keys: Mutex<HashSet<String>>,
    scheduler: Mutex<Option<SchedulerState>>,
}

#[derive(Clone)]
pub struct BackgroundTaskSync {
    shared: Arc<SyncShared>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn idle_status() -> BackgroundTaskSyncStatus {
    BackgroundTaskSyncStatus {
        phase: BackgroundTaskSyncPhase::Idle,
        is_running: false,
        completed_stages: 0,
        review_needed: false,
        board_updated: false,
        error: None,
    }
}

fn failure(
    operation: BackgroundSyncOperation,
    target: &str,
    reason: String,
) -> BackgroundSyncFailurePayload {
    BackgroundSyncFailurePayload {
        operation,
        target: target.to_string(),
        reason,
        task_id: None,
        branch_name: None,
        ssh_host: None,
    }
}

/// 단계 실패는 기록하고 빈 결과로 다음 단계를 이어 간다
fn run_stage<T: Default>(stage_name: &str, result: Result<T, String>) -> (T, Option<String>) {
    match result {
        Ok(value) => (value, None),
        Err(reason) => {
            eprintln!("[background-task-sync] {stage_name} failed: {reason}");
            (T::default(), Some(reason))
        }
    }
}

const TASK_STATUS_TARGET: &str = "task 상태 sync";
const WORKTREE_TARGET: &str = "등록 프로젝트 worktree sync";

/// 한 사이클의 단계별 결과와 단계 자체의 실패 이유
struct StageResults {
    status_sync: TaskStatusSyncResult,
    status_failure: Option<String>,
    worktree_sync: WorktreeSyncOutcome,
    worktree_failure: Option<String>,
    pull_request_sync: PullRequestSyncOutcome,
    pull_request_failure: Option<String>,
    pull_sync: TaskPullSyncOutcome,
    pull_failure: Option<String>,
}

impl StageResults {
    /// 단계 실패와 단계가 돌려준 개별 실패를 TS와 같은 순서로 모은다
    fn failures(&self) -> Vec<BackgroundSyncFailurePayload> {
        use BackgroundSyncOperation::{PullRequestSync, TaskPullSync, WorktreeSync};
        let stage_failure = |reason: &Option<String>, operation, target: &str| {
            reason
                .clone()
                .map(|reason| failure(operation, target, reason))
        };
        let listed = |reasons: &[String], target: &str| {
            reasons
                .iter()
                .map(|reason| failure(WorktreeSync, target, reason.clone()))
                .collect::<Vec<_>>()
        };
        let mut failures = Vec::new();
        failures.extend(stage_failure(
            &self.status_failure,
            WorktreeSync,
            TASK_STATUS_TARGET,
        ));
        failures.extend(listed(&self.status_sync.errors, TASK_STATUS_TARGET));
        failures.extend(stage_failure(
            &self.worktree_failure,
            WorktreeSync,
            WORKTREE_TARGET,
        ));
        failures.extend(listed(&self.worktree_sync.errors, WORKTREE_TARGET));
        failures.extend(stage_failure(
            &self.pull_request_failure,
            PullRequestSync,
            "PR 상태 sync",
        ));
        failures.extend(self.pull_request_sync.failures.iter().cloned());
        failures.extend(stage_failure(
            &self.pull_failure,
            TaskPullSync,
            "active task pull sync",
        ));
        failures
    }

    fn has_review_targets(&self) -> bool {
        !self.worktree_sync.registered_worktrees.is_empty()
            || !self.pull_request_sync.merged_pull_requests.is_empty()
            || !self.pull_sync.pulled_tasks.is_empty()
    }

    fn has_board_changes(&self) -> bool {
        let has_updated_pulls = self
            .pull_sync
            .pulled_tasks
            .iter()
            .any(|task| task.status == PulledTaskStatus::Updated);
        self.status_sync.has_changed
            || self.worktree_sync.has_changed
            || !self.pull_request_sync.updated_task_ids.is_empty()
            || has_updated_pulls
    }
}

impl BackgroundTaskSync {
    pub fn new(
        stages: Arc<dyn BackgroundSyncStages>,
        settings: Arc<dyn BackgroundSyncSettings>,
        board: Arc<BoardNotifier>,
        timer: Arc<dyn SyncTimer>,
    ) -> Self {
        Self {
            shared: Arc::new(SyncShared {
                stages,
                settings,
                board,
                timer,
                status: Mutex::new(idle_status()),
                active_cycle: Mutex::new(None),
                emitted_merge_event_keys: Mutex::new(HashSet::new()),
                scheduler: Mutex::new(None),
            }),
        }
    }

    pub fn get_background_task_sync_status(&self) -> BackgroundTaskSyncStatus {
        lock(&self.shared.status).clone()
    }

    fn update_status(&self, update: impl FnOnce(&mut BackgroundTaskSyncStatus)) {
        let status = {
            let mut status = lock(&self.shared.status);
            update(&mut status);
            status.clone()
        };
        self.shared.board.broadcast_background_sync_status(status);
    }

    /// 고비용 단계를 차례로 돌린다. 한 단계가 실패해도 빈 결과로 다음 단계를 이어 간다
    fn run_stages(&self) -> StageResults {
        let stages = &self.shared.stages;
        let (status_sync, status_failure) =
            run_stage("task status sync", stages.sync_task_statuses_from_files());
        let (worktree_sync, worktree_failure) =
            run_stage("worktree sync", stages.sync_registered_project_worktrees());
        self.update_status(|status| {
            status.phase = BackgroundTaskSyncPhase::PullRequests;
            status.completed_stages = 1;
        });
        let pull_request_result = {
            let mut emitted_keys = lock(&self.shared.emitted_merge_event_keys);
            stages.sync_active_task_pull_requests(&mut emitted_keys)
        };
        let (pull_request_sync, pull_request_failure) =
            run_stage("pull request sync", pull_request_result);
        self.update_status(|status| {
            status.phase = BackgroundTaskSyncPhase::Pulls;
            status.completed_stages = 2;
        });
        let (pull_sync, pull_failure) =
            run_stage("task pull sync", stages.sync_active_task_pulls());
        StageResults {
            status_sync,
            status_failure,
            worktree_sync,
            worktree_failure,
            pull_request_sync,
            pull_request_failure,
            pull_sync,
            pull_failure,
        }
    }

    /// 사이클 하나를 돌리고 검토할 내용과 보드 갱신 여부를 알린다
    fn execute_cycle(&self) -> BackgroundTaskSyncRunResult {
        let results = self.run_stages();
        let failures = results.failures();
        let review_needed = results.has_review_targets() || !failures.is_empty();
        let board_updated = results.has_board_changes();
        if review_needed {
            self.shared.board.broadcast_background_sync_review_needed(
                BackgroundSyncReviewPayload {
                    registered_worktrees: results.worktree_sync.registered_worktrees,
                    merged_pull_requests: results.pull_request_sync.merged_pull_requests,
                    pulled_tasks: Some(results.pull_sync.pulled_tasks),
                    failures: (!failures.is_empty()).then_some(failures),
                },
            );
        }
        if board_updated {
            self.shared.board.broadcast_board_update();
        }
        BackgroundTaskSyncRunResult {
            review_needed,
            board_updated,
        }
    }

    /// 진행 중인 사이클이 있으면 합류하고, 없으면 새로 시작해 상태를 실행 중으로 바꾼다
    fn join_or_begin_cycle(&self) -> (Arc<InFlightCycle>, bool) {
        let mut active = lock(&self.shared.active_cycle);
        if let Some(cycle) = active.as_ref() {
            return (Arc::clone(cycle), false);
        }
        let cycle = Arc::new(InFlightCycle::default());
        *active = Some(Arc::clone(&cycle));
        drop(active);
        self.update_status(|status| {
            *status = BackgroundTaskSyncStatus {
                phase: BackgroundTaskSyncPhase::Worktrees,
                is_running: true,
                ..idle_status()
            };
        });
        (cycle, true)
    }

    /// 시작한 쪽이 사이클을 끝까지 돌리고 결과를 기다리던 쪽에 넘긴다. 예기치 못한 실패도 실패 상태로 남긴다
    fn lead_cycle(&self, cycle: &InFlightCycle) -> CycleOutcome {
        let execution = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_with_background_ssh_commands(|| self.execute_cycle())
        }));
        let outcome = execution.map_err(|panic| {
            panic
                .downcast_ref::<&str>()
                .map(|message| message.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "background sync failed".to_string())
        });
        self.update_status(|status| match &outcome {
            Ok(result) => {
                status.review_needed = result.review_needed;
                status.board_updated = result.board_updated;
                status.phase = BackgroundTaskSyncPhase::Completed;
                status.is_running = false;
                status.completed_stages = 3;
            }
            Err(message) => {
                status.phase = BackgroundTaskSyncPhase::Failed;
                status.is_running = false;
                status.error = Some(message.clone());
            }
        });
        lock(&self.shared.active_cycle).take();
        cycle.finish(outcome.clone());
        outcome
    }

    fn run_shared_cycle(&self) -> CycleOutcome {
        let (cycle, is_leader) = self.join_or_begin_cycle();
        if is_leader {
            self.lead_cycle(&cycle)
        } else {
            cycle.wait()
        }
    }

    /// 사이클을 돌리고(진행 중이면 합류해) 끝날 때까지 기다린다. 설정에서 꺼져 있어도 돈다
    pub fn run_background_task_sync_now(&self) -> ServiceResult<BackgroundTaskSyncRunResult> {
        self.run_shared_cycle().map_err(ServiceError::new)
    }

    /// 사이클을 백그라운드에서 시작하고 지금 상태를 바로 돌려준다
    pub fn start_background_task_sync_now(&self) -> BackgroundTaskSyncStatus {
        let (cycle, is_leader) = self.join_or_begin_cycle();
        if is_leader {
            let service = self.clone();
            std::thread::spawn(move || {
                if let Err(error) = service.lead_cycle(&cycle) {
                    eprintln!("[background-task-sync] Manual sync failed: {error}");
                }
            });
        }
        self.get_background_task_sync_status()
    }
}
