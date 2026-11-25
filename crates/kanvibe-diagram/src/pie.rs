//! pie 다이어그램(pieRenderer.ts). 450×450 원판, 반지름 185, 조각 글자는 반지름 0.75 위치, 오른쪽 범례.

use crate::DiagramError;
use crate::scene::{Color, PathCommand, Point, Scene, StrokeStyle, TextItem, polygon};
use crate::text::{FontBook, FontWeight};
use crate::theme::{Palette, PiePalette};

const HEIGHT: f64 = 450.0;
const MARGIN: f64 = 40.0;
const LEGEND_RECT_SIZE: f64 = 18.0;
const LEGEND_SPACING: f64 = 4.0;
const TEXT_POSITION: f64 = 0.75;
/// 원판 반지름(450 / 2 - 40)
const RADIUS: f64 = HEIGHT / 2.0 - MARGIN;
const OUTER_STROKE_WIDTH: f64 = 2.0;
const SLICE_OPACITY: f32 = 0.7;
const TITLE_SIZE: f64 = 25.0;
const SECTION_TEXT_SIZE: f64 = 17.0;

struct PieChart {
    title: Option<String>,
    show_data: bool,
    sections: Vec<(String, f64)>,
}

fn parse(definition: &str) -> Result<PieChart, DiagramError> {
    let mut lines = definition
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"));
    let header = lines
        .next()
        .ok_or_else(|| DiagramError::Syntax("빈 정의".to_string()))?;
    let mut rest = header
        .strip_prefix("pie")
        .ok_or_else(|| DiagramError::Syntax("pie 머리줄".to_string()))?
        .trim();
    let mut chart = PieChart {
        title: None,
        show_data: false,
        sections: Vec::new(),
    };
    if let Some(after) = rest.strip_prefix("showData") {
        chart.show_data = true;
        rest = after.trim();
    }
    if let Some(title) = rest.strip_prefix("title") {
        chart.title = Some(title.trim().to_string());
    }
    for line in lines {
        if let Some(title) = line.strip_prefix("title") {
            chart.title = Some(title.trim().to_string());
            continue;
        }
        if line.starts_with("accTitle") || line.starts_with("accDescr") {
            continue;
        }
        chart.sections.push(parse_section(line)?);
    }
    Ok(chart)
}

fn parse_section(line: &str) -> Result<(String, f64), DiagramError> {
    let invalid = || DiagramError::Syntax(format!("pie 항목 형식: {line}"));
    let rest = line.strip_prefix('"').ok_or_else(invalid)?;
    let end = rest.find('"').ok_or_else(invalid)?;
    let label = rest[..end].to_string();
    let value = rest[end + 1..]
        .trim()
        .strip_prefix(':')
        .ok_or_else(invalid)?
        .trim()
        .parse::<f64>()
        .map_err(|_| invalid())?;
    if value < 0.0 {
        return Err(invalid());
    }
    Ok((label, value))
}

/// 원호를 90° 이하 조각의 3차 베지어로 근사한다(각은 12시에서 시계 방향 라디안)
fn arc_commands(radius: f64, start: f64, end: f64) -> Vec<PathCommand> {
    let point = |angle: f64| Point::new(radius * angle.sin(), -radius * angle.cos());
    let segments = ((end - start) / std::f64::consts::FRAC_PI_2)
        .ceil()
        .max(1.0) as usize;
    let step = (end - start) / segments as f64;
    let handle = 4.0 / 3.0 * (step / 4.0).tan() * radius;
    let mut commands = vec![PathCommand::MoveTo(point(start))];
    for index in 0..segments {
        let from = start + step * index as f64;
        let to = from + step;
        let (from_point, to_point) = (point(from), point(to));
        let tangent = |angle: f64| Point::new(angle.cos(), angle.sin());
        let (from_tangent, to_tangent) = (tangent(from), tangent(to));
        commands.push(PathCommand::CubicTo(
            Point::new(
                from_point.x + handle * from_tangent.x,
                from_point.y + handle * from_tangent.y,
            ),
            Point::new(
                to_point.x - handle * to_tangent.x,
                to_point.y - handle * to_tangent.y,
            ),
            to_point,
        ));
    }
    commands
}

fn centered_text(
    fonts: &FontBook,
    text: String,
    center_x: f64,
    baseline: f64,
    font_size: f64,
    color: Color,
) -> TextItem {
    let width = fonts
        .shape_line(&text, font_size, FontWeight::Regular)
        .width;
    TextItem {
        text,
        x: center_x - width / 2.0,
        baseline,
        font_size,
        weight: FontWeight::Regular,
        color,
    }
}

pub fn render(
    definition: &str,
    fonts: &FontBook,
    palette: &Palette,
) -> Result<Scene, DiagramError> {
    let chart = parse(definition)?;
    let colors = &palette.pie;
    let mut scene = Scene {
        width: 0.0,
        height: HEIGHT,
        items: Vec::new(),
    };
    let outer_radius = RADIUS + OUTER_STROKE_WIDTH / 2.0;
    let outer_stroke = StrokeStyle {
        color: colors.outer_stroke,
        width: OUTER_STROKE_WIDTH,
        dash: Vec::new(),
    };
    scene.push_path(
        crate::scene::ellipse(Point::new(0.0, 0.0), outer_radius, outer_radius),
        None,
        Some(outer_stroke),
    );
    let color_of = |index: usize| colors.slices[index % colors.slices.len()];
    draw_slices(&mut scene, fonts, &chart, colors, &color_of);
    let title_width = draw_title(&mut scene, fonts, &chart, colors.title);
    let legend_width = draw_legend(&mut scene, fonts, &chart, colors.legend_text, &color_of);
    let chart_and_legend = HEIGHT + MARGIN + LEGEND_RECT_SIZE + LEGEND_SPACING + legend_width;
    let title_left = HEIGHT / 2.0 - title_width / 2.0;
    let title_right = HEIGHT / 2.0 + title_width / 2.0;
    let view_x = title_left.min(0.0);
    scene.width = chart_and_legend.max(title_right) - view_x;
    scene.translate(HEIGHT / 2.0 - view_x, HEIGHT / 2.0);
    Ok(scene)
}

