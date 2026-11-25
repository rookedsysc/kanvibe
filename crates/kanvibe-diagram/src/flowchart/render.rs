//! flowchart 요소를 장면으로 그린다: 노드 모양과 라벨, 클러스터 상자, 간선과 화살표, 간선 라벨.

use super::SVG_EDGE_LABEL_PADDING;
use super::effects::{drop_shadow, gradient_stroke};
use super::geometry::{BoundaryBox, end_tangent};
use super::layout::{EdgeRoute, LevelNode, LevelPainter, edge_path};
use super::parser::{ArrowType, Stroke};
use super::{FlowEdge, FlowNode, FlowRender};
use crate::LabelMode;
use crate::label::{FONT_SIZE, LabelBox};
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

/// 클러스터 상자를 직접 그리는 함수. 상자 크기·diff는 flowchart와 같게 정한 뒤 그 상자(`frame`)를 넘긴다
pub(crate) type ClusterPainter = fn(&FlowRender, &mut Group, &FlowNode, &BoundaryBox);

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
            is_start_aligned: false,
            width: 0.0,
            height: 0.0,
            items: other.items,
        };
        scene.translate(dx, dy);
        self.bounds.union(&other.bounds.translated(dx, dy));
        self.items.extend(scene.items);
    }

    pub fn push_path(
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
        is_start_aligned: false,
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
    let box_top = label.box_top(render.fonts, top);
    group
        .bounds
        .include_rect(left, box_top, label.width, label.height);
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
        let commands = translate_commands(outline, center.x, center.y);
        if let Some(shadow) = flow.style.drop_shadow {
            group.items.extend(drop_shadow(&commands, shadow));
        }
        match flow.style.stroke_gradient {
            Some(colors) => {
                group.push_path(commands.clone(), Some(fill), None);
                let strips = gradient_stroke(&commands, stroke.width, colors);
                group.items.extend(strips);
            }
            None => group.push_path(commands, Some(fill), Some(stroke.clone())),
        }
    }
    let left = center.x - flow.label.width / 2.0;
    let top = center.y - flow.label.height / 2.0 + geometry.label_offset_y;
    let color = flow.style.text_color.unwrap_or(render.palette.text);
    draw_label(render, group, &flow.label, left, top, color);
    append_decorations(group, &flow.decorations, center);
}

/// 노드 중심 기준 장식 요소를 노드 자리로 옮겨 붙인다
fn append_decorations(group: &mut Group, decorations: &[SceneItem], center: Point) {
    if decorations.is_empty() {
        return;
    }
    let mut decorated = Group::default();
    for item in decorations {
        if let SceneItem::Path(path) = item {
            decorated.bounds.include_path(&path.commands);
        }
        decorated.items.push(item.clone());
    }
    group.append_translated(decorated, center.x, center.y);
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
    let frame = BoundaryBox {
        center: Point::new(node.x, node.y),
        width,
        height: node.height,
    };
    if let Some(painter) = flow.cluster_painter {
        painter(render, group, flow, &frame);
        return frame;
    }
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
    frame
}

fn edge_stroke(render: &FlowRender, edge: &FlowEdge) -> Option<StrokeStyle> {
    let width = match edge.stroke {
        Stroke::Invisible => return None,
        Stroke::Thick => 3.5,
        _ => 1.0,
    };
    let dash = if !edge.style.dash.is_empty() {
        edge.style.dash.clone()
    } else {
        match edge.stroke {
            Stroke::Dotted => vec![2.0, 2.0],
            Stroke::Dashed => vec![5.0, 5.0],
            _ => Vec::new(),
        }
    };
    Some(StrokeStyle {
        color: edge.style.stroke.unwrap_or(render.palette.line),
        width: edge.style.stroke_width.unwrap_or(width),
        dash,
    })
}

