//! 앱 안 알림 목록(TS `desktop/main/notificationStore.ts`). 알림 전체를 `app_settings.app_notifications` 한 칸에
//! JSON 배열로 저장하므로, 생성과 모든 읽음 처리는 읽기-쓰기를 한 잠금 안에서 차례로 한다.
//! 최근 100개만 남기고, 같은 알림이 4초 안에 다시 오면 새로 만들지 않는다.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, SecondsFormat, Utc};
use kanvibe_db::Database;
use kanvibe_db::repositories::app_settings::{find_app_setting, save_app_setting};
use kanvibe_domain::notifications::{AppNotification, DesktopNotificationPayload};
use serde_json::Value;

use crate::error::{ServiceError, ServiceResult};

const APP_NOTIFICATIONS_KEY: &str = "app_notifications";
const MAX_NOTIFICATIONS: usize = 100;
const DEDUPE_WINDOW_MS: i64 = 4000;

/// 알림 목록이 저장되는 설정 칸. 실제 앱은 DB를 쓰고 테스트는 실패를 흉내 내는 저장소를 쓴다
pub trait NotificationListStorage: Send + Sync {
    fn read(&self) -> ServiceResult<Option<String>>;
    fn write(&self, serialized: &str) -> ServiceResult<()>;
}

pub struct DatabaseNotificationListStorage {
    database: Arc<Database>,
}

impl DatabaseNotificationListStorage {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

impl NotificationListStorage for DatabaseNotificationListStorage {
    fn read(&self) -> ServiceResult<Option<String>> {
        let setting = find_app_setting(&self.database.connection(), APP_NOTIFICATIONS_KEY)?;
        Ok(setting.map(|setting| setting.value))
    }

