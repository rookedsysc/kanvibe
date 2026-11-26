//! task 상태·설명·세션을 저장소의 `.kanvibe/` 파일로 남기고 읽는다(TS `kanvibeTaskStateService.ts`).
//! 파일 기록 실패는 호출자 흐름을 막지 않고 로그만 남긴다.

mod exclude_checks;
pub mod status_sync;
#[cfg(test)]
mod tests;

use std::time::Instant;

use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, TaskStatus};

use self::exclude_checks::StateDirectoryExcludeChecks;
use crate::host_file_access::HostFileError;
use crate::kanvibe_project_state::{
    KanvibeTaskMetadataUpdate, KanvibeTaskSession, KanvibeTaskSyncState,
};
use crate::kanvibe_state_store::{KanvibeStateStore, ProjectRecords};
use crate::project_color_service::ProjectColorService;

/// 보드에 다시 그리라고 알리는 프로세스 내 이벤트(TS `boardNotifier.broadcastBoardUpdate`)
pub trait BoardUpdateNotifier: Send + Sync {
    fn broadcast_board_update(&self);
}

/// 시각을 고정해야 하는 테스트가 바꿔 끼우는 시계
pub type MonotonicClock = dyn Fn() -> Instant + Send + Sync;

/// 상태 파일을 쓸 저장소 위치
#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskStateLocation {
    repo_path: String,
    ssh_host: Option<String>,
}

pub struct TaskStateService<'a> {
    state_store: &'a dyn KanvibeStateStore,
    records: &'a dyn ProjectRecords,
    exclude_checks: StateDirectoryExcludeChecks,
}

fn host_label(ssh_host: Option<&str>) -> String {
    ssh_host.map_or("null".into(), |host| format!("{host:?}"))
}

impl<'a> TaskStateService<'a> {
    pub fn new(state_store: &'a dyn KanvibeStateStore, records: &'a dyn ProjectRecords) -> Self {
        Self::with_clock(state_store, records, Box::new(Instant::now))
    }

    pub fn with_clock(
        state_store: &'a dyn KanvibeStateStore,
        records: &'a dyn ProjectRecords,
        clock: Box<MonotonicClock>,
    ) -> Self {
        Self {
            state_store,
            records,
            exclude_checks: StateDirectoryExcludeChecks::new(clock),
        }
    }

    pub fn read_persisted_task_status_at_path(
        &self,
        repo_path: Option<&str>,
        ssh_host: Option<&str>,
    ) -> Result<Option<TaskStatus>, HostFileError> {
        let Some(repo_path) = repo_path.filter(|path| !path.is_empty()) else {
            return Ok(None);
        };
        let task_state = self.state_store.read_task_state(repo_path, ssh_host)?;
        Ok(task_state.map(|state| state.status))
    }

    /// 다른 기기가 남긴 상태와 설명을 함께 읽는다. 기록이 없으면 빈 값이라 DB 값을 그대로 둔다
    pub fn read_persisted_task_sync_state_at_path(
        &self,
        repo_path: Option<&str>,
        ssh_host: Option<&str>,
        project_repo_path: Option<&str>,
    ) -> Result<KanvibeTaskSyncState, HostFileError> {
        let Some(repo_path) = repo_path.filter(|path| !path.is_empty()) else {
            return Ok(KanvibeTaskSyncState::default());
        };
        self.state_store
            .read_task_sync_state(repo_path, ssh_host, project_repo_path)
    }

    pub fn persist_task_state_at_path(
        &self,
        repo_path: Option<&str>,
        task_id: &str,
        status: TaskStatus,
        ssh_host: Option<&str>,
    ) {
        let Some(repo_path) = repo_path.filter(|path| !path.is_empty()) else {
            return;
        };
        self.ensure_kanvibe_state_directory_excluded(repo_path, ssh_host);
        if let Err(error) = self
            .state_store
            .write_task_status(repo_path, status, ssh_host)
        {
            eprintln!(
                ".kanvibe task 상태 저장 실패: {{ repoPath: {repo_path:?}, targetPath: {repo_path:?}, taskId: {task_id:?}, status: {:?}, sshHost: {}, error: {:?} }}",
                status.as_str(),
                host_label(ssh_host),
                error.to_string()
            );
        }
    }

    pub fn persist_task_state_for_task(&self, task: &KanbanTask) {
        let location = self.resolve_task_state_location(task);
        let repo_path = location
            .as_ref()
            .map(|location| location.repo_path.as_str());
        let ssh_host = location
            .as_ref()
            .and_then(|location| location.ssh_host.as_deref());
        self.persist_task_state_at_path(repo_path, &task.id, task.status, ssh_host);
    }

