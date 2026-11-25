//! 앱 전역 이벤트. 런타임(다른 스레드)이 내는 보드 이벤트·알림 변경을 GPUI 전경 스레드로 옮기고,
//! 화면이 구독하는 새로 고침 신호(TS `useRefreshSignal`·`triggerDesktopRefresh`)를 낸다.
//!
//! TS는 `board-updated`를 250ms 동안 모아 `"all"` 새로 고침 한 번으로 바꿨다(App.tsx). 여기서도 같은 규칙을 쓴다.

use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::{
    App, AppContext, AsyncApp, Context, Entity, EventEmitter, Global, Subscription, WindowId,
};
use kanvibe_domain::notifications::AppNotification;
use kanvibe_services::board_notifier::{BoardEvent, BoardEventSubscription};
use kanvibe_services::desktop_notification::NotificationClick;

/// TS `BOARD_REFRESH_DEBOUNCE_MS`
const BOARD_REFRESH_DEBOUNCE: Duration = Duration::from_millis(250);

/// TS `DesktopRefreshScope`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshScope {
    All,
    Board,
    TaskDetail,
    Diff,
    AiAccounts,
    PaneLayout,
    Settings,
}

impl RefreshScope {
    /// 현재 경로의 새로 고침 범위(TS `navigation.tsx` `getRefreshScope`)
    pub fn for_path(path: &str) -> Self {
        if path.contains("/task/") && path.ends_with("/diff") {
            Self::Diff
        } else if path.contains("/task/") {
            Self::TaskDetail
        } else if path.ends_with("/ai-accounts") {
            Self::AiAccounts
        } else if path.ends_with("/pane-layout") {
            Self::PaneLayout
        } else if path.contains("/settings") {
            Self::Settings
        } else {
            Self::Board
        }
    }
}

/// 새로 고침 요청. `All`은 모든 구독자에게, 나머지는 그 범위를 구독한 화면에만 간다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshRequested {
    pub scope: RefreshScope,
}

impl RefreshRequested {
    pub fn applies_to(&self, scopes: &[RefreshScope]) -> bool {
        self.scope == RefreshScope::All || scopes.contains(&self.scope)
    }
}

/// 런타임 보드 이벤트(TS `kanvibe:board-event`). 화면이 토스트·배너를 띄울 때 구독한다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardEventReceived(pub BoardEvent);

/// 알림 목록이 바뀌었다(TS `onNotificationsChanged`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationsChanged;

/// main이 창에 알린 알림 열기(TS `kanvibe:notification-activated`). `window_id`가 없으면 모든 창에 보낸 것이다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationActivated {
    pub notification: AppNotification,
    pub window_id: Option<WindowId>,
}

/// 앱 전역 이벤트를 내는 엔티티
#[derive(Default)]
pub struct AppEventBus {
    is_board_refresh_scheduled: bool,
}

impl EventEmitter<RefreshRequested> for AppEventBus {}
impl EventEmitter<BoardEventReceived> for AppEventBus {}
impl EventEmitter<NotificationsChanged> for AppEventBus {}
impl EventEmitter<NotificationActivated> for AppEventBus {}

#[derive(Clone)]
pub struct AppEvents(pub Entity<AppEventBus>);

impl Global for AppEvents {}

/// 런타임 스레드에서 전경으로 넘기는 일
pub enum RuntimeEvent {
    Board(BoardEvent),
    NotificationsChanged,
    ShowNotification(AppNotification, NotificationClick),
}

/// 런타임 설정에 넣을 송신 쪽과 [`install_app_events`]가 받을 수신 쪽
pub fn runtime_event_channel() -> (
    UnboundedSender<RuntimeEvent>,
    UnboundedReceiver<RuntimeEvent>,
) {
    unbounded()
}

/// 보드 이벤트 구독. 앱이 끝날 때까지 쥔다
struct BoardEventRelay {
    _subscription: Option<BoardEventSubscription>,
}

impl Global for BoardEventRelay {}

/// 이벤트 버스를 만들고 런타임 이벤트를 전경으로 옮기는 루프를 건다
pub fn install_app_events(
    receiver: UnboundedReceiver<RuntimeEvent>,
    board_subscription: Option<BoardEventSubscription>,
    cx: &mut App,
) {
    let bus = cx.new(|_| AppEventBus::default());
    cx.set_global(AppEvents(bus));
    cx.set_global(BoardEventRelay {
        _subscription: board_subscription,
    });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let mut receiver = receiver;
        while let Some(event) = receiver.next().await {
            cx.update(|cx| handle_runtime_event(event, cx));
        }
    })
    .detach();
}

