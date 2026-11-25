//! radar-beta 다이어그램(radar/renderer.ts·styles.ts).
//!
//! 그림 영역(기본 600×600)과 사방 여백(기본 50)을 두고, 영역 가운데를 원점으로 눈금(원 또는 다각형), 축 선과
//! 라벨, 곡선, 범례, 제목 순서로 그린다. 크기·여백·곡선 장력은 `config.radar`, 색과 굵기는
//! `themeVariables.radar`·`cScale0..11`로 바꿀 수 있다. 글자는 모두 SVG `<text>`라 HTML 라벨 방식과 상관없이
//! 같은 장면이 나온다.

mod config;
mod parser;

use crate::chart_text::{AnchoredText, BaselineAnchor, TextAnchor, push_anchored_text};
use crate::directive::read_config;
use crate::sanitize::sanitize_text;
use crate::scene::{Color, PathCommand, Point, Scene, StrokeStyle, ellipse, polygon};
use crate::text::FontBook;
use crate::{DiagramError, RenderContext};

use config::{RadarConfig, RadarStyle};
use parser::{Graticule, RadarChart};

/// 범례 상자 크기와 줄 간격(renderer.ts가 고정한 값)
const LEGEND_BOX_SIZE: f64 = 12.0;
const LEGEND_LINE_HEIGHT: f64 = 20.0;
/// `.radarTitle` 글자 크기(themeVariables.fontSize)
const TITLE_FONT_SIZE: f64 = 16.0;

/// 그리는 데 쓰는 설정·색·중심·반지름
struct RadarFrame {
    config: RadarConfig,
    style: RadarStyle,
    center: Point,
    radius: f64,
}

fn with_alpha(color: Color, alpha: f32) -> Color {
    Color { alpha, ..color }
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let mut chart = parser::parse(definition)?;
    let directives = read_config(definition);
    if chart.title.is_empty() {
        chart.title = directives
            .title
            .as_deref()
            .map(sanitize_text)
            .unwrap_or_default();
    }
    let config = RadarConfig::read(&directives);
    let frame = RadarFrame {
        style: RadarStyle::for_theme(context.palette.theme),
        center: Point::new(
            config.margin_left + config.width / 2.0,
            config.margin_top + config.height / 2.0,
        ),
        radius: config.width.min(config.height) / 2.0,
        config,
    };
    let mut scene = Scene {
        width: config.width + config.margin_left + config.margin_right,
        height: config.height + config.margin_top + config.margin_bottom,
        items: Vec::new(),
        is_start_aligned: false,
    };
    draw_graticule(&mut scene, &chart, &frame);
    draw_axes(&mut scene, context.fonts, &chart, &frame);
    draw_curves(&mut scene, &chart, &frame);
    if chart.show_legend {
        draw_legend(&mut scene, context.fonts, &chart, &frame);
    }
    let title = AnchoredText {
        text: &chart.title,
        x: frame.center.x,
        y: frame.center.y - config.height / 2.0 - config.margin_top,
        font_size: TITLE_FONT_SIZE,
        color: frame.style.text,
        anchor: TextAnchor::Middle,
        baseline: BaselineAnchor::Hanging,
        degrees: 0.0,
    };
    push_anchored_text(&mut scene, context.fonts, &title);
    Ok(scene)
}

/// 축 `index`의 각도. 첫 축은 12시 방향이고 시계 방향으로 돈다
fn axis_angle(index: usize, axis_count: usize) -> f64 {
    2.0 * index as f64 * std::f64::consts::PI / axis_count as f64 - std::f64::consts::FRAC_PI_2
}

fn polar(center: Point, radius: f64, angle: f64) -> Point {
    Point::new(
        center.x + radius * angle.cos(),
        center.y + radius * angle.sin(),
    )
}

/// 눈금은 안쪽부터 `ticks`개를 같은 간격으로 겹쳐 그린다
fn draw_graticule(scene: &mut Scene, chart: &RadarChart, frame: &RadarFrame) {
    let style = &frame.style;
    let fill = with_alpha(style.graticule, style.graticule_opacity);
    let stroke = StrokeStyle {
        color: style.graticule,
        width: style.graticule_stroke_width,
        dash: Vec::new(),
    };
    let axis_count = chart.axes.len();
    for tick in 0..chart.ticks {
        let ring_radius = frame.radius * (tick + 1) as f64 / chart.ticks as f64;
        let commands = match chart.graticule {
            Graticule::Circle => ellipse(frame.center, ring_radius, ring_radius),
            Graticule::Polygon => {
                let corners: Vec<Point> = (0..axis_count)
                    .map(|index| polar(frame.center, ring_radius, axis_angle(index, axis_count)))
                    .collect();
                polygon(&corners)
            }
        };
        scene.push_path(commands, Some(fill), Some(stroke.clone()));
    }
}

fn draw_axes(scene: &mut Scene, fonts: &FontBook, chart: &RadarChart, frame: &RadarFrame) {
    let (style, config) = (&frame.style, &frame.config);
    let axis_count = chart.axes.len();
    for (index, axis) in chart.axes.iter().enumerate() {
        let angle = axis_angle(index, axis_count);
        let end = polar(frame.center, frame.radius * config.axis_scale_factor, angle);
        let stroke = StrokeStyle {
            color: style.axis,
            width: style.axis_stroke_width,
            dash: Vec::new(),
        };
        let line = vec![PathCommand::MoveTo(frame.center), PathCommand::LineTo(end)];
        scene.push_path(line, None, Some(stroke));
        let anchor = polar(frame.center, frame.radius * config.axis_label_factor, angle);
        let label = AnchoredText {
            text: &axis.label,
            x: anchor.x,
            y: anchor.y,
            font_size: style.axis_label_font_size,
            color: style.text,
            anchor: TextAnchor::Middle,
            baseline: BaselineAnchor::Middle,
            degrees: 0.0,
        };
        push_anchored_text(scene, fonts, &label);
    }
}

