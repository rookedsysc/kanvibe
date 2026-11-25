//! dagre `order/`: 층마다 노드 순서를 바꿔 간선 교차를 줄인다(barycenter, 제약 그래프, 하위 그래프 정렬).

use std::collections::HashMap;

use super::LayoutState;
use crate::graph::{Graph, js_key_order};

/// 층 그래프 노드. 복합 노드만 이 층의 좌·우 경계를 가진다(order는 원래 그래프 라벨에서 읽는다)
#[derive(Clone, Debug, Default)]
struct LayerNode {
    border_left: Option<String>,
    border_right: Option<String>,
}

type LayerGraph = Graph<LayerNode, f64>;
type ConstraintGraph = Graph<(), ()>;

struct BuiltLayer {
    graph: LayerGraph,
    root: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Relationship {
    InEdges,
    OutEdges,
}

pub(super) fn order(state: &mut LayoutState) {
    let max_rank = state.max_rank();
    let down: Vec<BuiltLayer> = (1..=max_rank)
        .map(|rank| build_layer_graph(state, rank, Relationship::InEdges))
        .collect();
    let up: Vec<BuiltLayer> = (0..max_rank)
        .rev()
        .map(|rank| build_layer_graph(state, rank, Relationship::OutEdges))
        .collect();
    let layering = init_order(state);
    assign_order(state, &layering);
    let mut best_cross_count = f64::INFINITY;
    let mut best = layering;
    let mut index = 0;
    let mut last_best = 0;
    while last_best < 4 {
        let layers = if index % 2 == 1 { &down } else { &up };
        sweep_layer_graphs(state, layers, index % 4 >= 2);
        let layering = state.build_layer_matrix();
        let cross_count = cross_count(state, &layering);
        if cross_count < best_cross_count {
            last_best = 0;
            best = layering;
            best_cross_count = cross_count;
        }
        index += 1;
        last_best += 1;
    }
    assign_order(state, &best);
}

fn assign_order(state: &mut LayoutState, layering: &[Vec<String>]) {
    for layer in layering {
        for (index, v) in layer.iter().enumerate() {
            state.graph.node_mut(v).order = Some(index);
        }
    }
}

fn init_order(state: &LayoutState) -> Vec<Vec<String>> {
    let simple_nodes: Vec<String> = state
        .graph
        .nodes()
        .into_iter()
        .filter(|v| state.graph.children(Some(v)).is_empty())
        .collect();
    let max_rank = simple_nodes
        .iter()
        .filter_map(|v| state.graph.node(v).rank)
        .max()
        .unwrap_or(0);
    let mut layers: Vec<Vec<String>> = vec![Vec::new(); usize::try_from(max_rank).unwrap_or(0) + 1];
    let mut visited: HashMap<String, bool> = HashMap::new();
    let mut ordered = simple_nodes;
    ordered.sort_by_key(|v| state.graph.node(v).rank.unwrap_or(0));
    for v in ordered {
        init_order_dfs(state, &v, &mut visited, &mut layers);
    }
    layers
}

fn init_order_dfs(
    state: &LayoutState,
    v: &str,
    visited: &mut HashMap<String, bool>,
    layers: &mut [Vec<String>],
) {
    if visited.contains_key(v) {
        return;
    }
    visited.insert(v.to_string(), true);
    let rank = state.graph.node(v).rank.unwrap_or(0);
    layers[usize::try_from(rank).unwrap_or(0)].push(v.to_string());
    for successor in state.graph.successors(v) {
        init_order_dfs(state, &successor, visited, layers);
    }
}

fn build_layer_graph(state: &mut LayoutState, rank: i32, relationship: Relationship) -> BuiltLayer {
    let root = state.unique_id("_root");
    let mut graph = LayerGraph::new(true, true);
    for v in state.graph.nodes() {
        let node = state.graph.node(&v);
        let is_in_rank = node.rank == Some(rank)
            || (node.min_rank.is_some_and(|min| min <= rank)
                && node.max_rank.is_some_and(|max| rank <= max));
        if !is_in_rank {
            continue;
        }
        graph.ensure_node(&v);
        let parent = state.graph.parent(&v).unwrap_or_else(|| root.clone());
        graph.set_parent(&v, Some(&parent));
        let edges = match relationship {
            Relationship::InEdges => state.graph.in_edges(&v),
            Relationship::OutEdges => state.graph.out_edges(&v),
        };
        for edge in edges {
            let u = if edge.v == v { &edge.w } else { &edge.v };
            let previous = graph.edge(u, &v, None).copied().unwrap_or(0.0);
            graph.set_edge(
                u,
                &v,
                state.graph.edge_by_key(&edge).weight + previous,
                None,
            );
        }
        if node.min_rank.is_some() {
            let label = LayerNode {
                border_left: node.border_left.get(&rank).cloned(),
                border_right: node.border_right.get(&rank).cloned(),
            };
            graph.set_node(&v, label);
        }
    }
    BuiltLayer { graph, root }
}

fn sweep_layer_graphs(state: &mut LayoutState, layers: &[BuiltLayer], is_bias_right: bool) {
    let mut constraints = ConstraintGraph::new(true, false);
    for layer in layers {
        let sorted = sort_subgraph(
            state,
            &layer.graph,
            &layer.root,
            &constraints,
            is_bias_right,
        );
        for (index, v) in sorted.vs.iter().enumerate() {
            state.graph.node_mut(v).order = Some(index);
        }
        add_subgraph_constraints(state, &layer.graph, &mut constraints, &sorted.vs);
    }
}

#[derive(Clone, Debug, Default)]
struct SortResult {
    vs: Vec<String>,
    barycenter: Option<f64>,
    weight: f64,
}

#[derive(Clone, Debug)]
struct BarycenterEntry {
    v: String,
    barycenter: Option<f64>,
    weight: f64,
}

fn order_of(state: &LayoutState, v: &str) -> f64 {
    state.graph.node(v).order.unwrap_or(0) as f64
}

fn barycenters(
    state: &LayoutState,
    graph: &LayerGraph,
    movable: &[String],
) -> Vec<BarycenterEntry> {
    movable
        .iter()
        .map(|v| {
            let in_edges = graph.in_edges(v);
            if in_edges.is_empty() {
                return BarycenterEntry {
                    v: v.clone(),
                    barycenter: None,
                    weight: 0.0,
                };
            }
            let mut sum = 0.0;
            let mut weight = 0.0;
            for edge in in_edges {
                let edge_weight = *graph.edge_by_key(&edge);
                sum += edge_weight * order_of(state, &edge.v);
                weight += edge_weight;
            }
            BarycenterEntry {
                v: v.clone(),
                barycenter: Some(sum / weight),
                weight,
            }
        })
        .collect()
}

fn sort_subgraph(
    state: &LayoutState,
    graph: &LayerGraph,
    v: &str,
    constraints: &ConstraintGraph,
    is_bias_right: bool,
) -> SortResult {
    let node = graph.has_node(v).then(|| graph.node(v).clone());
    let border_left = node.as_ref().and_then(|node| node.border_left.clone());
    let border_right = node.as_ref().and_then(|node| node.border_right.clone());
    let mut movable = graph.children(Some(v));
    if border_left.is_some() {
        movable.retain(|w| Some(w) != border_left.as_ref() && Some(w) != border_right.as_ref());
    }
    let mut entries = barycenters(state, graph, &movable);
    let mut subgraphs: HashMap<String, SortResult> = HashMap::new();
    for entry in &mut entries {
        if graph.children(Some(&entry.v)).is_empty() {
            continue;
        }
        let result = sort_subgraph(state, graph, &entry.v, constraints, is_bias_right);
        if let Some(barycenter) = result.barycenter {
            merge_barycenters(entry, barycenter, result.weight);
        }
        subgraphs.insert(entry.v.clone(), result);
    }
    let mut resolved = resolve_conflicts(&entries, constraints);
    expand_subgraphs(&mut resolved, &subgraphs);
    let mut result = sort_entries(resolved, is_bias_right);
    if let (Some(left), Some(right)) = (border_left, border_right) {
        let mut vs = vec![left.clone()];
        vs.append(&mut result.vs);
        vs.push(right.clone());
        result.vs = vs;
        if let Some(left_pred) = graph.predecessors(&left).first() {
            let right_pred = graph.predecessors(&right)[0].clone();
            let (barycenter, weight) = (result.barycenter.unwrap_or(0.0), result.weight);
            let total =
                barycenter * weight + order_of(state, left_pred) + order_of(state, &right_pred);
            result.barycenter = Some(total / (weight + 2.0));
            result.weight = weight + 2.0;
        }
    }
    result
}

fn merge_barycenters(target: &mut BarycenterEntry, barycenter: f64, weight: f64) {
    match target.barycenter {
        Some(current) => {
            target.barycenter =
                Some((current * target.weight + barycenter * weight) / (target.weight + weight));
            target.weight += weight;
        }
        None => {
            target.barycenter = Some(barycenter);
            target.weight = weight;
        }
    }
}

#[derive(Clone, Debug)]
struct ConflictEntry {
    indegree: usize,
    incoming: Vec<usize>,
    outgoing: Vec<usize>,
    vs: Vec<String>,
    index: usize,
    barycenter: Option<f64>,
    weight: f64,
    is_merged: bool,
}

#[derive(Clone, Debug)]
struct ResolvedEntry {
    vs: Vec<String>,
    index: usize,
    barycenter: Option<f64>,
    weight: f64,
}

fn resolve_conflicts(
    entries: &[BarycenterEntry],
    constraints: &ConstraintGraph,
) -> Vec<ResolvedEntry> {
    let mut mapped: Vec<ConflictEntry> = Vec::new();
    let mut position: HashMap<String, usize> = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        position.insert(entry.v.clone(), mapped.len());
        mapped.push(ConflictEntry {
            indegree: 0,
            incoming: Vec::new(),
            outgoing: Vec::new(),
            vs: vec![entry.v.clone()],
            index,
            barycenter: entry.barycenter,
            weight: if entry.barycenter.is_some() {
                entry.weight
            } else {
                0.0
            },
            is_merged: false,
        });
    }
    for edge in constraints.edges() {
        if let (Some(&v), Some(&w)) = (position.get(&edge.v), position.get(&edge.w)) {
            mapped[w].indegree += 1;
            mapped[v].outgoing.push(w);
        }
    }
    // `_.filter(mappedEntries)`는 객체 키(노드 이름) 순서로 돈다
    let keys: Vec<String> = entries.iter().map(|entry| entry.v.clone()).collect();
    let source_set: Vec<usize> = js_key_order(keys.iter())
        .iter()
        .map(|key| position[key])
        .filter(|index| mapped[*index].indegree == 0)
        .collect();
    do_resolve_conflicts(mapped, source_set)
}

