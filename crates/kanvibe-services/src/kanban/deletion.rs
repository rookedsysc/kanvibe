//! task 삭제와 등록 제거. 삭제는 다른 client로 먼저 전파한 뒤 세션·worktree·브랜치를 정리하고 레코드를 지운다.
//! 전파가 worktree 삭제보다 앞서야 하는 이유는 상대 client의 taskId와 토큰이 그 worktree의 targets.json에만 있기 때문이다.

use std::collections::HashSet;

use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType};
use kanvibe_process::exec::ExecGitOptions;
use kanvibe_process::task_target_propagation::PeerTargetResult;
use kanvibe_process::worktree::{ResourceCleanupOptions, format_project_branch_session_name};

use super::github_cli::quote_for_shell;
use super::records::{delete_task_record, find_project_by_id, find_task_by_id};
use super::{KanbanService, TaskDeletion, TaskDeletionOptions};
use crate::board_notifier::TaskPeerDeletionFailedPayload;
use crate::error::{ServiceError, ServiceResult};

const INVALID_WORKTREE_MARKER: &str = "__KANVIBE_INVALID_WORKTREE__";

/// 삭제 실패. 메시지는 TS 오류 문자열과 같다
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TaskDeletionError {
    /// 커밋하지 않은 변경이 남은 worktree를 지우지 않으려고 멈췄다(TS `DirtyWorktreeError`)
    #[error("커밋하지 않은 변경이 있어 삭제하지 않았습니다: {worktree_path}")]
    DirtyWorktree { worktree_path: String },
    /// worktree의 Git 메타데이터가 없거나 깨져 등록만 제거할 수 있다(TS `InvalidWorktreeError`)
    #[error("Git 저장소를 찾을 수 없습니다. 파일을 보존하고 태스크 등록만 제거할 수 있습니다.")]
    InvalidWorktree,
    #[error(transparent)]
    Failed(#[from] ServiceError),
}

impl From<kanvibe_db::DatabaseError> for TaskDeletionError {
    fn from(error: kanvibe_db::DatabaseError) -> Self {
        Self::Failed(error.into())
    }
}

/// SSH 오류 요약이 Git의 stderr를 놓칠 수 있어 실행한 셸에서 실패 원인을 구분한다.
/// 이미 다른 client가 지운 worktree는 잃을 변경이 없으므로 깨끗한 것으로 본다
fn build_uncommitted_changes_command(worktree_path: &str) -> String {
    let quoted = quote_for_shell(worktree_path);
    format!(
        r#"if [ -d {quoted} ]; then
      __kanvibe_status_output=$(LC_ALL=C git -C {quoted} status --porcelain 2>&1)
      __kanvibe_status_code=$?
      if [ "$__kanvibe_status_code" -eq 0 ]; then
        printf '%s' "$__kanvibe_status_output"
      else
        case "$__kanvibe_status_output" in
          *"fatal: not a git repository"*|*"fatal: gitfile does not point to a valid repository"*) printf '%s' '{INVALID_WORKTREE_MARKER}' ;;
          *) printf '%s\n' "$__kanvibe_status_output" >&2; exit "$__kanvibe_status_code" ;;
        esac
      fi
    fi"#
    )
}

/// 정리할 세션. sessionName이 사라진 원격 tmux task는 프로젝트와 브랜치에서 이름을 다시 계산한다
fn resolve_cleanup_session(
    task: &KanbanTask,
    project: Option<&Project>,
    ssh_host: Option<&str>,
) -> Option<(SessionType, String)> {
    if let Some(session_name) = task.session_name.clone() {
        return task
            .session_type
            .map(|session_type| (session_type, session_name));
    }
    let is_tmux_or_unknown = matches!(task.session_type, None | Some(SessionType::Tmux));
    let branch_name = task.branch_name.as_deref()?;
    let project = project?;
    (is_tmux_or_unknown && ssh_host.is_some()).then(|| {
        let session_name = format_project_branch_session_name(&project.repo_path, branch_name);
        (SessionType::Tmux, session_name)
    })
}

/// 프로젝트 없이 브랜치·worktree만 남은 stale task. 정리 실패를 알려야 하면 오류다
fn stale_task_cleanup_result(should_throw_on_error: bool) -> ServiceResult<()> {
    if should_throw_on_error {
        return Err(ServiceError::new(
            "연결된 프로젝트를 찾을 수 없어 worktree/브랜치 정리를 완료할 수 없습니다.",
        ));
    }
    Ok(())
}

