//! 보드가 직접 부르는 서비스(계획 AD-8). TS 렌더러가 IPC로 부르던 `kanban.*`·`project.*`·`appSettings.*`·알림 호출을
//! 런타임(`DesktopRuntime`)이 가진 `KanbanService`·`DiffService`·`NotificationStore`·`ProjectService`·
//! `BackgroundTaskSync` 호출로 바꾼다. 모든 호출은 QA 고장 주입 판정을 TS IPC 이름으로 먼저 거친다.

use std::collections::BTreeMap;
use std::sync::Arc;

use gpui_kit::{App, AppContext, Task};
use kanvibe_domain::background_sync::BackgroundTaskSyncStatus;
use kanvibe_domain::board_sort::BoardSortPreference;
use kanvibe_domain::notifications::AppNotification;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskStatus};
use kanvibe_domain::task_diff_stats::TaskDiffStats;
use kanvibe_runtime::DesktopRuntime;
use kanvibe_services::kanban::{CreateTaskInput, SearchableTask, TasksByStatus};
use kanvibe_services::project::ScanResult;

use crate::qa::faults::{FaultOutcome, FaultRegistry};
use crate::services::{AppServices, apply_fault, format_service_error_for};

/// 보드 화면 서비스 호출 실패. `reason`은 TS가 화면에 보이던 IPC 오류 문구 그대로이며
/// `common.loadFailed`·`task.deleteFailed` 같은 문구에 그대로 들어간다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardServiceFailure {
    pub reason: String,
}

pub type BoardServiceResult<T> = Result<T, BoardServiceFailure>;

impl BoardServiceFailure {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    /// 서비스가 낸 사유를 TS 렌더러가 받던 문구로 감싼다(`format_service_error_for`)
    fn from_service(target: &str, reason: &str) -> Self {
        Self::new(format_service_error_for(target, reason))
    }
}

/// 보드 첫 조회. TS `BoardRoute`의 `Promise.all` 묶음이 한 덩어리로 성공하거나 실패한다
pub const BOARD_TASKS_SERVICE: &str = "kanban.getTasksByStatus";
pub const MORE_DONE_TASKS_SERVICE: &str = "kanban.getMoreDoneTasks";
pub const MOVE_TASK_SERVICE: &str = "kanban.moveTaskToColumn";
pub const DELETE_TASK_SERVICE: &str = "kanban.deleteTask";
pub const CREATE_TASK_SERVICE: &str = "kanban.createTask";
pub const BRANCH_FROM_TASK_SERVICE: &str = "kanban.branchFromTask";
pub const CONNECT_TERMINAL_SERVICE: &str = "kanban.connectTerminalSession";
pub const DIFF_STATS_SERVICE: &str = "diff.getTaskDiffStats";
pub const PROJECT_BRANCHES_SERVICE: &str = "project.getProjectBranches";
pub const NOTIFICATION_ACTIVATE_SERVICE: &str = "kanvibe:notifications-activate";
pub const NOTIFICATION_MARK_READ_SERVICE: &str = "kanvibe:notifications-mark-read";
pub const SEARCHABLE_TASKS_SERVICE: &str = "kanban.getSearchableTasks";
pub const BACKGROUND_SYNC_STATUS_SERVICE: &str = "backgroundTaskSync.getBackgroundTaskSyncStatus";
pub const BACKGROUND_SYNC_START_SERVICE: &str = "backgroundTaskSync.startBackgroundTaskSyncNow";
pub const PROJECT_SCAN_SERVICE: &str = "project.scanAndRegisterProjects";
pub const PROJECT_DELETE_SERVICE: &str = "project.deleteProject";
pub const LIST_SUBDIRECTORIES_SERVICE: &str = "project.listSubdirectories";

/// AppSettings `board_view_mode`
pub const BOARD_VIEW_MODE_KEY: &str = "board_view_mode";

/// 보드 경로가 그릴 뷰(TS `BoardViewMode`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardViewMode {
    Kanban,
    Workspace,
}

impl BoardViewMode {
    /// 모르는 값과 읽기 실패는 칸반이다
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("workspace") => Self::Workspace,
            _ => Self::Kanban,
        }
    }
}

