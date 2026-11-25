//! 알림 센터 버튼과 목록(TS `NotificationCenterButton`). 보드 머리와 작업 상세 dock이 같은 요소를 쓴다.
//! 목록은 런타임 알림 저장소(`NotificationCenter` 전역)에서 읽고, 알림을 누르면 읽음으로 바꾼 뒤 대상 작업 상세로 간다.
//! 같은 작업 창이 이미 열려 있으면 그 창을 앞으로 가져온다(`navigate_to_task_detail`).
//!
//! TS 버튼: `relative rounded-md p-1.5 text-text-muted hover:bg-bg-page hover:text-text-primary`, 배지
//! `absolute -right-1 -top-1 min-w-4 h-4 rounded-full bg-brand-primary px-1 text-[10px] font-semibold text-white`.
//! 패널: `absolute right-0 z-50 mt-2 w-96 rounded-xl border border-border-default bg-bg-surface shadow-xl`.
//! 아직 옮기지 않은 것: 화살표·Enter·Space 키보드 선택, background sync 상태 줄, 작업이 지워진 알림 안내 대화상자.

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, MouseButton, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Window, anchored, deferred,
    div, px,
};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::notifications::AppNotification;

use crate::app_events::{AppEvents, NotificationsChanged};
use crate::components::icon::{IconName, icon};
use crate::components::popover::POPOVER_LAYER_PRIORITY;
use crate::components::{TailwindText, box_shadows, fade};
use crate::notifications::{NotificationCenter, reload_notification_center, unread_badge_label};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::services::AppServices;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::WindowNavigator;
use crate::windows::{TaskNavigationRequest, navigate_to_task_detail};

/// 패널 폭(`w-96`)
const PANEL_WIDTH: f32 = 384.0;
/// 목록 최대 높이(`max-h-[28rem]`)
const LIST_MAX_HEIGHT: f32 = 448.0;

pub struct NotificationCenterButton {
    locale: Locale,
    navigator: WindowNavigator,
    is_open: bool,
    is_unread_only: bool,
    action_failure: Option<String>,
    _notifications_changed: Option<Subscription>,
}

impl NotificationCenterButton {
    pub fn new(locale: Locale, navigator: WindowNavigator, cx: &mut Context<Self>) -> Self {
        let notifications_changed = cx
            .try_global::<AppEvents>()
            .map(|events| events.0.clone())
            .map(|bus| cx.subscribe(&bus, |_, _, _: &NotificationsChanged, cx| cx.notify()));
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
            is_open: false,
            is_unread_only,
            action_failure: None,
            _notifications_changed: notifications_changed,
        }
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        self.is_open = !self.is_open;
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.is_open {
            self.is_open = false;
            cx.notify();
        }
    }

    fn change_unread_only_filter(&mut self, is_unread_only: bool, cx: &mut Context<Self>) {
        self.is_unread_only = is_unread_only;
        if let Some(runtime) = cx.global::<AppServices>().runtime() {
            let _ = runtime
                .app_settings()
                .set_notification_unread_only_enabled(is_unread_only);
        }
        cx.notify();
    }

    fn mark_all_read(&mut self, cx: &mut Context<Self>) {
        let Some(runtime) = cx.global::<AppServices>().runtime().cloned() else {
            return;
        };
        self.action_failure = runtime
            .notification_store()
            .mark_all_notifications_read()
            .err()
            .map(|error| error.to_string());
        reload_notification_center(cx);
        cx.notify();
    }

    /// 읽음으로 바꾸고 패널을 닫은 뒤 작업 상세(없으면 알림의 경로)로 간다. Shift를 누르면 새 창으로 연다
    fn open_notification(
        &mut self,
        notification: &AppNotification,
        should_open_new_window: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(runtime) = cx.global::<AppServices>().runtime().cloned()
            && !notification.is_read
        {
            let _ = runtime
                .notification_store()
                .mark_notification_read(&notification.id);
            reload_notification_center(cx);
        }
        self.is_open = false;
        cx.notify();
        let Some(task_id) = notification.task_id.as_deref() else {
            self.navigator.navigate(&notification.relative_path, cx);
            return;
        };
        let request = TaskNavigationRequest {
            origin_window: Some(window.window_handle().window_id()),
            should_open_new_window,
        };
        let locale = kanvibe_domain::task_notifications::notification_locale(&notification.locale);
        navigate_to_task_detail(task_id, locale, request, cx);
    }
}

