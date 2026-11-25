//! 설정 계열 네 화면이 함께 쓰는 작은 UI 조각. TS에서는 화면마다 같은 Tailwind 조합이 반복돼 있었다.
//! 공용 `components/`에 없는 모양(스위치, 브라우저 기본 체크박스, 인라인 조회 오류, 이동 링크 행)만 둔다.

use std::rc::Rc;

use gpui_kit::{
    AnyElement, App, ClickEvent, Div, ElementId, Hsla, InteractiveElement, IntoElement,
    ParentElement, Pixels, Rgba, ScrollHandle, SharedString, Stateful, StatefulInteractiveElement,
    Styled, Window, canvas, div, px,
};

use crate::components::scroll_area::scroll_area;
use crate::components::{DISABLED_OPACITY, TailwindText, box_shadows, fade, focus_ring};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{TextScale, ThemeMode, ThemeTokens, radius, spacing, text_scale};

/// 블록 안 인라인 `<span class="text-sm">`. 글자는 14px지만 줄 상자는 부모(body) 줄 높이 21px를 따른다
pub const INLINE_TEXT_SM: TextScale = TextScale {
    font_size: px(14.0),
    line_height: px(21.0),
};

/// `setting_label`의 높이(제목 21 + 2 + 설명 16)
pub const SETTING_LABEL_HEIGHT: f32 = 39.0;
/// `toggle_switch` 높이(`h-5`)
pub const SWITCH_HEIGHT: f32 = 20.0;

/// `items-center`가 만드는 위쪽 여백. Chromium은 .5를 위로 반올림하지만 GPUI 레이아웃은 0 쪽으로 내리므로 미리 올림한다
pub fn chromium_center_offset(container_height: f32, child_height: f32) -> Pixels {
    px(((container_height - child_height) / 2.0).ceil().max(0.0))
}

pub type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// 재생 드라이버가 찾을 수 있게 이름을 단 누를 수 있는 요소의 바탕. 모양은 호출부가 더한다
pub fn pressable(
    id: impl Into<ElementId>,
    role: &'static str,
    name: impl Into<SharedString>,
    text: impl Into<SharedString>,
) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .child(qa_target_layer(QaTargetDescription {
            role,
            name: name.into(),
            text: text.into(),
        }))
}

/// 문자열 두 개로 만든 요소 id(목록 항목마다 다른 id)
pub fn keyed_id(prefix: &str, key: &str) -> ElementId {
    ElementId::Name(format!("{prefix}-{key}").into())
}

/// 비활성이면 흐리게 두고 누르지 못하게, 활성이면 Tab 포커스·포커스 링·클릭을 건다
pub fn make_pressable(
    element: Stateful<Div>,
    theme: &ThemeTokens,
    is_disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    if is_disabled {
        return element.opacity(DISABLED_OPACITY);
    }
    let ring = focus_ring(theme);
    element
        .tab_index(0)
        .cursor_pointer()
        .focus_visible(move |style| style.shadow(ring))
        .on_click(on_click)
}

/// 화면 글자를 `wait_for` 대상으로 남긴다(보이는 글자와 같은 이름)
pub fn visible_text(text: impl Into<SharedString>) -> Div {
    let text = text.into();
    div()
        .relative()
        .child(text.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "text",
            name: text.clone(),
            text,
        }))
}

/// `text-xs text-text-muted uppercase tracking-wide mb-3` 섹션 제목
pub fn section_heading(theme: &ThemeTokens, text: impl Into<SharedString>) -> Div {
    div()
        .mb(spacing(3.0))
        .text_scale(text_scale::XS)
        .text_color(theme.text.muted)
        .child(text.into())
}

