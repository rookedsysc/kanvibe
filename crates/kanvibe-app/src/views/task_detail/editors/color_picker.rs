//! 색 고르기 판(TS `react-colorful`의 `HexColorPicker`). 위는 채도·명도 판, 아래는 색상 띠다.
//! 판은 `높이 150px` 가운데 아래 24px이 색상 띠이고, 채도 판 아래쪽 12px은 검은 테두리다(react-colorful 기본 CSS).
//! 누르거나 끌면 그 자리의 색으로 바꾼다.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{
    App, Bounds, BoxShadow, Div, Hsla, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement, Pixels, Point, Rgba, Stateful, Styled, Window,
    canvas, div, hsla, linear_color_stop, linear_gradient, point, px,
};

use crate::theme::radius;

/// 채도 판 높이(검은 아래 테두리 12px 포함)와 색상 띠 높이
const SATURATION_HEIGHT: f32 = 126.0;
const SATURATION_BOTTOM_BORDER: f32 = 12.0;
const HUE_HEIGHT: f32 = 24.0;
/// `.react-colorful__pointer` 지름
const POINTER_SIZE: f32 = 28.0;

/// 판이나 띠에서 고른 새 색을 받는 쪽
pub type ColorChangeHandler = Rc<dyn Fn(Hsv, &mut Window, &mut App)>;

/// 색상(0..360)·채도·명도(0..1)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsv {
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
}

/// `#rrggbb`(대소문자 무관, `#` 생략 가능)를 읽는다. 세 자리 표기도 받는다(react-colorful `validHex`)
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.trim().trim_start_matches('#');
    let expanded: String = match digits.len() {
        3 => digits.chars().flat_map(|digit| [digit, digit]).collect(),
        6 => digits.to_string(),
        _ => return None,
    };
    let value = u32::from_str_radix(&expanded, 16).ok()?;
    Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}

pub fn to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

pub fn rgb_to_hsv(rgb: [u8; 3]) -> Hsv {
    let [red, green, blue] = rgb.map(|channel| channel as f32 / 255.0);
    let max = red.max(green).max(blue);
    let delta = max - red.min(green).min(blue);
    let hue = if delta == 0.0 {
        0.0
    } else if max == red {
        60.0 * (((green - blue) / delta).rem_euclid(6.0))
    } else if max == green {
        60.0 * ((blue - red) / delta + 2.0)
    } else {
        60.0 * ((red - green) / delta + 4.0)
    };
    Hsv {
        hue,
        saturation: if max == 0.0 { 0.0 } else { delta / max },
        value: max,
    }
}

pub fn hsv_to_rgb(hsv: Hsv) -> [u8; 3] {
    let chroma = hsv.value * hsv.saturation;
    let sector = (hsv.hue.rem_euclid(360.0)) / 60.0;
    let second = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (red, green, blue) = match sector as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let offset = hsv.value - chroma;
    [red, green, blue].map(|channel| ((channel + offset) * 255.0).round() as u8)
}

fn hue_color(hue: f32) -> Hsla {
    hsla(hue.rem_euclid(360.0) / 360.0, 1.0, 0.5, 1.0)
}

fn rgba(rgb: [u8; 3]) -> Rgba {
    gpui_kit::rgb((rgb[0] as u32) << 16 | (rgb[1] as u32) << 8 | rgb[2] as u32)
}

/// 그린 자리. 누른 위치를 판 안의 비율로 바꿀 때 쓴다
#[derive(Clone, Default)]
pub struct PickerBounds {
    saturation: Rc<Cell<Option<Bounds<Pixels>>>>,
    hue: Rc<Cell<Option<Bounds<Pixels>>>>,
}

fn ratio_within(bounds: Bounds<Pixels>, position: Point<Pixels>) -> (f32, f32) {
    let x = f32::from(position.x - bounds.origin.x) / f32::from(bounds.size.width);
    let y = f32::from(position.y - bounds.origin.y) / f32::from(bounds.size.height);
    (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0))
}

/// 그린 뒤 자리를 남기는 투명 층
fn bounds_recorder(slot: Rc<Cell<Option<Bounds<Pixels>>>>) -> impl IntoElement {
    canvas(move |bounds, _, _| slot.set(Some(bounds)), |_, _, _, _| {})
        .absolute()
        .size_full()
}

/// `.react-colorful__pointer`: 흰 테두리 2px 원과 안을 채운 색, `0 2px 4px rgba(0,0,0,.2)` 그림자
fn pointer(fill: Rgba) -> Div {
    div()
        .absolute()
        .size(px(POINTER_SIZE))
        .rounded_full()
        .border_2()
        .border_color(gpui_kit::rgb(0xffffff))
        .bg(fill)
        .shadow(vec![BoxShadow {
            color: hsla(0.0, 0.0, 0.0, 0.2),
            offset: point(px(0.0), px(2.0)),
            blur_radius: px(4.0),
            spread_radius: px(0.0),
            inset: false,
        }])
}

/// 색 판. `on_change`는 누르거나 끄는 동안 새 색을 받는다
pub fn render_color_picker(
    hsv: Hsv,
    width: f32,
    bounds: &PickerBounds,
    on_change: ColorChangeHandler,
) -> Div {
    div()
        .w(px(width))
        .flex()
        .flex_col()
        .child(render_saturation(hsv, width, bounds, on_change.clone()))
        .child(render_hue(hsv, width, bounds, on_change))
}

