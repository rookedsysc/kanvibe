//! dagre-d3-es 7.0.14 `layout()` 이식.
//!
//! Mermaid 11 flowchart·state·class 다이어그램은 이 레이아웃으로 노드 좌표와 간선 경로를 정한다.
//! 기준 이미지와 같은 좌표가 나오도록 단계 순서, 순회 순서, 동점 처리까지 원본을 따른다.
//! 원본 라벨 객체를 여러 그래프가 공유하던 부분은 이름으로 다시 찾아 쓰는 방식으로 바꿨다.

mod nesting;
mod order;
mod position;
mod rank;

use std::collections::HashMap;

use crate::graph::{EdgeKey, Graph};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RankDir {
    #[default]
    TopBottom,
    BottomTop,
    LeftRight,
    RightLeft,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelPos {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DummyKind {
    Edge,
    EdgeLabel,
    EdgeProxy,
    Border,
    Root,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderSide {
    Left,
    Right,
}

/// 레이아웃 설정(graph label)
#[derive(Clone, Debug)]
pub struct GraphConfig {
    pub rankdir: RankDir,
    pub nodesep: f64,
    pub edgesep: f64,
    pub ranksep: f64,
    pub marginx: f64,
    pub marginy: f64,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            rankdir: RankDir::TopBottom,
            nodesep: 50.0,
            edgesep: 20.0,
            ranksep: 50.0,
            marginx: 0.0,
            marginy: 0.0,
        }
    }
}

/// 입력·출력 노드. 복합 노드는 레이아웃 뒤 width·height가 채워진다
#[derive(Clone, Debug, Default)]
pub struct LayoutNode {
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
}

/// 입력·출력 간선. 라벨이 있으면 width·height를 주고, 레이아웃 뒤 x·y와 points가 채워진다
#[derive(Clone, Debug)]
pub struct LayoutEdge {
    pub minlen: i32,
    pub weight: f64,
    pub width: f64,
    pub height: f64,
    pub labelpos: LabelPos,
    pub labeloffset: f64,
    pub points: Vec<Point>,
    pub label_position: Option<Point>,
}

impl Default for LayoutEdge {
    fn default() -> Self {
        Self {
            minlen: 1,
            weight: 1.0,
            width: 0.0,
            height: 0.0,
            labelpos: LabelPos::Right,
            labeloffset: 10.0,
            points: Vec::new(),
            label_position: None,
        }
    }
}

/// 레이아웃 내부 노드 라벨
#[derive(Clone, Debug, Default)]
pub(crate) struct NodeLabel {
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
    pub rank: Option<i32>,
    /// edge-proxy 더미의 소수 순위((w - v) / 2 + v가 정수가 아닐 때)
    pub fractional_rank: Option<f64>,
    pub order: Option<usize>,
    pub dummy: Option<DummyKind>,
    pub labelpos: Option<LabelPos>,
    pub border_top: Option<String>,
    pub border_bottom: Option<String>,
    pub border_left: HashMap<i32, String>,
    pub border_right: HashMap<i32, String>,
    pub min_rank: Option<i32>,
    pub max_rank: Option<i32>,
    pub border_type: Option<BorderSide>,
    /// edge·edge-label 더미가 대신하는 원래 간선
    pub edge_obj: Option<EdgeKey>,
    /// 더미 사슬 첫 노드가 들고 있는 원래 간선 라벨
    pub edge_label: Option<Box<EdgeLabel>>,
    /// edge-proxy 더미가 가리키는 간선
    pub proxy_edge: Option<EdgeKey>,
}

/// 레이아웃 내부 간선 라벨
#[derive(Clone, Debug, Default)]
pub(crate) struct EdgeLabel {
    pub minlen: i32,
    pub weight: f64,
    pub width: f64,
    pub height: f64,
    pub labelpos: Option<LabelPos>,
    pub labeloffset: f64,
    pub points: Vec<Point>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub label_rank: Option<f64>,
    pub reversed: bool,
    pub forward_name: Option<Option<String>>,
    pub is_nesting_edge: bool,
}

pub(crate) type LayoutGraph = Graph<NodeLabel, EdgeLabel>;

/// 레이아웃 상태. lodash `uniqueId`처럼 더미 이름 번호를 이어서 센다
pub(crate) struct LayoutState {
    pub graph: LayoutGraph,
    pub config: GraphConfig,
    pub next_unique_id: usize,
    pub nesting_root: Option<String>,
    pub node_rank_factor: i32,
    pub dummy_chains: Vec<String>,
}

impl LayoutState {
    pub fn unique_id(&mut self, prefix: &str) -> String {
        loop {
            self.next_unique_id += 1;
            let candidate = format!("{prefix}{}", self.next_unique_id);
            if !self.graph.has_node(&candidate) {
                return candidate;
            }
        }
    }

    pub fn add_dummy_node(
        &mut self,
        kind: DummyKind,
        mut label: NodeLabel,
        prefix: &str,
    ) -> String {
        let name = self.unique_id(prefix);
        label.dummy = Some(kind);
        self.graph.set_node(&name, label);
        name
    }

    pub fn max_rank(&self) -> i32 {
        self.graph
            .nodes()
            .iter()
            .filter_map(|v| self.graph.node(v).rank)
            .max()
            .unwrap_or(0)
    }

    /// util.buildLayerMatrix: 순위별로 order 자리에 노드를 놓는다
    pub fn build_layer_matrix(&self) -> Vec<Vec<String>> {
        let max_rank = usize::try_from(self.max_rank()).unwrap_or(0);
        let mut layers: Vec<Vec<Option<String>>> = vec![Vec::new(); max_rank + 1];
        for v in self.graph.nodes() {
            let node = self.graph.node(&v);
            let (Some(rank), Some(order)) = (node.rank, node.order) else {
                continue;
            };
            let layer = &mut layers[usize::try_from(rank).unwrap_or(0)];
            if layer.len() <= order {
                layer.resize(order + 1, None);
            }
            layer[order] = Some(v);
        }
        layers
            .into_iter()
            .map(|layer| layer.into_iter().flatten().collect())
            .collect()
    }
}

/// dagre 입력 그래프
pub struct DagreInput {
    pub config: GraphConfig,
    pub graph: Graph<LayoutNode, LayoutEdge>,
}

/// 레이아웃을 돌려 입력 그래프의 좌표를 채우고 전체 크기를 돌려준다
pub fn layout(input: &mut DagreInput) -> (f64, f64) {
    let mut state = build_layout_graph(input);
    let size = run_layout(&mut state);
    update_input_graph(input, &state);
    size
}

fn build_layout_graph(input: &DagreInput) -> LayoutState {
    let mut graph = LayoutGraph::new(true, true);
    for v in input.graph.nodes() {
        let node = input.graph.node(&v);
        graph.set_node(
            &v,
            NodeLabel {
                width: node.width,
                height: node.height,
                ..NodeLabel::default()
            },
        );
        graph.set_parent(&v, input.graph.parent(&v).as_deref());
    }
    for edge in input.graph.edges() {
        let source = input.graph.edge_by_key(&edge);
        let label = EdgeLabel {
            minlen: source.minlen,
            weight: source.weight,
            width: source.width,
            height: source.height,
            labelpos: Some(source.labelpos),
            labeloffset: source.labeloffset,
            ..EdgeLabel::default()
        };
        graph.set_edge(&edge.v, &edge.w, label, edge.name.as_deref());
    }
    LayoutState {
        graph,
        config: input.config.clone(),
        next_unique_id: 0,
        nesting_root: None,
        node_rank_factor: 1,
        dummy_chains: Vec::new(),
    }
}

fn run_layout(state: &mut LayoutState) -> (f64, f64) {
    make_space_for_edge_labels(state);
    nesting::acyclic_run(state);
    nesting::nesting_graph_run(state);
    rank_graph(state);
    inject_edge_label_proxies(state);
    nesting::remove_empty_ranks(state);
    nesting::nesting_graph_cleanup(state);
    nesting::normalize_ranks(state);
    assign_rank_min_max(state);
    remove_edge_label_proxies(state);
    nesting::normalize_run(state);
    nesting::parent_dummy_chains(state);
    nesting::add_border_segments(state);
    order::order(state);
    assign_self_edge_free_order(state);
    coordinate_adjust(state);
    position::position(state);
    remove_border_nodes(state);
    nesting::normalize_undo(state);
    fixup_edge_label_coords(state);
    coordinate_undo(state);
    let size = translate_graph(state);
    assign_node_intersects(state);
    reverse_points_for_reversed_edges(state);
    nesting::acyclic_undo(state);
    size
}

fn make_space_for_edge_labels(state: &mut LayoutState) {
    state.config.ranksep /= 2.0;
    let is_vertical = matches!(
        state.config.rankdir,
        RankDir::TopBottom | RankDir::BottomTop
    );
    for edge in state.graph.edges() {
        let label = state.graph.edge_mut(&edge);
        label.minlen *= 2;
        if label.labelpos != Some(LabelPos::Center) {
            if is_vertical {
                label.width += label.labeloffset;
            } else {
                label.height += label.labeloffset;
            }
        }
    }
}

/// rank(util.asNonCompoundGraph(g)): 자식 없는 노드만 순위를 받는다
fn rank_graph(state: &mut LayoutState) {
    let mut simple: Graph<i32, rank::RankEdge> = Graph::new(true, false);
    for v in state.graph.nodes() {
        if state.graph.children(Some(&v)).is_empty() {
            simple.set_node(&v, 0);
        }
    }
    for edge in state.graph.edges() {
        let label = state.graph.edge_by_key(&edge);
        let rank_edge = rank::RankEdge {
            weight: label.weight,
            minlen: label.minlen,
        };
        simple.set_edge(&edge.v, &edge.w, rank_edge, edge.name.as_deref());
    }
    let ranks = rank::network_simplex(&simple);
    for (v, rank) in ranks {
        if state.graph.has_node(&v) {
            state.graph.node_mut(&v).rank = Some(rank);
        }
    }
}

fn inject_edge_label_proxies(state: &mut LayoutState) {
    for edge in state.graph.edges() {
        let label = state.graph.edge_by_key(&edge);
        if label.width == 0.0 || label.height == 0.0 {
            continue;
        }
        let v_rank = state.graph.node(&edge.v).rank.unwrap_or(0);
        let w_rank = state.graph.node(&edge.w).rank.unwrap_or(0);
        let doubled = w_rank - v_rank + 2 * v_rank;
        // 원본은 (w - v) / 2 + v를 그대로 둔다. 소수 순위는 층 배열에 들어가지 않으므로 따로 들고 간다
        let proxy = if doubled % 2 == 0 {
            NodeLabel {
                rank: Some(doubled / 2),
                proxy_edge: Some(edge.clone()),
                ..NodeLabel::default()
            }
        } else {
            let fractional = f64::from(doubled) / 2.0;
            NodeLabel {
                fractional_rank: Some(fractional),
                proxy_edge: Some(edge.clone()),
                ..NodeLabel::default()
            }
        };
        state.add_dummy_node(DummyKind::EdgeProxy, proxy, "_ep");
    }
}

fn assign_rank_min_max(state: &mut LayoutState) {
    for v in state.graph.nodes() {
        let Some(top) = state.graph.node(&v).border_top.clone() else {
            continue;
        };
        let bottom = state
            .graph
            .node(&v)
            .border_bottom
            .clone()
            .expect("border bottom");
        let min_rank = state.graph.node(&top).rank;
        let max_rank = state.graph.node(&bottom).rank;
        let node = state.graph.node_mut(&v);
        node.min_rank = min_rank;
        node.max_rank = max_rank;
    }
}

fn remove_edge_label_proxies(state: &mut LayoutState) {
    for v in state.graph.nodes() {
        let node = state.graph.node(&v);
        if node.dummy != Some(DummyKind::EdgeProxy) {
            continue;
        }
        let edge = node.proxy_edge.clone().expect("proxy edge");
        // 소수 순위는 어떤 더미와도 같지 않아 라벨 자리가 생기지 않는다(원본과 같은 동작)
        let label_rank = node.rank.map(f64::from).or(node.fractional_rank);
        state.graph.edge_mut(&edge).label_rank = label_rank;
        state.graph.remove_node(&v);
    }
}

/// insertSelfEdges에서 순서 재배치만 남긴 것. Mermaid는 자기 간선을 미리 풀어 넘기므로 더미는 생기지 않는다
fn assign_self_edge_free_order(state: &mut LayoutState) {
    for layer in state.build_layer_matrix() {
        for (index, v) in layer.iter().enumerate() {
            state.graph.node_mut(v).order = Some(index);
        }
    }
}

fn is_horizontal(rankdir: RankDir) -> bool {
    matches!(rankdir, RankDir::LeftRight | RankDir::RightLeft)
}

fn swap_width_height(state: &mut LayoutState) {
    for v in state.graph.nodes() {
        let node = state.graph.node_mut(&v);
        std::mem::swap(&mut node.width, &mut node.height);
    }
    for edge in state.graph.edges() {
        let label = state.graph.edge_mut(&edge);
        std::mem::swap(&mut label.width, &mut label.height);
    }
}

fn coordinate_adjust(state: &mut LayoutState) {
    if is_horizontal(state.config.rankdir) {
        swap_width_height(state);
    }
}

fn coordinate_undo(state: &mut LayoutState) {
    let rankdir = state.config.rankdir;
    if matches!(rankdir, RankDir::BottomTop | RankDir::RightLeft) {
        for v in state.graph.nodes() {
            let node = state.graph.node_mut(&v);
            node.y = -node.y;
        }
        for edge in state.graph.edges() {
            let label = state.graph.edge_mut(&edge);
            label.points.iter_mut().for_each(|point| point.y = -point.y);
            label.y = label.y.map(|y| -y);
        }
    }
    if is_horizontal(rankdir) {
        for v in state.graph.nodes() {
            let node = state.graph.node_mut(&v);
            std::mem::swap(&mut node.x, &mut node.y);
        }
        for edge in state.graph.edges() {
            let label = state.graph.edge_mut(&edge);
            label
                .points
                .iter_mut()
                .for_each(|point| std::mem::swap(&mut point.x, &mut point.y));
            if let (Some(x), Some(y)) = (label.x, label.y) {
                label.x = Some(y);
                label.y = Some(x);
            }
        }
        swap_width_height(state);
    }
}

fn remove_border_nodes(state: &mut LayoutState) {
    for v in state.graph.nodes() {
        if state.graph.children(Some(&v)).is_empty() {
            continue;
        }
        let node = state.graph.node(&v).clone();
        let top = state.graph.node(node.border_top.as_deref().expect("top")).y;
        let bottom = state
            .graph
            .node(node.border_bottom.as_deref().expect("bottom"))
            .y;
        let left = state.graph.node(last_border(&node.border_left)).x;
        let right = state.graph.node(last_border(&node.border_right)).x;
        let label = state.graph.node_mut(&v);
        label.width = (right - left).abs();
        label.height = (bottom - top).abs();
        label.x = left + label.width / 2.0;
        label.y = top + label.height / 2.0;
    }
    for v in state.graph.nodes() {
        if state.graph.node(&v).dummy == Some(DummyKind::Border) {
            state.graph.remove_node(&v);
        }
    }
}

/// `_.last(node.borderLeft)`: 가장 큰 순위의 경계 노드
fn last_border(borders: &HashMap<i32, String>) -> &str {
    let max_rank = borders.keys().max().expect("border nodes");
    &borders[max_rank]
}

fn fixup_edge_label_coords(state: &mut LayoutState) {
    for edge in state.graph.edges() {
        let label = state.graph.edge_mut(&edge);
        let Some(x) = label.x else {
            continue;
        };
        if matches!(label.labelpos, Some(LabelPos::Left | LabelPos::Right)) {
            label.width -= label.labeloffset;
        }
        match label.labelpos {
            Some(LabelPos::Left) => label.x = Some(x - label.width / 2.0 - label.labeloffset),
            Some(LabelPos::Right) => label.x = Some(x + label.width / 2.0 + label.labeloffset),
            _ => {}
        }
    }
}

fn translate_graph(state: &mut LayoutState) -> (f64, f64) {
    let mut bounds = Bounds::new();
    for v in state.graph.nodes() {
        let node = state.graph.node(&v);
        bounds.include(node.x, node.y, node.width, node.height);
    }
    for edge in state.graph.edges() {
        let label = state.graph.edge_by_key(&edge);
        if let (Some(x), Some(y)) = (label.x, label.y) {
            bounds.include(x, y, label.width, label.height);
        }
    }
    let min_x = bounds.min_x - state.config.marginx;
    let min_y = bounds.min_y - state.config.marginy;
    for v in state.graph.nodes() {
        let node = state.graph.node_mut(&v);
        node.x -= min_x;
        node.y -= min_y;
    }
    for edge in state.graph.edges() {
        let label = state.graph.edge_mut(&edge);
        for point in &mut label.points {
            point.x -= min_x;
            point.y -= min_y;
        }
        label.x = label.x.map(|x| x - min_x);
        label.y = label.y.map(|y| y - min_y);
    }
    (
        bounds.max_x - min_x + state.config.marginx,
        bounds.max_y - min_y + state.config.marginy,
    )
}

struct Bounds {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

impl Bounds {
    /// 원본처럼 max는 0에서 시작한다
    fn new() -> Self {
        Self {
            min_x: f64::INFINITY,
            max_x: 0.0,
            min_y: f64::INFINITY,
            max_y: 0.0,
        }
    }

    fn include(&mut self, x: f64, y: f64, width: f64, height: f64) {
        self.min_x = self.min_x.min(x - width / 2.0);
        self.max_x = self.max_x.max(x + width / 2.0);
        self.min_y = self.min_y.min(y - height / 2.0);
        self.max_y = self.max_y.max(y + height / 2.0);
    }
}

/// util.intersectRect: 사각형 노드 경계와 중심→점 선분이 만나는 점
pub fn intersect_rect(center: Point, width: f64, height: f64, point: Point) -> Point {
    let dx = point.x - center.x;
    let dy = point.y - center.y;
    let mut w = width / 2.0;
    let mut h = height / 2.0;
    if dx == 0.0 && dy == 0.0 {
        return center;
    }
    let (sx, sy) = if dy.abs() * w > dx.abs() * h {
        if dy < 0.0 {
            h = -h;
        }
        (h * dx / dy, h)
    } else {
        if dx < 0.0 {
            w = -w;
        }
        (w, w * dy / dx)
    };
    Point {
        x: center.x + sx,
        y: center.y + sy,
    }
}

fn assign_node_intersects(state: &mut LayoutState) {
    for edge in state.graph.edges() {
        let v = state.graph.node(&edge.v).clone();
        let w = state.graph.node(&edge.w).clone();
        let label = state.graph.edge_mut(&edge);
        let (toward_v, toward_w) = match (label.points.first(), label.points.last()) {
            (Some(first), Some(last)) => (*first, *last),
            _ => (Point { x: w.x, y: w.y }, Point { x: v.x, y: v.y }),
        };
        let start = intersect_rect(Point { x: v.x, y: v.y }, v.width, v.height, toward_v);
        let end = intersect_rect(Point { x: w.x, y: w.y }, w.width, w.height, toward_w);
        label.points.insert(0, start);
        label.points.push(end);
    }
}

fn reverse_points_for_reversed_edges(state: &mut LayoutState) {
    for edge in state.graph.edges() {
        let label = state.graph.edge_mut(&edge);
        if label.reversed {
            label.points.reverse();
        }
    }
}

fn update_input_graph(input: &mut DagreInput, state: &LayoutState) {
    for v in input.graph.nodes() {
        let layout_node = state.graph.node(&v);
        let has_children = !state.graph.children(Some(&v)).is_empty();
        let node = input.graph.node_mut(&v);
        node.x = layout_node.x;
        node.y = layout_node.y;
        if has_children {
            node.width = layout_node.width;
            node.height = layout_node.height;
        }
    }
    for edge in input.graph.edges() {
        let layout_edge = state.graph.edge_by_key(&edge).clone();
        let target = input.graph.edge_mut(&edge);
        target.points = layout_edge.points;
        if let (Some(x), Some(y)) = (layout_edge.x, layout_edge.y) {
            target.label_position = Some(Point { x, y });
        }
    }
}