impl Render for NotificationCenterButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let common = Translator::new(self.locale, "common");
        let center = cx.default_global::<NotificationCenter>();
        let notifications = center.notifications().to_vec();
        let unread_count = center.unread_count();
        let mut root =
            div()
                .relative()
                .child(self.render_trigger(&theme, &common, unread_count, cx));
        if self.is_open {
            root = root.child(self.render_panel(&theme, &common, &notifications, unread_count, cx));
        }
        root
    }
}

impl NotificationCenterButton {
    fn render_trigger(
        &self,
        theme: &ThemeTokens,
        common: &Translator,
        unread_count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label: SharedString = common.translate("notifications").into();
        let hover_background = theme.background.page;
        div()
            .id("notification-center-button")
            .relative()
            .rounded(radius::MD)
            .p(spacing(1.5))
            .cursor_pointer()
            .hover(move |style| style.bg(hover_background))
            .on_click(cx.listener(|this, _, _, cx| this.toggle(cx)))
            .child(icon(IconName::Bell, px(20.0)).text_color(theme.text.muted))
            .children(unread_badge_label(unread_count).map(|badge| unread_badge(theme, badge)))
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: label,
                text: SharedString::default(),
            }))
    }

    fn render_panel(
        &self,
        theme: &ThemeTokens,
        common: &Translator,
        notifications: &[AppNotification],
        unread_count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let visible: Vec<AppNotification> = notifications
            .iter()
            .filter(|notification| !self.is_unread_only || !notification.is_read)
            .cloned()
            .collect();
        let panel = div()
            .id("notification-center-panel")
            .occlude()
            .mt(spacing(2.0))
            .w(px(PANEL_WIDTH))
            .overflow_hidden()
            .rounded(radius::XL)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .shadow(box_shadows(&theme.shadow.lg))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close(cx)))
            .child(self.render_header(theme, common, unread_count, cx))
            .children(self.action_failure.clone().map(|reason| {
                div()
                    .mx(spacing(4.0))
                    .mt(spacing(3.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.status.error)
                    .child(common.translate_with("markAllReadFailed", &[("reason", &reason)]))
            }))
            .child(self.render_list(theme, common, visible, cx));
        deferred(anchored().snap_to_window().child(panel)).with_priority(POPOVER_LAYER_PRIORITY)
    }

    fn render_header(
        &self,
        theme: &ThemeTokens,
        common: &Translator,
        unread_count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let unread_label = common.translate("notificationFilterUnread");
        let all_label = common.translate("notificationFilterAll");
        let mark_all_read = div()
            .id("notification-mark-all-read")
            .cursor_pointer()
            .text_scale(text_scale::XS)
            .text_color(theme.text.secondary)
            .on_click(cx.listener(|this, _, _, cx| this.mark_all_read(cx)))
            .child(common.translate("markAllRead"));
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
                    .child(header_title(theme, common, unread_count))
                    .child(mark_all_read),
            )
            .child(
                div()
                    .mt(spacing(3.0))
                    .flex()
                    .w_auto()
                    .gap(px(2.0))
                    .rounded(radius::LG)
                    .bg(theme.background.page)
                    .p(px(2.0))
                    .child(self.filter_tab(theme, all_label, None, false, cx))
                    .child(self.filter_tab(theme, unread_label, Some(unread_count), true, cx)),
            )
    }

    fn filter_tab(
        &self,
        theme: &ThemeTokens,
        label: String,
        count: Option<usize>,
        is_unread_only_tab: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_selected = self.is_unread_only == is_unread_only_tab;
        let (background, text) = if is_selected {
            (Some(theme.brand.primary), theme.text.inverse)
        } else {
            (None, theme.text.secondary)
        };
        let count_text = count.filter(|count| *count > 0).map(|count| {
            let color = if is_selected {
                fade(theme.text.inverse, 0.8)
            } else {
                theme.text.muted
            };
            div()
                .ml(spacing(1.5))
                .text_color(color)
                .child(count.to_string())
        });
        let mut tab = div()
            .id(if is_unread_only_tab {
                "notification-filter-unread"
            } else {
                "notification-filter-all"
            })
            .flex()
            .rounded(radius::MD)
            .px(spacing(2.5))
            .py(spacing(1.0))
            .text_scale(text_scale::XS)
            .css_weight(500)
            .text_color(text)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change_unread_only_filter(is_unread_only_tab, cx)
            }))
            .child(label)
            .children(count_text);
        if let Some(background) = background {
            tab = tab.bg(background);
        }
        tab
    }

    fn render_list(
        &self,
        theme: &ThemeTokens,
        common: &Translator,
        visible: Vec<AppNotification>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if visible.is_empty() {
            let empty_key = if self.is_unread_only {
                "noUnreadNotifications"
            } else {
                "noNotifications"
            };
            return div()
                .px(spacing(4.0))
                .py(spacing(8.0))
                .text_center()
                .text_scale(text_scale::SM)
                .text_color(theme.text.muted)
                .child(common.translate(empty_key))
                .into_any_element();
        }
        let mut rows = Vec::with_capacity(visible.len());
        for (index, notification) in visible.into_iter().enumerate() {
            rows.push(notification_row(theme, index, notification, cx).into_any_element());
        }
        div()
            .id("notification-list")
            .max_h(px(LIST_MAX_HEIGHT))
            .overflow_y_scroll()
            .children(rows)
            .into_any_element()
    }
}

