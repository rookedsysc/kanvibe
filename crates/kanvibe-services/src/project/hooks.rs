//! 프로젝트·task의 provider별 hook 상태 조회와 재설치. 조회는 root task를 읽기만 하고 복구하지 않는다.
//! TS의 provider별 함수(`getTaskHooksStatus`, `installTaskGeminiHooks` 등)는 provider 인자 하나로 묶었다.

use kanvibe_domain::project::Project;
use kanvibe_domain::task::KanbanTask;
use kanvibe_process::hooks::providers::{
    KanvibeHookProvider, KanvibeHookStatus, KanvibeHookStatuses,
};

use super::{ProjectService, records};
use crate::error::{ServiceError, ServiceResult};

/// hook 재설치 결과(TS `{ success, error?, status? }`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookInstallResult {
    pub is_success: bool,
    pub error: Option<String>,
    pub status: Option<KanvibeHookStatus>,
}

impl HookInstallResult {
    fn failure(error: impl Into<String>) -> Self {
        Self {
            is_success: false,
            error: Some(error.into()),
            status: None,
        }
    }
}

/// hook을 설치·조회할 위치와 묶을 task
struct HookTarget {
    target_path: String,
    task_id: String,
    ssh_host: Option<String>,
}

/// TS는 OpenCode 설치 함수만 영어 오류 문구를 쓴다
struct InstallMessages {
    project_not_found: &'static str,
    task_not_found: &'static str,
    default_task_not_found: &'static str,
}

fn install_messages(provider: KanvibeHookProvider) -> InstallMessages {
    match provider {
        KanvibeHookProvider::OpenCode => InstallMessages {
            project_not_found: "Project not found",
            task_not_found: "Task or project not found",
            default_task_not_found: "Default branch task not found",
        },
        _ => InstallMessages {
            project_not_found: "프로젝트를 찾을 수 없습니다.",
            task_not_found: "프로젝트를 찾을 수 없습니다.",
            default_task_not_found: "기본 브랜치 태스크를 찾을 수 없습니다.",
        },
    }
}

/// 브랜치가 있으면 기본 브랜치와 같은지, 없으면 기준 브랜치가 비었거나 기본 브랜치인지
fn is_default_branch_task(task: &KanbanTask, default_branch: &str) -> bool {
    match task
        .branch_name
        .as_deref()
        .filter(|branch| !branch.is_empty())
    {
        Some(branch_name) => branch_name == default_branch,
        None => task
            .base_branch
            .as_deref()
            .is_none_or(|base| base.is_empty() || base == default_branch),
    }
}

impl ProjectService {
    /// task와 그 프로젝트. 프로젝트가 없는 task는 `None`이다
    pub(super) fn task_with_project(
        &self,
        task_id: &str,
    ) -> ServiceResult<Option<(KanbanTask, Project)>> {
        let Some(task) = records::task_by_id(&self.database, task_id)? else {
            return Ok(None);
        };
        let Some(project_id) = task.project_id.as_deref() else {
            return Ok(None);
        };
        Ok(records::project_by_id(&self.database, project_id)?.map(|project| (task, project)))
    }

    /// 프로젝트 루트에서 기본 브랜치로 도는 task는 현재 root task에 hook을 묶는다.
    /// 조회(`is_read_only`)는 root task를 읽기만 하고, 설치는 없으면 만든다
    fn resolve_task_hook_target(
        &self,
        task: &KanbanTask,
        project: &Project,
        is_read_only: bool,
    ) -> ServiceResult<HookTarget> {
        let target_path = task
            .worktree_path
            .clone()
            .filter(|path| !path.is_empty())
            .unwrap_or_else(|| project.repo_path.clone());
        let is_project_root = target_path == project.repo_path
            && is_default_branch_task(task, &project.default_branch);
        let root_task = match (is_project_root, is_read_only) {
            (false, _) => None,
            (true, true) => self.project_root_task(project)?,
            (true, false) => self.ensure_project_root_task(project, false)?.task,
        };
        Ok(HookTarget {
            target_path,
            task_id: root_task.map_or_else(|| task.id.clone(), |root| root.id),
            ssh_host: project.ssh_host.clone(),
        })
    }

