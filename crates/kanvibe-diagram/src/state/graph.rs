//! dataFetcher.ts: 문서 나무를 통합 렌더러가 받는 노드·간선 목록으로 편다.
//!
//! 노드 모양은 그 id를 처음 만났을 때의 문장으로 정해진다(뒤에 나온 `state X <<choice>>`는 이미 만든 노드를 바꾸지
//! 않는다). 노드 순서는 처음 넣은 순서이고, 같은 id를 다시 만나면 그 자리의 값을 고친다. 간선 id의 번호는 전이와
//! 메모가 함께 쓰는 카운터(graphItemCount)에서 나온다.

use std::collections::HashMap;

use super::parser::{NotePosition, StateItem, StateKind, Statement};
use crate::DiagramError;
use crate::flowchart::parser::Direction;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateShape {
    Start,
    End,
    Rect,
    RectWithTitle,
    Choice,
    Fork,
    Join,
    /// roundedWithTitle 클러스터(복합 상태)
    Composite,
    /// `--`로 나눈 동시 영역 클러스터
    Divider,
    Note,
    /// 메모를 감싸는 보이지 않는 클러스터
    NoteGroup,
}

/// 원본 nodeDb의 description: 처음엔 id 문자열이고 설명이 붙으면 줄 목록이 된다
#[derive(Clone, Debug)]
enum Description {
    Text(String),
    Lines(Vec<String>),
}

#[derive(Clone, Debug)]
pub struct GraphNode {
    pub id: String,
    pub shape: StateShape,
    description: Description,
    pub is_group: bool,
    /// 중첩 복합 상태의 statediagram-cluster-alt
    pub is_alt: bool,
    /// 복합 상태 안쪽 방향
    pub dir: Option<Direction>,
    pub parent: Option<String>,
    /// classDef·style 선언(적용 순서대로)
    pub styles: Vec<String>,
    /// 노드가 놓인 문서의 방향(fork·join 막대 방향)
    pub layout_dir: Direction,
}

impl GraphNode {
    /// 그릴 제목. 설명 목록이면 첫 줄이다
    pub fn label(&self) -> &str {
        if self.shape == StateShape::Divider {
            return "";
        }
        match &self.description {
            Description::Text(text) => text,
            Description::Lines(lines) => lines.first().map(String::as_str).unwrap_or(""),
        }
    }

    /// rectWithTitle의 제목 아래 설명 줄들
    pub fn descriptions(&self) -> &[String] {
        match &self.description {
            Description::Lines(lines) if lines.len() > 1 => &lines[1..],
            _ => &[],
        }
    }
}

#[derive(Clone, Debug)]
pub struct GraphEdge {
    pub id: String,
    pub start: String,
    pub end: String,
    pub label: String,
    pub is_note: bool,
}

pub struct StateGraph {
    pub direction: Direction,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// 최상위 문서에서 읽은 classDef와 상태별 클래스·style(원본은 최상위 문장만 extract한다)
#[derive(Default)]
struct StyleBook {
    class_styles: HashMap<String, Vec<String>>,
    state_classes: HashMap<String, Vec<String>>,
    state_styles: HashMap<String, Vec<String>>,
}

impl StyleBook {
    fn from_root(root: &[Statement]) -> Self {
        let mut book = Self::default();
        for statement in root {
            book.read(statement);
        }
        book
    }

    fn read(&mut self, statement: &Statement) {
        match statement {
            Statement::ClassDef { name, styles } => {
                self.class_styles
                    .entry(name.clone())
                    .or_default()
                    .extend(styles.iter().cloned());
            }
            Statement::ApplyClass { ids, class_name } => {
                for id in ids {
                    self.add_class(id, class_name);
                }
            }
            Statement::Style { ids, styles } => {
                for id in ids {
                    self.state_styles.insert(id.clone(), styles.clone());
                }
            }
            Statement::Relation { from, to, .. } => {
                for state in [from, to] {
                    state
                        .classes
                        .iter()
                        .for_each(|class_name| self.add_class(&state.id, class_name));
                }
            }
            _ => {}
        }
    }

    fn add_class(&mut self, id: &str, class_name: &str) {
        self.state_classes
            .entry(id.to_string())
            .or_default()
            .push(class_name.to_string());
    }

