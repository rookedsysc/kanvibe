//! 보드 이벤트를 알림 목록과 OS 알림으로 옮기는 전달(TS `electron/main.js` `registerBoardEventForwarding`).
//! 전달 서비스는 런타임에 하나이고, 구독은 [`crate::DesktopRuntime::start`]가 한 번만 건다.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use kanvibe_domain::notifications::AppNotification;
use kanvibe_domain::task_notifications::notification_locale;
use kanvibe_services::board_notifier::{BoardEventSubscription, BoardNotifier};
use kanvibe_services::desktop_notification::{
    DesktopNotificationPresenter, DesktopNotificationService, NotificationClick,
    NotificationDeliveryCallbacks,
};

/// 알림 문구 locale을 고르는 쪽(TS `getDesktopNotificationLocale`). 이벤트마다 한 번 부른다.
/// 앱은 앞에 있는 창 경로의 locale을, 창이 없는 실행은 [`system_notification_locale`]을 돌려준다
pub type NotificationLocaleSource = Arc<dyn Fn() -> String + Send + Sync>;

/// TS 기본 locale(`DEFAULT_LOCALE`). 앱 첫 창이 여는 경로(`/ko`)의 locale이기도 하다
pub const DEFAULT_NOTIFICATION_LOCALE: &str = "ko";

/// 열린 창이 없을 때 TS가 쓰던 `normalizeNotificationLocale(app.getLocale())`.
///
/// Chromium은 Linux에서 POSIX 순서(`LC_ALL` → `LC_MESSAGES` → `LANG`)로 locale을 정하고, `C`·`POSIX`나 빈 값이면
/// `en-US`로 둔다. 그 결과를 TS와 같이 `en*` → `en`, `zh*` → `zh`, 나머지 → `ko`로 줄인다.
/// 값은 호출자가 시작할 때 잡아 둔 환경 스냅샷에서만 읽는다
pub fn system_notification_locale(environment: &BTreeMap<String, String>) -> String {
    let system_locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| environment.get(*name))
        .find(|value| !value.is_empty())
        .map(String::as_str)
        .unwrap_or_default();
    let language = system_locale.split(['.', '@']).next().unwrap_or_default();
    let is_default_locale = language.is_empty() || language == "C" || language == "POSIX";
    let chromium_locale = if is_default_locale { "en-US" } else { language };
    notification_locale(chromium_locale).as_str().to_string()
}

/// OS 알림을 띄우지 않는 표시기. 창이 없는 실행(QA 서버, 테스트)이 쓴다
pub struct SilentNotificationPresenter;

impl DesktopNotificationPresenter for SilentNotificationPresenter {
    fn is_supported(&self) -> bool {
        false
    }

    fn show(&self, _notification: &AppNotification, _on_click: NotificationClick) {}
}

pub(crate) struct NotificationForwarding {
    pub service: Arc<DesktopNotificationService>,
    callbacks: NotificationDeliveryCallbacks,
    locale: NotificationLocaleSource,
    subscription: Mutex<Option<BoardEventSubscription>>,
}

impl NotificationForwarding {
    pub fn new(
        service: Arc<DesktopNotificationService>,
        callbacks: NotificationDeliveryCallbacks,
        locale: NotificationLocaleSource,
    ) -> Self {
        Self {
            service,
            callbacks,
            locale,
            subscription: Mutex::new(None),
        }
    }

    /// 이미 구독 중이면 아무것도 하지 않는다. 같은 이벤트가 알림 두 개로 남지 않게 한다
    pub fn start(&self, board: &Arc<BoardNotifier>) {
        let mut subscription = self.lock_subscription();
        if subscription.is_some() {
            return;
        }
        let service = Arc::clone(&self.service);
        let callbacks = self.callbacks.clone();
        let locale = Arc::clone(&self.locale);
        *subscription = Some(board.subscribe(move |event| {
            if let Err(error) =
                service.deliver_board_event_notification(event, &locale(), &callbacks)
            {
                eprintln!("[kanvibe] board event notification failed: {error}");
            }
        }));
    }

    pub fn stop(&self) {
        self.lock_subscription().take();
    }

    fn lock_subscription(&self) -> std::sync::MutexGuard<'_, Option<BoardEventSubscription>> {
        self.subscription
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::system_notification_locale;

    fn locale_for(pairs: &[(&str, &str)]) -> String {
        let environment: BTreeMap<String, String> = pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        system_notification_locale(&environment)
    }

    #[test]
    fn system_locale_follows_posix_precedence_and_ts_normalization() {
        assert_eq!(locale_for(&[("LANG", "ko_KR.UTF-8")]), "ko");
        assert_eq!(locale_for(&[("LANG", "en_US.UTF-8")]), "en");
        assert_eq!(locale_for(&[("LANG", "zh_CN.UTF-8")]), "zh");
        assert_eq!(locale_for(&[("LANG", "fr_FR.UTF-8")]), "ko");
        assert_eq!(
            locale_for(&[("LANG", "ko_KR.UTF-8"), ("LC_MESSAGES", "zh_TW.UTF-8")]),
            "zh"
        );
        assert_eq!(
            locale_for(&[("LANG", "ko_KR.UTF-8"), ("LC_ALL", "en_GB.UTF-8")]),
            "en"
        );
    }

    #[test]
    fn posix_default_locale_reads_as_chromium_en_us() {
        assert_eq!(locale_for(&[]), "en");
        assert_eq!(locale_for(&[("LANG", "C.UTF-8")]), "en");
        assert_eq!(locale_for(&[("LC_ALL", ""), ("LANG", "POSIX")]), "en");
    }
}
