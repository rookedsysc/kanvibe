//! quadrantChart 다이어그램(quadrantBuilder.ts·quadrantRenderer.ts).
//!
//! 차트(기본 500×500) 안에 제목, 네 사분면, 축 라벨, 점을 둔다. 글자는 모두 SVG `<text>`라 HTML 라벨 방식과
//! 상관없이 같은 장면이 나온다. 글자는 `dominant-baseline`이 `hanging`(위 끝 기준) 또는 `middle`이다.
//! 크기·여백·글자 크기·축 위치는 `config.quadrantChart`(앞머리·`init` 지시문)로 바꿀 수 있다.

mod config;
mod parse;

use crate::scene::{Color, PathCommand, Point, Rotation, Scene, StrokeStyle, TextItem, polygon};
use crate::text::{FontBook, FontWeight};
use crate::theme::Theme;
use crate::theme_variables::ThemeVariables;
use crate::{DiagramError, RenderContext};

use config::ChartConfig;
use parse::{PointStyle, QuadrantChart};

/// Blink가 `dominant-baseline: hanging`에 쓰는 기준선 높이(ascent의 80%)
const HANGING_ASCENT_RATIO: f64 = 0.8;
/// Noto Sans ascender(1em 기준). 반올림하지 않은 값이 기준 이미지와 더 가깝다
const ASCENT_EM: f64 = 1.069;
/// Noto Sans x-height(1em 기준). `dominant-baseline: middle`은 x-height의 절반만큼 내린다
const X_HEIGHT_RATIO: f64 = 0.536;

/// 사분면 차트 색(quadrantDb `setThemeConfig`가 읽는 `quadrant*` 테마 변수)
struct QuadrantPalette {
    quadrant_fills: [Color; 4],
    quadrant_text_fills: [Color; 4],
    title: Color,
    x_axis_text: Color,
    y_axis_text: Color,
    point_text: Color,
    external_border: Color,
    internal_border: Color,
    /// 기본 테마의 `quadrantPointFill`은 `hsl(…, NaN%)`로 무효라 SVG 루트의 `fill`(`textColor`)을 물려받는다
    point_fill: Color,
}

/// 앱 테마 기본값(기준 SVG에서 읽은 값). 테마 변수가 비었거나 색으로 읽히지 않을 때 쓴다
fn palette_for(theme: Theme) -> QuadrantPalette {
    let (fills, text_fills, border, root_fill) = match theme {
        Theme::Light => (
            [0xECECFF, 0xF1F1FF, 0xF6F6FF, 0xFBFBFF],
            [0x131300, 0x0E0E00, 0x090900, 0x040400],
            Color::rgb(199, 199, 241),
            Color::rgb(0x33, 0x33, 0x33),
        ),
        Theme::Dark => (
            [0x1F2020, 0x242525, 0x292A2A, 0x2E2F2F],
            [0xE0DFDF, 0xDBDADA, 0xD6D5D5, 0xD1D0D0],
            Color::rgb(204, 204, 204),
            Color::rgb(0xCC, 0xCC, 0xCC),
        ),
    };
    let primary_text = hex(text_fills[0]);
    QuadrantPalette {
        quadrant_fills: fills.map(hex),
        quadrant_text_fills: text_fills.map(hex),
        title: primary_text,
        x_axis_text: primary_text,
        y_axis_text: primary_text,
        point_text: primary_text,
        external_border: border,
        internal_border: border,
        point_fill: root_fill,
    }
}