/// 간선 경로와 표식을 그리고 그린 경로를 돌려준다
pub fn draw_edge(
    render: &FlowRender,
    group: &mut Group,
    edge: &FlowEdge,
    points: &[Point],
) -> Vec<PathCommand> {
    let commands = edge_path(edge, points);
    let Some(stroke) = edge_stroke(render, edge) else {
        group.bounds.include_path(&commands);
        return commands;
    };
    let color = stroke.color;
    let drawn = match edge.style.sketch_seed {
        Some(seed) => sketched_edge(&commands, seed),
        None => commands.clone(),
    };
    group.push_path(drawn.clone(), None, Some(stroke));
    if let Some((anchor, angle)) = end_tangent(&drawn, true) {
        draw_marker(group, edge.arrow_end, true, anchor, angle, color);
    }
    if let Some((anchor, angle)) = end_tangent(&drawn, false) {
        draw_marker(group, edge.arrow_start, false, anchor, angle, color);
    }
    commands
}

/// handDrawn 간선: roughjs `path(d, { roughness: 0.3, seed })`의 테두리 경로(라벨 자리는 원래 경로를 쓴다)
fn sketched_edge(commands: &[PathCommand], seed: i32) -> Vec<PathCommand> {
    let options = crate::rough::RoughOptions {
        roughness: 0.3,
        ..crate::rough::RoughOptions::new(seed)
    };
    let segments = crate::rough::segments_from_commands(commands);
    crate::rough::path(&segments, &options)
        .pop()
        .map(|set| set.commands)
        .unwrap_or_default()
}

impl LevelPainter for FlowRender<'_> {
    fn draw_cluster(&self, group: &mut Group, flow: &FlowNode, node: &LevelNode) -> BoundaryBox {
        draw_cluster(self, group, flow, node)
    }

    fn draw_edge(
        &self,
        edges: &mut Group,
        labels: &mut Group,
        _key: &crate::graph::EdgeKey,
        edge: &FlowEdge,
        route: &EdgeRoute,
    ) {
        let commands = draw_edge(self, edges, edge, &route.points);
        if !edge.label.is_empty() {
            let (_, position) = route.label_placement(&commands);
            draw_edge_label(self, labels, edge, position);
        }
    }

    fn draw_node(&self, group: &mut Group, flow: &FlowNode, center: Point) {
        draw_node(self, group, flow, center);
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
        ArrowType::Barb => barb_marker(anchor, angle, color),
        ArrowType::BarbNeo => barb_neo_marker(anchor, angle, color),
        ArrowType::Custom(marker) => {
            draw_custom_marker(group, marker, anchor, angle, color);
            return;
        }
    };
    group.items.push(SceneItem::Path(crate::scene::PathItem {
        commands,
        fill,
        stroke: Some(stroke),
    }));
}

type MarkerShape = (Vec<PathCommand>, Option<Color>, StrokeStyle);

/// viewBox 없는 SVG marker(markerUnits=strokeWidth, 간선 굵기 1 기준 배율 1). 다른 다이어그램이 정적으로 정의한다
#[derive(Debug)]
pub struct CustomMarker {
    /// refX·refY
    pub reference: Point,
    /// 선보다 먼저 그리는 원(중심, 반지름, 채움색). 채움색이 `None`이면 테두리만 그린다
    pub circle: Option<(Point, f64, Option<Color>)>,
    /// 테두리로만 그리는 선(marker 좌표)
    pub strokes: &'static [PathCommand],
}

