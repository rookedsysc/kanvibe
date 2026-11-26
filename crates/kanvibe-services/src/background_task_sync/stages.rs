//! 실제 앱의 sync 단계. 상태 파일 반영은 kanvibe-process, worktree 발견은 프로젝트 서비스, PR·pull은 보드 서비스가 맡는다.

use std::collections::HashSet;
use std::sync::Arc;

use kanvibe_db::Database;
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::kanvibe_state_store::HostKanvibeStateStore;
use kanvibe_process::task_state_service::status_sync::{TaskStatusSync, TaskStatusSyncResult};

use super::{
    BackgroundSyncStages, PullRequestSyncOutcome, TaskPullSyncOutcome, WorktreeSyncOutcome,
};
use crate::board_notifier::BoardNotifier;
use crate::kanban::KanbanService;
use crate::project::ProjectService;

pub struct ServiceBackgroundSyncStages {
    pub database: Arc<Database>,
    pub files: Arc<HostFileAccess>,
    pub board: Arc<BoardNotifier>,
    pub kanban: Arc<KanbanService>,
    pub project: Arc<ProjectService>,
}

impl BackgroundSyncStages for ServiceBackgroundSyncStages {
    fn sync_task_statuses_from_files(&self) -> Result<TaskStatusSyncResult, String> {
        let state_store = HostKanvibeStateStore::new(&self.files);
        TaskStatusSync::new(&self.database, &state_store, self.board.as_ref())
            .sync_task_statuses_from_files()
    }

    fn sync_registered_project_worktrees(&self) -> Result<WorktreeSyncOutcome, String> {
        let result = self
            .project
            .sync_registered_project_worktrees(None)
            .map_err(|error| error.message)?;
        Ok(WorktreeSyncOutcome {
            registered_worktrees: result.registered_worktrees,
            errors: result.errors,
            has_changed: result.has_changed,
        })
    }

    fn sync_active_task_pull_requests(
        &self,
        emitted_merge_event_keys: &mut HashSet<String>,
    ) -> Result<PullRequestSyncOutcome, String> {
        let result = self
            .kanban
            .sync_active_task_pull_requests(emitted_merge_event_keys)
            .map_err(|error| error.message)?;
        Ok(PullRequestSyncOutcome {
            updated_task_ids: result.updated_task_ids,
            merged_pull_requests: result.merged_pull_requests,
            failures: result.failures.unwrap_or_default(),
        })
    }

    fn sync_active_task_pulls(&self) -> Result<TaskPullSyncOutcome, String> {
        let result = self
            .kanban
            .sync_active_task_pulls()
            .map_err(|error| error.message)?;
        Ok(TaskPullSyncOutcome {
            pulled_tasks: result.pulled_tasks,
        })
    }
}
