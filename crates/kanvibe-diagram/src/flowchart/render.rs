//! flowchart 요소를 장면으로 그린다: 노드 모양과 라벨, 클러스터 상자, 간선과 화살표, 간선 라벨.

use super::geometry::{BoundaryBox, end_tangent};
use super::label::LabelBox;
use super::layout::{LevelNode, edge_path};
use super::parser::{ArrowType, Stroke};
use super::{FlowEdge, FlowNode, FlowRender};
use crate::scene::{Color, PathCommand, Point, SceneItem, StrokeStyle, polygon, rounded_rect};

/// getBBox처럼 그린 요소의 기하 상자를 모은다. 크기가 0인 상자는 넣지 않는다
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        }
    }
}

impl Bounds {
    pub fn include_rect(&mut self, x: f64, y: f64, width: f64, height: f64) {
        if width == 0.0 && height == 0.0 {
            return;
        }
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x + width);
        self.max_y = self.max_y.max(y + height);
    }

    pub fn is_empty(&self) -> bool {
        self.min_x > self.max_x
    }

    pub fn width(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            self.max_x - self.min_x
        }
    }

    pub fn height(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            self.max_y - self.min_y
        }
    }

    fn translated(&self, dx: f64, dy: f64) -> Self {
        if self.is_empty() {
            return *self;
        }
        Self {
            min_x: self.min_x + dx,
            min_y: self.min_y + dy,
            max_x: self.max_x + dx,
            max_y: self.max_y + dy,
        }
    }

    fn union(&mut self, other: &Bounds) {
        if other.is_empty() {
            return;
        }
        self.min_x = self.min_x.min(other.min_x);
        self.min_y = self.min_y.min(other.min_y);
        self.max_x = self.max_x.max(other.max_x);
        self.max_y = self.max_y.max(other.max_y);
    }
}

/// SVG `<g>` 하나에 해당하는 장면 조각
#[derive(Clone, Debug, Default)]
pub struct Group {
    pub items: Vec<SceneItem>,
    pub bounds: Bounds,
    /// 클러스터 상자가 dagre 크기보다 넓어진 만큼(positionNode의 diff)
    pub diff: f64,
}

impl Group {
    pub fn append(&mut self, other: Group) {
        self.bounds.union(&other.bounds);
        self.items.extend(other.items);
    }

    pub fn append_translated(&mut self, other: Group, dx: f64, dy: f64) {
        let mut scene = crate::scene::Scene {
            width: 0.0,
            height: 0.0,
            items: other.items,
        };
        scene.translate(dx, dy);
        self.bounds.union(&other.bounds.translated(dx, dy));
        self.items.extend(scene.items);
    }

    fn push_path(
        &mut self,
        commands: Vec<PathCommand>,
        fill: Option<Color>,
        stroke: Option<StrokeStyle>,
    ) {
        self.bounds.include_path(&commands);
        self.items.push(SceneItem::Path(crate::scene::PathItem {
            commands,
            fill,
            stroke,
        }));
    }
}

fn translate_commands(commands: &[PathCommand], dx: f64, dy: f64) -> Vec<PathCommand> {
    let mut scene = crate::scene::Scene {
        width: 0.0,
        height: 0.0,
        items: Vec::new(),
    };
    scene.push_path(commands.to_vec(), None, None);
    scene.translate(dx, dy);
    match scene.items.pop() {
        Some(SceneItem::Path(path)) => path.commands,
        _ => Vec::new(),
    }
}

fn draw_label(
    render: &FlowRender,
    group: &mut Group,
    label: &LabelBox,
    left: f64,
    top: f64,
    color: Color,
) {
    group
        .bounds
        .include_rect(left, top, label.width, label.height);
    for item in label.text_items(render.fonts, left, top, color) {
        group.items.push(SceneItem::Text(item));
    }
}