/// 보드가 처음 그리는 데 필요한 값 전부
#[derive(Debug, Clone, PartialEq)]
pub struct BoardSnapshot {
    pub tasks: TasksByStatus,
    pub done_total: usize,
    pub done_limit: usize,
    pub ssh_hosts: Vec<String>,
    pub projects: Vec<Project>,
    pub is_done_alert_dismissed: bool,
    pub default_session_type: SessionType,
    pub is_vim_mode_enabled: bool,
    pub sort_preference: BoardSortPreference,
    pub diff_stats_by_task_id: BTreeMap<String, TaskDiffStats>,
    pub notifications: Vec<AppNotification>,
    pub is_notification_unread_only: bool,
}

impl BoardSnapshot {
    /// DB 없이 뜬 앱(테스트)이 그리는 빈 보드
    pub fn empty() -> Self {
        Self {
            tasks: TasksByStatus::default(),
            done_total: 0,
            done_limit: kanvibe_services::kanban::DONE_PAGE_SIZE,
            ssh_hosts: Vec::new(),
            projects: Vec::new(),
            is_done_alert_dismissed: false,
            default_session_type: SessionType::Herdr,
            is_vim_mode_enabled: true,
            sort_preference: BoardSortPreference::default(),
            diff_stats_by_task_id: BTreeMap::new(),
            notifications: Vec::new(),
            is_notification_unread_only: false,
        }
    }
}

/// 보드 화면이 쓰는 런타임 서비스와 QA 고장 판정. 보드 뷰마다 만들어도 같은 런타임 인스턴스를 가리킨다
pub struct BoardServices {
    runtime: Arc<DesktopRuntime>,
    faults: Option<Arc<FaultRegistry>>,
}

/// 보드 서비스를 돌려준다. 데이터 디렉터리 없이 뜬 앱이면 `None`이다
pub fn board_services(cx: &App) -> Option<Arc<BoardServices>> {
    let app_services = cx.try_global::<AppServices>()?;
    let runtime = app_services.runtime()?.clone();
    Some(Arc::new(BoardServices {
        runtime,
        faults: app_services.faults().cloned(),
    }))
}

impl BoardServices {
    fn check_fault(&self, target: &str, arguments: &str) -> FaultOutcome {
        self.faults
            .as_ref()
            .map(|faults| faults.check(target, arguments))
            .unwrap_or(FaultOutcome::Proceed)
    }

    /// AppSettings `board_view_mode`. 읽지 못하면 칸반이다
    pub fn board_view_mode(&self) -> BoardViewMode {
        let stored = self
            .runtime
            .app_settings()
            .get_app_setting(BOARD_VIEW_MODE_KEY)
            .ok();
        BoardViewMode::parse(stored.flatten().as_deref())
    }

    fn read_snapshot(&self) -> Result<BoardSnapshot, String> {
        let runtime = &self.runtime;
        let tasks = runtime
            .kanban()
            .get_tasks_by_status()
            .map_err(|error| error.message)?;
        let projects = runtime
            .project()
            .get_all_projects()
            .map_err(|error| error.to_string())?;
        let settings = runtime.app_settings();
        let database_failure = |error: kanvibe_db::DatabaseError| error.to_string();
        let is_done_alert_dismissed = settings
            .get_done_alert_dismissed()
            .map_err(database_failure)?;
        let default_session_type = settings
            .get_default_session_type()
            .map_err(database_failure)?;
        let is_vim_mode_enabled = settings.get_vim_mode_enabled().map_err(database_failure)?;
        let sort_preference = settings
            .get_board_sort_preference()
            .map_err(database_failure)?;
        let is_notification_unread_only = settings
            .get_notification_unread_only_enabled()
            .unwrap_or(false);
        Ok(BoardSnapshot {
            ssh_hosts: runtime.ssh_config().available_hosts(),
            projects,
            is_done_alert_dismissed,
            default_session_type,
            is_vim_mode_enabled,
            sort_preference,
            diff_stats_by_task_id: runtime.diff().get_task_diff_stats(&[]),
            notifications: runtime
                .notification_store()
                .list_notifications()
                .unwrap_or_default(),
            is_notification_unread_only,
            tasks: tasks.tasks,
            done_total: tasks.done_total,
            done_limit: tasks.done_limit,
        })
    }
}

