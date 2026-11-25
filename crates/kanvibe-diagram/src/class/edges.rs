//! 클래스 관계선: rendering-util edges.js insertEdge·positionEdgeLabel과 markers.js의 class 표식.
//!
//! 선 모양(모서리 보정 → 표식 자리만큼 끝 당기기 → curveBasis)은 flowchart geometry를 그대로 쓰고, 표식
//! 다섯 가지(상속·합성·집합·의존·막대사탕)와 양 끝 다중성 라벨, 관계 라벨 바탕은 classDiagram 스타일을 따른다.
//! 관계 라벨·다중성 자리는 flowchart [`EdgeRoute::label_placement`](crate::flowchart::layout::EdgeRoute)
//! (클러스터 자르기·isLabelCoordinateInPath 판정)를 따른다.

use super::ClassColors;
use super::class_box::LineBox;
use super::measure::MeasuredText;
use super::parser::RelationKind;
use crate::LabelMode;
use crate::flowchart::geometry::{apply_line_offsets, curve_basis, end_tangent, fix_corners};
use crate::flowchart::layout::EdgeRoute;
use crate::flowchart::render::Group;
use crate::label::FONT_SIZE;
use crate::scene::{
    ClipItem, PathCommand, PathItem, Point, SceneItem, StrokeStyle, ellipse, polygon, rounded_rect,
};
use crate::text::FontBook;

/// `.edgeTerminals { font-size: 11px }`
pub const TERMINAL_FONT_SIZE: f64 = 11.0;
/// calcTerminalLabelPosition: class 간선은 표식 이름이 늘 있어 표식 크기 10을 쓴다
const TERMINAL_MARKER_SIZE: f64 = 10.0;
/// SVG 글자 관계 라벨 바탕은 글자 bbox보다 사방 2px 크다
const SVG_LABEL_PADDING: f64 = 2.0;
/// setTerminalWidth: HTML 다중성 foreignObject는 글자당 9px 폭, 12px 높이로 바뀐다
const TERMINAL_BOX_CHARACTER_WIDTH: f64 = 9.0;
const TERMINAL_BOX_HEIGHT: f64 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinePattern {
    Solid,
    Dashed,
    Dotted,
}

/// 관계 하나의 그리기 정보
#[derive(Clone, Debug)]
pub struct ClassEdge {
    pub start_kind: Option<RelationKind>,
    pub end_kind: Option<RelationKind>,
    pub pattern: LinePattern,
    pub label: Option<MeasuredText>,
    pub start_label: Option<MeasuredText>,
    pub end_label: Option<MeasuredText>,
}

/// 자기 간선을 세 조각으로 나누면 시작 조각만 시작 표식, 끝 조각만 끝 표식, 가운데만 라벨을 갖는다
#[derive(Clone, Copy, Debug)]
pub struct EdgePart {
    pub has_start: bool,
    pub has_end: bool,
    pub has_label: bool,
}

/// 관계선을 그리는 데 쓰는 글꼴·색·라벨 방식
pub struct EdgePainter<'a> {
    pub fonts: &'a FontBook,
    pub colors: &'a ClassColors,
    pub mode: LabelMode,
}

