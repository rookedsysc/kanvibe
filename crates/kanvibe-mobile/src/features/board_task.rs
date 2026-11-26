//! 태스크 검색·Done 페이지·설명 수정·브랜치 연결·PR 갱신과 프로젝트 색·원격 폴더·삭제(TS `mobileBoardTaskService`).

use kanvibe_domain::task::{KanbanTask, SessionType, TaskPriority};
use kanvibe_process::git_ref::is_safe_git_ref;
use kanvibe_services::kanban::{LoadMoreDoneResponse, SearchableTask, TaskUpdate, TaskWithProject};
use kanvibe_services::project::ScanResult;
use serde_json::{Map, Value};

use super::MobileFeatures;
use super::validation::{
    DEFAULT_TEXT_MAX_LENGTH, is_safe_integer, read_mobile_enum, read_mobile_object,
    read_mobile_text,
};
use crate::error::{MobileError, MobileResult};

const MAX_DESCRIPTION_LENGTH: usize = 16_000;
const MAX_DONE_PAGE_LIMIT: f64 = 100.0;
/// 설명은 문자열(1만6천 자 이하, NUL 없음)이나 `null`, 우선순위는 열거값이나 `null`만 받는다
fn read_task_details(body: &Map<String, Value>) -> MobileResult<TaskUpdate> {
    let mut updates = TaskUpdate::default();
    if let Some(description) = body.get("description") {
        updates.description = Some(read_description(description)?);
    }
    if let Some(priority) = body.get("priority") {
        updates.priority = Some(match priority {
            Value::Null => None,
            _ => Some(read_mobile_enum(
                TaskPriority::parse,
                Some(priority),
                "우선순위",
            )?),
        });
    }
    let has_only_known_keys = body
        .keys()
        .all(|key| key == "description" || key == "priority");
    let has_updates = updates.description.is_some() || updates.priority.is_some();
    if !has_updates || !has_only_known_keys {
        return Err(MobileError::feature(
            400,
            "설명 또는 우선순위만 변경할 수 있습니다",
        ));
    }
    Ok(updates)
}

fn read_description(value: &Value) -> MobileResult<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::String(text)
            if kanvibe_ai::javascript::utf16_length(text) <= MAX_DESCRIPTION_LENGTH
                && !text.contains('\0') =>
        {
            Ok(Some(text.clone()))
        }
        _ => Err(MobileError::feature(400, "설명 값이 올바르지 않습니다")),
    }
}

fn is_hex_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

/// 브랜치 연결 요청. 브랜치 두 칸은 셸과 git이 다르게 읽을 문자가 없어야 한다
struct BranchRequest {
    project_id: String,
    branch_name: String,
    base_branch: String,
    session_type: SessionType,
}

fn read_branch_request(input: &Value) -> MobileResult<BranchRequest> {
    let body = read_mobile_object(input)?;
    let request = BranchRequest {
        project_id: read_mobile_text(body.get("projectId"), "프로젝트", 128)?,
        branch_name: read_mobile_text(body.get("branchName"), "브랜치", DEFAULT_TEXT_MAX_LENGTH)?,
        base_branch: read_mobile_text(
            body.get("baseBranch"),
            "베이스 브랜치",
            DEFAULT_TEXT_MAX_LENGTH,
        )?,
        session_type: read_mobile_enum(SessionType::parse, body.get("sessionType"), "세션 종류")?,
    };
    if !is_safe_git_ref(&request.branch_name) || !is_safe_git_ref(&request.base_branch) {
        return Err(MobileError::feature(400, "브랜치 값이 올바르지 않습니다"));
    }
    Ok(request)
}

impl MobileFeatures {
    pub fn get_mobile_searchable_tasks(&self) -> MobileResult<Vec<SearchableTask>> {
        Ok(self.tasks.get_searchable_tasks()?)
    }

    pub fn get_mobile_done_tasks(
        &self,
        offset: f64,
        limit: f64,
    ) -> MobileResult<LoadMoreDoneResponse> {
        let is_valid = is_safe_integer(offset)
            && offset >= 0.0
            && is_safe_integer(limit)
            && (1.0..=MAX_DONE_PAGE_LIMIT).contains(&limit);
        if !is_valid {
            return Err(MobileError::feature(400, "페이지 값이 올바르지 않습니다"));
        }
        Ok(self
            .tasks
            .get_more_done_tasks(offset as usize, limit as usize)?)
    }

    pub fn update_mobile_task_details(
        &self,
        task_id: &str,
        input: &Value,
    ) -> MobileResult<KanbanTask> {
        let updates = read_task_details(read_mobile_object(input)?)?;
        self.require_mobile_task(task_id)?;
        self.tasks
            .update_task(task_id, &updates)?
            .ok_or_else(|| MobileError::feature(404, "없는 태스크입니다"))
    }

    pub fn update_mobile_project_color(
        &self,
        project_id: &str,
        input: &Value,
    ) -> MobileResult<String> {
        let body = read_mobile_object(input)?;
        let Some(color) = body
            .get("color")
            .and_then(Value::as_str)
            .filter(|color| is_hex_color(color))
        else {
            return Err(MobileError::feature(400, "색상 값이 올바르지 않습니다"));
        };
        self.require_mobile_project(project_id)?;
        self.tasks.update_project_color(project_id, color)?;
        Ok(color.to_string())
    }

