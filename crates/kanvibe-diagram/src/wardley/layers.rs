//! 제목·축·진화 단계, 파이프라인 상자, 연결선, 진화 추세 층.

use crate::scene::{Color, Point, rounded_rect};
use crate::text::FontWeight;

use super::parser::{LinkFlow, MapLink, WardleyMap};
use super::{Anchor, Baseline, Frame, PADDING, Painter, TextStyle};

const DEFAULT_STAGES: [&str; 4] = ["Genesis", "Custom Built", "Product", "Commodity"];
const AXIS_FONT_SIZE: f64 = 12.0;
const LABEL_FONT_SIZE: f64 = 10.0;
const NODE_RADIUS: f64 = 6.0;
/// 파이프라인 부모 네모 한 변(nodeRadius × 1.6)
pub const SQUARE_SIZE: f64 = NODE_RADIUS * 1.6;
/// 연결 화살표(markerWidth 5)와 추세 화살표(markerWidth 6)의 배율
const LINK_MARKER_SCALE: f64 = 0.5;
const TREND_MARKER_SCALE: f64 = 0.6;
/// `M 0 0 L 10 5 L 0 10 z`를 refX 9·refY 5 기준으로 옮긴 점
const END_ARROW: [(f64, f64); 3] = [(-9.0, -5.0), (1.0, 0.0), (-9.0, 5.0)];
/// `M 10 0 L 0 5 L 10 10 z`를 refX 1·refY 5 기준으로 옮긴 점
const START_ARROW: [(f64, f64); 3] = [(9.0, -5.0), (-1.0, 0.0), (9.0, 5.0)];

pub fn draw_title_and_axes(painter: &mut Painter, map: &WardleyMap, frame: Frame) {
    let text_color = painter.colors.axis_text;
    let bold = |font_size: f64, baseline: Baseline| TextStyle {
        font_size,
        weight: FontWeight::Bold,
        anchor: Anchor::Middle,
        baseline,
        color: text_color,
    };
    if let Some(title) = &map.title {
        let at = Point::new(frame.width / 2.0, PADDING / 2.0);
        painter.text(
            title,
            at,
            bold(AXIS_FONT_SIZE * 1.05, Baseline::Middle),
            None,
        );
    }
    let axis = painter.colors.axis;
    let (left, bottom) = (PADDING, frame.height - PADDING);
    let right = frame.width - PADDING;
    painter.line(
        Point::new(left, bottom),
        Point::new(right, bottom),
        axis,
        1.0,
        &[],
    );
    painter.line(
        Point::new(left, PADDING),
        Point::new(left, bottom),
        axis,
        1.0,
        &[],
    );
    let x_label = Point::new(
        PADDING + frame.chart_width() / 2.0,
        frame.height - PADDING / 4.0,
    );
    painter.text(
        "Evolution",
        x_label,
        bold(AXIS_FONT_SIZE, Baseline::Alphabetic),
        None,
    );
    let y_label = Point::new(PADDING / 3.0, PADDING + frame.chart_height() / 2.0);
    let style = bold(AXIS_FONT_SIZE, Baseline::Alphabetic);
    painter.text("Visibility", y_label, style, Some(-90.0));
}

/// 진화 단계: 경계가 단계 수만큼 있으면 그 경계로, 아니면 같은 폭으로 나눈다
pub fn draw_stages(painter: &mut Painter, map: &WardleyMap, frame: Frame) {
    let stages: Vec<&str> = match map.stages.is_empty() {
        true => DEFAULT_STAGES.to_vec(),
        false => map.stages.iter().map(String::as_str).collect(),
    };
    let count = stages.len() as f64;
    let spans: Vec<(f64, f64)> = match map.stage_boundaries.len() == stages.len() {
        true => map
            .stage_boundaries
            .iter()
            .scan(0.0, |previous, boundary| {
                let span = (*previous, *boundary);
                *previous = *boundary;
                Some(span)
            })
            .collect(),
        false => (0..stages.len())
            .map(|index| (index as f64 / count, (index as f64 + 1.0) / count))
            .collect(),
    };
    let style = TextStyle {
        font_size: AXIS_FONT_SIZE - 2.0,
        weight: FontWeight::Regular,
        anchor: Anchor::Middle,
        baseline: Baseline::Alphabetic,
        color: painter.colors.axis_text,
    };
    let divider = Color::rgba(0, 0, 0, 0.8);
    for (index, (stage, (start, end))) in stages.iter().zip(spans).enumerate() {
        let start_x = PADDING + start * frame.chart_width();
        let end_x = PADDING + end * frame.chart_width();
        if index > 0 {
            let (top, bottom) = (
                Point::new(start_x, PADDING),
                Point::new(start_x, frame.height - PADDING),
            );
            painter.line(top, bottom, divider, 1.0, &[5.0, 5.0]);
        }
        let at = Point::new((start_x + end_x) / 2.0, frame.height - PADDING / 1.5);
        painter.text(stage, at, style, None);
    }
}

fn node_index(map: &WardleyMap, id: &str) -> Option<usize> {
    map.nodes.iter().position(|node| node.id == id)
}

