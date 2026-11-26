//! 대화 본문 글자를 고르고 복사할 수 있게 하는 글자 요소.
//!
//! TS 대화 본문은 브라우저 문서라 어디서든 끌어서 고르고 `Mod+C`로 복사할 수 있었다. GPUI에서는 gpui-base의 창 단위
//! 글자 선택(`TextSelectionLayer`)에 참여하는 요소가 그 자리를 맡는다. gpui-base `SelectableText`는 글꼴을 구간마다
//! 바꿀 수 없어서(인라인 코드는 고정폭) 같은 등록 방식을 쓰되 `TextRun` 목록으로 그리는 요소를 따로 둔다.
//! 링크 구간을 끌지 않고 누르면 그 주소를 앱 링크 규칙(`open_href`)으로 연다.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::base::{TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};
use gpui_kit::{
    App, BorderStyle, Bounds, Corners, CursorStyle, Edges, Element, ElementId, GlobalElementId,
    Hitbox, HitboxBehavior, Hsla, InspectorElementId, LayoutId, MouseButton, MouseDownEvent,
    MouseUpEvent, PaintQuad, Pixels, Point, SharedString, StyledText, Subscription, TextRun,
    Window, transparent_black,
};

/// 누른 위치에서 이만큼 넘게 움직였으면 링크 열기가 아니라 글자 고르기다
const LINK_CLICK_SLOP: f32 = 4.0;

/// 글자와 그 구간별 모양
#[derive(Clone, Default)]
pub struct RichTextSpans {
    pub text: String,
    pub runs: Vec<TextRun>,
    /// 링크 구간(바이트)과 주소
    pub links: Vec<(Range<usize>, String)>,
}

impl RichTextSpans {
    pub fn push(&mut self, text: &str, run: &TextRun) {
        if text.is_empty() {
            return;
        }
        self.text.push_str(text);
        self.runs.push(TextRun {
            len: text.len(),
            ..run.clone()
        });
    }
}

/// 창 글자 선택에 참여하는 여러 모양의 글자
pub struct SelectableRichText {
    id: ElementId,
    text: SharedString,
    styled_text: StyledText,
    links: Vec<(Range<usize>, String)>,
    selection_color: Hsla,
}

impl SelectableRichText {
    pub fn new(id: impl Into<ElementId>, spans: RichTextSpans, selection_color: Hsla) -> Self {
        let text = SharedString::from(spans.text);
        Self {
            id: id.into(),
            styled_text: StyledText::new(text.clone()).with_runs(spans.runs),
            text,
            links: spans.links,
            selection_color,
        }
    }
}

/// 한 프레임에서 다음 프레임으로 넘기는 선택 손잡이
struct OwnedSelection {
    handle: TextSelectionHandle,
    // 끌기는 그 자체로 프레임을 만들지 않으므로 선택이 바뀌면 창을 다시 그린다
    _refresh: Subscription,
}

/// 링크를 누른 시작 위치. 누름과 뗌 처리기가 함께 본다
struct PressedLink(Rc<Cell<Option<Point<Pixels>>>>);