    /// task 설명과 priority를 책임에 맞는 공유 파일에 기록한다. 프로젝트 루트 task의 priority는 project.json에 둔다
    pub fn persist_task_metadata_for_task(&self, task: &KanbanTask) {
        let Some(location) = self.resolve_task_state_location(task) else {
            return;
        };
        let ssh_host = location.ssh_host.as_deref();
        if let Err(message) = self.write_task_metadata(task, &location) {
            eprintln!(
                ".kanvibe task 메타데이터 저장 실패: {{ repoPath: {:?}, taskId: {:?}, sshHost: {}, error: {message:?} }}",
                location.repo_path,
                task.id,
                host_label(ssh_host)
            );
        }
    }

    fn write_task_metadata(
        &self,
        task: &KanbanTask,
        location: &TaskStateLocation,
    ) -> Result<(), String> {
        let ssh_host = location.ssh_host.as_deref();
        self.ensure_kanvibe_state_directory_excluded(&location.repo_path, ssh_host);
        let project = self.find_task_project(task)?;
        let root_project = project
            .filter(|project| task.branch_name.as_deref() == Some(project.default_branch.as_str()));
        let metadata = KanvibeTaskMetadataUpdate {
            description: Some(task.description.clone()),
            priority: root_project.is_none().then_some(task.priority),
        };
        self.state_store
            .write_task_metadata(&location.repo_path, &metadata, ssh_host)
            .map_err(|error| error.to_string())?;
        if let Some(project) = root_project {
            ProjectColorService::new(self.state_store, self.records)
                .persist_project_priority_to_kanvibe_state(&project, task.priority);
        }
        Ok(())
    }

    /// 데스크탑이 정한 세션을 task.json에 남겨 같은 저장소를 보는 다른 KanVibe가 따라오게 한다
    pub fn persist_task_session_for_task(&self, task: &KanbanTask) {
        let (Some(session_type), Some(session_name)) =
            (task.session_type, task.session_name.as_deref())
        else {
            return;
        };
        if session_name.is_empty() {
            return;
        }
        let Some(location) = self.resolve_task_state_location(task) else {
            return;
        };
        let ssh_host = location.ssh_host.as_deref();
        self.ensure_kanvibe_state_directory_excluded(&location.repo_path, ssh_host);
        let session = KanvibeTaskSession {
            session_type,
            session_name: session_name.to_string(),
        };
        if let Err(error) =
            self.state_store
                .write_task_session(&location.repo_path, &session, ssh_host)
        {
            eprintln!(
                ".kanvibe task 세션 저장 실패: {{ repoPath: {:?}, taskId: {:?}, sshHost: {}, error: {:?} }}",
                location.repo_path,
                task.id,
                host_label(ssh_host),
                error.to_string()
            );
        }
    }

    /// git exclude 확인은 원격에서 호스트 키를 검증하는 새 SSH 접속을 두세 번 쓴다.
    /// 에이전트 hook 이벤트마다 되풀이하지 않도록 성공한 확인은 일정 시간 기억한다.
    fn ensure_kanvibe_state_directory_excluded(&self, repo_path: &str, ssh_host: Option<&str>) {
        self.exclude_checks.run_once(repo_path, ssh_host, || {
            let excluded = self.state_store.add_ai_tool_patterns_to_git_exclude(repo_path, ssh_host);
            if let Err(error) = &excluded {
                eprintln!(
                    ".kanvibe 상태 디렉터리 git exclude 갱신 실패: {{ repoPath: {repo_path:?}, sshHost: {}, error: {:?} }}",
                    host_label(ssh_host),
                    error.to_string()
                );
            }
            excluded.is_ok()
        });
    }

    fn resolve_task_state_location(&self, task: &KanbanTask) -> Option<TaskStateLocation> {
        if let Some(worktree_path) = task
            .worktree_path
            .as_deref()
            .filter(|path| !path.is_empty())
        {
            return Some(TaskStateLocation {
                repo_path: worktree_path.to_string(),
                ssh_host: task.ssh_host.clone(),
            });
        }

        let project = self.find_task_project(task).ok().flatten()?;
        let branch_name = task
            .branch_name
            .as_deref()
            .filter(|branch| !branch.is_empty())?;
        if branch_name != project.default_branch {
            return None;
        }
        let ssh_host =
            non_empty(task.ssh_host.clone()).or_else(|| non_empty(project.ssh_host.clone()));
        Some(TaskStateLocation {
            repo_path: project.repo_path,
            ssh_host,
        })
    }

    fn find_task_project(&self, task: &KanbanTask) -> Result<Option<Project>, String> {
        match task.project_id.as_deref().filter(|id| !id.is_empty()) {
            Some(project_id) => self.records.find_project(project_id),
            None => Ok(None),
        }
    }
}

/// TS의 `a || b`처럼 빈 문자열을 값이 없는 것으로 본다
fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.is_empty())
}
