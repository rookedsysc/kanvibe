//! venn `text` 글자 상자(vennRenderer.ts renderTextNodes).
//!
//! 글자 상자 자리는 원을 그린 배치와 따로, `venn.layout`(여백 `config.venn.padding` 8)로 다시 구한 영역 라벨
//! 점을 기준으로 잡는다. 영역마다 라벨 점 둘레에서 모든 원 안에 드는 가장 큰 원(없으면 가장 작은 원의 60%)을
//! 정사각에 가까운 격자로 나누고, 칸마다 90% 크기 foreignObject 안에 HTML 글자를 가로·세로 가운데로 둔다.
//! `useDebugLayout`이면 그 안쪽 원(보라 점선)과 격자 칸(청록 점선)도 그린다.

use crate::scene::{Color, Point, Scene, StrokeStyle, TextItem, ellipse, polygon};
use crate::text::{FontBook, FontWeight};

use super::geometry::Circle;
use super::parser::{Subset, TextNode};

/// 그림 크기에 맞춘 글자 상자 배치 값
pub struct TextNodeStyle {
    pub scale: f64,
    pub color: Color,
    pub is_debug_layout: bool,
}

const DEBUG_CIRCLE_COLOR: Color = Color::rgb(128, 0, 128);
const DEBUG_CELL_COLOR: Color = Color::rgb(0, 128, 128);