fn do_resolve_conflicts(
    mut mapped: Vec<ConflictEntry>,
    mut source_set: Vec<usize>,
) -> Vec<ResolvedEntry> {
    let mut visited_order: Vec<usize> = Vec::new();
    while let Some(entry) = source_set.pop() {
        visited_order.push(entry);
        let mut incoming = mapped[entry].incoming.clone();
        incoming.reverse();
        mapped[entry].incoming.reverse();
        for u in incoming {
            if mapped[u].is_merged {
                continue;
            }
            let should_merge = match (mapped[u].barycenter, mapped[entry].barycenter) {
                (Some(u_value), Some(v_value)) => u_value >= v_value,
                _ => true,
            };
            if should_merge {
                merge_entries(&mut mapped, entry, u);
            }
        }
        for w in mapped[entry].outgoing.clone() {
            mapped[w].incoming.push(entry);
            mapped[w].indegree -= 1;
            if mapped[w].indegree == 0 {
                source_set.push(w);
            }
        }
    }
    visited_order
        .into_iter()
        .filter(|index| !mapped[*index].is_merged)
        .map(|index| {
            let entry = &mapped[index];
            ResolvedEntry {
                vs: entry.vs.clone(),
                index: entry.index,
                barycenter: entry.barycenter,
                weight: entry.weight,
            }
        })
        .collect()
}

