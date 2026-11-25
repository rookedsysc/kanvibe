//! task CLI 요청이 쓰는 앱 서비스(TS `taskCliService.ts`가 import하던 토큰·저장소·보드·프로젝트·터미널·전파 서비스).

use std::sync::Arc;

use kanvibe_db::Database;
use kanvibe_db::repositories::{projects, tasks};
use kanvibe_domain::project::Project;
use kanvibe_domain::task::KanbanTask;
use kanvibe_process::task_cli::service::{
    CliTaskCreation, CliTaskSession, HerdrTaskPane, ProjectTaskSelector, TaskCliBackend,
    TaskDeletion, TaskDeletionError as CliTaskDeletionError, TaskDeletionOptions,
};
use kanvibe_process::task_target_propagation::{PeerTargetResult, TaskTargetPropagation};
use kanvibe_services::board_notifier::BoardNotifier;
use kanvibe_services::kanban::{CreateTaskInput, KanbanService, TaskDeletionError};
use kanvibe_services::project::ProjectService;
use kanvibe_terminal::terminal_bridge::TerminalBridge;
use kanvibe_terminal::terminal_registry::PreparedTerminalSession;

use crate::hooks::TaskCliTokens;

pub struct DesktopTaskCliBackend {
    pub(crate) database: Arc<Database>,
    pub(crate) tokens: Arc<TaskCliTokens>,
    pub(crate) kanban: Arc<KanbanService>,
    pub(crate) project: Arc<ProjectService>,
    pub(crate) board: Arc<BoardNotifier>,
    pub(crate) terminal_bridge: Arc<TerminalBridge>,
    pub(crate) task_target_propagation: Arc<TaskTargetPropagation<'static>>,
}

fn message(error: impl ToString) -> String {
    error.to_string()
}

/// 서비스의 삭제 실패를 task CLI가 고르는 응답 종류로 옮긴다. 더러운 worktree만 409가 된다
fn into_cli_deletion_error(error: TaskDeletionError) -> CliTaskDeletionError {
    match error {
        TaskDeletionError::DirtyWorktree { .. } => {
            CliTaskDeletionError::DirtyWorktree(error.to_string())
        }
        TaskDeletionError::InvalidWorktree | TaskDeletionError::Failed(_) => {
            CliTaskDeletionError::Failed(error.to_string())
        }
    }
}

fn into_cli_task_session(prepared: PreparedTerminalSession) -> CliTaskSession {
    CliTaskSession {
        is_prepared: prepared.is_prepared,
        skipped_reason: prepared.skipped_reason,
        error: None,
        herdr_workspace_id: prepared.herdr_workspace_id,
        herdr_panes: prepared.herdr_panes.map(|panes| {
            panes
                .into_iter()
                .map(|pane| HerdrTaskPane {
                    pane_id: pane.pane_id,
                    tab_id: pane.tab_id,
                })
                .collect()
        }),
    }
}

impl DesktopTaskCliBackend {
    fn find_task_in_project(
        &self,
        project_id: &str,
        task_id: &str,
    ) -> Result<Option<KanbanTask>, String> {
        let task = self.find_task(task_id)?;
        Ok(task.filter(|task| task.project_id.as_deref() == Some(project_id)))
    }
}

impl TaskCliBackend for DesktopTaskCliBackend {
    fn is_valid_task_cli_token(&self, task_id: &str, token: &str) -> Result<bool, String> {
        self.tokens
            .is_valid_task_cli_token(task_id, token)
            .map_err(message)
    }

    fn find_task(&self, task_id: &str) -> Result<Option<KanbanTask>, String> {
        tasks::find_task(&self.database.connection(), task_id).map_err(message)
    }

    fn find_project_task(
        &self,
        project_id: &str,
        selector: &ProjectTaskSelector,
    ) -> Result<Option<KanbanTask>, String> {
        let task_id = match selector {
            ProjectTaskSelector::Id(task_id) => Some(task_id.clone()),
            ProjectTaskSelector::Branch(branch_name) => {
                self.task_id_by_project_and_branch(project_id, branch_name)?
            }
        };
        match task_id {
            Some(task_id) => self.find_task_in_project(project_id, &task_id),
            None => Ok(None),
        }
    }

    fn find_project(&self, project_id: &str) -> Result<Option<Project>, String> {
        projects::find_project(&self.database.connection(), project_id).map_err(message)
    }

    /// 만든 순서(`createdAt ASC`). 같은 시각은 저장소가 준 순서를 지킨다
    fn list_project_tasks(&self, project_id: &str) -> Result<Vec<KanbanTask>, String> {
        let all_tasks = tasks::list_tasks(&self.database.connection()).map_err(message)?;
        let mut project_tasks: Vec<KanbanTask> = all_tasks
            .into_iter()
            .filter(|task| task.project_id.as_deref() == Some(project_id))
            .collect();
        project_tasks.sort_by(|left, right| left.created_at.cmp(&right.created_at));
        Ok(project_tasks)
    }

    fn task_id_by_project_and_branch(
        &self,
        project_id: &str,
        branch_name: &str,
    ) -> Result<Option<String>, String> {
        self.kanban
            .get_task_id_by_project_and_branch(project_id, branch_name)
            .map_err(|error| error.message)
    }

    fn create_task(&self, creation: &CliTaskCreation) -> Result<KanbanTask, String> {
        let input = CreateTaskInput {
            title: creation.title.clone(),
            description: creation.description.clone(),
            branch_name: Some(creation.branch_name.clone()),
            base_branch: Some(creation.base_branch.clone()),
            session_type: Some(creation.session_type),
            ssh_host: None,
            project_id: Some(creation.project_id.clone()),
            priority: creation.priority,
        };
        self.kanban
            .create_task(&input)
            .map_err(|error| error.message)
    }

    fn delete_task_across_targets(
        &self,
        task_id: &str,
        options: &TaskDeletionOptions,
    ) -> Result<TaskDeletion, CliTaskDeletionError> {
        self.kanban
            .delete_task_across_targets(task_id, options)
            .map_err(into_cli_deletion_error)
    }

    fn sync_registered_project_worktrees(&self, project: &Project) -> Result<bool, String> {
        let result = self
            .project
            .sync_registered_project_worktrees(Some(std::slice::from_ref(project)))
            .map_err(|error| error.message)?;
        Ok(result.has_changed)
    }

    fn broadcast_board_update(&self) {
        self.board.broadcast_board_update();
    }

    fn prepare_task_terminal(&self, task: &KanbanTask) -> Result<CliTaskSession, String> {
        self.terminal_bridge
            .prepare_task_terminal(task)
            .map(into_cli_task_session)
            .map_err(|error| error.message)
    }

    fn propagate_task_registration_to_peers(
        &self,
        project_repo_path: &str,
        branch_name: &str,
        ssh_host: Option<&str>,
    ) -> Vec<PeerTargetResult> {
        self.task_target_propagation
            .propagate_task_registration_to_peers(project_repo_path, branch_name, ssh_host)
    }
}
