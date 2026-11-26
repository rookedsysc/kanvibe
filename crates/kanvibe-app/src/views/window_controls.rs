//! Linux 창 조작 버튼 겹침. TS 앱은 Electron `titleBarStyle: "hidden"` + `titleBarOverlay`(흰 배경 `#ffffff`,
//! 기호 `#111827`, 높이 40)로 오른쪽 위에 최소화·최대화·닫기 버튼을 겹쳐 그렸다. 앱 테마와 상관없이 같은 색이다.

use gpui_kit::{
    App, Bounds, InteractiveElement, IntoElement, ParentElement, PathBuilder, Pixels, Rgba,
    StatefulInteractiveElement, Styled, Window, canvas, div, fill, point, px, rgb, size,
};

pub const WINDOW_CONTROLS_HEIGHT: Pixels = px(40.0);
const BUTTON_WIDTH: f32 = 32.0;
/// 버튼 세 개(최소화·최대화·닫기)를 겹친 폭. 창 맨 위에서 오른쪽 끝까지 닿는 줄이 이만큼 비워 둔다
pub const WINDOW_CONTROLS_WIDTH: Pixels = px(BUTTON_WIDTH * 3.0);
const ICON_SIZE: f32 = 10.0;
/// 기호 굵기. 최소화·최대화 기호는 2px 선이다
const STROKE: f32 = 2.0;
/// 닫기 기호의 대각선 굵기(기준 이미지의 안티에일리어싱 폭에 맞춘 값)
const CLOSE_STROKE: f32 = 2.0;

fn overlay_background() -> Rgba {
    rgb(0xffffff)
}

fn symbol_color() -> Rgba {
    rgb(0x111827)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowControlKind {
    Minimize,
    Maximize,
    Close,
}

/// 창 오른쪽 위에 겹쳐 그리는 버튼 묶음. 부모가 창 전체를 덮는 `relative()` 요소여야 한다.
pub fn window_controls_overlay() -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .right_0()
        .h(WINDOW_CONTROLS_HEIGHT)
        .flex()
        .bg(overlay_background())
        .child(window_control_button(WindowControlKind::Minimize))
        .child(window_control_button(WindowControlKind::Maximize))
        .child(window_control_button(WindowControlKind::Close))
}

fn window_control_button(kind: WindowControlKind) -> impl IntoElement {
    let id = match kind {
        WindowControlKind::Minimize => "window-control-minimize",
        WindowControlKind::Maximize => "window-control-maximize",
        WindowControlKind::Close => "window-control-close",
    };
    div()
        .id(id)
        .w(px(BUTTON_WIDTH))
        .h_full()
        .on_click(move |_, window, _| match kind {
            WindowControlKind::Minimize => window.minimize_window(),
            WindowControlKind::Maximize => window.zoom_window(),
            WindowControlKind::Close => window.remove_window(),
        })
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window: &mut Window, _: &mut App| {
                    paint_window_control_symbol(kind, bounds, window);
                },
            )
            .size_full(),
        )
}

/// 기호는 버튼 가운데 10x10 칸 안에 그린다
fn paint_window_control_symbol(
    kind: WindowControlKind,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) {
    let icon_left = bounds.origin.x + px((BUTTON_WIDTH - ICON_SIZE) / 2.0);
    let icon_top = bounds.origin.y + px((f32::from(WINDOW_CONTROLS_HEIGHT) - ICON_SIZE) / 2.0);
    let icon_origin = point(icon_left, icon_top);
    match kind {
        WindowControlKind::Minimize => {
            let line = Bounds::new(
                icon_origin + point(px(0.0), px(ICON_SIZE - STROKE)),
                size(px(ICON_SIZE), px(STROKE)),
            );
            window.paint_quad(fill(line, symbol_color()));
        }
        WindowControlKind::Maximize => {
            let square = Bounds::new(icon_origin, size(px(ICON_SIZE), px(ICON_SIZE)));
            window.paint_quad(
                gpui_kit::outline(square, symbol_color(), gpui_kit::BorderStyle::Solid)
                    .border_widths(px(STROKE)),
            );
        }
        WindowControlKind::Close => paint_close_symbol(icon_origin, window),
    }
}

fn paint_close_symbol(icon_origin: gpui_kit::Point<Pixels>, window: &mut Window) {
    let inset = px(0.5);
    let far = px(ICON_SIZE - 0.5);
    for (start, end) in [
        (point(inset, inset), point(far, far)),
        (point(far, inset), point(inset, far)),
    ] {
        let mut builder = PathBuilder::stroke(px(CLOSE_STROKE));
        builder.move_to(icon_origin + start);
        builder.line_to(icon_origin + end);
        if let Ok(path) = builder.build() {
            window.paint_path(path, symbol_color());
        }
    }
}
