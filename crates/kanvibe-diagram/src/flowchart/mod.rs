//! flowchart·graph 다이어그램(Mermaid 11 flowchart-v2, htmlLabels, dagre 레이아웃).

pub(crate) mod clusters;
pub(crate) mod geometry;
pub(crate) mod layout;
pub(crate) mod parser;
pub(crate) mod render;
pub(crate) mod shapes;

use std::collections::HashMap;
use std::rc::Rc;

use self::clusters::{ClusterContext, adjust_clusters_and_edges};
use self::layout::{Level, LevelEdge, LevelNode, LevelPainter, render_level};
use self::parser::{ArrowType, Direction, Flowchart, Stroke, Vertex};
use self::render::ClusterPainter;
use self::shapes::{NodeGeometry, node_geometry};
use crate::label::{LabelBox, measure_label};
use crate::scene::{Color, Scene, SceneItem};
use crate::text::FontBook;
use crate::theme::Palette;
use crate::{DiagramError, LabelMode, RenderContext};

/// 노드·subgraph 패딩(flowchart.padding 기본 15, subgraph는 8)
const NODE_PADDING: f64 = 15.0;
const CLUSTER_PADDING: f64 = 8.0;
/// setupViewPortForSVG의 diagramPadding
const DIAGRAM_PADDING: f64 = 8.0;

/// classDef·style·linkStyle에서 읽은 그리기 속성
#[derive(Clone, Debug, Default)]
pub struct ElementStyle {
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: Option<f64>,
    pub dash: Vec<f64>,
    pub text_color: Option<Color>,
}

impl ElementStyle {
    fn apply(&mut self, declaration: &str) {
        let Some((key, value)) = declaration.split_once(':') else {
            return;
        };
        let value = value.trim().trim_end_matches("!important").trim();
        match key.trim() {
            "fill" => self.fill = Color::parse_css(value),
            "stroke" => self.stroke = Color::parse_css(value),
            "stroke-width" => self.stroke_width = value.trim_end_matches("px").parse().ok(),
            "stroke-dasharray" => {
                self.dash = value
                    .split([',', ' '])
                    .filter_map(|part| part.trim().parse().ok())
                    .collect();
            }
            "color" => self.text_color = Color::parse_css(value),
            _ => {}
        }
    }

    pub(crate) fn from_declarations<'a>(declarations: impl Iterator<Item = &'a String>) -> Self {
        let mut style = Self::default();
        for declaration in declarations {
            style.apply(declaration);
        }
        style
    }
}

#[derive(Clone, Debug)]
pub struct FlowNode {
    pub id: String,
    pub label: LabelBox,
    /// 자기 간선용 보조 노드처럼 그리지 않는 노드
    pub is_hidden: bool,
    pub parent: Option<String>,
    pub geometry: Option<NodeGeometry>,
    pub style: ElementStyle,
    pub dir: Option<Direction>,
    pub padding: f64,
    /// 윤곽선·라벨 위에 덧그리는 요소(노드 중심 기준 좌표). 다른 다이어그램의 장식용이며 flowchart는 비워 둔다
    pub decorations: Vec<SceneItem>,
    /// 클러스터 상자를 기본 사각형 대신 그리는 함수. 비어 있으면 flowchart 상자를 그린다
    pub cluster_painter: Option<ClusterPainter>,
}

#[derive(Clone, Debug)]
pub struct FlowEdge {
    pub id: String,
    pub start: String,
    pub end: String,
    pub label: LabelBox,
    pub arrow_start: ArrowType,
    pub arrow_end: ArrowType,
    pub stroke: Stroke,
    pub minlen: i32,
    pub style: ElementStyle,
}

/// SVG 글자 간선 라벨의 바탕 사각형이 글자 상자 밖으로 나가는 폭(createFormattedText의 padding)
pub const SVG_EDGE_LABEL_PADDING: f64 = 2.0;

impl FlowEdge {
    /// dagre에 넘기는 간선 라벨 크기. HTML 라벨은 div 상자, SVG 글자 라벨은 바탕 사각형까지 넣은 묶음 상자다
    pub fn label_layout_size(&self) -> (f64, f64) {
        let label = &self.label;
        if label.mode == LabelMode::Html || label.is_empty() {
            return (label.width, label.height);
        }
        let padding = SVG_EDGE_LABEL_PADDING * 2.0;
        (label.width + padding, label.height + padding)
    }
}

