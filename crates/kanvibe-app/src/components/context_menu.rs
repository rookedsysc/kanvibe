//! 우클릭 메뉴(TaskContextMenu류).
//! TS 패널 `fixed z-[500] min-w-[220px] rounded-lg border border-border-default bg-bg-surface py-1 shadow-lg`,
//! 항목 `w-full text-left px-4 py-2 text-sm text-text-primary hover:bg-bg-page focus:bg-bg-page`,
//! 위험 항목 `text-status-error hover:bg-red-50 focus:bg-red-50`.

use gpui_kit::{
    Deferred, Div, ElementId, InteractiveElement, IntoElement, ParentElement, Pixels, Point,
    SharedString, Stateful, Styled, anchored, deferred, div, px, rgb,
};

use crate::components::popover::POPOVER_LAYER_PRIORITY;
use crate::components::{TailwindText, box_shadows, focus_ring};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// `min-w-[220px]`
const MENU_MIN_WIDTH: f32 = 220.0;
/// Tailwind 기본 팔레트 `red-50`(v4 `oklch(97.1% 0.013 17.38)`를 sRGB로 옮긴 값). 앱 토큰이 아니어서 두 테마 모두 같다
const DESTRUCTIVE_HOVER_BACKGROUND: u32 = 0xfef2f2;

/// 마우스 위치에 메뉴를 띄운다. 창 밖으로 넘치면 창 안으로 밀어 넣는다
pub fn context_menu_at(
    theme: &ThemeTokens,
    position: Point<Pixels>,
    items: impl IntoIterator<Item = impl IntoElement>,
) -> Deferred {
    deferred(
        anchored().position(position).snap_to_window().child(
            div()
                .occlude()
                .min_w(px(MENU_MIN_WIDTH))
                .rounded(radius::LG)
                .border_1()
                .border_color(theme.border.default)
                .bg(theme.background.surface)
                .py(spacing(1.0))
                .shadow(box_shadows(&theme.shadow.lg))
                .children(items),
        ),
    )
    .with_priority(POPOVER_LAYER_PRIORITY)
}

/// 메뉴 항목. 화면이 `on_click`을 더한다
pub fn context_menu_item(
    id: impl Into<ElementId>,
    theme: &ThemeTokens,
    label: impl Into<SharedString>,
    is_destructive: bool,
) -> Stateful<Div> {
    let label = label.into();
    let (text_color, hover_background) = if is_destructive {
        (theme.status.error, rgb(DESTRUCTIVE_HOVER_BACKGROUND))
    } else {
        (theme.text.primary, theme.background.page)
    };
    let ring = focus_ring(theme);
    div()
        .id(id)
        .relative()
        .w_full()
        .px(spacing(4.0))
        .py(spacing(2.0))
        .text_scale(text_scale::SM)
        .text_color(text_color)
        .cursor_pointer()
        .tab_index(0)
        .hover(move |style| style.bg(hover_background))
        .focus_visible(move |style| style.bg(hover_background).shadow(ring))
        .child(label.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "menuitem",
            name: label.clone(),
            text: label,
        }))
}
