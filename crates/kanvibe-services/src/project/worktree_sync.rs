//! 등록된 프로젝트의 worktree를 task와 맞춘다. 사라진 프로젝트·worktree의 task를 정리하고,
//! 새 worktree는 TODO task로 등록하며, 다른 기기가 `.kanvibe/`에 남긴 상태·설명·priority·세션을 반영한다.

use std::collections::BTreeMap;
use std::sync::Arc;

use kanvibe_ai::session_listing::map_with_concurrency;
use kanvibe_db::repositories::tasks::NewKanbanTask;
use kanvibe_domain::notifications::BackgroundSyncRegisteredWorktreePayload;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskStatus};
use kanvibe_process::git_operations::WorktreeEntry;
use kanvibe_process::kanvibe_project_state::KanvibeTaskSyncState;
use kanvibe_process::worktree::{format_project_branch_session_name, sanitize_herdr_session_name};

use super::root_task::{apply_persisted_task_session, matches_task_location};
use super::{
    ProjectService, RegisteredProjectWorktreeSyncResult, is_remote_connection_error,
    project_path_key, records,
};
use crate::error::{ServiceError, ServiceResult};

/// 아이콘 backfill 동시 실행 수. GitHub에 과도한 동시 요청을 보내지 않으면서 순차 대기를 줄이는 값
const PROJECT_ICON_BACKFILL_CONCURRENCY: usize = 8;

const NOT_GIT_REPOSITORY_MARKER: &str = "__KANVIBE_NOT_GIT_REPOSITORY__";

/// 경로 확인 결과. 판정할 수 없으면 오류 메시지를 남긴다
type ProjectPathCheck = Result<bool, String>;

/// 살아 있는 브랜치 세션
pub(super) struct AliveBranchSession {
    pub session_type: SessionType,
    pub session_name: String,
}

/// 호스트별로 묶는다. 같은 호스트 안에서는 입력 순서를 지킨다
fn group_by_host(projects: &[Project]) -> BTreeMap<String, Vec<&Project>> {
    let mut groups: BTreeMap<String, Vec<&Project>> = BTreeMap::new();
    for project in projects {
        let host_key = project.ssh_host.clone().unwrap_or_default();
        groups.entry(host_key).or_default().push(project);
    }
    groups
}

/// 호스트마다 스레드 하나에서 그 호스트의 프로젝트를 입력 순서대로 처리하고, 결과를 프로젝트 id로 모은다
fn run_per_host<R: Send>(
    projects: &[Project],
    run: impl Fn(&Project) -> R + Sync,
) -> BTreeMap<String, R> {
    let groups = group_by_host(projects);
    let run = &run;
    std::thread::scope(|scope| {
        let handles: Vec<_> = groups
            .values()
            .map(|host_projects| scope.spawn(move || run_host_projects(host_projects, run)))
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("호스트별 작업 스레드"))
            .collect()
    })
}

fn run_host_projects<R>(projects: &[&Project], run: &impl Fn(&Project) -> R) -> Vec<(String, R)> {
    projects
        .iter()
        .map(|project| (project.id.clone(), run(project)))
        .collect()
}

impl ProjectService {
    /// 등록된 프로젝트(`projects`가 없으면 전부)의 worktree를 동기화한다.
    /// 경로가 사라진 프로젝트는 task와 함께 지우고, 아이콘 backfill은 worktree 스캔과 분리해 먼저 병렬로 끝낸다
    pub fn sync_registered_project_worktrees(
        &self,
        projects: Option<&[Project]>,
    ) -> ServiceResult<RegisteredProjectWorktreeSyncResult> {
        let projects = match projects {
            Some(projects) => projects.to_vec(),
            None => records::all_projects(&self.database)?,
        };
        let mut merged = RegisteredProjectWorktreeSyncResult::default();
        let path_checks = self.check_project_paths_by_host(&projects);
        let mut existing_projects = Vec::new();
        for (project, path_check) in projects.into_iter().zip(path_checks) {
            match path_check
                .map_err(ServiceError::new)
                .and_then(|is_missing| {
                    if is_missing {
                        self.delete_project(&project.id).map(Some)
                    } else {
                        Ok(None)
                    }
                }) {
                Ok(Some(has_deleted)) => merged.has_changed = has_deleted || merged.has_changed,
                Ok(None) => existing_projects.push(project),
                Err(error) => merged.errors.push(format!(
                    "{} 경로 확인 실패: {}",
                    project.name, error.message
                )),
            }
        }

        merged.has_changed =
            self.backfill_missing_project_icons(&existing_projects) || merged.has_changed;
        for result in self.schedule_worktree_syncs(&existing_projects) {
            merged.merge(result);
        }
        Ok(merged)
    }

