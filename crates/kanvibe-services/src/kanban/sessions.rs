//! task 터미널 세션 연결과 세션 종류 변경.
//! 세션 종류를 바꿀 때는 새 세션을 만든 뒤에만 기존 세션을 닫아, 전환이 실패해도 작업 중이던 세션은 남는다.

use kanvibe_ai::session_listing::map_with_concurrency;
use kanvibe_domain::task::{KanbanTask, SessionType};

use super::hook_preparation::HookTargetTask;
use super::records::{find_project_by_id, find_task_by_id, select_tasks};
use super::task_editing::non_empty;
use super::{KanbanService, SessionTypeChangeResult, SessionTypeChangeTargets};
use crate::error::{ServiceError, ServiceResult};

/// 일괄 변경에서 동시에 전환하는 task 수. 원격 호스트에 SSH 명령이 한꺼번에 몰리지 않게 한다
const SESSION_TYPE_CHANGE_CONCURRENCY: usize = 4;

fn has_session(task: &KanbanTask) -> bool {
    task.session_type.is_some() && non_empty(task.session_name.as_deref()).is_some()
}

impl KanbanService {
    /// 세션이 없는 task에 터미널 세션을 붙인다. worktree는 만들지 않고 프로젝트 루트나 worktree 경로에서 연다.
    /// 화면이 세션 없는 task를 자동으로 연결하므로, 여러 창이 함께 요청해도 세션은 하나만 남긴다
    pub fn connect_terminal_session(
        &self,
        task_id: &str,
        session_type: SessionType,
    ) -> ServiceResult<Option<KanbanTask>> {
        let Some(task) = find_task_by_id(&self.database, task_id)? else {
            return Ok(None);
        };
        if task.project_id.is_none() {
            return Ok(None);
        }
        if has_session(&task) {
            return Ok(Some(task));
        }
        self.create_task_terminal_session(task, session_type)
    }

    /// task에 새 터미널 세션을 만들어 기록한다. 세션이 이미 있는지는 부른 쪽이 정한다
    fn create_task_terminal_session(
        &self,
        task: KanbanTask,
        session_type: SessionType,
    ) -> ServiceResult<Option<KanbanTask>> {
        let Some(project_id) = task.project_id.clone() else {
            return Ok(None);
        };
        let branch_for_session = non_empty(task.branch_name.as_deref())
            .or(non_empty(task.base_branch.as_deref()))
            .map(str::to_string);
        let Some(branch_for_session) = branch_for_session else {
            return Ok(None);
        };
        let Some(project) = find_project_by_id(&self.database, &project_id)? else {
            return Ok(None);
        };
        let working_directory = non_empty(task.worktree_path.as_deref())
            .unwrap_or(&project.repo_path)
            .to_string();
        let attached = self.attach_new_session(
            task,
            &project,
            &branch_for_session,
            &working_directory,
            session_type,
        );
        match attached {
            Ok(saved) => Ok(Some(saved)),
            Err(error) => {
                eprintln!("터미널 세션 생성 실패: {error}");
                Ok(None)
            }
        }
    }

    fn attach_new_session(
        &self,
        mut task: KanbanTask,
        project: &kanvibe_domain::project::Project,
        branch_for_session: &str,
        working_directory: &str,
        session_type: SessionType,
    ) -> ServiceResult<KanbanTask> {
        let hook_target = HookTargetTask {
            id: task.id.clone(),
            title: task.title.clone(),
            ssh_host: project.ssh_host.clone(),
        };
        self.install_task_hook_files_before_terminal_attach(working_directory, hook_target);
        let session_name = self.worktrees.create_session_without_worktree(
            &project.repo_path,
            branch_for_session,
            session_type,
            project.ssh_host.as_deref(),
            working_directory,
            Some(&project.id),
        )?;
        task.session_type = Some(session_type);
        task.session_name = Some(session_name);
        task.worktree_path = Some(working_directory.to_string());
        task.ssh_host = project.ssh_host.clone();
        let saved = self.save_task(&task)?;
        self.state_files.persist_task_state(&saved);
        self.board.broadcast_board_update();
        Ok(saved)
    }

    /// task의 현재 세션이 켜져 있는지. 세션 종류를 바꾸기 전에 닫을지 묻는 데 쓴다
    pub fn is_task_session_alive(&self, task_id: &str) -> ServiceResult<bool> {
        let Some(task) = find_task_by_id(&self.database, task_id)? else {
            return Ok(false);
        };
        let (Some(session_type), Some(session_name)) =
            (task.session_type, non_empty(task.session_name.as_deref()))
        else {
            return Ok(false);
        };
        Ok(self
            .worktrees
            .is_session_alive(session_type, session_name, task.ssh_host.as_deref()))
    }

