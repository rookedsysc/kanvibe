//! dagre-d3-es graphlib `Graph`와 같은 순회 순서를 지키는 그래프.
//!
//! dagre의 결과는 노드·간선을 도는 순서에 따라 달라진다. graphlib은 JS 객체 키 순서를 그대로 쓰므로
//! 정수처럼 보이는 키("0", "12")는 숫자 순으로 먼저, 나머지 키는 넣은 순서로 돈다. 간선 id에는 `\x01`이
//! 들어가 항상 넣은 순서다. 지운 키를 다시 넣으면 맨 뒤로 간다(`shift_remove`).

use std::collections::HashMap;

use indexmap::IndexMap;

const GRAPH_ROOT: &str = "\u{0}";
const EDGE_KEY_DELIMITER: char = '\u{1}';
const DEFAULT_EDGE_NAME: &str = "\u{0}";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EdgeKey {
    pub v: String,
    pub w: String,
    pub name: Option<String>,
}

impl EdgeKey {
    pub fn new(v: &str, w: &str, name: Option<&str>) -> Self {
        Self {
            v: v.to_string(),
            w: w.to_string(),
            name: name.map(str::to_string),
        }
    }
}

/// JS 배열 인덱스로 읽히는 키인지(객체 키 순서에서 앞으로 오는 키)
fn array_index_key(key: &str) -> Option<u64> {
    let is_canonical = key == "0" || (!key.starts_with('0') && !key.is_empty());
    if !is_canonical || !key.bytes().all(|byte| byte.is_ascii_digit()) || key.len() > 10 {
        return None;
    }
    key.parse::<u64>()
        .ok()
        .filter(|value| *value < u64::from(u32::MAX))
}

/// JS `Object.keys` 순서로 키를 늘어놓는다
pub fn js_key_order<'a>(keys: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut numeric: Vec<(u64, String)> = Vec::new();
    let mut named: Vec<String> = Vec::new();
    for key in keys {
        match array_index_key(key) {
            Some(value) => numeric.push((value, key.clone())),
            None => named.push(key.clone()),
        }
    }
    numeric.sort_by_key(|(value, _)| *value);
    numeric
        .into_iter()
        .map(|(_, key)| key)
        .chain(named)
        .collect()
}

#[derive(Clone, Debug)]
pub struct Graph<N, E> {
    is_directed: bool,
    is_compound: bool,
    nodes: IndexMap<String, N>,
    parent: HashMap<String, String>,
    children: HashMap<String, IndexMap<String, ()>>,
    in_edges: HashMap<String, IndexMap<String, EdgeKey>>,
    out_edges: HashMap<String, IndexMap<String, EdgeKey>>,
    preds: HashMap<String, IndexMap<String, usize>>,
    sucs: HashMap<String, IndexMap<String, usize>>,
    edge_labels: IndexMap<String, (EdgeKey, E)>,
}

impl<N: Default, E> Graph<N, E> {
    pub fn new(is_directed: bool, is_compound: bool) -> Self {
        let mut children = HashMap::new();
        if is_compound {
            children.insert(GRAPH_ROOT.to_string(), IndexMap::new());
        }
        Self {
            is_directed,
            is_compound,
            nodes: IndexMap::new(),
            parent: HashMap::new(),
            children,
            in_edges: HashMap::new(),
            out_edges: HashMap::new(),
            preds: HashMap::new(),
            sucs: HashMap::new(),
            edge_labels: IndexMap::new(),
        }
    }

    fn edge_id(&self, v: &str, w: &str, name: Option<&str>) -> String {
        let (first, second) = if !self.is_directed && v > w {
            (w, v)
        } else {
            (v, w)
        };
        let name = name.unwrap_or(DEFAULT_EDGE_NAME);
        format!("{first}{EDGE_KEY_DELIMITER}{second}{EDGE_KEY_DELIMITER}{name}")
    }

