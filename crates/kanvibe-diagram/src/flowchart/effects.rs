//! `look: neo`의 CSS 효과: `filter: drop-shadow(1px 2px 2px …)` 그림자와 `stroke: url(#gradient)` 가로 그라디언트
//! 테두리. 둘 다 `getBBox()`에 들어가지 않으므로 경계를 넓히지 않는 조각으로 돌려준다.

use super::geometry::path_bounds;
use crate::scene::{ClipItem, Color, PathCommand, PathItem, Point, SceneItem, StrokeStyle};

/// 그림자를 옮기는 거리(1px, 2px)
const SHADOW_OFFSET: Point = Point::new(1.0, 2.0);
/// Chromium이 blur 2px 그림자를 그린 흐림 표준편차(기준 이미지 단면에서 잰 값)
const SHADOW_DEVIATION: f64 = 1.8;
/// 그림자를 그리는 가장 바깥 거리와 띠 간격
const SHADOW_REACH: f64 = 6.5;
/// 요소 테두리 반폭: 그림자는 테두리까지 넣은 모양에서 시작한다
const OUTLINE_HALF_WIDTH: f64 = 0.5;
/// 가로 그라디언트를 나눠 칠하는 세로 띠 수
const GRADIENT_STRIPS: usize = 32;

/// 모양을 (1, 2) 옮겨 흐린 그림자. 바깥 띠부터 겹쳐 칠해 띠마다 가우스 덮임에 맞추고, 모양 안쪽은 한 번 채운다
pub(crate) fn drop_shadow(commands: &[PathCommand], color: Color) -> Vec<SceneItem> {
    let shifted = shift_commands(commands, SHADOW_OFFSET);
    let spreads: Vec<f64> = (0..=SHADOW_REACH.floor() as usize)
        .map(|step| SHADOW_REACH - step as f64)
        .collect();
    let coverages = spreads
        .iter()
        .map(|spread| 1.0 - normal_cdf((spread - 0.5) / SHADOW_DEVIATION))
        .chain(std::iter::once(1.0 - normal_cdf(-0.5 / SHADOW_DEVIATION)));
    let alphas = layer_alphas(coverages);
    let mut items: Vec<SceneItem> = spreads
        .iter()
        .zip(&alphas)
        .map(|(spread, alpha)| {
            let width = 2.0 * (spread + OUTLINE_HALF_WIDTH);
            let stroke = StrokeStyle {
                color: Color {
                    alpha: *alpha,
                    ..color
                },
                width,
                dash: Vec::new(),
            };
            path_item(shifted.clone(), None, Some(stroke))
        })
        .collect();
    let inner_alpha = alphas.last().copied().unwrap_or(1.0);
    items.push(path_item(
        shifted,
        Some(Color {
            alpha: inner_alpha,
            ..color
        }),
        None,
    ));
    items
}

/// objectBoundingBox 가로 그라디언트(`x1 0% → x2 100%`) 테두리. 모양 상자를 세로 띠로 나눠 띠 가운데 색으로 긋는다
pub(crate) fn gradient_stroke(
    commands: &[PathCommand],
    width: f64,
    (start, end): (Color, Color),
) -> Vec<SceneItem> {
    let Some((min_x, min_y, max_x, max_y)) = path_bounds(commands) else {
        return Vec::new();
    };
    let strip_width = (max_x - min_x) / GRADIENT_STRIPS as f64;
    let reach = width;
    (0..GRADIENT_STRIPS)
        .map(|index| {
            let left = min_x + strip_width * index as f64;
            let is_first = index == 0;
            let is_last = index + 1 == GRADIENT_STRIPS;
            let clip_left = if is_first { left - reach } else { left };
            let clip_right = left + strip_width + if is_last { reach } else { 0.0 };
            let ratio = (index as f64 + 0.5) / GRADIENT_STRIPS as f64;
            let stroke = StrokeStyle {
                color: mix(start, end, ratio),
                width,
                dash: Vec::new(),
            };
            SceneItem::Clip(ClipItem {
                x: clip_left,
                y: min_y - reach,
                width: clip_right - clip_left,
                height: max_y - min_y + 2.0 * reach,
                items: vec![path_item(commands.to_vec(), None, Some(stroke))],
            })
        })
        .collect()
}

