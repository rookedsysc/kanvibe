//! 모바일 보드와 알림이 가리키는 태스크 하나.

use std::collections::HashMap;

use kanvibe_domain::project::Project;
use kanvibe_domain::task::SessionType;
use kanvibe_services::kanban::TasksByStatusWithMeta;
use serde::Serialize;
use serde_json::{Map, Value};

use super::MobileBridge;
use crate::app_settings::open_app_settings;
use crate::error::{MobileError, MobileResult};

/// 모바일 보드가 한 번에 받는 것. 데스크탑 보드와 같은 조회를 쓰므로 정렬과 done 페이지 크기가 같다
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileBoard {
    #[serde(flatten)]
    pub board: TasksByStatusWithMeta,
    pub projects: Vec<Value>,
    /// `iconFormat=deduplicated`로 요청했을 때만 있다
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_icons: Option<Map<String, Value>>,
    pub default_session_type: SessionType,
}

fn to_value(project: &Project) -> MobileResult<Value> {
    serde_json::to_value(project).map_err(MobileError::internal)
}

/// 같은 아이콘은 한 번만 싣고 프로젝트에는 키만 남긴다. 키는 처음 나온 순서대로 "0", "1", …이다
fn deduplicate_project_icons(
    projects: &[Project],
) -> MobileResult<(Vec<Value>, Map<String, Value>)> {
    let mut icon_keys: HashMap<&str, String> = HashMap::new();
    let mut project_icons = Map::new();
    let mut compact_projects = Vec::with_capacity(projects.len());
    for project in projects {
        let icon_key = project.icon_data_url.as_deref().map(|icon| {
            let next_key = icon_keys.len().to_string();
            let key = icon_keys.entry(icon).or_insert(next_key).clone();
            project_icons.insert(key.clone(), Value::String(icon.to_string()));
            key
        });
        let mut compact = to_value(project)?;
        compact["iconDataUrl"] = Value::Null;
        compact["iconKey"] = icon_key.map_or(Value::Null, Value::String);
        compact_projects.push(compact);
    }
    Ok((compact_projects, project_icons))
}

impl MobileBridge {
    pub fn get_mobile_board(&self, is_icon_deduplicated: bool) -> MobileResult<MobileBoard> {
        let board = self.tasks.get_tasks_by_status()?;
        let projects = self.projects.get_all_projects()?;
        let default_session_type = open_app_settings(&self.database).get_default_session_type()?;
        let (projects, project_icons) = if is_icon_deduplicated {
            let (compact, icons) = deduplicate_project_icons(&projects)?;
            (compact, Some(icons))
        } else {
            let full = projects.iter().map(to_value).collect::<MobileResult<_>>()?;
            (full, None)
        };
        Ok(MobileBoard {
            board,
            projects,
            project_icons,
            default_session_type,
        })
    }

    /// 완료 태스크의 보드 페이지와 관계없이 알림이 가리키는 태스크를 직접 조회한다. 프로젝트 이름을 덧붙인다
    pub fn get_mobile_task(&self, task_id: &str) -> MobileResult<Option<Value>> {
        let Some(task) = self.tasks.get_task_by_id(task_id)? else {
            return Ok(None);
        };
        let project_name = task
            .project
            .as_ref()
            .map_or(Value::Null, |project| Value::String(project.name.clone()));
        let mut value = serde_json::to_value(&task).map_err(MobileError::internal)?;
        value["projectName"] = project_name;
        Ok(Some(value))
    }
}