impl Element for SelectableRichText {
    type RequestLayoutState = TextSelectionHandle;
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let text = self.text.clone();
        let handle = window.with_element_state(
            global_id.expect("SelectableRichText에는 고정 id가 있다"),
            |retained: Option<OwnedSelection>, window| {
                let retained = retained.unwrap_or_else(|| {
                    let handle = TextSelectionHandle::new(text, cx);
                    let refresh = handle.refresh_window_on_change(window, cx);
                    OwnedSelection {
                        handle,
                        _refresh: refresh,
                    }
                });
                (retained.handle.clone(), retained)
            },
        );
        let (layout_id, ()) = self
            .styled_text
            .request_layout(global_id, inspector_id, window, cx);
        (layout_id, handle)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        handle: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled_text
            .prepaint(global_id, inspector_id, bounds, &mut (), window, cx);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let registration = TextSelectionRegistration::new(hitbox.clone(), bounds)
            .with_text_bounds(vec![bounds])
            .with_rendered_element(handle, window, cx);
        handle.register(registration, window, cx);
        hitbox
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        handle: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.styled_text.layout().clone();
        let projection = handle.update_runs(
            &[TextSelectionRun::new(
                self.text.clone(),
                layout.clone(),
                bounds,
            )],
            cx,
        );
        for range in projection.ranges().iter().flatten().cloned() {
            paint_selection(&layout, range, self.selection_color, window);
        }
        self.styled_text.paint(
            global_id,
            inspector_id,
            bounds,
            &mut (),
            &mut (),
            window,
            cx,
        );
        if let Some(global_id) = global_id {
            self.paint_links(global_id, hitbox, window);
        }
    }
}

impl SelectableRichText {
    /// 링크 위에서는 손가락 모양을 보이고, 끌지 않고 누른 링크는 연다
    fn paint_links(&self, global_id: &GlobalElementId, hitbox: &Hitbox, window: &mut Window) {
        if self.links.is_empty() {
            return;
        }
        let layout = self.styled_text.layout().clone();
        let links = self.links.clone();
        let link_at = move |position: Point<Pixels>| -> Option<String> {
            let index = layout.index_for_position(position).ok()?;
            links
                .iter()
                .find(|(range, _)| range.contains(&index))
                .map(|(_, href)| href.clone())
        };
        if hitbox.is_hovered(window) && link_at(window.mouse_position()).is_some() {
            window.set_cursor_style(CursorStyle::PointingHand, hitbox);
        }
        let pressed = window.with_element_state(global_id, |state: Option<PressedLink>, _| {
            let state = state.unwrap_or_else(|| PressedLink(Rc::new(Cell::new(None))));
            (state.0.clone(), state)
        });
        let press_hitbox = hitbox.clone();
        let press = pressed.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
            if phase.bubble() && event.button == MouseButton::Left {
                press.set(press_hitbox.is_hovered(window).then_some(event.position));
            }
        });
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if !phase.bubble() || event.button != MouseButton::Left {
                return;
            }
            let Some(start) = pressed.take() else {
                return;
            };
            let distance = (event.position - start).magnitude() as f32;
            if distance <= LINK_CLICK_SLOP
                && let Some(href) = link_at(event.position)
            {
                let window_id = window.window_handle().window_id();
                crate::windows::open_href(&href, Some(window_id), cx);
            }
        });
    }
}

/// 고른 구간 뒤에 배경을 칠한다(gpui-base `SelectableText`와 같은 줄 단위 사각형)
fn paint_selection(
    layout: &gpui_kit::TextLayout,
    range: Range<usize>,
    color: Hsla,
    window: &mut Window,
) {
    let (Some(start), Some(end)) = (
        layout.position_for_index(range.start),
        layout.position_for_index(range.end),
    ) else {
        return;
    };
    let line_height = layout.line_height();
    let bounds = layout.bounds();
    let mut quads = Vec::new();
    if start.y == end.y {
        quads.push(Bounds::from_corners(
            start,
            Point::new(end.x, end.y + line_height),
        ));
    } else {
        quads.push(Bounds::from_corners(
            start,
            Point::new(bounds.right(), start.y + line_height),
        ));
        if end.y > start.y + line_height {
            quads.push(Bounds::from_corners(
                Point::new(bounds.left(), start.y + line_height),
                Point::new(bounds.right(), end.y),
            ));
        }
        quads.push(Bounds::from_corners(
            Point::new(bounds.left(), end.y),
            Point::new(end.x, end.y + line_height),
        ));
    }
    for quad in quads {
        window.paint_quad(PaintQuad {
            bounds: quad,
            background: color.into(),
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
            border_color: transparent_black(),
            border_style: BorderStyle::default(),
        });
    }
}
