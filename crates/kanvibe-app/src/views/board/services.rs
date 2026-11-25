//! 보드가 직접 부르는 서비스(계획 AD-8). TS 렌더러가 IPC로 부르던 `kanban.*`·`project.*`·`appSettings.*`·알림 호출을
//! 같은 프로세스의 `KanbanService`·`DiffService`·`NotificationStore`·`AppSettingsService` 호출로 바꾼다.
//!
//! 서비스 묶음은 보드가 처음 열릴 때 한 번 만들어 GPUI 전역(`BoardServices`)에 둔다. 앱 조립 지점이 생기면
//! 그쪽으로 옮길 후보다. 모든 호출은 QA 고장 주입 판정을 TS IPC 이름으로 먼저 거친다.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{App, AppContext, BackgroundExecutor, Global, Task};
use kanvibe_db::Database;
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_db::repositories::projects::list_projects;
use kanvibe_domain::board_sort::BoardSortPreference;
use kanvibe_domain::notifications::AppNotification;
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskStatus};
use kanvibe_domain::task_diff_stats::TaskDiffStats;
use kanvibe_process::exec::system::SystemShellCommandRunner;
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::kanvibe_state_store::HostKanvibeStateStore;
use kanvibe_process::shell_environment::current_user_home_directory;
use kanvibe_process::ssh_config::SshConfigReader;
use kanvibe_services::board_notifier::BoardNotifier;
use kanvibe_services::diff::DiffService;
use kanvibe_services::kanban::ports::{KanvibeStateWriters, ProcessTaskWorktrees};
use kanvibe_services::kanban::{
    CreateTaskInput, KanbanService, KanbanServiceParts, KanbanTimings, TasksByStatus,
};
use kanvibe_services::notification_store::{DatabaseNotificationListStorage, NotificationStore};

use super::adapters::{BoardHookInstallation, BoardPeerPropagation, BoardTerminalSessions};
use crate::qa::faults::{FaultOutcome, FaultRegistry};
use crate::services::AppServices;

/// 보드 화면 서비스 호출 실패. `reason`은 `common.loadFailed`·`task.deleteFailed` 같은 문구에 그대로 들어간다
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
}

/// QA 고장 주입 실패 문구. 기준 이미지는 Electron IPC가 오류 앞에 붙이던 머리말까지 담고 있어서 같은 모양으로 만든다
/// (parity/screens/approximations.md "고장 주입 오류 문구 전체")
/// 전용 IPC 채널(`kanvibe:*`)로 부르던 동작은 그 채널 이름이, 나머지는 범용 `kanvibe:invoke`가 머리말에 들어간다
fn injected_failure_reason(target: &str, message: &str) -> String {
    let channel = if target.starts_with("kanvibe:") {
        target
    } else {
        "kanvibe:invoke"
    };
    format!("Error invoking remote method '{channel}': Error: {message}")
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

/// 보드 서비스 묶음
pub struct BoardServices {
    database: Arc<Database>,
    files: Arc<HostFileAccess>,
    kanban: Arc<KanbanService>,
    diff: Arc<DiffService>,
    notifications: Arc<NotificationStore>,
    ssh_config: Arc<SshConfigReader>,
    faults: Option<Arc<FaultRegistry>>,
    platform: ShortcutPlatform,
}

/// 전역에 한 벌만 둔다. 클론은 같은 서비스를 가리킨다
#[derive(Clone)]
pub struct SharedBoardServices(pub Arc<BoardServices>);

impl Global for SharedBoardServices {}

/// 보드 서비스를 돌려준다. DB 없이 뜬 앱이면 `None`이다. 처음 부를 때 한 번만 만든다
pub fn board_services(cx: &mut App) -> Option<Arc<BoardServices>> {
    if let Some(shared) = cx.try_global::<SharedBoardServices>() {
        return Some(shared.0.clone());
    }
    let app_services = cx.global::<AppServices>();
    let database = app_services.database()?.clone();
    let faults = app_services.faults().cloned();
    let platform = app_services.shortcut_platform;
    let services = Arc::new(BoardServices::compose(database, faults, platform));
    cx.set_global(SharedBoardServices(services.clone()));
    Some(services)
}

/// 상태 저장 파일 기록기는 `&'static` 협력 객체를 받으므로 앱 수명 동안 한 번만 만들어 둔다
fn leak<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}

