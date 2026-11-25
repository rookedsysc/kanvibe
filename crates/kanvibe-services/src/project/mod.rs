//! 프로젝트 서비스(TS `src/desktop/main/services/projectService.ts`). 프로젝트 등록·삭제·스캔, 등록된 프로젝트의
//! worktree↔task 동기화, provider별 hook 설치·상태, task의 AI 세션 조회를 맡는다.
//!
//! git·파일·세션·hook·AI 기록은 모두 `ports`의 trait으로 받는다. 앱은 `adapters`의 실제 구현을 넘긴다.
//! TS 모듈 전역이던 동기화 작업 공유·호스트 직렬화·삭제 중 프로젝트 목록은 서비스 필드가 갖는다.

pub mod adapters;
mod ai_sessions;
mod hooks;
pub mod ports;
mod records;
mod registration;
mod root_task;
mod worktree_jobs;
mod worktree_sync;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use kanvibe_db::Database;
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_domain::notifications::BackgroundSyncRegisteredWorktreePayload;
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::project::{Project, posix_basename};
use kanvibe_domain::task::SessionType;
use kanvibe_process::git_operations::resolve_path_for_shell;
use serde::Serialize;

use self::ports::{
    BranchSessions, ProjectHookOperations, ProjectIconSource, ProjectRepositoryProbe,
    ProjectStateFiles, TaskAiSessionReader,
};
use self::worktree_jobs::WorktreeSyncJobs;
use crate::board_notifier::BoardNotifier;
use crate::error::ServiceResult;

pub use self::hooks::HookInstallResult;
pub use self::registration::{ProjectRegistrationResult, ScanResult};

/// 등록된 프로젝트 worktree 동기화 결과(TS `RegisteredProjectWorktreeSyncResult`)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredProjectWorktreeSyncResult {
    pub worktree_tasks: Vec<String>,
    pub registered_worktrees: Vec<BackgroundSyncRegisteredWorktreePayload>,
    pub hooks_setup: Vec<String>,
    pub errors: Vec<String>,
    #[serde(rename = "changed")]
    pub has_changed: bool,
}

impl RegisteredProjectWorktreeSyncResult {
    /// TS `mergeRegisteredProjectWorktreeSyncResult`. hooks 설정 이름은 겹치지 않게 더한다
    fn merge(&mut self, next: RegisteredProjectWorktreeSyncResult) {
        self.has_changed = self.has_changed || next.has_changed;
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

/// 프로젝트 서비스가 받는 협력 객체
pub struct ProjectServiceParts {
    pub database: Arc<Database>,
    pub board: Arc<BoardNotifier>,
    pub repositories: Arc<dyn ProjectRepositoryProbe>,
    pub sessions: Arc<dyn BranchSessions>,
    pub hooks: Arc<dyn ProjectHookOperations>,
    pub state_files: Arc<dyn ProjectStateFiles>,
    pub icons: Arc<dyn ProjectIconSource>,
    pub ai_sessions: Arc<dyn TaskAiSessionReader>,
}

/// 삭제 중이거나 지운 프로젝트. 그 사이 끝난 동기화가 기본 브랜치 task를 되살리지 않게 막는다
#[derive(Default)]
struct RootRepairBlocks {
    deleting: HashSet<String>,
    deleted: HashSet<String>,
}

pub struct ProjectService {
    database: Arc<Database>,
    board: Arc<BoardNotifier>,
    repositories: Arc<dyn ProjectRepositoryProbe>,
    sessions: Arc<dyn BranchSessions>,
    hooks: Arc<dyn ProjectHookOperations>,
    state_files: Arc<dyn ProjectStateFiles>,
    icons: Arc<dyn ProjectIconSource>,
    ai_sessions: Arc<dyn TaskAiSessionReader>,
    worktree_jobs: WorktreeSyncJobs,
    root_repair_blocks: Mutex<RootRepairBlocks>,
    /// 등록 뒤 백그라운드로 도는 아이콘 확보. 테스트가 끝나기를 기다릴 수 있게 남긴다
    background_jobs: Mutex<Vec<JoinHandle<()>>>,
}

/// `${sshHost || ""}:${repoPath}`
fn project_path_key(repo_path: &str, ssh_host: Option<&str>) -> String {
    format!("{}:{repo_path}", ssh_host.unwrap_or_default())
}

/// 빈 문자열 호스트는 로컬이다(TS의 falsy `sshHost`)
fn normalize_host(ssh_host: Option<&str>) -> Option<&str> {
    ssh_host.filter(|host| !host.is_empty())
}

/// TS `isRemoteConnectionError`. 원격 연결이 끊겨 생긴 실패는 로그를 남기지 않는다
fn is_remote_connection_error(message: &str) -> bool {
    static PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)원격 명령 실패:.*(Connection (?:reset|closed)|kex_exchange_identification|operation timed out|no route to host|connection refused|could not resolve hostname|broken pipe)")
            .expect("유효한 정규식")
    });
    PATTERN.is_match(message)
}

