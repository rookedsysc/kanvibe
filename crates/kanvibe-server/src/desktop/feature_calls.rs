//! 기능 경로 하나를 모바일 기능 서비스 메서드 하나에 잇는다(TS `FEATURE_ROUTES`의 `run`).

use serde::Serialize;
use serde_json::{Map, Value};

use super::DesktopServices;
use super::errors::{from_mobile, from_serde};
use crate::bridge::{BridgeError, BridgeResult, FeatureCall};

/// `{ <name>: value }`
fn field<T: Serialize>(name: &str, value: T) -> BridgeResult<Map<String, Value>> {
    let mut result = Map::new();
    let mut value = serde_json::to_value(value).map_err(from_serde)?;
    super::dates::normalize_database_dates(&mut value);
    result.insert(name.to_string(), value);
    Ok(result)
}

/// 결과 객체 자체를 펼친다
fn spread<T: Serialize>(value: T) -> BridgeResult<Map<String, Value>> {
    let mut value = serde_json::to_value(value).map_err(from_serde)?;
    super::dates::normalize_database_dates(&mut value);
    match value {
        Value::Object(fields) => Ok(fields),
        _ => Err(BridgeError::Internal(
            "feature result is not an object".to_string(),
        )),
    }
}

impl DesktopServices {
    pub(super) fn run_feature_call(&self, call: FeatureCall) -> BridgeResult<Map<String, Value>> {
        match call {
            FeatureCall::SearchTasks
            | FeatureCall::DoneTasks { .. }
            | FeatureCall::UpdateTaskDetails { .. }
            | FeatureCall::BranchTask { .. }
            | FeatureCall::RefreshTaskPr { .. }
            | FeatureCall::Hosts
            | FeatureCall::UpdateProjectColor { .. }
            | FeatureCall::ProjectTasks { .. }
            | FeatureCall::ProjectBranchTask { .. }
            | FeatureCall::DeleteProject { .. } => self.run_board_feature(call),
            FeatureCall::DiffStats
            | FeatureCall::TaskDiff { .. }
            | FeatureCall::TaskDiffFile { .. }
            | FeatureCall::SaveTaskDiffFile { .. }
            | FeatureCall::RenameTab { .. }
            | FeatureCall::MoveTab { .. }
            | FeatureCall::PastePaneImage { .. }
            | FeatureCall::PaneLayouts
            | FeatureCall::SavePaneLayout { .. }
            | FeatureCall::DeletePaneLayout { .. } => self.run_workspace_feature(call),
            _ => self.run_settings_feature(call),
        }
    }

    fn run_board_feature(&self, call: FeatureCall) -> BridgeResult<Map<String, Value>> {
        let features = &self.features;
        let outcome = match call {
            FeatureCall::SearchTasks => features
                .get_mobile_searchable_tasks()
                .map(|tasks| field("tasks", tasks)),
            FeatureCall::DoneTasks { offset, limit } => features
                .get_mobile_done_tasks(offset as f64, limit as f64)
                .map(spread),
            FeatureCall::UpdateTaskDetails { task_id, body } => features
                .update_mobile_task_details(&task_id, &body)
                .map(|task| field("task", task)),
            FeatureCall::BranchTask { task_id, body } => features
                .branch_mobile_task(&task_id, &body)
                .map(|task| field("task", task)),
            FeatureCall::RefreshTaskPr { task_id } => features
                .refresh_mobile_task_pr(&task_id)
                .map(|url| field("prUrl", url)),
            FeatureCall::Hosts => Ok(field("hosts", features.get_mobile_hosts())),
            FeatureCall::UpdateProjectColor { project_id, body } => features
                .update_mobile_project_color(&project_id, &body)
                .map(|color| field("color", color)),
            FeatureCall::ProjectTasks { project_id } => features
                .get_mobile_project_tasks(&project_id)
                .map(|tasks| field("tasks", tasks)),
            FeatureCall::ProjectBranchTask { project_id, branch } => features
                .get_mobile_project_branch_task(&project_id, branch.as_deref())
                .map(|task| field("task", task)),
            FeatureCall::DeleteProject { project_id } => features
                .delete_mobile_project(&project_id)
                .map(|()| Ok(Map::new())),
            _ => unreachable!("기능 분류에서 보드 경로만 넘어온다"),
        };
        outcome.map_err(from_mobile)?
    }