    fn declarations(&self, id: &str) -> Vec<String> {
        let mut declarations: Vec<String> = Vec::new();
        for class_name in self.state_classes.get(id).into_iter().flatten() {
            declarations.extend(
                self.class_styles
                    .get(class_name)
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
        }
        declarations.extend(self.state_styles.get(id).into_iter().flatten().cloned());
        declarations
    }
}

/// 문서에서 마지막 direction 문장(없으면 TB)
pub fn doc_direction(doc: &[Statement]) -> Direction {
    doc.iter()
        .rev()
        .find_map(|statement| match statement {
            Statement::Direction(direction) => Some(*direction),
            _ => None,
        })
        .unwrap_or(Direction::TopBottom)
}

pub fn build_graph(root: &[Statement]) -> Result<StateGraph, DiagramError> {
    let direction = doc_direction(root);
    let mut fetcher = Fetcher {
        styles: StyleBook::from_root(root),
        nodes: HashMap::new(),
        order: Vec::new(),
        edges: Vec::new(),
        item_count: 0,
    };
    fetcher.setup_doc(None, root, false, direction);
    let nodes: Vec<GraphNode> = fetcher
        .order
        .iter()
        .map(|id| fetcher.nodes[id].clone())
        .collect();
    if let Some(group) = nodes
        .iter()
        .find(|node| node.is_group && !node.descriptions().is_empty())
    {
        return Err(DiagramError::Syntax(format!(
            "복합 상태에는 설명을 하나만 붙일 수 있습니다: {}",
            group.id
        )));
    }
    Ok(StateGraph {
        direction,
        nodes,
        edges: fetcher.edges,
    })
}

struct Fetcher {
    styles: StyleBook,
    nodes: HashMap<String, GraphNode>,
    /// 노드 목록에 처음 넣은 순서
    order: Vec<String>,
    edges: Vec<GraphEdge>,
    item_count: usize,
}

impl Fetcher {
    fn setup_doc(
        &mut self,
        parent: Option<&str>,
        doc: &[Statement],
        is_alt: bool,
        doc_dir: Direction,
    ) {
        for statement in doc {
            match statement {
                Statement::State(state) => self.fetch(parent, state, is_alt, doc_dir),
                Statement::Relation { from, to, label } => {
                    self.fetch(parent, from, is_alt, doc_dir);
                    self.fetch(parent, to, is_alt, doc_dir);
                    self.edges.push(GraphEdge {
                        id: format!("edge{}", self.item_count),
                        start: from.id.clone(),
                        end: to.id.clone(),
                        label: label.clone(),
                        is_note: false,
                    });
                    self.item_count += 1;
                }
                _ => {}
            }
        }
    }

    fn fetch(&mut self, parent: Option<&str>, item: &StateItem, is_alt: bool, doc_dir: Direction) {
        let entry = self.entry(item, doc_dir);
        apply_description(entry, &item.description);
        if !entry.is_group
            && let Some(doc) = &item.doc
        {
            entry.is_group = true;
            entry.dir = Some(doc_direction(doc));
            entry.is_alt = is_alt;
            entry.shape = if item.kind == StateKind::Divider {
                StateShape::Divider
            } else {
                StateShape::Composite
            };
        }
        if let Some(parent) = parent {
            entry.parent = Some(parent.to_string());
        }
        let inner_dir = entry.dir.unwrap_or(doc_dir);
        if item.note.is_some() {
            self.add_note(item, doc_dir);
        }
        self.insert_in_order(&item.id);
        if let Some(doc) = &item.doc {
            self.setup_doc(Some(&item.id), doc, !is_alt, inner_dir);
        }
    }

    /// nodeDb: 처음 만난 문장으로 모양을 정해 만든다
    fn entry(&mut self, item: &StateItem, doc_dir: Direction) -> &mut GraphNode {
        let styles = self.styles.declarations(&item.id);
        self.nodes
            .entry(item.id.clone())
            .or_insert_with(|| GraphNode {
                id: item.id.clone(),
                shape: first_shape(item),
                description: Description::Text(item.id.clone()),
                is_group: false,
                is_alt: false,
                dir: None,
                parent: None,
                styles,
                layout_dir: doc_dir,
            })
    }

    fn insert_in_order(&mut self, id: &str) {
        if !self.order.iter().any(|existing| existing == id) {
            self.order.push(id.to_string());
        }
    }

