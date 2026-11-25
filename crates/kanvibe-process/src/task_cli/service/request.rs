//! task CLI 요청 본문(JSON 객체)의 필드 검증. 거절 메시지는 TS와 같은 한국어 문구다.

use kanvibe_domain::task::{SessionType, TaskPriority};

use super::TaskCliError;

pub type RequestBody = serde_json::Map<String, serde_json::Value>;

pub(super) use crate::git_ref::is_safe_git_ref;

/// 없거나 null이면 `None`, 문자열이 아니면 400, 앞뒤 공백을 지운 빈 문자열도 `None`이다
pub(super) fn read_optional_string_field(
    body: &RequestBody,
    field_name: &str,
) -> Result<Option<String>, TaskCliError> {
    let value = match body.get(field_name) {
        None | Some(serde_json::Value::Null) => return Ok(None),
        Some(serde_json::Value::String(value)) => value,
        Some(_) => {
            return Err(TaskCliError::request(
                format!("{field_name}는 문자열이어야 합니다."),
                400,
            ));
        }
    };
    let trimmed = value.trim();
    Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
}

pub(super) fn read_required_string_field(
    body: &RequestBody,
    field_name: &str,
) -> Result<String, TaskCliError> {
    read_optional_string_field(body, field_name)?
        .ok_or_else(|| TaskCliError::request(format!("{field_name}는 필수입니다."), 400))
}

pub(super) fn read_git_ref_field(
    body: &RequestBody,
    field_name: &str,
) -> Result<String, TaskCliError> {
    let value = read_required_string_field(body, field_name)?;
    if !is_safe_git_ref(&value) {
        return Err(TaskCliError::request(
            format!("{field_name}에 쓸 수 없는 문자가 있습니다: {value}"),
            400,
        ));
    }
    Ok(value)
}

pub(super) fn read_priority_field(
    body: &RequestBody,
) -> Result<Option<TaskPriority>, TaskCliError> {
    let Some(value) = read_optional_string_field(body, "priority")? else {
        return Ok(None);
    };
    TaskPriority::parse(&value).map(Some).ok_or_else(|| {
        TaskCliError::request(
            format!("priority 값이 올바르지 않습니다: {value} (low, medium, high 중 하나)"),
            400,
        )
    })
}

pub(super) fn read_session_type_field(
    body: &RequestBody,
) -> Result<Option<SessionType>, TaskCliError> {
    let Some(value) = read_optional_string_field(body, "sessionType")? else {
        return Ok(None);
    };
    SessionType::parse(&value).map(Some).ok_or_else(|| {
        TaskCliError::request(format!("sessionType 값이 올바르지 않습니다: {value}"), 400)
    })
}
