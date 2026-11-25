//! 알림 기반. 앱 안 알림 센터(버튼 배지·목록)의 모델과, OS 알림을 띄우고 누르면 작업 상세로 보내는 경로.
//!
//! OS 알림은 `OsNotificationBackend` 뒤에 둔다. Linux 구현은 GPUI의 시스템 알림(XDG D-Bus, notify-rust)을 쓴다.
//! TODO(M9, macOS 패키징 담당): macOS 번들 식별자로 `UNUserNotificationCenter` 권한·클릭 응답을 검증한다.

use gpui_kit::{App, Global, SharedString, SystemNotification, SystemNotificationResponse};
use kanvibe_domain::i18n::Locale;
use kanvibe_domain::notifications::{AppNotification, DesktopNotificationPayload};
use kanvibe_domain::task_notifications::notification_locale;

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
    pub fn replace_all(&mut self, notifications: Vec<AppNotification>) {
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
        let tag = payload
            .task_id
            .as_ref()
            .map(|task_id| {
                TaskNotificationTarget {
                    task_id: task_id.clone(),
                    locale: notification_locale(&payload.locale),
                }
                .to_tag()
            })
            .or_else(|| payload.dedupe_key.clone())
            .unwrap_or_default();
        cx.show_system_notification(SystemNotification {
            tag: SharedString::from(tag),
            title: SharedString::from(payload.title.clone()),
            body: SharedString::from(payload.body.clone()),
            actions: Vec::new(),
        });
    }
}

/// OS 알림을 눌렀을 때 작업 상세로 보낸다. 같은 작업 창이 열려 있으면 그 창을 앞으로 가져온다.
pub fn handle_system_notification_response(
    response: &SystemNotificationResponse,
    cx: &mut App,
) -> Option<TaskNavigationOutcome> {
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
