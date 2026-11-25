//! 모바일 기능 경로가 받는 값 검증(TS `mobileFeatureValidation.ts`). 본문은 기기가 보낸 JSON 그대로 받아,
//! 모양이 맞지 않으면 쓰기 전에 `MobileFeatureError`로 거절한다.

use kanvibe_ai::javascript::{js_trim, utf16_length};
use kanvibe_domain::project::Project;
use kanvibe_services::kanban::TaskWithProject;
use serde_json::{Map, Value};

use super::MobileFeatures;
use crate::error::{MobileError, MobileResult};

pub(crate) const DEFAULT_TEXT_MAX_LENGTH: usize = 4096;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

pub(crate) fn invalid_value(label: &str) -> MobileError {
    MobileError::feature(400, format!("{label} 값이 올바르지 않습니다"))
}

pub(crate) fn read_mobile_object(value: &Value) -> MobileResult<&Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| MobileError::feature(400, "요청 값이 올바르지 않습니다"))
}

fn is_control_character(character: char) -> bool {
    matches!(character, '\u{0}'..='\u{1f}' | '\u{7f}')
}

/// 비어 있지 않고 길이(UTF-16 단위) 안이며 제어 문자가 없는 문자열. 앞뒤 공백을 뗀 값을 돌려준다
pub(crate) fn read_mobile_text_str(
    text: &str,
    label: &str,
    max_length: usize,
) -> MobileResult<String> {
    let trimmed = js_trim(text);
    let is_valid = !trimmed.is_empty()
        && utf16_length(text) <= max_length
        && !text.chars().any(is_control_character);
    if !is_valid {
        return Err(invalid_value(label));
    }
    Ok(trimmed.to_string())
}

pub(crate) fn read_mobile_text(
    value: Option<&Value>,
    label: &str,
    max_length: usize,
) -> MobileResult<String> {
    let text = value
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_value(label))?;
    read_mobile_text_str(text, label, max_length)
}

pub(crate) fn read_mobile_enum<T>(
    parse: impl Fn(&str) -> Option<T>,
    value: Option<&Value>,
    label: &str,
) -> MobileResult<T> {
    value
        .and_then(Value::as_str)
        .and_then(parse)
        .ok_or_else(|| invalid_value(label))
}

pub(crate) fn read_mobile_boolean(value: Option<&Value>, label: &str) -> MobileResult<bool> {
    value
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid_value(label))
}

/// JS `Number.isSafeInteger`
pub(crate) fn is_safe_integer(number: f64) -> bool {
    number.fract() == 0.0 && number.abs() <= MAX_SAFE_INTEGER
}

/// 안전한 정수인 숫자 값. 숫자가 아니거나 소수면 `None`
pub(crate) fn read_safe_integer(value: Option<&Value>) -> Option<i64> {
    let number = value?.as_f64()?;
    is_safe_integer(number).then_some(number as i64)
}

impl MobileFeatures {
    pub(crate) fn require_mobile_task(&self, task_id: &str) -> MobileResult<TaskWithProject> {
        let task_id = read_mobile_text_str(task_id, "태스크", 128)?;
        self.tasks
            .get_task_by_id(&task_id)?
            .ok_or_else(|| MobileError::feature(404, "없는 태스크입니다"))
    }

    pub(crate) fn require_mobile_project(&self, project_id: &str) -> MobileResult<Project> {
        let project_id = read_mobile_text_str(project_id, "프로젝트", 128)?;
        self.projects
            .get_project_by_id(&project_id)?
            .ok_or_else(|| MobileError::feature(404, "없는 프로젝트입니다"))
    }
}
