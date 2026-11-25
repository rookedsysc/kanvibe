//! 화면이 직접 부르는 서비스(계획 AD-8: 범용 IPC 없이 같은 프로세스 호출).
//! QA 모드에서는 모든 호출이 TS IPC 이름(`namespace.method`)으로 고장 주입 판정을 먼저 거친다.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{App, AppContext, BackgroundExecutor, Global, SharedString, Task};
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_db::repositories::projects::list_projects;
use kanvibe_db::repositories::tasks::list_tasks;
use kanvibe_db::{Database, DatabaseLocation};
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::shortcuts::bindings::ShortcutBindings;
use kanvibe_domain::task::TASK_STATUS_ORDER;

use crate::qa::AppClock;
use crate::qa::faults::{FaultOutcome, FaultRegistry};
use crate::theme::ThemePreference;

/// 서비스 호출 실패. 화면은 `reason`을 `loadFailed` 문구에 그대로 넣는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceFailure {
    pub reason: String,
}

pub type ServiceResult<T> = Result<T, ServiceFailure>;

/// 보드 헤더와 열 머리가 그리는 데이터. 카드 본체는 M7에서 더한다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardHeaderSnapshot {
    pub project_names: Vec<SharedString>,
    pub sort_active_count: usize,
    /// `TASK_STATUS_ORDER` 순서의 상태별 작업 수
    pub status_counts: [usize; 5],
}

pub struct AppServices {
    database: Option<Arc<Database>>,
    faults: Option<Arc<FaultRegistry>>,
    pub clock: AppClock,
    pub shortcut_platform: ShortcutPlatform,
}

impl Global for AppServices {}

/// 보드 첫 조회. TS `BoardRoute`가 `kanban.getTasksByStatus`와 함께 부르는 묶음이다
pub const BOARD_TASKS_SERVICE: &str = "kanban.getTasksByStatus";

impl AppServices {
    pub fn new(
        database: Option<Arc<Database>>,
        faults: Option<Arc<FaultRegistry>>,
        clock: AppClock,
    ) -> Self {
        Self {
            database,
            faults,
            clock,
            shortcut_platform: ShortcutPlatform::current(),
        }
    }

    /// 앱 데이터 디렉터리의 `kanvibe.db`를 연다
    pub fn open_database(data_dir: &std::path::Path) -> anyhow::Result<Arc<Database>> {
        let location = DatabaseLocation::in_app_data_directory(data_dir, None);
        Ok(Arc::new(Database::open(&location)?))
    }

    pub fn faults(&self) -> Option<&Arc<FaultRegistry>> {
        self.faults.as_ref()
    }

    fn settings(&self) -> Option<AppSettingsService<'_>> {
        self.database
            .as_deref()
            .map(|database| AppSettingsService::new(database, self.shortcut_platform))
    }

    /// AppSettings `theme_preference`. DB가 없거나 읽지 못하면 `system`이다
    pub fn theme_preference(&self) -> ThemePreference {
        let stored = self.settings().and_then(|settings| {
            settings
                .get_app_setting(ThemePreference::APP_SETTING_KEY)
                .ok()
                .flatten()
        });
        ThemePreference::parse(stored.as_deref())
    }

    /// AppSettings `shortcut_bindings`(재배정 반영). 읽지 못하면 기본 표다
    pub fn shortcut_bindings(&self) -> ShortcutBindings {
        self.settings()
            .and_then(|settings| settings.get_shortcut_bindings().ok())
            .unwrap_or_else(ShortcutBindings::defaults)
    }

    /// 보드 헤더 데이터를 백그라운드에서 읽는다
    pub fn load_board_header(&self, cx: &App) -> Task<ServiceResult<BoardHeaderSnapshot>> {
        let database = self.database.clone();
        let platform = self.shortcut_platform;
        let fault = self.check_fault(BOARD_TASKS_SERVICE, "[]");
        let executor = cx.background_executor().clone();
        cx.background_spawn(async move {
            apply_fault(fault, &executor).await?;
            read_board_header(database.as_deref(), platform)
        })
    }

    fn check_fault(&self, target: &str, serialized_arguments: &str) -> FaultOutcome {
        self.faults
            .as_ref()
            .map(|faults| faults.check(target, serialized_arguments))
            .unwrap_or(FaultOutcome::Proceed)
    }
}

/// 고장 판정 결과대로 기다리거나 실패한다. `Result`는 이 서비스가 값 대체를 지원하지 않아 그대로 진행한다
async fn apply_fault(fault: FaultOutcome, executor: &BackgroundExecutor) -> ServiceResult<()> {
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
            Err(ServiceFailure { reason: message })
        }
    }
}

fn read_board_header(
    database: Option<&Database>,
    platform: ShortcutPlatform,
) -> ServiceResult<BoardHeaderSnapshot> {
    let Some(database) = database else {
        return Ok(BoardHeaderSnapshot {
            project_names: Vec::new(),
            sort_active_count: 0,
            status_counts: [0; 5],
        });
    };
    let failure = |error: kanvibe_db::DatabaseError| ServiceFailure {
        reason: error.to_string(),
    };
    let projects = list_projects(&database.connection()).map_err(failure)?;
    let tasks = list_tasks(&database.connection()).map_err(failure)?;
    let status_counts =
        TASK_STATUS_ORDER.map(|status| tasks.iter().filter(|task| task.status == status).count());
    let sort_preference = AppSettingsService::new(database, platform)
        .get_board_sort_preference()
        .map_err(failure)?;
    Ok(BoardHeaderSnapshot {
        project_names: projects
            .into_iter()
            .map(|project| project.name.into())
            .collect(),
        sort_active_count: sort_preference.keys.len(),
        status_counts,
    })
}
