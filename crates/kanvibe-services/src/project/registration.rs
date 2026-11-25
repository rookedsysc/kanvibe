//! 프로젝트 등록·삭제·일괄 스캔. 등록하면 기본 브랜치 task를 함께 만들고, 실패하면 등록을 되돌린다.

use std::collections::HashSet;
use std::sync::Arc;

use kanvibe_ai::session_listing::map_with_concurrency;
use kanvibe_db::repositories::projects::{NewProject, insert_project};
use kanvibe_domain::notifications::BackgroundSyncRegisteredWorktreePayload;
use kanvibe_domain::project::{
    DUPLICATE_PROJECT_NAME_ERROR, Project, posix_basename, posix_dirname,
    resolve_unique_project_name,
};
use kanvibe_process::project_color::compute_project_color;
use serde::Serialize;

use super::{
    ProjectService, RegisteredProjectWorktreeSyncResult, is_remote_connection_error,
    normalize_host, project_path_key, records,
};
use crate::error::ServiceResult;

/// 프로젝트 하나를 등록한 결과(TS `{ success, error?, project? }`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRegistrationResult {
    #[serde(rename = "success")]
    pub is_success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<Project>,
}

impl ProjectRegistrationResult {
    fn failure(error: impl Into<String>) -> Self {
        Self {
            is_success: false,
            error: Some(error.into()),
            project: None,
        }
    }
}

/// 일괄 스캔 결과(TS `ScanResult`)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub registered: Vec<String>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
    pub worktree_tasks: Vec<String>,
    pub registered_worktrees: Vec<BackgroundSyncRegisteredWorktreePayload>,
    pub hooks_setup: Vec<String>,
}

impl ScanResult {
    fn merge(&mut self, next: RegisteredProjectWorktreeSyncResult) {
        self.errors.extend(next.errors);
        self.worktree_tasks.extend(next.worktree_tasks);
        self.registered_worktrees.extend(next.registered_worktrees);
        for name in next.hooks_setup {
            if !self.hooks_setup.contains(&name) {
                self.hooks_setup.push(name);
            }
        }
    }
}

/// 프로젝트 이름 중복은 같은 PC 안에서만 따지므로 대상 PC에 등록된 이름만 모은다
fn project_names_on_same_host(projects: &[Project], ssh_host: Option<&str>) -> HashSet<String> {
    projects
        .iter()
        .filter(|project| normalize_host(project.ssh_host.as_deref()) == ssh_host)
        .map(|project| project.name.clone())
        .collect()
}

impl ProjectService {
    /// worktree나 submodule 경로라도 git common dir의 상위 경로를 저장소 경로로 쓴다. 판정할 수 없으면 원래 경로다
    fn resolve_common_repo_path(&self, repo_path: &str, ssh_host: Option<&str>) -> String {
        let command = format!(
            "git -C {} rev-parse --path-format=absolute --git-common-dir",
            self.shell_path(repo_path, ssh_host)
        );
        match self.repositories.exec_git(&command, ssh_host) {
            Ok(common_directory) if !common_directory.trim().is_empty() => {
                posix_dirname(common_directory.trim()).to_string()
            }
            _ => repo_path.to_string(),
        }
    }

    fn is_submodule_repo_path(&self, repo_path: &str, ssh_host: Option<&str>) -> bool {
        let command = format!(
            "git -C {} rev-parse --show-superproject-working-tree",
            self.shell_path(repo_path, ssh_host)
        );
        self.repositories
            .exec_git(&command, ssh_host)
            .is_ok_and(|superproject| !superproject.trim().is_empty())
    }

    /// 같은 저장소를 이미 보고 있는 다른 client가 정한 색상을 이어받고, 없으면 이름으로 계산한다
    fn resolve_project_color(
        &self,
        repo_path: &str,
        project_name: &str,
        ssh_host: Option<&str>,
    ) -> String {
        self.state_files
            .read_shared_project_color(repo_path, ssh_host)
            .unwrap_or_else(|| compute_project_color(project_name).to_string())
    }

    fn insert_registered_project(
        &self,
        project_name: &str,
        repo_path: &str,
        default_branch: String,
        ssh_host: Option<&str>,
    ) -> ServiceResult<Project> {
        let color = self.resolve_project_color(repo_path, project_name, ssh_host);
        let project = NewProject {
            name: project_name.to_string(),
            repo_path: repo_path.to_string(),
            default_branch,
            ssh_host: ssh_host.map(str::to_string),
            is_worktree: false,
            color: Some(color),
            icon_data_url: None,
        };
        let saved = insert_project(&self.database.connection(), &project)?;
        self.state_files.persist_project_color(&saved);
        Ok(saved)
    }

