//! 작업 상세 데이터 조회(TS `TaskDetailContent`의 첫 조회와 보조 조회).
//!
//! 첫 조회는 작업 한 건과 `sidebar_default_collapsed`만 기다리고, 프로젝트 목록·기본 세션 타입·Done 경고 해제 여부·
//! base 브랜치 작업은 함께 읽되 실패해도 화면을 막지 않는다(TS `applySupplementalState`). QA 모드에서는 TS IPC 이름으로
//! 고장 주입 판정을 먼저 거친다.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{App, AppContext, BackgroundExecutor, Task};
use kanvibe_db::Database;
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_db::repositories::{projects, tasks};
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType};

use crate::qa::faults::FaultOutcome;
use crate::services::{AppServices, ServiceFailure, ServiceResult};

/// TS IPC `kanban.getTaskById`
pub const GET_TASK_BY_ID_SERVICE: &str = "kanban.getTaskById";

/// 상세 화면이 그리는 작업과 설정
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDetailSnapshot {
    pub task: KanbanTask,
    pub project: Option<Project>,
    pub sidebar_default_collapsed: bool,
    pub default_session_type: SessionType,
    pub done_alert_dismissed: bool,
    /// 같은 프로젝트에서 이 작업의 base 브랜치를 맡은 작업. 자기 자신이면 `None`
    pub base_branch_task_id: Option<String>,
}

impl TaskDetailSnapshot {
    /// 터미널이 붙을 세션이 있는지(TS `hasTerminal`)
    pub fn has_terminal(&self) -> bool {
        self.task.session_type.is_some()
            && self
                .task
                .session_name
                .as_deref()
                .is_some_and(|name| !name.is_empty())
    }

    pub fn is_remote(&self) -> bool {
        self.task
            .ssh_host
            .as_deref()
            .is_some_and(|host| !host.is_empty())
    }
}

/// 작업을 백그라운드에서 읽는다. `Ok(None)`은 조회는 성공했지만 작업이 없다는 뜻이다
pub fn load_task_detail(
    task_id: &str,
    cx: &App,
) -> Task<ServiceResult<Option<TaskDetailSnapshot>>> {
    let services = cx.global::<AppServices>();
    let database = services.database().cloned();
    let platform = services.shortcut_platform;
    let arguments = serde_json::to_string(&[task_id]).unwrap_or_default();
    let fault = services
        .faults()
        .map(|faults| faults.check(GET_TASK_BY_ID_SERVICE, &arguments))
        .unwrap_or(FaultOutcome::Proceed);
    let executor = cx.background_executor().clone();
    let task_id = task_id.to_string();
    cx.background_spawn(async move {
        apply_fault(fault, &executor).await?;
        let Some(database) = database else {
            return Ok(None);
        };
        read_snapshot(&database, platform, &task_id)
    })
}

/// 고장 판정 결과대로 기다리거나 실패한다(`services.rs`의 같은 판정과 같은 규칙)
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

fn failure(error: kanvibe_db::DatabaseError) -> ServiceFailure {
    ServiceFailure {
        reason: error.to_string(),
    }
}

fn read_snapshot(
    database: &Arc<Database>,
    platform: ShortcutPlatform,
    task_id: &str,
) -> ServiceResult<Option<TaskDetailSnapshot>> {
    let Some(task) = tasks::find_task(&database.connection(), task_id).map_err(failure)? else {
        return Ok(None);
    };
    let project = match task.project_id.as_deref() {
        Some(project_id) => projects::find_project(&database.connection(), project_id)
            .ok()
            .flatten(),
        None => None,
    };
    let settings = AppSettingsService::new(database, platform);
    // TS는 설정 조회가 실패하면 기본값(펼침)으로 계속한다
    let sidebar_default_collapsed = settings.get_sidebar_default_collapsed().unwrap_or(false);
    let base_branch_task_id = find_base_branch_task_id(database, &task);
    Ok(Some(TaskDetailSnapshot {
        default_session_type: settings
            .get_default_session_type()
            .unwrap_or(SessionType::Herdr),
        done_alert_dismissed: settings.get_done_alert_dismissed().unwrap_or(false),
        task,
        project,
        sidebar_default_collapsed,
        base_branch_task_id,
    }))
}

/// TS `getTaskIdByProjectAndBranch(projectId, baseBranch ?? "main")`. 찾은 작업이 자기 자신이면 `None`
fn find_base_branch_task_id(database: &Database, task: &KanbanTask) -> Option<String> {
    let project_id = task.project_id.as_deref()?;
    let base_branch = task.base_branch.as_deref().unwrap_or("main");
    let all_tasks = tasks::list_tasks(&database.connection()).ok()?;
    all_tasks
        .into_iter()
        .find(|candidate| {
            candidate.project_id.as_deref() == Some(project_id)
                && candidate.branch_name.as_deref() == Some(base_branch)
        })
        .map(|candidate| candidate.id)
        .filter(|found_id| *found_id != task.id)
}
