//! 릴리스 업데이트 대화상자(TS `ReleaseUpdateDialog`). 보드가 열리면 새 릴리스를 확인하고 한 시간마다 다시 본다.
//! 다시 보지 않기로 한 버전, 이번 실행에서 이미 보인 버전, 다른 창이 먼저 차지한 버전은 띄우지 않는다.
//! 닫을 때 "이 버전 다시 보지 않기"가 켜져 있으면 AppSettings에 그 버전을 남긴다.

use std::time::Duration;

use gpui_kit::{
    AnyElement, AppContext, Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, point, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_services::release_update::ReleaseUpdate;

use super::BoardView;
use super::header::{qa_button_layer, qa_text_layer};
use super::notification_center::FocusRing;
use super::release_notes::release_notes;
use crate::components::TailwindText;
use crate::services::AppServices;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// TS `RELEASE_UPDATE_CHECK_INTERVAL_MS`
const RELEASE_CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// `z-[540]`
const RELEASE_DIALOG_LAYER_PRIORITY: usize = 540;
/// `max-w-4xl`
const RELEASE_DIALOG_WIDTH: f32 = 896.0;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ReleaseDialogState {
    pub release: Option<ReleaseUpdate>,
    pub should_dismiss_version: bool,
    /// 이번 실행에서 이미 보였거나 다시 보지 않기로 한 버전
    pub shown_versions: Vec<String>,
    pub is_checking: bool,
    /// 창이 앞에 있어 대화상자를 보일 수 있는지(TS `document.visibilityState === "visible" && document.hasFocus()`)
    pub can_present: bool,
}

impl BoardView {
    pub fn release_update_version(&self) -> Option<String> {
        self.release_dialog
            .release
            .as_ref()
            .map(|release| release.version.clone())
    }

    /// 첫 확인과 한 시간마다의 재확인
    pub(super) fn start_release_update_checks(&mut self, cx: &mut Context<Self>) {
        self.check_for_release_update(cx);
        self._release_check_loop = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(RELEASE_CHECK_INTERVAL).await;
                if this
                    .update(cx, |this, cx| this.check_for_release_update(cx))
                    .is_err()
                {
                    return;
                }
            }
        });
    }

    /// 창이 앞으로 오면 다시 확인한다(TS `focus`·`visibilitychange`)
    pub(super) fn observe_release_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // QA 재생은 Playwright처럼 창이 앞에 있다고 본다(Xvfb에는 창 관리자가 없어 활성 창이 없다)
        let is_qa = cx.has_global::<crate::qa::QaRuntime>();
        self.release_dialog.can_present = is_qa || window.is_window_active();
        let subscription = cx.observe_window_activation(window, move |this, window, cx| {
            this.release_dialog.can_present = is_qa || window.is_window_active();
            if this.board.is_some() {
                this.check_for_release_update(cx);
            }
        });
        self._subscriptions.push(subscription);
    }

    fn check_for_release_update(&mut self, cx: &mut Context<Self>) {
        let dialog = &self.release_dialog;
        if dialog.is_checking || dialog.release.is_some() || !dialog.can_present {
            return;
        }
        let Some(runtime) = cx.global::<AppServices>().runtime().cloned() else {
            return;
        };
        self.release_dialog.is_checking = true;
        let check = cx.background_spawn(async move {
            let result = runtime.release_update().check_for_release_update();
            let dismissed = runtime
                .app_settings()
                .get_release_update_dismissed_versions()
                .unwrap_or_default();
            (result, dismissed, runtime)
        });
        cx.spawn(async move |this, cx| {
            let (result, dismissed, runtime) = check.await;
            let _ = this.update(cx, |this, cx| {
                this.release_dialog.is_checking = false;
                let Some(release) = result.release.filter(|_| result.is_update_available) else {
                    return;
                };
                let version = release.version.clone();
                if this.release_dialog.shown_versions.contains(&version) {
                    return;
                }
                this.release_dialog.shown_versions.push(version.clone());
                // 다시 보지 않기로 한 버전이거나 다른 창이 먼저 띄운 버전은 넘긴다
                if dismissed.contains(&version)
                    || !runtime
                        .release_update()
                        .claim_release_update_version(&version)
                {
                    return;
                }
                this.release_dialog.should_dismiss_version = false;
                this.release_dialog.release = Some(release);
                cx.notify();
            });
        })
        .detach();
    }

    fn close_release_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(release) = self.release_dialog.release.take() else {
            return;
        };
        if self.release_dialog.should_dismiss_version
            && let Some(runtime) = cx.global::<AppServices>().runtime()
            && let Err(error) = runtime
                .app_settings()
                .dismiss_release_update_version(&release.version)
        {
            eprintln!("[kanvibe] Failed to dismiss release version: {error}");
        }
        self.release_dialog.should_dismiss_version = false;
        self.board_focus.focus(window, cx);
        cx.notify();
    }

    fn open_release_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(release) = &self.release_dialog.release {
            cx.open_url(&release.html_url);
        }
        self.close_release_dialog(window, cx);
    }

    pub(super) fn render_release_dialog(
        &mut self,
        theme: &ThemeTokens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let release = self.release_dialog.release.clone()?;
        if !self.release_focus.contains_focused(window, cx)
            && !self.release_focus.is_focused(window)
        {
            self.release_focus.focus(window, cx);
        }
        let labels = Translator::new(self.locale(), "common.releaseUpdate");
        let viewport = self.overlay_viewport(window);
        let body = if release.body.is_empty() {
            div()
                .text_scale(text_scale::SM)
                .text_color(theme.text.muted)
                .child(labels.translate("emptyBody"))
        } else {
            release_notes(theme, &release.body)
        };
        let dialog = div()
            .id("release-update-dialog")
            .track_focus(&self.release_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                cx.stop_propagation();
                if event.keystroke.key == "escape" {
                    this.close_release_dialog(window, cx);
                }
            }))
            .occlude()
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .max_w(px(RELEASE_DIALOG_WIDTH))
            .max_h(viewport.height - px(64.0))
            .overflow_hidden()
            .rounded(radius::LG)
            .bg(theme.background.surface)
            .when_focus_ring(true, theme)
            .child(qa_text_layer(
                "dialog",
                &labels.translate_with("title", &[("version", &release.version)]),
            ))
            .child(release_header(theme, &labels, &release))
            .child(
                div()
                    .id("release-notes")
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .px(spacing(5.0))
                    .py(spacing(4.0))
                    .child(body),
            )
            .child(self.render_release_footer(theme, &labels, cx));
        let close = cx.listener(|this, _, window, cx| this.close_release_dialog(window, cx));
        let layer = div()
            .occlude()
            .relative()
            .w(viewport.width)
            .h(viewport.height)
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.background.overlay)
            .px(spacing(4.0))
            .py(spacing(8.0))
            .child(
                div()
                    .id("release-update-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .on_click(close),
            )
            .child(dialog);
        Some(
            deferred(anchored().position(point(px(0.0), px(0.0))).child(layer))
                .with_priority(RELEASE_DIALOG_LAYER_PRIORITY)
                .into_any_element(),
        )
    }

    fn render_release_footer(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let close_label = Translator::new(self.locale(), "common").translate("close");
        let view_label = labels.translate("viewRelease");
        let dismiss_label = labels.translate("dontShowVersionAgain");
        let is_checked = self.release_dialog.should_dismiss_version;
        let checkbox = native_checkbox(theme, is_checked);
        let hover_border = theme.brand.primary;
        let hover_brand = theme.brand.hover;
        div()
            .flex()
            .items_center()
            .justify_end()
            .gap(spacing(2.0))
            .border_t_1()
            .border_color(theme.border.subtle)
            .bg(theme.background.page)
            .px(spacing(5.0))
            .py(spacing(3.0))
            .child(
                div()
                    .id("release-dismiss-version")
                    .relative()
                    .mr_auto()
                    .flex()
                    .items_center()
                    .gap(spacing(2.0))
                    .cursor_pointer()
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.muted)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.release_dialog.should_dismiss_version =
                            !this.release_dialog.should_dismiss_version;
                        cx.notify();
                    }))
                    .child(checkbox)
                    .child(dismiss_label.clone())
                    .child(qa_button_layer(&dismiss_label, &dismiss_label)),
            )
            .child(
                div()
                    .id("release-close")
                    .relative()
                    .rounded(radius::MD)
                    .border_1()
                    .border_color(theme.border.default)
                    .bg(theme.background.surface)
                    .px(spacing(3.0))
                    .py(px(6.0))
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.secondary)
                    .cursor_pointer()
                    .hover(move |style| style.border_color(hover_border))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.close_release_dialog(window, cx)),
                    )
                    .child(close_label.clone())
                    .child(qa_button_layer(&close_label, &close_label)),
            )
            .child(
                div()
                    .id("release-open")
                    .relative()
                    .rounded(radius::MD)
                    .bg(theme.brand.primary)
                    .px(spacing(3.0))
                    .py(px(6.0))
                    .text_scale(text_scale::SM)
                    .css_weight(500)
                    .text_color(theme.text.inverse)
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover_brand))
                    .on_click(cx.listener(|this, _, window, cx| this.open_release_page(window, cx)))
                    .child(view_label.clone())
                    .child(qa_button_layer(&view_label, &view_label)),
            )
    }
}

