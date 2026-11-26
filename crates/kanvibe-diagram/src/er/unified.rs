//! erDiagram·requirementDiagram이 함께 쓰는 Mermaid unified 렌더러 조각.
//!
//! 두 다이어그램 모두 flowchart와 같은 dagre 렌더러(rendering-util/layout-algorithms/dagre)로 그리므로
//! flowchart의 단계 렌더링을 그대로 부르고, 노드 안쪽 그림은 roughjs(거칠기 0) 사각형·선으로 만든다.

use crate::LabelMode;
use crate::flowchart::clusters::{ClusterContext, adjust_clusters_and_edges};
use crate::flowchart::layout::render_level;
use crate::flowchart::parser::Direction;
use crate::flowchart::render::Bounds;
use crate::flowchart::shapes::{Intersect, NodeGeometry};
use crate::flowchart::{FlowEdge, FlowNode, FlowRender, build_level};
use crate::label::{FONT_SIZE, LINE_HEIGHT, LabelBox, LabelLine};
use crate::scene::{
    Color, PathCommand, PathItem, Point, Scene, SceneItem, StrokeStyle, TextItem, polygon,
};
use crate::text::{FontBook, FontWeight};

/// setupViewPortForSVG의 diagramPadding
const DIAGRAM_PADDING: f64 = 8.0;
/// insertTitle의 titleTopMargin 기본값: 제목 기준선이 y = -25에 놓인다
const TITLE_TOP_MARGIN: f64 = 25.0;
/// roughjs 선 굵기(userNodeOverrides 기본 strokeWidth)
const ROUGH_STROKE_WIDTH: f64 = 1.3;

/// 간선 라벨 바탕 안쪽 색. HTML 라벨은 바깥 div(labelBkg) 한 겹만 칠해지므로 투명하게 두고, SVG 글자 라벨은
/// flowchart가 자기 스타일의 `.edgeLabel rect{opacity:0.5}`대로 알파를 절반으로 그리는데 이 다이어그램들의
/// 스타일에는 그 규칙이 없으므로 미리 두 배로 맞춰 둔다
pub fn edge_label_background(color: Color, mode: LabelMode) -> Color {
    let alpha = match mode {
        LabelMode::Html => 0.0,
        LabelMode::SvgText => color.alpha * 2.0,
    };
    Color { alpha, ..color }
}

/// dagre에 넘길 노드·간선과 간격
pub struct UnifiedGraph {
    pub direction: Direction,
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
    pub node_spacing: f64,
    pub rank_spacing: f64,
    pub title: Option<DiagramTitle>,
}

/// 앞머리 `title`을 insertTitle처럼 그림 위 가운데에 놓는 글자
pub struct DiagramTitle {
    pub text: String,
    pub font_size: f64,
    pub color: Color,
}

/// flowchart와 같은 단계 렌더링 뒤 viewBox를 8px 여백으로 맞춘다
pub fn render_unified(render: &FlowRender, graph: UnifiedGraph) -> Scene {
    let mut level = build_level(graph.direction, graph.nodes, graph.edges);
    level.nodesep = graph.node_spacing;
    level.ranksep = graph.rank_spacing;
    let mut context = ClusterContext::default();
    adjust_clusters_and_edges(&mut level, &mut context);
    let group = render_level(render, &mut level, &mut context, None);
    let mut bounds = group.bounds;
    let mut items = group.items;
    if let Some(title) = graph.title.filter(|title| !title.text.is_empty()) {
        items.push(place_title(render.fonts, &title, &mut bounds));
    }
    let mut scene = Scene {
        is_start_aligned: false,
        width: bounds.width() + DIAGRAM_PADDING * 2.0,
        height: bounds.height() + DIAGRAM_PADDING * 2.0,
        items,
    };
    scene.translate(
        DIAGRAM_PADDING - bounds.min_x,
        DIAGRAM_PADDING - bounds.min_y,
    );
    scene
}

/// insertTitle: 그때까지 그린 것의 가운데 위(y = -25)에 가운데 맞춤 글자를 두고, 그 글자의 `getBBox()`를
/// 경계에 더한다
fn place_title(fonts: &FontBook, title: &DiagramTitle, bounds: &mut Bounds) -> SceneItem {
    let center = if bounds.is_empty() {
        0.0
    } else {
        (bounds.min_x + bounds.max_x) / 2.0
    };
    let extent = fonts.measure_svg_runs(&[&title.text], title.font_size, FontWeight::Regular);
    let metrics = fonts.primary_metrics(title.font_size);
    let start_x = center - extent.advance / 2.0;
    let baseline = -TITLE_TOP_MARGIN;
    bounds.include_rect(
        start_x + extent.left,
        baseline - metrics.ascent,
        extent.width(),
        metrics.ascent + metrics.descent,
    );
    SceneItem::Text(TextItem {
        text: title.text.clone(),
        x: start_x,
        baseline,
        font_size: title.font_size,
        weight: FontWeight::Regular,
        color: title.color,
        rotation: None,
    })
}