fn merge_entries(mapped: &mut [ConflictEntry], target: usize, source: usize) {
    let mut sum = 0.0;
    let mut weight = 0.0;
    if mapped[target].weight != 0.0 {
        sum += mapped[target].barycenter.unwrap_or(0.0) * mapped[target].weight;
        weight += mapped[target].weight;
    }
    if mapped[source].weight != 0.0 {
        sum += mapped[source].barycenter.unwrap_or(0.0) * mapped[source].weight;
        weight += mapped[source].weight;
    }
    let mut vs = mapped[source].vs.clone();
    vs.extend(mapped[target].vs.clone());
    mapped[target].vs = vs;
    mapped[target].barycenter = Some(sum / weight);
    mapped[target].weight = weight;
    mapped[target].index = mapped[source].index.min(mapped[target].index);
    mapped[source].is_merged = true;
}

fn expand_subgraphs(entries: &mut [ResolvedEntry], subgraphs: &HashMap<String, SortResult>) {
    for entry in entries {
        entry.vs = entry
            .vs
            .iter()
            .flat_map(|v| {
                subgraphs
                    .get(v)
                    .map_or_else(|| vec![v.clone()], |result| result.vs.clone())
            })
            .collect();
    }
}

fn sort_entries(entries: Vec<ResolvedEntry>, is_bias_right: bool) -> SortResult {
    let (mut sortable, mut unsortable): (Vec<ResolvedEntry>, Vec<ResolvedEntry>) = entries
        .into_iter()
        .partition(|entry| entry.barycenter.is_some());
    // NaN 바리센터(가중치 0끼리 합친 항목)도 원본처럼 정렬 가능한 쪽에 남는다
    unsortable.sort_by_key(|entry| std::cmp::Reverse(entry.index));
    sortable.sort_by(|left, right| compare_with_bias(left, right, is_bias_right));
    let mut vs: Vec<String> = Vec::new();
    let mut sum = 0.0;
    let mut weight = 0.0;
    let mut vs_index = consume_unsortable(&mut vs, &mut unsortable, 0);
    for entry in sortable {
        vs_index += entry.vs.len();
        vs.extend(entry.vs.clone());
        sum += entry.barycenter.unwrap_or(0.0) * entry.weight;
        weight += entry.weight;
        vs_index = consume_unsortable(&mut vs, &mut unsortable, vs_index);
    }
    let barycenter = (weight != 0.0).then(|| sum / weight);
    SortResult {
        vs,
        barycenter,
        weight: if weight != 0.0 { weight } else { 0.0 },
    }
}

