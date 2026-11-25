//! mindmap 다이어그램(mindmapRenderer.ts + rendering-util cose-bilkent 배치).
//!
//! 노드를 그려 getBBox 크기를 얻고, cytoscape-cose-bilkent로 중심을 정한 뒤 간선을 그린다. 간선은 cytoscape가 기본
//! 노드 크기(30px 원)로 자른 두 끝점과 그 가운데 점을 basis 곡선으로 이은 것이라 결국 곧은 선이다.
//! 아이콘(`::icon`)과 사용자 클래스(`:::`)는 색을 바꾸지 않으므로 읽고 버린다.

mod colors;
mod cose;
mod parser;
mod shapes;
mod skia_arc;

use crate::LabelMode;
use crate::label::{LabelBox, measure_label};
use crate::scene::{PathCommand, Point, Scene, StrokeStyle};
use crate::{DiagramError, RenderContext};

use colors::SectionColors;
use parser::MindmapNode;
use shapes::{NodeGeometry, node_geometry};

/// setupViewPortForSVG 여백(`mindmap.padding`)
const VIEWPORT_PADDING: f64 = 10.0;
/// 아이콘 라벨 배경 여백
const ICON_BACKGROUND_PADDING: f64 = 2.0;
/// cytoscape 기본 노드 반지름. 간선 끝점은 노드 중심에서 이만큼 떨어진다
const CYTOSCAPE_NODE_RADIUS: f64 = 15.0;

struct PlacedNode {
    label: LabelBox,
    geometry: NodeGeometry,
    center: Point,
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let nodes = parser::parse(definition)?;
    let measured: Vec<(LabelBox, NodeGeometry)> = nodes
        .iter()
        .map(|node| {
            let measured = measure_label(context.fonts, &node.label, context.label_mode);
            let label = with_icon_background(measured, node.has_icon);
            let geometry = node_geometry(node.shape, &label);
            (label, geometry)
        })
        .collect();
    let sizes: Vec<(f64, f64)> = measured
        .iter()
        .map(|(_, geometry)| {
            let (min_x, min_y, max_x, max_y) = geometry.bounds;
            (max_x - min_x, max_y - min_y)
        })
        .collect();
    let edges = tree_edges(&nodes);
    let centers = cose::layout(&sizes, &edges);
    let placed: Vec<PlacedNode> = measured
        .into_iter()
        .zip(centers)
        .map(|((label, geometry), center)| PlacedNode {
            label,
            geometry,
            center,
        })
        .collect();
    let mut scene = Scene::default();
    draw_edges(&mut scene, &nodes, &placed, &edges, context);
    for (node, placed_node) in nodes.iter().zip(&placed) {
        draw_node(&mut scene, node, placed_node, context);
    }
    fit_viewport(&mut scene, &placed, &edges);
    Ok(scene)
}

/// createFormattedText(addBackground): 아이콘이 있으면 SVG 글자 라벨 상자가 글자 범위보다 사방 2px 크다.
/// 글자는 상자 가운데가 아니라 상자 왼쪽 위에서 그대로 시작하므로 그릴 때 2px 왼쪽으로 옮긴다
fn with_icon_background(mut label: LabelBox, has_icon: bool) -> LabelBox {
    if has_icon && label.mode == LabelMode::SvgText {
        label.width += ICON_BACKGROUND_PADDING * 2.0;
        label.height += ICON_BACKGROUND_PADDING * 2.0;
    }
    label
}

/// generateEdges: 부모→자식 간선을 앞선 깊이 우선 순서로 모은다. 노드 번호가 이미 그 순서라 자식 번호순이다
fn tree_edges(nodes: &[MindmapNode]) -> Vec<(usize, usize)> {
    let mut edges: Vec<(usize, usize)> = nodes
        .iter()
        .enumerate()
        .flat_map(|(parent, node)| node.children.iter().map(move |&child| (parent, child)))
        .collect();
    edges.sort_by_key(|&(_, child)| child);
    edges
}