/// 정리할 프로젝트 경로를 몰라 worktree·브랜치 정리를 건너뛴다. 정리 실패를 알려야 하면 오류다
fn skip_cleanup_without_project_path(
    task: &KanbanTask,
    branch_name: &str,
    ssh_host: Option<&str>,
    should_throw_on_error: bool,
) -> ServiceResult<()> {
    eprintln!(
        "worktree/브랜치 정리 건너뜀: 정리할 프로젝트 정보가 부족합니다. {{ taskId: {:?}, branchName: {branch_name:?}, worktreePath: {:?}, sshHost: {ssh_host:?} }}",
        task.id, task.worktree_path
    );
    if should_throw_on_error {
        return Err(ServiceError::new(
            "정리할 프로젝트 정보가 부족해 worktree/브랜치 정리를 건너뛰었습니다.",
        ));
    }
    Ok(())
}

impl KanbanService {
    fn cleanup_task_session(
        &self,
        task: &KanbanTask,
        session: (SessionType, String),
        ssh_host: Option<&str>,
        should_throw_on_error: bool,
    ) -> ServiceResult<()> {
        // 세션을 끝내기 전에 앱이 붙잡고 있는 PTY부터 놓아야 멀티플렉서가 세션을 바로 닫는다
        self.terminal
            .detach_session(&task.id, "cleanup-task-resources");
        let (session_type, session_name) = session;
        let result = self.worktrees.remove_session_only(
            session_type,
            &session_name,
            ssh_host,
            should_throw_on_error,
        );
        match result {
            Err(error) if should_throw_on_error => Err(error),
            Err(error) => {
                eprintln!("세션 정리 실패: {error}");
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }

    /// 작업에 연결된 세션, worktree, 브랜치를 정리한다. task 레코드는 지우지 않는다
    fn cleanup_task_resources(
        &self,
        task: &KanbanTask,
        should_throw_on_error: bool,
    ) -> ServiceResult<()> {
        let project = match task.project_id.as_deref() {
            Some(project_id) => find_project_by_id(&self.database, project_id)?,
            None => None,
        };
        let ssh_host = task
            .ssh_host
            .clone()
            .filter(|host| !host.is_empty())
            .or_else(|| {
                project
                    .as_ref()
                    .and_then(|project| project.ssh_host.clone())
            })
            .filter(|host| !host.is_empty());
        if let Some(session) = resolve_cleanup_session(task, project.as_ref(), ssh_host.as_deref())
        {
            self.cleanup_task_session(task, session, ssh_host.as_deref(), should_throw_on_error)?;
        }
        self.cleanup_task_worktree(
            task,
            project.as_ref(),
            ssh_host.as_deref(),
            should_throw_on_error,
        )
    }

    /// 프로젝트 루트 브랜치를 뺀 worktree와 브랜치를 지운다.
    /// 프로젝트 없이 브랜치·worktree만 남은 task는 stale 상태라 정리를 건너뛴다
    fn cleanup_task_worktree(
        &self,
        task: &KanbanTask,
        project: Option<&Project>,
        ssh_host: Option<&str>,
        should_throw_on_error: bool,
    ) -> ServiceResult<()> {
        let Some(branch_name) = task.branch_name.as_deref() else {
            return Ok(());
        };
        if project.is_none() && task.worktree_path.is_some() {
            return stale_task_cleanup_result(should_throw_on_error);
        }
        if project.is_some_and(|project| task.worktree_path.as_deref() == Some(&project.repo_path))
        {
            return Ok(());
        }
        let Some(project_repo_path) = project
            .map(|project| project.repo_path.as_str())
            .filter(|path| !path.is_empty())
        else {
            return skip_cleanup_without_project_path(
                task,
                branch_name,
                ssh_host,
                should_throw_on_error,
            );
        };
        let options = ResourceCleanupOptions {
            should_throw_on_error,
            worktree_path: task.worktree_path.clone(),
        };
        let result = self.worktrees.remove_worktree_and_branch(
            project_repo_path,
            branch_name,
            ssh_host,
            &options,
        );
        match result {
            Err(error) if should_throw_on_error => Err(error),
            Err(error) => {
                eprintln!("worktree/브랜치 정리 실패: {error}");
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }

    /// 파일·브랜치·세션과 다른 데스크탑은 그대로 두고 이 데스크탑의 등록만 제거한다
    pub fn remove_task_registration(&self, task_id: &str) -> ServiceResult<bool> {
        if find_task_by_id(&self.database, task_id)?.is_none() {
            return Ok(false);
        }
        delete_task_record(&self.database, task_id)?;
        self.board.broadcast_task_session_changed(task_id);
        self.board.broadcast_board_update();
        Ok(true)
    }

    /// 작업을 지운다. worktree와 세션을 함께 정리하고 같은 task를 가진 다른 client에서도 지운다
    pub fn delete_task(&self, task_id: &str) -> Result<bool, TaskDeletionError> {
        let options = TaskDeletionOptions {
            should_reject_dirty_worktree: false,
            should_propagate_to_peers: true,
        };
        Ok(self
            .delete_task_across_targets(task_id, &options)?
            .is_deleted)
    }

    /// 작업을 지우면서 다른 client로 전파한다. 상대가 먼저 worktree·브랜치를 지워도 이 client의 정리는 없는 자원을 건너뛴다
    pub fn delete_task_across_targets(
        &self,
        task_id: &str,
        options: &TaskDeletionOptions,
    ) -> Result<TaskDeletion, TaskDeletionError> {
        let Some(task) = find_task_by_id(&self.database, task_id)? else {
            return Ok(TaskDeletion {
                is_deleted: false,
                peer_results: Vec::new(),
            });
        };
        if let (true, Some(worktree_path)) = (
            options.should_reject_dirty_worktree,
            task.worktree_path.as_deref(),
        ) {
            self.assert_worktree_has_no_uncommitted_changes(
                worktree_path,
                task.ssh_host.as_deref(),
            )?;
        }
        let peer_results = if options.should_propagate_to_peers {
            self.propagate_deletion_to_task_peers(&task)
        } else {
            Vec::new()
        };
        self.cleanup_task_resources(&task, true)?;
        delete_task_record(&self.database, task_id)?;
        self.board.broadcast_task_session_changed(task_id);
        self.board.broadcast_board_update();
        Ok(TaskDeletion {
            is_deleted: true,
            peer_results,
        })
    }

    fn assert_worktree_has_no_uncommitted_changes(
        &self,
        worktree_path: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), TaskDeletionError> {
        let command = build_uncommitted_changes_command(worktree_path);
        let output = self
            .files
            .runner()
            .exec_git(&command, ssh_host, &ExecGitOptions::default())
            .map_err(ServiceError::from)?;
        match output.trim() {
            INVALID_WORKTREE_MARKER => Err(TaskDeletionError::InvalidWorktree),
            "" => Ok(()),
            _ => Err(TaskDeletionError::DirtyWorktree {
                worktree_path: worktree_path.to_string(),
            }),
        }
    }

    fn propagate_deletion_to_task_peers(&self, task: &KanbanTask) -> Vec<PeerTargetResult> {
        let Some(worktree_path) = task.worktree_path.as_deref() else {
            return Vec::new();
        };
        let peer_results = self
            .peers
            .propagate_task_deletion_to_peers(worktree_path, task.ssh_host.as_deref());
        let failed_peer_urls: Vec<String> = peer_results
            .iter()
            .filter(|result| !result.is_success)
            .map(|result| result.url.clone())
            .collect();
        if !failed_peer_urls.is_empty() {
            eprintln!(
                "[task-cli] 일부 client에서 task를 지우지 못했습니다: {{ taskId: {:?}, failedPeerUrls: {failed_peer_urls:?} }}",
                task.id
            );
            self.board
                .broadcast_task_peer_deletion_failed(TaskPeerDeletionFailedPayload {
                    task_id: task.id.clone(),
                    task_title: task.title.clone(),
                    failed_peer_urls,
                });
        }
        peer_results
    }

    /// 여러 작업을 차례로 지운다. 각 task를 다른 client로 전파하고, 보드 갱신은 한 번만 보낸다.
    /// 중간에 실패해도 그때까지 지운 task가 있으면 보드 갱신을 보낸 뒤 오류를 돌려준다
    pub fn delete_tasks(&self, task_ids: &[String]) -> Result<Vec<String>, TaskDeletionError> {
        let mut seen_ids = HashSet::new();
        let unique_ids: Vec<&String> = task_ids.iter().filter(|id| seen_ids.insert(*id)).collect();
        let mut deleted_task_ids = Vec::new();
        let result = self.delete_tasks_in_order(&unique_ids, &mut deleted_task_ids);
        if !deleted_task_ids.is_empty() {
            self.board.broadcast_board_update();
        }
        result.map(|()| deleted_task_ids)
    }

    fn delete_tasks_in_order(
        &self,
        task_ids: &[&String],
        deleted_task_ids: &mut Vec<String>,
    ) -> Result<(), TaskDeletionError> {
        for task_id in task_ids {
            let Some(task) = find_task_by_id(&self.database, task_id)? else {
                continue;
            };
            self.propagate_deletion_to_task_peers(&task);
            self.cleanup_task_resources(&task, true)?;
            delete_task_record(&self.database, task_id)?;
            self.board.broadcast_task_session_changed(task_id);
            deleted_task_ids.push(task_id.to_string());
        }
        Ok(())
    }
}
