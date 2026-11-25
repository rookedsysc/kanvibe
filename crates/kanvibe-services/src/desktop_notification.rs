//! 보드 이벤트를 앱 알림 목록에 남기고 OS 알림으로 띄운다(TS `desktopNotificationService.ts`).
//! OS 알림 표시는 앱이 구현하는 `DesktopNotificationPresenter`에 맡기고, 여기서는 어떤 이벤트를 어떻게 남길지만 정한다.

use std::sync::Arc;

use kanvibe_db::Database;
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_domain::notifications::{AppNotification, DesktopNotificationPayload};
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::task_notifications::{
    HookStatusTargetMissingNotification, TaskStatusNotification,
    build_background_sync_review_notification, build_hook_status_target_missing_notification,
    build_task_status_notification,
};

use crate::board_notifier::BoardEvent;
use crate::error::ServiceResult;
use crate::notification_store::NotificationStore;

/// 알림을 눌렀을 때 부르는 처리. 표시 구현은 클릭마다 한 번 부른다
pub type NotificationClick = Box<dyn FnOnce() + Send>;

/// OS 알림 표시(TS Electron `Notification`). 아이콘처럼 표시에만 쓰는 값은 구현이 갖는다
pub trait DesktopNotificationPresenter: Send + Sync {
    fn is_supported(&self) -> bool;
    /// 소리 있는 알림을 띄운다
    fn show(&self, notification: &AppNotification, on_click: NotificationClick);
}

pub type NotificationsChangedHandler = Arc<dyn Fn() + Send + Sync>;
pub type NotificationClickHandler = Arc<dyn Fn(&AppNotification) + Send + Sync>;

/// 알림 목록이 바뀌었을 때와 알림을 눌렀을 때 화면이 할 일
#[derive(Clone, Default)]
pub struct NotificationDeliveryCallbacks {
    pub on_notifications_changed: Option<NotificationsChangedHandler>,
    pub on_notification_click: Option<NotificationClickHandler>,
}

impl NotificationDeliveryCallbacks {
    fn notify_changed(&self) {
        if let Some(on_changed) = &self.on_notifications_changed {
            on_changed();
        }
    }
}

pub struct DesktopNotificationService {
    database: Arc<Database>,
    store: Arc<NotificationStore>,
    presenter: Arc<dyn DesktopNotificationPresenter>,
}

impl DesktopNotificationService {
    pub fn new(
        database: Arc<Database>,
        store: Arc<NotificationStore>,
        presenter: Arc<dyn DesktopNotificationPresenter>,
    ) -> Self {
        Self {
            database,
            store,
            presenter,
        }
    }

    /// 알림 목록에 남기고 새 알림이면 OS 알림을 띄운다. 이미 있던 알림이면 처리 완료로 본다.
    /// OS 알림을 지원하지 않을 때만 `false`를 돌려 화면이 대신 보여 주게 한다
    pub fn deliver_desktop_notification(
        &self,
        payload: &DesktopNotificationPayload,
        callbacks: &NotificationDeliveryCallbacks,
    ) -> ServiceResult<bool> {
        let creation = self.store.create_notification(payload)?;
        if !creation.is_created {
            return Ok(true);
        }
        callbacks.notify_changed();
        if !self.presenter.is_supported() {
            return Ok(false);
        }
        let notification = creation.notification;
        let on_click = self.build_click_handler(&notification, callbacks);
        self.presenter.show(&notification, on_click);
        Ok(true)
    }

    /// 누르면 읽음 처리 후 목록 변경을 알리고, 화면 이동은 따로 시도한다. 이동 실패는 무시한다
    fn build_click_handler(
        &self,
        notification: &AppNotification,
        callbacks: &NotificationDeliveryCallbacks,
    ) -> NotificationClick {
        let store = Arc::clone(&self.store);
        let callbacks = callbacks.clone();
        let notification = notification.clone();
        Box::new(move || {
            if store.mark_notification_read(&notification.id).is_ok() {
                callbacks.notify_changed();
            }
            if let Some(on_click) = &callbacks.on_notification_click {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    on_click(&notification)
                }));
            }
        })
    }

    /// 보드 이벤트를 알림으로 바꾼다. 상태 변경과 hook 대상 누락만 설정에 따라 OS 알림이 되고,
    /// background sync 검토는 설정과 상관없이 알림 목록에만 조용히 남는다
    pub fn deliver_board_event_notification(
        &self,
        event: &BoardEvent,
        locale: &str,
        callbacks: &NotificationDeliveryCallbacks,
    ) -> ServiceResult<bool> {
        if let BoardEvent::BackgroundSyncReviewNeeded(review) = event {
            let prepared = build_background_sync_review_notification(review, locale);
            if self
                .store
                .create_notification(&prepared.desktop_payload)?
                .is_created
            {
                callbacks.notify_changed();
            }
            return Ok(true);
        }
        let Some((status, payload)) = notification_candidate(event, locale) else {
            return Ok(false);
        };
        let settings = AppSettingsService::new(&self.database, ShortcutPlatform::current())
            .get_notification_settings()?;
        if !settings.is_enabled || !settings.enabled_statuses.contains(&status) {
            return Ok(false);
        }
        self.deliver_desktop_notification(&payload, callbacks)
    }
}

/// OS 알림 후보가 되는 이벤트와 설정에서 확인할 상태
fn notification_candidate(
    event: &BoardEvent,
    locale: &str,
) -> Option<(String, DesktopNotificationPayload)> {
    match event {
        BoardEvent::TaskStatusChanged(changed) => {
            let prepared = build_task_status_notification(&TaskStatusNotification {
                project_name: changed.project_name.clone(),
                branch_name: changed.branch_name.clone(),
                task_title: changed.task_title.clone(),
                description: changed.description.clone(),
                new_status: changed.new_status.clone(),
                task_id: changed.task_id.clone(),
                locale: locale.to_string(),
            });
            Some((changed.new_status.clone(), prepared.desktop_payload))
        }
        BoardEvent::HookStatusTargetMissing(missing) => {
            let prepared = build_hook_status_target_missing_notification(
                &HookStatusTargetMissingNotification {
                    task_id: missing.task_id.clone(),
                    requested_status: missing.requested_status.clone(),
                    locale: locale.to_string(),
                },
            );
            Some((missing.requested_status.clone(), prepared.desktop_payload))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