fn handle_runtime_event(event: RuntimeEvent, cx: &mut App) {
    match event {
        RuntimeEvent::Board(board_event) => receive_board_event(board_event, cx),
        RuntimeEvent::NotificationsChanged => {
            crate::notifications::reload_notification_center(cx);
            emit(NotificationsChanged, cx);
        }
        RuntimeEvent::ShowNotification(notification, on_click) => {
            crate::notifications::show_runtime_notification(&notification, on_click, cx);
        }
    }
}

fn receive_board_event(board_event: BoardEvent, cx: &mut App) {
    let is_board_updated = board_event == BoardEvent::BoardUpdated;
    emit(BoardEventReceived(board_event), cx);
    if is_board_updated {
        schedule_board_refresh(cx);
    }
}

/// 이미 예약했으면 그 예약 하나로 모은다
fn schedule_board_refresh(cx: &mut App) {
    let Some(bus) = cx.try_global::<AppEvents>().map(|events| events.0.clone()) else {
        return;
    };
    let is_already_scheduled = bus.update(cx, |bus, _| {
        std::mem::replace(&mut bus.is_board_refresh_scheduled, true)
    });
    if is_already_scheduled {
        return;
    }
    cx.spawn(async move |cx: &mut AsyncApp| {
        cx.background_executor().timer(BOARD_REFRESH_DEBOUNCE).await;
        cx.update(|cx| {
            bus.update(cx, |bus, _| bus.is_board_refresh_scheduled = false);
            trigger_refresh(RefreshScope::All, cx);
        });
    })
    .detach();
}

fn emit<E>(event: E, cx: &mut App)
where
    AppEventBus: EventEmitter<E>,
    E: 'static,
{
    if let Some(bus) = cx.try_global::<AppEvents>().map(|events| events.0.clone()) {
        bus.update(cx, |_, cx| cx.emit(event));
    }
}

/// 화면이 알림 저장소를 바꾼 뒤 다른 화면(카드 배지·알림 센터)에 알린다
pub fn notify_notifications_changed(cx: &mut App) {
    emit(NotificationsChanged, cx);
}

/// TS main `sendNotificationActivated`·`broadcastNotificationActivated`
pub fn activate_notification(
    notification: AppNotification,
    window_id: Option<WindowId>,
    cx: &mut App,
) {
    emit(
        NotificationActivated {
            notification,
            window_id,
        },
        cx,
    );
}

/// TS `triggerDesktopRefresh(scope)`
pub fn trigger_refresh(scope: RefreshScope, cx: &mut App) {
    emit(RefreshRequested { scope }, cx);
}

/// TS `useRefreshSignal(scopes)`. 돌려준 구독을 뷰가 쥐고 있는 동안 `on_refresh`를 부른다
pub fn subscribe_to_refresh<V: 'static>(
    scopes: &'static [RefreshScope],
    cx: &mut Context<V>,
    on_refresh: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> Option<Subscription> {
    let bus = cx.try_global::<AppEvents>()?.0.clone();
    Some(
        cx.subscribe(&bus, move |view, _, request: &RefreshRequested, cx| {
            if request.applies_to(scopes) {
                on_refresh(view, cx);
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_scope_follows_the_ts_route_rules() {
        assert_eq!(
            RefreshScope::for_path("/ko/task/t1/diff"),
            RefreshScope::Diff
        );
        assert_eq!(
            RefreshScope::for_path("/ko/task/t1"),
            RefreshScope::TaskDetail
        );
        assert_eq!(
            RefreshScope::for_path("/ko/settings/ai-accounts"),
            RefreshScope::AiAccounts
        );
        assert_eq!(RefreshScope::for_path("/ko"), RefreshScope::Board);
    }

    #[test]
    fn all_reaches_every_subscriber_and_named_scopes_only_their_own() {
        let all = RefreshRequested {
            scope: RefreshScope::All,
        };
        let diff = RefreshRequested {
            scope: RefreshScope::Diff,
        };
        assert!(all.applies_to(&[RefreshScope::Board]));
        assert!(diff.applies_to(&[RefreshScope::All, RefreshScope::Diff]));
        assert!(!diff.applies_to(&[RefreshScope::Board]));
    }
}
