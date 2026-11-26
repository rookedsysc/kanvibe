//! rendering-util/layout-algorithms/dagre/index.js의 recursiveRender: 단계별로 dagre를 돌리고
//! 클러스터·간선·간선 라벨·노드 순서로 장면 조각을 쌓는다.

use std::collections::HashMap;
use std::rc::Rc;

use super::clusters::{ClusterContext, find_non_cluster_child};
use super::geometry::{BoundaryBox, is_coordinate_in_path};
use super::geometry::{
    apply_line_offsets, curve_basis, cut_path_at_intersect, fix_corners, path_bounds,
    polyline_midpoint,
};
use super::parser::{ArrowType, Direction};
use super::render::{Bounds, Group};
use super::{FlowEdge, FlowNode};
use crate::dagre::{self, DagreInput, GraphConfig, LabelPos, LayoutEdge, LayoutNode, RankDir};
use crate::graph::{EdgeKey, Graph};
use crate::scene::{PathCommand, Point};

#[derive(Clone, Debug, Default)]
pub struct LevelNode {
    pub node: Option<Rc<FlowNode>>,
    /// 따로 떼어 낸 subgraph(clusterNode)의 내부 그래프
    pub cluster_level: Option<Box<Level>>,
    pub cluster_data: Option<Rc<FlowNode>>,
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug)]
pub struct LevelEdge {
    pub edge: Rc<FlowEdge>,
    pub from_cluster: Option<String>,
    pub to_cluster: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Level {
    pub graph: Graph<LevelNode, LevelEdge>,
    pub rankdir: Direction,
    pub nodesep: f64,
    pub ranksep: f64,
    /// dagre가 돌려준 간선 점과 라벨 자리(간선 순서대로)
    pub edge_results: Vec<(EdgeKey, Vec<Point>, Option<Point>)>,
}

impl Level {
    pub fn new(rankdir: Direction) -> Self {
        Self {
            graph: Graph::new(true, true),
            rankdir,
            nodesep: 50.0,
            ranksep: 50.0,
            edge_results: Vec::new(),
        }
    }
}

fn to_rankdir(direction: Direction) -> RankDir {
    match direction {
        Direction::TopBottom => RankDir::TopBottom,
        Direction::BottomTop => RankDir::BottomTop,
        Direction::LeftRight => RankDir::LeftRight,
        Direction::RightLeft => RankDir::RightLeft,
    }
}

/// recursiveRender가 그리는 요소별 그리기. flowchart는 [`super::FlowRender`]가, 같은 dagre 단계를 쓰는
/// 다른 다이어그램(classDiagram 등)은 자기 모양으로 구현한다
pub trait LevelPainter {
    /// dagre 복합 노드로 남은 클러스터 상자를 그리고 간선 자르기에 쓸 경계를 돌려준다
    fn draw_cluster(&self, group: &mut Group, flow: &FlowNode, node: &LevelNode) -> BoundaryBox;
    /// 간선 하나와 그 라벨. `key`는 자기 간선 조각처럼 그래프 간선이 원래 간선과 다를 때 쓴다
    fn draw_edge(
        &self,
        edges: &mut Group,
        labels: &mut Group,
        key: &EdgeKey,
        edge: &FlowEdge,
        route: &EdgeRoute,
    );
    fn draw_node(&self, group: &mut Group, flow: &FlowNode, center: Point);
}

/// insertEdge가 받는 점들: 노드 교점·클러스터 자르기를 마친 점, dagre가 준 점과 라벨 자리
pub struct EdgeRoute<'a> {
    pub points: Vec<Point>,
    pub dagre_points: &'a [Point],
    pub dagre_label: Option<Point>,
    /// 클러스터 경계에서 잘렸는지(pointsHasChanged)
    pub is_cut: bool,
}

