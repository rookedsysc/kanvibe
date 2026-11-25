//! 알림 센터 버튼과 패널(TS `NotificationCenterButton` + `BackgroundSyncStatus`). 보드 머리와 작업 상세 dock이 같은 뷰를 쓴다.
//! 목록은 런타임 알림 저장소(`NotificationCenter` 전역)에서 읽고 최신 알림이 위에 온다. 처음에는 맨 위 알림을 고른 채 열린다.
//! ↑↓로 고르고 Enter(새 창은 Shift+Enter)로 열며, Space는 열지 않고 읽음만 표시하고, Esc는 닫는다.
//! 알림을 누르면 읽음으로 바꾼 뒤 대상 작업 상세로 간다. 같은 작업 창이 있으면 그 창을 앞으로 가져온다.
//! 대상 작업이 지워졌으면 안내 대화상자를, 백그라운드 sync 검토 알림이면 지금 화면 위에 검토 대화상자를 연다.
//!
//! TS 버튼: `relative rounded-md p-1.5 text-text-muted hover:bg-bg-page hover:text-text-primary`, 배지
//! `absolute -right-1 -top-1 min-w-4 h-4 rounded-full bg-brand-primary px-1 text-[10px] font-semibold text-white`.
//! 패널: `absolute right-0 z-50 mt-2 w-96 rounded-xl border border-border-default bg-bg-surface shadow-xl`.

use std::sync::Arc;

use gpui_kit::{
    AnyElement, AnyWindowHandle, App, Bounds, Context, Entity, FocusHandle, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, PathBuilder, Pixels, Render, Rgba,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Window, canvas, deferred, div,
    point, px, svg,
};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::notifications::AppNotification;

use crate::app_events::{
    AppEvents, NotificationsChanged, activate_notification, notify_notifications_changed,
};
use crate::assets::icon_paths;
use crate::components::dialog::modal_layer;
use crate::components::popover::POPOVER_LAYER_PRIORITY;
use crate::components::toast::ToastKind;
use crate::components::{FocusRing, TailwindText, fade};
use crate::locale_format::format_date_time;
use crate::notifications::{NotificationCenter, reload_notification_center, unread_badge_label};
use crate::qa::targets::{qa_button_layer, qa_text_layer};
use crate::services::AppServices;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::WindowNavigator;
use crate::views::board::inline_load_error;
use crate::views::board::services::{BoardServices, board_services};
use crate::views::locale_shell::background_sync::{BackgroundSyncStore, background_sync_store};
use crate::views::locale_shell::sync_review::{BackgroundSyncReviewStore, sync_review_store};
use crate::window_open::{
    resolve_navigation_target_window, should_keep_current_route_for_notification_activation,
};
use crate::windows::{TaskNavigationRequest, WindowRegistry, navigate_to_task_detail};

/// 패널 폭(`w-96`)
const PANEL_WIDTH: f32 = 384.0;
/// 목록 최대 높이(`max-h-[28rem]`)
const LIST_MAX_HEIGHT: f32 = 448.0;

/// 최신 알림이 위로(`createdAt` 내림차순)
pub fn newest_first(notifications: &[AppNotification]) -> Vec<AppNotification> {
    let mut sorted = notifications.to_vec();
    sorted.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    sorted
}

/// 안읽음만 보기면 읽은 알림을 뺀다
pub fn visible_notifications(
    notifications: &[AppNotification],
    is_unread_only: bool,
) -> Vec<AppNotification> {
    newest_first(notifications)
        .into_iter()
        .filter(|notification| !is_unread_only || !notification.is_read)
        .collect()
}

pub struct NotificationCenterButton {
    locale: Locale,
    navigator: WindowNavigator,
    services: Option<Arc<BoardServices>>,
    background_sync: Entity<BackgroundSyncStore>,
    sync_review: Entity<BackgroundSyncReviewStore>,
    panel_focus: FocusHandle,
    /// 패널을 열 때 포커스가 있던 곳. 닫으면 돌려준다(TS `requestActiveTerminalFocusAfterUiSettles`)
    return_focus: Option<FocusHandle>,
    is_open: bool,
    is_unread_only: bool,
    highlighted_index: usize,
    /// 읽음 저장과 이동을 기다리는 알림. 같은 알림을 거듭 열지 않는다
    opening_notification_id: Option<String>,
    is_marking_all_read: bool,
    mark_all_read_failure: Option<String>,
    /// 대상 작업이 사라져 안내를 띄운 알림
    missing_task_notification: Option<String>,
    /// 단축키로 열었으면 패널에 키보드 포커스 링을 그린다(Chromium `:focus-visible`)
    is_keyboard_opened: bool,
    _subscriptions: Vec<Subscription>,
}

