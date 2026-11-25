//! 작업 결과 토스트(ActionStatusToast).
//! TS 묶음 `fixed bottom-4 right-4 z-[600] flex w-[min(24rem,calc(100%-2rem))] flex-col gap-2`,
//! 항목 `flex items-start gap-3 rounded-lg border bg-bg-surface px-3 py-2 text-sm shadow-md`
//! (실패 `border-status-error/30 text-status-error`, 나머지 `border-border-default text-text-primary`),
//! 진행 표시 `mt-0.5 h-3.5 w-3.5 rounded-full border-2 border-current border-t-transparent`,
//! 동작 버튼 `rounded px-1.5 py-0.5 text-xs font-medium text-text-brand hover:underline`,
//! 닫기 `rounded px-1 text-xs text-text-muted hover:text-text-primary`.
//! 항목 목록과 성공 항목 자동 닫기 시간은 토스트를 띄우는 쪽이 갖는다.

use std::f32::consts::PI;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, Bounds, ClickEvent, Deferred, ElementId, Hsla, InteractiveElement, IntoElement,
    ParentElement, PathBuilder, Pixels, RenderOnce, SharedString, Size, StatefulInteractiveElement,
    Styled, Window, anchored, canvas, deferred, div, point, px,
};

use crate::components::{TailwindText, box_shadows, fade};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 모달보다 위(`z-[600]`)
pub const TOAST_LAYER_PRIORITY: usize = 3;
/// 묶음 최대 폭(`24rem`)과 창 가장자리 여백(`bottom-4 right-4`, `100% - 2rem`)
const STACK_MAX_WIDTH: f32 = 384.0;
const STACK_EDGE_INSET: f32 = 16.0;
/// 진행 표시 한 변(`h-3.5 w-3.5`)과 선 굵기(`border-2`)
const SPINNER_SIZE: f32 = 14.0;
const SPINNER_STROKE: f32 = 2.0;
/// 진행 표시 원을 그리는 꺾은선 마디 수
const SPINNER_SEGMENTS: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Progress,
    Success,
    Failure,
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// 창 오른쪽 아래에 토스트를 쌓는다. `viewport`는 `window.viewport_size()`다
pub fn toast_stack(viewport: Size<Pixels>, toasts: impl IntoIterator<Item = Toast>) -> Deferred {
    let stack_width = STACK_MAX_WIDTH.min(f32::from(viewport.width) - STACK_EDGE_INSET * 2.0);
    deferred(
        anchored()
            .position(point(
                viewport.width - px(STACK_EDGE_INSET + stack_width),
                px(0.0),
            ))
            .child(
                div()
                    .w(px(stack_width))
                    .h(viewport.height - px(STACK_EDGE_INSET))
                    .flex()
                    .flex_col()
                    .justify_end()
                    .gap(spacing(2.0))
                    .children(toasts),
            ),
    )
    .with_priority(TOAST_LAYER_PRIORITY)
}

#[derive(IntoElement)]
pub struct Toast {
    id: ElementId,
    kind: ToastKind,
    message: SharedString,
    action: Option<(SharedString, ClickHandler)>,
    on_dismiss: Option<ClickHandler>,
    dismiss_label: SharedString,
}

impl Toast {
    /// `dismiss_label`은 닫기 버튼의 접근성 이름(`common.close`)이다
    pub fn new(
        id: impl Into<ElementId>,
        kind: ToastKind,
        message: impl Into<SharedString>,
        dismiss_label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            message: message.into(),
            action: None,
            on_dismiss: None,
            dismiss_label: dismiss_label.into(),
        }
    }

    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(handler)));
        self
    }

    pub fn on_dismiss(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Toast {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let is_failure = self.kind == ToastKind::Failure;
        let (border_color, text_color) = if is_failure {
            (fade(theme.status.error, 0.3), theme.status.error)
        } else {
            (theme.border.default, theme.text.primary)
        };
        let action_button = self.action.map(|(label, handler)| {
            div()
                .id("toast-action")
                .relative()
                .flex_none()
                .rounded(radius::BASE)
                .px(spacing(1.5))
                .py(spacing(0.5))
                .text_scale(text_scale::XS)
                .css_weight(500)
                .text_color(theme.text.brand)
                .cursor_pointer()
                .hover(|style| style.underline())
                .on_click(move |event, window, cx| handler(event, window, cx))
                .child(label.clone())
                .child(toast_button_target(&label, &label))
        });
        let muted = theme.text.muted;
        let hover_text = theme.text.primary;
        let dismiss_button = div()
            .id("toast-dismiss")
            .relative()
            .flex_none()
            .rounded(radius::BASE)
            .px(spacing(1.0))
            .text_scale(text_scale::XS)
            .text_color(muted)
            .cursor_pointer()
            .hover(move |style| style.text_color(hover_text))
            .when_some(self.on_dismiss, |button, handler| {
                button.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .child("×")
            .child(toast_button_target(&self.dismiss_label, "×"));
        div()
            .id(self.id)
            .occlude()
            .flex()
            .items_start()
            .gap(spacing(3.0))
            .rounded(radius::LG)
            .border_1()
            .border_color(border_color)
            .bg(theme.background.surface)
            .px(spacing(3.0))
            .py(spacing(2.0))
            .text_scale(text_scale::SM)
            .text_color(text_color)
            .shadow(box_shadows(&theme.shadow.md))
            .when(self.kind == ToastKind::Progress, |toast| {
                toast.child(progress_ring(text_color.into()))
            })
            .child(div().min_w_0().flex_1().child(self.message))
            .children(action_button)
            .child(dismiss_button)
    }
}

fn toast_button_target(name: &SharedString, text: &str) -> impl IntoElement {
    qa_target_layer(QaTargetDescription {
        role: "button",
        name: name.clone(),
        text: text.to_string().into(),
    })
}

/// 윗변이 투명한 원 테두리. 기준 이미지는 애니메이션을 끄고 찍으므로 돌지 않는 모양을 그린다
fn progress_ring(color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
            let radius = (SPINNER_SIZE - SPINNER_STROKE) / 2.0;
            let center = bounds.center();
            let mut path = PathBuilder::stroke(px(SPINNER_STROKE));
            // CSS border-top 투명 구간은 위쪽 90도(-135°~-45°)다. 나머지 270도를 꺾은선으로 그린다
            for segment in 0..=SPINNER_SEGMENTS {
                let angle =
                    -PI / 4.0 + (segment as f32 / SPINNER_SEGMENTS as f32) * (3.0 * PI / 2.0);
                let vertex = point(
                    center.x + px(radius * angle.cos()),
                    center.y + px(radius * angle.sin()),
                );
                if segment == 0 {
                    path.move_to(vertex);
                } else {
                    path.line_to(vertex);
                }
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .mt(spacing(0.5))
    .size(px(SPINNER_SIZE))
    .flex_none()
}
