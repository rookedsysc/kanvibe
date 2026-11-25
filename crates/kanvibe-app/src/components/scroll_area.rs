//! 페이지·패널이 창보다 길 때 Chromium(Linux)이 그리는 기본 스크롤바를 옮겼다.
//! `* { scrollbar-color: var(--color-border-strong) transparent }` 규칙이라 트랙은 투명하고 thumb·화살표만 border-strong 색이다.
//! 폭 15px 홈을 항상 차지하므로 내용 폭이 그만큼 줄어든다.
//!
//! `page_scrollbar`는 문서 높이를 아는 화면(보드)이 직접 그리고, `scroll_area`는 `ScrollHandle`이 잰 높이로 그린다.

use gpui_kit::{
    AnyElement, App, Bounds, ElementId, Hsla, InteractiveElement, IntoElement, ParentElement,
    Pixels, ScrollHandle, StatefulInteractiveElement, Styled, Window, canvas, div, fill, point, px,
    size,
};

/// 스크롤바 홈 폭. 이 폭만큼 페이지 내용 폭이 줄어든다
pub const PAGE_SCROLLBAR_WIDTH: Pixels = px(15.0);
/// 위·아래 화살표 버튼 높이
const ARROW_BUTTON_LENGTH: f32 = 15.0;
/// thumb은 홈 안쪽으로 좌우 2px씩 들어간다
const THUMB_INSET: f32 = 2.0;
/// 화살표 삼각형 줄 폭(위에서 아래로). 아래 화살표 기준이고 위 화살표는 뒤집는다
const ARROW_ROW_WIDTHS: [f32; 4] = [7.0, 5.0, 3.0, 1.0];
/// 아래 화살표가 버튼 위쪽에서 떨어진 거리
const DOWN_ARROW_OFFSET: f32 = 5.0;

/// thumb 위치. 트랙(화살표 버튼 사이) 안에서 보이는 비율만큼 길고 스크롤 비율만큼 내려간다.
pub fn thumb_span(viewport_height: f32, content_height: f32, scroll_top: f32) -> (f32, f32) {
    let track_length = viewport_height - ARROW_BUTTON_LENGTH * 2.0;
    let thumb_length = (track_length * viewport_height / content_height).round();
    let max_scroll = (content_height - viewport_height).max(1.0);
    let travel = track_length - thumb_length;
    let thumb_top = ARROW_BUTTON_LENGTH + (travel * scroll_top / max_scroll).round();
    (thumb_top, thumb_length)
}

/// 스크롤바 홈 전체를 채우는 요소. `content_height`는 문서 전체 높이다.
pub fn page_scrollbar(content_height: f32, scroll_top: f32, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window: &mut Window, _: &mut App| {
            paint_page_scrollbar(bounds, content_height, scroll_top, color, window);
        },
    )
    .w(PAGE_SCROLLBAR_WIDTH)
    .h_full()
    .flex_none()
}

/// 세로로 스크롤되는 영역과 그 오른쪽 스크롤바 홈(CSS `overflow-y: scroll`). 높이는 부모가 정한다.
/// 스크롤 위치는 `handle`이 갖고, 스크롤바는 직전 레이아웃에서 잰 내용 높이로 그린다.
pub fn scroll_area(
    id: impl Into<ElementId>,
    handle: &ScrollHandle,
    color: Hsla,
    content: impl IntoElement,
) -> AnyElement {
    let measured = handle.clone();
    div()
        .flex()
        .size_full()
        .min_h_0()
        .child(
            div()
                .id(id)
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .track_scroll(handle)
                .child(content),
        )
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window: &mut Window, _: &mut App| {
                    let viewport_height = f32::from(bounds.size.height);
                    let max_scroll = f32::from(measured.max_offset().y);
                    if max_scroll <= 0.0 {
                        return;
                    }
                    let scroll_top = -f32::from(measured.offset().y);
                    paint_page_scrollbar(
                        bounds,
                        viewport_height + max_scroll,
                        scroll_top,
                        color,
                        window,
                    );
                },
            )
            .w(PAGE_SCROLLBAR_WIDTH)
            .h_full()
            .flex_none(),
        )
        .into_any_element()
}

fn paint_page_scrollbar(
    bounds: Bounds<Pixels>,
    content_height: f32,
    scroll_top: f32,
    color: Hsla,
    window: &mut Window,
) {
    let viewport_height = f32::from(bounds.size.height);
    let (thumb_top, thumb_length) = thumb_span(viewport_height, content_height, scroll_top);
    let thumb_bounds = Bounds::new(
        bounds.origin + point(px(THUMB_INSET), px(thumb_top)),
        size(bounds.size.width - px(THUMB_INSET * 2.0), px(thumb_length)),
    );
    window.paint_quad(fill(thumb_bounds, color));

    let center_x = f32::from(bounds.origin.x) + (f32::from(PAGE_SCROLLBAR_WIDTH) - 1.0) / 2.0;
    let bottom_button_top = f32::from(bounds.origin.y) + viewport_height - ARROW_BUTTON_LENGTH;
    let top_arrow_last_row =
        f32::from(bounds.origin.y) + ARROW_BUTTON_LENGTH - 1.0 - DOWN_ARROW_OFFSET;
    for (row_index, row_width) in ARROW_ROW_WIDTHS.into_iter().enumerate() {
        let row_left = center_x - (row_width - 1.0) / 2.0;
        let down_row_top = bottom_button_top + DOWN_ARROW_OFFSET + row_index as f32;
        let up_row_top = top_arrow_last_row - row_index as f32;
        for row_top in [down_row_top, up_row_top] {
            let row = Bounds::new(
                point(px(row_left), px(row_top)),
                size(px(row_width), px(1.0)),
            );
            window.paint_quad(fill(row, color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_matches_the_chromium_board_baseline_geometry() {
        // 1599x999 창, 문서 높이 1022(보드 스켈레톤)에서 기준 이미지의 thumb은 y=15..962다
        assert_eq!(thumb_span(999.0, 1022.0, 0.0), (15.0, 947.0));
    }
}
