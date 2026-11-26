//! `Board.tsx` 헤더(Linux 배치). 로고는 창 폭 1280 이상(`xl`)에서만 보인다.
//! 컨트롤 순서: 종류 필터 · 프로젝트 필터 · 데스크톱 필터 · 정렬 · 프로젝트 스캔 · 새 작업 · 설정 · 알림.

use gpui_kit::{
    AnyElement, App, Context, Div, InteractiveElement, IntoElement, ParentElement, Rgba, Stateful,
    StatefulInteractiveElement, Styled, Window, div, img, px, svg,
};
use kanvibe_domain::i18n::{Locale, Translator};

use super::header_popovers::desktop_filter_label;
use super::task_filters::TaskKindFilter;
use super::{BoardView, HeaderPopover};
use crate::assets::{icon_paths, image_paths};
use crate::components::{TailwindText, box_shadows};
use crate::theme::{ThemeMode, ThemeTokens, radius, spacing, text_scale};

/// Tailwind `xl` 경계
const LOGO_MIN_WINDOW_WIDTH: f32 = 1280.0;
/// 원본 1024x278 이미지를 높이 32로 줄인 폭
const LOGO_WIDTH: f32 = 1024.0 * 32.0 / 278.0;

pub(super) struct HeaderLabels {
    pub board: Translator,
    pub settings: Translator,
    pub common: Translator,
    pub sort: Translator,
}

impl HeaderLabels {
    pub fn new(locale: Locale) -> Self {
        Self {
            board: Translator::new(locale, "board"),
            settings: Translator::new(locale, "settings"),
            common: Translator::new(locale, "common"),
            sort: Translator::new(locale, "board.sort"),
        }
    }
}

pub(super) use crate::qa::targets::{qa_button_layer, qa_text_layer};

fn logo(window_width: f32, mode: ThemeMode) -> Option<AnyElement> {
    if window_width < LOGO_MIN_WINDOW_WIDTH {
        return None;
    }
    let logo_path = match mode {
        ThemeMode::Dark => image_paths::LOGO_INVERTED,
        ThemeMode::Light => image_paths::LOGO,
    };
    Some(
        div()
            .mr_auto()
            .flex_none()
            .pr(spacing(6.0))
            .child(img(logo_path).h(spacing(8.0)).w(px(LOGO_WIDTH)))
            .into_any_element(),
    )
}