    pub fn nodes(&self) -> Vec<String> {
        js_key_order(self.nodes.keys())
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn has_node(&self, v: &str) -> bool {
        self.nodes.contains_key(v)
    }

    pub fn node(&self, v: &str) -> &N {
        &self.nodes[v]
    }

    pub fn node_mut(&mut self, v: &str) -> &mut N {
        self.nodes.get_mut(v).expect("graph node must exist")
    }

    /// 없으면 기본 라벨로 만든다. 이미 있으면 라벨을 그대로 둔다
    pub fn ensure_node(&mut self, v: &str) {
        if !self.has_node(v) {
            self.set_node(v, N::default());
        }
    }

    pub fn set_node(&mut self, v: &str, label: N) {
        if let Some(existing) = self.nodes.get_mut(v) {
            *existing = label;
            return;
        }
        self.nodes.insert(v.to_string(), label);
        if self.is_compound {
            self.parent.insert(v.to_string(), GRAPH_ROOT.to_string());
            self.children.insert(v.to_string(), IndexMap::new());
            self.children
                .get_mut(GRAPH_ROOT)
                .expect("root")
                .insert(v.to_string(), ());
        }
        self.in_edges.insert(v.to_string(), IndexMap::new());
        self.out_edges.insert(v.to_string(), IndexMap::new());
        self.preds.insert(v.to_string(), IndexMap::new());
        self.sucs.insert(v.to_string(), IndexMap::new());
    }

    pub fn remove_node(&mut self, v: &str) {
        if self.nodes.shift_remove(v).is_none() {
            return;
        }
        if self.is_compound {
            self.detach_from_parent(v);
            self.parent.remove(v);
            for child in self.children(Some(v)) {
                self.set_parent(&child, None);
            }
            self.children.remove(v);
        }
        let incident: Vec<EdgeKey> = self.in_edges[v]
            .values()
            .chain(self.out_edges[v].values())
            .cloned()
            .collect();
        for key in incident {
            self.remove_edge(&key);
        }
        self.in_edges.remove(v);
        self.out_edges.remove(v);
        self.preds.remove(v);
        self.sucs.remove(v);
    }

    fn detach_from_parent(&mut self, v: &str) {
        if let Some(parent) = self.parent.get(v).cloned()
            && let Some(siblings) = self.children.get_mut(&parent)
        {
            siblings.shift_remove(v);
        }
    }

    pub fn set_parent(&mut self, v: &str, parent: Option<&str>) {
        let parent_key = match parent {
            Some(parent) => {
                self.ensure_node(parent);
                parent.to_string()
            }
            None => GRAPH_ROOT.to_string(),
        };
        self.ensure_node(v);
        self.detach_from_parent(v);
        self.parent.insert(v.to_string(), parent_key.clone());
        self.children
            .get_mut(&parent_key)
            .expect("parent children")
            .insert(v.to_string(), ());
    }

    pub fn parent(&self, v: &str) -> Option<String> {
        self.parent
            .get(v)
            .filter(|parent| parent.as_str() != GRAPH_ROOT)
            .cloned()
    }

    /// `None`이면 최상위 자식. 복합 그래프가 아니면 최상위는 전체 노드다
    pub fn children(&self, v: Option<&str>) -> Vec<String> {
        let key = v.unwrap_or(GRAPH_ROOT);
        if !self.is_compound {
            return if v.is_none() {
                self.nodes()
            } else {
                Vec::new()
            };
        }
        self.children
            .get(key)
            .map(|children| js_key_order(children.keys()))
            .unwrap_or_default()
    }

    pub fn predecessors(&self, v: &str) -> Vec<String> {
        self.preds
            .get(v)
            .map(|preds| js_key_order(preds.keys()))
            .unwrap_or_default()
    }

    pub fn successors(&self, v: &str) -> Vec<String> {
        self.sucs
            .get(v)
            .map(|sucs| js_key_order(sucs.keys()))
            .unwrap_or_default()
    }

    pub fn neighbors(&self, v: &str) -> Vec<String> {
        let mut result = self.predecessors(v);
        for successor in self.successors(v) {
            if !result.contains(&successor) {
                result.push(successor);
            }
        }
        result
    }

    pub fn sources(&self) -> Vec<String> {
        self.nodes()
            .into_iter()
            .filter(|v| self.in_edges[v].is_empty())
            .collect()
    }

    pub fn edges(&self) -> Vec<EdgeKey> {
        self.edge_labels
            .values()
            .map(|(key, _)| key.clone())
            .collect()
    }

    pub fn edge_count(&self) -> usize {
        self.edge_labels.len()
    }

    pub fn has_edge(&self, v: &str, w: &str, name: Option<&str>) -> bool {
        self.edge_labels.contains_key(&self.edge_id(v, w, name))
    }

    pub fn edge(&self, v: &str, w: &str, name: Option<&str>) -> Option<&E> {
        self.edge_labels
            .get(&self.edge_id(v, w, name))
            .map(|(_, label)| label)
    }

    pub fn edge_by_key(&self, key: &EdgeKey) -> &E {
        self.edge(&key.v, &key.w, key.name.as_deref())
            .expect("graph edge must exist")
    }

    pub fn edge_mut(&mut self, key: &EdgeKey) -> &mut E {
        let id = self.edge_id(&key.v, &key.w, key.name.as_deref());
        &mut self
            .edge_labels
            .get_mut(&id)
            .expect("graph edge must exist")
            .1
    }

    /// 같은 id가 있으면 라벨만 바꾸고 순서는 그대로 둔다
    pub fn set_edge(&mut self, v: &str, w: &str, label: E, name: Option<&str>) {
        let id = self.edge_id(v, w, name);
        if let Some(existing) = self.edge_labels.get_mut(&id) {
            existing.1 = label;
            return;
        }
        self.ensure_node(v);
        self.ensure_node(w);
        let (v, w) = if !self.is_directed && v > w {
            (w, v)
        } else {
            (v, w)
        };
        let key = EdgeKey::new(v, w, name);
        *self
            .preds
            .get_mut(w)
            .expect("preds")
            .entry(v.to_string())
            .or_insert(0) += 1;
        *self
            .sucs
            .get_mut(v)
            .expect("sucs")
            .entry(w.to_string())
            .or_insert(0) += 1;
        self.in_edges
            .get_mut(w)
            .expect("in")
            .insert(id.clone(), key.clone());
        self.out_edges
            .get_mut(v)
            .expect("out")
            .insert(id.clone(), key.clone());
        self.edge_labels.insert(id, (key, label));
    }

    pub fn remove_edge(&mut self, key: &EdgeKey) {
        let id = self.edge_id(&key.v, &key.w, key.name.as_deref());
        let Some((stored, _)) = self.edge_labels.shift_remove(&id) else {
            return;
        };
        decrement_entry(self.preds.get_mut(&stored.w), &stored.v);
        decrement_entry(self.sucs.get_mut(&stored.v), &stored.w);
        if let Some(in_edges) = self.in_edges.get_mut(&stored.w) {
            in_edges.shift_remove(&id);
        }
        if let Some(out_edges) = self.out_edges.get_mut(&stored.v) {
            out_edges.shift_remove(&id);
        }
    }

    pub fn in_edges(&self, v: &str) -> Vec<EdgeKey> {
        self.in_edges
            .get(v)
            .map(|edges| edges.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn out_edges(&self, v: &str) -> Vec<EdgeKey> {
        self.out_edges
            .get(v)
            .map(|edges| edges.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn node_edges(&self, v: &str) -> Vec<EdgeKey> {
        let mut edges = self.in_edges(v);
        edges.extend(self.out_edges(v));
        edges
    }
}

fn decrement_entry(map: Option<&mut IndexMap<String, usize>>, key: &str) {
    let Some(map) = map else {
        return;
    };
    let should_remove = match map.get_mut(key) {
        Some(count) => {
            *count -= 1;
            *count == 0
        }
        None => false,
    };
    if should_remove {
        map.shift_remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 정수_키는_숫자_순으로_먼저_돈다() {
        let mut graph: Graph<(), ()> = Graph::new(true, false);
        for name in ["b", "10", "a", "2"] {
            graph.set_node(name, ());
        }
        assert_eq!(graph.nodes(), ["2", "10", "b", "a"]);
    }

    #[test]
    fn 지운_노드를_다시_넣으면_맨_뒤로_간다() {
        let mut graph: Graph<(), ()> = Graph::new(true, true);
        graph.set_node("a", ());
        graph.set_node("b", ());
        graph.remove_node("a");
        graph.set_node("a", ());
        assert_eq!(graph.nodes(), ["b", "a"]);
        assert_eq!(graph.children(None), ["b", "a"]);
    }

    #[test]
    fn 방향_없는_간선은_양쪽_이름으로_찾는다() {
        let mut graph: Graph<(), u32> = Graph::new(false, false);
        graph.set_edge("b", "a", 3, None);
        assert_eq!(graph.edge("a", "b", None), Some(&3));
        assert_eq!(graph.neighbors("a"), ["b"]);
    }
}