/// Chromium 기본 체크 상자(16px, 회색 테두리). 다크 화면은 어두운 회색, 라이트 화면은 흰 바탕이다
fn native_checkbox(theme: &ThemeTokens, is_checked: bool) -> impl IntoElement {
    let (background, border) = match theme.mode {
        crate::theme::ThemeMode::Dark => (gpui_kit::rgb(0x3b3b3b), gpui_kit::rgb(0x858585)),
        crate::theme::ThemeMode::Light => (gpui_kit::rgb(0xffffff), gpui_kit::rgb(0x767676)),
    };
    let checkbox = div()
        .size(spacing(4.0))
        .flex_none()
        .rounded(radius::SM)
        .border_1()
        .border_color(border)
        .bg(background);
    if is_checked {
        checkbox
            .bg(theme.brand.primary)
            .border_color(theme.brand.primary)
    } else {
        checkbox
    }
}

/// 머리: 작은 안내·제목·버전 알약·릴리스 이름
fn release_header(
    theme: &ThemeTokens,
    labels: &Translator,
    release: &ReleaseUpdate,
) -> impl IntoElement {
    let title = labels.translate_with("title", &[("version", &release.version)]);
    let eyebrow = div()
        .text_scale(text_scale::PX_11)
        .css_weight(500)
        .text_color(theme.text.muted)
        .child(labels.translate("eyebrow").to_uppercase());
    let heading = div()
        .relative()
        .mt(spacing(1.0))
        .text_scale(text_scale::BASE)
        .css_weight(600)
        .text_color(theme.text.primary)
        .child(title.clone())
        .child(qa_text_layer("heading", &title));
    let pill = div()
        .flex_none()
        .rounded(radius::FULL)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.page)
        .px(px(10.0))
        .py(spacing(1.0))
        .text_scale(text_scale::XS)
        .css_weight(500)
        .text_color(theme.text.secondary)
        .child(format!("v{}", release.version));
    div()
        .border_b_1()
        .border_color(theme.border.subtle)
        .px(spacing(5.0))
        .py(spacing(4.0))
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap(spacing(4.0))
                .child(div().min_w_0().child(eyebrow).child(heading))
                .child(pill),
        )
        .child(
            div()
                .mt(spacing(2.0))
                .truncate()
                .text_scale(text_scale::SM)
                .text_color(theme.text.secondary)
                .child(release.name.clone()),
        )
}