impl EdgeRoute<'_> {
    /// positionEdgeLabel: 점이 바뀌었으면(클러스터 자르기, 또는 가운데 점 좌표가 경로 문자열에 없으면)
    /// 고친 점들과 그 가운데를, 아니면 dagre 점들과 dagre 라벨 자리를 쓴다
    pub fn label_placement(&self, commands: &[PathCommand]) -> (&[Point], Point) {
        let middle = self.points.get(self.points.len() / 2);
        let is_changed =
            self.is_cut || middle.is_some_and(|point| !is_coordinate_in_path(*point, commands));
        match self.dagre_label {
            Some(position) if !is_changed => (self.dagre_points, position),
            _ => (&self.points, polyline_midpoint(&self.points)),
        }
    }
}

/// recursiveRender. `parent_cluster`는 떼어 낸 subgraph를 그릴 때 그 subgraph의 데이터다
pub fn render_level(
    render: &dyn LevelPainter,
    level: &mut Level,
    context: &mut ClusterContext,
    parent_cluster: Option<(&str, Rc<FlowNode>)>,
) -> Group {
    if let Some((cluster_id, cluster_data)) = &parent_cluster {
        wrap_in_parent_cluster(level, cluster_id, cluster_data);
    }
    let mut nested: Vec<(String, Group)> = Vec::new();
    for v in level.graph.nodes() {
        if level.graph.node(&v).cluster_level.is_some() {
            let group = render_cluster_node(render, level, context, &v);
            nested.push((v, group));
            continue;
        }
        if !level.graph.children(Some(&v)).is_empty() {
            let anchor = find_non_cluster_child(&level.graph, &v, &v);
            let entry = context.cluster_db.entry(v.clone()).or_insert_with(|| {
                super::clusters::ClusterEntry {
                    anchor: None,
                    has_external_connections: false,
                    cluster_data: None,
                }
            });
            entry.anchor = anchor;
            continue;
        }
        size_leaf_node(level, &v);
    }
    run_dagre(level);
    paint_level(render, level, context, nested)
}

/// 떼어 낸 subgraph 안에서는 subgraph 자신을 모든 노드의 부모 복합 노드로 넣는다
fn wrap_in_parent_cluster(level: &mut Level, cluster_id: &str, cluster_data: &Rc<FlowNode>) {
    for v in level.graph.nodes() {
        level.graph.set_node(
            cluster_id,
            LevelNode {
                node: Some(cluster_data.clone()),
                ..LevelNode::default()
            },
        );
        if v != cluster_id && level.graph.parent(&v).is_none() {
            level.graph.set_parent(&v, Some(cluster_id));
        }
    }
}

fn render_cluster_node(
    render: &dyn LevelPainter,
    level: &mut Level,
    context: &mut ClusterContext,
    v: &str,
) -> Group {
    let (ranksep, nodesep) = (level.ranksep, level.nodesep);
    let node = level.graph.node_mut(v);
    let mut inner = node.cluster_level.take().expect("cluster level");
    inner.ranksep = ranksep + 25.0;
    inner.nodesep = nodesep;
    let cluster_data = node.cluster_data.clone().expect("cluster data");
    let group = render_level(render, &mut inner, context, Some((v, cluster_data)));
    let node = level.graph.node_mut(v);
    node.width = group.bounds.width();
    node.height = group.bounds.height();
    node.cluster_level = Some(inner);
    group
}

fn size_leaf_node(level: &mut Level, v: &str) {
    let node = level.graph.node_mut(v);
    if let Some(flow) = &node.node
        && let Some(geometry) = &flow.geometry
    {
        node.width = geometry.width;
        node.height = geometry.height;
    }
}

