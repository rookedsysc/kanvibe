//! dagre의 그래프 변형 단계: acyclic, nesting-graph, 빈 순위 제거, normalize, parent-dummy-chains,
//! add-border-segments. 복합 노드(subgraph)는 위·아래 경계 노드와 순위별 좌·우 경계 노드로 바뀐다.

use std::collections::HashMap;

use super::{BorderSide, DummyKind, EdgeLabel, LayoutState, NodeLabel, Point};
use crate::graph::EdgeKey;

pub(super) fn acyclic_run(state: &mut LayoutState) {
    for edge in dfs_feedback_arc_set(state) {
        let mut label = state.graph.edge_by_key(&edge).clone();
        state.graph.remove_edge(&edge);
        label.forward_name = Some(edge.name.clone());
        label.reversed = true;
        let name = state.unique_id("rev");
        state.graph.set_edge(&edge.w, &edge.v, label, Some(&name));
    }
}

fn dfs_feedback_arc_set(state: &LayoutState) -> Vec<EdgeKey> {
    let mut fas = Vec::new();
    let mut on_stack: HashMap<String, bool> = HashMap::new();
    let mut visited: HashMap<String, bool> = HashMap::new();
    for v in state.graph.nodes() {
        feedback_dfs(state, &v, &mut visited, &mut on_stack, &mut fas);
    }
    fas
}

fn feedback_dfs(
    state: &LayoutState,
    v: &str,
    visited: &mut HashMap<String, bool>,
    on_stack: &mut HashMap<String, bool>,
    fas: &mut Vec<EdgeKey>,
) {
    if visited.contains_key(v) {
        return;
    }
    visited.insert(v.to_string(), true);
    on_stack.insert(v.to_string(), true);
    for edge in state.graph.out_edges(v) {
        if on_stack.contains_key(&edge.w) {
            fas.push(edge);
        } else {
            feedback_dfs(state, &edge.w, visited, on_stack, fas);
        }
    }
    on_stack.remove(v);
}

pub(super) fn acyclic_undo(state: &mut LayoutState) {
    for edge in state.graph.edges() {
        let label = state.graph.edge_by_key(&edge).clone();
        if !label.reversed {
            continue;
        }
        state.graph.remove_edge(&edge);
        let forward_name = label.forward_name.clone().flatten();
        let restored = EdgeLabel {
            reversed: false,
            forward_name: None,
            ..label
        };
        state
            .graph
            .set_edge(&edge.w, &edge.v, restored, forward_name.as_deref());
    }
}

fn tree_depths(state: &LayoutState) -> HashMap<String, i32> {
    let mut depths = HashMap::new();
    for v in state.graph.children(None) {
        depth_dfs(state, &v, 1, &mut depths);
    }
    depths
}

fn depth_dfs(state: &LayoutState, v: &str, depth: i32, depths: &mut HashMap<String, i32>) {
    for child in state.graph.children(Some(v)) {
        depth_dfs(state, &child, depth + 1, depths);
    }
    depths.insert(v.to_string(), depth);
}

struct NestingContext {
    root: String,
    node_sep: i32,
    weight: f64,
    height: i32,
    depths: HashMap<String, i32>,
}

pub(super) fn nesting_graph_run(state: &mut LayoutState) {
    let root = state.add_dummy_node(DummyKind::Root, NodeLabel::default(), "_root");
    let depths = tree_depths(state);
    let height = depths.values().copied().max().unwrap_or(1) - 1;
    let node_sep = 2 * height + 1;
    state.nesting_root = Some(root.clone());
    for edge in state.graph.edges() {
        state.graph.edge_mut(&edge).minlen *= node_sep;
    }
    let weight = state
        .graph
        .edges()
        .iter()
        .map(|edge| state.graph.edge_by_key(edge).weight)
        .sum::<f64>()
        + 1.0;
    let context = NestingContext {
        root,
        node_sep,
        weight,
        height,
        depths,
    };
    for child in state.graph.children(None) {
        nesting_dfs(state, &context, &child);
    }
    state.node_rank_factor = node_sep;
}

fn nesting_edge(weight: f64, minlen: i32, is_nesting_edge: bool) -> EdgeLabel {
    EdgeLabel {
        weight,
        minlen,
        is_nesting_edge,
        ..EdgeLabel::default()
    }
}

