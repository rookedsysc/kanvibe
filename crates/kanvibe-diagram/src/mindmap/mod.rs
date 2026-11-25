//! mindmap 다이어그램(mindmapRenderer.ts + rendering-util cose-bilkent 배치).
//!
//! 노드를 그려 getBBox 크기를 얻고, cytoscape-cose-bilkent로 중심을 정한 뒤 간선을 그린다. 간선은 cytoscape가 기본
//! 노드 크기(30px 원)로 자른 두 끝점과 그 가운데 점을 basis 곡선으로 이은 것이라 결국 곧은 선이다.
//! 노드 라벨은 `labelType: "markdown"`이라 `**굵게**`·`*기울임*`을 글자 굵기·기울임으로 그린다. 아이콘
//! (`::icon`)은 TS 앱이 아이콘 글꼴을 싣지 않아 글리프가 보이지 않고, SVG 글자 라벨에만 배경 상자가 생긴다.
//! 사용자 클래스(`:::`)는 색을 바꾸지 않으므로 읽고 버린다.

mod colors;
mod cose;
mod parser;
mod shapes;
mod skia_arc;
mod styled_label;

use crate::LabelMode;
use crate::class::measure::MeasuredText;
use crate::label::{LabelBox, measure_label};
use crate::scene::{Color, PathCommand, Point, Scene, StrokeStyle, TextItem, rounded_rect};
use crate::text::FontBook;
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

/// 잰 노드 라벨. 강조가 섞인 라벨은 줄마다 굵기별 조각(`styled`)으로 그린다
struct NodeLabel {
    label: LabelBox,
    styled: Option<Vec<MeasuredText>>,
}