impl NotificationCenterButton {
    pub fn new(locale: Locale, navigator: WindowNavigator, cx: &mut Context<Self>) -> Self {
        let background_sync = background_sync_store(cx);
        let sync_review = sync_review_store(cx);
        let mut subscriptions = vec![
            cx.observe(&background_sync, |_, _, cx| cx.notify()),
            cx.observe(&sync_review, |_, _, cx| cx.notify()),
        ];
        if let Some(bus) = cx.try_global::<AppEvents>().map(|events| events.0.clone()) {
            subscriptions
                .push(cx.subscribe(&bus, |_, _, _: &NotificationsChanged, cx| cx.notify()));
        }
        // TS는 버튼이 붙을 때 목록과 안읽음만 보기 설정을 읽었다
        reload_notification_center(cx);
        let is_unread_only = cx
            .global::<AppServices>()
            .runtime()
            .and_then(|runtime| {
                runtime
                    .app_settings()
                    .get_notification_unread_only_enabled()
                    .ok()
            })
            .unwrap_or(false);
        Self {
            locale,
            navigator,
            services: board_services(cx),
            background_sync,
            sync_review,
            panel_focus: cx.focus_handle(),
            return_focus: None,
            is_open: false,
            is_unread_only,
            highlighted_index: 0,
            opening_notification_id: None,
            is_marking_all_read: false,
            mark_all_read_failure: None,
            missing_task_notification: None,
            is_keyboard_opened: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// 대상 작업이 사라져 안내를 띄운 알림
    pub fn missing_task_notification_id(&self) -> Option<String> {
        self.missing_task_notification.clone()
    }

    /// 단축키(`Mod+Shift+I`)로 연다. 키보드로 열었으므로 포커스 링을 그린다
    pub fn toggle_by_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle(window, cx);
        self.is_keyboard_opened = self.is_open;
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.is_keyboard_opened = false;
        if self.is_open {
            self.close(window, cx);
            return;
        }
        self.is_open = true;
        self.highlighted_index = 0;
        self.return_focus = window.focused(cx);
        self.panel_focus.focus(window, cx);
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_open {
            return;
        }
        self.is_open = false;
        if let Some(handle) = self.return_focus.take() {
            handle.focus(window, cx);
        }
        cx.notify();
    }

    fn all_notifications(&self, cx: &App) -> Vec<AppNotification> {
        cx.try_global::<NotificationCenter>()
            .map(|center| center.notifications().to_vec())
            .unwrap_or_default()
    }

    fn current_visible_notifications(&self, cx: &App) -> Vec<AppNotification> {
        visible_notifications(&self.all_notifications(cx), self.is_unread_only)
    }

    fn set_unread_only(&mut self, is_unread_only: bool, cx: &mut Context<Self>) {
        self.is_unread_only = is_unread_only;
        self.highlighted_index = 0;
        if let Some(services) = &self.services
            && let Err(failure) = services.save_notification_unread_only(is_unread_only)
        {
            eprintln!(
                "[kanvibe] failed to save unread-only filter: {}",
                failure.reason
            );
        }
        cx.notify();
    }

    /// 저장한 읽음을 알림 목록과 다른 화면(카드 배지)에 바로 반영한다
    fn set_read_locally(&mut self, notification_ids: &[String], cx: &mut Context<Self>) {
        if cx.has_global::<NotificationCenter>() {
            let center = cx.global_mut::<NotificationCenter>();
            for notification_id in notification_ids {
                center.mark_read(notification_id);
            }
        }
        notify_notifications_changed(cx);
        cx.notify();
    }

    /// 패널이 닫힌 뒤에도 보이도록 실패는 작업 결과 토스트로 알린다(TS `showActionStatus`)
    fn show_failure_toast(&self, message: String, cx: &mut Context<Self>) {
        match self.navigator.locale_shell(cx) {
            Some(shell) => shell.update(cx, |shell, cx| {
                shell.show_action_status(ToastKind::Failure, message, None, cx)
            }),
            None => eprintln!("[kanvibe] {message}"),
        }
    }

    /// Space: 열지 않고 읽음만 표시한다. 이미 읽은 알림이면 다시 저장하지 않는다
    fn mark_highlighted_read(&mut self, cx: &mut Context<Self>) {
        let visible = self.current_visible_notifications(cx);
        let Some(notification) = visible.get(self.highlighted_index).cloned() else {
            return;
        };
        if notification.is_read {
            return;
        }
        let Some(services) = self.services.clone() else {
            return;
        };
        let save = services.mark_notification_read(notification.id.clone(), cx);
        cx.spawn(async move |this, cx| {
            let result = save.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => this.set_read_locally(&[notification.id], cx),
                Err(failure) => {
                    let message = Translator::new(this.locale, "common").translate_with(
                        "notificationMarkReadFailed",
                        &[("reason", &failure.reason)],
                    );
                    this.show_failure_toast(message, cx);
                }
            });
        })
        .detach();
    }

    /// 알림을 연다. 여는 동안 같은 알림을 다시 열지 않고, 실패는 패널이 닫힌 뒤에도 보이도록 토스트로 알린다
    fn open_notification(
        &mut self,
        notification: AppNotification,
        should_open_new_window: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.opening_notification_id.is_some() {
            return;
        }
        let Some(services) = self.services.clone() else {
            return;
        };
        self.opening_notification_id = Some(notification.id.clone());
        let mark_read = (!notification.is_read)
            .then(|| services.mark_notification_read(notification.id.clone(), cx));
        let check_task = services.activate_notification(notification.task_id.clone(), cx);
        let window_handle = window.window_handle();
        cx.spawn_in(window, async move |this, cx| {
            let read_result = match mark_read {
                Some(task) => task.await,
                None => Ok(()),
            };
            let task_result = match read_result {
                Ok(()) => check_task.await,
                Err(failure) => Err(failure),
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.opening_notification_id = None;
                match task_result {
                    Ok(does_task_exist) => {
                        this.set_read_locally(std::slice::from_ref(&notification.id), cx);
                        this.finish_opening_notification(
                            &notification,
                            does_task_exist,
                            should_open_new_window,
                            window_handle,
                            window,
                            cx,
                        );
                    }
                    Err(failure) => {
                        let message = Translator::new(this.locale, "common").translate_with(
                            "notificationOpenFailed",
                            &[("reason", &failure.reason)],
                        );
                        this.show_failure_toast(message, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_opening_notification(
        &mut self,
        notification: &AppNotification,
        does_task_exist: bool,
        should_open_new_window: bool,
        window_handle: AnyWindowHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close(window, cx);
        let action = notification.action.clone().flatten();
        if should_keep_current_route_for_notification_activation(action.as_ref()) {
            // 백그라운드 sync 검토는 지금 화면 위에 검토 대화상자로 연다(TS `openBackgroundSyncReview`)
            activate_notification(notification.clone(), Some(window_handle.window_id()), cx);
            return;
        }
        if let Some(task_id) = &notification.task_id {
            if !does_task_exist {
                self.missing_task_notification = Some(notification.id.clone());
                return;
            }
            let locale =
                kanvibe_domain::task_notifications::notification_locale(&notification.locale);
            let request = TaskNavigationRequest {
                origin_window: Some(window_handle.window_id()),
                should_open_new_window,
            };
            navigate_to_task_detail(task_id, locale, request, cx);
            return;
        }
        let open_windows = open_window_paths(cx);
        let current_window = window_handle.window_id();
        let target = resolve_navigation_target_window(
            Some(current_window),
            &notification.relative_path,
            &open_windows,
            None,
        );
        match target {
            Some(window_id) if window_id != current_window => {
                if let Some(handle) = cx
                    .windows()
                    .into_iter()
                    .find(|handle| handle.window_id() == window_id)
                {
                    let _ = handle.update(cx, |_, window, _| window.activate_window());
                }
            }
            _ => self.navigator.navigate(&notification.relative_path, cx),
        }
    }

    fn mark_all_notifications_read(&mut self, cx: &mut Context<Self>) {
        if self.is_marking_all_read {
            return;
        }
        let Some(services) = self.services.clone() else {
            return;
        };
        self.is_marking_all_read = true;
        self.mark_all_read_failure = None;
        let save = services.mark_all_notifications_read(cx);
        cx.spawn(async move |this, cx| {
            let result = save.await;
            let _ = this.update(cx, |this, cx| {
                this.is_marking_all_read = false;
                match result {
                    Ok(()) => {
                        let ids: Vec<String> = this
                            .all_notifications(cx)
                            .into_iter()
                            .map(|notification| notification.id)
                            .collect();
                        this.set_read_locally(&ids, cx);
                    }
                    Err(failure) => this.mark_all_read_failure = Some(failure.reason),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// 검토 버튼: 지우는 중이면 그 검토를, 아니면 목록의 검토 알림을 연다
    fn open_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let review = self.sync_review.read(cx);
        if review.is_cleaning() {
            let Some(notification) = review.notification().cloned() else {
                return;
            };
            self.close(window, cx);
            activate_notification(notification, Some(window.window_handle().window_id()), cx);
            return;
        }
        if let Some(notification) = self.review_notification(cx) {
            self.open_notification(notification, false, window, cx);
        }
    }

    /// 다시 열 검토 알림(TS `reviewNotification`)
    fn review_notification(&self, cx: &App) -> Option<AppNotification> {
        let review = self.sync_review.read(cx);
        if review.is_cleaning() {
            return review.notification().cloned();
        }
        self.all_notifications(cx).into_iter().find(|notification| {
            should_keep_current_route_for_notification_activation(
                notification.action.clone().flatten().as_ref(),
            )
        })
    }

    fn handle_panel_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .sync_review
            .read(cx)
            .is_open_in(window.window_handle().window_id())
        {
            return;
        }
        let visible = self.current_visible_notifications(cx);
        let count = visible.len();
        let index = self.highlighted_index.min(count.saturating_sub(1));
        match event.keystroke.key.as_str() {
            "down" if count > 0 => self.highlighted_index = (index + 1) % count,
            "up" if count > 0 => self.highlighted_index = (index + count - 1) % count,
            "enter" => {
                if let Some(notification) = visible.get(index).cloned() {
                    self.open_notification(
                        notification,
                        event.keystroke.modifiers.shift,
                        window,
                        cx,
                    );
                }
            }
            "space" => self.mark_highlighted_read(cx),
            "escape" => self.close(window, cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

/// 열린 창과 각 창의 현재 경로
fn open_window_paths(cx: &App) -> Vec<(gpui_kit::WindowId, String)> {
    let Some(registry) = cx.try_global::<WindowRegistry>() else {
        return Vec::new();
    };
    cx.windows()
        .iter()
        .filter_map(|handle| {
            let window_id = handle.window_id();
            registry
                .current_path(window_id)
                .map(|path| (window_id, path.to_string()))
        })
        .collect()
}

impl Render for NotificationCenterButton {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let panel = self.is_open.then(|| self.render_panel(&theme, cx));
        let missing_notice = self
            .missing_task_notification
            .is_some()
            .then(|| self.render_missing_task_notice(&theme, window, cx));
        div()
            .relative()
            .flex_none()
            .child(self.render_trigger(&theme, cx))
            .children(panel)
            .children(missing_notice)
    }
}

impl NotificationCenterButton {
    fn render_trigger(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let common = Translator::new(self.locale, "common");
        let label = common.translate("notifications");
        let hover_background = theme.background.page;
        let sync = self.background_sync.read(cx).state();
        let is_cleaning = self.sync_review.read(cx).is_cleaning();
        let indicator = if is_cleaning || sync.is_running() {
            let key = if is_cleaning {
                "backgroundSyncStatus.cleaning"
            } else {
                "backgroundSyncStatus.running"
            };
            Some(sync_spinner(theme, &common.translate(key), cx).into_any_element())
        } else if sync.is_failed() {
            Some(
                sync_failed_badge(theme, &common.translate("backgroundSyncStatus.failed"))
                    .into_any_element(),
            )
        } else {
            None
        };
        let unread_count = cx
            .try_global::<NotificationCenter>()
            .map_or(0, NotificationCenter::unread_count);
        let badge = indicator.or_else(|| {
            unread_badge_label(unread_count)
                .map(|badge| unread_badge(theme, badge).into_any_element())
        });
        div()
            .id("notification-center-button")
            .relative()
            .flex_none()
            .rounded(radius::MD)
            .p(px(6.0))
            .cursor_pointer()
            .hover(move |style| style.bg(hover_background))
            .on_click(cx.listener(|this, _, window, cx| this.toggle(window, cx)))
            .child(
                svg()
                    .path(icon_paths::BELL)
                    .size(px(20.0))
                    .text_color(theme.text.muted),
            )
            .child(qa_button_layer(&label, ""))
            .children(badge)
    }

    fn render_panel(&mut self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let labels = Translator::new(self.locale, "common");
        let all = self.all_notifications(cx);
        let unread_count = all.iter().filter(|item| !item.is_read).count();
        let visible = visible_notifications(&all, self.is_unread_only);
        let highlighted = self.highlighted_index.min(visible.len().saturating_sub(1));
        let rows: Vec<AnyElement> = if visible.is_empty() {
            let key = if self.is_unread_only {
                "noUnreadNotifications"
            } else {
                "noNotifications"
            };
            let message = labels.translate(key);
            vec![
                div()
                    .relative()
                    .px(spacing(4.0))
                    .py(spacing(8.0))
                    .flex()
                    .justify_center()
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.muted)
                    .child(message.clone())
                    .child(qa_text_layer("status", &message))
                    .into_any_element(),
            ]
        } else {
            visible
                .iter()
                .enumerate()
                .map(|(index, notification)| {
                    self.render_notification_row(
                        theme,
                        notification,
                        index,
                        index == highlighted,
                        cx,
                    )
                })
                .collect()
        };
        let mark_all_failure = self.mark_all_read_failure.clone().map(|reason| {
            inline_load_error(
                theme,
                self.locale,
                labels.translate_with("markAllReadFailed", &[("reason", &reason)]),
                "notification-mark-all-retry",
                cx.listener(|this, _, _, cx| this.mark_all_notifications_read(cx)),
            )
            .mx(spacing(4.0))
            .mt(spacing(3.0))
        });
        let panel = div()
            .id("notification-panel")
            .track_focus(&self.panel_focus)
            .on_key_down(cx.listener(Self::handle_panel_key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    // 패널 안을 누르면 포커스가 눌린 버튼으로 옮겨 가 패널의 키보드 포커스 링이 사라진다
                    if this.is_keyboard_opened {
                        this.is_keyboard_opened = false;
                        cx.notify();
                    }
                }),
            )
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close(window, cx)))
            .occlude()
            .relative()
            .mt(spacing(2.0))
            .w(px(PANEL_WIDTH))
            .overflow_hidden()
            .rounded(radius::XL)
            .when_focus_ring(self.is_keyboard_opened, theme)
            .bg(theme.background.surface)
            .child(self.render_background_sync_status(theme, cx))
            .child(self.render_header(theme, &labels, unread_count, cx))
            .children(mark_all_failure)
            .child(
                div()
                    .id("notification-rows")
                    .max_h(px(LIST_MAX_HEIGHT))
                    .overflow_y_scroll()
                    .children(rows),
            )
            .child(qa_text_layer("dialog", &labels.translate("notifications")));
        div()
            .absolute()
            .top_full()
            .right_0()
            .w_0()
            .child(
                deferred(
                    gpui_kit::anchored()
                        .anchor(gpui_kit::Anchor::TopRight)
                        .snap_to_window()
                        .child(panel),
                )
                .with_priority(POPOVER_LAYER_PRIORITY),
            )
            .into_any_element()
    }

    fn render_header(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        unread_count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let summary = if unread_count > 0 {
            labels.translate_with("unreadCount", &[("count", &unread_count.to_string())])
        } else {
            labels.translate("allCaughtUp")
        };
        let mark_all_label = labels.translate(if self.is_marking_all_read {
            "markingAllRead"
        } else {
            "markAllRead"
        });
        let is_unread_only = self.is_unread_only;
        let tab = |id: &'static str, label: String, count: Option<usize>, is_selected: bool| {
            let base = div()
                .id(id)
                .relative()
                .flex()
                .rounded(radius::MD)
                .px(px(10.0))
                .py(spacing(1.0))
                .text_scale(text_scale::XS)
                .css_weight(500)
                .cursor_pointer()
                .child(label.clone())
                .children(count.filter(|count| *count > 0).map(|count| {
                    div()
                        .ml(px(6.0))
                        .text_color(if is_selected {
                            fade(theme.text.inverse, 0.8)
                        } else {
                            theme.text.muted
                        })
                        .child(count.to_string())
                }))
                .child(qa_button_layer(&label, &label));
            if is_selected {
                base.bg(theme.brand.primary).text_color(theme.text.inverse)
            } else {
                base.text_color(theme.text.secondary)
            }
        };
        let mark_all = div()
            .id("notification-mark-all")
            .relative()
            .text_scale(text_scale::XS)
            .text_color(theme.text.secondary)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.mark_all_notifications_read(cx)))
            .child(mark_all_label.clone())
            .child(qa_button_layer(&mark_all_label, &mark_all_label));
        let mark_all = if self.is_marking_all_read {
            mark_all.opacity(0.6)
        } else {
            mark_all
        };
        div()
            .border_b_1()
            .border_color(theme.border.default)
            .px(spacing(4.0))
            .py(spacing(3.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .child(
                                div()
                                    .text_scale(text_scale::SM)
                                    .css_weight(600)
                                    .text_color(theme.text.primary)
                                    .child(labels.translate("notifications")),
                            )
                            .child(
                                div()
                                    .text_scale(text_scale::XS)
                                    .text_color(theme.text.muted)
                                    .child(summary),
                            ),
                    )
                    .child(mark_all),
            )
            .child(
                div().mt(spacing(3.0)).flex().child(
                    div()
                        .flex()
                        .gap(px(2.0))
                        .rounded(radius::LG)
                        .bg(theme.background.page)
                        .p(px(2.0))
                        .child(
                            tab(
                                "notification-filter-all",
                                labels.translate("notificationFilterAll"),
                                None,
                                !is_unread_only,
                            )
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_unread_only(false, cx)),
                            ),
                        )
                        .child(
                            tab(
                                "notification-filter-unread",
                                labels.translate("notificationFilterUnread"),
                                Some(unread_count),
                                is_unread_only,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.set_unread_only(true, cx))),
                        ),
                ),
            )
    }

    fn render_notification_row(
        &self,
        theme: &ThemeTokens,
        notification: &AppNotification,
        index: usize,
        is_highlighted: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let background = if is_highlighted {
            fade(theme.brand.primary, 0.1)
        } else if notification.is_read {
            theme.background.surface
        } else {
            fade(theme.brand.primary, 0.05)
        };
        let is_opening = self.opening_notification_id.as_deref() == Some(notification.id.as_str());
        let marker = if is_opening {
            div()
                .mt(px(2.0))
                .size(px(12.0))
                .flex_none()
                .child(spinner_ring(theme.text.muted, cx))
                .into_any_element()
        } else {
            let dot_color = if notification.is_read {
                theme.border.default
            } else {
                theme.brand.primary
            };
            div()
                .mt(spacing(1.0))
                .size(px(10.0))
                .flex_none()
                .rounded(radius::FULL)
                .bg(dot_color)
                .into_any_element()
        };
        let clicked = notification.clone();
        let hover_background = theme.background.page;
        div()
            .id(SharedString::from(format!(
                "notification-{}",
                notification.id
            )))
            .relative()
            .w_full()
            .border_b_1()
            .border_color(theme.border.subtle)
            .px(spacing(4.0))
            .py(spacing(3.0))
            .bg(background)
            .hover(move |style| style.bg(hover_background))
            .cursor_pointer()
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.highlighted_index != index {
                    this.highlighted_index = index;
                    cx.notify();
                }
            }))
            .on_click(
                cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                    this.open_notification(clicked.clone(), event.modifiers().shift, window, cx)
                }),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(spacing(3.0))
                    .child(marker)
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap(spacing(3.0))
                                    .child(
                                        div()
                                            .text_scale(text_scale::SM)
                                            .css_weight(500)
                                            .text_color(theme.text.primary)
                                            .child(notification.title.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_scale(text_scale::PX_11)
                                            .text_color(theme.text.muted)
                                            .child(format_date_time(&notification.created_at)),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(spacing(1.0))
                                    .text_scale(text_scale::XS)
                                    .text_color(theme.text.secondary)
                                    .child(notification.body.clone()),
                            ),
                    ),
            )
            .child(qa_text_layer("button", &notification.body))
            .into_any_element()
    }

    /// 백그라운드 동기화 칸(TS `BackgroundSyncStatus`)
    fn render_background_sync_status(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let locale = self.locale;
        let sync = self.background_sync.read(cx).state().clone();
        let labels = Translator::new(locale, "common.backgroundSyncStatus");
        let message = sync.message(locale);
        let message_color = if sync.is_failed() {
            theme.status.error
        } else {
            theme.text.secondary
        };
        let summary = div()
            .min_w_0()
            .child(
                div()
                    .text_scale(text_scale::SM)
                    .css_weight(600)
                    .text_color(theme.text.primary)
                    .child(labels.translate("title")),
            )
            .child(
                div()
                    .relative()
                    .mt(spacing(1.0))
                    .text_scale(text_scale::XS)
                    .text_color(message_color)
                    .child(message.clone())
                    .child(qa_text_layer("status", &message)),
            );
        let review = self.render_review_button(theme, &labels, cx);
        let progress = sync
            .is_running()
            .then(|| sync_progress(theme, &sync, &labels));
        div()
            .border_b_1()
            .border_color(theme.border.default)
            .bg(fade(theme.brand.primary, 0.05))
            .px(spacing(4.0))
            .py(spacing(3.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(spacing(3.0))
                    .child(summary)
                    .child(self.render_background_sync_button(theme, cx)),
            )
            .children(review)
            .children(progress)
    }

    /// 지우는 중이거나 검토 알림이 있으면 "탐지 항목 N개"/"정리 중" 버튼과 계속 진행 안내
    fn render_review_button(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let review_notification = self.review_notification(cx);
        let review = self.sync_review.read(cx);
        let is_cleaning = review.is_cleaning();
        if !is_cleaning && review_notification.is_none() {
            return None;
        }
        let count = review_notification
            .as_ref()
            .map_or(0, |notification| review.review_count(notification));
        let label = labels.translate_with(
            if is_cleaning { "cleaning" } else { "detected" },
            &[("count", &count.to_string())],
        );
        let hover_brand = theme.brand.hover;
        Some(
            div()
                .mt(spacing(3.0))
                .child(
                    div().flex().child(
                        div()
                            .id("background-sync-review")
                            .relative()
                            .rounded(radius::MD)
                            .bg(theme.brand.primary)
                            .px(spacing(3.0))
                            .py(spacing(2.0))
                            .text_scale(text_scale::XS)
                            .css_weight(500)
                            .text_color(theme.text.inverse)
                            .cursor_pointer()
                            .hover(move |style| style.bg(hover_brand))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_review(window, cx)),
                            )
                            .child(label.clone())
                            .child(qa_button_layer(&label, &label)),
                    ),
                )
                .child(
                    div()
                        .mt(spacing(2.0))
                        .text_scale(text_scale::XS)
                        .text_color(theme.text.muted)
                        .child(labels.translate("cleanupContinues")),
                ),
        )
    }

    fn render_background_sync_button(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let sync = self.background_sync.read(cx).state();
        let label = sync.button_label(self.locale);
        let is_disabled = sync.is_button_disabled();
        let hover_background = theme.background.page;
        let button = div()
            .id("background-sync-start")
            .relative()
            .flex_none()
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .px(spacing(3.0))
            .py(spacing(2.0))
            .text_scale(text_scale::XS)
            .text_color(theme.text.primary)
            .child(label.clone())
            .child(qa_button_layer(&label, &label));
        if is_disabled {
            return button.opacity(0.6);
        }
        button
            .cursor_pointer()
            .hover(move |style| style.bg(hover_background))
            .on_click(cx.listener(|this, _, _, cx| {
                this.background_sync
                    .update(cx, |sync, cx| sync.press_button(cx))
            }))
    }

    /// TS 대상 작업 없음 안내(`fixed inset-0 z-[500] … max-w-sm`)
    fn render_missing_task_notice(
        &self,
        theme: &ThemeTokens,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let labels = Translator::new(self.locale, "common");
        let title = labels.translate("notificationTaskMissingTitle");
        let confirm_label = labels.translate("confirm");
        let hover_brand = theme.brand.hover;
        let panel = crate::components::dialog::dialog_panel(
            theme,
            crate::components::dialog::DialogWidth::Small,
        )
        .relative()
        .child(qa_text_layer("dialog", &title))
        .child(
            div()
                .text_scale(text_scale::LG)
                .css_weight(600)
                .text_color(theme.text.primary)
                .child(title.clone()),
        )
        .child(
            div()
                .mt(spacing(2.0))
                .text_scale(text_scale::SM)
                .text_color(theme.text.secondary)
                .child(labels.translate("notificationTaskMissingBody")),
        )
        .child(
            div().mt(spacing(5.0)).flex().justify_end().child(
                div()
                    .id("notification-missing-task-confirm")
                    .relative()
                    .rounded(radius::MD)
                    .bg(theme.brand.primary)
                    .px(spacing(4.0))
                    .py(px(6.0))
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.inverse)
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover_brand))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.missing_task_notification = None;
                        cx.notify();
                    }))
                    .child(confirm_label.clone())
                    .child(qa_button_layer(&confirm_label, &confirm_label)),
            ),
        );
        modal_layer(
            theme,
            window.viewport_size(),
            div()
                .px(spacing(4.0))
                .w_full()
                .flex()
                .justify_center()
                .child(panel),
        )
        .into_any_element()
    }
}

