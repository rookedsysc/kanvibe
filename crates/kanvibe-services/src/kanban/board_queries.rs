//! 보드·검색·상세 화면이 읽는 task 조회.

use std::collections::HashMap;

use kanvibe_domain::project::Project;
use kanvibe_domain::task::KanbanTask;

use super::records::{
    count_done_tasks, find_project_by_id, find_task_by_id, find_task_id_by_project_and_branch,
    select_tasks,
};
use super::{
    DONE_PAGE_SIZE, KanbanService, LoadMoreDoneResponse, SearchableTask, TaskWithProject,
    TasksByStatus, TasksByStatusWithMeta,
};
use crate::error::ServiceResult;

impl KanbanService {
    /// 모든 작업을 상태별로 묶는다. Done 컬럼은 첫 페이지만 읽는다
    pub fn get_tasks_by_status(&self) -> ServiceResult<TasksByStatusWithMeta> {
        let non_done_tasks = select_tasks(
            &self.database,
            "WHERE status != 'done' ORDER BY updated_at DESC, rowid ASC",
            &[],
        )?;
        let done_page = self.get_more_done_tasks(0, DONE_PAGE_SIZE)?;
        let mut tasks = TasksByStatus {
            done: done_page.tasks,
            ..TasksByStatus::default()
        };
        for task in non_done_tasks {
            tasks.column_mut(task.status).push(task);
        }
        Ok(TasksByStatusWithMeta {
            tasks,
            done_total: done_page.done_total,
            done_limit: DONE_PAGE_SIZE,
        })
    }

    /// Done 작업을 더 불러온다
    pub fn get_more_done_tasks(
        &self,
        offset: usize,
        limit: usize,
    ) -> ServiceResult<LoadMoreDoneResponse> {
        let (limit, offset) = (limit as i64, offset as i64);
        let tasks = select_tasks(
            &self.database,
            "WHERE status = 'done' ORDER BY updated_at DESC, rowid ASC \
             LIMIT ?1 OFFSET ?2",
            &[&limit, &offset],
        )?;
        let done_total = count_done_tasks(&self.database)?;
        Ok(LoadMoreDoneResponse { tasks, done_total })
    }

    /// 작업 하나와 연결된 프로젝트
    pub fn get_task_by_id(&self, task_id: &str) -> ServiceResult<Option<TaskWithProject>> {
        let Some(task) = find_task_by_id(&self.database, task_id)? else {
            return Ok(None);
        };
        let project = match task.project_id.as_deref() {
            Some(project_id) => find_project_by_id(&self.database, project_id)?,
            None => None,
        };
        Ok(Some(TaskWithProject { task, project }))
    }

    /// 빠른 검색 오버레이가 쓰는 목록. Done도 포함한다
    pub fn get_searchable_tasks(&self) -> ServiceResult<Vec<SearchableTask>> {
        let tasks = select_tasks(
            &self.database,
            "ORDER BY updated_at DESC, created_at DESC, rowid ASC",
            &[],
        )?;
        let mut projects: HashMap<String, Option<Project>> = HashMap::new();
        let mut searchable_tasks = Vec::with_capacity(tasks.len());
        for task in tasks {
            let project = self.cached_project(&mut projects, &task)?;
            searchable_tasks.push(build_searchable_task(task, project.as_ref()));
        }
        Ok(searchable_tasks)
    }

    fn cached_project(
        &self,
        projects: &mut HashMap<String, Option<Project>>,
        task: &KanbanTask,
    ) -> ServiceResult<Option<Project>> {
        let Some(project_id) = task.project_id.as_deref() else {
            return Ok(None);
        };
        if let Some(project) = projects.get(project_id) {
            return Ok(project.clone());
        }
        let project = find_project_by_id(&self.database, project_id)?;
        projects.insert(project_id.to_string(), project.clone());
        Ok(project)
    }

    /// 같은 프로젝트에서 브랜치가 같은 task의 id
    pub fn get_task_id_by_project_and_branch(
        &self,
        project_id: &str,
        branch_name: &str,
    ) -> ServiceResult<Option<String>> {
        Ok(find_task_id_by_project_and_branch(
            &self.database,
            project_id,
            branch_name,
        )?)
    }
}

fn build_searchable_task(task: KanbanTask, project: Option<&Project>) -> SearchableTask {
    let ssh_host = task
        .ssh_host
        .clone()
        .or_else(|| project.and_then(|project| project.ssh_host.clone()));
    SearchableTask {
        id: task.id,
        title: task.title,
        branch_name: task.branch_name,
        project_id: task.project_id,
        project_name: project.map(|project| project.name.clone()),
        ssh_host,
        status: task.status,
        updated_at: task.updated_at,
    }
}
