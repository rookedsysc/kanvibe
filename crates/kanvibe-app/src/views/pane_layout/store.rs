//! pane 레이아웃 화면의 조회·저장. 저장 규칙(전역/프로젝트 upsert)은 `kanvibe_terminal::pane_layout_service`가 갖는다.
//! 고장 주입 대상 이름은 TS IPC `paneLayout.*`, `project.getAllProjects`다.

use gpui_kit::{App, Task};
use kanvibe_db::repositories::projects::list_projects;
use kanvibe_domain::pane_layout::{PaneCommand, PaneLayoutConfig, PaneLayoutType};
use kanvibe_domain::project::Project;
use kanvibe_terminal::pane_layout_service::{
    SavePaneLayoutInput, delete_pane_layout, get_global_pane_layout, get_project_pane_layout,
    save_pane_layout,
};

use crate::services::ServiceResult;
use crate::views::settings::service_call::{call_database_read, call_database_service, failure};

pub fn load_global_pane_layout(cx: &App) -> Task<ServiceResult<Option<PaneLayoutConfig>>> {
    call_database_service(
        cx,
        "paneLayout.getGlobalPaneLayout",
        "[]".to_string(),
        |database| get_global_pane_layout(database).map_err(failure),
    )
}

pub fn load_projects(cx: &App) -> Task<ServiceResult<Vec<Project>>> {
    call_database_read(cx, "project.getAllProjects", |database| {
        list_projects(&database.connection()).map_err(failure)
    })
}

pub fn load_project_pane_layout(
    project_id: String,
    cx: &App,
) -> Task<ServiceResult<Option<PaneLayoutConfig>>> {
    let arguments = serde_json::json!([project_id]).to_string();
    call_database_service(
        cx,
        "paneLayout.getProjectPaneLayout",
        arguments,
        move |database| get_project_pane_layout(database, &project_id).map_err(failure),
    )
}

/// 레이아웃 저장 요청 하나(TS `savePaneLayout` 인자)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneLayoutSaveRequest {
    pub layout_type: PaneLayoutType,
    pub commands: Vec<String>,
    pub project_id: Option<String>,
    pub is_global: bool,
}

pub fn save_layout(request: PaneLayoutSaveRequest, cx: &App) -> Task<ServiceResult<()>> {
    let panes: Vec<PaneCommand> = request
        .commands
        .iter()
        .enumerate()
        .map(|(position, command)| PaneCommand {
            position: position as i64,
            command: command.clone(),
        })
        .collect();
    let arguments = serde_json::json!([{
        "layoutType": request.layout_type.as_str(),
        "panes": panes,
        "projectId": request.project_id,
        "isGlobal": request.is_global,
    }])
    .to_string();
    let input = SavePaneLayoutInput {
        layout_type: request.layout_type,
        panes,
        project_id: request.project_id,
        is_global: request.is_global,
    };
    call_database_service(
        cx,
        "paneLayout.savePaneLayout",
        arguments,
        move |database| {
            save_pane_layout(database, input)
                .map(|_| ())
                .map_err(failure)
        },
    )
}

pub fn delete_layout(layout_id: String, cx: &App) -> Task<ServiceResult<()>> {
    let arguments = serde_json::json!([layout_id]).to_string();
    call_database_service(
        cx,
        "paneLayout.deletePaneLayout",
        arguments,
        move |database| {
            delete_pane_layout(database, &layout_id)
                .map(|_| ())
                .map_err(failure)
        },
    )
}