fn unread_badge(theme: &ThemeTokens, badge: String) -> impl IntoElement {
    div()
        .absolute()
        .top(px(-4.0))
        .right(px(-4.0))
        .min_w(spacing(4.0))
        .h(spacing(4.0))
        .rounded(radius::FULL)
        .bg(theme.brand.primary)
        .px(spacing(1.0))
        .text_scale(text_scale::PX_10)
        .css_weight(600)
        .text_color(theme.text.inverse)
        .flex()
        .items_center()
        .justify_center()
        .child(badge)
}

/// 세 단계 진행 막대와 "닫아도 계속" 안내
fn sync_progress(
    theme: &ThemeTokens,
    sync: &crate::views::locale_shell::background_sync::BackgroundSyncState,
    labels: &Translator,
) -> impl IntoElement {
    let completed = sync
        .status
        .as_ref()
        .map(|status| status.completed_stages)
        .unwrap_or(0);
    let segments = (0..3u32).map(|stage| {
        let color = if stage < completed {
            theme.brand.primary
        } else if stage == completed {
            fade(theme.brand.primary, 0.5)
        } else {
            theme.border.default
        };
        div().h(px(4.0)).flex_1().rounded(radius::FULL).bg(color)
    });
    div()
        .child(
            div()
                .mt(spacing(3.0))
                .flex()
                .gap(spacing(1.0))
                .children(segments),
        )
        .child(
            div()
                .mt(spacing(2.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(labels.translate("continues")),
        )
}

/// 동기화 중 표시(TS `h-3 w-3 rounded-full border-2 border-brand-primary border-t-transparent animate-spin`)
fn sync_spinner(theme: &ThemeTokens, label: &str, cx: &App) -> impl IntoElement + use<> {
    div()
        .absolute()
        .top(px(-4.0))
        .right(px(-4.0))
        .size(px(12.0))
        .child(qa_text_layer("status", label))
        .child(spinner_ring(theme.brand.primary, cx))
}

/// 위쪽 테두리만 투명한 2px 고리. QA 재생(움직임 줄이기)이 아니면 1초에 한 바퀴 돈다
fn spinner_ring(color: Rgba, cx: &App) -> impl IntoElement + use<> {
    let is_animated = !cx.has_global::<crate::qa::QaRuntime>();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let turn = if is_animated {
                window.request_animation_frame();
                spinner_turn()
            } else {
                0.0
            };
            paint_spinner_ring(bounds, turn, color, window);
        },
    )
    .size_full()
}