fn hex(value: u32) -> Color {
    Color::rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

/// quadrantDb `getQuadrantData`가 넘기는 테마 변수로 색을 덮는다. 색으로 읽히지 않는 값(기본 테마의 점 색)은
/// SVG 속성이 무시되어 루트 `fill: textColor`를 물려받는다
fn palette_with_variables(theme: Theme, variables: &ThemeVariables) -> QuadrantPalette {
    let mut palette = palette_for(theme);
    for index in 0..4 {
        let number = index + 1;
        variables.read_color(
            &format!("quadrant{number}Fill"),
            &mut palette.quadrant_fills[index],
        );
        variables.read_color(
            &format!("quadrant{number}TextFill"),
            &mut palette.quadrant_text_fills[index],
        );
    }
    variables.read_color("quadrantTitleFill", &mut palette.title);
    variables.read_color("quadrantXAxisTextFill", &mut palette.x_axis_text);
    variables.read_color("quadrantYAxisTextFill", &mut palette.y_axis_text);
    variables.read_color("quadrantPointTextFill", &mut palette.point_text);
    variables.read_color(
        "quadrantExternalBorderStrokeFill",
        &mut palette.external_border,
    );
    variables.read_color(
        "quadrantInternalBorderStrokeFill",
        &mut palette.internal_border,
    );
    variables.read_color("textColor", &mut palette.point_fill);
    variables.read_color("quadrantPointFill", &mut palette.point_fill);
    palette
}

/// 사분면 영역(calculateSpace의 quadrantSpace)
struct QuadrantSpace {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

/// 글자 세로 기준: 위 끝(`hanging`) 또는 가운데(`middle`)
#[derive(Clone, Copy)]
enum VerticalAnchor {
    Top,
    Middle,
}

/// `translate(x, y) rotate(r)`로 놓는 SVG 글자 하나
struct ChartText<'a> {
    text: &'a str,
    x: f64,
    y: f64,
    font_size: f64,
    color: Color,
    vertical: VerticalAnchor,
    is_centered: bool,
    degrees: f64,
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let chart = parse::parse(definition)?;
    let colors = palette_with_variables(context.palette.theme, &context.theme_variables);
    let layout = ChartLayout::new(&chart);
    let config = &chart.config;
    let mut scene = Scene {
        is_start_aligned: false,
        width: config.chart_width,
        height: config.chart_height,
        items: Vec::new(),
    };
    draw_quadrants(&mut scene, context.fonts, &chart, &layout, &colors);
    draw_borders(&mut scene, &layout.space, config, &colors);
    draw_points(&mut scene, context.fonts, &chart, &layout.space, &colors);
    draw_axis_labels(&mut scene, context.fonts, &chart, &layout, &colors);
    if layout.has_title {
        let title = ChartText {
            text: &chart.title,
            x: config.chart_width / 2.0,
            y: config.title_padding,
            font_size: config.title_font_size,
            color: colors.title,
            vertical: VerticalAnchor::Top,
            is_centered: true,
            degrees: 0.0,
        };
        push_chart_text(&mut scene, context.fonts, &title);
    }
    Ok(scene)
}

/// build()가 정하는 표시 여부와 사분면 영역
struct ChartLayout {
    has_title: bool,
    has_x_axis: bool,
    has_y_axis: bool,
    is_x_axis_top: bool,
    title_space: f64,
    space: QuadrantSpace,
}

impl ChartLayout {
    /// calculateSpace: 제목·x축은 위(또는 아래), y축은 왼쪽(또는 오른쪽)에 자리를 뺀다
    fn new(chart: &QuadrantChart) -> Self {
        let config = &chart.config;
        let has_x_axis =
            config.show_x_axis && (!chart.x_axis_left.is_empty() || !chart.x_axis_right.is_empty());
        let has_y_axis =
            config.show_y_axis && (!chart.y_axis_bottom.is_empty() || !chart.y_axis_top.is_empty());
        let has_title = config.show_title && !chart.title.is_empty();
        // 점이 있으면 x축 라벨을 늘 아래로 내린다
        let is_x_axis_top = chart.points.is_empty() && config.is_x_axis_top;
        let x_axis_space = if has_x_axis {
            config.x_axis_label_padding * 2.0 + config.x_axis_label_font_size
        } else {
            0.0
        };
        let y_axis_space = if has_y_axis {
            config.y_axis_label_padding * 2.0 + config.y_axis_label_font_size
        } else {
            0.0
        };
        let title_space = if has_title {
            config.title_font_size + config.title_padding * 2.0
        } else {
            0.0
        };
        let x_axis_top = if is_x_axis_top { x_axis_space } else { 0.0 };
        let y_axis_left = if config.is_y_axis_left {
            y_axis_space
        } else {
            0.0
        };
        let padding = config.quadrant_padding;
        let space = QuadrantSpace {
            left: padding + y_axis_left,
            top: padding + x_axis_top + title_space,
            width: config.chart_width - padding * 2.0 - y_axis_space,
            height: config.chart_height - padding * 2.0 - x_axis_space - title_space,
        };
        Self {
            has_title,
            has_x_axis,
            has_y_axis,
            is_x_axis_top,
            title_space,
            space,
        }
    }
}

fn rect_commands(x: f64, y: f64, width: f64, height: f64) -> Vec<PathCommand> {
    polygon(&[
        Point::new(x, y),
        Point::new(x + width, y),
        Point::new(x + width, y + height),
        Point::new(x, y + height),
    ])
}

/// 사분면 1(오른쪽 위)부터 반시계 방향으로 채우고, 점이 있으면 라벨을 위에, 없으면 가운데에 둔다
fn draw_quadrants(
    scene: &mut Scene,
    fonts: &FontBook,
    chart: &QuadrantChart,
    layout: &ChartLayout,
    colors: &QuadrantPalette,
) {
    let space = &layout.space;
    let (half_width, half_height) = (space.width / 2.0, space.height / 2.0);
    let origins = [
        (space.left + half_width, space.top),
        (space.left, space.top),
        (space.left, space.top + half_height),
        (space.left + half_width, space.top + half_height),
    ];
    let has_points = !chart.points.is_empty();
    for (index, (x, y)) in origins.into_iter().enumerate() {
        let corners = rect_commands(x, y, half_width, half_height);
        scene.push_path(corners, Some(colors.quadrant_fills[index]), None);
        let (text_y, vertical) = if has_points {
            (
                y + chart.config.quadrant_text_top_padding,
                VerticalAnchor::Top,
            )
        } else {
            (y + half_height / 2.0, VerticalAnchor::Middle)
        };
        let text = ChartText {
            text: &chart.quadrant_texts[index],
            x: x + half_width / 2.0,
            y: text_y,
            font_size: chart.config.quadrant_label_font_size,
            color: colors.quadrant_text_fills[index],
            vertical,
            is_centered: true,
            degrees: 0.0,
        };
        push_chart_text(scene, fonts, &text);
    }
}

/// 바깥 테두리 네 줄(기본 굵기 2)과 가운데 십자(기본 굵기 1)
fn draw_borders(
    scene: &mut Scene,
    space: &QuadrantSpace,
    config: &ChartConfig,
    colors: &QuadrantPalette,
) {
    let (external, internal) = (config.external_border_width, config.internal_border_width);
    let half = external / 2.0;
    let (left, top) = (space.left, space.top);
    let (right, bottom) = (left + space.width, top + space.height);
    let (middle_x, middle_y) = (left + space.width / 2.0, top + space.height / 2.0);
    let (outer, inner) = (colors.external_border, colors.internal_border);
    let lines = [
        (left - half, top, right + half, top, external, outer),
        (right, top + half, right, bottom - half, external, outer),
        (left - half, bottom, right + half, bottom, external, outer),
        (left, top + half, left, bottom - half, external, outer),
        (
            middle_x,
            top + half,
            middle_x,
            bottom - half,
            internal,
            inner,
        ),
        (
            left + half,
            middle_y,
            right - half,
            middle_y,
            internal,
            inner,
        ),
    ];
    for (x1, y1, x2, y2, width, color) in lines {
        let commands = vec![
            PathCommand::MoveTo(Point::new(x1, y1)),
            PathCommand::LineTo(Point::new(x2, y2)),
        ];
        let stroke = StrokeStyle {
            color,
            width,
            dash: Vec::new(),
        };
        scene.push_path(commands, None, Some(stroke));
    }
}

/// 점은 정의의 역순(addPoints가 앞에 붙인다)으로 그리고, 라벨은 점 아래 5px에 위 끝을 맞춘다
fn draw_points(
    scene: &mut Scene,
    fonts: &FontBook,
    chart: &QuadrantChart,
    space: &QuadrantSpace,
    colors: &QuadrantPalette,
) {
    for point in chart.points.iter().rev() {
        let style = chart.resolve_style(point);
        let x = space.left + point.x * space.width;
        let y = space.top + space.height - point.y * space.height;
        let radius = style.radius.unwrap_or(chart.config.point_radius);
        draw_point_marker(scene, Point::new(x, y), radius, &style, colors.point_fill);
        let label = ChartText {
            text: &point.text,
            x,
            y: y + chart.config.point_text_padding,
            font_size: chart.config.point_label_font_size,
            color: colors.point_text,
            vertical: VerticalAnchor::Top,
            is_centered: true,
            degrees: 0.0,
        };
        push_chart_text(scene, fonts, &label);
    }
}

/// 점 원. 테두리는 색과 굵기를 모두 줄 때만 그린다(기본 굵기 0px)
fn draw_point_marker(
    scene: &mut Scene,
    center: Point,
    radius: f64,
    style: &PointStyle,
    default_fill: Color,
) {
    let fill = style.color.unwrap_or(default_fill);
    // 기본 테두리 색도 무효한 `hsl(…, NaN%)`라 루트의 stroke(없음)를 물려받는다
    let stroke = style
        .stroke_color
        .zip(style.stroke_width)
        .filter(|(_, width)| *width > 0.0)
        .map(|(color, width)| StrokeStyle {
            color,
            width,
            dash: Vec::new(),
        });
    let circle = crate::scene::ellipse(center, radius, radius);
    scene.push_path(circle, Some(fill), stroke);
}

/// getAxisLabels: 오른쪽(위쪽) 라벨이 있으면 두 라벨을 각 절반의 가운데에, 없으면 왼쪽(아래) 끝에 맞춘다
fn draw_axis_labels(
    scene: &mut Scene,
    fonts: &FontBook,
    chart: &QuadrantChart,
    layout: &ChartLayout,
    colors: &QuadrantPalette,
) {
    if layout.has_x_axis {
        for label in x_axis_labels(chart, layout, colors.x_axis_text) {
            push_chart_text(scene, fonts, &label);
        }
    }
    if layout.has_y_axis {
        for label in y_axis_labels(chart, &layout.space, colors.y_axis_text) {
            push_chart_text(scene, fonts, &label);
        }
    }
}

/// 축 라벨 하나: 위 끝 기준, 가운데 또는 왼쪽 맞춤
struct AxisText<'a> {
    text: &'a str,
    anchor: Point,
    font_size: f64,
    is_centered: bool,
    degrees: f64,
}