pub fn draw_node(render: &FlowRender, group: &mut Group, flow: &FlowNode, center: Point) {
    let Some(geometry) = &flow.geometry else {
        return;
    };
    if flow.is_hidden {
        return;
    }
    let fill = flow.style.fill.unwrap_or(render.palette.node_fill);
    let stroke = StrokeStyle {
        color: flow.style.stroke.unwrap_or(render.palette.node_stroke),
        width: flow.style.stroke_width.unwrap_or(1.0),
        dash: flow.style.dash.clone(),
    };
    for outline in &geometry.outlines {
        group.push_path(
            translate_commands(outline, center.x, center.y),
            Some(fill),
            Some(stroke.clone()),
        );
    }
    let left = center.x - flow.label.width / 2.0;
    let top = center.y - flow.label.height / 2.0 + geometry.label_offset_y;
    let color = flow.style.text_color.unwrap_or(render.palette.text);
    draw_label(render, group, &flow.label, left, top, color);
}

/// clusters.js rect(): 라벨이 dagre 폭보다 넓으면 상자를 넓힌다. 그린 상자를 돌려준다
pub fn draw_cluster(
    render: &FlowRender,
    group: &mut Group,
    flow: &FlowNode,
    node: &LevelNode,
) -> BoundaryBox {
    let padding = flow.padding;
    let fits_label = node.width <= flow.label.width + padding;
    let width = if fits_label {
        flow.label.width + padding
    } else {
        node.width
    };
    group.diff = if fits_label {
        (width - node.width) / 2.0 - padding
    } else {
        -padding
    };
    let (x, y) = (node.x - width / 2.0, node.y - node.height / 2.0);
    let fill = flow.style.fill.unwrap_or(render.palette.cluster_fill);
    let stroke = StrokeStyle {
        color: flow.style.stroke.unwrap_or(render.palette.cluster_stroke),
        width: flow.style.stroke_width.unwrap_or(1.0),
        dash: Vec::new(),
    };
    group.push_path(
        rounded_rect(x, y, width, node.height, 0.0),
        Some(fill),
        Some(stroke),
    );
    let color = flow.style.text_color.unwrap_or(render.palette.cluster_text);
    draw_label(
        render,
        group,
        &flow.label,
        node.x - flow.label.width / 2.0,
        y,
        color,
    );
    BoundaryBox {
        center: Point::new(node.x, node.y),
        width,
        height: node.height,
    }
}

fn edge_stroke(render: &FlowRender, edge: &FlowEdge) -> Option<StrokeStyle> {
    let width = match edge.stroke {
        Stroke::Invisible => return None,
        Stroke::Thick => 3.5,
        _ => 1.0,
    };
    let dash = if edge.stroke == Stroke::Dotted {
        vec![2.0, 2.0]
    } else {
        Vec::new()
    };
    Some(StrokeStyle {
        color: edge.style.stroke.unwrap_or(render.palette.line),
        width: edge.style.stroke_width.unwrap_or(width),
        dash,
    })
}

pub fn draw_edge(render: &FlowRender, group: &mut Group, edge: &FlowEdge, points: &[Point]) {
    let commands = edge_path(edge, points);
    let Some(stroke) = edge_stroke(render, edge) else {
        group.bounds.include_path(&commands);
        return;
    };
    let color = stroke.color;
    group.push_path(commands.clone(), None, Some(stroke));
    if let Some((anchor, angle)) = end_tangent(&commands, true) {
        draw_marker(group, edge.arrow_end, true, anchor, angle, color);
    }
    if let Some((anchor, angle)) = end_tangent(&commands, false) {
        draw_marker(group, edge.arrow_start, false, anchor, angle, color);
    }
}

/// 표식 좌표(viewBox 단위)를 경로 끝점·방향에 맞춰 옮기는 변환
#[derive(Clone, Copy)]
struct MarkerPlacement {
    anchor: Point,
    angle: f64,
    reference: Point,
    scale: f64,
}

impl MarkerPlacement {
    fn place(&self, point: Point) -> Point {
        let (sin, cos) = self.angle.sin_cos();
        let x = (point.x - self.reference.x) * self.scale;
        let y = (point.y - self.reference.y) * self.scale;
        Point::new(
            self.anchor.x + x * cos - y * sin,
            self.anchor.y + x * sin + y * cos,
        )
    }
}

fn solid_stroke(color: Color, width: f64) -> StrokeStyle {
    StrokeStyle {
        color,
        width,
        dash: Vec::new(),
    }
}

