//! 활성 task worktree를 fast-forward로 pull한다. task마다 차례로 실행하고, 같은 task의 실패는 성공 전까지 한 번만 알린다.

use std::sync::LazyLock;
use std::time::Duration;

use kanvibe_domain::notifications::{BackgroundSyncPulledTaskPayload, PulledTaskStatus};
use kanvibe_domain::task::KanbanTask;
use kanvibe_process::exec::{ExecGitError, ExecGitOptions};
use kanvibe_process::git_operations::{pull_current_branch, remote_branch_exists};
use regex::Regex;

use super::pull_requests::{has_branch, is_default_branch_task};
use super::records::{find_project_by_id, select_tasks};
use super::{ActiveTaskPullSyncResult, KanbanService};
use crate::error::ServiceResult;

const ACTIVE_TASK_PULL_GIT_TIMEOUT: Duration = Duration::from_millis(10_000);

static PULL_NOOP_PATTERNS: LazyLock<[Regex; 2]> = LazyLock::new(|| {
    [
        Regex::new(r"(?i)already up[- ]to[- ]date").expect("유효한 정규식"),
        Regex::new(r"(?i)current branch .* is up to date").expect("유효한 정규식"),
    ]
});
static MISSING_REMOTE_BRANCH_PATTERNS: LazyLock<[Regex; 3]> = LazyLock::new(|| {
    [
        Regex::new(r"(?i)no such ref was fetched").expect("유효한 정규식"),
        Regex::new(r"(?i)could(?: not|n't) find remote ref").expect("유효한 정규식"),
        Regex::new(r"(?i)requested upstream branch .* does not exist").expect("유효한 정규식"),
    ]
});

fn summarize_pull_output(output: &str) -> String {
    output
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).trim())
        .find(|line| !line.is_empty())
        .unwrap_or("Pull completed")
        .to_string()
}

fn is_pull_noop(output: &str) -> bool {
    PULL_NOOP_PATTERNS
        .iter()
        .any(|pattern| pattern.is_match(output))
}

fn is_missing_remote_branch_pull_error(error: &ExecGitError) -> bool {
    MISSING_REMOTE_BRANCH_PATTERNS
        .iter()
        .any(|pattern| pattern.is_match(&error.message))
}

/// 로컬만 명령 자체에 제한 시간을 걸고, 원격은 SSH 계층의 제한 시간을 따른다
fn pull_exec_options(ssh_host: Option<&str>) -> ExecGitOptions {
    ExecGitOptions {
        timeout: ssh_host.is_none().then_some(ACTIVE_TASK_PULL_GIT_TIMEOUT),
        ..ExecGitOptions::default()
    }
}

/// pull할 task와 그 위치
struct PullTarget<'a> {
    task: &'a KanbanTask,
    branch_name: &'a str,
    worktree_path: &'a str,
    ssh_host: Option<String>,
}

impl PullTarget<'_> {
    fn failure_key(&self) -> String {
        [
            self.task.id.as_str(),
            self.branch_name,
            self.worktree_path,
            self.ssh_host.as_deref().unwrap_or_default(),
        ]
        .join("::")
    }

    fn payload(
        &self,
        status: PulledTaskStatus,
        summary: String,
    ) -> BackgroundSyncPulledTaskPayload {
        BackgroundSyncPulledTaskPayload {
            task_id: self.task.id.clone(),
            task_title: self.task.title.clone(),
            branch_name: self.branch_name.to_string(),
            worktree_path: self.worktree_path.to_string(),
            ssh_host: self.ssh_host.clone(),
            status,
            summary,
        }
    }
}

impl KanbanService {
    /// Done이 아닌 task의 worktree를 pull한다. 기본 브랜치 task와 remote 브랜치가 없는 task는 건너뛴다
    pub fn sync_active_task_pulls(&self) -> ServiceResult<ActiveTaskPullSyncResult> {
        let tasks = select_tasks(
            &self.database,
            "SELECT id FROM kanban_tasks WHERE status IN ('todo', 'progress', 'pending', 'review') \
             ORDER BY updated_at ASC, rowid ASC",
            &[],
        )?;
        let mut pulled_tasks = Vec::new();
        for task in &tasks {
            if let Some(target) = self.resolve_pull_target(task)? {
                pulled_tasks.extend(self.pull_task(&target));
            }
        }
        Ok(ActiveTaskPullSyncResult { pulled_tasks })
    }

    fn resolve_pull_target<'a>(
        &self,
        task: &'a KanbanTask,
    ) -> ServiceResult<Option<PullTarget<'a>>> {
        let worktree_path = task
            .worktree_path
            .as_deref()
            .filter(|path| !path.is_empty());
        let (true, Some(worktree_path)) = (has_branch(task), worktree_path) else {
            return Ok(None);
        };
        let project = match task.project_id.as_deref() {
            Some(project_id) => find_project_by_id(&self.database, project_id)?,
            None => None,
        };
        if is_default_branch_task(task, project.as_ref()) {
            return Ok(None);
        }
        let ssh_host = task
            .ssh_host
            .clone()
            .filter(|host| !host.is_empty())
            .or_else(|| project.and_then(|project| project.ssh_host))
            .filter(|host| !host.is_empty());
        Ok(Some(PullTarget {
            task,
            branch_name: task.branch_name.as_deref().unwrap_or_default(),
            worktree_path,
            ssh_host,
        }))
    }

    /// pull 결과 중 화면에 알릴 것. 변화가 없거나 이미 알린 실패면 `None`이다
    fn pull_task(&self, target: &PullTarget<'_>) -> Option<BackgroundSyncPulledTaskPayload> {
        let failure_key = target.failure_key();
        let notified_failures = || self.notified_pull_failure_keys.lock().unwrap();
        match self.run_task_pull(target) {
            Ok(None) => {
                notified_failures().remove(&failure_key);
                None
            }
            Ok(Some(output)) => {
                notified_failures().remove(&failure_key);
                (!is_pull_noop(&output)).then(|| {
                    target.payload(PulledTaskStatus::Updated, summarize_pull_output(&output))
                })
            }
            Err(error) if is_missing_remote_branch_pull_error(&error) => {
                notified_failures().remove(&failure_key);
                None
            }
            Err(error) => notified_failures()
                .insert(failure_key)
                .then(|| target.payload(PulledTaskStatus::Failed, error.message)),
        }
    }

    /// remote 브랜치가 없으면 `None`, 있으면 pull 출력이다
    fn run_task_pull(&self, target: &PullTarget<'_>) -> Result<Option<String>, ExecGitError> {
        let ssh_host = target.ssh_host.as_deref();
        let options = pull_exec_options(ssh_host);
        let has_remote_branch = remote_branch_exists(
            &self.files,
            target.worktree_path,
            target.branch_name,
            ssh_host,
            &options,
        )?;
        if !has_remote_branch {
            return Ok(None);
        }
        pull_current_branch(&self.files, target.worktree_path, ssh_host, &options).map(Some)
    }
}