fn nesting_dfs(state: &mut LayoutState, context: &NestingContext, v: &str) {
    let children = state.graph.children(Some(v));
    if children.is_empty() {
        if v != context.root {
            state.graph.set_edge(
                &context.root,
                v,
                nesting_edge(0.0, context.node_sep, false),
                None,
            );
        }
        return;
    }
    let top = state.add_dummy_node(DummyKind::Border, NodeLabel::default(), "_bt");
    let bottom = state.add_dummy_node(DummyKind::Border, NodeLabel::default(), "_bb");
    state.graph.set_parent(&top, Some(v));
    state.graph.node_mut(v).border_top = Some(top.clone());
    state.graph.set_parent(&bottom, Some(v));
    state.graph.node_mut(v).border_bottom = Some(bottom.clone());
    for child in children {
        nesting_dfs(state, context, &child);
        connect_child_borders(state, context, v, &child, (&top, &bottom));
    }
    if state.graph.parent(v).is_none() {
        let minlen = context.height + context.depths[v];
        state
            .graph
            .set_edge(&context.root, &top, nesting_edge(0.0, minlen, false), None);
    }
}

/// 부모의 위·아래 경계를 자식(또는 자식 subgraph의 경계)과 잇는다
fn connect_child_borders(
    state: &mut LayoutState,
    context: &NestingContext,
    v: &str,
    child: &str,
    borders: (&str, &str),
) {
    let (top, bottom) = borders;
    let child_node = state.graph.node(child);
    let child_top = child_node
        .border_top
        .clone()
        .unwrap_or_else(|| child.to_string());
    let child_bottom = child_node
        .border_bottom
        .clone()
        .unwrap_or_else(|| child.to_string());
    let this_weight = if child_node.border_top.is_some() {
        context.weight
    } else {
        2.0 * context.weight
    };
    let minlen = if child_top != child_bottom {
        1
    } else {
        context.height - context.depths[v] + 1
    };
    state.graph.set_edge(
        top,
        &child_top,
        nesting_edge(this_weight, minlen, true),
        None,
    );
    state.graph.set_edge(
        &child_bottom,
        bottom,
        nesting_edge(this_weight, minlen, true),
        None,
    );
}

pub(super) fn nesting_graph_cleanup(state: &mut LayoutState) {
    if let Some(root) = state.nesting_root.take() {
        state.graph.remove_node(&root);
    }
    for edge in state.graph.edges() {
        if state.graph.edge_by_key(&edge).is_nesting_edge {
            state.graph.remove_edge(&edge);
        }
    }
}

/// 순위가 있는 노드(정수·소수 모두)의 최솟값
fn min_rank(state: &LayoutState) -> f64 {
    state
        .graph
        .nodes()
        .iter()
        .filter_map(|v| {
            let node = state.graph.node(v);
            node.rank.map(f64::from).or(node.fractional_rank)
        })
        .fold(f64::INFINITY, f64::min)
}

pub(super) fn remove_empty_ranks(state: &mut LayoutState) {
    let offset = min_rank(state) as i32;
    let mut layers: Vec<Option<Vec<String>>> = Vec::new();
    for v in state.graph.nodes() {
        let Some(rank) = state.graph.node(&v).rank else {
            continue;
        };
        let index = usize::try_from(rank - offset).unwrap_or(0);
        if layers.len() <= index {
            layers.resize(index + 1, None);
        }
        layers[index].get_or_insert_with(Vec::new).push(v);
    }
    let mut delta = 0;
    for (index, layer) in layers.iter().enumerate() {
        let index = i32::try_from(index).unwrap_or(0);
        match layer {
            None if index % state.node_rank_factor != 0 => delta -= 1,
            Some(nodes) if delta != 0 => shift_ranks(state, nodes, delta),
            _ => {}
        }
    }
}

fn shift_ranks(state: &mut LayoutState, nodes: &[String], delta: i32) {
    for v in nodes {
        *state.graph.node_mut(v).rank.as_mut().expect("rank") += delta;
    }
}

pub(super) fn normalize_ranks(state: &mut LayoutState) {
    let min = min_rank(state);
    for v in state.graph.nodes() {
        let node = state.graph.node_mut(&v);
        if let Some(rank) = node.rank.as_mut() {
            *rank -= min as i32;
        }
        if let Some(rank) = node.fractional_rank.as_mut() {
            *rank -= min;
        }
    }
}