/// createPieArcs: 1% 미만 조각은 그리지 않고, 남은 조각끼리 각을 나눈다. 글자는 반올림해 0%면 뺀다
fn draw_slices(
    scene: &mut Scene,
    fonts: &FontBook,
    chart: &PieChart,
    colors: &PiePalette,
    color_of: &dyn Fn(usize) -> Color,
) {
    let total: f64 = chart.sections.iter().map(|(_, value)| value).sum();
    let shown = shown_sections(chart, total);
    let shown_total: f64 = shown.iter().map(|(_, value)| value).sum();
    let stroke = StrokeStyle {
        color: Color {
            alpha: SLICE_OPACITY,
            ..colors.stroke
        },
        width: 2.0,
        dash: Vec::new(),
    };
    let mut angle = 0.0;
    let mut labels = Vec::new();
    for (index, value) in &shown {
        let sweep = value / shown_total * std::f64::consts::TAU;
        let mut commands = arc_commands(RADIUS, angle, angle + sweep);
        commands.extend([
            PathCommand::LineTo(Point::new(0.0, 0.0)),
            PathCommand::Close,
        ]);
        let fill = Color {
            alpha: SLICE_OPACITY,
            ..color_of(*index)
        };
        scene.push_path(commands, Some(fill), Some(stroke.clone()));
        labels.extend(slice_label(value / total * 100.0, angle + sweep / 2.0));
        angle += sweep;
    }
    for (text, x, y) in labels {
        scene.push_text(centered_text(
            fonts,
            text,
            x,
            y,
            SECTION_TEXT_SIZE,
            colors.section_text,
        ));
    }
}

/// 전체의 1% 이상인 조각(번호, 값)
fn shown_sections(chart: &PieChart, total: f64) -> Vec<(usize, f64)> {
    let is_visible = |value: f64| value / total * 100.0 >= 1.0;
    let sections = chart.sections.iter().enumerate();
    sections
        .filter(|(_, (_, value))| is_visible(*value))
        .map(|(index, (_, value))| (index, *value))
        .collect()
}

/// 조각 가운데 각의 반지름 0.75 자리에 놓는 백분율 글자. 반올림해 0이면 없다
fn slice_label(percent: f64, middle_angle: f64) -> Option<(String, f64, f64)> {
    let rounded = format!("{percent:.0}");
    if rounded == "0" {
        return None;
    }
    let label_radius = RADIUS * TEXT_POSITION;
    Some((
        format!("{rounded}%"),
        label_radius * middle_angle.sin(),
        -label_radius * middle_angle.cos(),
    ))
}

fn draw_title(scene: &mut Scene, fonts: &FontBook, chart: &PieChart, color: Color) -> f64 {
    let Some(title) = chart.title.clone().filter(|title| !title.is_empty()) else {
        return 0.0;
    };
    let width = fonts
        .shape_line(&title, TITLE_SIZE, FontWeight::Regular)
        .width;
    scene.push_text(centered_text(
        fonts,
        title,
        0.0,
        -(HEIGHT - 50.0) / 2.0,
        TITLE_SIZE,
        color,
    ));
    width
}

fn draw_legend(
    scene: &mut Scene,
    fonts: &FontBook,
    chart: &PieChart,
    text_color: Color,
    color_of: &dyn Fn(usize) -> Color,
) -> f64 {
    let row_height = LEGEND_RECT_SIZE + LEGEND_SPACING;
    let offset = row_height * chart.sections.len() as f64 / 2.0;
    let mut longest: f64 = 0.0;
    for (index, (label, value)) in chart.sections.iter().enumerate() {
        let (x, y) = (12.0 * LEGEND_RECT_SIZE, index as f64 * row_height - offset);
        let color = color_of(index);
        let corners = [
            Point::new(x, y),
            Point::new(x + LEGEND_RECT_SIZE, y),
            Point::new(x + LEGEND_RECT_SIZE, y + LEGEND_RECT_SIZE),
            Point::new(x, y + LEGEND_RECT_SIZE),
        ];
        scene.push_path(
            polygon(&corners),
            Some(color),
            Some(StrokeStyle {
                color,
                width: 1.0,
                dash: Vec::new(),
            }),
        );
        let text = if chart.show_data {
            format!("{label} [{value}]")
        } else {
            label.clone()
        };
        longest = longest.max(fonts.measure_line(&text, SECTION_TEXT_SIZE, FontWeight::Regular));
        scene.push_text(TextItem {
            text,
            x: x + LEGEND_RECT_SIZE + LEGEND_SPACING,
            baseline: y + LEGEND_RECT_SIZE - LEGEND_SPACING,
            font_size: SECTION_TEXT_SIZE,
            weight: FontWeight::Regular,
            color: text_color,
        });
    }
    longest
}