    /// 새 프로젝트를 등록한다. git 저장소를 확인하고 기본 브랜치를 감지하며, 아이콘은 등록을 끝낸 뒤 백그라운드로 채운다
    pub fn register_project(
        self: &Arc<Self>,
        name: &str,
        repo_path: &str,
        ssh_host: Option<&str>,
    ) -> ServiceResult<ProjectRegistrationResult> {
        let ssh_host = normalize_host(ssh_host);
        let (normalized_repo_path, existing_projects) =
            match self.check_registration_target(name, repo_path, ssh_host)? {
                Ok(target) => target,
                Err(rejection) => return Ok(rejection),
            };
        let default_branch = self
            .repositories
            .default_branch(&normalized_repo_path, ssh_host);
        let mut names = project_names_on_same_host(&existing_projects, ssh_host);
        let Some(project_name) =
            resolve_unique_project_name(name, &normalized_repo_path, &mut names)
        else {
            return Ok(ProjectRegistrationResult::failure(
                DUPLICATE_PROJECT_NAME_ERROR,
            ));
        };
        let saved = self.insert_registered_project(
            &project_name,
            &normalized_repo_path,
            default_branch,
            ssh_host,
        )?;
        if let Err(error) = self.ensure_project_root_task(&saved, ssh_host.is_none()) {
            records::remove_project(&self.database, &saved.id)?;
            return Ok(ProjectRegistrationResult::failure(error.message));
        }

        self.schedule_project_icon_backfill(saved.clone());
        self.board.broadcast_board_update();
        Ok(ProjectRegistrationResult {
            is_success: true,
            error: None,
            project: Some(saved),
        })
    }

    /// 이름·경로·저장소를 확인하고 git common dir 기준 경로와 기존 프로젝트를 돌려준다. 등록할 수 없으면 거절 결과다
    fn check_registration_target(
        &self,
        name: &str,
        repo_path: &str,
        ssh_host: Option<&str>,
    ) -> ServiceResult<Result<(String, Vec<Project>), ProjectRegistrationResult>> {
        if name.is_empty() || repo_path.is_empty() {
            let rejection = ProjectRegistrationResult::failure("이름과 경로는 필수입니다.");
            return Ok(Err(rejection));
        }
        if !self.repositories.validate_git_repo(repo_path, ssh_host) {
            let rejection = ProjectRegistrationResult::failure("유효한 git 저장소가 아닙니다.");
            return Ok(Err(rejection));
        }
        let normalized_repo_path = self.resolve_common_repo_path(repo_path, ssh_host);
        let existing_projects = records::all_projects(&self.database)?;
        let path_key = project_path_key(&normalized_repo_path, ssh_host);
        let is_already_registered = existing_projects.iter().any(|project| {
            project_path_key(&project.repo_path, project.ssh_host.as_deref()) == path_key
        });
        if is_already_registered {
            let rejection = ProjectRegistrationResult::failure("이미 등록된 프로젝트입니다.");
            return Ok(Err(rejection));
        }
        Ok(Ok((normalized_repo_path, existing_projects)))
    }

    /// GitHub 아이콘 조회는 느린 회선에서 타임아웃까지 기다리므로 등록 응답을 막지 않게 따로 실행한다
    fn schedule_project_icon_backfill(self: &Arc<Self>, project: Project) {
        let service = Arc::clone(self);
        self.spawn_background(move || match service.sync_missing_project_icon(&project) {
            Ok(true) => service.board.broadcast_board_update(),
            Ok(false) => {}
            Err(error) if is_remote_connection_error(&error.message) => {}
            Err(error) => eprintln!("{} 아이콘 백그라운드 확보 실패: {error}", project.name),
        });
    }

    /// 프로젝트와 그 task 레코드를 지운다. git 브랜치와 worktree는 건드리지 않는다
    pub fn delete_project(&self, project_id: &str) -> ServiceResult<bool> {
        self.root_repair_blocks
            .lock()
            .unwrap()
            .deleting
            .insert(project_id.to_string());
        let deleted = records::delete_project_with_tasks(&self.database, project_id);
        self.root_repair_blocks
            .lock()
            .unwrap()
            .deleting
            .remove(project_id);
        if !deleted? {
            return Ok(false);
        }
        self.root_repair_blocks
            .lock()
            .unwrap()
            .deleted
            .insert(project_id.to_string());
        self.board.broadcast_board_update();
        Ok(true)
    }