impl ProjectService {
    pub fn new(parts: ProjectServiceParts) -> Arc<Self> {
        Arc::new(Self {
            database: parts.database,
            board: parts.board,
            repositories: parts.repositories,
            sessions: parts.sessions,
            hooks: parts.hooks,
            state_files: parts.state_files,
            icons: parts.icons,
            ai_sessions: parts.ai_sessions,
            worktree_jobs: WorktreeSyncJobs::default(),
            root_repair_blocks: Mutex::new(RootRepairBlocks::default()),
            background_jobs: Mutex::new(Vec::new()),
        })
    }

    fn default_session_type(&self) -> ServiceResult<SessionType> {
        let settings = AppSettingsService::new(&self.database, ShortcutPlatform::current());
        Ok(settings.get_default_session_type()?)
    }

    fn is_root_repair_blocked(&self, project_id: &str) -> bool {
        let blocks = self.root_repair_blocks.lock().unwrap();
        blocks.deleting.contains(project_id) || blocks.deleted.contains(project_id)
    }

    fn shell_path(&self, target_path: &str, ssh_host: Option<&str>) -> String {
        resolve_path_for_shell(
            target_path,
            ssh_host,
            self.repositories.local_home_directory(),
        )
    }

    /// 등록 순서대로 모든 프로젝트. 경로 확인이나 복구는 하지 않는다
    pub fn get_all_projects(&self) -> ServiceResult<Vec<Project>> {
        records::all_projects(&self.database)
    }

    pub fn get_available_hosts(&self) -> Vec<String> {
        self.repositories.available_hosts()
    }

    pub fn get_project_by_id(&self, project_id: &str) -> ServiceResult<Option<Project>> {
        records::project_by_id(&self.database, project_id)
    }

    /// 지정 디렉터리의 직속 하위 디렉터리 이름(fzf drill-down 탐색용). 숨김 폴더는 뺀다
    pub fn list_subdirectories(&self, parent_path: &str, ssh_host: Option<&str>) -> Vec<String> {
        let ssh_host = normalize_host(ssh_host);
        let resolved_path = self.shell_path(parent_path, ssh_host);
        let command =
            format!("find {resolved_path} -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort");
        match self.repositories.exec_git(&command, ssh_host) {
            Ok(output) => output
                .split('\n')
                .filter(|line| !line.is_empty())
                .map(|directory| posix_basename(directory).to_string())
                .filter(|name| !name.starts_with('.'))
                .collect(),
            Err(error) => {
                eprintln!(
                    "[remote-scan] subdirectory scan failed {{ sshHost: {ssh_host:?}, parentPath: {parent_path:?}, resolvedPath: {resolved_path:?}, command: {command:?}, error: {error:?} }}"
                );
                Vec::new()
            }
        }
    }

    /// 프로젝트 브랜치 목록. 원격은 화면을 막는 fetch 없이 조회한다
    pub fn get_project_branches(&self, project_id: &str) -> ServiceResult<Vec<String>> {
        let Some(project) = records::project_by_id(&self.database, project_id)? else {
            return Ok(Vec::new());
        };
        let ssh_host = normalize_host(project.ssh_host.as_deref());
        self.repositories
            .list_branches(&project.repo_path, ssh_host, ssh_host.is_none())
            .map_err(crate::error::ServiceError::new)
    }

    /// 등록 직후의 아이콘 확보처럼 기다리는 화면이 없는 작업을 띄운다
    fn spawn_background(&self, job: impl FnOnce() + Send + 'static) {
        let handle = std::thread::spawn(job);
        let mut jobs = self.background_jobs.lock().unwrap();
        jobs.retain(|existing| !existing.is_finished());
        jobs.push(handle);
    }

    #[cfg(test)]
    fn join_background_jobs(&self) {
        let jobs = std::mem::take(&mut *self.background_jobs.lock().unwrap());
        for job in jobs {
            job.join().expect("백그라운드 작업");
        }
    }
}