fn axis_text(axis: AxisText<'_>, color: Color) -> ChartText<'_> {
    ChartText {
        text: axis.text,
        x: axis.anchor.x,
        y: axis.anchor.y,
        font_size: axis.font_size,
        color,
        vertical: VerticalAnchor::Top,
        is_centered: axis.is_centered,
        degrees: axis.degrees,
    }
}

/// x축 왼쪽·오른쪽 라벨. 위에 두면 제목 아래, 아래에 두면 사분면 아래다
fn x_axis_labels<'a>(
    chart: &'a QuadrantChart,
    layout: &ChartLayout,
    color: Color,
) -> [ChartText<'a>; 2] {
    let (config, space) = (&chart.config, &layout.space);
    let half_width = space.width / 2.0;
    let is_split = !chart.x_axis_right.is_empty();
    let shift = if is_split { half_width / 2.0 } else { 0.0 };
    let padding = config.x_axis_label_padding;
    let y = if layout.is_x_axis_top {
        padding + layout.title_space
    } else {
        padding + space.top + space.height + config.quadrant_padding
    };
    let label = |text, x| AxisText {
        text,
        anchor: Point::new(x, y),
        font_size: config.x_axis_label_font_size,
        is_centered: is_split,
        degrees: 0.0,
    };
    [
        axis_text(label(&chart.x_axis_left, space.left + shift), color),
        axis_text(
            label(&chart.x_axis_right, space.left + half_width + shift),
            color,
        ),
    ]
}