    /// 메모: 보이지 않는 그룹 안에 메모 노드를 두고 상태와 점선 간선으로 잇는다
    fn add_note(&mut self, item: &StateItem, doc_dir: Direction) {
        let Some(note) = &item.note else {
            return;
        };
        let group_id = format!("{}----parent", item.id);
        let note_id = format!("{}----note-{}", item.id, self.item_count);
        self.item_count += 1;
        let template = GraphNode {
            id: String::new(),
            shape: StateShape::NoteGroup,
            description: Description::Text(note.text.clone()),
            is_group: true,
            is_alt: false,
            dir: None,
            parent: None,
            styles: Vec::new(),
            layout_dir: doc_dir,
        };
        let group = GraphNode {
            id: group_id.clone(),
            ..template.clone()
        };
        let note_node = GraphNode {
            id: note_id.clone(),
            shape: StateShape::Note,
            is_group: false,
            parent: Some(group_id.clone()),
            ..template
        };
        for node in [group, note_node] {
            let id = node.id.clone();
            self.nodes.insert(id.clone(), node);
            self.insert_in_order(&id);
        }
        let (start, end) = match note.position {
            NotePosition::LeftOf => (note_id, item.id.clone()),
            NotePosition::RightOf => (item.id.clone(), note_id),
        };
        self.edges.push(GraphEdge {
            id: format!("{start}-{end}"),
            start,
            end,
            label: String::new(),
            is_note: true,
        });
    }
}

fn first_shape(item: &StateItem) -> StateShape {
    match (item.start, item.kind) {
        (_, StateKind::Fork) => StateShape::Fork,
        (_, StateKind::Join) => StateShape::Join,
        (_, StateKind::Choice) => StateShape::Choice,
        (_, StateKind::Divider) => StateShape::Divider,
        (Some(true), _) => StateShape::Start,
        (Some(false), _) => StateShape::End,
        (None, StateKind::Default) => StateShape::Rect,
    }
}

/// 설명이 붙을 때마다 줄을 늘리고, 줄이 하나뿐이면 제목만 바꾼 상자(복합 상태면 roundedWithTitle)로 되돌린다
fn apply_description(node: &mut GraphNode, description: &str) {
    if !description.is_empty() {
        let id = node.id.clone();
        node.description =
            match std::mem::replace(&mut node.description, Description::Lines(Vec::new())) {
                Description::Lines(mut lines) => {
                    lines.push(description.to_string());
                    node.shape = StateShape::RectWithTitle;
                    Description::Lines(lines)
                }
                Description::Text(text) if text.is_empty() => {
                    node.shape = StateShape::Rect;
                    Description::Text(description.to_string())
                }
                Description::Text(text) => {
                    node.shape = StateShape::RectWithTitle;
                    let first = (text != id).then_some(text);
                    Description::Lines(first.into_iter().chain([description.to_string()]).collect())
                }
            };
    }
    let is_single_line = matches!(&node.description, Description::Lines(lines) if lines.len() == 1);
    if is_single_line && node.shape == StateShape::RectWithTitle {
        node.shape = if node.is_group {
            StateShape::Composite
        } else {
            StateShape::Rect
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::parser::parse;

    fn graph(source: &str) -> StateGraph {
        build_graph(&parse(source).expect("parse")).expect("graph")
    }

    #[test]
    fn 처음_만난_문장이_모양을_정한다() {
        let graph = graph(
            "stateDiagram-v2\nA --> Check\nstate Check <<choice>>\nstate F <<fork>>\nA --> F",
        );
        let shape = |id: &str| {
            graph
                .nodes
                .iter()
                .find(|node| node.id == id)
                .expect(id)
                .shape
        };
        assert_eq!(shape("Check"), StateShape::Rect);
        assert_eq!(shape("F"), StateShape::Fork);
    }

    #[test]
    fn 메모는_그룹과_간선을_만들고_간선_번호를_쓴다() {
        let graph = graph("stateDiagram-v2\nA --> B\nnote right of B : hi\nB --> A");
        let ids: Vec<&str> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
        assert_eq!(ids, ["A", "B", "B----parent", "B----note-1"]);
        let edge_ids: Vec<&str> = graph.edges.iter().map(|edge| edge.id.as_str()).collect();
        assert_eq!(edge_ids, ["edge0", "B-B----note-1", "edge2"]);
    }

    #[test]
    fn 설명이_하나면_제목이_되고_둘이면_rect_with_title이다() {
        let graph = graph("stateDiagram-v2\nA : 첫 설명\nB : 하나\nB : 둘");
        assert_eq!(graph.nodes[0].label(), "첫 설명");
        assert_eq!(graph.nodes[0].shape, StateShape::Rect);
        assert_eq!(graph.nodes[1].shape, StateShape::RectWithTitle);
        assert_eq!(
            (graph.nodes[1].label(), graph.nodes[1].descriptions()),
            ("하나", &["둘".to_string()][..])
        );
    }
}