    /// 스캔한 경로 가운데 submodule을 빼고 git common dir 기준으로 겹치지 않게 모은다
    fn discover_registerable_repo_paths(
        &self,
        root_path: &str,
        ssh_host: Option<&str>,
    ) -> Vec<String> {
        let discovered = self.repositories.scan_git_repos(root_path, ssh_host);
        let concurrency = discovered.len().max(1);
        let is_submodule = map_with_concurrency(&discovered, concurrency, |repo_path, _| {
            self.is_submodule_repo_path(repo_path, ssh_host)
        });
        let registerable: Vec<&String> = discovered
            .iter()
            .zip(is_submodule)
            .filter(|(_, is_submodule)| !is_submodule)
            .map(|(repo_path, _)| repo_path)
            .collect();
        let common_paths = map_with_concurrency(&registerable, concurrency, |repo_path, _| {
            self.resolve_common_repo_path(repo_path, ssh_host)
        });
        let mut repo_paths: Vec<String> = Vec::new();
        for repo_path in common_paths {
            if !repo_paths.contains(&repo_path) {
                repo_paths.push(repo_path);
            }
        }
        repo_paths
    }

    /// 하위 git 저장소를 스캔해 미등록 프로젝트를 일괄 등록하고, 이번 스캔에서 찾은 저장소의 worktree를 동기화한다
    pub fn scan_and_register_projects(
        &self,
        root_path: &str,
        ssh_host: Option<&str>,
    ) -> ServiceResult<ScanResult> {
        let ssh_host = normalize_host(ssh_host);
        let mut result = ScanResult::default();
        let repo_paths = self.discover_registerable_repo_paths(root_path, ssh_host);
        if repo_paths.is_empty() {
            return Ok(result);
        }
        let scanned_keys: HashSet<String> = repo_paths
            .iter()
            .map(|repo_path| project_path_key(repo_path, ssh_host))
            .collect();
        let existing = records::all_projects(&self.database)?;
        let mut existing_keys: HashSet<String> = existing
            .iter()
            .map(|project| project_path_key(&project.repo_path, project.ssh_host.as_deref()))
            .collect();
        let mut names = project_names_on_same_host(&existing, ssh_host);
        for repo_path in &repo_paths {
            let path_key = project_path_key(repo_path, ssh_host);
            if existing_keys.contains(&path_key) {
                result.skipped.push(repo_path.clone());
                continue;
            }
            if self.register_scanned_repo(repo_path, ssh_host, &mut names, &mut result) {
                existing_keys.insert(path_key);
            }
        }

        let scanned_projects: Vec<Project> = records::all_projects(&self.database)?
            .into_iter()
            .filter(|project| {
                scanned_keys.contains(&project_path_key(
                    &project.repo_path,
                    project.ssh_host.as_deref(),
                ))
            })
            .collect();
        result.merge(self.sync_registered_project_worktrees(Some(&scanned_projects))?);
        self.board.broadcast_board_update();
        Ok(result)
    }

    /// 스캔한 저장소 하나를 등록한다. 기본 브랜치 task를 만들지 못하면 등록을 되돌린다. 등록했으면 `true`
    fn register_scanned_repo(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
        names: &mut HashSet<String>,
        result: &mut ScanResult,
    ) -> bool {
        let default_branch = self.repositories.default_branch(repo_path, ssh_host);
        let Some(project_name) =
            resolve_unique_project_name(posix_basename(repo_path), repo_path, names)
        else {
            result
                .errors
                .push(format!("{repo_path}: {DUPLICATE_PROJECT_NAME_ERROR}"));
            return false;
        };
        let saved = match self.insert_registered_project(
            &project_name,
            repo_path,
            default_branch,
            ssh_host,
        ) {
            Ok(saved) => saved,
            Err(error) => {
                result
                    .errors
                    .push(format!("{repo_path}: {}", error.message));
                return false;
            }
        };
        self.finish_scanned_registration(&saved, project_name, result)
    }

    /// 기본 브랜치 task를 준비해 등록을 마친다. 실패하면 방금 저장한 프로젝트를 되돌린다
    fn finish_scanned_registration(
        &self,
        saved: &Project,
        project_name: String,
        result: &mut ScanResult,
    ) -> bool {
        let should_repair_hooks = saved.ssh_host.is_none();
        match self.ensure_project_root_task(saved, should_repair_hooks) {
            Ok(repair) => {
                result.registered.push(project_name.clone());
                // 기본 브랜치 작업이 준비되면 hooks가 보장된다
                if repair.task.is_some() && should_repair_hooks {
                    result.hooks_setup.push(project_name);
                }
                true
            }
            Err(error) => {
                let _ = records::remove_project(&self.database, &saved.id);
                eprintln!("{project_name} 기본 브랜치 태스크 생성 실패: {error}");
                result.errors.push(format!(
                    "{project_name} 기본 브랜치 태스크 생성 실패: {}",
                    error.message
                ));
                false
            }
        }
    }
}