impl EdgePainter<'_> {
    /// 선과 표식은 `edges`(edgePaths)에, 관계 라벨과 다중성은 `labels`(edgeLabels)에 쌓는다
    pub fn draw(
        &self,
        edges: &mut Group,
        labels: &mut Group,
        edge: &ClassEdge,
        part: EdgePart,
        route: &EdgeRoute,
    ) {
        let start_kind = edge.start_kind.filter(|_| part.has_start);
        let end_kind = edge.end_kind.filter(|_| part.has_end);
        let shifted = apply_line_offsets(
            &fix_corners(&route.points),
            start_kind.map(line_offset),
            end_kind.map(line_offset),
        );
        let commands = curve_basis(&shifted);
        edges.push_path(commands.clone(), None, Some(self.line_stroke(edge.pattern)));
        self.draw_markers(edges, &commands, start_kind, end_kind);
        let (label_points, label_position) = route.label_placement(&commands);
        if let Some(label) = edge.label.as_ref().filter(|_| part.has_label) {
            self.draw_relation_label(labels, label, label_position);
        }
        if let Some(label) = edge.start_label.as_ref().filter(|_| part.has_start) {
            let position = terminal_position(label_points, true);
            self.draw_terminal(labels, label, position, true);
        }
        if let Some(label) = edge.end_label.as_ref().filter(|_| part.has_end) {
            let position = terminal_position(label_points, false);
            self.draw_terminal(labels, label, position, false);
        }
    }

    fn line_stroke(&self, pattern: LinePattern) -> StrokeStyle {
        let dash = match pattern {
            LinePattern::Solid => Vec::new(),
            LinePattern::Dashed => vec![3.0, 3.0],
            LinePattern::Dotted => vec![2.0, 2.0],
        };
        StrokeStyle {
            color: self.colors.line,
            width: 1.0,
            dash,
        }
    }

    fn draw_markers(
        &self,
        edges: &mut Group,
        commands: &[PathCommand],
        start_kind: Option<RelationKind>,
        end_kind: Option<RelationKind>,
    ) {
        let ends = [(start_kind, true), (end_kind, false)];
        for (kind, is_start) in ends {
            let Some(kind) = kind else {
                continue;
            };
            let Some((anchor, angle)) = end_tangent(commands, !is_start) else {
                continue;
            };
            // 표식은 getBBox에 들어가지 않는다
            let shape = marker_shape(kind, is_start);
            edges.items.push(SceneItem::Path(PathItem {
                commands: shape.placed(anchor, angle),
                fill: shape.is_filled.then_some(self.colors.line),
                stroke: Some(self.line_stroke(LinePattern::Solid)),
            }));
        }
    }

    /// 관계 라벨: HTML은 라벨 크기 바탕, SVG 글자는 글자 bbox ±2 바탕(.edgeLabel .label rect)
    fn draw_relation_label(&self, labels: &mut Group, label: &MeasuredText, center: Point) {
        let line = LineBox::for_mode(self.fonts, self.mode);
        let (background, baseline) = match self.mode {
            LabelMode::Html => (
                (label.width, line.height),
                center.y - line.height / 2.0 + line.baseline,
            ),
            LabelMode::SvgText => (
                (
                    label.width + SVG_LABEL_PADDING * 2.0,
                    line.height + SVG_LABEL_PADDING * 2.0,
                ),
                center.y - (line.top + line.height / 2.0) + line.baseline,
            ),
        };
        let (width, height) = background;
        let (left, top) = (center.x - width / 2.0, center.y - height / 2.0);
        labels.push_path(
            rounded_rect(left, top, width, height, 0.0),
            Some(self.colors.label_background),
            None,
        );
        let text_left = center.x - label.width / 2.0 - label.left;
        let texts = label.text_items(text_left, baseline, FONT_SIZE, self.colors.class_text);
        labels.items.extend(texts.into_iter().map(SceneItem::Text));
    }

    /// 다중성 라벨. 원본은 시작 라벨만 가운데로 옮기고 끝 라벨은 왼쪽 위를 그 자리에 둔다
    fn draw_terminal(
        &self,
        labels: &mut Group,
        label: &MeasuredText,
        position: Point,
        is_start: bool,
    ) {
        let metrics = self.fonts.primary_metrics(TERMINAL_FONT_SIZE);
        let content = metrics.ascent + metrics.descent;
        if self.mode == LabelMode::SvgText {
            let baseline = if is_start {
                position.y + metrics.ascent - content / 2.0
            } else {
                position.y + TERMINAL_FONT_SIZE
            };
            let left = position.x - label.width / 2.0 - label.left;
            let top = baseline - metrics.ascent;
            labels
                .bounds
                .include_rect(left + label.left, top, label.width, content);
            let texts = label.text_items(left, baseline, TERMINAL_FONT_SIZE, self.colors.root_text);
            labels.items.extend(texts.into_iter().map(SceneItem::Text));
            return;
        }
        let line_height = TERMINAL_FONT_SIZE * 1.5;
        let (left, top) = if is_start {
            (
                position.x - label.width / 2.0,
                position.y - line_height / 2.0,
            )
        } else {
            (position.x, position.y)
        };
        // setTerminalWidth가 foreignObject를 글자 수 × 9px, 높이 12px로 줄여 그 밖은 잘린다
        let box_width = label.text().chars().count() as f64 * TERMINAL_BOX_CHARACTER_WIDTH;
        labels
            .bounds
            .include_rect(left, top, box_width, TERMINAL_BOX_HEIGHT);
        let baseline = top + (line_height - content) / 2.0 + metrics.ascent;
        let texts = label.text_items(left, baseline, TERMINAL_FONT_SIZE, self.colors.class_text);
        labels.items.push(SceneItem::Clip(ClipItem {
            x: left,
            y: top,
            width: box_width,
            height: TERMINAL_BOX_HEIGHT,
            items: texts.into_iter().map(SceneItem::Text).collect(),
        }));
    }
}

/// markerOffsets: 표식이 차지하는 만큼 선 끝을 당긴다
fn line_offset(kind: RelationKind) -> f64 {
    match kind {
        RelationKind::Aggregation | RelationKind::Extension | RelationKind::Composition => 17.25,
        RelationKind::Dependency => 6.0,
        RelationKind::Lollipop => 13.5,
    }
}

/// markers.js의 class 표식(표식 좌표, refX·refY, 채움 여부). 크기 배율은 선 굵기 1이라 1이다
struct MarkerShape {
    outline: Vec<PathCommand>,
    reference: Point,
    is_filled: bool,
}

