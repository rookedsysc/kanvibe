//! 보드 이벤트를 알림 목록과 OS 알림으로 옮기는 전달(TS `electron/main.js` `registerBoardEventForwarding`).
//! 전달 서비스는 런타임에 하나이고, 구독은 [`crate::DesktopRuntime::start`]가 한 번만 건다.

use std::sync::{Arc, Mutex};

use kanvibe_domain::notifications::AppNotification;
use kanvibe_services::board_notifier::{BoardEventSubscription, BoardNotifier};
use kanvibe_services::desktop_notification::{
    DesktopNotificationPresenter, DesktopNotificationService, NotificationClick,
    NotificationDeliveryCallbacks,
};

/// 알림 문구 언어. TS는 열린 창 주소의 locale을 썼다. 창 locale을 받기 전까지는 앱 기본 locale이다
const DEFAULT_NOTIFICATION_LOCALE: &str = "ko";

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
    subscription: Mutex<Option<BoardEventSubscription>>,
}

impl NotificationForwarding {
    pub fn new(
        service: Arc<DesktopNotificationService>,
        callbacks: NotificationDeliveryCallbacks,
    ) -> Self {
        Self {
            service,
            callbacks,
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
        *subscription = Some(board.subscribe(move |event| {
            if let Err(error) = service.deliver_board_event_notification(
                event,
                DEFAULT_NOTIFICATION_LOCALE,
                &callbacks,
            ) {
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
