//! 알림 센터(TS `NotificationCenterButton` + `BackgroundSyncStatus`). 최신 알림이 위에 오고 처음에는 맨 위 알림을 고른 채 열린다.
//! ↑↓로 고르고 Enter(새 창은 Shift+Enter)로 열며, Space는 열지 않고 읽음만 표시하고, Esc는 닫는다.
//! 알림을 누르면 읽음으로 바꾼 뒤 대상 작업 상세로 간다. 같은 작업 창이 있으면 그 창을 앞으로 가져온다.

use gpui_kit::{
    AnyElement, AnyWindowHandle, Context, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, ParentElement, StatefulInteractiveElement, Styled, Window, deferred, div, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::notifications::AppNotification;

use super::BoardView;
use super::header::{qa_button_layer, qa_text_layer};
use crate::components::popover::POPOVER_LAYER_PRIORITY;
use crate::components::{TailwindText, box_shadows, fade};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::window_open::{
    resolve_navigation_target_window, should_keep_current_route_for_notification_activation,
};
use crate::windows::{TaskNavigationRequest, WindowRegistry, navigate_to_task_detail};

/// 알림 센터 열림 상태
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct NotificationPanelState {
    pub is_open: bool,
    pub is_unread_only: bool,
    pub highlighted_index: usize,
    pub opening_notification_id: Option<String>,
    pub is_marking_all_read: bool,
    pub mark_all_read_failure: Option<String>,
    pub missing_task_notification: Option<String>,
    /// 단축키로 열었으면 패널에 키보드 포커스 링을 그린다(Chromium `:focus-visible`)
    pub is_keyboard_opened: bool,
}

/// 최신 알림이 위로(`createdAt` 내림차순)
pub(super) fn newest_first(notifications: &[AppNotification]) -> Vec<AppNotification> {
    let mut sorted = notifications.to_vec();
    sorted.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    sorted
}

/// 안읽음만 보기면 읽은 알림을 뺀다
pub(super) fn visible_notifications(
    notifications: &[AppNotification],
    is_unread_only: bool,
) -> Vec<AppNotification> {
    newest_first(notifications)
        .into_iter()
        .filter(|notification| !is_unread_only || !notification.is_read)
        .collect()
}

/// `new Date(iso).toLocaleString()`(en-US, UTC): `1/15/2026, 2:57:00 AM`
pub(super) fn format_notification_time(iso: &str) -> String {
    let parse =
        |range: std::ops::Range<usize>| iso.get(range).and_then(|part| part.parse::<u32>().ok());
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        parse(0..4),
        parse(5..7),
        parse(8..10),
        parse(11..13),
        parse(14..16),
        parse(17..19),
    ) else {
        return iso.to_string();
    };
    let (display_hour, meridiem) = match hour {
        0 => (12, "AM"),
        1..=11 => (hour, "AM"),
        12 => (12, "PM"),
        _ => (hour - 12, "PM"),
    };
    format!("{month}/{day}/{year}, {display_hour}:{minute:02}:{second:02} {meridiem}")
}