/// markers.js의 point·circle·cross 표식
fn draw_marker(
    group: &mut Group,
    arrow: ArrowType,
    is_end: bool,
    anchor: Point,
    angle: f64,
    color: Color,
) {
    let (commands, fill, stroke) = match arrow {
        ArrowType::None => return,
        ArrowType::Point => point_marker(is_end, anchor, angle, color),
        ArrowType::Circle => circle_marker(is_end, anchor, angle, color),
        ArrowType::Cross => cross_marker(is_end, anchor, angle, color),
    };
    group.items.push(SceneItem::Path(crate::scene::PathItem {
        commands,
        fill,
        stroke: Some(stroke),
    }));
}

type MarkerShape = (Vec<PathCommand>, Option<Color>, StrokeStyle);

/// viewBox 0 0 10 10, 표식 크기 8(배율 0.8), 끝은 refX 5·시작은 refX 4.5
fn point_marker(is_end: bool, anchor: Point, angle: f64, color: Color) -> MarkerShape {
    let (shape, reference) = if is_end {
        (
            [
                Point::new(0.0, 0.0),
                Point::new(10.0, 5.0),
                Point::new(0.0, 10.0),
            ],
            Point::new(5.0, 5.0),
        )
    } else {
        (
            [
                Point::new(0.0, 5.0),
                Point::new(10.0, 10.0),
                Point::new(10.0, 0.0),
            ],
            Point::new(4.5, 5.0),
        )
    };
    let placement = MarkerPlacement {
        anchor,
        angle,
        reference,
        scale: 0.8,
    };
    let points: Vec<Point> = shape.iter().map(|point| placement.place(*point)).collect();
    (polygon(&points), Some(color), solid_stroke(color, 0.8))
}

/// viewBox 0 0 10 10, 표식 크기 11(배율 1.1), 끝은 refX 11·시작은 refX -1
fn circle_marker(is_end: bool, anchor: Point, angle: f64, color: Color) -> MarkerShape {
    let reference = if is_end {
        Point::new(11.0, 5.0)
    } else {
        Point::new(-1.0, 5.0)
    };
    let center = MarkerPlacement {
        anchor,
        angle,
        reference,
        scale: 1.1,
    }
    .place(Point::new(5.0, 5.0));
    (
        crate::scene::ellipse(center, 5.5, 5.5),
        Some(color),
        solid_stroke(color, 1.1),
    )
}

/// viewBox 0 0 11 11, 배율 1, 끝은 refX 12·시작은 refX -1, 선 굵기 2
fn cross_marker(is_end: bool, anchor: Point, angle: f64, color: Color) -> MarkerShape {
    let reference = if is_end {
        Point::new(12.0, 5.2)
    } else {
        Point::new(-1.0, 5.2)
    };
    let placement = MarkerPlacement {
        anchor,
        angle,
        reference,
        scale: 1.0,
    };
    let lines = vec![
        PathCommand::MoveTo(placement.place(Point::new(1.0, 1.0))),
        PathCommand::LineTo(placement.place(Point::new(10.0, 10.0))),
        PathCommand::MoveTo(placement.place(Point::new(10.0, 1.0))),
        PathCommand::LineTo(placement.place(Point::new(1.0, 10.0))),
    ];
    (lines, None, solid_stroke(color, 2.0))
}

pub fn draw_edge_label(render: &FlowRender, group: &mut Group, edge: &FlowEdge, center: Point) {
    let label = &edge.label;
    let left = center.x - label.width / 2.0;
    let top = center.y - label.height / 2.0;
    let background = rounded_rect(left, top, label.width, label.height, 0.0);
    group.items.push(SceneItem::Path(crate::scene::PathItem {
        commands: background.clone(),
        fill: Some(render.palette.edge_label_outer),
        stroke: None,
    }));
    group.items.push(SceneItem::Path(crate::scene::PathItem {
        commands: background,
        fill: Some(render.palette.edge_label_inner),
        stroke: None,
    }));
    let color = edge.style.text_color.unwrap_or(render.palette.text);
    draw_label(render, group, label, left, top, color);
}