/// 파이프라인: 구성 요소 상자를 모두 그린 뒤 진화 순서로 이은 점선을 그린다. 부모는 상자 위 가운데로 옮긴다
pub fn draw_pipelines(painter: &mut Painter, map: &WardleyMap, positions: &mut [Point]) {
    let mut evolution_links = Vec::new();
    for pipeline in &map.pipelines {
        let mut members: Vec<usize> = pipeline
            .component_ids
            .iter()
            .filter_map(|id| node_index(map, id))
            .collect();
        if members.is_empty() {
            continue;
        }
        let xs = members.iter().map(|index| positions[*index].x);
        let min_x = xs.clone().fold(f64::INFINITY, f64::min);
        let max_x = xs.fold(f64::NEG_INFINITY, f64::max);
        let y = positions[*members.last().unwrap_or(&0)].y;
        let box_height = NODE_RADIUS * 4.0;
        let box_top = y - box_height / 2.0;
        if let Some(parent) = node_index(map, &pipeline.parent) {
            positions[parent] = Point::new((min_x + max_x) / 2.0, box_top - SQUARE_SIZE / 6.0);
        }
        let outline = rounded_rect(min_x - 15.0, box_top, max_x - min_x + 30.0, box_height, 4.0);
        let stroke = super::Painter::stroke(painter.colors.axis, 1.5, &[]);
        painter.scene.push_path(outline, None, stroke);
        members.sort_by(|a, b| map.nodes[*a].evolution.total_cmp(&map.nodes[*b].evolution));
        evolution_links.extend(members.windows(2).map(|pair| (pair[0], pair[1])));
    }
    let color = painter.colors.link;
    for (from, to) in evolution_links {
        painter.line(positions[from], positions[to], color, 1.0, &[4.0, 4.0]);
    }
}

/// 노드 둘레에서 끊는 반지름. 파이프라인 부모 네모는 대각선 절반이다
fn node_radius(map: &WardleyMap, index: usize) -> f64 {
    match map.nodes[index].is_pipeline_parent {
        true => SQUARE_SIZE / 2.0_f64.sqrt(),
        false => NODE_RADIUS,
    }
}

/// 그릴 연결선: 양 끝 노드가 있고, 파이프라인 구성 요소에서 제 부모로 가는 선은 뺀다
fn visible_links(map: &WardleyMap) -> Vec<(&MapLink, usize, usize)> {
    map.links
        .iter()
        .filter_map(|link| {
            let source = node_index(map, &link.source)?;
            let target = node_index(map, &link.target)?;
            let is_member_to_parent = map.pipelines.iter().any(|pipeline| {
                pipeline.parent == link.target && pipeline.component_ids.contains(&link.source)
            });
            (!is_member_to_parent).then_some((link, source, target))
        })
        .collect()
}

pub fn draw_links(painter: &mut Painter, map: &WardleyMap, positions: &[Point]) {
    let links = visible_links(map);
    let color = painter.colors.link;
    for (link, source, target) in &links {
        let (from, to) = (positions[*source], positions[*target]);
        let distance = (to.x - from.x).hypot(to.y - from.y);
        let unit = ((to.x - from.x) / distance, (to.y - from.y) / distance);
        let start = Point::new(
            from.x + unit.0 * node_radius(map, *source),
            from.y + unit.1 * node_radius(map, *source),
        );
        let end = Point::new(
            to.x - unit.0 * node_radius(map, *target),
            to.y - unit.1 * node_radius(map, *target),
        );
        let dash: &[f64] = if link.is_dashed { &[6.0, 6.0] } else { &[] };
        painter.line(start, end, color, 1.0, dash);
        if matches!(link.flow, Some(LinkFlow::Forward | LinkFlow::Bidirectional)) {
            painter.marker(end, unit, LINK_MARKER_SCALE, &END_ARROW, color);
        }
        if matches!(
            link.flow,
            Some(LinkFlow::Backward | LinkFlow::Bidirectional)
        ) {
            painter.marker(start, unit, LINK_MARKER_SCALE, &START_ARROW, color);
        }
    }
    for (link, source, target) in &links {
        if let Some(label) = &link.label {
            draw_link_label(painter, label, positions[*source], positions[*target]);
        }
    }
}

/// 연결선 가운데에서 수직으로 8px 비킨 자리에, 선 방향으로 돌린(뒤집히지 않게) 10px 글자
fn draw_link_label(painter: &mut Painter, label: &str, from: Point, to: Point) {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let distance = dx.hypot(dy);
    let at = Point::new(
        (from.x + to.x) / 2.0 + dy / distance * 8.0,
        (from.y + to.y) / 2.0 - dx / distance * 8.0,
    );
    let mut angle = dy.atan2(dx).to_degrees();
    if !(-90.0..=90.0).contains(&angle) {
        angle += 180.0;
    }
    let style = TextStyle {
        font_size: LABEL_FONT_SIZE,
        weight: FontWeight::Regular,
        anchor: Anchor::Middle,
        baseline: Baseline::Middle,
        color: painter.colors.axis_text,
    };
    painter.text(label, at, style, Some(angle));
}

/// 진화 추세: 노드에서 목표 진화 자리(같은 가시성)까지 노드 반지름 + 2만큼 줄인 빨간 점선 화살표
pub fn draw_trends(painter: &mut Painter, map: &WardleyMap, frame: Frame, positions: &[Point]) {
    let color = painter.colors.evolution;
    for (id, target_evolution) in &map.trends {
        let Some(index) = node_index(map, id) else {
            continue;
        };
        let origin = positions[index];
        let target = Point::new(
            frame.project_x(*target_evolution),
            frame.project_y(map.nodes[index].visibility),
        );
        let (dx, dy) = (target.x - origin.x, target.y - origin.y);
        let distance = dx.hypot(dy);
        let shorten = NODE_RADIUS + 2.0;
        let end = match distance > shorten {
            true => Point::new(
                target.x - dx / distance * shorten,
                target.y - dy / distance * shorten,
            ),
            false => target,
        };
        painter.line(origin, end, color, 1.0, &[4.0, 4.0]);
        if distance > 0.0 {
            let unit = (dx / distance, dy / distance);
            painter.marker(end, unit, TREND_MARKER_SCALE, &END_ARROW, color);
        }
    }
}
