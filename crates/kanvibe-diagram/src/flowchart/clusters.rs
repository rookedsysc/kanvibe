//! mermaid-graphlib.js: subgraph를 레이아웃 전에 정리한다.
//!
//! 바깥과 이어진 간선이 없는 subgraph는 따로 떼어 내 자기 방향(부모가 TB면 LR, 아니면 TB)으로 먼저 배치하고
//! 부모에서는 그 크기의 노드 하나로 다룬다. 바깥과 이어진 subgraph는 dagre 복합 노드로 남기고,
//! subgraph 자체를 가리키는 간선은 안쪽 노드(anchor)로 옮긴 뒤 그릴 때 subgraph 경계에서 자른다.

use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;

use super::FlowNode;
use super::geometry::BoundaryBox;
use super::layout::{Level, LevelEdge, LevelNode};
use super::parser::Direction;
use crate::graph::{EdgeKey, Graph};

#[derive(Clone, Debug)]
pub struct ClusterEntry {
    pub anchor: Option<String>,
    pub has_external_connections: bool,
    pub cluster_data: Option<Rc<FlowNode>>,
}

/// 원본의 모듈 전역 Map(descendants, clusterDb). 한 다이어그램을 그리는 동안 모든 단계가 공유한다
#[derive(Default)]
pub struct ClusterContext {
    pub descendants: HashMap<String, Vec<String>>,
    pub cluster_db: IndexMap<String, ClusterEntry>,
    /// 그린 뒤의 클러스터 상자(insertCluster가 clusterDb에 남기는 node)
    pub cluster_boxes: HashMap<String, BoundaryBox>,
}

type LevelGraph = Graph<LevelNode, LevelEdge>;

impl ClusterContext {
    fn is_descendant(&self, id: &str, ancestor: &str) -> bool {
        self.descendants
            .get(ancestor)
            .is_some_and(|list| list.iter().any(|item| item == id))
    }

    fn edge_in_cluster(&self, edge: &EdgeKey, cluster: &str) -> bool {
        if edge.v == cluster || edge.w == cluster {
            return false;
        }
        self.is_descendant(&edge.v, cluster) || self.is_descendant(&edge.w, cluster)
    }

    fn anchor_id(&self, id: &str) -> String {
        match self.cluster_db.get(id) {
            Some(entry) if entry.has_external_connections => {
                entry.anchor.clone().unwrap_or_else(|| id.to_string())
            }
            _ => id.to_string(),
        }
    }
}

fn extract_descendants(graph: &LevelGraph, id: &str) -> Vec<String> {
    let children = graph.children(Some(id));
    let mut result = children.clone();
    for child in children {
        result.extend(extract_descendants(graph, &child));
    }
    result
}

fn find_common_edges(graph: &LevelGraph, cluster: &str, candidate: &str) -> usize {
    let edges = graph.edges();
    let first: Vec<(String, String)> = edges
        .iter()
        .filter(|edge| edge.v == cluster || edge.w == cluster)
        .map(|edge| {
            let v = if edge.v == cluster {
                candidate.to_string()
            } else {
                edge.v.clone()
            };
            let w = if edge.w == cluster {
                cluster.to_string()
            } else {
                edge.w.clone()
            };
            (v, w)
        })
        .collect();
    let second: Vec<(&String, &String)> = edges
        .iter()
        .filter(|edge| edge.v == candidate || edge.w == candidate)
        .map(|edge| (&edge.v, &edge.w))
        .collect();
    first
        .iter()
        .filter(|(v, w)| {
            second
                .iter()
                .any(|(other_v, other_w)| *other_v == v && *other_w == w)
        })
        .count()
}

pub fn find_non_cluster_child(graph: &LevelGraph, id: &str, cluster: &str) -> Option<String> {
    let children = graph.children(Some(id));
    if children.is_empty() {
        return Some(id.to_string());
    }
    let mut reserve = None;
    for child in children {
        let Some(candidate) = find_non_cluster_child(graph, &child, cluster) else {
            continue;
        };
        if find_common_edges(graph, cluster, &candidate) > 0 {
            reserve = Some(candidate);
        } else {
            return Some(candidate);
        }
    }
    reserve
}

/// adjustClustersAndEdges
pub fn adjust_clusters_and_edges(level: &mut Level, context: &mut ClusterContext) {
    let graph = &mut level.graph;
    for id in graph.nodes() {
        if graph.children(Some(&id)).is_empty() {
            continue;
        }
        context
            .descendants
            .insert(id.clone(), extract_descendants(graph, &id));
        let anchor = find_non_cluster_child(graph, &id, &id);
        let cluster_data = graph.node(&id).node.clone();
        context.cluster_db.insert(
            id,
            ClusterEntry {
                anchor,
                has_external_connections: false,
                cluster_data,
            },
        );
    }
    mark_external_connections(graph, context);
    lift_anchors_into_isolated_parents(graph, context);
    reroute_cluster_edges(graph, context);
    extract_isolated_clusters(level, context, 0);
}