/// 서비스 호출 하나를 고장 판정 뒤 백그라운드에서 실행한다. 실패 사유는 TS IPC 문구로 감싼다
fn spawn_service_call<T: Send + 'static>(
    services: &Arc<BoardServices>,
    target: &'static str,
    arguments: &str,
    cx: &App,
    call: impl FnOnce(&BoardServices) -> Result<T, String> + Send + 'static,
) -> Task<BoardServiceResult<T>> {
    let fault = services.check_fault(target, arguments);
    let executor = cx.background_executor().clone();
    let services = services.clone();
    cx.background_spawn(async move {
        let outcome = match apply_fault(fault, &executor).await {
            Ok(()) => call(&services),
            Err(failure) => Err(failure.reason),
        };
        outcome.map_err(|reason| BoardServiceFailure::from_service(target, &reason))
    })
}

fn json_arguments(arguments: &[&str]) -> String {
    serde_json::to_string(arguments).unwrap_or_default()
}

/// 화면 문구 없이 로그로만 남기는 설정 저장 실패
fn settings_failure(error: kanvibe_db::DatabaseError) -> BoardServiceFailure {
    BoardServiceFailure::new(error.to_string())
}

/// 서비스 오류를 사유 글자로 바꾼다
fn service_failure(error: impl std::fmt::Display) -> String {
    error.to_string()
}

impl BoardServices {
    pub fn load_snapshot(self: &Arc<Self>, cx: &App) -> Task<BoardServiceResult<BoardSnapshot>> {
        spawn_service_call(
            self,
            BOARD_TASKS_SERVICE,
            "[]",
            cx,
            BoardServices::read_snapshot,
        )
    }

    /// 진행 중 카드의 변경 집계를 다시 돌리고, 저장된 집계 전체를 돌려준다(TS `useBoardTaskDiffStats`)
    pub fn load_diff_stats(
        self: &Arc<Self>,
        task_ids_to_refresh: Vec<String>,
        cx: &App,
    ) -> Task<BoardServiceResult<BTreeMap<String, TaskDiffStats>>> {
        let arguments = serde_json::to_string(&task_ids_to_refresh).unwrap_or_default();
        spawn_service_call(self, DIFF_STATS_SERVICE, &arguments, cx, move |services| {
            Ok(services
                .runtime
                .diff()
                .get_task_diff_stats(&task_ids_to_refresh))
        })
    }

    /// 세션이 없는 작업에 기본 세션 타입으로 터미널 세션을 붙인다
    pub fn connect_terminal_session(
        self: &Arc<Self>,
        task_id: String,
        session_type: SessionType,
        cx: &App,
    ) -> Task<BoardServiceResult<Option<KanbanTask>>> {
        let arguments = json_arguments(&[&task_id, session_type.as_str()]);
        spawn_service_call(
            self,
            CONNECT_TERMINAL_SERVICE,
            &arguments,
            cx,
            move |services| {
                services
                    .runtime
                    .kanban()
                    .connect_terminal_session(&task_id, session_type)
                    .map_err(service_failure)
            },
        )
    }

    /// Done 열 다음 페이지
    pub fn load_more_done(
        self: &Arc<Self>,
        offset: usize,
        cx: &App,
    ) -> Task<BoardServiceResult<(Vec<KanbanTask>, usize)>> {
        let arguments = format!("[{offset}]");
        spawn_service_call(
            self,
            MORE_DONE_TASKS_SERVICE,
            &arguments,
            cx,
            move |services| {
                let page = services
                    .runtime
                    .kanban()
                    .get_more_done_tasks(offset, kanvibe_services::kanban::DONE_PAGE_SIZE)
                    .map_err(service_failure)?;
                Ok((page.tasks, page.done_total))
            },
        )
    }

    pub fn move_task_to_column(
        self: &Arc<Self>,
        task_id: String,
        status: TaskStatus,
        cx: &App,
    ) -> Task<BoardServiceResult<()>> {
        let arguments = json_arguments(&[&task_id, status.as_str()]);
        spawn_service_call(self, MOVE_TASK_SERVICE, &arguments, cx, move |services| {
            services
                .runtime
                .kanban()
                .move_task_to_column(&task_id, status)
                .map_err(service_failure)
        })
    }

