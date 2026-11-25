//! `.kanvibe/` 상태 파일 내용의 생성과 해석. 생성 결과는 TS `JSON.stringify(payload, null, 2) + "\n"`과 같다.

use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use kanvibe_domain::task::{SessionType, TaskPriority, TaskStatus};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use crate::hooks::socket_path::{is_valid_hook_client_id, is_valid_host_namespace};

pub const KANVIBE_STATE_SCHEMA_VERSION: u32 = 1;

/// `#RRGGBB` 형태의 프로젝트 색상만 허용한다
static PROJECT_COLOR_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^#[0-9a-fA-F]{6}$").expect("유효한 정규식"));

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KanvibeTaskState {
    pub schema_version: u32,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// 프로젝트 단위 공유 상태. root task priority와 색상을 task 상태 파일에서 분리해 hook(status.json 기록)과
/// KanVibe client(project.json 기록)가 서로의 값을 덮어쓸 수 없게 한다.
/// `priority`의 바깥 `None`은 기록 없음, `Some(None)`은 명시적 null이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KanvibeProjectState {
    pub schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<Option<TaskPriority>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// 데스크탑이 정한 task 터미널 세션. 같은 저장소를 보는 다른 KanVibe가 이 값을 따라 세션을 맞춘다
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KanvibeTaskSession {
    pub session_type: SessionType,
    pub session_name: String,
}

/// task 메타데이터 공유 상태. hook이 통째로 재작성하는 status.json과 파일을 분리해
/// agent가 상태를 기록해도 사용자가 남긴 설명과 priority가 지워지지 않게 한다.
/// description이 `Some(None)`(null)이면 다른 기기에서도 설명을 지우라는 뜻이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KanvibeTaskDescription {
    pub schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<Option<TaskPriority>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<KanvibeTaskSession>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HookTargetTransport {
    Local,
    Ssh,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KanvibeHookTarget {
    pub url: String,
    pub task_id: String,
    /// 이 client가 task CLI 요청을 받을 때 확인하는 task 단위 자격 증명. 파일을 읽을 수 있는 사용자만 쓸 수 있다
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// 원격 hook·CLI의 데스크탑 라우팅 ID. 이전 전송에서는 소켓 이름에도 쓴다
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<HookTargetTransport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_host_namespace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KanvibeTargetsState {
    pub schema_version: u32,
    pub targets: Vec<KanvibeHookTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Node `new Date().toISOString()`과 같은 `YYYY-MM-DDTHH:MM:SS.mmmZ`
pub(super) fn iso_timestamp_now() -> String {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_milliseconds = elapsed.as_millis() as i64;
    let days = total_milliseconds.div_euclid(86_400_000);
    let day_milliseconds = total_milliseconds.rem_euclid(86_400_000);
    let (year, month, day) = civil_date_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        day_milliseconds / 3_600_000,
        day_milliseconds / 60_000 % 60,
        day_milliseconds / 1000 % 60,
        day_milliseconds % 1000
    )
}

/// 1970-01-01부터 지난 일수를 그레고리력 날짜로 바꾼다(Howard Hinnant의 days_from_civil 역변환)
fn civil_date_from_days(days: i64) -> (i64, i64, i64) {
    let shifted_days = days + 719_468;
    let era = shifted_days.div_euclid(146_097);
    let day_of_era = shifted_days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn to_pretty_json_line(payload: &impl Serialize) -> String {
    let json =
        serde_json::to_string_pretty(payload).expect("상태 파일 내용은 항상 JSON으로 바뀐다");
    json + "\n"
}

pub fn build_kanvibe_task_state_content(status: TaskStatus) -> String {
    to_pretty_json_line(&KanvibeTaskState {
        schema_version: KANVIBE_STATE_SCHEMA_VERSION,
        status,
        updated_at: Some(iso_timestamp_now()),
    })
}

/// 바깥 `None`인 값은 파일에 쓰지 않고, `Some(None)`은 null로 쓴다
pub fn build_kanvibe_task_description_content(
    description: Option<Option<String>>,
    priority: Option<Option<TaskPriority>>,
    session: Option<KanvibeTaskSession>,
) -> String {
    to_pretty_json_line(&KanvibeTaskDescription {
        schema_version: KANVIBE_STATE_SCHEMA_VERSION,
        description,
        priority,
        session,
        updated_at: Some(iso_timestamp_now()),
    })
}

pub fn build_kanvibe_project_state_content(
    project_color: Option<String>,
    priority: Option<Option<TaskPriority>>,
) -> String {
    to_pretty_json_line(&KanvibeProjectState {
        schema_version: KANVIBE_STATE_SCHEMA_VERSION,
        project_color,
        priority,
        updated_at: Some(iso_timestamp_now()),
    })
}

pub fn build_kanvibe_targets_content(targets: &[KanvibeHookTarget]) -> String {
    let normalized_targets: Vec<Option<KanvibeHookTarget>> =
        targets.iter().map(normalize_kanvibe_target).collect();
    to_pretty_json_line(&KanvibeTargetsState {
        schema_version: KANVIBE_STATE_SCHEMA_VERSION,
        targets: deduplicate_targets_by_url(normalized_targets),
        updated_at: Some(iso_timestamp_now()),
    })
}

/// 저장된 상태 이름은 대소문자를 가리지 않는다
pub fn parse_task_status(value: &str) -> Option<TaskStatus> {
    TaskStatus::parse(&value.to_lowercase())
}

fn parse_task_status_value(value: Option<&Value>) -> Option<TaskStatus> {
    value.and_then(Value::as_str).and_then(parse_task_status)
}

pub fn parse_project_color(value: &str) -> Option<String> {
    let normalized = value.trim();
    PROJECT_COLOR_PATTERN
        .is_match(normalized)
        .then(|| normalized.to_string())
}

fn parse_project_color_value(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).and_then(parse_project_color)
}

pub fn parse_task_priority(value: &str) -> Option<TaskPriority> {
    TaskPriority::parse(value)
}

/// 키가 있고 null이거나 알려진 priority면 그 값을, 아니면 기록 없음(`None`)을 돌려준다
fn parse_priority_field(object: &serde_json::Map<String, Value>) -> Option<Option<TaskPriority>> {
    match object.get("priority")? {
        Value::Null => Some(None),
        Value::String(priority) => parse_task_priority(priority).map(Some),
        _ => None,
    }
}

fn non_empty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn parse_json_object(content: &str) -> Option<serde_json::Map<String, Value>> {
    if content.trim().is_empty() {
        return None;
    }
    match serde_json::from_str::<Value>(content).ok()? {
        Value::Object(object) => Some(object),
        _ => None,
    }
}

/// 마크다운 시절처럼 상태 이름만 적힌 파일도 읽는다
pub fn parse_kanvibe_task_state(content: &str) -> Option<KanvibeTaskState> {
    if content.trim().is_empty() {
        return None;
    }
    if let Some(status) = parse_task_status(content.trim()) {
        return Some(KanvibeTaskState {
            schema_version: KANVIBE_STATE_SCHEMA_VERSION,
            status,
            updated_at: None,
        });
    }
    let object = parse_json_object(content)?;
    let status = parse_task_status_value(object.get("status"))?;
    Some(KanvibeTaskState {
        schema_version: KANVIBE_STATE_SCHEMA_VERSION,
        status,
        updated_at: non_empty_string(object.get("updatedAt")),
    })
}

/// 이 버전이 모르는 세션 타입은 따라갈 수 없으므로 기록이 없는 것으로 본다
fn parse_kanvibe_task_session(value: Option<&Value>) -> Option<KanvibeTaskSession> {
    let object = value?.as_object()?;
    let session_type = SessionType::parse(object.get("sessionType")?.as_str()?)?;
    let session_name = non_empty_string(object.get("sessionName"))?;
    Some(KanvibeTaskSession {
        session_type,
        session_name,
    })
}

/// description이 문자열이거나 명시적 null일 때만 유효한 기록으로 본다.
/// 파일이 없거나 형식이 깨진 경우는 "정보 없음"이므로 `None`을 돌려 DB 값을 유지하게 한다.
pub fn parse_kanvibe_task_description(content: &str) -> Option<KanvibeTaskDescription> {
    let object = parse_json_object(content)?;
    let description = match object.get("description") {
        Some(Value::Null) => Some(None),
        Some(Value::String(description)) => Some(Some(description.clone())),
        _ => None,
    };
    let priority = parse_priority_field(&object);
    let session = parse_kanvibe_task_session(object.get("session"));
    if description.is_none() && priority.is_none() && session.is_none() {
        return None;
    }
    Some(KanvibeTaskDescription {
        schema_version: KANVIBE_STATE_SCHEMA_VERSION,
        description,
        priority,
        session,
        updated_at: non_empty_string(object.get("updatedAt")),
    })
}

pub fn parse_kanvibe_project_state(content: &str) -> Option<KanvibeProjectState> {
    let object = parse_json_object(content)?;
    let project_color = parse_project_color_value(object.get("projectColor"));
    let priority = parse_priority_field(&object);
    if project_color.is_none() && priority.is_none() {
        return None;
    }
    Some(KanvibeProjectState {
        schema_version: KANVIBE_STATE_SCHEMA_VERSION,
        project_color,
        priority,
        updated_at: non_empty_string(object.get("updatedAt")),
    })
}

/// 형식이 깨진 파일은 대상 없음으로 본다
pub fn parse_kanvibe_targets(content: &str) -> KanvibeTargetsState {
    let object = parse_json_object(content).unwrap_or_default();
    let raw_targets = match object.get("targets") {
        Some(Value::Array(targets)) => targets.as_slice(),
        _ => &[],
    };
    let normalized_targets: Vec<Option<KanvibeHookTarget>> =
        raw_targets.iter().map(target_from_value).collect();
    KanvibeTargetsState {
        schema_version: KANVIBE_STATE_SCHEMA_VERSION,
        targets: deduplicate_targets_by_url(normalized_targets),
        updated_at: non_empty_string(object.get("updatedAt")),
    }
}

/// 유효하지 않은 항목은 버리고 같은 url은 처음 것만 남긴다
fn deduplicate_targets_by_url(targets: Vec<Option<KanvibeHookTarget>>) -> Vec<KanvibeHookTarget> {
    let mut unique_targets: Vec<KanvibeHookTarget> = Vec::new();
    for target in targets.into_iter().flatten() {
        if !unique_targets.iter().any(|seen| seen.url == target.url) {
            unique_targets.push(target);
        }
    }
    unique_targets
}

fn normalize_kanvibe_target_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn parse_transport(value: &str) -> Option<HookTargetTransport> {
    match value {
        "local" => Some(HookTargetTransport::Local),
        "ssh" => Some(HookTargetTransport::Ssh),
        _ => None,
    }
}

/// JSON에 있는 대상 항목 하나. 전송 종류나 namespace 키가 있는데 값이 틀리면 옛 등록으로 바꾸지 않고 버린다
fn target_from_value(value: &Value) -> Option<KanvibeHookTarget> {
    let object = value.as_object()?;
    let transport = match object.get("transport") {
        None => None,
        Some(transport) => Some(parse_transport(transport.as_str()?)?),
    };
    let remote_host_namespace = match object.get("remoteHostNamespace") {
        None => None,
        Some(namespace) => Some(namespace.as_str()?.to_string()),
    };
    let raw_target = KanvibeHookTarget {
        url: object.get("url")?.as_str()?.to_string(),
        task_id: object.get("taskId")?.as_str()?.to_string(),
        token: object
            .get("token")
            .and_then(Value::as_str)
            .map(str::to_string),
        client_id: object
            .get("clientId")
            .and_then(Value::as_str)
            .map(str::to_string),
        transport,
        remote_host_namespace,
    };
    normalize_kanvibe_target(&raw_target)
}

/// 앞뒤 공백과 url 끝 `/`를 정리하고, 쓸 수 없는 client ID와 빈 토큰은 뺀다
pub(super) fn normalize_kanvibe_target(target: &KanvibeHookTarget) -> Option<KanvibeHookTarget> {
    let url = normalize_kanvibe_target_url(&target.url);
    let task_id = target.task_id.trim().to_string();
    let has_invalid_namespace = target
        .remote_host_namespace
        .as_deref()
        .is_some_and(|namespace| !is_valid_host_namespace(namespace));
    if url.is_empty() || task_id.is_empty() || has_invalid_namespace {
        return None;
    }
    let token = target
        .token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty());
    let client_id = target
        .client_id
        .as_deref()
        .filter(|client_id| is_valid_hook_client_id(client_id));
    Some(KanvibeHookTarget {
        url,
        task_id,
        token: token.map(str::to_string),
        client_id: client_id.map(str::to_string),
        transport: target.transport,
        remote_host_namespace: target.remote_host_namespace.clone(),
    })
}

/// hook 알림 대상을 client 단위로 등록한다.
/// 같은 client가 다시 설치하면 그 항목을 교체하고, 새로운 client는 뒤에 추가된다.
/// IP가 바뀐 뒤 재설치하면 url이 달라지므로 client ID가 같은 항목과, client ID 없이 같은 taskId를 가진 옛 항목도
/// 같은 client로 보고 교체한다. 옛 항목을 남기면 CLI가 그 주소로 토큰을 먼저 보낸다.
pub(super) fn upsert_kanvibe_target(
    targets: Vec<KanvibeHookTarget>,
    target: &KanvibeHookTarget,
) -> Vec<KanvibeHookTarget> {
    let Some(normalized_target) = normalize_kanvibe_target(target) else {
        return targets;
    };
    let is_same_client = |value: &KanvibeHookTarget| {
        let is_same_client_id = normalized_target.client_id.is_some()
            && (value.client_id == normalized_target.client_id
                || (value.client_id.is_none() && value.task_id == normalized_target.task_id));
        value.url == normalized_target.url || is_same_client_id
    };
    let Some(target_index) = targets.iter().position(is_same_client) else {
        let mut next_targets = targets;
        next_targets.push(normalized_target);
        return next_targets;
    };
    let mut next_targets = Vec::new();
    for (index, value) in targets.into_iter().enumerate() {
        if index == target_index {
            next_targets.push(normalized_target.clone());
        } else if !is_same_client(&value) {
            next_targets.push(value);
        }
    }
    next_targets
}

/// targets.json에 해당 client url과 taskId 쌍이 등록되어 있는지 확인한다
pub fn has_kanvibe_hook_target(content: &str, target: &KanvibeHookTarget) -> bool {
    let Some(expected_target) = normalize_kanvibe_target(target) else {
        return false;
    };
    parse_kanvibe_targets(content)
        .targets
        .iter()
        .any(|value| value.url == expected_target.url && value.task_id == expected_target.task_id)
}
