//! 프로젝트 기본 브랜치 task(root task)를 만들고 공유 상태 파일에 맞춰 복구한다.

use kanvibe_db::repositories::tasks::NewKanbanTask;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskStatus};
use kanvibe_process::kanvibe_project_state::{KanvibeTaskSession, KanvibeTaskSyncState};

use super::{ProjectService, records};
use crate::error::{ServiceError, ServiceResult};

/// root task 복구 결과
pub(super) struct RootTaskRepair {
    pub task: Option<KanbanTask>,
    pub has_repaired: bool,
}

/// 위치가 같은 task인지. 같은 경로라도 다른 호스트면 다른 task다
pub(super) fn matches_task_location(
    task: &KanbanTask,
    expected_path: &str,
    ssh_host: Option<&str>,
) -> bool {
    let task_host = task.ssh_host.as_deref().filter(|host| !host.is_empty());
    let expected_host = ssh_host.filter(|host| !host.is_empty());
    task.worktree_path.as_deref() == Some(expected_path) && task_host == expected_host
}

/// 다른 KanVibe가 task.json에 남긴 세션이 이 기기의 기록과 다르면 그 세션을 따른다.
/// 기존 세션을 닫을지는 바꾼 쪽이 이미 정했으므로 여기서는 세션 정보만 맞춘다
pub(super) fn apply_persisted_task_session(
    task: &mut KanbanTask,
    persisted_session: Option<&KanvibeTaskSession>,
) -> bool {
    let Some(session) = persisted_session else {
        return false;
    };
    let is_same_session = task.session_type == Some(session.session_type)
        && task.session_name.as_deref() == Some(session.session_name.as_str());
    if is_same_session {
        return false;
    }
    task.session_type = Some(session.session_type);
    task.session_name = Some(session.session_name.clone());
    true
}

impl ProjectService {
    /// 이전 세션에 붙어 있던 터미널을 끊고, 화면들이 새 세션으로 다시 붙도록 알린다
    pub(super) fn reconnect_task_to_followed_session(&self, task_id: &str) {
        self.sessions
            .detach_session(task_id, "follow-shared-task-session");
        self.board.broadcast_task_session_changed(task_id);
    }

    /// 기본 브랜치를 checkout한 worktree 경로. 판정할 수 없거나 사라졌으면 `None`이다
    pub(super) fn resolve_default_branch_worktree_path(&self, project: &Project) -> Option<String> {
        let ssh_host = project.ssh_host.as_deref();
        let worktrees = self
            .repositories
            .list_worktrees(&project.repo_path, ssh_host);
        let default_worktree = worktrees
            .into_iter()
            .find(|worktree| !worktree.is_bare && worktree.branch == project.default_branch)?;
        match self
            .repositories
            .is_path_missing(&default_worktree.path, ssh_host)
        {
            Ok(false) => Some(default_worktree.path),
            _ => None,
        }
    }

    pub(super) fn project_root_task(&self, project: &Project) -> ServiceResult<Option<KanbanTask>> {
        records::project_branch_task(&self.database, &project.id, &project.default_branch)
    }

    fn read_task_status(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
    ) -> ServiceResult<Option<TaskStatus>> {
        self.state_files
            .read_task_status(Some(repo_path), ssh_host)
            .map_err(ServiceError::new)
    }

    /// 프로젝트의 메인 브랜치 task를 만든다. 이미 있으면 상태 파일만 맞추고, 같은 위치의 orphan task는 이 프로젝트에 붙인다
    fn create_default_branch_task(&self, project: &Project) -> ServiceResult<KanbanTask> {
        let ssh_host = project.ssh_host.as_deref();
        let default_session_type = self.default_session_type()?;
        let worktree_path = self.resolve_default_branch_worktree_path(project);
        let state_path = worktree_path
            .clone()
            .unwrap_or_else(|| project.repo_path.clone());
        let persisted_status = self.read_task_status(&state_path, ssh_host)?;

        let reused = match self.project_root_task(project)? {
            Some(existing) => Some(existing),
            None => self.adopt_root_orphan(project, persisted_status)?,
        };
        let saved = match reused {
            Some(task) => task,
            None => self.insert_default_branch_task(
                project,
                worktree_path,
                persisted_status,
                default_session_type,
            )?,
        };
        self.state_files
            .persist_task_state(Some(&state_path), &saved.id, saved.status, ssh_host);
        Ok(saved)
    }

    /// 새 root task를 저장한다. 로컬 기본 브랜치 worktree가 있으면 기본 세션 이름도 함께 남긴다
    fn insert_default_branch_task(
        &self,
        project: &Project,
        worktree_path: Option<String>,
        persisted_status: Option<TaskStatus>,
        default_session_type: SessionType,
    ) -> ServiceResult<KanbanTask> {
        let ssh_host = project.ssh_host.as_deref();
        let mut task = NewKanbanTask {
            title: project.default_branch.clone(),
            branch_name: Some(project.default_branch.clone()),
            worktree_path: worktree_path.clone(),
            ssh_host: project.ssh_host.clone(),
            status: persisted_status.unwrap_or(TaskStatus::Todo),
            project_id: Some(project.id.clone()),
            base_branch: Some(project.default_branch.clone()),
            ..records::empty_new_task()
        };
        if let (None, Some(directory)) = (ssh_host, worktree_path.as_deref()) {
            match self.sessions.create_session_without_worktree(
                &project.repo_path,
                &project.default_branch,
                default_session_type,
                ssh_host,
                Some(directory),
            ) {
                Ok(session_name) => {
                    task.session_type = Some(default_session_type);
                    task.session_name = Some(session_name);
                }
                Err(error) => eprintln!("메인 브랜치 tmux 세션 생성 실패: {error}"),
            }
        }
        records::insert_new_task(&self.database, &task)
    }