    /// 지웠으면 `true`. 이미 없는 task면 `false`다
    pub fn delete_task(
        self: &Arc<Self>,
        task_id: String,
        cx: &App,
    ) -> Task<BoardServiceResult<bool>> {
        let arguments = json_arguments(&[&task_id]);
        spawn_service_call(self, DELETE_TASK_SERVICE, &arguments, cx, move |services| {
            services
                .runtime
                .kanban()
                .delete_task(&task_id)
                .map_err(service_failure)
        })
    }

    /// 기존 작업에서 브랜치를 분기한다(worktree·세션을 만들고 진행 중으로 옮긴다)
    pub fn branch_from_task(
        self: &Arc<Self>,
        task_id: String,
        project_id: String,
        base_branch: String,
        branch_name: String,
        session_type: SessionType,
        cx: &App,
    ) -> Task<BoardServiceResult<Option<KanbanTask>>> {
        let arguments = json_arguments(&[&task_id, &project_id, &base_branch, &branch_name]);
        spawn_service_call(
            self,
            BRANCH_FROM_TASK_SERVICE,
            &arguments,
            cx,
            move |services| {
                services
                    .runtime
                    .kanban()
                    .branch_from_task(
                        &task_id,
                        &project_id,
                        &base_branch,
                        &branch_name,
                        session_type,
                    )
                    .map_err(service_failure)
            },
        )
    }

    /// 작업을 만든다. 브랜치·프로젝트가 있으면 worktree와 세션도 서비스가 함께 준비한다
    pub fn create_task(
        self: &Arc<Self>,
        input: CreateTaskInput,
        cx: &App,
    ) -> Task<BoardServiceResult<Option<KanbanTask>>> {
        let arguments = json_arguments(&[&input.title]);
        spawn_service_call(self, CREATE_TASK_SERVICE, &arguments, cx, move |services| {
            services
                .runtime
                .kanban()
                .create_task(&input)
                .map(Some)
                .map_err(service_failure)
        })
    }

    /// 프로젝트 브랜치 목록. 원격 프로젝트는 화면을 막는 fetch 없이 읽는다
    pub fn load_project_branches(
        self: &Arc<Self>,
        project_id: String,
        cx: &App,
    ) -> Task<BoardServiceResult<Vec<String>>> {
        let arguments = json_arguments(&[&project_id]);
        spawn_service_call(
            self,
            PROJECT_BRANCHES_SERVICE,
            &arguments,
            cx,
            move |services| {
                services
                    .runtime
                    .project()
                    .get_project_branches(&project_id)
                    .map_err(service_failure)
            },
        )
    }

    /// Done 이동 안내를 다시 보지 않는다(AppSettings `done_alert_dismissed`)
    pub fn dismiss_done_alert(&self) -> BoardServiceResult<()> {
        self.runtime
            .app_settings()
            .dismiss_done_alert()
            .map_err(settings_failure)
    }

    pub fn save_board_sort_preference(
        &self,
        preference: &BoardSortPreference,
    ) -> BoardServiceResult<()> {
        self.runtime
            .app_settings()
            .set_board_sort_preference(preference)
            .map_err(settings_failure)
    }

    pub fn save_notification_unread_only(&self, is_enabled: bool) -> BoardServiceResult<()> {
        self.runtime
            .app_settings()
            .set_notification_unread_only_enabled(is_enabled)
            .map_err(settings_failure)
    }

    /// 알림 하나를 읽음으로 바꾼다
    pub fn mark_notification_read(
        self: &Arc<Self>,
        notification_id: String,
        cx: &App,
    ) -> Task<BoardServiceResult<()>> {
        let arguments = json_arguments(&[&notification_id]);
        spawn_service_call(
            self,
            NOTIFICATION_MARK_READ_SERVICE,
            &arguments,
            cx,
            move |services| {
                services
                    .runtime
                    .notification_store()
                    .mark_notification_read(&notification_id)
                    .map(|_| ())
                    .map_err(service_failure)
            },
        )
    }

    pub fn mark_all_notifications_read(self: &Arc<Self>, cx: &App) -> Task<BoardServiceResult<()>> {
        spawn_service_call(self, NOTIFICATION_MARK_READ_SERVICE, "[]", cx, |services| {
            services
                .runtime
                .notification_store()
                .mark_all_notifications_read()
                .map_err(service_failure)
        })
    }