struct PlacedNode {
    label: NodeLabel,
    geometry: NodeGeometry,
    center: Point,
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let nodes = parser::parse(definition)?;
    let measured: Vec<(NodeLabel, NodeGeometry)> = nodes
        .iter()
        .map(|node| measure_node(context.fonts, node, context.label_mode))
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

fn measure_node(
    fonts: &FontBook,
    node: &MindmapNode,
    mode: LabelMode,
) -> (NodeLabel, NodeGeometry) {
    let measured = measure_node_label(fonts, &node.label, mode);
    let has_background = has_icon_background(node, mode);
    let label = NodeLabel {
        label: with_icon_background(measured.label, has_background),
        styled: measured.styled,
    };
    let shift = if has_background {
        icon_background_offset(fonts, &label.label)
    } else {
        Point::new(0.0, 0.0)
    };
    let geometry = node_geometry(node.shape, &label.label, shift);
    (label, geometry)
}

/// 강조가 섞인 라벨은 [`styled_label`]로, 나머지는 일반 라벨로 잰다
fn measure_node_label(fonts: &FontBook, text: &str, mode: LabelMode) -> NodeLabel {
    match styled_label::measure_styled_label(fonts, text, mode) {
        Some(styled) => NodeLabel {
            label: styled.label,
            styled: Some(styled.lines),
        },
        None => NodeLabel {
            label: measure_label(fonts, text, mode),
            styled: None,
        },
    }
}

fn has_icon_background(node: &MindmapNode, mode: LabelMode) -> bool {
    node.has_icon && mode == LabelMode::SvgText
}

/// createFormattedText(addBackground): 아이콘이 있으면 SVG 글자 라벨 상자가 글자 범위보다 사방 2px 크다
fn with_icon_background(mut label: LabelBox, has_background: bool) -> LabelBox {
    if has_background {
        label.width += ICON_BACKGROUND_PADDING * 2.0;
        label.height += ICON_BACKGROUND_PADDING * 2.0;
    }
    label
}

/// 아이콘 배경 상자 왼쪽 위(라벨 묶음 원점 기준). 글자 getBBox에서 2px 바깥이고, 글자 상자 위 끝은 첫 기준선
/// (원점에서 1em 아래)에서 ascent만큼 위다
fn icon_background_offset(fonts: &FontBook, label: &LabelBox) -> Point {
    Point::new(
        -ICON_BACKGROUND_PADDING,
        label.box_top(fonts, 0.0) - ICON_BACKGROUND_PADDING,
    )
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
    let origin = Point::new(
        center.x + placed.geometry.label_origin.x,
        center.y + placed.geometry.label_origin.y,
    );
    let label = &placed.label.label;
    let has_background = has_icon_background(node, label.mode);
    if has_background {
        let offset = icon_background_offset(context.fonts, label);
        let (x, y) = (origin.x + offset.x, origin.y + offset.y);
        let commands = rounded_rect(x, y, label.width, label.height, 0.0);
        scene.push_path(commands, Some(colors.fill), None);
    }
    // 아이콘 배경이 있으면 글자는 상자 왼쪽 끝이 아니라 라벨 묶음 원점(배경보다 2px 안쪽)에서 시작한다
    let background_shift = if has_background {
        ICON_BACKGROUND_PADDING
    } else {
        0.0
    };
    let left = origin.x - background_shift;
    let texts = label.text_items(context.fonts, left, origin.y, colors.text);
    let texts = match &placed.label.styled {
        Some(styled) => styled_text_items(styled, &texts, label.mode, origin.x, colors.text),
        None => texts,
    };
    texts.into_iter().for_each(|text| scene.push_text(text));
}

/// 강조 조각 라벨은 일반 라벨로 그렸을 때의 줄 기준선을 그대로 쓴다. HTML은 줄마다 가운데 맞춘 x도 같고,
/// SVG 글자는 줄마다 라벨 묶음 원점(`left`)에서 시작한다(tspan x = 0, text-anchor 없음)
fn styled_text_items(
    styled: &[MeasuredText],
    plain_items: &[TextItem],
    mode: LabelMode,
    left: f64,
    color: Color,
) -> Vec<TextItem> {
    styled
        .iter()
        .zip(plain_items)
        .flat_map(|(line, plain)| {
            let x = match mode {
                LabelMode::Html => plain.x,
                LabelMode::SvgText => left,
            };
            line.text_items(x, plain.baseline, plain.font_size, color)
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::FontWeight;
    use crate::text::tests::bundled_fonts;

    fn first_child(definition: &str) -> MindmapNode {
        parser::parse(definition).unwrap().remove(1)
    }

    #[test]
    fn 강조가_섞인_라벨은_굵기별_조각으로_잰다() {
        let fonts = bundled_fonts();
        let label = measure_node_label(&fonts, "Mixed **bold** and *italic*", LabelMode::Html);
        let styled = label.styled.unwrap();
        let weights: Vec<FontWeight> = styled[0].runs.iter().map(|run| run.weight).collect();
        assert_eq!(
            weights,
            vec![
                FontWeight::Regular,
                FontWeight::Bold,
                FontWeight::Regular,
                FontWeight::Italic
            ]
        );
        // 기준 SVG foreignObject 폭 155.9375
        let width = label.label.width;
        assert!((width - 155.9375).abs() < 0.5, "{width}");
        let plain = measure_node_label(&fonts, "plain", LabelMode::Html);
        assert!(plain.styled.is_none());
    }

    #[test]
    fn 아이콘_배경은_svg_글자_라벨을_사방_2px_넓힌다() {
        let fonts = bundled_fonts();
        let square =
            first_child("mindmap\n  root((Icons))\n    sq[Square]\n      ::icon(mdi mdi-skull)\n");
        let (label, geometry) = measure_node(&fonts, &square, LabelMode::SvgText);
        // 기준 SVG: 배경 상자 56.8125×26(라벨 원점에서 (−2, −3)), 사각형 x −48.40625..48.40625
        let width = label.label.width;
        assert!((width - 56.8125).abs() < 0.5, "{width}");
        assert_eq!(label.label.height, 26.0);
        let (min_x, min_y, max_x, max_y) = geometry.bounds;
        assert!((max_x - 54.8125).abs() < 0.5, "{max_x}");
        assert!((min_x + 48.40625).abs() < 0.5, "{min_x}");
        assert_eq!((min_y, max_y), (-23.0, 23.0));
        let (html_label, _) = measure_node(&fonts, &square, LabelMode::Html);
        assert_eq!(html_label.label.height, 24.0);
    }
}