    fn run_workspace_feature(&self, call: FeatureCall) -> BridgeResult<Map<String, Value>> {
        let features = &self.features;
        let outcome = match call {
            FeatureCall::DiffStats => features
                .get_mobile_diff_stats()
                .map(|stats| field("stats", stats)),
            FeatureCall::TaskDiff { task_id } => features
                .get_mobile_diff(&task_id)
                .map(|files| field("files", files)),
            FeatureCall::TaskDiffFile { task_id, path } => features
                .get_mobile_diff_file(&task_id, path.as_deref())
                .map(spread),
            FeatureCall::SaveTaskDiffFile {
                task_id,
                path,
                body,
            } => features
                .save_mobile_diff_file(&task_id, path.as_deref(), &body)
                .map(|()| Ok(Map::new())),
            FeatureCall::RenameTab {
                task_id,
                tab_id,
                body,
            } => features
                .rename_mobile_tab(&task_id, &tab_id, &body)
                .map(|tabs| field("tabs", tabs)),
            FeatureCall::MoveTab {
                task_id,
                tab_id,
                body,
            } => features
                .move_mobile_tab(&task_id, &tab_id, &body)
                .map(|tabs| field("tabs", tabs)),
            FeatureCall::PastePaneImage {
                task_id,
                pane_id,
                body,
            } => features
                .paste_mobile_remote_image(&task_id, &pane_id, &body)
                .map(|path| field("remotePath", path)),
            FeatureCall::PaneLayouts => features
                .get_mobile_pane_layouts()
                .map(|layouts| field("layouts", layouts)),
            FeatureCall::SavePaneLayout { body } => features
                .save_mobile_pane_layout(&body)
                .map(|layout| field("layout", layout)),
            FeatureCall::DeletePaneLayout { layout_id } => features
                .delete_mobile_pane_layout(&layout_id)
                .map(|()| Ok(Map::new())),
            _ => unreachable!("기능 분류에서 작업 공간 경로만 넘어온다"),
        };
        outcome.map_err(from_mobile)?
    }

    fn run_settings_feature(&self, call: FeatureCall) -> BridgeResult<Map<String, Value>> {
        let features = &self.features;
        let outcome = match call {
            FeatureCall::Settings => features
                .get_mobile_settings()
                .map(|settings| field("settings", settings)),
            FeatureCall::UpdateSettings { body } => features
                .update_mobile_settings(&body)
                .map(|settings| field("settings", settings)),
            FeatureCall::SessionChangeTargets { session_type } => features
                .get_mobile_session_change_targets(session_type.as_deref())
                .map(|targets| field("targets", targets)),
            FeatureCall::ChangeAllSessions { body } => features
                .change_all_mobile_task_sessions(&body)
                .map(|result| field("result", result)),
            FeatureCall::RunSync => features
                .run_mobile_sync()
                .map(|result| field("result", result)),
            FeatureCall::SyncStatus => Ok(field("status", features.get_mobile_sync_status())),
            FeatureCall::StartSync => Ok(field("status", features.start_mobile_sync())),
            FeatureCall::SyncReview { notification_id } => features
                .get_mobile_sync_review(&notification_id)
                .map(|review| field("review", review)),
            FeatureCall::DeleteSyncReviewTasks {
                notification_id,
                body,
            } => features
                .delete_mobile_sync_review_tasks(&notification_id, &body)
                .map(|ids| field("deletedTaskIds", ids)),
            FeatureCall::MarkTaskNotificationsRead { task_id } => features
                .mark_mobile_task_notifications_read(&task_id)
                .map(|count| field("updatedCount", count)),
            _ => unreachable!("기능 분류에서 설정 경로만 넘어온다"),
        };
        outcome.map_err(from_mobile)?
    }
}