pub(super) fn normalize_run(state: &mut LayoutState) {
    state.dummy_chains.clear();
    for edge in state.graph.edges() {
        normalize_edge(state, &edge);
    }
}

fn normalize_edge(state: &mut LayoutState, edge: &EdgeKey) {
    let mut v = edge.v.clone();
    let mut v_rank = state.graph.node(&v).rank.unwrap_or(0);
    let w_rank = state.graph.node(&edge.w).rank.unwrap_or(0);
    if w_rank == v_rank + 1 {
        return;
    }
    let mut edge_label = state.graph.edge_by_key(edge).clone();
    state.graph.remove_edge(edge);
    edge_label.points.clear();
    let weight = edge_label.weight;
    let mut index = 0;
    v_rank += 1;
    while v_rank < w_rank {
        let mut attrs = NodeLabel {
            rank: Some(v_rank),
            edge_obj: Some(edge.clone()),
            ..NodeLabel::default()
        };
        let mut kind = DummyKind::Edge;
        if edge_label.label_rank == Some(f64::from(v_rank)) {
            attrs.width = edge_label.width;
            attrs.height = edge_label.height;
            attrs.labelpos = edge_label.labelpos;
            kind = DummyKind::EdgeLabel;
        }
        if index == 0 {
            attrs.edge_label = Some(Box::new(edge_label.clone()));
        }
        let dummy = state.add_dummy_node(kind, attrs, "_d");
        state
            .graph
            .set_edge(&v, &dummy, weight_edge(weight), edge.name.as_deref());
        if index == 0 {
            state.dummy_chains.push(dummy.clone());
        }
        v = dummy;
        index += 1;
        v_rank += 1;
    }
    state
        .graph
        .set_edge(&v, &edge.w, weight_edge(weight), edge.name.as_deref());
}

fn weight_edge(weight: f64) -> EdgeLabel {
    EdgeLabel {
        weight,
        ..EdgeLabel::default()
    }
}

pub(super) fn normalize_undo(state: &mut LayoutState) {
    for head in state.dummy_chains.clone() {
        let head_node = state.graph.node(&head).clone();
        let mut label = *head_node
            .edge_label
            .clone()
            .expect("chain head keeps the edge label");
        let edge_obj = head_node.edge_obj.clone().expect("edge obj");
        let mut v = head;
        loop {
            let node = state.graph.node(&v).clone();
            if node.dummy.is_none() {
                break;
            }
            let next = state.graph.successors(&v)[0].clone();
            state.graph.remove_node(&v);
            label.points.push(Point {
                x: node.x,
                y: node.y,
            });
            if node.dummy == Some(DummyKind::EdgeLabel) {
                label.x = Some(node.x);
                label.y = Some(node.y);
                label.width = node.width;
                label.height = node.height;
            }
            v = next;
        }
        state
            .graph
            .set_edge(&edge_obj.v, &edge_obj.w, label, edge_obj.name.as_deref());
    }
}

#[derive(Clone, Copy)]
struct PostorderNumber {
    low: usize,
    lim: usize,
}

fn postorder_numbers(state: &LayoutState) -> HashMap<String, PostorderNumber> {
    let mut result = HashMap::new();
    let mut lim = 0;
    for v in state.graph.children(None) {
        postorder_number_dfs(state, &v, &mut lim, &mut result);
    }
    result
}

fn postorder_number_dfs(
    state: &LayoutState,
    v: &str,
    lim: &mut usize,
    result: &mut HashMap<String, PostorderNumber>,
) {
    let low = *lim;
    for child in state.graph.children(Some(v)) {
        postorder_number_dfs(state, &child, lim, result);
    }
    result.insert(v.to_string(), PostorderNumber { low, lim: *lim });
    *lim += 1;
}

