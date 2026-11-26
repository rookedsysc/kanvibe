//! task 생성·수정·컬럼 이동·브랜치 분기와 프로젝트 색상 변경.

use kanvibe_db::repositories::tasks::{NewKanbanTask, insert_task, update_task};
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskStatus};
use kanvibe_process::exec::ExecGitOptions;
use kanvibe_process::host_file_access::quote_shell_argument;

use super::hook_preparation::{HookTargetTask, install_task_hook_files_immediately};
use super::records::{
    TaskResourceFields, all_projects, find_project_by_id, find_task_by_id,
    restore_task_resource_fields, update_project_colors, update_task_resource_fields,
    update_task_status,
};
use super::{CreateTaskInput, KanbanService, TaskUpdate};
use crate::error::ServiceResult;

/// 빈 문자열은 값이 없는 것으로 본다(TS `value || null`)
pub(super) fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|text| !text.is_empty())
}

pub(super) fn hook_target(task: &KanbanTask) -> HookTargetTask {
    HookTargetTask {
        id: task.id.clone(),
        title: task.title.clone(),
        ssh_host: task.ssh_host.clone(),
    }
}

fn new_task_from_input(input: &CreateTaskInput) -> NewKanbanTask {
    let title = non_empty(Some(&input.title))
        .or(non_empty(input.branch_name.as_deref()))
        .unwrap_or("Untitled");
    let to_owned =
        |value: Option<&String>| non_empty(value.map(String::as_str)).map(str::to_string);
    NewKanbanTask {
        title: title.to_string(),
        description: to_owned(input.description.as_ref()),
        status: TaskStatus::Todo,
        branch_name: to_owned(input.branch_name.as_ref()),
        worktree_path: None,
        session_type: input.session_type,
        session_name: None,
        ssh_host: to_owned(input.ssh_host.as_ref()),
        agent_type: None,
        project_id: to_owned(input.project_id.as_ref()),
        base_branch: to_owned(input.base_branch.as_ref()),
        pr_url: None,
        priority: input.priority,
    }
}

impl KanbanService {
    /// task를 저장하고 DB가 정한 수정 시각까지 담아 다시 읽는다
    pub(super) fn save_task(&self, task: &KanbanTask) -> ServiceResult<KanbanTask> {
        update_task(&self.database.connection(), task)?;
        Ok(find_task_by_id(&self.database, &task.id)?.unwrap_or_else(|| task.clone()))
    }

    /// 브랜치·세션 종류·프로젝트가 모두 있으면 worktree와 세션 이름을 정해 새 task에 채운다
    fn prepare_new_task_worktree(
        &self,
        input: &CreateTaskInput,
        new_task: &mut NewKanbanTask,
    ) -> ServiceResult<Option<String>> {
        let branch_name = non_empty(input.branch_name.as_deref());
        let project_id = non_empty(input.project_id.as_deref());
        let (Some(branch_name), Some(session_type), Some(project_id)) =
            (branch_name, input.session_type, project_id)
        else {
            return Ok(None);
        };
        let Some(project) = find_project_by_id(&self.database, project_id)? else {
            return Ok(None);
        };
        let base_branch =
            non_empty(input.base_branch.as_deref()).unwrap_or(&project.default_branch);
        let session = self.worktrees.create_worktree_with_session(
            &project.repo_path,
            branch_name,
            base_branch,
            session_type,
            project.ssh_host.as_deref(),
            Some(project_id),
        )?;
        new_task.worktree_path = Some(session.worktree_path.clone());
        new_task.session_name = Some(session.session_name);
        new_task.ssh_host = project.ssh_host.clone();
        Ok(non_empty(Some(&session.worktree_path)).map(str::to_string))
    }

    /// 새 작업을 만든다. 브랜치와 프로젝트가 있으면 worktree와 세션도 함께 만든다.
    /// worktree를 만들지 못하면 task도 남기지 않는다(이 경로에서는 늘 프로젝트가 지정돼 있다).
    pub fn create_task(&self, input: &CreateTaskInput) -> ServiceResult<KanbanTask> {
        let mut new_task = new_task_from_input(input);
        let hook_target_path = self
            .prepare_new_task_worktree(input, &mut new_task)
            .inspect_err(|error| eprintln!("Worktree/세션 생성 실패: {error}"))?;
        let saved = insert_task(&self.database.connection(), &new_task)?;
        self.prepare_created_task(&saved, hook_target_path.as_deref());
        self.board.broadcast_board_update();
        Ok(saved)
    }