/// 열린 창과 각 창의 현재 경로
fn open_window_paths(cx: &gpui_kit::App) -> Vec<(gpui_kit::WindowId, String)> {
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

impl BoardView {
    /// 대상 작업이 사라져 안내를 띄운 알림
    pub fn missing_task_notification_id(&self) -> Option<String> {
        self.notification_panel.missing_task_notification.clone()
    }

    pub(super) fn is_notification_center_open(&self) -> bool {
        self.notification_panel.is_open
    }

    /// 단축키(`Mod+Shift+I`)로 연다
    pub(super) fn toggle_notification_center_by_keyboard(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_notification_center(window, cx);
        self.notification_panel.is_keyboard_opened = self.notification_panel.is_open;
    }

    pub(super) fn toggle_notification_center(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.notification_panel.is_keyboard_opened = false;
        if self.notification_panel.is_open {
            self.close_notification_center(window, cx);
            return;
        }
        self.notification_panel.is_open = true;
        self.notification_panel.highlighted_index = 0;
        self.notification_focus.focus(window, cx);
        cx.notify();
    }

    fn close_notification_center(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.notification_panel.is_open {
            return;
        }
        self.notification_panel.is_open = false;
        self.board_focus.focus(window, cx);
        cx.notify();
    }

    fn current_visible_notifications(&self) -> Vec<AppNotification> {
        let notifications = self
            .board
            .as_ref()
            .map(|board| board.notifications.as_slice())
            .unwrap_or_default();
        visible_notifications(notifications, self.notification_panel.is_unread_only)
    }

    fn set_unread_only(&mut self, is_unread_only: bool, cx: &mut Context<Self>) {
        self.notification_panel.is_unread_only = is_unread_only;
        self.notification_panel.highlighted_index = 0;
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

    fn set_notification_read_locally(&mut self, notification_id: &str, cx: &mut Context<Self>) {
        if let Some(board) = self.board.as_mut()
            && let Some(notification) = board
                .notifications
                .iter_mut()
                .find(|item| item.id == notification_id)
        {
            notification.is_read = true;
        }
        if cx.has_global::<crate::notifications::NotificationCenter>() {
            cx.global_mut::<crate::notifications::NotificationCenter>()
                .mark_read(notification_id);
        }
        cx.notify();
    }

    /// Space: 열지 않고 읽음만 표시한다. 이미 읽은 알림이면 다시 저장하지 않는다
    fn mark_highlighted_read(&mut self, cx: &mut Context<Self>) {
        let visible = self.current_visible_notifications();
        let Some(notification) = visible
            .get(self.notification_panel.highlighted_index)
            .cloned()
        else {
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
                Ok(()) => this.set_notification_read_locally(&notification.id, cx),
                Err(failure) => {
                    let message = Translator::new(this.locale(), "common").translate_with(
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
    pub(super) fn open_notification(
        &mut self,
        notification: AppNotification,
        should_open_new_window: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.notification_panel.opening_notification_id.is_some() {
            return;
        }
        let Some(services) = self.services.clone() else {
            return;
        };
        self.notification_panel.opening_notification_id = Some(notification.id.clone());
        let mark_read = (!notification.is_read)
            .then(|| services.mark_notification_read(notification.id.clone(), cx));
        let check_task = services.activate_notification(notification.task_id.clone(), cx);
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let read_result = match mark_read {
                Some(task) => task.await,
                None => Ok(()),
            };
            let task_result = match read_result {
                Ok(()) => check_task.await,
                Err(failure) => Err(failure),
            };
            let _ = this.update(cx, |this, cx| {
                this.notification_panel.opening_notification_id = None;
                match task_result {
                    Ok(does_task_exist) => {
                        this.set_notification_read_locally(&notification.id, cx);
                        this.finish_opening_notification(
                            &notification,
                            does_task_exist,
                            should_open_new_window,
                            window_handle,
                            cx,
                        );
                    }
                    Err(failure) => {
                        let message = Translator::new(this.locale(), "common").translate_with(
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
        cx: &mut Context<Self>,
    ) {
        self.notification_panel.is_open = false;
        let action = notification.action.clone().flatten();
        if should_keep_current_route_for_notification_activation(action.as_ref()) {
            // 백그라운드 동기화 결과는 지금 화면 위에 검토 대화상자로 연다(전역 대화상자 담당)
            return;
        }
        if let Some(task_id) = &notification.task_id {
            if !does_task_exist {
                self.notification_panel.missing_task_notification = Some(notification.id.clone());
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
            _ => {
                let navigator = self.screen.navigator.clone();
                navigator.navigate(&notification.relative_path, cx);
            }
        }
    }

    fn mark_all_notifications_read(&mut self, cx: &mut Context<Self>) {
        if self.notification_panel.is_marking_all_read {
            return;
        }
        let Some(services) = self.services.clone() else {
            return;
        };
        self.notification_panel.is_marking_all_read = true;
        self.notification_panel.mark_all_read_failure = None;
        let save = services.mark_all_notifications_read(cx);
        cx.spawn(async move |this, cx| {
            let result = save.await;
            let _ = this.update(cx, |this, cx| {
                this.notification_panel.is_marking_all_read = false;
                match result {
                    Ok(()) => {
                        let ids: Vec<String> = this
                            .board
                            .as_ref()
                            .map(|board| {
                                board
                                    .notifications
                                    .iter()
                                    .map(|item| item.id.clone())
                                    .collect()
                            })
                            .unwrap_or_default();
                        for id in ids {
                            this.set_notification_read_locally(&id, cx);
                        }
                    }
                    Err(failure) => {
                        this.notification_panel.mark_all_read_failure = Some(failure.reason)
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn handle_notification_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.has_shortcut_blocker() {
            return;
        }
        let visible = self.current_visible_notifications();
        let count = visible.len();
        let index = self
            .notification_panel
            .highlighted_index
            .min(count.saturating_sub(1));
        match event.keystroke.key.as_str() {
            "down" if count > 0 => self.notification_panel.highlighted_index = (index + 1) % count,
            "up" if count > 0 => {
                self.notification_panel.highlighted_index = (index + count - 1) % count
            }
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
            "escape" => self.close_notification_center(window, cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn render_notification_panel(
        &mut self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let labels = Translator::new(self.locale(), "common");
        let all = self
            .board
            .as_ref()
            .map(|board| board.notifications.clone())
            .unwrap_or_default();
        let unread_count = all.iter().filter(|item| !item.is_read).count();
        let visible = visible_notifications(&all, self.notification_panel.is_unread_only);
        let highlighted = self
            .notification_panel
            .highlighted_index
            .min(visible.len().saturating_sub(1));
        let rows: Vec<AnyElement> = if visible.is_empty() {
            let key = if self.notification_panel.is_unread_only {
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
                    self.render_notification_row(theme, notification, index == highlighted, cx)
                })
                .collect()
        };
        let panel = div()
            .id("notification-panel")
            .track_focus(&self.notification_focus)
            .on_key_down(cx.listener(Self::handle_notification_key))
            .on_mouse_down_out(
                cx.listener(|this, _, window, cx| this.close_notification_center(window, cx)),
            )
            .occlude()
            .relative()
            .mt(spacing(2.0))
            .w(px(384.0))
            .overflow_hidden()
            .rounded(radius::XL)
            .when_focus_ring(self.notification_panel.is_keyboard_opened, theme)
            .bg(theme.background.surface)
            .child(self.render_background_sync_status(theme))
            .child(self.render_notification_header(theme, &labels, unread_count, cx))
            .children(
                self.notification_panel
                    .mark_all_read_failure
                    .clone()
                    .map(|reason| {
                        super::inline_load_error(
                            theme,
                            self.locale(),
                            labels.translate_with("markAllReadFailed", &[("reason", &reason)]),
                            "notification-mark-all-retry",
                            cx.listener(|this, _, _, cx| this.mark_all_notifications_read(cx)),
                        )
                        .mx(spacing(4.0))
                        .mt(spacing(3.0))
                    }),
            )
            .child(
                div()
                    .id("notification-rows")
                    .max_h(px(448.0))
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

    /// 백그라운드 동기화 상태 칸. 동기화 실행 루프는 앱 조립 지점이 맡으므로 여기서는 대기 상태를 보인다
    fn render_background_sync_status(&self, theme: &ThemeTokens) -> impl IntoElement + use<> {
        let labels = Translator::new(self.locale(), "common.backgroundSyncStatus");
        let start_label = labels.translate("start");
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
                    .child(
                        div()
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
                                    .mt(spacing(1.0))
                                    .text_scale(text_scale::XS)
                                    .text_color(theme.text.secondary)
                                    .child(labels.translate("idle")),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .rounded(radius::MD)
                            .border_1()
                            .border_color(theme.border.default)
                            .px(spacing(3.0))
                            .py(spacing(2.0))
                            .text_scale(text_scale::XS)
                            .text_color(theme.text.primary)
                            .child(start_label.clone())
                            .child(qa_button_layer(&start_label, &start_label)),
                    ),
            )
    }

    fn render_notification_header(
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
        let mark_all_label = labels.translate(if self.notification_panel.is_marking_all_read {
            "markingAllRead"
        } else {
            "markAllRead"
        });
        let is_unread_only = self.notification_panel.is_unread_only;
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
                    .child(
                        div()
                            .id("notification-mark-all")
                            .relative()
                            .text_scale(text_scale::XS)
                            .text_color(theme.text.secondary)
                            .cursor_pointer()
                            .on_click(
                                cx.listener(|this, _, _, cx| this.mark_all_notifications_read(cx)),
                            )
                            .child(mark_all_label.clone())
                            .child(qa_button_layer(&mark_all_label, &mark_all_label)),
                    ),
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
        let dot_color = if notification.is_read {
            theme.border.default
        } else {
            theme.brand.primary
        };
        let clicked = notification.clone();
        let hover_background = theme.background.page;
        div()
            .id(gpui_kit::SharedString::from(format!(
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
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
                    .child(
                        div()
                            .mt(spacing(1.0))
                            .size(px(10.0))
                            .flex_none()
                            .rounded(radius::FULL)
                            .bg(dot_color),
                    )
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
                                            .child(format_notification_time(
                                                &notification.created_at,
                                            )),
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
}

/// Chromium 기본 포커스 링: 2px 링과 바깥 1px 테두리. 다크 화면은 흰 링, 라이트 화면은 주황 링이다
trait FocusRing: Styled + Sized {
    fn when_focus_ring(self, is_visible: bool, theme: &ThemeTokens) -> Self {
        if !is_visible {
            return self
                .border_1()
                .border_color(theme.border.default)
                .shadow(box_shadows(&theme.shadow.lg));
        }
        let (ring, halo) = match theme.mode {
            crate::theme::ThemeMode::Dark => (gpui_kit::rgb(0xffffff), gpui_kit::rgb(0x101010)),
            crate::theme::ThemeMode::Light => (gpui_kit::rgb(0xe59700), gpui_kit::rgb(0xffffff)),
        };
        let mut shadows = vec![gpui_kit::BoxShadow {
            color: halo.into(),
            offset: gpui_kit::point(px(0.0), px(0.0)),
            blur_radius: px(0.0),
            spread_radius: px(1.0),
            inset: false,
        }];
        shadows.extend(box_shadows(&theme.shadow.lg));
        self.border_2().border_color(ring).shadow(shadows)
    }
}

impl<E: Styled> FocusRing for E {}

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

    #[test]
    fn notification_times_use_the_us_locale_string() {
        assert_eq!(
            format_notification_time("2026-01-15T02:57:00.000Z"),
            "1/15/2026, 2:57:00 AM"
        );
        assert_eq!(
            format_notification_time("2026-01-15T14:05:09.000Z"),
            "1/15/2026, 2:05:09 PM"
        );
    }
}