/// 그리는 동안 쓰는 글꼴과 색
pub struct FlowRender<'a> {
    pub fonts: &'a FontBook,
    pub palette: &'a Palette,
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let fonts = context.fonts;
    let chart = parser::parse(definition)?;
    if chart.edges.len() > crate::MAX_EDGES {
        return Err(DiagramError::Syntax(format!(
            "간선이 {}개를 넘습니다",
            crate::MAX_EDGES
        )));
    }
    let render = FlowRender {
        fonts,
        palette: context.palette,
    };
    let (nodes, edges) = build_layout_data(&chart, fonts, context.label_mode);
    Ok(render_layout(&render, chart.direction, nodes, edges))
}

/// 노드·간선 목록을 단계별 dagre로 배치해 그리고 diagramPadding을 둘러 장면으로 만든다(통합 렌더러 공용)
pub(crate) fn render_layout(
    render: &dyn LevelPainter,
    direction: Direction,
    nodes: Vec<FlowNode>,
    edges: Vec<FlowEdge>,
) -> Scene {
    let mut level = build_level(direction, nodes, edges);
    let mut context = ClusterContext::default();
    adjust_clusters_and_edges(&mut level, &mut context);
    let group = render_level(render, &mut level, &mut context, None);
    let bounds = group.bounds;
    let mut scene = Scene {
        is_start_aligned: false,
        width: bounds.width() + DIAGRAM_PADDING * 2.0,
        height: bounds.height() + DIAGRAM_PADDING * 2.0,
        items: group.items,
    };
    scene.translate(
        DIAGRAM_PADDING - bounds.min_x,
        DIAGRAM_PADDING - bounds.min_y,
    );
    scene
}

fn node_style(chart: &Flowchart, vertex: &Vertex) -> ElementStyle {
    let class_names = ["default".to_string()]
        .into_iter()
        .chain(vertex.classes.iter().cloned());
    let mut declarations: Vec<String> = Vec::new();
    for class_name in class_names {
        if let Some(styles) = chart.class_styles.get(&class_name) {
            declarations.extend(styles.iter().cloned());
        }
    }
    declarations.extend(vertex.styles.iter().cloned());
    ElementStyle::from_declarations(declarations.iter())
}

/// FlowDB.getData: subgraph(바깥부터) 다음 정점, 그다음 간선 순서
/// subgraph는 바깥부터 클러스터 노드로 넣는다
fn subgraph_nodes(
    chart: &Flowchart,
    fonts: &FontBook,
    mode: LabelMode,
    parents: &HashMap<&str, &str>,
) -> Vec<FlowNode> {
    chart
        .subgraphs
        .iter()
        .rev()
        .map(|subgraph| FlowNode {
            id: subgraph.id.clone(),
            label: measure_label(fonts, &subgraph.title, mode),
            is_hidden: false,
            parent: parents
                .get(subgraph.id.as_str())
                .map(|parent| parent.to_string()),
            geometry: None,
            style: ElementStyle::default(),
            dir: subgraph.dir,
            padding: CLUSTER_PADDING,
            decorations: Vec::new(),
            cluster_painter: None,
        })
        .collect()
}

fn build_layout_data(
    chart: &Flowchart,
    fonts: &FontBook,
    mode: LabelMode,
) -> (Vec<FlowNode>, Vec<FlowEdge>) {
    let mut parents: HashMap<&str, &str> = HashMap::new();
    for subgraph in chart.subgraphs.iter().rev() {
        for id in &subgraph.nodes {
            parents.insert(id, &subgraph.id);
        }
    }
    let mut nodes = subgraph_nodes(chart, fonts, mode, &parents);
    for vertex in &chart.vertices {
        if nodes.iter().any(|node| node.id == vertex.id) {
            continue;
        }
        let label = measure_label(fonts, &vertex.text, mode);
        let geometry = node_geometry(vertex.shape, label.width, label.height, NODE_PADDING);
        nodes.push(FlowNode {
            id: vertex.id.clone(),
            label,
            is_hidden: false,
            parent: parents
                .get(vertex.id.as_str())
                .map(|parent| parent.to_string()),
            geometry: Some(geometry),
            style: node_style(chart, vertex),
            dir: None,
            padding: NODE_PADDING,
            decorations: Vec::new(),
            cluster_painter: None,
        });
    }
    (nodes, build_edges(chart, fonts, mode))
}