/// 두 노드 사이 부모 경로와 최소 공통 조상. `None`은 그래프 뿌리다
fn find_path(
    state: &LayoutState,
    numbers: &HashMap<String, PostorderNumber>,
    v: &str,
    w: &str,
) -> (Vec<Option<String>>, Option<String>) {
    let low = numbers[v].low.min(numbers[w].low);
    let lim = numbers[v].lim.max(numbers[w].lim);
    let mut v_path = Vec::new();
    let mut parent = Some(v.to_string());
    loop {
        parent = parent.and_then(|current| state.graph.parent(&current));
        v_path.push(parent.clone());
        let should_continue = parent.as_ref().is_some_and(|current| {
            let number = numbers[current];
            number.low > low || lim > number.lim
        });
        if !should_continue {
            break;
        }
    }
    let lca = parent;
    let mut w_path = Vec::new();
    let mut current = state.graph.parent(w);
    while current != lca {
        w_path.push(current.clone());
        current = current.and_then(|node| state.graph.parent(&node));
    }
    w_path.reverse();
    v_path.extend(w_path);
    (v_path, lca)
}

pub(super) fn parent_dummy_chains(state: &mut LayoutState) {
    let numbers = postorder_numbers(state);
    for head in state.dummy_chains.clone() {
        let edge_obj = state.graph.node(&head).edge_obj.clone().expect("edge obj");
        let (path, lca) = find_path(state, &numbers, &edge_obj.v, &edge_obj.w);
        assign_chain_parents(state, &head, &edge_obj.w, &path, &lca);
    }
}

fn rank_of(state: &LayoutState, v: &Option<String>, pick_max: bool) -> Option<i32> {
    let node = state.graph.node(v.as_deref()?);
    if pick_max {
        node.max_rank
    } else {
        node.min_rank
    }
}

fn assign_chain_parents(
    state: &mut LayoutState,
    head: &str,
    end: &str,
    path: &[Option<String>],
    lca: &Option<String>,
) {
    let mut v = head.to_string();
    let mut path_index = 0;
    let mut is_ascending = true;
    while v != end {
        let rank = state.graph.node(&v).rank.unwrap_or(0);
        let mut path_v = path[path_index].clone();
        if is_ascending {
            while path[path_index] != *lca
                && rank_of(state, &path[path_index], true).is_some_and(|max| max < rank)
            {
                path_index += 1;
            }
            path_v = path[path_index].clone();
            if path_v == *lca {
                is_ascending = false;
            }
        }
        if !is_ascending {
            while path_index < path.len() - 1
                && rank_of(state, &path[path_index + 1], false).is_some_and(|min| min <= rank)
            {
                path_index += 1;
            }
            path_v = path[path_index].clone();
        }
        state.graph.set_parent(&v, path_v.as_deref());
        v = state.graph.successors(&v)[0].clone();
    }
}

pub(super) fn add_border_segments(state: &mut LayoutState) {
    for v in state.graph.children(None) {
        border_dfs(state, &v);
    }
}

fn border_dfs(state: &mut LayoutState, v: &str) {
    for child in state.graph.children(Some(v)) {
        border_dfs(state, &child);
    }
    let node = state.graph.node(v);
    let (Some(min_rank), Some(max_rank)) = (node.min_rank, node.max_rank) else {
        return;
    };
    for rank in min_rank..=max_rank {
        add_border_node(state, BorderSide::Left, "_bl", v, rank);
        add_border_node(state, BorderSide::Right, "_br", v, rank);
    }
}

fn add_border_node(
    state: &mut LayoutState,
    side: BorderSide,
    prefix: &str,
    subgraph: &str,
    rank: i32,
) {
    let label = NodeLabel {
        rank: Some(rank),
        border_type: Some(side),
        ..NodeLabel::default()
    };
    let borders = |node: &NodeLabel| match side {
        BorderSide::Left => node.border_left.clone(),
        BorderSide::Right => node.border_right.clone(),
    };
    let previous = borders(state.graph.node(subgraph))
        .get(&(rank - 1))
        .cloned();
    let current = state.add_dummy_node(DummyKind::Border, label, prefix);
    let subgraph_node = state.graph.node_mut(subgraph);
    match side {
        BorderSide::Left => subgraph_node.border_left.insert(rank, current.clone()),
        BorderSide::Right => subgraph_node.border_right.insert(rank, current.clone()),
    };
    state.graph.set_parent(&current, Some(subgraph));
    if let Some(previous) = previous {
        state
            .graph
            .set_edge(&previous, &current, weight_edge(1.0), None);
    }
}