/// 설정 행 왼쪽의 제목(`text-sm text-text-primary`)과 설명(`text-xs text-text-muted mt-0.5`)
pub fn setting_label(theme: &ThemeTokens, title: String, description: String) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .child(
            visible_text(title)
                .text_scale(INLINE_TEXT_SM)
                .text_color(theme.text.primary),
        )
        .child(
            div()
                .mt(px(2.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(description),
        )
}

/// `relative inline-flex h-5 w-9 rounded-full border-2 border-transparent`, 켜지면 brand, 꺼지면 border-default.
/// 손잡이는 `h-4 w-4 rounded-full bg-white shadow`이고 켜지면 16px 오른쪽으로 간다
pub fn toggle_switch(
    theme: &ThemeTokens,
    id: impl Into<ElementId>,
    accessible_name: impl Into<SharedString>,
    is_on: bool,
    is_disabled: bool,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let track = if is_on {
        theme.brand.primary
    } else {
        theme.border.default
    };
    let knob = div()
        .size(px(16.0))
        .rounded(radius::FULL)
        .bg(gpui_kit::white())
        .shadow(box_shadows(&knob_shadow()))
        .ml(px(if is_on { 16.0 } else { 0.0 }));
    let element = pressable(id, "switch", accessible_name, "")
        .flex()
        .flex_none()
        .w(px(36.0))
        .h(px(20.0))
        .p(px(2.0))
        .rounded(radius::FULL)
        .bg(track)
        .child(knob);
    make_pressable(element, theme, is_disabled, on_toggle)
}

/// Tailwind `shadow`(`0 1px 3px 0 rgb(0 0 0 / .1), 0 1px 2px -1px rgb(0 0 0 / .1)`)의 첫 겹
fn knob_shadow() -> [crate::theme::ShadowLayer; 1] {
    [crate::theme::ShadowLayer {
        offset_y: 1.0,
        blur: 3.0,
        spread: 0.0,
        color: Rgba {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.1,
        },
    }]
}

/// Chromium 기본 체크박스. 다크 문서(`color-scheme: dark`)에서는 회색 바탕에 밝은 테두리,
/// 라이트에서는 흰 바탕에 회색 테두리다. 체크되면 accent(brand) 바탕에 흰 체크를 그린다
pub fn native_checkbox(
    theme: &ThemeTokens,
    id: impl Into<ElementId>,
    accessible_name: impl Into<SharedString>,
    box_size: f32,
    is_checked: bool,
    is_disabled: bool,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (background, border) = checkbox_colors(theme, is_checked);
    let element = pressable(id, "checkbox", accessible_name, "")
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(box_size))
        .rounded(px(2.0))
        .border_1()
        .border_color(border)
        .bg(background)
        .when_checked(is_checked, box_size);
    make_pressable(element, theme, is_disabled, on_toggle)
}

fn checkbox_colors(theme: &ThemeTokens, is_checked: bool) -> (Rgba, Rgba) {
    if is_checked {
        return (theme.brand.primary, theme.brand.primary);
    }
    match theme.mode {
        ThemeMode::Dark => (gray(59), gray(133)),
        ThemeMode::Light => (gpui_kit::white().into(), gray(118)),
    }
}

fn gray(level: u8) -> Rgba {
    let value = level as f32 / 255.0;
    Rgba {
        r: value,
        g: value,
        b: value,
        a: 1.0,
    }
}

trait CheckMark {
    fn when_checked(self, is_checked: bool, box_size: f32) -> Self;
}

impl CheckMark for Stateful<Div> {
    /// 체크 표시는 흰 ✓ 글자로 그린다
    fn when_checked(self, is_checked: bool, box_size: f32) -> Self {
        if !is_checked {
            return self;
        }
        self.child(
            div()
                .text_size(px(box_size * 0.8))
                .line_height(px(box_size))
                .text_color(gpui_kit::white())
                .child("✓"),
        )
    }
}