impl BoardServices {
    fn compose(
        database: Arc<Database>,
        faults: Option<Arc<FaultRegistry>>,
        platform: ShortcutPlatform,
    ) -> Self {
        let home_directory = current_user_home_directory();
        let runner = Arc::new(SystemShellCommandRunner::for_current_process());
        let files = Arc::new(HostFileAccess::new(runner, home_directory.clone()));
        let terminal = Arc::new(BoardTerminalSessions::new(files.clone(), &home_directory));
        let static_files: &'static HostFileAccess = leak(files.clone()).as_ref();
        let static_database: &'static Database = leak(database.clone()).as_ref();
        let state_store = leak(HostKanvibeStateStore::new(static_files));
        let kanban = KanbanService::new(KanbanServiceParts {
            database: database.clone(),
            files: files.clone(),
            board: BoardNotifier::new(),
            terminal: terminal.clone(),
            worktrees: Arc::new(ProcessTaskWorktrees::new(
                files.clone(),
                terminal,
                database.clone(),
            )),
            hooks: Arc::new(BoardHookInstallation),
            state_files: Arc::new(KanvibeStateWriters::new(state_store, static_database)),
            peers: Arc::new(BoardPeerPropagation),
            clock: Arc::new(std::time::Instant::now),
            timings: KanbanTimings::default(),
        });
        Self {
            notifications: Arc::new(NotificationStore::new(Arc::new(
                DatabaseNotificationListStorage::new(database.clone()),
            ))),
            diff: Arc::new(DiffService::new(database.clone(), files.clone())),
            files,
            kanban: Arc::new(kanban),
            ssh_config: Arc::new(SshConfigReader::new(&home_directory)),
            database,
            faults,
            platform,
        }
    }

    fn settings(&self) -> AppSettingsService<'_> {
        AppSettingsService::new(&self.database, self.platform)
    }

    fn check_fault(&self, target: &str, arguments: &str) -> FaultOutcome {
        self.faults
            .as_ref()
            .map(|faults| faults.check(target, arguments))
            .unwrap_or(FaultOutcome::Proceed)
    }

    /// AppSettings `board_view_mode`. 읽지 못하면 칸반이다
    pub fn board_view_mode(&self) -> BoardViewMode {
        let stored = self.settings().get_app_setting(BOARD_VIEW_MODE_KEY).ok();
        BoardViewMode::parse(stored.flatten().as_deref())
    }

    fn read_snapshot(&self) -> BoardServiceResult<BoardSnapshot> {
        let failure = |reason: String| BoardServiceFailure::new(reason);
        let tasks = self
            .kanban
            .get_tasks_by_status()
            .map_err(|error| failure(error.message))?;
        let settings = self.settings();
        let database_failure = |error: kanvibe_db::DatabaseError| failure(error.to_string());
        // 연결 잠금을 문장마다 풀어야 한다. 한 식 안에서 두 번 잡으면 같은 스레드에서 멈춘다
        let projects = list_projects(&self.database.connection()).map_err(database_failure)?;
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
            ssh_hosts: self.ssh_config.available_hosts(),
            projects,
            is_done_alert_dismissed,
            default_session_type,
            is_vim_mode_enabled,
            sort_preference,
            diff_stats_by_task_id: self.diff.get_task_diff_stats(&[]),
            notifications: self.notifications.list_notifications().unwrap_or_default(),
            is_notification_unread_only,
            tasks: tasks.tasks,
            done_total: tasks.done_total,
            done_limit: tasks.done_limit,
        })
    }
}