fn dagre_input(level: &Level) -> DagreInput {
    let mut graph: Graph<LayoutNode, LayoutEdge> = Graph::new(true, true);
    for v in level.graph.nodes() {
        let node = level.graph.node(&v);
        graph.set_node(
            &v,
            LayoutNode {
                width: node.width,
                height: node.height,
                ..LayoutNode::default()
            },
        );
    }
    for v in level.graph.nodes() {
        if let Some(parent) = level.graph.parent(&v) {
            graph.set_parent(&v, Some(&parent));
        }
    }
    for key in level.graph.edges() {
        let edge = &level.graph.edge_by_key(&key).edge;
        let (label_width, label_height) = edge.label_layout_size();
        let layout_edge = LayoutEdge {
            minlen: edge.minlen,
            width: label_width,
            height: label_height,
            labelpos: LabelPos::Center,
            ..LayoutEdge::default()
        };
        graph.set_edge(&key.v, &key.w, layout_edge, key.name.as_deref());
    }
    let config = GraphConfig {
        rankdir: to_rankdir(level.rankdir),
        nodesep: level.nodesep,
        ranksep: level.ranksep,
        marginx: 8.0,
        marginy: 8.0,
        ..GraphConfig::default()
    };
    DagreInput { config, graph }
}

fn run_dagre(level: &mut Level) {
    let mut input = dagre_input(level);
    dagre::layout(&mut input);
    for v in level.graph.nodes() {
        let placed = input.graph.node(&v);
        let has_children = !level.graph.children(Some(&v)).is_empty();
        let node = level.graph.node_mut(&v);
        node.x = placed.x;
        node.y = placed.y;
        if has_children {
            node.width = placed.width;
            node.height = placed.height;
        }
    }
    level_edge_results(level, &input);
}

fn level_edge_results(level: &mut Level, input: &DagreInput) {
    let convert = |point: &dagre::Point| Point::new(point.x, point.y);
    let mut results = Vec::new();
    for key in level.graph.edges() {
        let placed = input.graph.edge_by_key(&key);
        let points = placed.points.iter().map(convert).collect();
        results.push((key, points, placed.label_position.as_ref().map(convert)));
    }
    level.edge_results = results;
}

/// 원본 sortNodesByHierarchy: 최상위부터 자식 순서로 펼친다
fn nodes_by_hierarchy(graph: &Graph<LevelNode, LevelEdge>, nodes: Vec<String>) -> Vec<String> {
    let mut result = nodes.clone();
    for node in nodes {
        let children = graph.children(Some(&node));
        result.extend(nodes_by_hierarchy(graph, children));
    }
    result
}

fn paint_level(
    render: &dyn LevelPainter,
    level: &Level,
    context: &mut ClusterContext,
    nested: Vec<(String, Group)>,
) -> Group {
    let mut group = paint_clusters(render, level, context);
    let (edges, edge_labels) = paint_edges(render, level, context);
    let nodes = paint_nodes(render, level, nested);
    let diff = group.diff;
    group.append(edges);
    group.append(edge_labels);
    group.append(nodes);
    group.diff = diff;
    group
}

/// dagre 복합 노드로 남은 클러스터 상자. Group.diff에는 노드 순서로 마지막 클러스터의 diff를 남긴다
fn paint_clusters(render: &dyn LevelPainter, level: &Level, context: &mut ClusterContext) -> Group {
    let mut clusters = Group::default();
    let mut cluster_diffs: HashMap<String, f64> = HashMap::new();
    for v in nodes_by_hierarchy(&level.graph, level.graph.children(None)) {
        let node = level.graph.node(&v);
        if node.cluster_level.is_some() || level.graph.children(Some(&v)).is_empty() {
            continue;
        }
        let flow = node.node.clone().expect("cluster data");
        let boundary = render.draw_cluster(&mut clusters, &flow, node);
        cluster_diffs.insert(v.clone(), clusters.diff);
        context.cluster_boxes.insert(v, boundary);
    }
    let nodes = level.graph.nodes();
    clusters.diff = nodes
        .iter()
        .rev()
        .find_map(|v| cluster_diffs.get(v).copied())
        .unwrap_or(0.0);
    clusters
}

fn paint_edges(
    render: &dyn LevelPainter,
    level: &Level,
    context: &ClusterContext,
) -> (Group, Group) {
    let mut edges = Group::default();
    let mut edge_labels = Group::default();
    for (key, points, label_position) in &level.edge_results {
        let label = level.graph.edge_by_key(key);
        let start = level.graph.node(&key.v);
        let end = level.graph.node(&key.w);
        let mut route = route_edge(context, label, start, end, points);
        route.dagre_label = *label_position;
        render.draw_edge(&mut edges, &mut edge_labels, key, &label.edge, &route);
    }
    (edges, edge_labels)
}