    /// 프로젝트 루트의 provider hook 상태. 기존 root task만 읽고 복구하지 않는다
    pub fn get_project_hooks_status(
        &self,
        project_id: &str,
        provider: KanvibeHookProvider,
    ) -> ServiceResult<Option<KanvibeHookStatus>> {
        let Some(project) = records::project_by_id(&self.database, project_id)? else {
            return Ok(None);
        };
        let root_task = self.project_root_task(&project)?;
        self.hooks
            .provider_status(
                provider,
                &project.repo_path,
                root_task.as_ref().map(|task| task.id.as_str()),
                project.ssh_host.as_deref(),
            )
            .map(Some)
            .map_err(ServiceError::new)
    }

    /// 프로젝트 루트에 provider hook 하나를 다시 설치한다. root task가 없으면 먼저 만든다
    pub fn install_project_hooks(
        &self,
        project_id: &str,
        provider: KanvibeHookProvider,
    ) -> ServiceResult<HookInstallResult> {
        let messages = install_messages(provider);
        let Some(project) = records::project_by_id(&self.database, project_id)? else {
            return Ok(HookInstallResult::failure(messages.project_not_found));
        };
        let Some(task) = self.ensure_project_root_task(&project, false)?.task else {
            return Ok(HookInstallResult::failure(messages.default_task_not_found));
        };
        let target = HookTarget {
            target_path: project.repo_path.clone(),
            task_id: task.id,
            ssh_host: project.ssh_host.clone(),
        };
        Ok(self.install_provider_at(&target, provider))
    }

    fn install_provider_at(
        &self,
        target: &HookTarget,
        provider: KanvibeHookProvider,
    ) -> HookInstallResult {
        let ssh_host = target.ssh_host.as_deref();
        let installed = self
            .hooks
            .install_hook_provider(&target.target_path, &target.task_id, provider, ssh_host)
            .and_then(|()| {
                self.hooks.provider_status(
                    provider,
                    &target.target_path,
                    Some(&target.task_id),
                    ssh_host,
                )
            });
        match installed {
            Ok(status) => HookInstallResult {
                is_success: true,
                error: None,
                status: Some(status),
            },
            Err(error) => HookInstallResult::failure(error),
        }
    }

    /// task가 쓰는 worktree(또는 프로젝트 루트)의 네 provider 상태를 한 번에 읽는다
    pub fn get_task_hooks_statuses(
        &self,
        task_id: &str,
    ) -> ServiceResult<Option<KanvibeHookStatuses>> {
        let Some((task, project)) = self.task_with_project(task_id)? else {
            return Ok(None);
        };
        let target = self.resolve_task_hook_target(&task, &project, true)?;
        self.hooks
            .hooks_statuses(
                &target.target_path,
                &target.task_id,
                target.ssh_host.as_deref(),
            )
            .map(Some)
            .map_err(ServiceError::new)
    }

    /// task의 provider hook 상태. 조회 실패는 성공 상태로 감추지 않고 오류로 돌려준다
    pub fn get_task_hooks_status(
        &self,
        task_id: &str,
        provider: KanvibeHookProvider,
    ) -> ServiceResult<Option<KanvibeHookStatus>> {
        let Some((task, project)) = self.task_with_project(task_id)? else {
            return Ok(None);
        };
        let target = self.resolve_task_hook_target(&task, &project, true)?;
        self.hooks
            .provider_status(
                provider,
                &target.target_path,
                Some(&target.task_id),
                target.ssh_host.as_deref(),
            )
            .map(Some)
            .map_err(ServiceError::new)
    }

    /// task의 worktree(또는 프로젝트 루트)에 provider hook 하나만 다시 설치한다
    pub fn install_task_hooks(
        &self,
        task_id: &str,
        provider: KanvibeHookProvider,
    ) -> ServiceResult<HookInstallResult> {
        let messages = install_messages(provider);
        let Some((task, project)) = self.task_with_project(task_id)? else {
            return Ok(HookInstallResult::failure(messages.task_not_found));
        };
        match self.resolve_task_hook_target(&task, &project, false) {
            Ok(target) => Ok(self.install_provider_at(&target, provider)),
            Err(error) => Ok(HookInstallResult::failure(error.message)),
        }
    }
}
