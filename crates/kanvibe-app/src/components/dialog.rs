//! 모달 대화상자(DoneConfirmDialog·CreateTaskModal류).
//! TS 바탕 `fixed inset-0 z-[500] flex items-center justify-center bg-bg-overlay`,
//! 패널 `w-full max-w-sm|max-w-md bg-bg-surface rounded-xl border border-border-default shadow-lg p-6`,
//! 제목 `text-lg font-semibold text-text-primary mb-2`, 설명 `text-sm text-text-secondary mb-4`, 버튼 줄 `flex justify-end gap-2`.
//!
//! 모달 호스트: `modal_layer`가 `deferred`로 창 전체를 덮으므로 화면은 자기 렌더 트리 어디서든 모달을 그리면 된다.
//! 바탕은 아래 요소의 마우스 입력을 막는다. Esc 닫기와 열 때 포커스 이동은 화면이 소유한 FocusHandle로 한다.

use gpui_kit::{
    Deferred, Div, InteractiveElement, IntoElement, ParentElement, Pixels, SharedString, Size,
    Styled, anchored, deferred, div, point, px,
};

use crate::components::{TailwindText, box_shadows};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 팝오버보다 위
pub const MODAL_LAYER_PRIORITY: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogWidth {
    /// `max-w-sm`(384px)
    Small,
    /// `max-w-md`(448px)
    Medium,
}

impl DialogWidth {
    fn max_width(self) -> f32 {
        match self {
            Self::Small => 384.0,
            Self::Medium => 448.0,
        }
    }
}

/// 창 전체(`viewport`, `window.viewport_size()`)를 덮는 바탕과 그 가운데의 패널.
/// 렌더 트리 어디에 두어도 창 원점에 붙도록 `anchored`로 창 좌표에 놓는다
pub fn modal_layer(
    theme: &ThemeTokens,
    viewport: Size<Pixels>,
    panel: impl IntoElement,
) -> Deferred {
    deferred(
        anchored().position(point(px(0.0), px(0.0))).child(
            div()
                .id("kanvibe-modal-backdrop")
                .occlude()
                .w(viewport.width)
                .h(viewport.height)
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.background.overlay)
                .child(panel),
        ),
    )
    .with_priority(MODAL_LAYER_PRIORITY)
}

pub fn dialog_panel(theme: &ThemeTokens, width: DialogWidth) -> Div {
    div()
        .w_full()
        .max_w(px(width.max_width()))
        .rounded(radius::XL)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.surface)
        .shadow(box_shadows(&theme.shadow.lg))
        .p(spacing(6.0))
}

pub fn dialog_title(theme: &ThemeTokens, title: impl Into<SharedString>) -> Div {
    div()
        .mb(spacing(2.0))
        .text_scale(text_scale::LG)
        .css_weight(600)
        .text_color(theme.text.primary)
        .child(title.into())
}

pub fn dialog_description(theme: &ThemeTokens, description: impl Into<SharedString>) -> Div {
    div()
        .mb(spacing(4.0))
        .text_scale(text_scale::SM)
        .text_color(theme.text.secondary)
        .child(description.into())
}

/// 오른쪽 정렬 버튼 줄
pub fn dialog_actions() -> Div {
    div().flex().justify_end().gap(spacing(2.0))
}