/// 고장 판정 결과대로 기다리거나 실패한다
async fn apply_fault(
    target: &str,
    fault: FaultOutcome,
    executor: &BackgroundExecutor,
) -> BoardServiceResult<()> {
    match fault {
        FaultOutcome::Proceed | FaultOutcome::Result(_) => Ok(()),
        FaultOutcome::Hang => std::future::pending().await,
        FaultOutcome::Delay(wait) => {
            executor.timer(wait).await;
            Ok(())
        }
        FaultOutcome::Fail { message, after } => {
            if after > Duration::ZERO {
                executor.timer(after).await;
            }
            Err(BoardServiceFailure::new(injected_failure_reason(
                target, &message,
            )))
        }
    }
}

/// 서비스 호출 하나를 고장 판정 뒤 백그라운드에서 실행한다
fn spawn_service_call<T: Send + 'static>(
    services: &Arc<BoardServices>,
    target: &str,
    arguments: &str,
    cx: &App,
    call: impl FnOnce(&BoardServices) -> BoardServiceResult<T> + Send + 'static,
) -> Task<BoardServiceResult<T>> {
    let fault = services.check_fault(target, arguments);
    let executor = cx.background_executor().clone();
    let services = services.clone();
    let target = target.to_string();
    cx.background_spawn(async move {
        apply_fault(&target, fault, &executor).await?;
        call(&services)
    })
}

fn json_arguments(arguments: &[&str]) -> String {
    serde_json::to_string(arguments).unwrap_or_default()
}

/// 서비스 오류를 화면 문구용 실패로 바꾼다
fn service_failure(error: impl std::fmt::Display) -> BoardServiceFailure {
    BoardServiceFailure::new(error.to_string())
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
            Ok(services.diff.get_task_diff_stats(&task_ids_to_refresh))
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
                    .kanban
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
                    .kanban
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
                .kanban
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
                .kanban
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
                    .kanban
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
                .kanban
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
                let project = kanvibe_db::repositories::projects::find_project(
                    &services.database.connection(),
                    &project_id,
                )
                .map_err(service_failure)?;
                let Some(project) = project else {
                    return Ok(Vec::new());
                };
                let ssh_host = project.ssh_host.as_deref().filter(|host| !host.is_empty());
                kanvibe_process::git_operations::list_branches(
                    &services.files,
                    &project.repo_path,
                    ssh_host,
                    ssh_host.is_none(),
                )
                .map_err(|error| BoardServiceFailure::new(error.message))
            },
        )
    }

    /// Done 이동 안내를 다시 보지 않는다(AppSettings `done_alert_dismissed`)
    pub fn dismiss_done_alert(&self) -> BoardServiceResult<()> {
        self.settings()
            .dismiss_done_alert()
            .map_err(service_failure)
    }

    pub fn save_board_sort_preference(
        &self,
        preference: &BoardSortPreference,
    ) -> BoardServiceResult<()> {
        self.settings()
            .set_board_sort_preference(preference)
            .map_err(service_failure)
    }

    pub fn save_notification_unread_only(&self, is_enabled: bool) -> BoardServiceResult<()> {
        self.settings()
            .set_notification_unread_only_enabled(is_enabled)
            .map_err(service_failure)
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
                    .notifications
                    .mark_notification_read(&notification_id)
                    .map(|_| ())
                    .map_err(service_failure)
            },
        )
    }

    pub fn mark_all_notifications_read(self: &Arc<Self>, cx: &App) -> Task<BoardServiceResult<()>> {
        spawn_service_call(self, NOTIFICATION_MARK_READ_SERVICE, "[]", cx, |services| {
            services
                .notifications
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
                    .kanban
                    .get_task_by_id(&task_id)
                    .map(|task| task.is_some())
                    .map_err(service_failure),
                None => Ok(true),
            },
        )
    }
}