/// 제목과 읽지 않은 수 요약(`읽지 않음 {count}개` 또는 `모든 알림을 확인했습니다.`)
fn header_title(theme: &ThemeTokens, common: &Translator, unread_count: usize) -> impl IntoElement {
    let summary = if unread_count > 0 {
        common.translate_with("unreadCount", &[("count", &unread_count.to_string())])
    } else {
        common.translate("allCaughtUp")
    };
    div()
        .child(
            div()
                .text_scale(text_scale::SM)
                .css_weight(600)
                .text_color(theme.text.primary)
                .child(common.translate("notifications")),
        )
        .child(
            div()
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(summary),
        )
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

/// 알림 한 줄. 읽지 않은 알림은 `bg-brand-primary/5`와 brand 점으로 구분한다
fn notification_row(
    theme: &ThemeTokens,
    index: usize,
    notification: AppNotification,
    cx: &mut Context<NotificationCenterButton>,
) -> impl IntoElement {
    let (row_background, dot_color) = if notification.is_read {
        (theme.background.surface, theme.border.default)
    } else {
        (fade(theme.brand.primary, 0.05), theme.brand.primary)
    };
    let hover_background = theme.background.page;
    let title: SharedString = notification.title.clone().into();
    div()
        .id(("notification-row", index))
        .w_full()
        .border_b_1()
        .border_color(theme.border.subtle)
        .px(spacing(4.0))
        .py(spacing(3.0))
        .bg(row_background)
        .hover(move |style| style.bg(hover_background))
        .cursor_pointer()
        .flex()
        .items_start()
        .gap(spacing(3.0))
        .child(
            div()
                .mt(spacing(1.0))
                .size(px(10.0))
                .flex_none()
                .rounded(radius::FULL)
                .bg(dot_color),
        )
        .child(notification_text(theme, &notification))
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: title.clone(),
            text: title,
        }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(
            cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                let should_open_new_window = event.modifiers().shift;
                this.open_notification(&notification, should_open_new_window, window, cx);
            }),
        )
}

fn notification_text(theme: &ThemeTokens, notification: &AppNotification) -> impl IntoElement {
    div()
        .min_w_0()
        .flex_1()
        .child(
            div()
                .text_scale(text_scale::SM)
                .css_weight(500)
                .text_color(theme.text.primary)
                .child(notification.title.clone()),
        )
        .child(
            div()
                .mt(spacing(1.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.secondary)
                .child(notification.body.clone()),
        )
}