    pub fn get_mobile_project_tasks(&self, project_id: &str) -> MobileResult<Vec<SearchableTask>> {
        self.require_mobile_project(project_id)?;
        let tasks = self.tasks.get_searchable_tasks()?;
        Ok(tasks
            .into_iter()
            .filter(|task| task.project_id.as_deref() == Some(project_id))
            .collect())
    }

    pub fn get_mobile_project_branch_task(
        &self,
        project_id: &str,
        submitted_branch: Option<&str>,
    ) -> MobileResult<Option<TaskWithProject>> {
        let branch_value = submitted_branch.map(|branch| Value::String(branch.to_string()));
        let branch = read_mobile_text(branch_value.as_ref(), "브랜치", DEFAULT_TEXT_MAX_LENGTH)?;
        self.require_mobile_project(project_id)?;
        match self
            .tasks
            .get_task_id_by_project_and_branch(project_id, &branch)?
        {
            Some(task_id) => Ok(self.tasks.get_task_by_id(&task_id)?),
            None => Ok(None),
        }
    }

    /// 이미 있는 태스크에 브랜치를 연결한다. 같은 태스크의 연결이 겹치면 두 번째는 409로 거절한다
    pub fn branch_mobile_task(&self, task_id: &str, input: &Value) -> MobileResult<KanbanTask> {
        let request = read_branch_request(input)?;
        if !self.lock_branching_tasks().insert(task_id.to_string()) {
            return Err(MobileError::feature(409, "이미 브랜치를 연결하고 있습니다"));
        }
        let result = self.connect_task_branch(task_id, &request);
        self.lock_branching_tasks().remove(task_id);
        result
    }

    fn lock_branching_tasks(&self) -> std::sync::MutexGuard<'_, std::collections::HashSet<String>> {
        self.branching_task_ids
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn connect_task_branch(
        &self,
        task_id: &str,
        request: &BranchRequest,
    ) -> MobileResult<KanbanTask> {
        let task = self.require_mobile_task(task_id)?;
        if task
            .task
            .branch_name
            .as_deref()
            .is_some_and(|branch| !branch.is_empty())
        {
            return Err(MobileError::feature(
                409,
                "이미 브랜치가 연결된 태스크입니다",
            ));
        }
        let project = self.require_mobile_project(&request.project_id)?;
        if project.is_worktree {
            return Err(MobileError::feature(400, "원본 프로젝트를 선택해 주세요"));
        }
        let dependency = self
            .session_dependencies
            .get_session_dependency_status(request.session_type, project.ssh_host.as_deref())?;
        if !dependency.is_available {
            return Err(MobileError::feature(
                409,
                "데스크탑에서 세션 도구를 설치해 주세요",
            ));
        }
        self.tasks
            .branch_from_task(
                task_id,
                &request.project_id,
                &request.base_branch,
                &request.branch_name,
                request.session_type,
            )?
            .ok_or_else(|| MobileError::feature(404, "없는 태스크입니다"))
    }

    pub fn refresh_mobile_task_pr(&self, task_id: &str) -> MobileResult<Option<String>> {
        let task = self.require_mobile_task(task_id)?;
        if task.task.branch_name.as_deref().is_none_or(str::is_empty) {
            return Err(MobileError::feature(409, "브랜치가 없는 태스크입니다"));
        }
        Ok(self.tasks.fetch_and_save_pr_url(task_id)?)
    }

    pub fn get_mobile_hosts(&self) -> Vec<String> {
        self.projects.get_available_hosts()
    }

    /// 기기가 고른 호스트가 SSH 설정에 있는 별칭일 때만 받는다. 아무 별칭이나 받으면 임의의 호스트에 접속하게 된다
    fn read_mobile_ssh_host(&self, input: Option<&Value>) -> MobileResult<String> {
        let ssh_host = read_mobile_text(input, "SSH 호스트", 256)?;
        if !self.projects.get_available_hosts().contains(&ssh_host) {
            return Err(MobileError::feature(
                400,
                "등록된 SSH 호스트를 선택해 주세요",
            ));
        }
        Ok(ssh_host)
    }

    pub fn get_mobile_remote_directories(
        &self,
        parent_path: &str,
        ssh_host: &Value,
    ) -> MobileResult<Vec<String>> {
        let ssh_host = self.read_mobile_ssh_host(Some(ssh_host))?;
        let path_value = Value::String(parent_path.to_string());
        let parent_path =
            read_mobile_text(Some(&path_value), "폴더 경로", DEFAULT_TEXT_MAX_LENGTH)?;
        Ok(self
            .projects
            .list_subdirectories(&parent_path, Some(&ssh_host)))
    }

    pub fn scan_mobile_remote_projects(
        &self,
        root_path: &str,
        ssh_host: &Value,
    ) -> MobileResult<ScanResult> {
        let ssh_host = self.read_mobile_ssh_host(Some(ssh_host))?;
        let path_value = Value::String(root_path.to_string());
        let root_path = read_mobile_text(Some(&path_value), "폴더 경로", DEFAULT_TEXT_MAX_LENGTH)?;
        Ok(self
            .projects
            .scan_and_register_projects(&root_path, Some(&ssh_host))?)
    }

    pub fn delete_mobile_project(&self, project_id: &str) -> MobileResult<()> {
        self.require_mobile_project(project_id)?;
        if !self.projects.delete_project(project_id)? {
            return Err(MobileError::feature(404, "없는 프로젝트입니다"));
        }
        Ok(())
    }
}