    /// 프로젝트 경로가 남아 있는지 확인한다. 같은 호스트는 차례로, 다른 호스트끼리는 동시에 확인한다.
    /// 모두 한 줄로 세우면 느리거나 끊긴 호스트 하나가 다른 호스트와 로컬 프로젝트의 동기화까지 미룬다
    fn check_project_paths_by_host(&self, projects: &[Project]) -> Vec<ProjectPathCheck> {
        let checks = run_per_host(projects, |project| {
            self.repositories
                .is_path_missing(&project.repo_path, project.ssh_host.as_deref())
        });
        projects
            .iter()
            .map(|project| checks.get(&project.id).cloned().unwrap_or(Ok(false)))
            .collect()
    }

    /// 같은 호스트의 프로젝트는 차례로, 다른 호스트끼리는 함께 동기화한다. 결과는 입력 순서로 모은다
    fn schedule_worktree_syncs(
        &self,
        projects: &[Project],
    ) -> Vec<RegisteredProjectWorktreeSyncResult> {
        let mut results = run_per_host(projects, |project| {
            self.schedule_project_worktree_sync(project)
        });
        projects
            .iter()
            .filter_map(|project| results.remove(&project.id))
            .collect()
    }

    fn schedule_project_worktree_sync(
        &self,
        project: &Project,
    ) -> RegisteredProjectWorktreeSyncResult {
        let project_key = project_path_key(&project.repo_path, project.ssh_host.as_deref());
        let host_key = project.ssh_host.clone().unwrap_or_default();
        self.worktree_jobs.run(&project_key, &host_key, || {
            if self.is_root_repair_blocked(&project.id) {
                return RegisteredProjectWorktreeSyncResult::default();
            }
            self.sync_project_worktrees(project)
        })
    }

    /// 아이콘이 없는 프로젝트들의 아이콘 확보를 병렬로 처리한다. 새로 확보한 아이콘이 있으면 `true`
    fn backfill_missing_project_icons(&self, projects: &[Project]) -> bool {
        let projects_without_icon: Vec<&Project> = projects
            .iter()
            .filter(|project| project.icon_data_url.is_none())
            .collect();
        let icon_results = map_with_concurrency(
            &projects_without_icon,
            PROJECT_ICON_BACKFILL_CONCURRENCY,
            |project, _| self.sync_missing_project_icon(project).unwrap_or(false),
        );
        icon_results.into_iter().any(|has_icon| has_icon)
    }

    /// 아직 아이콘이 없는 프로젝트의 GitHub 아이콘을 뒤늦게 채운다. 이미 있는 행에만 반영한다
    pub(super) fn sync_missing_project_icon(&self, project: &Project) -> ServiceResult<bool> {
        if project.icon_data_url.is_some() {
            return Ok(false);
        }
        let Some(icon_data_url) = self
            .icons
            .resolve_project_icon_data_url(&project.repo_path, project.ssh_host.as_deref())
        else {
            return Ok(false);
        };
        records::update_existing_project_icon(&self.database, &project.id, &icon_data_url)
    }