fn build_edges(chart: &Flowchart, fonts: &FontBook, mode: LabelMode) -> Vec<FlowEdge> {
    let mut counters: HashMap<(String, String), usize> = HashMap::new();
    chart
        .edges
        .iter()
        .map(|edge| {
            let counter = counters
                .entry((edge.start.clone(), edge.end.clone()))
                .or_insert(0);
            let id = format!("L_{}_{}_{}", edge.start, edge.end, counter);
            *counter += 1;
            let is_open = edge.stroke == Stroke::Invisible;
            FlowEdge {
                id,
                start: edge.start.clone(),
                end: edge.end.clone(),
                label: measure_label(fonts, &edge.text, mode),
                arrow_start: if is_open {
                    ArrowType::None
                } else {
                    edge.arrow_start
                },
                arrow_end: if is_open {
                    ArrowType::None
                } else {
                    edge.arrow_end
                },
                stroke: edge.stroke,
                minlen: edge.length,
                style: ElementStyle::from_declarations(edge.styles.iter()),
            }
        })
        .collect()
}

pub(crate) fn build_level(
    direction: Direction,
    nodes: Vec<FlowNode>,
    edges: Vec<FlowEdge>,
) -> Level {
    let mut level = Level::new(direction);
    for node in &nodes {
        let entry = LevelNode {
            node: Some(Rc::new(node.clone())),
            ..LevelNode::default()
        };
        level.graph.set_node(&node.id, entry);
        if let Some(parent) = &node.parent {
            level.graph.set_parent(&node.id, Some(parent));
        }
    }
    for edge in edges {
        if edge.start == edge.end {
            add_self_loop(&mut level, &nodes, edge);
            continue;
        }
        let (start, end, id) = (edge.start.clone(), edge.end.clone(), edge.id.clone());
        let label = LevelEdge {
            edge: Rc::new(edge),
            from_cluster: None,
            to_cluster: None,
        };
        level.graph.set_edge(&start, &end, label, Some(&id));
    }
    level
}

/// 자기 간선은 10×10 보이지 않는 노드 두 개를 거치는 간선 세 개로 바꾼다
fn add_self_loop(level: &mut Level, nodes: &[FlowNode], edge: FlowEdge) {
    let node_id = edge.start.clone();
    let parent = parent_of(nodes, &node_id);
    let special = [1, 2].map(|number| format!("{node_id}---{node_id}---{number}"));
    for id in &special {
        let helper = self_loop_helper(id, parent.clone());
        level.graph.set_node(
            id,
            LevelNode {
                node: Some(Rc::new(helper)),
                ..LevelNode::default()
            },
        );
        level.graph.set_parent(id, parent.as_deref());
    }
    let [first, middle, last] = self_loop_parts(&edge);
    let links = [
        (
            node_id.clone(),
            special[0].clone(),
            first,
            format!("{node_id}-cyclic-special-0"),
        ),
        (
            special[0].clone(),
            special[1].clone(),
            middle,
            format!("{node_id}-cyclic-special-1"),
        ),
        (
            special[1].clone(),
            node_id.clone(),
            last,
            format!("{node_id}-cyc<lic-special-2"),
        ),
    ];
    for (start, end, link, name) in links {
        let label = LevelEdge {
            edge: Rc::new(link),
            from_cluster: None,
            to_cluster: None,
        };
        level.graph.set_edge(&start, &end, label, Some(&name));
    }
}

fn parent_of(nodes: &[FlowNode], id: &str) -> Option<String> {
    nodes
        .iter()
        .find(|node| node.id == id)
        .and_then(|node| node.parent.clone())
}

/// 자기 간선의 세 조각: 시작 화살표만, 라벨만, 끝 화살표만 남긴다
fn self_loop_parts(edge: &FlowEdge) -> [FlowEdge; 3] {
    let without_label = |start: ArrowType, end: ArrowType| FlowEdge {
        label: LabelBox::default(),
        arrow_start: start,
        arrow_end: end,
        ..edge.clone()
    };
    let middle = FlowEdge {
        arrow_start: ArrowType::None,
        arrow_end: ArrowType::None,
        ..edge.clone()
    };
    [
        without_label(edge.arrow_start, ArrowType::None),
        middle,
        without_label(ArrowType::None, edge.arrow_end),
    ]
}

/// 자기 간선이 거치는 labelRect 보조 노드(그리지 않음). 원본은 10×10으로 넣지만 insertNode의
/// updateNodeBounds가 dagre 전에 0.1×0.1 사각형 크기로 덮어쓴다
fn self_loop_helper(id: &str, parent: Option<String>) -> FlowNode {
    let geometry = NodeGeometry {
        width: 0.1,
        height: 0.1,
        ..node_geometry(parser::NodeShape::Rect, 0.0, 0.0, 0.0)
    };
    FlowNode {
        id: id.to_string(),
        label: LabelBox::default(),
        is_hidden: true,
        parent,
        geometry: Some(geometry),
        style: ElementStyle::default(),
        dir: None,
        padding: 0.0,
        decorations: Vec::new(),
        cluster_painter: None,
    }
}
