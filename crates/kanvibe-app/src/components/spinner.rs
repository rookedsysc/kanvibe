//! 진행 중 고리(TS `rounded-full border-2 border-current border-t-transparent motion-safe:animate-spin`).
//! 알림 센터·작업 상세 dock·삭제 버튼이 함께 쓴다. QA 재생은 기준 이미지처럼 움직임을 줄여 멈춘 고리를 그린다.

use gpui_kit::{
    App, Bounds, IntoElement, PathBuilder, Pixels, Rgba, Styled, Window, canvas, point, px,
};

/// 고리를 돌릴지. QA 재생은 움직임을 줄인 기준 이미지(`motion-safe`)처럼 멈춘다
pub fn is_spinner_animated(cx: &App) -> bool {
    !cx.has_global::<crate::qa::QaRuntime>()
}

/// 위쪽 테두리만 투명한 2px 고리. `is_animated`면 1초에 한 바퀴 돈다(`animate-spin`)
pub fn spinner_ring(color: Rgba, is_animated: bool) -> impl IntoElement + use<> {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let turn = if is_animated {
                window.request_animation_frame();
                spinner_turn()
            } else {
                0.0
            };
            paint_spinner_ring(bounds, turn, color, window);
        },
    )
    .size_full()
}

/// 지금 회전 각(라디안, 1초에 한 바퀴)
fn spinner_turn() -> f32 {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (elapsed.as_millis() % 1000) as f32 / 1000.0 * std::f32::consts::TAU
}

/// 2px 테두리 고리 가운데 위쪽 4분의 1(-135°~-45°)을 뺀 호
fn paint_spinner_ring(bounds: Bounds<Pixels>, turn: f32, color: Rgba, window: &mut Window) {
    let center = bounds.center();
    let radius = f32::from(bounds.size.width) / 2.0 - 1.0;
    let at = |angle: f32| {
        let angle = angle + turn;
        point(
            center.x + px(radius * angle.cos()),
            center.y + px(radius * angle.sin()),
        )
    };
    let quarter = std::f32::consts::FRAC_PI_4;
    let mut builder = PathBuilder::stroke(px(2.0));
    builder.move_to(at(-quarter));
    builder.arc_to(
        point(px(radius), px(radius)),
        px(0.0),
        true,
        true,
        at(-3.0 * quarter),
    );
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}
