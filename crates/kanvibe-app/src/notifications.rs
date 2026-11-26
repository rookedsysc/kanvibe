//! 알림 기반. 앱 안 알림 센터(버튼 배지·목록)의 모델과, OS 알림을 띄우고 누르면 작업 상세로 보내는 경로.
//!
//! OS 알림은 `OsNotificationBackend` 뒤에 둔다. Linux 구현은 GPUI의 시스템 알림(XDG D-Bus, notify-rust)을 쓴다.
//! TODO(M9, macOS 패키징 담당): macOS 번들 식별자로 `UNUserNotificationCenter` 권한·클릭 응답을 검증한다.

use std::collections::HashMap;

use futures::channel::mpsc::UnboundedSender;
use gpui_kit::{App, Global, SharedString, SystemNotification, SystemNotificationResponse};
use kanvibe_domain::i18n::Locale;
use kanvibe_domain::notifications::{AppNotification, DesktopNotificationPayload};
use kanvibe_domain::task_notifications::notification_locale;
use kanvibe_services::desktop_notification::{DesktopNotificationPresenter, NotificationClick};

use crate::app_events::RuntimeEvent;
use crate::services::AppServices;
use crate::windows::{TaskNavigationOutcome, TaskNavigationRequest, navigate_to_task_detail};

/// 알림 센터 버튼 배지가 담을 수 있는 가장 큰 수. 넘으면 `9+`로 줄인다
const MAX_BADGE_COUNT: usize = 9;

/// 앱 안 알림 목록. 최신 알림이 앞에 온다
#[derive(Debug, Default)]
pub struct NotificationCenter {
    notifications: Vec<AppNotification>,
}

impl Global for NotificationCenter {}

impl NotificationCenter {
    /// 최신 알림이 앞에 오도록(`createdAt` 내림차순) 바꿔 넣는다
    pub fn replace_all(&mut self, mut notifications: Vec<AppNotification>) {
        notifications.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        self.notifications = notifications;
    }

    pub fn notifications(&self) -> &[AppNotification] {
        &self.notifications
    }

    pub fn unread_count(&self) -> usize {
        self.notifications
            .iter()
            .filter(|notification| !notification.is_read)
            .count()
    }

    pub fn mark_read(&mut self, notification_id: &str) {
        if let Some(notification) = self
            .notifications
            .iter_mut()
            .find(|notification| notification.id == notification_id)
        {
            notification.is_read = true;
        }
    }
}

/// 알림 센터 버튼 배지 글자. 읽지 않은 알림이 없으면 배지를 그리지 않는다
pub fn unread_badge_label(unread_count: usize) -> Option<String> {
    match unread_count {
        0 => None,
        count if count > MAX_BADGE_COUNT => Some(format!("{MAX_BADGE_COUNT}+")),
        count => Some(count.to_string()),
    }
}

/// OS 알림 클릭이 앱으로 돌아올 때 다시 읽는 표식. `tag`에 locale과 작업 ID를 담는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskNotificationTarget {
    pub task_id: String,
    pub locale: Locale,
}

const TASK_TAG_PREFIX: &str = "kanvibe-task";

impl TaskNotificationTarget {
    pub fn to_tag(&self) -> String {
        format!(
            "{TASK_TAG_PREFIX}:{}:{}",
            self.locale.as_str(),
            self.task_id
        )
    }

    /// `kanvibe-task:<locale>:<taskId>`. 작업 ID에 `:`가 있어도 locale 뒤 전체를 ID로 본다
    pub fn parse_tag(tag: &str) -> Option<Self> {
        let rest = tag.strip_prefix(TASK_TAG_PREFIX)?.strip_prefix(':')?;
        let (locale, task_id) = rest.split_once(':')?;
        (!task_id.is_empty()).then(|| Self {
            task_id: task_id.to_string(),
            locale: notification_locale(locale),
        })
    }
}

/// OS 알림을 띄우는 쪽. 시험에서는 가짜 구현으로 바꾼다
pub trait OsNotificationBackend {
    fn show(&self, payload: &DesktopNotificationPayload, cx: &mut App);
}

/// GPUI 시스템 알림(Linux: XDG 알림 D-Bus)
pub struct GpuiSystemNotificationBackend;

impl OsNotificationBackend for GpuiSystemNotificationBackend {
    fn show(&self, payload: &DesktopNotificationPayload, cx: &mut App) {
        let tag = notification_tag(
            payload.task_id.as_deref(),
            &payload.locale,
            payload.dedupe_key.as_deref().unwrap_or_default(),
        );
        cx.show_system_notification(SystemNotification {
            tag: SharedString::from(tag),
            title: SharedString::from(payload.title.clone()),
            body: SharedString::from(payload.body.clone()),
            actions: Vec::new(),
        });
    }
}