impl BoardView {
    pub(super) fn render_board_header(
        &mut self,
        theme: &ThemeTokens,
        window_width: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let labels = HeaderLabels::new(self.locale());
        let controls = vec![
            self.render_task_kind_filter(theme, &labels, cx)
                .into_any_element(),
            self.render_project_filter(theme, &labels, false, cx)
                .into_any_element(),
            self.render_desktop_filter(theme, &labels, cx)
                .into_any_element(),
            self.render_sort_button(theme, &labels, cx)
                .into_any_element(),
            self.render_scan_button(theme, &labels, cx)
                .into_any_element(),
            self.render_new_task_button(theme, &labels, cx)
                .into_any_element(),
            self.render_settings_button(theme, &labels, cx)
                .into_any_element(),
            self.render_notification_button().into_any_element(),
        ];
        let header = div().flex().items_center().justify_end().px(spacing(6.0));
        // macOS 데스크톱 앱은 신호등 버튼 자리를 비우고 타이틀바 한 줄에 컨트롤을 놓는다(TS `shouldUseMacTitlebarLayout`)
        let header = if is_mac_titlebar_layout(cx) {
            header
                .bg(theme.background.page)
                .pb(spacing(3.0))
                .pl(spacing(20.0))
                .pt(spacing(10.0))
        } else {
            header
                .border_b_1()
                .border_color(theme.border.default)
                .bg(theme.background.surface)
                .py(spacing(3.0))
        };
        header.children(logo(window_width, theme.mode)).child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_end()
                .gap(spacing(3.0))
                .children(controls),
        )
    }

    fn render_task_kind_filter(
        &mut self,
        theme: &ThemeTokens,
        labels: &HeaderLabels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let active_filter = self.filters.task_kind;
        let buttons = TaskKindFilter::ORDER.into_iter().map(|filter| {
            let label = labels
                .board
                .translate(&format!("taskKindFilter.options.{}", filter.as_str()));
            let is_active = filter == active_filter;
            let button = div()
                .id(filter.as_str())
                .relative()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .rounded(radius::BASE)
                .text_scale(text_scale::SM)
                .css_weight(500)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.set_task_kind_filter(filter, cx)))
                .child(label.clone())
                .child(qa_button_layer(&label, &label));
            if is_active {
                button
                    .bg(theme.brand.primary)
                    .text_color(theme.text.inverse)
                    .shadow(box_shadows(&theme.shadow.sm))
            } else {
                let hover_background = theme.background.surface;
                let hover_text = theme.text.primary;
                button
                    .text_color(theme.text.secondary)
                    .hover(move |style| style.bg(hover_background).text_color(hover_text))
            }
        });
        div()
            .flex()
            .h(px(34.0))
            .w(px(180.0))
            .flex_none()
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.page)
            .p(px(2.0))
            .children(buttons)
    }

    /// 칸반 헤더는 `w-64`, 워크스페이스 목록은 열 폭 전체를 쓴다
    pub(super) fn render_project_filter(
        &mut self,
        theme: &ThemeTokens,
        labels: &HeaderLabels,
        is_full_width: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let placeholder = labels.board.translate("allProjects");
        let selected_names = self.selected_project_names();
        let trigger_text = if selected_names.is_empty() {
            placeholder.clone()
        } else {
            selected_names.join(", ")
        };
        let content: AnyElement = if selected_names.is_empty() {
            div()
                .px(spacing(1.0))
                .text_scale(text_scale::SM)
                .text_color(theme.text.muted)
                .child(placeholder.clone())
                .into_any_element()
        } else {
            self.render_project_chips(theme).into_any_element()
        };
        let is_open = self.open_popover == Some(HeaderPopover::ProjectFilter);
        let popover = is_open.then(|| self.render_project_filter_popover(theme, cx));
        let trigger_border = if is_open {
            theme.brand.primary
        } else {
            theme.border.default
        };
        let container = if is_full_width {
            div().w_full()
        } else {
            div().w(spacing(64.0)).flex_none()
        };
        container
            .relative()
            .child(
                div()
                    .id("board-project-filter")
                    .relative()
                    .w_full()
                    .px(spacing(2.0))
                    .py(spacing(1.0))
                    .min_h(px(34.0))
                    .bg(theme.background.page)
                    .border_1()
                    .border_color(trigger_border)
                    .rounded(radius::MD)
                    .text_color(theme.text.primary)
                    .flex()
                    .items_center()
                    .gap(spacing(1.0))
                    .overflow_hidden()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_popover(HeaderPopover::ProjectFilter, window, cx)
                    }))
                    .child(content)
                    .child(chevron(theme))
                    .child(qa_button_layer(&placeholder, &trigger_text)),
            )
            .children(popover)
    }

    fn render_desktop_filter(
        &mut self,
        theme: &ThemeTokens,
        labels: &HeaderLabels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let label = desktop_filter_label(self, labels);
        let aria_label = labels.board.translate("desktopFilter");
        let popover = (self.open_popover == Some(HeaderPopover::DesktopFilter))
            .then(|| self.render_desktop_filter_popover(theme, labels, cx));
        div()
            .relative()
            .w(spacing(48.0))
            .flex_none()
            .child(
                div()
                    .id("board-desktop-filter")
                    .relative()
                    .flex()
                    .h(px(34.0))
                    .w_full()
                    .items_center()
                    .gap(spacing(2.0))
                    .rounded(radius::MD)
                    .border_1()
                    .border_color(theme.border.default)
                    .bg(theme.background.page)
                    .px(spacing(3.0))
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.primary)
                    .cursor_pointer()
                    .hover({
                        let hover_border = theme.border.strong;
                        move |style| style.border_color(hover_border)
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_popover(HeaderPopover::DesktopFilter, window, cx)
                    }))
                    .child(
                        svg()
                            .path(icon_paths::MONITOR)
                            .size(px(16.0))
                            .flex_none()
                            .text_color(theme.text.muted),
                    )
                    .child(div().min_w_0().truncate().child(label.clone()))
                    .child(chevron(theme))
                    .child(qa_button_layer(&aria_label, &label)),
            )
            .children(popover)
    }

    pub(super) fn render_sort_button(
        &mut self,
        theme: &ThemeTokens,
        labels: &HeaderLabels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let label = labels.sort.translate("label");
        let active_count = self
            .board
            .as_ref()
            .map(|board| board.sort_preference.keys.len())
            .unwrap_or(0);
        let is_active = active_count > 0;
        let base = div()
            .id("board-sort")
            .relative()
            .flex()
            .flex_none()
            .h(px(34.0))
            .items_center()
            .gap(px(6.0))
            .rounded(radius::MD)
            .border_1()
            .px(px(10.0))
            .text_scale(text_scale::SM)
            .css_weight(500)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_popover(HeaderPopover::Sort, window, cx)
            }));
        let styled = if is_active {
            base.border_color(theme.border.brand)
                .bg(theme.brand.subtle)
                .text_color(theme.text.primary)
        } else {
            let hover_border = theme.brand.primary;
            let hover_text = theme.text.primary;
            base.border_color(theme.border.default)
                .bg(theme.background.surface)
                .text_color(theme.text.secondary)
                .hover(move |style| style.border_color(hover_border).text_color(hover_text))
        };
        let icon_color = if is_active {
            theme.text.primary
        } else {
            theme.text.secondary
        };
        let count_badge = is_active.then(|| {
            div()
                .rounded(radius::FULL)
                .bg(theme.brand.primary)
                .px(px(6.0))
                .text_scale(text_scale::PX_11)
                .css_weight(600)
                .text_color(theme.text.inverse)
                .relative()
                .child(active_count.to_string())
                .child(qa_text_layer("status", &active_count.to_string()))
        });
        let popover = (self.open_popover == Some(HeaderPopover::Sort))
            .then(|| self.render_sort_popover(theme, labels, cx));
        div()
            .relative()
            .flex_none()
            .child(
                styled
                    .child(
                        svg()
                            .path(icon_paths::SORT_LINES)
                            .size(px(18.0))
                            .flex_none()
                            .text_color(icon_color),
                    )
                    .child(label.clone())
                    .children(count_badge)
                    .child(qa_button_layer(&label, &label)),
            )
            .children(popover)
    }

    fn render_scan_button(
        &mut self,
        theme: &ThemeTokens,
        labels: &HeaderLabels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let hover_border = theme.brand.primary;
        icon_button(
            "board-scan",
            icon_paths::FOLDER_PLUS,
            labels.settings.translate("scanTitle"),
            theme.text.secondary,
        )
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.surface)
        .hover(move |style| style.border_color(hover_border))
        .on_click(cx.listener(|this, _, window, cx| this.open_project_registry(window, cx)))
    }

    fn render_new_task_button(
        &mut self,
        theme: &ThemeTokens,
        labels: &HeaderLabels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let label = labels.board.translate("newTask");
        let hover = theme.brand.hover;
        div()
            .id("board-new-task")
            .relative()
            .flex_none()
            .px(spacing(4.0))
            .py(px(6.0))
            .text_scale(text_scale::SM)
            .css_weight(500)
            .bg(theme.brand.primary)
            .hover(move |style| style.bg(hover))
            .text_color(theme.text.inverse)
            .rounded(radius::MD)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, window, cx| this.open_create_task(None, window, cx)))
            .child(label.clone())
            .child(qa_button_layer(&label, &label))
    }

    fn render_settings_button(
        &mut self,
        theme: &ThemeTokens,
        labels: &HeaderLabels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let hover_border = theme.border.default;
        let hover_background = theme.background.page;
        icon_button(
            "board-settings",
            icon_paths::SETTINGS,
            labels.common.translate("settings"),
            theme.text.muted,
        )
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .hover(move |style| style.border_color(hover_border).bg(hover_background))
        .on_click(cx.listener(|this, _, _, cx| {
            let navigator = this.screen.navigator.clone();
            navigator.navigate("/settings", cx);
        }))
    }

    fn render_notification_button(&self) -> impl IntoElement + use<> {
        self.notification_center.clone()
    }
}

