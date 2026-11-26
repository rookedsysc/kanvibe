//! 모바일 알림함과 데스크탑과 공유하는 알림 설정. 저장소는 데스크탑 알림 목록 그대로라 읽음 처리가 양쪽에 같이 보인다.

use std::sync::Arc;

use kanvibe_domain::notifications::{AppNotification, AppNotificationAction};
use kanvibe_domain::task::TaskStatus;
use kanvibe_services::notification_store::NotificationChangeSubscription;
use serde::Serialize;
use serde_json::Value;

use super::MobileBridge;
use crate::app_settings::open_app_settings;
use crate::error::{MobileError, MobileResult};

/// 모바일에 보내는 알림. 이동 경로와 검토 payload는 숨기고 동작 종류만 싣는다
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileNotification {
    pub id: String,
    pub title: String,
    pub body: String,
    pub task_id: Option<String>,
    pub is_read: bool,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileNotificationSettings {
    pub is_enabled: bool,
    pub enabled_statuses: Vec<String>,
}

fn action_type(action: &AppNotificationAction) -> &'static str {
    match action {
        AppNotificationAction::BackgroundSyncReview { .. } => "background-sync-review",
    }
}

fn to_mobile_notification(notification: AppNotification) -> MobileNotification {
    let action_type = notification
        .action
        .as_ref()
        .and_then(Option::as_ref)
        .map(|action| action_type(action).to_string());
    MobileNotification {
        id: notification.id,
        title: notification.title,
        body: notification.body,
        task_id: notification.task_id,
        is_read: notification.is_read,
        created_at: notification.created_at,
        action_type,
    }
}

/// 켜짐 여부는 불리언, 상태 목록은 보드 상태 이름만 받는다
fn read_notification_settings_input(input: &Value) -> MobileResult<(bool, Vec<String>)> {
    let invalid = || MobileError::task_input("알림 상태 설정이 올바르지 않습니다.");
    let is_enabled = input
        .get("isEnabled")
        .and_then(Value::as_bool)
        .ok_or_else(invalid)?;
    let submitted = input
        .get("enabledStatuses")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let mut statuses: Vec<String> = Vec::new();
    for status in submitted {
        let status = status
            .as_str()
            .filter(|status| TaskStatus::parse(status).is_some())
            .ok_or_else(invalid)?;
        if !statuses.iter().any(|existing| existing == status) {
            statuses.push(status.to_string());
        }
    }
    Ok((is_enabled, statuses))
}

impl MobileBridge {
    pub fn get_mobile_notifications(&self) -> MobileResult<Vec<MobileNotification>> {
        let notifications = self.notifications.list_notifications()?;
        Ok(notifications
            .into_iter()
            .map(to_mobile_notification)
            .collect())
    }

    pub fn get_mobile_notification_settings(&self) -> MobileResult<MobileNotificationSettings> {
        let settings = open_app_settings(&self.database).get_notification_settings()?;
        Ok(MobileNotificationSettings {
            is_enabled: settings.is_enabled,
            enabled_statuses: settings.enabled_statuses,
        })
    }

    /// 잘못된 값은 저장 전에 거절하고, 겹친 상태는 처음 나온 순서로 한 번만 남긴다
    pub fn update_mobile_notification_settings(
        &self,
        input: &Value,
    ) -> MobileResult<MobileNotificationSettings> {
        let (is_enabled, statuses) = read_notification_settings_input(input)?;
        let settings = open_app_settings(&self.database);
        settings.set_notification_statuses(&statuses)?;
        settings.set_notification_enabled(is_enabled)?;
        self.get_mobile_notification_settings()
    }

    /// 없는 알림이면 `None`
    pub fn mark_mobile_notification_read(
        &self,
        notification_id: &str,
    ) -> MobileResult<Option<MobileNotification>> {
        let notification = self.notifications.mark_notification_read(notification_id)?;
        Ok(notification.map(to_mobile_notification))
    }

    /// 데스크탑 상세 화면이 포커스를 받을 때와 같은 저장소 경로로 task 알림을 읽음 처리하고 바꾼 개수를 돌려준다
    pub fn mark_mobile_task_notifications_read(&self, task_id: &str) -> MobileResult<usize> {
        Ok(self.notifications.mark_task_notifications_read(task_id)?)
    }

    pub fn mark_all_mobile_notifications_read(&self) -> MobileResult<()> {
        Ok(self.notifications.mark_all_notifications_read()?)
    }

    /// 저장이 끝나 알림 목록이 바뀌면 부른다. 핸들을 버리면 구독이 풀린다
    pub fn on_mobile_notifications_changed(
        &self,
        listener: Arc<dyn Fn() + Send + Sync>,
    ) -> NotificationChangeSubscription {
        self.notifications
            .subscribe_to_notification_changes(move || listener())
    }
}