/// 런타임 알림 저장소를 다시 읽어 알림 센터를 바꾼다. 읽지 못하면 이전 목록을 둔다
pub fn reload_notification_center(cx: &mut App) {
    let Some(runtime) = cx.global::<AppServices>().runtime().cloned() else {
        return;
    };
    match runtime.notification_store().list_notifications() {
        Ok(notifications) => cx
            .default_global::<NotificationCenter>()
            .replace_all(notifications),
        Err(error) => eprintln!("[kanvibe] failed to read notifications: {error}"),
    }
}

/// 런타임이 띄우라고 한 알림의 누름 처리(읽음 표시·목록 변경 알림). OS 알림 tag마다 하나를 둔다
#[derive(Default)]
struct PendingNotificationClicks(HashMap<String, NotificationClick>);

impl Global for PendingNotificationClicks {}

/// 런타임 알림 표시기. 런타임 스레드에서 불리므로 전경 스레드로 넘기기만 한다
pub struct ForegroundNotificationPresenter {
    pub sender: UnboundedSender<RuntimeEvent>,
}

impl DesktopNotificationPresenter for ForegroundNotificationPresenter {
    fn is_supported(&self) -> bool {
        true
    }

    fn show(&self, notification: &AppNotification, on_click: NotificationClick) {
        let _ = self.sender.unbounded_send(RuntimeEvent::ShowNotification(
            notification.clone(),
            on_click,
        ));
    }
}

/// 작업 알림이면 작업 상세로 돌아올 tag를, 아니면 중복 제거 키를 tag로 쓴다
fn notification_tag(task_id: Option<&str>, locale: &str, dedupe_key: &str) -> String {
    match task_id {
        Some(task_id) => TaskNotificationTarget {
            task_id: task_id.to_string(),
            locale: notification_locale(locale),
        }
        .to_tag(),
        None => dedupe_key.to_string(),
    }
}

/// 전경 스레드에서 OS 알림을 띄우고 누름 처리를 tag에 묶어 둔다
pub fn show_runtime_notification(
    notification: &AppNotification,
    on_click: NotificationClick,
    cx: &mut App,
) {
    let tag = notification_tag(
        notification.task_id.as_deref(),
        &notification.locale,
        &notification.dedupe_key,
    );
    cx.default_global::<PendingNotificationClicks>()
        .0
        .insert(tag.clone(), on_click);
    cx.show_system_notification(SystemNotification {
        tag: SharedString::from(tag),
        title: SharedString::from(notification.title.clone()),
        body: SharedString::from(notification.body.clone()),
        actions: Vec::new(),
    });
}

/// OS 알림을 눌렀을 때 읽음으로 표시하고 작업 상세로 보낸다. 같은 작업 창이 열려 있으면 그 창을 앞으로 가져온다.
pub fn handle_system_notification_response(
    response: &SystemNotificationResponse,
    cx: &mut App,
) -> Option<TaskNavigationOutcome> {
    let pending_click = cx
        .default_global::<PendingNotificationClicks>()
        .0
        .remove(response.tag.as_ref());
    if let Some(on_click) = pending_click {
        on_click();
    }
    let target = TaskNotificationTarget::parse_tag(&response.tag)?;
    Some(navigate_to_task_detail(
        &target.task_id,
        target.locale,
        TaskNavigationRequest::default(),
        cx,
    ))
}

/// 앱 시작 때 한 번 건다
pub fn install_notification_click_routing(cx: &mut App) {
    cx.on_system_notification_response(|response, cx| {
        handle_system_notification_response(&response, cx);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_shows_counts_up_to_nine_then_nine_plus() {
        assert_eq!(unread_badge_label(0), None);
        assert_eq!(unread_badge_label(3).as_deref(), Some("3"));
        assert_eq!(unread_badge_label(10).as_deref(), Some("9+"));
    }

    #[test]
    fn task_tags_round_trip_and_unsupported_locales_fall_back() {
        let target = TaskNotificationTarget {
            task_id: "task:1".into(),
            locale: Locale::En,
        };
        assert_eq!(
            TaskNotificationTarget::parse_tag(&target.to_tag()),
            Some(target)
        );
        assert_eq!(
            TaskNotificationTarget::parse_tag("kanvibe-task:fr:t1").map(|target| target.locale),
            Some(Locale::Ko)
        );
        assert_eq!(TaskNotificationTarget::parse_tag("other:ko:t1"), None);
    }
}