/// 렌더러가 macOS로 판정한 데스크톱 앱인지(플랫폼은 앱 공용 판정을 따른다)
pub(super) fn is_mac_titlebar_layout(cx: &App) -> bool {
    cx.global::<crate::services::AppServices>()
        .shortcut_platform
        == kanvibe_domain::platform::ShortcutPlatform::Mac
}

fn chevron(theme: &ThemeTokens) -> impl IntoElement {
    svg()
        .path(icon_paths::CHEVRON_DOWN_SMALL)
        .size(px(12.0))
        .ml_auto()
        .flex_none()
        .text_color(theme.text.muted)
}

/// `p-1.5 rounded-md` 아이콘 버튼(20px SVG)
fn icon_button(
    id: &'static str,
    icon_path: &'static str,
    label: String,
    icon_color: Rgba,
) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .flex_none()
        .rounded(radius::MD)
        .p(px(6.0))
        .cursor_pointer()
        .child(svg().path(icon_path).size(px(20.0)).text_color(icon_color))
        .child(qa_button_layer(&label, ""))
}

/// 창 폭에서 스크롤바 홈을 뺀 폭(로고 표시 판정에 쓴다)
pub(super) fn content_width(window: &Window) -> f32 {
    f32::from(window.viewport_size().width)
        - f32::from(crate::components::scroll_area::PAGE_SCROLLBAR_WIDTH)
}