fn consume_unsortable(
    vs: &mut Vec<String>,
    unsortable: &mut Vec<ResolvedEntry>,
    mut index: usize,
) -> usize {
    while unsortable.last().is_some_and(|last| last.index <= index) {
        let last = unsortable.pop().expect("checked");
        vs.extend(last.vs);
        index += 1;
    }
    index
}

/// JS 비교 함수처럼 NaN은 어느 쪽으로도 작지 않아 index로만 가린다
fn compare_with_bias(
    left: &ResolvedEntry,
    right: &ResolvedEntry,
    is_bias_right: bool,
) -> std::cmp::Ordering {
    let (a, b) = (
        left.barycenter.unwrap_or(0.0),
        right.barycenter.unwrap_or(0.0),
    );
    if a < b {
        return std::cmp::Ordering::Less;
    }
    if a > b {
        return std::cmp::Ordering::Greater;
    }
    if is_bias_right {
        right.index.cmp(&left.index)
    } else {
        left.index.cmp(&right.index)
    }
}

fn add_subgraph_constraints(
    state: &LayoutState,
    graph: &LayerGraph,
    constraints: &mut ConstraintGraph,
    vs: &[String],
) {
    let mut previous: HashMap<String, String> = HashMap::new();
    let mut root_previous: Option<String> = None;
    for v in vs {
        let mut child = graph.parent(v);
        while let Some(current) = child {
            let parent = graph.parent(&current);
            let previous_child = match &parent {
                Some(parent) => previous.insert(parent.clone(), current.clone()),
                None => root_previous.replace(current.clone()),
            };
            if let Some(previous_child) = previous_child
                && previous_child != current
            {
                constraints.set_edge(&previous_child, &current, (), None);
                break;
            }
            child = parent;
        }
    }
    constraint_dfs(state, graph, constraints, None);
}

fn constraint_dfs(
    state: &LayoutState,
    graph: &LayerGraph,
    constraints: &mut ConstraintGraph,
    v: Option<&str>,
) -> f64 {
    let children = graph.children(v);
    if children.is_empty() {
        return v.map_or(0.0, |v| order_of(state, v));
    }
    let mut min = f64::INFINITY;
    let mut subgraphs: Vec<(String, f64)> = Vec::new();
    for child in children {
        let child_min = constraint_dfs(state, graph, constraints, Some(&child));
        if !graph.children(Some(&child)).is_empty() {
            subgraphs.push((child, child_min));
        }
        min = min.min(child_min);
    }
    subgraphs.sort_by(|left, right| {
        left.1
            .partial_cmp(&right.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for pair in subgraphs.windows(2) {
        constraints.set_edge(&pair[0].0, &pair[1].0, (), None);
    }
    min
}

fn cross_count(state: &LayoutState, layering: &[Vec<String>]) -> f64 {
    layering
        .windows(2)
        .map(|pair| two_layer_cross_count(state, &pair[0], &pair[1]))
        .sum()
}

fn two_layer_cross_count(state: &LayoutState, north: &[String], south: &[String]) -> f64 {
    let south_position: HashMap<&String, usize> = south
        .iter()
        .enumerate()
        .map(|(index, v)| (v, index))
        .collect();
    let mut entries: Vec<(usize, f64)> = Vec::new();
    for v in north {
        let mut row: Vec<(usize, f64)> = state
            .graph
            .out_edges(v)
            .iter()
            .map(|edge| {
                (
                    south_position.get(&edge.w).copied().unwrap_or(0),
                    state.graph.edge_by_key(edge).weight,
                )
            })
            .collect();
        row.sort_by_key(|(position, _)| *position);
        entries.extend(row);
    }
    let mut first_index = 1;
    while first_index < south.len() {
        first_index <<= 1;
    }
    let tree_size = 2 * first_index - 1;
    first_index -= 1;
    let mut tree = vec![0.0; tree_size];
    let mut count = 0.0;
    for (position, weight) in entries {
        let mut index = position + first_index;
        tree[index] += weight;
        let mut weight_sum = 0.0;
        while index > 0 {
            if index % 2 == 1 {
                weight_sum += tree[index + 1];
            }
            index = (index - 1) >> 1;
            tree[index] += weight;
        }
        count += weight * weight_sum;
    }
    count
}