/// 영역 하나의 글자 상자들이 들어갈 안쪽 정사각형
struct InnerBox {
    /// 라벨 점 둘레에서 모든 원 안에 드는 원의 반지름
    radius: f64,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

/// `nodes`를 영역별로 묶어(처음 나온 순서) 격자에 놓는다. 영역 라벨 점이 없는 묶음은 건너뛴다
pub fn draw_text_nodes(
    scene: &mut Scene,
    fonts: &FontBook,
    nodes: &[TextNode],
    areas: &[(Subset, Vec<Circle>, (f64, f64))],
    style: &TextNodeStyle,
    node_color: impl Fn(&str) -> Option<Color>,
) {
    let groups = group_by_area(nodes);
    for (sets, members) in groups {
        let Some((area, circles, centre)) = areas.iter().find(|(area, _, _)| area.sets == *sets)
        else {
            continue;
        };
        let inner = inner_box(area, circles, *centre, members.len(), style.scale);
        if style.is_debug_layout {
            let center = Point::new(centre.0, centre.1);
            let circle = ellipse(center, inner.radius, inner.radius);
            push_debug_outline(
                scene,
                circle,
                DEBUG_CIRCLE_COLOR,
                1.5,
                [6.0, 4.0],
                style.scale,
            );
        }
        let columns = (members.len() as f64).sqrt().ceil().max(1.0) as usize;
        let rows = members.len().div_ceil(columns).max(1);
        let cell_width = inner.width / columns as f64;
        let cell_height = inner.height / rows as f64;
        for (index, node) in members.iter().enumerate() {
            let center_x = inner.left + cell_width * ((index % columns) as f64 + 0.5);
            let center_y = inner.top + cell_height * ((index / columns) as f64 + 0.5);
            if style.is_debug_layout {
                let cell = (center_x, center_y, cell_width, cell_height);
                push_debug_cell(scene, cell, style.scale);
            }
            let text = node.label.as_deref().unwrap_or(&node.id);
            let color = node_color(&node.id).unwrap_or(style.color);
            let cell = (center_x, center_y, cell_width * 0.9, cell_height * 0.9);
            draw_centered_html_text(scene, fonts, text, cell, 40.0 * style.scale, color);
        }
    }
}

/// `useDebugLayout`일 때 글자 칸(가운데 x·y, 폭, 높이) 테두리를 점선으로 그린다
fn push_debug_cell(scene: &mut Scene, cell: (f64, f64, f64, f64), scale: f64) {
    let (center_x, center_y, width, height) = cell;
    let left = center_x - width / 2.0;
    let top = center_y - height / 2.0;
    let outline = polygon(&[
        Point::new(left, top),
        Point::new(left + width, top),
        Point::new(left + width, top + height),
        Point::new(left, top + height),
    ]);
    push_debug_outline(scene, outline, DEBUG_CELL_COLOR, 1.0, [4.0, 3.0], scale);
}

/// 같은 영역의 글자 상자끼리 처음 나온 순서로 묶는다
fn group_by_area(nodes: &[TextNode]) -> Vec<(&Vec<String>, Vec<&TextNode>)> {
    let mut groups: Vec<(&Vec<String>, Vec<&TextNode>)> = Vec::new();
    for node in nodes {
        match groups.iter_mut().find(|(sets, _)| **sets == node.sets) {
            Some((_, members)) => members.push(node),
            None => groups.push((&node.sets, vec![node])),
        }
    }
    groups
}

/// 라벨 점에서 모든 원 경계까지 남은 거리로 안쪽 상자를 정하고, 영역 라벨과 글자 상자 수에 따라 아래로 민다
fn inner_box(
    area: &Subset,
    circles: &[Circle],
    centre: (f64, f64),
    node_count: usize,
    scale: f64,
) -> InnerBox {
    let min_radius = circles
        .iter()
        .map(|circle| circle.radius)
        .fold(f64::INFINITY, f64::min);
    let raw = circles
        .iter()
        .map(|circle| circle.radius - (centre.0 - circle.x).hypot(centre.1 - circle.y))
        .fold(f64::INFINITY, f64::min);
    let mut inner_radius = if raw.is_finite() { raw.max(0.0) } else { 0.0 };
    if inner_radius == 0.0 && min_radius.is_finite() {
        inner_radius = min_radius * 0.6;
    }
    let width = f64::max(80.0 * scale, inner_radius * 2.0 * 0.95);
    let height = f64::max(60.0 * scale, inner_radius * 2.0 * 0.95);
    let has_label = area.label.as_deref().is_some_and(|label| !label.is_empty());
    let label_offset = if has_label {
        f64::min(32.0 * scale, inner_radius * 0.25)
    } else {
        0.0
    };
    let count_offset = if node_count <= 2 { 30.0 * scale } else { 0.0 };
    InnerBox {
        radius: inner_radius,
        left: centre.0 - width / 2.0,
        top: centre.1 - height / 2.0 + label_offset + count_offset,
        width,
        height,
    }
}

/// `display: flex`로 가운데 맞춘 HTML 글자. 칸 폭을 넘으면 낱말 단위로 줄을 바꾼다(줄 높이 normal)
fn draw_centered_html_text(
    scene: &mut Scene,
    fonts: &FontBook,
    text: &str,
    cell: (f64, f64, f64, f64),
    font_size: f64,
    color: Color,
) {
    let (center_x, center_y, width, _) = cell;
    let lines = wrap_words(fonts, text, font_size, width);
    let metrics = fonts.primary_metrics(font_size);
    let line_height = metrics.ascent + metrics.descent;
    let top = center_y - line_height * lines.len() as f64 / 2.0;
    for (index, (line, line_width)) in lines.into_iter().enumerate() {
        scene.push_text(TextItem {
            text: line,
            x: center_x - line_width / 2.0,
            baseline: top + line_height * index as f64 + metrics.ascent,
            font_size,
            weight: FontWeight::Regular,
            color,
            rotation: None,
        });
    }
}

/// 공백으로 나눈 낱말을 폭 안에서 이어 붙인다. 낱말 하나가 폭보다 길면 그대로 한 줄이다
fn wrap_words(fonts: &FontBook, text: &str, font_size: f64, width: f64) -> Vec<(String, f64)> {
    let measure = |line: &str| fonts.shape_line(line, font_size, FontWeight::Regular).width;
    let mut lines: Vec<(String, f64)> = Vec::new();
    for word in text.split_whitespace() {
        if let Some((line, line_width)) = lines.last_mut() {
            let joined = format!("{line} {word}");
            let joined_width = measure(&joined);
            if joined_width <= width {
                *line = joined;
                *line_width = joined_width;
                continue;
            }
        }
        lines.push((word.to_string(), measure(word)));
    }
    lines
}

/// 배율을 곱한 굵기·점선으로 채우지 않은 윤곽선을 그린다
fn push_debug_outline(
    scene: &mut Scene,
    commands: Vec<crate::scene::PathCommand>,
    color: Color,
    width: f64,
    dash: [f64; 2],
    scale: f64,
) {
    let stroke = StrokeStyle {
        color,
        width: width * scale,
        dash: dash.iter().map(|length| length * scale).collect(),
    };
    scene.push_path(commands, None, Some(stroke));
}
