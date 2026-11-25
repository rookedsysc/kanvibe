//! 드롭다운·셀렉트 팝오버(ProjectSelector·PrioritySelector·BoardSortPicker류).
//! TS 패널 `z-50 mt-1 w-full bg-bg-surface border border-border-default rounded-md shadow-md`,
//! 항목 `flex items-center gap-2 px-3 py-2 text-sm`, 강조 `bg-brand-primary/10`, 나머지 `hover:bg-bg-page`,
//! 체크 표시 `w-4 h-4 rounded border`(선택 `bg-brand-primary border-brand-primary text-text-inverse`).
//!
//! 패널은 `deferred(anchored())`로 그려 부모의 잘림·쌓임 순서와 상관없이 다른 요소 위에 뜬다.
//! 열림·강조 순번·바깥 클릭 닫기는 화면이 갖는다(`on_mouse_down_out`).

use gpui_kit::{
    AnyElement, Deferred, Div, ElementId, InteractiveElement, IntoElement, ParentElement,
    SharedString, Stateful, Styled, anchored, deferred, div, px,
};

use crate::components::{TailwindText, box_shadows, fade};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 팝오버·메뉴가 본문 위에 쌓이는 순서. 모달(`dialog::MODAL_LAYER_PRIORITY`)보다 아래다
pub const POPOVER_LAYER_PRIORITY: usize = 1;
/// 트리거와 패널 사이 간격(`mt-1`)
const PANEL_OFFSET: f32 = 4.0;
/// 체크 표시 한 변(`w-4 h-4`)
const CHECK_SIZE: f32 = 16.0;

/// 트리거 바로 아래에 뜨는 패널 껍데기. 트리거를 `relative()` 부모로 두고 그 자식으로 넣는다
pub fn popover_panel(theme: &ThemeTokens, content: impl IntoElement) -> Deferred {
    deferred(
        anchored().snap_to_window().child(
            div()
                .mt(px(PANEL_OFFSET))
                .overflow_hidden()
                .rounded(radius::MD)
                .border_1()
                .border_color(theme.border.default)
                .bg(theme.background.surface)
                .shadow(box_shadows(&theme.shadow.md))
                .child(content),
        ),
    )
    .with_priority(POPOVER_LAYER_PRIORITY)
}

/// 목록 한 줄. `is_checked`가 `Some`이면 다중 선택 체크 표시를 앞에 둔다
pub fn popover_option(
    id: impl Into<ElementId>,
    theme: &ThemeTokens,
    label: impl Into<SharedString>,
    is_highlighted: bool,
    is_checked: Option<bool>,
) -> Stateful<Div> {
    let label = label.into();
    let hover_background = theme.background.page;
    let row = div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .gap(spacing(2.0))
        .px(spacing(3.0))
        .py(spacing(2.0))
        .text_scale(text_scale::SM)
        .text_color(theme.text.primary)
        .cursor_pointer()
        .children(is_checked.map(|is_checked| check_mark(theme, is_checked)))
        .child(div().min_w_0().flex_1().truncate().child(label.clone()))
        .child(qa_target_layer(QaTargetDescription {
            role: "option",
            name: label.clone(),
            text: label,
        }));
    if is_highlighted {
        row.bg(fade(theme.brand.primary, 0.1))
    } else {
        row.hover(move |style| style.bg(hover_background))
    }
}

fn check_mark(theme: &ThemeTokens, is_checked: bool) -> AnyElement {
    let mark = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(CHECK_SIZE))
        .rounded(radius::BASE)
        .border_1();
    if is_checked {
        mark.border_color(theme.brand.primary)
            .bg(theme.brand.primary)
            .text_color(theme.text.inverse)
            .text_scale(text_scale::PX_10)
            .child("✓")
            .into_any_element()
    } else {
        mark.border_color(theme.border.default).into_any_element()
    }
}
