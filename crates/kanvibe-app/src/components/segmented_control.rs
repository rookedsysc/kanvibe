//! 둘 이상 중 하나를 고르는 버튼 묶음(보드 헤더 작업 종류 필터).
//! TS: 바깥 `flex h-[34px] rounded-md border border-border-default bg-bg-page p-0.5`,
//! 항목 `flex flex-1 items-center justify-center rounded text-sm font-medium`,
//! 선택 `bg-brand-primary text-text-inverse shadow-sm`, 나머지 `text-text-secondary hover:bg-bg-surface hover:text-text-primary`.

use std::rc::Rc;

use gpui_kit::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::components::{TailwindText, box_shadows, focus_ring};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, text_scale};

/// 바깥 높이(`h-[34px]`)
const CONTROL_HEIGHT: f32 = 34.0;
/// 바깥 안쪽 여백(`p-0.5`)
const CONTROL_PADDING: f32 = 2.0;

type SelectHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct SegmentedControl {
    id: ElementId,
    labels: Vec<SharedString>,
    selected_index: usize,
    width: Option<Pixels>,
    on_select: Option<SelectHandler>,
}

impl SegmentedControl {
    pub fn new(
        id: impl Into<ElementId>,
        labels: impl IntoIterator<Item = impl Into<SharedString>>,
        selected_index: usize,
    ) -> Self {
        Self {
            id: id.into(),
            labels: labels.into_iter().map(Into::into).collect(),
            selected_index,
            width: None,
            on_select: None,
        }
    }

    /// 고정 폭(보드 헤더는 `w-[180px]`). 없으면 내용 폭이다
    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    /// 누른 항목의 순번을 받는다
    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for SegmentedControl {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let segment_id = self.id.clone();
        let segments = self.labels.into_iter().enumerate().map(|(index, label)| {
            let is_selected = index == self.selected_index;
            let ring = focus_ring(&theme);
            let segment = div()
                .id(ElementId::NamedChild(
                    std::sync::Arc::new(segment_id.clone()),
                    index.to_string().into(),
                ))
                .relative()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .rounded(radius::BASE)
                .text_scale(text_scale::SM)
                .css_weight(500)
                .tab_index(0)
                .focus_visible(move |style| style.shadow(ring))
                .child(label.clone())
                .child(qa_target_layer(QaTargetDescription {
                    role: "button",
                    name: label.clone(),
                    text: label,
                }));
            let segment = if is_selected {
                segment
                    .bg(theme.brand.primary)
                    .text_color(theme.text.inverse)
                    .shadow(box_shadows(&theme.shadow.sm))
            } else {
                let hover_background = theme.background.surface;
                let hover_text = theme.text.primary;
                segment
                    .text_color(theme.text.secondary)
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover_background).text_color(hover_text))
            };
            match self.on_select.clone() {
                Some(handler) => segment.on_click(move |_, window, cx| handler(index, window, cx)),
                None => segment,
            }
        });
        let control = div()
            .flex()
            .flex_none()
            .h(px(CONTROL_HEIGHT))
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.page)
            .p(px(CONTROL_PADDING))
            .children(segments);
        match self.width {
            Some(width) => control.w(width),
            None => control,
        }
    }
}