/// 값 개수가 축 개수와 다른 곡선은 건너뛴다. 원 눈금이면 둥근 닫힌 곡선, 다각형 눈금이면 꺾은선이다
fn draw_curves(scene: &mut Scene, chart: &RadarChart, frame: &RadarFrame) {
    let axis_count = chart.axes.len();
    let max = chart.max.unwrap_or_else(|| {
        chart
            .curves
            .iter()
            .flat_map(|curve| curve.entries.iter().copied())
            .fold(f64::NEG_INFINITY, f64::max)
    });
    for (index, curve) in chart.curves.iter().enumerate() {
        if curve.entries.len() != axis_count {
            continue;
        }
        let points: Vec<Point> = curve
            .entries
            .iter()
            .enumerate()
            .map(|(axis, value)| {
                let clipped = value.max(chart.min).min(max);
                let distance = frame.radius * (clipped - chart.min) / (max - chart.min);
                polar(frame.center, distance, axis_angle(axis, axis_count))
            })
            .collect();
        let commands = match chart.graticule {
            Graticule::Circle => closed_round_curve(&points, frame.config.curve_tension),
            Graticule::Polygon => polygon(&points),
        };
        let color = frame.style.curve(index);
        let stroke = StrokeStyle {
            color,
            width: frame.style.curve_stroke_width,
            dash: Vec::new(),
        };
        let fill = with_alpha(color, frame.style.curve_opacity);
        scene.push_path(commands, Some(fill), Some(stroke));
    }
}

/// closedRoundCurve: 이웃 점 차이에 장력을 곱해 제어점을 잡는 닫힌 카디널 스플라인
fn closed_round_curve(points: &[Point], tension: f64) -> Vec<PathCommand> {
    let count = points.len();
    if count == 0 {
        return Vec::new();
    }
    let mut commands = vec![PathCommand::MoveTo(points[0])];
    for index in 0..count {
        let previous = points[(index + count - 1) % count];
        let current = points[index];
        let next = points[(index + 1) % count];
        let after = points[(index + 2) % count];
        let first_control = Point::new(
            current.x + (next.x - previous.x) * tension,
            current.y + (next.y - previous.y) * tension,
        );
        let second_control = Point::new(
            next.x - (after.x - current.x) * tension,
            next.y - (after.y - current.y) * tension,
        );
        commands.push(PathCommand::CubicTo(first_control, second_control, next));
    }
    commands.push(PathCommand::Close);
    commands
}

/// 범례는 오른쪽 위 사분면의 3/4 지점부터 한 줄에 20px씩 내려 쌓는다
fn draw_legend(scene: &mut Scene, fonts: &FontBook, chart: &RadarChart, frame: &RadarFrame) {
    let config = &frame.config;
    let legend_x = frame.center.x + (config.width / 2.0 + config.margin_right) * 3.0 / 4.0;
    let legend_y = frame.center.y - (config.height / 2.0 + config.margin_top) * 3.0 / 4.0;
    for (index, curve) in chart.curves.iter().enumerate() {
        let top = legend_y + index as f64 * LEGEND_LINE_HEIGHT;
        let color = frame.style.curve(index);
        let corners = [
            Point::new(legend_x, top),
            Point::new(legend_x + LEGEND_BOX_SIZE, top),
            Point::new(legend_x + LEGEND_BOX_SIZE, top + LEGEND_BOX_SIZE),
            Point::new(legend_x, top + LEGEND_BOX_SIZE),
        ];
        let stroke = StrokeStyle {
            color,
            width: 1.0,
            dash: Vec::new(),
        };
        let fill = with_alpha(color, frame.style.curve_opacity);
        scene.push_path(polygon(&corners), Some(fill), Some(stroke));
        let label = AnchoredText {
            text: &curve.label,
            x: legend_x + 16.0,
            y: top,
            font_size: frame.style.legend_font_size,
            color: frame.style.text,
            anchor: TextAnchor::Start,
            baseline: BaselineAnchor::Hanging,
            degrees: 0.0,
        };
        push_anchored_text(scene, fonts, &label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LabelMode;
    use crate::scene::SceneItem;
    use crate::text::tests::bundled_fonts;
    use crate::theme::{Palette, Theme};

    #[test]
    fn 두번째_축_끝은_기준_svg_좌표와_같다() {
        let end = polar(Point::new(0.0, 0.0), 300.0, axis_angle(1, 5));
        assert!((end.x - 285.316_954_888_546).abs() < 1e-9);
        assert!((end.y + 92.705_098_312_484_22).abs() < 1e-9);
    }

    #[test]
    fn 범례를_끄면_눈금_축_곡선_제목만_그린다() {
        let fonts = bundled_fonts();
        let palette = Palette::for_theme(Theme::Light);
        let context = RenderContext {
            fonts: &fonts,
            palette: &palette,
            label_mode: LabelMode::Html,
        };
        let scene = render(
            "radar-beta\naxis a, b, c\ncurve x{1, 2, 3}\nticks 2\nshowLegend false",
            &context,
        )
        .unwrap();
        let paths = scene
            .items
            .iter()
            .filter(|item| matches!(item, SceneItem::Path(_)))
            .count();
        assert_eq!(scene.width, 700.0);
        assert_eq!(paths, 2 + 3 + 1);
    }
}