fn mark_external_connections(graph: &LevelGraph, context: &mut ClusterContext) {
    for id in graph.nodes() {
        if graph.children(Some(&id)).is_empty() {
            continue;
        }
        let has_external = graph
            .edges()
            .iter()
            .any(|edge| context.is_descendant(&edge.v, &id) ^ context.is_descendant(&edge.w, &id));
        if has_external && let Some(entry) = context.cluster_db.get_mut(&id) {
            entry.has_external_connections = true;
        }
    }
}

fn lift_anchors_into_isolated_parents(graph: &LevelGraph, context: &mut ClusterContext) {
    let ids: Vec<String> = context.cluster_db.keys().cloned().collect();
    for id in ids {
        let Some(anchor) = context.cluster_db[&id].anchor.clone() else {
            continue;
        };
        let Some(parent) = graph.parent(&anchor) else {
            continue;
        };
        let is_isolated_parent = context
            .cluster_db
            .get(&parent)
            .is_some_and(|entry| !entry.has_external_connections);
        if parent != id && is_isolated_parent {
            context.cluster_db.get_mut(&id).expect("cluster").anchor = Some(parent);
        }
    }
}

fn reroute_cluster_edges(graph: &mut LevelGraph, context: &mut ClusterContext) {
    for edge in graph.edges() {
        if !context.cluster_db.contains_key(&edge.v) && !context.cluster_db.contains_key(&edge.w) {
            continue;
        }
        let v = context.anchor_id(&edge.v);
        let w = context.anchor_id(&edge.w);
        let mut label = graph.edge_by_key(&edge).clone();
        graph.remove_edge(&edge);
        if v != edge.v {
            mark_parent_external(graph, context, &v);
            label.from_cluster = Some(edge.v.clone());
        }
        if w != edge.w {
            mark_parent_external(graph, context, &w);
            label.to_cluster = Some(edge.w.clone());
        }
        graph.set_edge(&v, &w, label, edge.name.as_deref());
    }
}

fn mark_parent_external(graph: &LevelGraph, context: &mut ClusterContext, id: &str) {
    if let Some(parent) = graph.parent(id)
        && let Some(entry) = context.cluster_db.get_mut(&parent)
    {
        entry.has_external_connections = true;
    }
}

/// extractor: 바깥과 이어지지 않은 subgraph를 따로 떼어 낸다
fn extract_isolated_clusters(level: &mut Level, context: &mut ClusterContext, depth: usize) {
    if depth > 10 {
        return;
    }
    let nodes = level.graph.nodes();
    if nodes
        .iter()
        .all(|node| level.graph.children(Some(node)).is_empty())
    {
        return;
    }
    for node in &nodes {
        let is_isolated = context
            .cluster_db
            .get(node)
            .is_some_and(|entry| !entry.has_external_connections);
        if !is_isolated || level.graph.children(Some(node)).is_empty() {
            continue;
        }
        let entry = context.cluster_db[node].clone();
        let mut dir = if level.rankdir == Direction::TopBottom {
            Direction::LeftRight
        } else {
            Direction::TopBottom
        };
        if let Some(cluster_dir) = entry.cluster_data.as_ref().and_then(|data| data.dir) {
            dir = cluster_dir;
        }
        let mut cluster_level = Level::new(dir);
        copy_cluster(
            node,
            &mut level.graph,
            &mut cluster_level.graph,
            node,
            context,
        );
        let cluster_node = LevelNode {
            cluster_data: entry.cluster_data.clone(),
            cluster_level: Some(Box::new(cluster_level)),
            ..LevelNode::default()
        };
        level.graph.set_node(node, cluster_node);
    }
    for node in level.graph.nodes() {
        if let Some(mut inner) = level.graph.node_mut(&node).cluster_level.take() {
            extract_isolated_clusters(&mut inner, context, depth + 1);
            level.graph.node_mut(&node).cluster_level = Some(inner);
        }
    }
}

fn copy_cluster(
    cluster: &str,
    graph: &mut LevelGraph,
    target: &mut LevelGraph,
    root: &str,
    context: &ClusterContext,
) {
    let mut nodes = graph.children(Some(cluster));
    if cluster != root {
        nodes.push(cluster.to_string());
    }
    for node in nodes {
        if !graph.children(Some(&node)).is_empty() {
            copy_cluster(&node, graph, target, root, context);
            continue;
        }
        target.set_node(&node, graph.node(&node).clone());
        let parent = graph.parent(&node);
        if parent.as_deref() != Some(root)
            && let Some(parent) = &parent
        {
            target.set_parent(&node, Some(parent));
        }
        if cluster != root && node != cluster {
            target.set_parent(&node, Some(cluster));
        }
        for edge in graph.edges() {
            if context.edge_in_cluster(&edge, root) {
                let label = graph.edge_by_key(&edge).clone();
                target.set_edge(&edge.v, &edge.w, label, edge.name.as_deref());
            }
        }
        graph.remove_node(&node);
    }
}