fn path_item(
    commands: Vec<PathCommand>,
    fill: Option<Color>,
    stroke: Option<StrokeStyle>,
) -> SceneItem {
    SceneItem::Path(PathItem {
        commands,
        fill,
        stroke,
    })
}

/// sRGB 선형 보간
fn mix(start: Color, end: Color, ratio: f64) -> Color {
    let channel = |from: u8, to: u8| {
        (f64::from(from) + (f64::from(to) - f64::from(from)) * ratio).round() as u8
    };
    Color {
        red: channel(start.red, end.red),
        green: channel(start.green, end.green),
        blue: channel(start.blue, end.blue),
        alpha: start.alpha + (end.alpha - start.alpha) * ratio as f32,
    }
}

fn shift_commands(commands: &[PathCommand], offset: Point) -> Vec<PathCommand> {
    let shift = |point: &Point| Point::new(point.x + offset.x, point.y + offset.y);
    commands
        .iter()
        .map(|command| match command {
            PathCommand::MoveTo(point) => PathCommand::MoveTo(shift(point)),
            PathCommand::LineTo(point) => PathCommand::LineTo(shift(point)),
            PathCommand::QuadTo(control, point) => {
                PathCommand::QuadTo(shift(control), shift(point))
            }
            PathCommand::CubicTo(first, second, point) => {
                PathCommand::CubicTo(shift(first), shift(second), shift(point))
            }
            PathCommand::Close => PathCommand::Close,
        })
        .collect()
}

/// 바깥부터 겹쳐 칠할 때 각 띠가 목표 덮임에 닿도록 하는 층별 불투명도
fn layer_alphas(coverages: impl Iterator<Item = f64>) -> Vec<f32> {
    let mut covered = 0.0;
    coverages
        .map(|target: f64| {
            let target = target.clamp(covered, 0.999);
            let alpha = 1.0 - (1.0 - target) / (1.0 - covered);
            covered = target;
            alpha as f32
        })
        .collect()
}

/// 표준 정규 누적 분포(Abramowitz–Stegun 7.1.26 erf 근사)
fn normal_cdf(value: f64) -> f64 {
    let z = value.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let polynomial = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - polynomial * (-z * z).exp();
    if value >= 0.0 {
        0.5 * (1.0 + erf)
    } else {
        0.5 * (1.0 - erf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::rounded_rect;

    #[test]
    fn 그림자는_옮긴_모양_바깥으로_옅어지는_띠다() {
        let items = drop_shadow(
            &rounded_rect(0.0, 0.0, 10.0, 10.0, 0.0),
            Color::rgb(185, 185, 185),
        );
        let SceneItem::Path(outer) = &items[0] else {
            panic!("경로");
        };
        let SceneItem::Path(inner) = items.last().unwrap() else {
            panic!("경로");
        };
        assert_eq!(outer.commands[0], PathCommand::MoveTo(Point::new(1.0, 2.0)));
        assert!(outer.stroke.as_ref().unwrap().color.alpha < 0.01);
        assert!(inner.fill.unwrap().alpha > outer.stroke.as_ref().unwrap().color.alpha);
    }

    #[test]
    fn 그라디언트_테두리는_왼쪽에서_오른쪽으로_색이_바뀐다() {
        let start = Color::rgb(0xCC, 0xCC, 0xCC);
        let end = Color::rgb(0x2F, 0x2F, 0x2F);
        let strips = gradient_stroke(&rounded_rect(0.0, 0.0, 64.0, 10.0, 0.0), 1.0, (start, end));
        assert_eq!(strips.len(), GRADIENT_STRIPS);
        let color_of = |item: &SceneItem| match item {
            SceneItem::Clip(clip) => match &clip.items[0] {
                SceneItem::Path(path) => path.stroke.as_ref().unwrap().color,
                _ => panic!("경로"),
            },
            _ => panic!("잘라 그리기"),
        };
        assert!(color_of(&strips[0]).red > color_of(&strips[31]).red);
    }
}
