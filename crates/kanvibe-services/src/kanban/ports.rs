//! 보드 서비스가 바깥 계층에 맡기는 일. TS 테스트가 `vi.mock`으로 바꾸던 경계마다 좁은 trait를 두고,
//! 실제 앱은 kanvibe-process의 구현을 감싼 어댑터를 넘긴다.

use std::sync::Arc;

use kanvibe_db::Database;
use kanvibe_db::repositories::pane_layouts::{find_global_pane_layout, find_project_pane_layout};
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType};
use kanvibe_process::hooks::installer::{HookInstallScheduleOptions, KanvibeHooksInstaller};
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::kanvibe_state_store::{KanvibeStateStore, ProjectRecords};
use kanvibe_process::project_color_service::ProjectColorService;
use kanvibe_process::task_state_service::TaskStateService;
use kanvibe_process::task_target_propagation::{PeerTargetResult, TaskTargetPropagation};
use kanvibe_process::worktree::{
    PaneLayoutSelection, ResourceCleanupOptions, WorktreeSession, create_session_without_worktree,
    create_worktree_with_session, is_session_alive, remove_session_only,
    remove_worktree_and_branch,
};

use crate::error::{ServiceError, ServiceResult};
use crate::terminal_ports::TaskTerminalSessions;

/// worktree·멀티플렉서 세션의 생성·정리·생존 확인(TS `lib/worktree.ts`)
pub trait TaskWorktrees: Send + Sync {
    /// git worktree를 만들고 브랜치별 세션 이름을 정한다. `project_id`는 pane 레이아웃을 고르는 데 쓴다
    fn create_worktree_with_session(
        &self,
        project_path: &str,
        branch_name: &str,
        base_branch: &str,
        session_type: SessionType,
        ssh_host: Option<&str>,
        project_id: Option<&str>,
    ) -> ServiceResult<WorktreeSession>;

    /// worktree 없이 기존 디렉터리에서 쓸 세션 이름을 정한다
    fn create_session_without_worktree(
        &self,
        project_path: &str,
        branch_name: &str,
        session_type: SessionType,
        ssh_host: Option<&str>,
        working_directory: &str,
        project_id: Option<&str>,
    ) -> ServiceResult<String>;

    fn remove_worktree_and_branch(
        &self,
        project_path: &str,
        branch_name: &str,
        ssh_host: Option<&str>,
        options: &ResourceCleanupOptions,
    ) -> ServiceResult<()>;

    fn remove_session_only(
        &self,
        session_type: SessionType,
        session_name: &str,
        ssh_host: Option<&str>,
        should_throw_on_error: bool,
    ) -> ServiceResult<()>;

    fn is_session_alive(
        &self,
        session_type: SessionType,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> bool;
}

/// 예약 검증이 끝난 뒤 부르는 callback
pub type HookVerificationSuccess = Box<dyn FnOnce() + Send>;
pub type HookVerificationFailure = Box<dyn FnOnce(String) + Send>;

/// worktree에 KanVibe hook 파일을 쓰고 provider 검증을 예약한다(TS `kanvibeHooksInstaller`)
pub trait TaskHookInstallation: Send + Sync {
    fn install_hook_files(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), String>;

    fn schedule_hooks_verification(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
        on_success: HookVerificationSuccess,
        on_failure: HookVerificationFailure,
    );
}

/// task·프로젝트 상태를 저장소의 `.kanvibe/` 공유 파일에 남긴다. 실패는 구현이 로그로만 남긴다
pub trait KanvibeStateFiles: Send + Sync {
    fn persist_task_state(&self, task: &KanbanTask);
    fn persist_task_metadata(&self, task: &KanbanTask);
    fn persist_task_session(&self, task: &KanbanTask);
    fn persist_project_color(&self, project: &Project);
}

/// 같은 worktree를 공유하는 다른 KanVibe client에 task 삭제를 전파한다
pub trait TaskPeerPropagation: Send + Sync {
    fn propagate_task_deletion_to_peers(
        &self,
        worktree_path: &str,
        ssh_host: Option<&str>,
    ) -> Vec<PeerTargetResult>;
}

/// kanvibe-process worktree 함수를 쓰는 실제 구현. pane 레이아웃은 프로젝트 값이 없으면 전역 값을 쓴다
pub struct ProcessTaskWorktrees {
    files: Arc<HostFileAccess>,
    terminal: Arc<dyn TaskTerminalSessions>,
    database: Arc<Database>,
}

impl ProcessTaskWorktrees {
    pub fn new(
        files: Arc<HostFileAccess>,
        terminal: Arc<dyn TaskTerminalSessions>,
        database: Arc<Database>,
    ) -> Self {
        Self {
            files,
            terminal,
            database,
        }
    }