    fn write(&self, serialized: &str) -> ServiceResult<()> {
        save_app_setting(
            &self.database.connection(),
            APP_NOTIFICATIONS_KEY,
            serialized,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationCreation {
    pub is_created: bool,
    pub notification: AppNotification,
}

type ChangeListener = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct ChangeListeners {
    next_id: u64,
    listeners: Vec<(u64, ChangeListener)>,
}

/// 변경 구독 해제 핸들. 버리면 구독이 끝난다
pub struct NotificationChangeSubscription {
    listeners: Arc<Mutex<ChangeListeners>>,
    listener_id: u64,
}

impl Drop for NotificationChangeSubscription {
    fn drop(&mut self) {
        lock(&self.listeners)
            .listeners
            .retain(|(id, _)| *id != self.listener_id);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct NotificationStore {
    storage: Arc<dyn NotificationListStorage>,
    now_ms: Box<dyn Fn() -> i64 + Send + Sync>,
    /// 읽기-쓰기 순서를 지키는 잠금. 최근 생성 키도 같은 잠금 안에서 본다
    recent_notification_keys: Mutex<HashMap<String, i64>>,
    listeners: Arc<Mutex<ChangeListeners>>,
}

/// `createdAt` 값. JS `Date#toISOString()`과 같은 밀리초 UTC 형식이다
fn iso_timestamp(epoch_ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(epoch_ms)
        .unwrap_or_default()
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn relative_path(payload: &DesktopNotificationPayload) -> String {
    if let Some(task_id) = payload.task_id.as_deref().filter(|id| !id.is_empty()) {
        return format!("/{}/task/{task_id}", payload.locale);
    }
    match payload
        .relative_path
        .as_deref()
        .filter(|path| !path.is_empty())
    {
        Some(path) => path.to_string(),
        None => format!("/{}", payload.locale),
    }
}

fn dedupe_key(payload: &DesktopNotificationPayload) -> String {
    if let Some(key) = payload.dedupe_key.as_deref().filter(|key| !key.is_empty()) {
        return key.to_string();
    }
    [
        payload.title.as_str(),
        payload.body.as_str(),
        payload.task_id.as_deref().unwrap_or_default(),
        payload.locale.as_str(),
    ]
    .join("::")
}

/// 저장된 배열을 읽는다. 배열이 아니거나 JSON이 아니면 빈 목록이고, 모양이 맞지 않는 항목은 건너뛴다
fn parse_notifications(serialized: Option<&str>) -> Vec<AppNotification> {
    let Some(Value::Array(entries)) = serialized
        .filter(|value| !value.is_empty())
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
    else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|entry| serde_json::from_value(entry).ok())
        .collect()
}

impl NotificationStore {
    pub fn new(storage: Arc<dyn NotificationListStorage>) -> Self {
        Self::with_clock(storage, || Utc::now().timestamp_millis())
    }

    pub fn with_clock(
        storage: Arc<dyn NotificationListStorage>,
        now_ms: impl Fn() -> i64 + Send + Sync + 'static,
    ) -> Self {
        Self {
            storage,
            now_ms: Box::new(now_ms),
            recent_notification_keys: Mutex::new(HashMap::new()),
            listeners: Arc::new(Mutex::new(ChangeListeners::default())),
        }
    }

    /// 저장이 끝나 목록이 실제로 바뀐 뒤에만 부른다
    pub fn subscribe_to_notification_changes(
        &self,
        listener: impl Fn() + Send + Sync + 'static,
    ) -> NotificationChangeSubscription {
        let mut listeners = lock(&self.listeners);
        let listener_id = listeners.next_id;
        listeners.next_id += 1;
        listeners.listeners.push((listener_id, Arc::new(listener)));
        NotificationChangeSubscription {
            listeners: Arc::clone(&self.listeners),
            listener_id,
        }
    }

    fn read_notifications(&self) -> ServiceResult<Vec<AppNotification>> {
        Ok(parse_notifications(self.storage.read()?.as_deref()))
    }

    /// 최근 100개만 저장하고 구독자에게 알린다. 한 구독자의 실패가 다른 구독자 전달을 막지 않는다
    fn write_notifications(&self, notifications: &[AppNotification]) -> ServiceResult<()> {
        let retained = &notifications[..notifications.len().min(MAX_NOTIFICATIONS)];
        let serialized = serde_json::to_string(retained)
            .map_err(|error| ServiceError::new(error.to_string()))?;
        self.storage.write(&serialized)?;
        let listeners: Vec<ChangeListener> = lock(&self.listeners)
            .listeners
            .iter()
            .map(|(_, listener)| Arc::clone(listener))
            .collect();
        for listener in listeners {
            let delivery = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| listener()));
            if delivery.is_err() {
                eprintln!("[kanvibe] Notification change delivery failed");
            }
        }
        Ok(())
    }

    /// 최신 알림이 먼저 오도록 생성 시각의 역순으로 돌려준다
    pub fn list_notifications(&self) -> ServiceResult<Vec<AppNotification>> {
        let mut notifications = self.read_notifications()?;
        notifications.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(notifications)
    }

    pub fn get_notification_by_id(&self, id: &str) -> ServiceResult<Option<AppNotification>> {
        let notifications = self.read_notifications()?;
        Ok(notifications.into_iter().find(|entry| entry.id == id))
    }

    pub fn create_notification(
        &self,
        payload: &DesktopNotificationPayload,
    ) -> ServiceResult<NotificationCreation> {
        let mut recent_keys = lock(&self.recent_notification_keys);
        let notifications = self.read_notifications()?;
        let now = (self.now_ms)();
        let dedupe_key = dedupe_key(payload);
        recent_keys.retain(|_, created_at| now - *created_at <= DEDUPE_WINDOW_MS);
        let duplicate = recent_keys
            .contains_key(&dedupe_key)
            .then(|| {
                notifications
                    .iter()
                    .find(|entry| entry.dedupe_key == dedupe_key)
            })
            .flatten();
        if let Some(existing) = duplicate {
            return Ok(NotificationCreation {
                is_created: false,
                notification: existing.clone(),
            });
        }
        let notification = AppNotification {
            id: uuid::Uuid::new_v4().to_string(),
            title: payload.title.clone(),
            body: payload.body.clone(),
            task_id: payload.task_id.clone().filter(|id| !id.is_empty()),
            relative_path: relative_path(payload),
            locale: payload.locale.clone(),
            is_read: false,
            created_at: iso_timestamp(now),
            dedupe_key: dedupe_key.clone(),
            action: Some(payload.action.clone()),
        };
        let mut next = vec![notification.clone()];
        next.extend(notifications);
        self.write_notifications(&next)?;
        recent_keys.insert(dedupe_key, now);
        Ok(NotificationCreation {
            is_created: true,
            notification,
        })
    }

    /// 없는 알림이면 `None`, 이미 읽은 알림이면 다시 쓰지 않고 그대로 돌려준다
    pub fn mark_notification_read(&self, id: &str) -> ServiceResult<Option<AppNotification>> {
        let _write_order = lock(&self.recent_notification_keys);
        let mut notifications = self.read_notifications()?;
        let Some(index) = notifications.iter().position(|entry| entry.id == id) else {
            return Ok(None);
        };
        if notifications[index].is_read {
            return Ok(Some(notifications[index].clone()));
        }
        notifications[index].is_read = true;
        let updated = notifications[index].clone();
        self.write_notifications(&notifications)?;
        Ok(Some(updated))
    }

    /// 특정 task에 연결된 미읽음 알림을 모두 읽음 처리하고, 실제로 바뀐 알림 수를 돌려준다
    pub fn mark_task_notifications_read(&self, task_id: &str) -> ServiceResult<usize> {
        let _write_order = lock(&self.recent_notification_keys);
        let mut notifications = self.read_notifications()?;
        let mut updated_count = 0;
        for notification in notifications
            .iter_mut()
            .filter(|entry| entry.task_id.as_deref() == Some(task_id) && !entry.is_read)
        {
            notification.is_read = true;
            updated_count += 1;
        }
        if updated_count > 0 {
            self.write_notifications(&notifications)?;
        }
        Ok(updated_count)
    }

    pub fn mark_all_notifications_read(&self) -> ServiceResult<()> {
        let _write_order = lock(&self.recent_notification_keys);
        let mut notifications = self.read_notifications()?;
        if notifications.iter().all(|entry| entry.is_read) {
            return Ok(());
        }
        for notification in &mut notifications {
            notification.is_read = true;
        }
        self.write_notifications(&notifications)
    }
}

#[cfg(test)]
mod tests;
