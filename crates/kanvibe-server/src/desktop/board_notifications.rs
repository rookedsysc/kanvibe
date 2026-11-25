//! 보드 이벤트를 알림 목록과 OS 알림으로 옮긴다(TS `electron/main.js` `registerBoardEventForwarding`).
//!
//! 상태 변경·hook 대상 누락·background sync 검토가 알림 목록에 남아야 모바일 알림함과 데스크탑 알림함이 같은 내용을 본다.

use std::sync::Arc;

use kanvibe_domain::notifications::AppNotification;
use kanvibe_runtime::DesktopRuntime;
use kanvibe_services::board_notifier::BoardEventSubscription;
use kanvibe_services::desktop_notification::{
    DesktopNotificationPresenter, DesktopNotificationService, NotificationClick,
    NotificationDeliveryCallbacks,
};

/// 알림 문구 언어. TS는 열린 창 주소의 locale을 썼고, 창이 없는 서버는 앱 기본 locale을 쓴다
const DEFAULT_NOTIFICATION_LOCALE: &str = "ko";

/// OS 알림을 띄우지 않는 표시기. 창이 없는 QA 서버가 쓴다
pub struct SilentNotificationPresenter;

impl DesktopNotificationPresenter for SilentNotificationPresenter {
    fn is_supported(&self) -> bool {
        false
    }

    fn show(&self, _notification: &AppNotification, _on_click: NotificationClick) {}
}

/// 돌려준 구독을 버리면 전달이 멈춘다
pub fn forward_board_event_notifications(
    runtime: &DesktopRuntime,
    presenter: Arc<dyn DesktopNotificationPresenter>,
) -> BoardEventSubscription {
    let service = DesktopNotificationService::new(
        runtime.database().clone(),
        runtime.notification_store().clone(),
        presenter,
    );
    let callbacks = NotificationDeliveryCallbacks::default();
    runtime.board_notifier().subscribe(move |event| {
        if let Err(error) =
            service.deliver_board_event_notification(event, DEFAULT_NOTIFICATION_LOCALE, &callbacks)
        {
            eprintln!("[kanvibe] board event notification failed: {error}");
        }
    })
}
