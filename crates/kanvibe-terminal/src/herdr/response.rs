//! Herdr CLI·API 응답 JSON의 모양과 공통 해석. TS는 필드가 없어도 `undefined`로 읽고 넘어가므로 쓰지 않는 필드는 받지 않고,
//! 빠질 수 있는 필드는 `Option`이나 기본값으로 둔다.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::HerdrError;

pub(super) fn parse_json(output: &str) -> Result<Value, HerdrError> {
    serde_json::from_str(output).map_err(|error| HerdrError::Message(error.to_string()))
}

pub(super) fn decode<T: DeserializeOwned>(value: Value) -> Result<T, HerdrError> {
    serde_json::from_value(value).map_err(|error| HerdrError::Message(error.to_string()))
}

/// JS 조건식에서 참으로 읽히는 값인지
pub(super) fn is_js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// 템플릿 문자열에 넣은 JS 값. 없으면 `undefined`다
fn js_template_text(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_string(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// TS `readHerdrResult`. 오류 응답을 빈 성공으로 오인하지 않는다
pub(super) fn read_herdr_result(output: &str) -> Result<Value, HerdrError> {
    let response = parse_json(output)?;
    if let Some(error) = response.get("error").filter(|error| is_js_truthy(error)) {
        let reason = error
            .get("message")
            .filter(|message| !message.is_null())
            .or_else(|| error.get("code"));
        return Err(HerdrError::Message(format!(
            "Herdr: {}",
            js_template_text(reason)
        )));
    }
    match response.get("result") {
        Some(result) if is_js_truthy(result) => Ok(result.clone()),
        _ => Err(HerdrError::Message(
            "Herdr 응답에 result가 없습니다.".to_string(),
        )),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct HerdrTab {
    pub tab_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub focused: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct HerdrTabList {
    pub tabs: Vec<HerdrTab>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct HerdrSession {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub running: bool,
    pub socket_path: Option<String>,
    pub session_dir: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct HerdrScroll {
    pub viewport_rows: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct HerdrPane {
    pub agent: Option<String>,
    pub agent_session_id: Option<Value>,
    pub pane_id: String,
    pub tab_id: Option<String>,
    /// 작업 경로가 사라져 복원하지 못한 pane에만 있다. 이런 pane에는 터미널도 scroll도 없다
    pub restore_error: Option<String>,
    pub scroll: Option<HerdrScroll>,
}

impl HerdrPane {
    pub fn is_unrestorable(&self) -> bool {
        self.restore_error
            .as_deref()
            .is_some_and(|error| !error.is_empty())
    }

    pub fn viewport_rows(&self) -> Option<f64> {
        self.scroll.as_ref().and_then(|scroll| scroll.viewport_rows)
    }

    pub fn is_in_tab(&self, tab_id: &str) -> bool {
        self.tab_id.as_deref() == Some(tab_id)
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct HerdrPaneList {
    pub panes: Vec<HerdrPane>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub(super) struct HerdrRect {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub width: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct HerdrLayoutPane {
    pub pane_id: String,
    pub rect: HerdrRect,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct HerdrTabLayout {
    #[serde(default)]
    pub area: HerdrRect,
    pub focused_pane_id: Option<String>,
    #[serde(default)]
    pub zoomed: bool,
    #[serde(default)]
    pub panes: Vec<HerdrLayoutPane>,
}

impl HerdrTabLayout {
    pub fn pane_rect(&self, pane_id: &str) -> Option<HerdrRect> {
        self.panes
            .iter()
            .find(|entry| entry.pane_id == pane_id)
            .map(|entry| entry.rect)
    }

    /// 확대해도 layout의 pane 좌표는 그대로라, 확대된 pane의 너비는 탭 전체 영역에서 읽는다
    pub fn is_zoomed_pane(&self, pane_id: &str) -> bool {
        self.zoomed && self.focused_pane_id.as_deref() == Some(pane_id)
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct HerdrLayoutResult {
    pub layout: HerdrTabLayout,
}

#[derive(Debug, Deserialize)]
pub(super) struct HerdrWorkspace {
    pub workspace_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct HerdrWorkspaceList {
    pub workspaces: Vec<HerdrWorkspace>,
}

impl HerdrWorkspaceList {
    pub fn contains(&self, workspace_id: &str) -> bool {
        self.workspaces
            .iter()
            .any(|workspace| workspace.workspace_id == workspace_id)
    }
}

/// JS `Number.isInteger(size) && size > 0`
pub(super) fn is_positive_integer(size: Option<f64>) -> bool {
    size.is_some_and(|size| size.fract() == 0.0 && size > 0.0)
}