    /// 알림을 열기 전에 대상 task가 아직 있는지 본다(TS `kanvibe:notifications-activate`)
    pub fn activate_notification(
        self: &Arc<Self>,
        task_id: Option<String>,
        cx: &App,
    ) -> Task<BoardServiceResult<bool>> {
        let arguments = json_arguments(&[task_id.as_deref().unwrap_or_default()]);
        spawn_service_call(
            self,
            NOTIFICATION_ACTIVATE_SERVICE,
            &arguments,
            cx,
            move |services| match task_id {
                Some(task_id) => services
                    .runtime
                    .kanban()
                    .get_task_by_id(&task_id)
                    .map(|task| task.is_some())
                    .map_err(service_failure),
                None => Ok(true),
            },
        )
    }

    /// 지금 백그라운드 동기화 상태(TS `getBackgroundTaskSyncStatus`)
    pub fn load_background_sync_status(
        self: &Arc<Self>,
        cx: &App,
    ) -> Task<BoardServiceResult<BackgroundTaskSyncStatus>> {
        spawn_service_call(self, BACKGROUND_SYNC_STATUS_SERVICE, "[]", cx, |services| {
            Ok(services
                .runtime
                .background_task_sync()
                .get_background_task_sync_status())
        })
    }

    /// 동기화 한 사이클을 백그라운드에서 시작하고 지금 상태를 돌려준다(TS `startBackgroundTaskSyncNow`)
    pub fn start_background_sync(
        self: &Arc<Self>,
        cx: &App,
    ) -> Task<BoardServiceResult<BackgroundTaskSyncStatus>> {
        spawn_service_call(self, BACKGROUND_SYNC_START_SERVICE, "[]", cx, |services| {
            Ok(services
                .runtime
                .background_task_sync()
                .start_background_task_sync_now())
        })
    }

    /// 경로 아래 git 저장소를 찾아 등록한다(TS `project.scanAndRegisterProjects`)
    pub fn scan_projects(
        self: &Arc<Self>,
        root_path: String,
        ssh_host: Option<String>,
        cx: &App,
    ) -> Task<BoardServiceResult<ScanResult>> {
        let arguments = json_arguments(&[&root_path, ssh_host.as_deref().unwrap_or_default()]);
        spawn_service_call(
            self,
            PROJECT_SCAN_SERVICE,
            &arguments,
            cx,
            move |services| {
                services
                    .runtime
                    .project()
                    .scan_and_register_projects(&root_path, ssh_host.as_deref())
                    .map_err(service_failure)
            },
        )
    }

    /// 등록 프로젝트를 지운다(TS `project.deleteProject`)
    pub fn delete_project(
        self: &Arc<Self>,
        project_id: String,
        cx: &App,
    ) -> Task<BoardServiceResult<bool>> {
        let arguments = json_arguments(&[&project_id]);
        spawn_service_call(
            self,
            PROJECT_DELETE_SERVICE,
            &arguments,
            cx,
            move |services| {
                services
                    .runtime
                    .project()
                    .delete_project(&project_id)
                    .map_err(service_failure)
            },
        )
    }

    /// 폴더 검색칸의 직속 하위 폴더(TS `project.listSubdirectories`)
    pub fn list_subdirectories(
        self: &Arc<Self>,
        parent_path: String,
        ssh_host: Option<String>,
        cx: &App,
    ) -> Task<BoardServiceResult<Vec<String>>> {
        let arguments = json_arguments(&[&parent_path, ssh_host.as_deref().unwrap_or_default()]);
        spawn_service_call(
            self,
            LIST_SUBDIRECTORIES_SERVICE,
            &arguments,
            cx,
            move |services| {
                Ok(services
                    .runtime
                    .project()
                    .list_subdirectories(&parent_path, ssh_host.as_deref()))
            },
        )
    }

    /// 빠른 검색이 거를 작업 목록(TS `getSearchableTasks`)
    pub fn load_searchable_tasks(
        self: &Arc<Self>,
        cx: &App,
    ) -> Task<BoardServiceResult<Vec<SearchableTask>>> {
        spawn_service_call(self, SEARCHABLE_TASKS_SERVICE, "[]", cx, |services| {
            services
                .runtime
                .kanban()
                .get_searchable_tasks()
                .map_err(service_failure)
        })
    }
}