/// cytoscape가 기본 30px 원으로 자른 간선 끝점
fn edge_endpoints(from: Point, to: Point) -> (Point, Point) {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let length = (dx * dx + dy * dy).sqrt().max(f64::EPSILON);
    let (ux, uy) = (
        dx / length * CYTOSCAPE_NODE_RADIUS,
        dy / length * CYTOSCAPE_NODE_RADIUS,
    );
    (
        Point::new(from.x + ux, from.y + uy),
        Point::new(to.x - ux, to.y - uy),
    )
}

/// `.edge-depth-N`의 stroke-width(17 − 3·(N+1)). 음수 값은 CSS가 버리므로 `.edge`의 3이 남는다
fn edge_width(depth: usize) -> f64 {
    let width = 14.0 - 3.0 * depth as f64;
    if depth <= 10 && width >= 0.0 {
        width
    } else {
        3.0
    }
}

fn draw_edges(
    scene: &mut Scene,
    nodes: &[MindmapNode],
    placed: &[PlacedNode],
    edges: &[(usize, usize)],
    context: &RenderContext,
) {
    for &(parent, child) in edges {
        let (start, end) = edge_endpoints(placed[parent].center, placed[child].center);
        let colors = SectionColors::for_node(context.palette.theme, nodes[child].section);
        let stroke = StrokeStyle {
            color: colors.fill,
            width: edge_width(nodes[parent].level + 1),
            dash: Vec::new(),
        };
        let commands = vec![PathCommand::MoveTo(start), PathCommand::LineTo(end)];
        scene.push_path(commands, None, Some(stroke));
    }
}

fn draw_node(scene: &mut Scene, node: &MindmapNode, placed: &PlacedNode, context: &RenderContext) {
    let colors = SectionColors::for_node(context.palette.theme, node.section);
    let center = placed.center;
    let outline = offset_commands(&placed.geometry.outline, center);
    scene.push_path(outline, Some(colors.fill), None);
    if let (Some((from, to)), Some(line)) = (placed.geometry.underline, colors.line) {
        let commands = vec![
            PathCommand::MoveTo(Point::new(from.x + center.x, from.y + center.y)),
            PathCommand::LineTo(Point::new(to.x + center.x, to.y + center.y)),
        ];
        let stroke = StrokeStyle {
            color: line,
            width: 3.0,
            dash: Vec::new(),
        };
        scene.push_path(commands, None, Some(stroke));
    }
    let origin = placed.geometry.label_origin;
    let background_shift = if node.has_icon && placed.label.mode == LabelMode::SvgText {
        ICON_BACKGROUND_PADDING
    } else {
        0.0
    };
    let texts = placed.label.text_items(
        context.fonts,
        center.x + origin.x - background_shift,
        center.y + origin.y,
        colors.text,
    );
    texts.into_iter().for_each(|text| scene.push_text(text));
}

fn offset_commands(commands: &[PathCommand], offset: Point) -> Vec<PathCommand> {
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

/// setupViewPortForSVG: 그린 것 전체의 bbox에 여백 10을 둘러 viewBox를 잡는다
fn fit_viewport(scene: &mut Scene, placed: &[PlacedNode], edges: &[(usize, usize)]) {
    let mut bounds = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut include = |x: f64, y: f64| {
        bounds = (
            bounds.0.min(x),
            bounds.1.min(y),
            bounds.2.max(x),
            bounds.3.max(y),
        );
    };
    for node in placed {
        let (min_x, min_y, max_x, max_y) = node.geometry.bounds;
        include(node.center.x + min_x, node.center.y + min_y);
        include(node.center.x + max_x, node.center.y + max_y);
    }
    for &(parent, child) in edges {
        let (start, end) = edge_endpoints(placed[parent].center, placed[child].center);
        include(start.x, start.y);
        include(end.x, end.y);
    }
    let left = bounds.0 - VIEWPORT_PADDING;
    let top = bounds.1 - VIEWPORT_PADDING;
    scene.width = bounds.2 - bounds.0 + VIEWPORT_PADDING * 2.0;
    scene.height = bounds.3 - bounds.1 + VIEWPORT_PADDING * 2.0;
    scene.translate(-left, -top);
}