    /// 세션이 있는 task의 세션 종류를 바꾼다. 닫지 않기로 하면 기존 멀티플렉서 세션은 그대로 두고 KanVibe 연결만 끊는다
    pub fn change_task_session_type(
        &self,
        task_id: &str,
        session_type: SessionType,
        should_close_existing_session: bool,
    ) -> ServiceResult<Option<KanbanTask>> {
        let Some(task) = find_task_by_id(&self.database, task_id)? else {
            return Ok(None);
        };
        let (Some(previous_type), Some(previous_name)) = (
            task.session_type,
            non_empty(task.session_name.as_deref()).map(str::to_string),
        ) else {
            return Ok(None);
        };
        if previous_type == session_type {
            return Ok(Some(task));
        }
        let previous_host = task.ssh_host.clone();
        self.terminal
            .detach_session(&task.id, "change-session-type");
        let Some(changed_task) = self.create_task_terminal_session(task, session_type)? else {
            return Ok(None);
        };
        if should_close_existing_session {
            self.worktrees.remove_session_only(
                previous_type,
                &previous_name,
                previous_host.as_deref(),
                false,
            )?;
        }
        self.state_files.persist_task_session(&changed_task);
        self.board.broadcast_task_session_changed(task_id);
        Ok(Some(changed_task))
    }

    /// 일괄 변경 전에 바뀔 task 수와 그중 켜진 세션 수, 확인할 호스트를 알려 준다
    pub fn get_session_type_change_targets(
        &self,
        session_type: SessionType,
    ) -> ServiceResult<SessionTypeChangeTargets> {
        let target_tasks = self.find_session_type_change_target_tasks(session_type)?;
        let alive_results =
            map_with_concurrency(&target_tasks, SESSION_TYPE_CHANGE_CONCURRENCY, |task, _| {
                let (Some(session_type), Some(session_name)) =
                    (task.session_type, task.session_name.as_deref())
                else {
                    return false;
                };
                self.worktrees.is_session_alive(
                    session_type,
                    session_name,
                    task.ssh_host.as_deref(),
                )
            });
        let mut ssh_hosts: Vec<Option<String>> = Vec::new();
        for task in &target_tasks {
            if !ssh_hosts.contains(&task.ssh_host) {
                ssh_hosts.push(task.ssh_host.clone());
            }
        }
        Ok(SessionTypeChangeTargets {
            task_count: target_tasks.len(),
            live_session_count: alive_results
                .into_iter()
                .filter(|&is_alive| is_alive)
                .count(),
            ssh_hosts,
        })
    }

    /// 세션이 있는 모든 task(Done 포함)를 같은 세션 종류로 바꾼다
    pub fn change_all_task_session_types(
        &self,
        session_type: SessionType,
        should_close_existing_sessions: bool,
    ) -> ServiceResult<SessionTypeChangeResult> {
        let target_tasks = self.find_session_type_change_target_tasks(session_type)?;
        let changed_tasks =
            map_with_concurrency(&target_tasks, SESSION_TYPE_CHANGE_CONCURRENCY, |task, _| {
                self.change_task_session_type(
                    &task.id,
                    session_type,
                    should_close_existing_sessions,
                )
            });
        let changed_tasks = changed_tasks
            .into_iter()
            .collect::<Result<Vec<_>, ServiceError>>()?;
        let changed_count = changed_tasks.iter().filter(|task| task.is_some()).count();
        Ok(SessionTypeChangeResult {
            changed_count,
            failed_count: target_tasks.len() - changed_count,
        })
    }

    /// 세션을 다시 만들려면 프로젝트가 있어야 하므로 프로젝트 없는 task는 대상에서 뺀다
    fn find_session_type_change_target_tasks(
        &self,
        session_type: SessionType,
    ) -> ServiceResult<Vec<KanbanTask>> {
        let tasks_with_session = select_tasks(
            &self.database,
            "WHERE session_name IS NOT NULL ORDER BY rowid ASC",
            &[],
        )?;
        Ok(tasks_with_session
            .into_iter()
            .filter(|task| {
                task.session_type
                    .is_some_and(|current| current != session_type)
                    && task.project_id.is_some()
            })
            .collect())
    }
}