/// y축 아래·위 라벨. 왼쪽(또는 오른쪽) 여백에서 -90° 돌려 아래에서 위로 읽힌다
fn y_axis_labels<'a>(
    chart: &'a QuadrantChart,
    space: &QuadrantSpace,
    color: Color,
) -> [ChartText<'a>; 2] {
    let config = &chart.config;
    let half_height = space.height / 2.0;
    let is_split = !chart.y_axis_top.is_empty();
    let shift = if is_split { half_height / 2.0 } else { 0.0 };
    let padding = config.y_axis_label_padding;
    let x = if config.is_y_axis_left {
        padding
    } else {
        padding + space.left + space.width + config.quadrant_padding
    };
    let label = |text, y| AxisText {
        text,
        anchor: Point::new(x, y),
        font_size: config.y_axis_label_font_size,
        is_centered: is_split,
        degrees: -90.0,
    };
    [
        axis_text(
            label(&chart.y_axis_bottom, space.top + space.height - shift),
            color,
        ),
        axis_text(
            label(&chart.y_axis_top, space.top + half_height - shift),
            color,
        ),
    ]
}

/// `translate(x, y) rotate(r)` 글자를 장면 글자로 옮긴다. 기준선은 앵커에 맞춰 내리고, 돌림 중심은 (x, y)다
fn push_chart_text(scene: &mut Scene, fonts: &FontBook, text: &ChartText) {
    if text.text.is_empty() {
        return;
    }
    let width = fonts
        .shape_line(text.text, text.font_size, FontWeight::Regular)
        .width;
    let baseline_shift = match text.vertical {
        VerticalAnchor::Top => ASCENT_EM * text.font_size * HANGING_ASCENT_RATIO,
        VerticalAnchor::Middle => X_HEIGHT_RATIO * text.font_size / 2.0,
    };
    let left = if text.is_centered {
        text.x - width / 2.0
    } else {
        text.x
    };
    let rotation = (text.degrees != 0.0).then_some(Rotation {
        degrees: text.degrees,
        center: Point::new(text.x, text.y),
    });
    scene.push_text(TextItem {
        text: text.text.to_string(),
        x: left,
        baseline: text.y + baseline_shift,
        font_size: text.font_size,
        weight: FontWeight::Regular,
        color: text.color,
        rotation,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LabelMode;
    use crate::scene::SceneItem;
    use crate::text::tests::bundled_fonts;
    use crate::theme::Palette;

    #[test]
    fn 점이_있으면_x축_라벨을_아래에_두고_사분면을_줄인다() {
        let chart = parse::parse(
            "quadrantChart\n title T\n x-axis Low --> High\n y-axis Bottom --> Top\n A: [0.3, 0.6]",
        )
        .unwrap();
        let layout = ChartLayout::new(&chart);
        assert!(!layout.is_x_axis_top);
        assert_eq!(layout.space.left, 31.0);
        assert_eq!(layout.space.top, 45.0);
        assert_eq!(layout.space.width, 464.0);
        assert_eq!(layout.space.height, 424.0);
    }

    #[test]
    fn y축_라벨은_왼쪽에서_시계_반대로_돌린다() {
        let fonts = bundled_fonts();
        let palette = Palette::for_theme(Theme::Light);
        let context = RenderContext {
            fonts: &fonts,
            palette: &palette,
            label_mode: LabelMode::SvgText,
            theme_variables: Default::default(),
        };
        let scene = render(
            "quadrantChart\n y-axis Low --> High\n quadrant-1 Q1\n P: [0.5, 0.5]",
            &context,
        )
        .unwrap();
        let rotated: Vec<&TextItem> = scene
            .items
            .iter()
            .filter_map(|item| match item {
                SceneItem::Text(text) if text.rotation.is_some() => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(rotated.len(), 2);
        assert_eq!(rotated[0].rotation.unwrap().degrees, -90.0);
        assert_eq!(rotated[0].rotation.unwrap().center.x, 5.0);
    }
}
