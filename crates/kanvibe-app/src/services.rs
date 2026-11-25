//! 화면이 직접 부르는 서비스(계획 AD-8: 범용 IPC 없이 같은 프로세스 호출).
//! 장기 서비스는 모두 [`DesktopRuntime`] 하나가 갖고, 화면은 [`AppServices::runtime`]으로 같은 인스턴스를 받는다.
//! QA 모드에서는 모든 호출이 TS IPC 이름(`namespace.method`)으로 고장 주입 판정을 먼저 거친다.

use std::path::Path;
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
use kanvibe_mobile::devices::MobileDevices;
use kanvibe_runtime::DesktopRuntime;
use kanvibe_server::desktop::DesktopServices;

use crate::qa::AppClock;
use crate::qa::faults::{FaultOutcome, FaultRegistry};
use crate::theme::ThemePreference;

/// 서비스 호출 실패. 화면은 `reason`을 `loadFailed` 문구에 그대로 넣는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceFailure {
    pub reason: String,
}

pub type ServiceResult<T> = Result<T, ServiceFailure>;

/// TS 렌더러가 IPC로 부른 서비스가 실패했을 때 Electron이 붙이던 머리말. 화면 문구와 픽셀을 TS와 같게 둔다
const IPC_ERROR_PREFIX: &str = "Error invoking remote method 'kanvibe:invoke': Error: ";

/// 서비스 실패 사유를 화면에 보일 문구로 바꾸는 유일한 자리. 모든 화면이 이 함수를 거쳐야
/// 머리말을 붙일지 정하는 판단이 한 곳에 남는다
pub fn format_service_error(reason: &str) -> String {
    format!("{IPC_ERROR_PREFIX}{reason}")
}

impl ServiceFailure {
    /// 화면에 보일 문구(`format_service_error`)
    pub fn display_message(&self) -> String {
        format_service_error(&self.reason)
    }
}

/// 보드 헤더와 열 머리가 그리는 데이터. 카드 본체는 M7에서 더한다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardHeaderSnapshot {
    pub project_names: Vec<SharedString>,
    pub sort_active_count: usize,
    /// `TASK_STATUS_ORDER` 순서의 상태별 작업 수
    pub status_counts: [usize; 5],
}

pub struct AppServices {
    /// `--data-dir`로 연 런타임. 없으면 DB 없이 뜬 실행이다
    runtime: Option<Arc<DesktopRuntime>>,
    /// 런타임 위의 모바일 업무 계층(기기·페어링·LAN 평문 설정·푸시). 프로세스 안 서버가 같은 인스턴스를 쓴다
    desktop_services: Option<Arc<DesktopServices>>,
    faults: Option<Arc<FaultRegistry>>,
    pub clock: AppClock,
    pub shortcut_platform: ShortcutPlatform,
}

impl Global for AppServices {}

/// 보드 첫 조회. TS `BoardRoute`가 `kanban.getTasksByStatus`와 함께 부르는 묶음이다
pub const BOARD_TASKS_SERVICE: &str = "kanban.getTasksByStatus";

impl AppServices {
    pub fn new(
        runtime: Option<Arc<DesktopRuntime>>,
        faults: Option<Arc<FaultRegistry>>,
        clock: AppClock,
        shortcut_platform: ShortcutPlatform,
    ) -> Self {
        Self {
            runtime,
            desktop_services: None,
            faults,
            clock,
            shortcut_platform,
        }
    }

    /// 런타임과 같은 인스턴스로 만든 모바일 업무 계층을 단다
    pub fn with_desktop_services(mut self, desktop_services: Arc<DesktopServices>) -> Self {
        self.desktop_services = Some(desktop_services);
        self
    }

    /// 모바일 서버가 쓰는 업무 계층. 데이터 디렉터리 없이 뜬 실행이면 `None`이다
    pub fn desktop_services(&self) -> Option<&Arc<DesktopServices>> {
        self.desktop_services.as_ref()
    }

    /// 페어링 코드·기기 목록·기기 해제(설정 화면 모바일 연결). 서버가 띄운 페어링과 같은 인스턴스다
    pub fn mobile_devices(&self) -> Option<&Arc<MobileDevices>> {
        self.desktop_services
            .as_ref()
            .map(|desktop_services| &desktop_services.devices)
    }

    /// 앱 데이터 디렉터리의 `kanvibe.db`를 연다
    pub fn open_database(data_dir: &Path) -> anyhow::Result<Arc<Database>> {
        let location = DatabaseLocation::in_app_data_directory(data_dir, None);
        Ok(Arc::new(Database::open(&location)?))
    }

    /// 앱 프로세스에 하나인 서비스 그래프. 데이터 디렉터리 없이 뜬 실행이면 `None`이다
    pub fn runtime(&self) -> Option<&Arc<DesktopRuntime>> {
        self.runtime.as_ref()
    }

    pub fn database(&self) -> Option<&Arc<Database>> {
        self.runtime.as_ref().map(|runtime| runtime.database())
    }

    pub fn data_dir(&self) -> Option<&Path> {
        self.runtime
            .as_ref()
            .map(|runtime| runtime.app_data_directory())
    }

    pub fn faults(&self) -> Option<&Arc<FaultRegistry>> {
        self.faults.as_ref()
    }

    fn settings(&self) -> Option<AppSettingsService<'_>> {
        self.database()
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
        let database = self.database().cloned();
        let platform = self.shortcut_platform;
        let fault = self.check_fault(BOARD_TASKS_SERVICE, "[]");
        let executor = cx.background_executor().clone();
        cx.background_spawn(async move {
            apply_fault(fault, &executor).await?;
            read_board_header(database.as_deref(), platform)
        })
    }

    /// TS IPC 이름(`namespace.method`)과 JSON 인자로 고장 주입 판정을 한다. QA 모드가 아니면 늘 진행이다
    pub fn check_fault(&self, target: &str, serialized_arguments: &str) -> FaultOutcome {
        self.faults
            .as_ref()
            .map(|faults| faults.check(target, serialized_arguments))
            .unwrap_or(FaultOutcome::Proceed)
    }
}

/// 고장 판정 결과대로 기다리거나 실패한다. 값 대체(`Result`)는 받은 화면이 따로 쓰고 여기서는 그대로 진행한다
pub async fn apply_fault(fault: FaultOutcome, executor: &BackgroundExecutor) -> ServiceResult<()> {
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
