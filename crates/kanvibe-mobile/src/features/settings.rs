//! PC 설정·세션 일괄 변경·백그라운드 동기화와 검토 알림(TS `mobileSettingsService`).
//! 설정 변경은 모든 값을 먼저 검증하고 하나라도 틀리면 아무것도 쓰지 않는다.

use std::collections::HashSet;

use kanvibe_domain::background_sync::BackgroundTaskSyncStatus;
use kanvibe_domain::board_sort::{
    BoardSortDirection, BoardSortField, BoardSortKey, BoardSortPreference,
};
use kanvibe_domain::notifications::{AppNotificationAction, BackgroundSyncReviewPayload};
use kanvibe_domain::task::SessionType;
use kanvibe_services::background_task_sync::BackgroundTaskSyncRunResult;
use kanvibe_services::kanban::{SessionTypeChangeResult, SessionTypeChangeTargets};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::MobileFeatures;
use super::validation::{
    read_mobile_boolean, read_mobile_enum, read_mobile_object, read_mobile_text,
    read_mobile_text_str, read_safe_integer,
};
use crate::app_settings::open_app_settings;
use crate::error::{MobileError, MobileResult};

const SETTING_KEYS: [&str; 4] = [
    "defaultSessionType",
    "sortPreference",
    "doneAlertDismissed",
    "backgroundSync",
];
const BOARD_SORT_FIELD_COUNT: usize = 5;
const MIN_SYNC_INTERVAL_MS: i64 = 60_000;
const MAX_SYNC_INTERVAL_MS: i64 = 86_400_000;
const MAX_REVIEW_DELETE_COUNT: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileBackgroundSyncSettings {
    pub is_enabled: bool,
    pub interval_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileSettings {
    pub default_session_type: SessionType,
    pub sort_preference: BoardSortPreference,
    pub done_alert_dismissed: bool,
    pub background_sync: MobileBackgroundSyncSettings,
}

/// 검증을 마친 설정 변경. 없는 칸은 건드리지 않는다
#[derive(Default)]
struct SettingsUpdate {
    session_type: Option<SessionType>,
    sort_preference: Option<BoardSortPreference>,
    should_dismiss_done_alert: bool,
    background_sync: Option<MobileBackgroundSyncSettings>,
}

/// 열거형 이름을 serde 이름 그대로 읽는다
fn parse_serde_name<T: DeserializeOwned>(value: &str) -> Option<T> {
    serde_json::from_value(Value::String(value.to_string())).ok()
}

fn read_sort_preference(input: &Value) -> MobileResult<BoardSortPreference> {
    let body = read_mobile_object(input)?;
    let Some(submitted_keys) = body
        .get("keys")
        .and_then(Value::as_array)
        .filter(|keys| keys.len() <= BOARD_SORT_FIELD_COUNT)
    else {
        return Err(MobileError::feature(400, "정렬 값이 올바르지 않습니다"));
    };
    let mut keys = Vec::with_capacity(submitted_keys.len());
    for value in submitted_keys {
        let key = read_mobile_object(value)?;
        keys.push(BoardSortKey {
            field: read_mobile_enum(
                parse_serde_name::<BoardSortField>,
                key.get("field"),
                "정렬 기준",
            )?,
            direction: read_mobile_enum(
                parse_serde_name::<BoardSortDirection>,
                key.get("direction"),
                "정렬 방향",
            )?,
        });
    }
    let unique_fields: HashSet<BoardSortField> = keys.iter().map(|key| key.field).collect();
    if unique_fields.len() != keys.len() {
        return Err(MobileError::feature(400, "정렬 기준이 중복되었습니다"));
    }
    Ok(BoardSortPreference { keys })
}

fn read_background_sync(input: &Value) -> MobileResult<MobileBackgroundSyncSettings> {
    let settings = read_mobile_object(input)?;
    let is_enabled = read_mobile_boolean(settings.get("isEnabled"), "동기화 사용")?;
    let interval_ms = read_safe_integer(settings.get("intervalMs"))
        .filter(|interval| (MIN_SYNC_INTERVAL_MS..=MAX_SYNC_INTERVAL_MS).contains(interval))
        .ok_or_else(|| MobileError::feature(400, "동기화 주기는 1분에서 1440분 사이여야 합니다"))?;
    Ok(MobileBackgroundSyncSettings {
        is_enabled,
        interval_ms,
    })
}

fn read_settings_update(body: &Map<String, Value>) -> MobileResult<SettingsUpdate> {
    if body.is_empty() || body.keys().any(|key| !SETTING_KEYS.contains(&key.as_str())) {
        return Err(MobileError::feature(400, "지원하지 않는 설정입니다"));
    }
    let mut update = SettingsUpdate::default();
    if let Some(value) = body.get("defaultSessionType") {
        update.session_type = Some(read_mobile_enum(
            SessionType::parse,
            Some(value),
            "기본 세션",
        )?);
    }
    if let Some(value) = body.get("sortPreference") {
        update.sort_preference = Some(read_sort_preference(value)?);
    }
    if let Some(value) = body.get("doneAlertDismissed") {
        if !read_mobile_boolean(Some(value), "Done 안내")? {
            return Err(MobileError::feature(
                400,
                "Done 안내 숨김은 켜기만 지원합니다",
            ));
        }
        update.should_dismiss_done_alert = true;
    }
    if let Some(value) = body.get("backgroundSync") {
        update.background_sync = Some(read_background_sync(value)?);
    }
    Ok(update)
}

impl MobileFeatures {
    pub fn get_mobile_settings(&self) -> MobileResult<MobileSettings> {
        let settings = open_app_settings(&self.database);
        Ok(MobileSettings {
            default_session_type: settings.get_default_session_type()?,
            sort_preference: settings.get_board_sort_preference()?,
            done_alert_dismissed: settings.get_done_alert_dismissed()?,
            background_sync: MobileBackgroundSyncSettings {
                is_enabled: settings.get_background_sync_enabled()?,
                interval_ms: settings.get_background_sync_interval_ms()?,
            },
        })
    }

    pub fn update_mobile_settings(&self, input: &Value) -> MobileResult<MobileSettings> {
        let update = read_settings_update(read_mobile_object(input)?)?;
        let settings = open_app_settings(&self.database);
        if let Some(session_type) = update.session_type {
            settings.set_default_session_type(session_type)?;
        }
        if let Some(sort_preference) = &update.sort_preference {
            settings.set_board_sort_preference(sort_preference)?;
        }
        if update.should_dismiss_done_alert {
            settings.dismiss_done_alert()?;
        }
        if let Some(background_sync) = update.background_sync {
            settings.set_background_sync_interval_ms(background_sync.interval_ms)?;
            settings.set_background_sync_enabled(background_sync.is_enabled)?;
        }
        self.get_mobile_settings()
    }

    pub fn get_mobile_session_change_targets(
        &self,
        session_type: Option<&str>,
    ) -> MobileResult<SessionTypeChangeTargets> {
        let value = session_type.map(|text| Value::String(text.to_string()));
        let session_type = read_mobile_enum(SessionType::parse, value.as_ref(), "세션 종류")?;
        Ok(self.tasks.get_session_type_change_targets(session_type)?)
    }

    /// 대상 호스트 가운데 하나라도 새 세션 도구가 없으면 아무 태스크도 바꾸지 않는다
    pub fn change_all_mobile_task_sessions(
        &self,
        input: &Value,
    ) -> MobileResult<SessionTypeChangeResult> {
        let body = read_mobile_object(input)?;
        let session_type =
            read_mobile_enum(SessionType::parse, body.get("sessionType"), "세션 종류")?;
        let should_close =
            read_mobile_boolean(body.get("shouldCloseExistingSessions"), "기존 세션 종료")?;
        let targets = self.tasks.get_session_type_change_targets(session_type)?;
        for host in &targets.ssh_hosts {
            let dependency = self
                .session_dependencies
                .get_session_dependency_status(session_type, host.as_deref())?;
            if !dependency.is_available {
                return Err(MobileError::feature(
                    409,
                    "데스크탑에서 모든 대상의 세션 도구를 설치해 주세요",
                ));
            }
        }
        Ok(self
            .tasks
            .change_all_task_session_types(session_type, should_close)?)
    }

    pub fn run_mobile_sync(&self) -> MobileResult<BackgroundTaskSyncRunResult> {
        Ok(self.background_sync.run_background_task_sync_now()?)
    }

    pub fn get_mobile_sync_status(&self) -> BackgroundTaskSyncStatus {
        self.background_sync.get_background_task_sync_status()
    }

    pub fn start_mobile_sync(&self) -> BackgroundTaskSyncStatus {
        self.background_sync.start_background_task_sync_now()
    }

    pub fn get_mobile_sync_review(
        &self,
        notification_id: &str,
    ) -> MobileResult<BackgroundSyncReviewPayload> {
        let notification_id = read_mobile_text_str(notification_id, "알림", 128)?;
        let notification = self
            .notifications
            .get_notification_by_id(&notification_id)?
            .ok_or_else(|| MobileError::feature(404, "없는 알림입니다"))?;
        match notification.action.flatten() {
            Some(AppNotificationAction::BackgroundSyncReview { payload }) => Ok(payload),
            None => Err(MobileError::feature(409, "동기화 검토 알림이 아닙니다")),
        }
    }

    /// 검토 알림이 가리키는 태스크만, 검토 뒤로 브랜치나 PR이 바뀌지 않았을 때만 지운다
    pub fn delete_mobile_sync_review_tasks(
        &self,
        notification_id: &str,
        input: &Value,
    ) -> MobileResult<Vec<String>> {
        let task_ids = read_review_task_ids(read_mobile_object(input)?)?;
        let review = self.get_mobile_sync_review(notification_id)?;
        for task_id in &task_ids {
            self.verify_review_task_unchanged(task_id, &review)?;
        }
        Ok(self.tasks.delete_tasks(&task_ids)?)
    }

    fn verify_review_task_unchanged(
        &self,
        task_id: &str,
        review: &BackgroundSyncReviewPayload,
    ) -> MobileResult<()> {
        let Some(merged_pr) = review
            .merged_pull_requests
            .iter()
            .find(|entry| entry.task_id == task_id)
        else {
            return Err(MobileError::feature(
                400,
                "검토 목록에 있는 태스크만 삭제할 수 있습니다",
            ));
        };
        let task = self.require_mobile_task(task_id)?.task;
        let is_branch_changed = task.branch_name.as_deref() != Some(merged_pr.branch_name.as_str());
        let is_pr_changed = task
            .pr_url
            .as_deref()
            .is_some_and(|pr_url| !pr_url.is_empty() && pr_url != merged_pr.pr_url);
        if is_branch_changed || is_pr_changed {
            return Err(MobileError::feature(
                409,
                "태스크가 변경되었습니다. 최신 검토 내용을 확인해 주세요",
            ));
        }
        Ok(())
    }

    /// 존재하는 태스크인지 먼저 확인한 뒤 그 태스크 알림만 읽음 처리한다
    pub fn mark_mobile_task_notifications_read(&self, task_id: &str) -> MobileResult<usize> {
        self.require_mobile_task(task_id)?;
        Ok(self.notifications.mark_task_notifications_read(task_id)?)
    }
}

/// 최대 100개, 겹친 ID는 처음 나온 순서로 한 번만
fn read_review_task_ids(body: &Map<String, Value>) -> MobileResult<Vec<String>> {
    let Some(submitted) = body
        .get("taskIds")
        .and_then(Value::as_array)
        .filter(|ids| ids.len() <= MAX_REVIEW_DELETE_COUNT)
    else {
        return Err(MobileError::feature(
            400,
            "선택한 태스크가 올바르지 않습니다",
        ));
    };
    let mut task_ids: Vec<String> = Vec::new();
    for value in submitted {
        let task_id = read_mobile_text(Some(value), "태스크", 128)?;
        if !task_ids.contains(&task_id) {
            task_ids.push(task_id);
        }
    }
    Ok(task_ids)
}