    /// 다른 client가 `.kanvibe/project.json`에 남긴 색상을 DB에 반영한다. 읽어 둔 색상이 아직 DB에 있을 때만 덮어쓴다
    fn sync_shared_project_color(&self, project: &Project) -> ServiceResult<bool> {
        let Some(shared_color) = self.state_files.sync_project_color(project) else {
            return Ok(false);
        };
        records::update_existing_project_color(
            &self.database,
            &project.id,
            project.color.as_deref(),
            &shared_color,
        )
    }

    /// 브랜치 이름으로 살아 있는 세션을 찾는다. Herdr 세션 이름은 생성할 때처럼 변환해 비교한다
    pub(super) fn find_alive_branch_session(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Option<AliveBranchSession> {
        if self
            .sessions
            .is_session_alive(SessionType::Tmux, session_name, ssh_host)
        {
            return Some(AliveBranchSession {
                session_type: SessionType::Tmux,
                session_name: session_name.to_string(),
            });
        }
        let herdr_session_name = sanitize_herdr_session_name(session_name);
        let shared_session_name = format!("kanvibe:{herdr_session_name}");
        [shared_session_name, herdr_session_name]
            .into_iter()
            .find(|name| {
                self.sessions
                    .is_session_alive(SessionType::Herdr, name, ssh_host)
            })
            .map(|session_name| AliveBranchSession {
                session_type: SessionType::Herdr,
                session_name,
            })
    }

    /// 저장소가 아니라고 확인됐을 때만 `false`. 연결 실패처럼 판정할 수 없으면 오류다
    fn is_project_git_repository(&self, project: &Project) -> Result<bool, String> {
        let ssh_host = project.ssh_host.as_deref();
        let command = format!(
            "git -C {} rev-parse --is-inside-work-tree >/dev/null 2>&1 || printf '%s' '{NOT_GIT_REPOSITORY_MARKER}'",
            self.shell_path(&project.repo_path, ssh_host)
        );
        let output = self.repositories.exec_git(&command, ssh_host)?;
        Ok(output.trim() != NOT_GIT_REPOSITORY_MARKER)
    }

    /// 프로젝트 하나의 worktree를 task와 맞춘다. 스캔 중 실패는 결과의 오류로 남긴다
    fn sync_project_worktrees(&self, project: &Project) -> RegisteredProjectWorktreeSyncResult {
        let mut result = RegisteredProjectWorktreeSyncResult::default();
        // 판정할 수 없으면 기존처럼 동기화를 진행하고, 저장소가 아니라고 확인됐을 때만 건너뛴다
        if !self.is_project_git_repository(project).unwrap_or(true) {
            // 상태 파일을 쓰면 mkdir -p가 사라진 폴더를 다시 만들기 때문에 이 프로젝트에는 아무것도 쓰지 않는다
            eprintln!(
                "{} 경로가 git 저장소가 아니어서 동기화를 건너뜁니다: {{ repoPath: {:?}, sshHost: {:?} }}",
                project.name, project.repo_path, project.ssh_host
            );
            return result;
        }
        if let Err(error) = self.sync_project_worktree_tasks(project, &mut result) {
            result.errors.push(format!(
                "{} worktree 스캔 실패: {}",
                project.name, error.message
            ));
            return result;
        }
        if let Err(error) = self.attach_alive_main_branch_session(project, &mut result) {
            eprintln!(
                "{} 메인 브랜치 세션 감지 실패: {}",
                project.name, error.message
            );
        }
        result
    }

    fn sync_project_worktree_tasks(
        &self,
        project: &Project,
        result: &mut RegisteredProjectWorktreeSyncResult,
    ) -> ServiceResult<()> {
        let ssh_host = project.ssh_host.as_deref();
        let worktrees = self
            .repositories
            .list_worktrees(&project.repo_path, ssh_host);
        if !worktrees.is_empty() {
            result.has_changed =
                self.remove_deleted_worktree_tasks(project, &worktrees)? || result.has_changed;
        }
        let repair = self.ensure_project_root_task(project, false)?;
        result.has_changed = result.has_changed || repair.has_repaired;

        for worktree in &worktrees {
            if worktree.is_bare || worktree.branch.is_empty() {
                continue;
            }
            let is_missing = self
                .repositories
                .is_path_missing(&worktree.path, ssh_host)
                .map_err(ServiceError::new)?;
            // 메인 작업 디렉터리(프로젝트 루트)는 기본 브랜치 task와 중복되므로 건너뛴다
            if is_missing || worktree.path == project.repo_path {
                continue;
            }
            self.sync_worktree_task(project, worktree, result)?;
        }
        if !worktrees.is_empty() {
            result.has_changed = self.sync_shared_project_color(project)? || result.has_changed;
        }
        Ok(())
    }

    /// 디스크에서 사라진 worktree의 task만 지운다. git으로 옮긴 worktree의 task는 남긴다
    fn remove_deleted_worktree_tasks(
        &self,
        project: &Project,
        worktrees: &[WorktreeEntry],
    ) -> ServiceResult<bool> {
        let mut has_deleted = false;
        for task in records::project_tasks(&self.database, &project.id)? {
            if self.is_deleted_worktree_task(project, &task, worktrees)?
                && records::delete_project_worktree_task(&self.database, &task, &project.id)?
            {
                has_deleted = true;
            }
        }
        Ok(has_deleted)
    }

    fn is_deleted_worktree_task(
        &self,
        project: &Project,
        task: &KanbanTask,
        worktrees: &[WorktreeEntry],
    ) -> ServiceResult<bool> {
        let Some(worktree_path) = task
            .worktree_path
            .as_deref()
            .filter(|path| !path.is_empty())
        else {
            return Ok(false);
        };
        if task.branch_name.as_deref() == Some(project.default_branch.as_str()) {
            return Ok(false);
        }
        let task_host = task.ssh_host.as_deref().or(project.ssh_host.as_deref());
        let is_missing = |path: &str, host: Option<&str>| {
            self.repositories
                .is_path_missing(path, host)
                .map_err(ServiceError::new)
        };
        if !is_missing(worktree_path, task_host)? {
            return Ok(false);
        }
        let moved_worktree = worktrees.iter().find(|worktree| {
            !worktree.is_bare
                && !worktree.branch.is_empty()
                && task.branch_name.as_deref() == Some(worktree.branch.as_str())
                && worktree.path != project.repo_path
                && task_host == project.ssh_host.as_deref()
        });
        match moved_worktree {
            Some(moved) => is_missing(&moved.path, project.ssh_host.as_deref()),
            None => Ok(true),
        }
    }

    fn sync_worktree_task(
        &self,
        project: &Project,
        worktree: &WorktreeEntry,
        result: &mut RegisteredProjectWorktreeSyncResult,
    ) -> ServiceResult<()> {
        let ssh_host = project.ssh_host.as_deref();
        let existing_task =
            records::project_branch_task(&self.database, &project.id, &worktree.branch)?;
        let shared = self
            .state_files
            .read_task_sync_state(Some(&worktree.path), ssh_host, &project.repo_path)
            .map_err(ServiceError::new)?;
        if let Some(existing_task) = existing_task {
            return self.repair_existing_worktree_task(
                project,
                worktree,
                existing_task,
                &shared,
                result,
            );
        }
        let orphan = records::orphan_branch_task(&self.database, &worktree.branch)?
            .filter(|orphan| matches_task_location(orphan, &worktree.path, ssh_host));
        let saved = match orphan {
            Some(orphan) => self.adopt_worktree_orphan(project, worktree, orphan, &shared)?,
            None => self.create_worktree_task(project, worktree, &shared)?,
        };
        result.has_changed = true;
        self.state_files.persist_task_state(
            Some(&worktree.path),
            &saved.id,
            saved.status,
            ssh_host,
        );
        self.install_synced_worktree_hooks(
            project,
            &worktree.path,
            &saved.id,
            &worktree.branch,
            result,
        );
        record_registered_worktree(project, worktree, saved.id, result);
        Ok(())
    }

    fn repair_existing_worktree_task(
        &self,
        project: &Project,
        worktree: &WorktreeEntry,
        mut task: KanbanTask,
        shared: &KanvibeTaskSyncState,
        result: &mut RegisteredProjectWorktreeSyncResult,
    ) -> ServiceResult<()> {
        let ssh_host = project.ssh_host.as_deref();
        let has_followed_session = apply_persisted_task_session(&mut task, shared.session.as_ref());
        let should_repair = !matches_task_location(&task, &worktree.path, ssh_host)
            || task.base_branch.as_deref().is_none_or(str::is_empty)
            || shared.status.is_some_and(|status| status != task.status)
            || shared_description(shared)
                .is_some_and(|description| description != task.description)
            || shared
                .priority
                .is_some_and(|priority| priority != task.priority)
            || has_followed_session;
        if should_repair {
            task.worktree_path = Some(worktree.path.clone());
            task.ssh_host = project.ssh_host.clone();
            if task.base_branch.as_deref().is_none_or(str::is_empty) {
                task.base_branch = Some(project.default_branch.clone());
            }
            apply_worktree_shared_fields(&mut task, shared);
            task = records::save_task(&self.database, &task)?;
            result.has_changed = true;
            if has_followed_session {
                self.reconnect_task_to_followed_session(&task.id);
            }
            self.install_synced_worktree_hooks(
                project,
                &worktree.path,
                &task.id,
                &worktree.branch,
                result,
            );
        }
        if shared.status != Some(task.status) {
            self.state_files.persist_task_state(
                Some(&worktree.path),
                &task.id,
                task.status,
                ssh_host,
            );
        }
        Ok(())
    }

    /// 같은 위치의 orphan task(projectId 없음)를 이 프로젝트에 붙인다
    fn adopt_worktree_orphan(
        &self,
        project: &Project,
        worktree: &WorktreeEntry,
        mut orphan: KanbanTask,
        shared: &KanvibeTaskSyncState,
    ) -> ServiceResult<KanbanTask> {
        orphan.project_id = Some(project.id.clone());
        orphan.title = worktree.branch.clone();
        orphan.worktree_path = Some(worktree.path.clone());
        orphan.ssh_host = project.ssh_host.clone();
        if orphan.base_branch.as_deref().is_none_or(str::is_empty) {
            orphan.base_branch = Some(project.default_branch.clone());
        }
        orphan.status = shared.status.unwrap_or(TaskStatus::Todo);
        if let Some(description) = shared_description(shared) {
            orphan.description = description;
        }
        if let Some(priority) = shared.priority {
            orphan.priority = priority;
        }
        records::save_task(&self.database, &orphan)
    }

    /// 새 worktree를 task로 등록한다. 브랜치 이름의 세션이 살아 있으면 연결 정보를 함께 남긴다
    fn create_worktree_task(
        &self,
        project: &Project,
        worktree: &WorktreeEntry,
        shared: &KanvibeTaskSyncState,
    ) -> ServiceResult<KanbanTask> {
        let ssh_host = project.ssh_host.as_deref();
        let session_name = format_project_branch_session_name(&project.repo_path, &worktree.branch);
        let alive_session = self.find_alive_branch_session(&session_name, ssh_host);
        let task = NewKanbanTask {
            title: worktree.branch.clone(),
            branch_name: Some(worktree.branch.clone()),
            worktree_path: Some(worktree.path.clone()),
            project_id: Some(project.id.clone()),
            base_branch: Some(project.default_branch.clone()),
            status: shared.status.unwrap_or(TaskStatus::Todo),
            description: shared_description(shared).flatten(),
            priority: shared.priority.flatten(),
            ssh_host: project.ssh_host.clone(),
            session_type: alive_session.as_ref().map(|session| session.session_type),
            session_name: alive_session.map(|session| session.session_name),
            ..records::empty_new_task()
        };
        records::insert_new_task(&self.database, &task)
    }

    /// 원격은 hook 설치를 백그라운드로 예약하고, 로컬은 바로 설치해 실패를 결과에 남긴다
    pub(super) fn install_synced_worktree_hooks(
        &self,
        project: &Project,
        worktree_path: &str,
        task_id: &str,
        branch_name: &str,
        result: &mut RegisteredProjectWorktreeSyncResult,
    ) {
        let Some(ssh_host) = project.ssh_host.as_deref() else {
            if let Err(error) = self.hooks.install_hooks(worktree_path, task_id, None) {
                result.errors.push(format!(
                    "{}/{branch_name} hooks 설정 실패: {error}",
                    project.name
                ));
            }
            return;
        };
        let board = Arc::clone(&self.board);
        let failure_label = format!("{}/{branch_name}", project.name);
        self.hooks.schedule_hooks_install(
            worktree_path,
            task_id,
            Some(ssh_host),
            Box::new(move || board.broadcast_board_update()),
            Box::new(move |error| {
                if !is_remote_connection_error(error) {
                    eprintln!("{failure_label} hooks 백그라운드 설정 실패: {error}");
                }
            }),
        );
    }

    /// 메인 브랜치 task에 세션 기록이 없고 브랜치 이름의 세션이 살아 있으면 자동 연결한다
    fn attach_alive_main_branch_session(
        &self,
        project: &Project,
        result: &mut RegisteredProjectWorktreeSyncResult,
    ) -> ServiceResult<()> {
        let ssh_host = project.ssh_host.as_deref();
        let main_task = records::project_main_branch_task(
            &self.database,
            &project.id,
            &project.default_branch,
        )?;
        let Some(mut main_task) = main_task.filter(|task| task.session_type.is_none()) else {
            return Ok(());
        };
        let session_name =
            format_project_branch_session_name(&project.repo_path, &project.default_branch);
        let Some(alive_session) = self.find_alive_branch_session(&session_name, ssh_host) else {
            return Ok(());
        };
        let worktree_path = self.resolve_default_branch_worktree_path(project);
        let state_path = worktree_path
            .clone()
            .unwrap_or_else(|| project.repo_path.clone());
        let persisted_status = self
            .state_files
            .read_task_status(Some(&state_path), ssh_host)
            .map_err(ServiceError::new)?;
        main_task.session_type = Some(alive_session.session_type);
        main_task.session_name = Some(alive_session.session_name);
        main_task.worktree_path = worktree_path;
        main_task.ssh_host = project.ssh_host.clone();
        if let Some(status) = persisted_status {
            main_task.status = status;
        }
        let saved = records::save_task(&self.database, &main_task)?;
        self.state_files
            .persist_task_state(Some(&state_path), &saved.id, saved.status, ssh_host);
        result.has_changed = true;
        Ok(())
    }
}

/// 새로 등록한 worktree를 결과에 남긴다
fn record_registered_worktree(
    project: &Project,
    worktree: &WorktreeEntry,
    task_id: String,
    result: &mut RegisteredProjectWorktreeSyncResult,
) {
    result.worktree_tasks.push(worktree.branch.clone());
    result
        .registered_worktrees
        .push(BackgroundSyncRegisteredWorktreePayload {
            task_id,
            project_name: project.name.clone(),
            branch_name: worktree.branch.clone(),
            worktree_path: worktree.path.clone(),
            ssh_host: project.ssh_host.clone(),
        });
}

/// task.json에 description 키가 있으면 그 값(`null` 포함)
fn shared_description(shared: &KanvibeTaskSyncState) -> Option<Option<String>> {
    shared
        .description
        .as_ref()
        .map(|description| description.description.clone().flatten())
}

fn apply_worktree_shared_fields(task: &mut KanbanTask, shared: &KanvibeTaskSyncState) {
    if let Some(status) = shared.status {
        task.status = status;
    }
    if let Some(description) = shared_description(shared) {
        task.description = description;
    }
    if let Some(priority) = shared.priority {
        task.priority = priority;
    }
}