    /// TS `getEffectivePaneLayout(projectId)`
    fn effective_pane_layout(
        &self,
        project_id: Option<&str>,
    ) -> Result<Option<PaneLayoutSelection>, String> {
        let connection = self.database.connection();
        let project_layout = match project_id {
            Some(project_id) => find_project_pane_layout(&connection, project_id)
                .map_err(|error| error.to_string())?,
            None => None,
        };
        let layout = match project_layout {
            Some(layout) => Some(layout),
            None => find_global_pane_layout(&connection).map_err(|error| error.to_string())?,
        };
        Ok(layout.map(|layout| PaneLayoutSelection {
            layout_type: layout.layout_type,
            panes: layout.panes,
        }))
    }
}

impl TaskWorktrees for ProcessTaskWorktrees {
    fn create_worktree_with_session(
        &self,
        project_path: &str,
        branch_name: &str,
        base_branch: &str,
        session_type: SessionType,
        ssh_host: Option<&str>,
        project_id: Option<&str>,
    ) -> ServiceResult<WorktreeSession> {
        let pane_layout = || self.effective_pane_layout(project_id);
        Ok(create_worktree_with_session(
            &self.files,
            project_path,
            branch_name,
            base_branch,
            session_type,
            ssh_host,
            &pane_layout,
        )?)
    }

    fn create_session_without_worktree(
        &self,
        project_path: &str,
        branch_name: &str,
        session_type: SessionType,
        ssh_host: Option<&str>,
        working_directory: &str,
        project_id: Option<&str>,
    ) -> ServiceResult<String> {
        let pane_layout = || self.effective_pane_layout(project_id);
        Ok(create_session_without_worktree(
            &self.files,
            project_path,
            branch_name,
            session_type,
            ssh_host,
            Some(working_directory),
            &pane_layout,
        ))
    }

    fn remove_worktree_and_branch(
        &self,
        project_path: &str,
        branch_name: &str,
        ssh_host: Option<&str>,
        options: &ResourceCleanupOptions,
    ) -> ServiceResult<()> {
        remove_worktree_and_branch(&self.files, project_path, branch_name, ssh_host, options)
            .map_err(|error| ServiceError::new(error.to_string()))
    }

    fn remove_session_only(
        &self,
        session_type: SessionType,
        session_name: &str,
        ssh_host: Option<&str>,
        should_throw_on_error: bool,
    ) -> ServiceResult<()> {
        let options = ResourceCleanupOptions {
            should_throw_on_error,
            worktree_path: None,
        };
        remove_session_only(
            self.files.runner(),
            self.terminal.as_ref(),
            session_type,
            session_name,
            ssh_host,
            &options,
        )
        .map_err(|error| ServiceError::new(error.to_string()))
    }

    fn is_session_alive(
        &self,
        session_type: SessionType,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> bool {
        is_session_alive(
            self.files.runner(),
            self.terminal.as_ref(),
            session_type,
            session_name,
            ssh_host,
        )
    }
}

/// `KanvibeHooksInstaller`를 쓰는 실제 hook 설치. 검증은 지연 없이 바로 예약한다(TS 기본 `delayMs` 0)
pub struct ProcessTaskHookInstallation {
    installer: Arc<KanvibeHooksInstaller>,
}

impl ProcessTaskHookInstallation {
    pub fn new(installer: Arc<KanvibeHooksInstaller>) -> Self {
        Self { installer }
    }
}

impl TaskHookInstallation for ProcessTaskHookInstallation {
    fn install_hook_files(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), String> {
        self.installer
            .install_hook_files(target_path, task_id, ssh_host)
            .map_err(|error| error.to_string())
    }

    fn schedule_hooks_verification(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
        on_success: HookVerificationSuccess,
        on_failure: HookVerificationFailure,
    ) {
        let options = HookInstallScheduleOptions {
            delay: std::time::Duration::ZERO,
            on_success: Some(on_success),
            on_failure: Some(Box::new(move |error| on_failure(error.to_string()))),
        };
        // 검증 스레드는 스스로 끝나며, 결과는 callback으로만 받는다
        let _verification =
            self.installer
                .schedule_hooks_verification(target_path, task_id, ssh_host, options);
    }
}

/// kanvibe-process `TaskStateService`·`ProjectColorService`로 공유 파일을 쓰는 실제 구현.
/// exclude 확인 캐시를 앱 수명 동안 유지하도록 한 번 만들어 나눠 쓴다
pub struct KanvibeStateWriters<'a> {
    task_state: TaskStateService<'a>,
    project_color: ProjectColorService<'a>,
}

impl<'a> KanvibeStateWriters<'a> {
    pub fn new(state_store: &'a dyn KanvibeStateStore, records: &'a dyn ProjectRecords) -> Self {
        Self {
            task_state: TaskStateService::new(state_store, records),
            project_color: ProjectColorService::new(state_store, records),
        }
    }
}

impl KanvibeStateFiles for KanvibeStateWriters<'_> {
    fn persist_task_state(&self, task: &KanbanTask) {
        self.task_state.persist_task_state_for_task(task);
    }

    fn persist_task_metadata(&self, task: &KanbanTask) {
        self.task_state.persist_task_metadata_for_task(task);
    }

    fn persist_task_session(&self, task: &KanbanTask) {
        self.task_state.persist_task_session_for_task(task);
    }

    fn persist_project_color(&self, project: &Project) {
        self.project_color
            .persist_project_color_to_kanvibe_state(project);
    }
}

impl TaskPeerPropagation for TaskTargetPropagation<'_> {
    fn propagate_task_deletion_to_peers(
        &self,
        worktree_path: &str,
        ssh_host: Option<&str>,
    ) -> Vec<PeerTargetResult> {
        TaskTargetPropagation::propagate_task_deletion_to_peers(self, worktree_path, ssh_host)
    }
}