    /// hook 설치와 상태·메타데이터 기록을 함께 진행하고 모두 끝날 때까지 기다린다
    fn prepare_created_task(&self, saved: &KanbanTask, hook_target_path: Option<&str>) {
        std::thread::scope(|scope| {
            if let Some(target_path) = hook_target_path {
                scope.spawn(move || {
                    install_task_hook_files_immediately(
                        self.hooks.as_ref(),
                        &self.board,
                        target_path,
                        &hook_target(saved),
                        "새 태스크 hooks 동기 설치 실패",
                    );
                });
            }
            scope.spawn(|| self.state_files.persist_task_state(saved));
            if saved.description.is_some() || saved.priority.is_some() {
                scope.spawn(|| self.state_files.persist_task_metadata(saved));
            }
        });
    }

    /// 작업 상태를 바꾼다
    pub fn update_task_status(
        &self,
        task_id: &str,
        new_status: TaskStatus,
    ) -> ServiceResult<Option<KanbanTask>> {
        let Some(mut task) = find_task_by_id(&self.database, task_id)? else {
            return Ok(None);
        };
        // 상태가 같으면 DB에 쓰지 않는다. 쓰면 수정 시각이 올라 보드 정렬 순서가 바뀐다
        let saved = if task.status == new_status {
            task
        } else {
            task.status = new_status;
            self.save_task(&task)?
        };
        self.state_files.persist_task_state(&saved);
        self.board.broadcast_board_update();
        Ok(Some(saved))
    }

    /// 작업 제목·설명·priority를 부분 수정한다. 설명이나 priority가 바뀌면 공유 파일에도 남긴다
    pub fn update_task(
        &self,
        task_id: &str,
        updates: &TaskUpdate,
    ) -> ServiceResult<Option<KanbanTask>> {
        let Some(mut task) = find_task_by_id(&self.database, task_id)? else {
            return Ok(None);
        };
        if let Some(title) = &updates.title {
            task.title = title.clone();
        }
        if let Some(description) = &updates.description {
            task.description = description.clone();
        }
        if let Some(priority) = updates.priority {
            task.priority = priority;
        }
        let saved = self.save_task(&task)?;
        if updates.description.is_some() || updates.priority.is_some() {
            self.state_files.persist_task_metadata(&saved);
        }
        self.board.broadcast_board_update();
        Ok(Some(saved))
    }

    /// 기존 작업에서 브랜치를 분기한다. worktree와 세션을 만들고 상태를 progress로 바꾼다
    pub fn branch_from_task(
        &self,
        task_id: &str,
        project_id: &str,
        base_branch: &str,
        branch_name: &str,
        session_type: SessionType,
    ) -> ServiceResult<Option<KanbanTask>> {
        let Some(mut task) = find_task_by_id(&self.database, task_id)? else {
            return Ok(None);
        };
        let Some(project) = find_project_by_id(&self.database, project_id)? else {
            return Ok(None);
        };
        let session = self.worktrees.create_worktree_with_session(
            &project.repo_path,
            branch_name,
            base_branch,
            session_type,
            project.ssh_host.as_deref(),
            Some(project_id),
        )?;
        task.project_id = Some(project_id.to_string());
        task.branch_name = Some(branch_name.to_string());
        task.base_branch = Some(base_branch.to_string());
        task.session_type = Some(session_type);
        task.session_name = Some(session.session_name);
        task.worktree_path = Some(session.worktree_path.clone());
        task.ssh_host = project.ssh_host.clone();
        task.status = TaskStatus::Progress;
        let saved = self.save_task(&task)?;
        if !session.worktree_path.is_empty() {
            let target = hook_target(&saved);
            install_task_hook_files_immediately(
                self.hooks.as_ref(),
                &self.board,
                &session.worktree_path,
                &target,
                "Hooks 설정 실패",
            );
        }
        self.state_files.persist_task_state(&saved);
        self.board.broadcast_board_update();
        Ok(Some(saved))
    }
}

impl KanbanService {
    /// task를 다른 컬럼으로 옮긴다. 목적지 안의 자리는 저장하지 않는다(보드 순서는 정렬 기준과 최근 수정순으로만 정해진다).
    /// Done으로 옮기다 저장이 실패하면 이전 상태와 리소스 필드로 되돌린 뒤 오류를 돌려준다
    pub fn move_task_to_column(&self, task_id: &str, new_status: TaskStatus) -> ServiceResult<()> {
        let task = find_task_by_id(&self.database, task_id)?;
        let Err(error) = self.save_column_move(task_id, new_status, task.as_ref()) else {
            return Ok(());
        };
        if let (TaskStatus::Done, Some(task)) = (new_status, task.as_ref()) {
            restore_task_resource_fields(&self.database, task)?;
            self.state_files.persist_task_state(task);
            self.board.broadcast_board_update();
        }
        Err(error)
    }

