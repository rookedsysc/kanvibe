//! diff 화면이 부르는 서비스. TS 렌더러의 `getTaskById`·`getGitDiffFiles`·`getOriginalFileContent`·`getFileContent`·
//! `saveFileContent` IPC에 해당한다. QA 모드에서는 모든 호출이 TS IPC 이름(`namespace.method`)으로 고장 주입 판정을 먼저 거친다.
//! 화면 테스트는 응답 순서를 직접 쥐어야 하므로 화면은 이 trait을 통해서만 부른다.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{App, AppContext, BackgroundExecutor, Global, Task};
use kanvibe_db::Database;
use kanvibe_db::repositories::tasks::find_task;
use kanvibe_domain::task::KanbanTask;
use kanvibe_domain::task_diff_stats::DiffFile;
use kanvibe_process::exec::system::SystemShellCommandRunner;
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::shell_environment::current_user_home_directory;
use kanvibe_services::diff::DiffService;

use crate::qa::faults::{FaultOutcome, FaultRegistry};
use crate::services::AppServices;

/// 실패 사유. 화면은 TS `error.message`처럼 그대로 보여 준다
pub type SourceResult<T> = Result<T, String>;

pub const TASK_SERVICE: &str = "kanban.getTaskById";
pub const DIFF_FILES_SERVICE: &str = "diff.getGitDiffFiles";
pub const ORIGINAL_CONTENT_SERVICE: &str = "diff.getOriginalFileContent";
pub const FILE_CONTENT_SERVICE: &str = "diff.getFileContent";
pub const SAVE_SERVICE: &str = "diff.saveFileContent";

pub trait DiffSource {
    fn load_task(&self, task_id: &str, cx: &App) -> Task<SourceResult<Option<KanbanTask>>>;
    fn load_files(&self, task_id: &str, cx: &App) -> Task<SourceResult<Vec<DiffFile>>>;
    fn read_original(&self, task_id: &str, file_path: &str, cx: &App)
    -> Task<SourceResult<String>>;
    fn read_modified(&self, task_id: &str, file_path: &str, cx: &App)
    -> Task<SourceResult<String>>;
    /// 저장 실패는 서비스가 돌려준 사유를 담는다
    fn save(
        &self,
        task_id: &str,
        file_path: &str,
        content: String,
        cx: &App,
    ) -> Task<SourceResult<()>>;
}

pub type SharedDiffSource = Rc<dyn DiffSource>;

/// 앱 서비스(DB, diff 서비스, QA 고장 목록)로 부르는 실제 구현
pub struct ServiceDiffSource {
    database: Option<Arc<Database>>,
    diff_service: Option<DiffService>,
    faults: Option<Arc<FaultRegistry>>,
}

/// 앱 전체가 하나의 diff 서비스를 함께 쓴다. 같은 task 조회를 합치는 대기열이 서비스 안에 있다
struct SharedDiffService(DiffService);

impl Global for SharedDiffService {}

impl ServiceDiffSource {
    pub fn from_app(cx: &mut App) -> Self {
        let services = cx.global::<AppServices>();
        let database = services.database().cloned();
        let faults = services.faults().cloned();
        let diff_service = database
            .as_ref()
            .map(|database| shared_diff_service(database, cx));
        Self {
            database,
            diff_service,
            faults,
        }
    }

    fn check_fault(&self, target: &str, arguments: &[&str]) -> FaultOutcome {
        let serialized_arguments = serde_json::to_string(arguments).unwrap_or_default();
        self.faults
            .as_ref()
            .map(|faults| faults.check(target, &serialized_arguments))
            .unwrap_or(FaultOutcome::Proceed)
    }

    /// 고장 판정을 거친 뒤 막히는 서비스 호출을 백그라운드에서 돌린다
    fn spawn<T: Send + 'static>(
        &self,
        target: &str,
        arguments: &[&str],
        cx: &App,
        call: impl FnOnce() -> SourceResult<T> + Send + 'static,
    ) -> Task<SourceResult<T>> {
        let fault = self.check_fault(target, arguments);
        let executor = cx.background_executor().clone();
        cx.background_spawn(async move {
            apply_fault(fault, &executor).await?;
            call()
        })
    }

    fn diff_service(&self) -> SourceResult<DiffService> {
        self.diff_service
            .clone()
            .ok_or_else(|| "데이터 디렉터리가 없습니다".to_string())
    }
}

fn shared_diff_service(database: &Arc<Database>, cx: &mut App) -> DiffService {
    if let Some(shared) = cx.try_global::<SharedDiffService>() {
        return shared.0.clone();
    }
    let runner = Arc::new(SystemShellCommandRunner::for_current_process());
    let files = Arc::new(HostFileAccess::new(runner, current_user_home_directory()));
    let service = DiffService::new(Arc::clone(database), files);
    cx.set_global(SharedDiffService(service.clone()));
    service
}

impl DiffSource for ServiceDiffSource {
    fn load_task(&self, task_id: &str, cx: &App) -> Task<SourceResult<Option<KanbanTask>>> {
        let database = self.database.clone();
        let task_id_owned = task_id.to_string();
        self.spawn(TASK_SERVICE, &[task_id], cx, move || {
            let Some(database) = database else {
                return Ok(None);
            };
            find_task(&database.connection(), &task_id_owned).map_err(|error| error.to_string())
        })
    }

    fn load_files(&self, task_id: &str, cx: &App) -> Task<SourceResult<Vec<DiffFile>>> {
        let service = self.diff_service();
        let task_id_owned = task_id.to_string();
        self.spawn(DIFF_FILES_SERVICE, &[task_id], cx, move || {
            Ok(service?.get_git_diff_files(&task_id_owned))
        })
    }

    fn read_original(
        &self,
        task_id: &str,
        file_path: &str,
        cx: &App,
    ) -> Task<SourceResult<String>> {
        let service = self.diff_service();
        let (task_id_owned, file_path_owned) = (task_id.to_string(), file_path.to_string());
        self.spawn(
            ORIGINAL_CONTENT_SERVICE,
            &[task_id, file_path],
            cx,
            move || Ok(service?.get_original_file_content(&task_id_owned, &file_path_owned)),
        )
    }

    fn read_modified(
        &self,
        task_id: &str,
        file_path: &str,
        cx: &App,
    ) -> Task<SourceResult<String>> {
        let service = self.diff_service();
        let (task_id_owned, file_path_owned) = (task_id.to_string(), file_path.to_string());
        self.spawn(FILE_CONTENT_SERVICE, &[task_id, file_path], cx, move || {
            Ok(service?.get_file_content(&task_id_owned, &file_path_owned))
        })
    }

    fn save(
        &self,
        task_id: &str,
        file_path: &str,
        content: String,
        cx: &App,
    ) -> Task<SourceResult<()>> {
        let service = self.diff_service();
        let (task_id_owned, file_path_owned) = (task_id.to_string(), file_path.to_string());
        self.spawn(SAVE_SERVICE, &[task_id, file_path], cx, move || {
            let result = service?.save_file_content(&task_id_owned, &file_path_owned, &content);
            if result.success {
                Ok(())
            } else {
                Err(result.error.unwrap_or_default())
            }
        })
    }
}

/// 고장 판정 결과대로 기다리거나 실패한다. 값 대체(`result`)는 이 화면의 상태 목록이 쓰지 않아 그대로 진행한다
async fn apply_fault(fault: FaultOutcome, executor: &BackgroundExecutor) -> SourceResult<()> {
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
            Err(message)
        }
    }
}