/// 그림을 모두 장식 조각으로 그리는 사각형 노드(모양 윤곽선·라벨 없음, 교점은 사각형)
pub fn decorated_node(
    id: String,
    width: f64,
    height: f64,
    decorations: Vec<SceneItem>,
) -> FlowNode {
    FlowNode {
        id,
        label: LabelBox::default(),
        is_hidden: false,
        parent: None,
        geometry: Some(NodeGeometry {
            width,
            height,
            outlines: Vec::new(),
            intersect: Intersect::Rect,
            label_offset_y: 0.0,
        }),
        style: Default::default(),
        dir: None,
        padding: 0.0,
        decorations,
        cluster_painter: None,
    }
}

/// 줄을 바꾸지 않는 한 줄 라벨(createText의 width가 글자 폭보다 넉넉한 경우). 빈 글자는 높이 0이다.
/// SVG 글자면 상자 폭은 잉크까지 넣은 `getBBox()` 폭이고, 줄 폭은 진행 폭이다
pub fn measure_single_line(
    fonts: &FontBook,
    text: &str,
    mode: LabelMode,
    weight: FontWeight,
) -> LabelBox {
    let (advance, width, height) = match mode {
        LabelMode::Html => {
            let width = fonts.measure_line(text, FONT_SIZE, weight);
            (width, width, LINE_HEIGHT)
        }
        LabelMode::SvgText => {
            let extent = fonts.measure_svg_runs(&word_runs(text), FONT_SIZE, weight);
            let metrics = fonts.primary_metrics(FONT_SIZE);
            (
                extent.advance,
                extent.width(),
                metrics.ascent + metrics.descent,
            )
        }
    };
    LabelBox {
        lines: vec![LabelLine {
            text: text.to_string(),
            width: advance,
        }],
        width,
        height: if text.is_empty() { 0.0 } else { height },
        mode,
        font_size: FONT_SIZE,
    }
}

/// createText가 낱말마다 두는 `tspan`: 두 번째 낱말부터 앞에 공백이 붙는다
fn word_runs(text: &str) -> Vec<&str> {
    let mut runs = Vec::new();
    let mut start = 0;
    for (index, _) in text.match_indices(' ').filter(|(index, _)| *index > 0) {
        runs.push(&text[start..index]);
        start = index;
    }
    runs.push(&text[start..]);
    runs
}

/// 라벨 상자 왼쪽 위가 `origin`인 왼쪽 맞춤 글자 조각(text-anchor 없는 SVG 글자, text-align: start인 HTML)
pub fn label_items(
    fonts: &FontBook,
    label: &LabelBox,
    origin: Point,
    color: Color,
    weight: FontWeight,
) -> Vec<SceneItem> {
    label
        .text_items(fonts, origin.x, origin.y, color)
        .into_iter()
        .map(|mut item| {
            item.x = origin.x;
            item.weight = weight;
            SceneItem::Text(item)
        })
        .collect()
}

/// 빈 SVG `<text>`도 Chromium `getBBox()` 합집합에 자기 자리 점을 넣는다. 그리지 않고 크기 계산에만 쓰는
/// 높이가 거의 0인 경로다(크기 0 상자는 [`crate::flowchart::render::Bounds`]가 버린다)
pub fn empty_text_extent(origin: Point) -> SceneItem {
    SceneItem::Path(PathItem {
        commands: vec![
            PathCommand::MoveTo(origin),
            PathCommand::LineTo(Point::new(origin.x, origin.y + 1e-6)),
        ],
        fill: None,
        stroke: None,
    })
}

/// roughjs 테두리 펜. 사용자 `stroke-width`가 없으면 userNodeOverrides 기본 굵기 1.3이다
pub fn rough_pen(color: Color, width: Option<f64>) -> StrokeStyle {
    StrokeStyle {
        color,
        width: width.unwrap_or(ROUGH_STROKE_WIDTH),
        dash: Vec::new(),
    }
}

/// roughjs rectangle: 채움 경로 하나와 네 변을 따로 그은 테두리 경로 하나
pub fn rough_rect(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    fill: Color,
    pen: &StrokeStyle,
) -> [SceneItem; 2] {
    let corners = [
        Point::new(x, y),
        Point::new(x + width, y),
        Point::new(x + width, y + height),
        Point::new(x, y + height),
    ];
    let mut sides = Vec::with_capacity(8);
    for (index, corner) in corners.iter().enumerate() {
        sides.push(PathCommand::MoveTo(*corner));
        sides.push(PathCommand::LineTo(corners[(index + 1) % corners.len()]));
    }
    [
        SceneItem::Path(PathItem {
            commands: polygon(&corners),
            fill: Some(fill),
            stroke: None,
        }),
        SceneItem::Path(PathItem {
            commands: sides,
            fill: None,
            stroke: Some(pen.clone()),
        }),
    ]
}

/// roughjs line(또는 두께가 거의 0인 polygon): 펜 굵기 선 하나
pub fn rough_line(from: Point, to: Point, pen: &StrokeStyle) -> SceneItem {
    SceneItem::Path(PathItem {
        commands: vec![PathCommand::MoveTo(from), PathCommand::LineTo(to)],
        fill: None,
        stroke: Some(pen.clone()),
    })
}