impl MarkerShape {
    /// orient="auto": 기준점을 경로 끝에 두고 경로 방향으로 돌린다
    fn placed(&self, anchor: Point, angle: f64) -> Vec<PathCommand> {
        let (sin, cos) = angle.sin_cos();
        let place = |point: Point| {
            let (x, y) = (point.x - self.reference.x, point.y - self.reference.y);
            Point::new(anchor.x + x * cos - y * sin, anchor.y + x * sin + y * cos)
        };
        self.outline
            .iter()
            .map(|command| match *command {
                PathCommand::MoveTo(point) => PathCommand::MoveTo(place(point)),
                PathCommand::LineTo(point) => PathCommand::LineTo(place(point)),
                PathCommand::QuadTo(control, point) => {
                    PathCommand::QuadTo(place(control), place(point))
                }
                PathCommand::CubicTo(first, second, point) => {
                    PathCommand::CubicTo(place(first), place(second), place(point))
                }
                PathCommand::Close => PathCommand::Close,
            })
            .collect()
    }
}

fn marker_shape(kind: RelationKind, is_start: bool) -> MarkerShape {
    let points = |coordinates: &[(f64, f64)]| {
        let corners: Vec<Point> = coordinates
            .iter()
            .map(|(x, y)| Point::new(*x, *y))
            .collect();
        polygon(&corners)
    };
    let diamond = [(18.0, 7.0), (9.0, 13.0), (1.0, 7.0), (9.0, 1.0)];
    let (outline, reference_x) = match (kind, is_start) {
        (RelationKind::Extension, true) => (points(&[(1.0, 7.0), (18.0, 13.0), (18.0, 1.0)]), 18.0),
        (RelationKind::Extension, false) => (points(&[(1.0, 1.0), (1.0, 13.0), (18.0, 7.0)]), 1.0),
        (RelationKind::Aggregation | RelationKind::Composition, true) => (points(&diamond), 18.0),
        (RelationKind::Aggregation | RelationKind::Composition, false) => (points(&diamond), 1.0),
        (RelationKind::Dependency, true) => (
            points(&[(5.0, 7.0), (9.0, 13.0), (1.0, 7.0), (9.0, 1.0)]),
            6.0,
        ),
        (RelationKind::Dependency, false) => (
            points(&[(18.0, 7.0), (9.0, 13.0), (14.0, 7.0), (9.0, 1.0)]),
            13.0,
        ),
        (RelationKind::Lollipop, is_start) => (
            ellipse(Point::new(7.0, 7.0), 6.0, 6.0),
            if is_start { 13.0 } else { 1.0 },
        ),
    };
    MarkerShape {
        outline,
        reference: Point::new(reference_x, 7.0),
        is_filled: matches!(kind, RelationKind::Composition | RelationKind::Dependency),
    }
}

/// utils.calcTerminalLabelPosition: 시작 라벨은 "start_right", 끝 라벨은 "end_left" 자리
fn terminal_position(points: &[Point], is_start: bool) -> Point {
    let mut ordered = points.to_vec();
    if !is_start {
        ordered.reverse();
    }
    let Some(first) = ordered.first().copied() else {
        return Point::new(0.0, 0.0);
    };
    let center = point_at_distance(&ordered, 25.0 + TERMINAL_MARKER_SIZE);
    let distance = 10.0 + TERMINAL_MARKER_SIZE * 0.5;
    let angle = (first.y - center.y).atan2(first.x - center.x);
    let shift = if is_start { 0.0 } else { 5.0 };
    Point::new(
        angle.sin() * distance + (first.x + center.x) / 2.0 - shift,
        -angle.cos() * distance + (first.y + center.y) / 2.0 - shift,
    )
}

/// utils.calculatePoint: 꺾은선을 따라 `distance`만큼 간 점(소수 다섯째 자리 반올림)
fn point_at_distance(points: &[Point], distance: f64) -> Point {
    let mut remaining = distance;
    for pair in points.windows(2) {
        let (previous, point) = (pair[0], pair[1]);
        let segment = ((point.x - previous.x).powi(2) + (point.y - previous.y).powi(2)).sqrt();
        if segment == 0.0 {
            return previous;
        }
        if segment < remaining {
            remaining -= segment;
            continue;
        }
        let ratio = remaining / segment;
        let round = |value: f64| (value * 100_000.0).round() / 100_000.0;
        return Point::new(
            round((1.0 - ratio) * previous.x + ratio * point.x),
            round((1.0 - ratio) * previous.y + ratio * point.y),
        );
    }
    points[points.len() - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 다중성_자리는_끝에서_35px_지점_옆이다() {
        let points = [Point::new(0.0, 0.0), Point::new(0.0, 100.0)];
        let start = terminal_position(&points, true);
        assert!(
            (start.x + 15.0).abs() < 1e-9 && (start.y - 17.5).abs() < 1e-9,
            "{start:?}"
        );
        let end = terminal_position(&points, false);
        assert!(
            (end.x - 10.0).abs() < 1e-9 && (end.y - 77.5).abs() < 1e-9,
            "{end:?}"
        );
    }
}