/// 같은 정적 정의인지로만 비교한다
impl PartialEq for CustomMarker {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for CustomMarker {}

fn draw_custom_marker(
    group: &mut Group,
    marker: &CustomMarker,
    anchor: Point,
    angle: f64,
    color: Color,
) {
    let placement = MarkerPlacement {
        anchor,
        angle,
        reference: marker.reference,
        scale: 1.0,
    };
    if let Some((center, radius, fill)) = marker.circle {
        group.items.push(SceneItem::Path(crate::scene::PathItem {
            commands: crate::scene::ellipse(placement.place(center), radius, radius),
            fill,
            stroke: Some(solid_stroke(color, 1.0)),
        }));
    }
    let strokes = marker
        .strokes
        .iter()
        .map(|command| place_command(&placement, command))
        .collect();
    group.items.push(SceneItem::Path(crate::scene::PathItem {
        commands: strokes,
        fill: None,
        stroke: Some(solid_stroke(color, 1.0)),
    }));
}

fn place_command(placement: &MarkerPlacement, command: &PathCommand) -> PathCommand {
    match *command {
        PathCommand::MoveTo(point) => PathCommand::MoveTo(placement.place(point)),
        PathCommand::LineTo(point) => PathCommand::LineTo(placement.place(point)),
        PathCommand::QuadTo(control, point) => {
            PathCommand::QuadTo(placement.place(control), placement.place(point))
        }
        PathCommand::CubicTo(first, second, point) => PathCommand::CubicTo(
            placement.place(first),
            placement.place(second),
            placement.place(point),
        ),
        PathCommand::Close => PathCommand::Close,
    }
}

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

/// markers.js barb: userSpaceOnUse(배율 1), refX 19·refY 7, 뾰족한 끝이 경로 끝점에 닿는다
fn barb_marker(anchor: Point, angle: f64, color: Color) -> MarkerShape {
    let placement = MarkerPlacement {
        anchor,
        angle,
        reference: Point::new(19.0, 7.0),
        scale: 1.0,
    };
    let shape = [
        Point::new(19.0, 7.0),
        Point::new(9.0, 13.0),
        Point::new(14.0, 7.0),
        Point::new(9.0, 1.0),
    ];
    let points: Vec<Point> = shape.iter().map(|point| placement.place(*point)).collect();
    (polygon(&points), Some(color), solid_stroke(color, 1.0))
}

/// barbEnd-margin: userSpaceOnUse, refX 17, 테두리 없이 선 색으로 채운 화살촉
fn barb_neo_marker(anchor: Point, angle: f64, color: Color) -> MarkerShape {
    let placement = MarkerPlacement {
        anchor,
        angle,
        reference: Point::new(17.0, 7.0),
        scale: 1.0,
    };
    let shape = [
        Point::new(19.0, 7.0),
        Point::new(11.0, 14.0),
        Point::new(13.0, 7.0),
        Point::new(11.0, 0.0),
    ];
    let points: Vec<Point> = shape.iter().map(|point| placement.place(*point)).collect();
    let no_stroke = solid_stroke(
        Color {
            alpha: 0.0,
            ..color
        },
        0.0,
    );
    (polygon(&points), Some(color), no_stroke)
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
    if edge.label.mode == LabelMode::SvgText {
        draw_svg_edge_label(render, group, edge, center);
        return;
    }
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

/// SVG 글자 간선 라벨: 라벨 묶음을 글자 상자 가운데로 옮기는 computeLabelTransform 때문에 글자 상자 위 끝(기준선 위
/// ascent)이 중심에서 `높이/2` 위에 오고, 글자 묶음 원점은 그보다 ascent − 1em만큼 아래다. 바탕 사각형은 글자 상자를
/// 2px 넓힌 것이고 CSS `opacity: 0.5`가 채움 색 알파에 곱해진다
fn draw_svg_edge_label(render: &FlowRender, group: &mut Group, edge: &FlowEdge, center: Point) {
    let label = &edge.label;
    if label.is_empty() {
        return;
    }
    let ascent = render.fonts.primary_metrics(FONT_SIZE).ascent;
    let box_top = center.y - label.height / 2.0;
    let text_top = box_top + ascent - FONT_SIZE;
    let padding = SVG_EDGE_LABEL_PADDING;
    let background = rounded_rect(
        center.x - label.width / 2.0 - padding,
        box_top - padding,
        label.width + padding * 2.0,
        label.height + padding * 2.0,
        0.0,
    );
    let mut fill = render.palette.edge_label_inner;
    fill.alpha *= 0.5;
    group.push_path(background, Some(fill), None);
    let color = edge.style.text_color.unwrap_or(render.palette.text);
    draw_label(
        render,
        group,
        label,
        center.x - label.width / 2.0,
        text_top,
        color,
    );
}