/// 누른 자리의 판 안 비율을 색으로 바꾼다
type PointPicker = Rc<dyn Fn(Point<Pixels>) -> Option<Hsv>>;

/// 누르면 그 자리의 색으로, 누른 채 끌면 따라가며 바꾼다
fn pick_on_press_and_drag(
    element: Stateful<Div>,
    pick: PointPicker,
    on_change: ColorChangeHandler,
) -> Stateful<Div> {
    let (on_down, pick_down) = (on_change.clone(), pick.clone());
    element
        .on_mouse_down(
            MouseButton::Left,
            move |event: &MouseDownEvent, window, cx| {
                if let Some(next) = pick_down(event.position) {
                    on_down(next, window, cx);
                }
            },
        )
        .on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
            let is_dragging = event.pressed_button == Some(MouseButton::Left);
            if let Some(next) = pick(event.position).filter(|_| is_dragging) {
                on_change(next, window, cx);
            }
        })
}

/// 고른 색상 바탕에 가로 흰색·세로 검은색 그러데이션(react-colorful 채도 판)
fn saturation_layers(hue: f32) -> [Div; 3] {
    let layer = || div().absolute().size_full().rounded_t(radius::LG);
    [
        layer().bg(hue_color(hue)),
        layer().bg(linear_gradient(
            90.0,
            linear_color_stop(hsla(0.0, 0.0, 1.0, 1.0), 0.0),
            linear_color_stop(hsla(0.0, 0.0, 1.0, 0.0), 1.0),
        )),
        layer().bg(linear_gradient(
            0.0,
            linear_color_stop(hsla(0.0, 0.0, 0.0, 1.0), 0.0),
            linear_color_stop(hsla(0.0, 0.0, 0.0, 0.0), 1.0),
        )),
    ]
}

fn render_saturation(
    hsv: Hsv,
    width: f32,
    bounds: &PickerBounds,
    on_change: ColorChangeHandler,
) -> impl IntoElement {
    let area_height = SATURATION_HEIGHT - SATURATION_BOTTOM_BORDER;
    let slot = bounds.saturation.clone();
    let pick: PointPicker = Rc::new(move |position| {
        let (x, y) = ratio_within(slot.get()?, position);
        Some(Hsv {
            hue: hsv.hue,
            saturation: x,
            value: 1.0 - y,
        })
    });
    let marker_x = hsv.saturation * width - POINTER_SIZE / 2.0;
    let marker_y = (1.0 - hsv.value) * area_height - POINTER_SIZE / 2.0;
    let area = div()
        .id("project-color-saturation")
        .relative()
        .w_full()
        .h(px(area_height))
        .children(saturation_layers(hsv.hue))
        .child(bounds_recorder(bounds.saturation.clone()))
        .child(
            pointer(rgba(hsv_to_rgb(hsv)))
                .left(px(marker_x))
                .top(px(marker_y)),
        );
    div()
        .h(px(SATURATION_HEIGHT))
        .rounded_t(radius::LG)
        .bg(gpui_kit::rgb(0x000000))
        .child(pick_on_press_and_drag(area, pick, on_change))
}

/// `linear-gradient(90deg, red, #ff0 17%, #0f0 33%, #0ff 50%, #00f 67%, #f0f 83%, red)`를 두 색 구간 여섯 개로 그린다
fn hue_segments(width: f32) -> Vec<Div> {
    let stops = [0.0, 0.17, 0.33, 0.5, 0.67, 0.83, 1.0];
    let last_index = stops.len() - 2;
    stops
        .windows(2)
        .enumerate()
        .map(|(index, pair)| {
            let segment = div().h_full();
            let segment = match index {
                0 => segment.rounded_bl(radius::LG),
                _ if index == last_index => segment.rounded_br(radius::LG),
                _ => segment,
            };
            segment
                .w(px((pair[1] - pair[0]) * width))
                .bg(linear_gradient(
                    90.0,
                    linear_color_stop(hue_color(index as f32 * 60.0), 0.0),
                    linear_color_stop(hue_color((index + 1) as f32 * 60.0), 1.0),
                ))
        })
        .collect()
}

fn render_hue(
    hsv: Hsv,
    width: f32,
    bounds: &PickerBounds,
    on_change: ColorChangeHandler,
) -> impl IntoElement {
    let slot = bounds.hue.clone();
    let pick: PointPicker = Rc::new(move |position| {
        let (x, _) = ratio_within(slot.get()?, position);
        Some(Hsv {
            hue: x * 360.0,
            ..hsv
        })
    });
    let bar = div()
        .id("project-color-hue")
        .relative()
        .h(px(HUE_HEIGHT))
        .flex()
        .children(hue_segments(width))
        .child(bounds_recorder(bounds.hue.clone()))
        .child(
            pointer(hue_color(hsv.hue).into())
                .left(px(hsv.hue / 360.0 * width - POINTER_SIZE / 2.0))
                .top(px((HUE_HEIGHT - POINTER_SIZE) / 2.0)),
        );
    pick_on_press_and_drag(bar, pick, on_change)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_through_hsv() {
        let rgb = parse_hex("#7C5CFF").unwrap();
        assert_eq!(to_hex(hsv_to_rgb(rgb_to_hsv(rgb))), "#7c5cff");
        assert_eq!(parse_hex("abc"), Some([0xaa, 0xbb, 0xcc]));
        assert_eq!(parse_hex("#12345"), None);
    }
}