fn paint_nodes(render: &dyn LevelPainter, level: &Level, nested: Vec<(String, Group)>) -> Group {
    let mut nodes = Group::default();
    let mut nested = nested.into_iter();
    for v in level.graph.nodes() {
        let node = level.graph.node(&v);
        if node.cluster_level.is_some() {
            let (_, group) = nested.next().expect("nested group");
            // positionNode: translate(x + diff - width / 2, y - height / 2 - 8)
            let dx = node.x + group.diff - node.width / 2.0;
            let dy = node.y - node.height / 2.0 - 8.0;
            nodes.append_translated(group, dx, dy);
            continue;
        }
        if level.graph.children(Some(&v)).is_empty()
            && let Some(flow) = &node.node
        {
            render.draw_node(&mut nodes, flow, Point::new(node.x, node.y));
        }
    }
    nodes
}

/// 두 끝이 모두 모양 노드면 dagre 끝점 대신 모양 교점을 쓴다(떼어 낸 subgraph 노드는 교점 함수가 없다)
fn intersect_with_shapes(start: &LevelNode, end: &LevelNode, route: Vec<Point>) -> Vec<Point> {
    let shape = |node: &LevelNode| {
        node.node
            .as_ref()
            .and_then(|flow| flow.geometry.clone())
            .filter(|_| node.cluster_level.is_none())
    };
    let (Some(start_shape), Some(end_shape)) = (shape(start), shape(end)) else {
        return route;
    };
    if route.len() < 2 {
        return route;
    }
    let mut inner: Vec<Point> = route[1..route.len() - 1].to_vec();
    let first_target = inner.first().copied().unwrap_or(route[route.len() - 1]);
    inner.insert(
        0,
        start_shape.intersect(Point::new(start.x, start.y), first_target),
    );
    let last_target = inner[inner.len() - 1];
    inner.push(end_shape.intersect(Point::new(end.x, end.y), last_target));
    inner
}

/// insertEdge의 점 처리: 노드 모양 교점, 클러스터 경계 자르기
fn route_edge<'a>(
    context: &ClusterContext,
    label: &LevelEdge,
    start: &LevelNode,
    end: &LevelNode,
    points: &'a [Point],
) -> EdgeRoute<'a> {
    let mut route = intersect_with_shapes(start, end, points.to_vec());
    let to_box = label
        .to_cluster
        .as_ref()
        .and_then(|cluster| context.cluster_boxes.get(cluster));
    let from_box = label
        .from_cluster
        .as_ref()
        .and_then(|cluster| context.cluster_boxes.get(cluster));
    if let Some(boundary) = to_box {
        route = cut_path_at_intersect(points, boundary);
    }
    if let Some(boundary) = from_box {
        route.reverse();
        route = cut_path_at_intersect(&route, boundary);
        route.reverse();
    }
    route.retain(|point| !point.y.is_nan());
    EdgeRoute {
        points: route,
        dagre_points: points,
        dagre_label: None,
        is_cut: to_box.is_some() || from_box.is_some(),
    }
}

/// 간선 경로: 모서리 보정 → 화살표 자리 보정 → curveBasis
pub fn edge_path(edge: &FlowEdge, points: &[Point]) -> Vec<PathCommand> {
    let corrected = fix_corners(points);
    let offset = |arrow: ArrowType| match arrow {
        ArrowType::Point => Some(4.0),
        ArrowType::BarbNeo => Some(5.5),
        _ => None,
    };
    let shifted = apply_line_offsets(&corrected, offset(edge.arrow_start), offset(edge.arrow_end));
    curve_basis(&shifted)
}

impl Bounds {
    pub fn include_path(&mut self, commands: &[PathCommand]) {
        if let Some((min_x, min_y, max_x, max_y)) = path_bounds(commands) {
            self.include_rect(min_x, min_y, max_x - min_x, max_y - min_y);
        }
    }
}