    fn adopt_root_orphan(
        &self,
        project: &Project,
        persisted_status: Option<TaskStatus>,
    ) -> ServiceResult<Option<KanbanTask>> {
        let orphan = records::orphan_branch_task(&self.database, &project.default_branch)?;
        let Some(mut orphan) = orphan.filter(|orphan| {
            matches_task_location(orphan, &project.repo_path, project.ssh_host.as_deref())
        }) else {
            return Ok(None);
        };
        orphan.project_id = Some(project.id.clone());
        orphan.title = project.default_branch.clone();
        orphan.worktree_path = Some(project.repo_path.clone());
        orphan.ssh_host = project.ssh_host.clone();
        orphan.base_branch = Some(project.default_branch.clone());
        orphan.status = persisted_status.unwrap_or(TaskStatus::Todo);
        records::save_task(&self.database, &orphan).map(Some)
    }

    /// root task가 없으면 만들고, 위치·상태·설명·priority·세션을 공유 파일에 맞춘다.
    /// `should_repair_hooks`일 때만 hook을 확인해 복구하고, 그 실패는 로그만 남긴다
    pub(super) fn ensure_project_root_task(
        &self,
        project: &Project,
        should_repair_hooks: bool,
    ) -> ServiceResult<RootTaskRepair> {
        let ssh_host = project.ssh_host.as_deref();
        let is_missing = self
            .repositories
            .is_path_missing(&project.repo_path, ssh_host)
            .map_err(ServiceError::new)?;
        if self.is_root_repair_blocked(&project.id) || is_missing {
            return Ok(RootTaskRepair {
                task: None,
                has_repaired: false,
            });
        }

        let (task, has_created) = match self.project_root_task(project)? {
            Some(task) => (task, false),
            None => (self.create_default_branch_task(project)?, true),
        };
        let (task, has_synced) = self.sync_root_task_with_shared_state(project, task)?;
        let mut has_repaired = has_created || has_synced;
        if should_repair_hooks {
            has_repaired = self.repair_root_hooks(project, &task) || has_repaired;
        }
        Ok(RootTaskRepair {
            task: Some(task),
            has_repaired,
        })
    }

    fn repair_root_hooks(&self, project: &Project, task: &KanbanTask) -> bool {
        let ssh_host = project.ssh_host.as_deref();
        match self
            .hooks
            .ensure_hooks_current(&project.repo_path, &task.id, ssh_host)
        {
            Ok(has_repaired) => has_repaired,
            Err(error) => {
                eprintln!("{} 기본 브랜치 hooks 복구 실패: {error}", project.name);
                false
            }
        }
    }

    /// 공유 파일의 값으로 root task를 맞추고, 바뀐 것이 있으면 저장한다
    fn sync_root_task_with_shared_state(
        &self,
        project: &Project,
        mut task: KanbanTask,
    ) -> ServiceResult<(KanbanTask, bool)> {
        let ssh_host = project.ssh_host.as_deref();
        let expected_path = self.resolve_default_branch_worktree_path(project);
        let state_path = expected_path
            .clone()
            .unwrap_or_else(|| project.repo_path.clone());
        let shared = self
            .state_files
            .read_task_sync_state(Some(&state_path), ssh_host, &project.repo_path)
            .map_err(ServiceError::new)?;

        let mut should_save = apply_root_location(&mut task, project, expected_path);
        should_save = apply_root_shared_fields(&mut task, &shared) || should_save;
        let has_followed_session = apply_persisted_task_session(&mut task, shared.session.as_ref());
        should_save = should_save || has_followed_session;

        if should_save {
            task = records::save_task(&self.database, &task)?;
        }
        if has_followed_session {
            self.reconnect_task_to_followed_session(&task.id);
        }
        if shared.status != Some(task.status) {
            self.state_files
                .persist_task_state(Some(&state_path), &task.id, task.status, ssh_host);
        }
        Ok((task, should_save))
    }
}

fn apply_root_location(
    task: &mut KanbanTask,
    project: &Project,
    expected_path: Option<String>,
) -> bool {
    let mut has_changed = false;
    if task.base_branch.as_deref() != Some(project.default_branch.as_str()) {
        task.base_branch = Some(project.default_branch.clone());
        has_changed = true;
    }
    if task.worktree_path != expected_path {
        task.worktree_path = expected_path;
        has_changed = true;
    }
    if task.ssh_host != project.ssh_host {
        task.ssh_host = project.ssh_host.clone();
        has_changed = true;
    }
    has_changed
}

/// root task의 상태·설명·priority는 task 파일 값과 프로젝트 priority를 따른다
fn apply_root_shared_fields(task: &mut KanbanTask, shared: &KanvibeTaskSyncState) -> bool {
    let mut has_changed = false;
    if let Some(status) = shared.status.filter(|status| *status != task.status) {
        task.status = status;
        has_changed = true;
    }
    if let Some(description) = shared.description.as_ref() {
        let persisted = description.description.clone().flatten();
        if task.description != persisted {
            task.description = persisted;
            has_changed = true;
        }
    }
    if let Some(priority) = shared
        .project_priority
        .filter(|priority| *priority != task.priority)
    {
        task.priority = priority;
        has_changed = true;
    }
    has_changed
}