/// TS `InlineLoadError`: 실패 사유와 "다시 시도". `rounded-md border border-status-error/20 bg-bg-page px-3 py-2 text-xs text-status-error`
pub fn inline_load_error(
    theme: &ThemeTokens,
    id: impl Into<ElementId>,
    message: String,
    retry_label: String,
    on_retry: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    let retry = pressable(id, "button", retry_label.clone(), retry_label.clone())
        .flex_none()
        .rounded(radius::SM)
        .px(spacing(1.5))
        .py(spacing(0.5))
        .css_weight(500)
        .child(retry_label);
    div()
        .flex()
        .items_start()
        .justify_between()
        .gap(spacing(3.0))
        .rounded(radius::MD)
        .border_1()
        .border_color(fade(theme.status.error, 0.2))
        .bg(theme.background.page)
        .px(spacing(3.0))
        .py(spacing(2.0))
        .text_scale(text_scale::XS)
        .text_color(theme.status.error)
        .child(visible_text(message).min_w_0())
        .child(make_pressable(retry, theme, false, on_retry))
}

/// 다른 설정 화면으로 가는 행. `flex items-center justify-between w-full px-3 py-2 text-sm bg-bg-page
/// border border-border-default rounded-md text-text-primary hover:border-brand-primary`
pub fn navigation_row(
    theme: &ThemeTokens,
    id: impl Into<ElementId>,
    label: String,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let hover_border = theme.brand.primary;
    let row = pressable(id, "link", label.clone(), label.clone())
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .px(spacing(3.0))
        .py(spacing(2.0))
        .text_scale(text_scale::SM)
        .bg(theme.background.page)
        .border_1()
        .border_color(theme.border.default)
        .rounded(radius::MD)
        .text_color(theme.text.primary)
        .hover(move |style| style.border_color(hover_border))
        .child(label)
        .child(div().text_color(theme.text.muted).child("→"));
    make_pressable(row, theme, false, on_click)
}

/// 화면 오른쪽 위 "돌아가기" 글자 링크(`text-sm text-brand-primary hover:underline`)
pub fn text_link(
    theme: &ThemeTokens,
    id: impl Into<ElementId>,
    label: String,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let link = pressable(id, "link", label.clone(), label.clone())
        .text_scale(text_scale::SM)
        .text_color(theme.brand.primary)
        .hover(|style| style.underline())
        .child(label);
    make_pressable(link, theme, false, on_click)
}

/// 스크롤바 색(`scrollbar-color: var(--color-border-strong)`)
pub fn scrollbar_color(theme: &ThemeTokens) -> Hsla {
    theme.border.strong.into()
}

/// `overflow: auto` 문서. 내용이 창보다 길 때만 스크롤바 홈(15px)을 둔다. 넘침 여부는 지난 프레임에서 잰 값이라,
/// 홈 없이 그린 프레임에서 넘침이 보이면 한 번 더 그린다
pub fn overflow_scroll_area(
    id: &'static str,
    handle: &ScrollHandle,
    color: Hsla,
    content: impl IntoElement,
) -> AnyElement {
    if f32::from(handle.max_offset().y) > 0.0 {
        return scroll_area(id, handle, color, content);
    }
    let measured = handle.clone();
    div()
        .size_full()
        .child(
            div()
                .id(id)
                .size_full()
                .overflow_y_scroll()
                .track_scroll(handle)
                .child(content),
        )
        .child(
            div().absolute().size_0().child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window: &mut Window, _: &mut App| {
                        if f32::from(measured.max_offset().y) > 0.0 {
                            window.refresh();
                        }
                    },
                )
                .size_0(),
            ),
        )
        .into_any_element()
}

/// 창 제목(TS `document.title`)을 화면이 처음 그려질 때 한 번 건다
pub fn set_window_title_once(is_title_set: &mut bool, title: &str, window: &mut Window) {
    if !std::mem::replace(is_title_set, true) {
        window.set_window_title(title);
    }
}

/// 가운데 정렬 로딩 문구(TS 라우트 첫 조회의 `Loading...`)
pub fn centered_message(theme: &ThemeTokens, content: AnyElement) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(spacing(3.0))
        .bg(theme.background.page)
        .px(spacing(4.0))
        .text_color(theme.text.muted)
        .child(content)
}