/// 지금 회전 각(라디안, 1초에 한 바퀴)
fn spinner_turn() -> f32 {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (elapsed.as_millis() % 1000) as f32 / 1000.0 * std::f32::consts::TAU
}

/// 2px 테두리 고리 가운데 위쪽 4분의 1(-135°~-45°)을 뺀 호
fn paint_spinner_ring(bounds: Bounds<Pixels>, turn: f32, color: Rgba, window: &mut Window) {
    let center = bounds.center();
    let radius = f32::from(bounds.size.width) / 2.0 - 1.0;
    let at = |angle: f32| {
        let angle = angle + turn;
        point(
            center.x + px(radius * angle.cos()),
            center.y + px(radius * angle.sin()),
        )
    };
    let quarter = std::f32::consts::FRAC_PI_4;
    let mut builder = PathBuilder::stroke(px(2.0));
    builder.move_to(at(-quarter));
    builder.arc_to(
        point(px(radius), px(radius)),
        px(0.0),
        true,
        true,
        at(-3.0 * quarter),
    );
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// 동기화 실패 표시(TS `h-4 w-4 rounded-full bg-status-error text-[10px] font-semibold text-white` "!")
fn sync_failed_badge(theme: &ThemeTokens, label: &str) -> impl IntoElement + use<> {
    div()
        .absolute()
        .top(px(-4.0))
        .right(px(-4.0))
        .size(spacing(4.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(radius::FULL)
        .bg(theme.status.error)
        .text_scale(text_scale::PX_10)
        .css_weight(600)
        .text_color(gpui_kit::white())
        .child("!")
        .child(qa_text_layer("status", label))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(id: &str, created_at: &str, is_read: bool) -> AppNotification {
        AppNotification {
            id: id.into(),
            title: id.into(),
            body: String::new(),
            task_id: None,
            relative_path: "/ko".into(),
            locale: "ko".into(),
            is_read,
            created_at: created_at.into(),
            dedupe_key: id.into(),
            action: None,
        }
    }

    // parity: TS-components-NotificationCenterButton-b19efb
    #[test]
    fn newest_notification_comes_first() {
        let sorted = newest_first(&[
            notification("old", "2026-01-15T02:20:00.000Z", false),
            notification("new", "2026-01-15T02:57:00.000Z", false),
        ]);
        assert_eq!(sorted[0].id, "new");
    }

    // parity: TS-components-NotificationCenterButton-d68d74
    #[test]
    fn unread_only_hides_read_notifications() {
        let visible = visible_notifications(
            &[
                notification("read", "2026-01-15T02:20:00.000Z", true),
                notification("unread", "2026-01-15T02:57:00.000Z", false),
            ],
            true,
        );
        let ids: Vec<_> = visible.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["unread"]);
    }
}