    fn save_column_move(
        &self,
        task_id: &str,
        new_status: TaskStatus,
        task: Option<&KanbanTask>,
    ) -> ServiceResult<()> {
        match (new_status, task) {
            (TaskStatus::Done, Some(task)) => {
                let fields = TaskResourceFields {
                    status: new_status,
                    session_type: task.session_type,
                    session_name: task.session_name.as_deref(),
                    worktree_path: task.worktree_path.as_deref(),
                };
                update_task_resource_fields(&self.database, task_id, &fields)?;
            }
            (TaskStatus::Done, None) => {}
            _ => {
                update_task_status(&self.database, task_id, new_status)?;
            }
        }
        if let Some(task) = task {
            let moved = KanbanTask {
                status: new_status,
                ..task.clone()
            };
            self.state_files.persist_task_state(&moved);
        }
        self.board.broadcast_board_update();
        Ok(())
    }
}

impl KanbanService {
    /// 프로젝트 색상을 바꾸고, 같은 호스트에서 git common directory를 공유하는 worktree 프로젝트에도 반영한다.
    /// 색상의 권위는 저장소 루트의 `.kanvibe/project.json`이므로 DB보다 먼저 기록한다. 순서가 뒤집히면
    /// 그 사이 도는 background sync가 아직 이전 색상인 공유 파일을 읽어 방금 고른 색을 DB에서 되돌린다.
    pub fn update_project_color(&self, project_id: &str, color: &str) -> ServiceResult<()> {
        let Some(project) = find_project_by_id(&self.database, project_id)? else {
            return Ok(());
        };
        let common_directory = self.read_project_git_common_directory(&project);
        let related_projects = match common_directory.as_deref() {
            Some(directory) => self.find_projects_sharing_git_directory(&project, directory)?,
            None => Vec::new(),
        };
        let main_repo_path = common_directory
            .as_deref()
            .and_then(|directory| directory.strip_suffix("/.git"))
            .unwrap_or(&project.repo_path)
            .to_string();
        let mut recolored_projects = vec![project];
        recolored_projects.extend(related_projects);
        let recolored_projects = order_project_repo_root_first(recolored_projects, &main_repo_path);
        let mut recolored_ids = Vec::with_capacity(recolored_projects.len());
        for mut recolored in recolored_projects {
            recolored.color = Some(color.to_string());
            self.state_files.persist_project_color(&recolored);
            recolored_ids.push(recolored.id);
        }
        update_project_colors(&self.database, &recolored_ids, color)?;
        self.board.broadcast_board_update();
        Ok(())
    }

    fn find_projects_sharing_git_directory(
        &self,
        project: &Project,
        common_directory: &str,
    ) -> ServiceResult<Vec<Project>> {
        let mut related_projects = Vec::new();
        for candidate in all_projects(&self.database)? {
            if candidate.id == project.id || candidate.ssh_host != project.ssh_host {
                continue;
            }
            if self
                .read_project_git_common_directory(&candidate)
                .as_deref()
                == Some(common_directory)
            {
                related_projects.push(candidate);
            }
        }
        Ok(related_projects)
    }

    fn read_project_git_common_directory(&self, project: &Project) -> Option<String> {
        let command = format!(
            "git -C {} rev-parse --path-format=absolute --git-common-dir",
            quote_shell_argument(&project.repo_path)
        );
        let directory = self
            .files
            .runner()
            .exec_git(
                &command,
                project.ssh_host.as_deref(),
                &ExecGitOptions::default(),
            )
            .ok()?;
        let directory = directory.trim();
        (!directory.is_empty()).then(|| directory.to_string())
    }
}

/// 저장소 루트를 보는 프로젝트를 맨 앞으로 옮긴다. 색상의 권위 파일이 루트에 있어 루트부터 확정해야 한다
fn order_project_repo_root_first(mut projects: Vec<Project>, main_repo_path: &str) -> Vec<Project> {
    let root_index = projects
        .iter()
        .position(|project| project.repo_path == main_repo_path);
    if let Some(root_index) = root_index.filter(|&index| index > 0) {
        let root = projects.remove(root_index);
        projects.insert(0, root);
    }
    projects
}
