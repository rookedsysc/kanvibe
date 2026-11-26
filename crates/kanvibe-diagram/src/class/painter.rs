//! flowchart dagre 단계([`render_graph`](crate::flowchart::layout::render_graph))가 부르는 classDiagram 그리기.
//!
//! namespace 묶음은 flowchart 클러스터 상자를 그대로 쓰고, 노드는 클래스 상자·노트·인터페이스 라벨로,
//! 간선은 class 표식과 다중성 라벨([`EdgePainter`])로 그린다.

use std::collections::HashMap;

use super::ClassColors;
use super::class_box::ClassBoxLayout;
use super::edges::{ClassEdge, EdgePainter, EdgePart};
use crate::LabelMode;
use crate::flowchart::geometry::BoundaryBox;
use crate::flowchart::layout::{EdgeRoute, LevelNode, LevelPainter};
use crate::flowchart::render::{Group, draw_cluster};
use crate::flowchart::{ElementStyle, FlowEdge, FlowNode, FlowRender};
use crate::graph::EdgeKey;
use crate::label::{FONT_SIZE, LabelBox, SVG_LINE_HEIGHT_EM};
use crate::scene::{Color, Point, SceneItem, StrokeStyle, TextItem, rounded_rect};
use crate::text::FontWeight;

/// 노트 padding: config.class.padding이 비어 있어 `?? 6`을 쓴다(classBox는 같은 값에 `?? 12`)
pub const NOTE_PADDING: f64 = 6.0;

/// labelRect 보조 노드의 사각형 한 변
const SELF_LOOP_HELPER_SIZE: f64 = 0.1;

/// 노드마다 그리는 방식
pub enum NodeDrawing {
    /// style·classDef 선언을 함께 들고 있다
    ClassBox(ClassBoxLayout, ElementStyle),
    Note(LabelBox),
    /// 막대사탕 관계의 인터페이스: 투명한 사각형에 라벨만 보인다
    Interface(LabelBox),
}

impl NodeDrawing {
    /// dagre와 교점 계산에 쓰는 사각형 크기
    pub fn size(&self) -> (f64, f64) {
        match self {
            NodeDrawing::ClassBox(layout, _) => (layout.width, layout.height),
            NodeDrawing::Note(label) => (
                label.width + NOTE_PADDING * 2.0,
                label.height + NOTE_PADDING * 2.0,
            ),
            NodeDrawing::Interface(label) => (label.width, label.height),
        }
    }
}

pub struct ClassPainter<'a> {
    pub flow: FlowRender<'a>,
    pub edges: EdgePainter<'a>,
    pub colors: &'a ClassColors,
    pub drawings: HashMap<String, NodeDrawing>,
    pub relations: HashMap<String, ClassEdge>,
}

impl LevelPainter for ClassPainter<'_> {
    fn draw_cluster(&self, group: &mut Group, flow: &FlowNode, node: &LevelNode) -> BoundaryBox {
        draw_cluster(&self.flow, group, flow, node)
    }

    /// 자기 간선 조각은 시작 조각만 시작 표식·다중성을, 끝 조각만 끝 표식·다중성을 갖는다
    fn draw_edge(
        &self,
        edges: &mut Group,
        labels: &mut Group,
        key: &EdgeKey,
        edge: &FlowEdge,
        route: &EdgeRoute,
    ) {
        let part = EdgePart {
            has_start: key.v == edge.start,
            has_end: key.w == edge.end,
            has_label: !edge.label.is_empty(),
        };
        if let Some(relation) = self.relations.get(&edge.id) {
            self.edges.draw(edges, labels, relation, part, route);
        }
    }

    fn draw_node(&self, group: &mut Group, flow: &FlowNode, center: Point) {
        let Some(drawing) = self.drawings.get(&flow.id) else {
            // 자기 간선 보조 노드(labelRect)는 원점에서 0.1×0.1 사각형이라 그 크기만 getBBox에 든다
            group.bounds.include_rect(
                center.x,
                center.y,
                SELF_LOOP_HELPER_SIZE,
                SELF_LOOP_HELPER_SIZE,
            );
            return;
        };
        let (width, height) = drawing.size();
        let (left, top) = (center.x - width / 2.0, center.y - height / 2.0);
        group.bounds.include_rect(left, top, width, height);
        match drawing {
            NodeDrawing::ClassBox(layout, style) => {
                group.items.extend(layout.draw(center, self.colors, style));
            }
            NodeDrawing::Note(label) => {
                let stroke = StrokeStyle {
                    color: self.colors.note_stroke,
                    width: 1.0,
                    dash: Vec::new(),
                };
                group.push_path(
                    rounded_rect(left, top, width, height, 0.0),
                    Some(self.colors.note_fill),
                    Some(stroke),
                );
                // HTML은 `.noteLabel .nodeLabel`(noteTextColor), SVG 글자는 `.label text`(classText) 색이다
                let color = match label.mode {
                    LabelMode::Html => self.colors.note_text,
                    LabelMode::SvgText => self.colors.class_text,
                };
                let svg_left = center.x - label.width / 2.0;
                self.push_label(group, label, center, svg_left, color);
            }
            NodeDrawing::Interface(label) => {
                self.push_label(group, label, center, center.x, self.colors.class_text);
            }
        }
    }
}

impl ClassPainter<'_> {
    /// labelHelper 라벨. HTML은 가운데 맞춘 div이고, SVG 글자는 text-anchor 없이 `svg_left`에서 줄마다 왼쪽
    /// 맞춤으로 시작하며 글자 상자 위 끝이 노드 가운데에서 높이/2 위다
    fn push_label(
        &self,
        group: &mut Group,
        label: &LabelBox,
        center: Point,
        svg_left: f64,
        color: Color,
    ) {
        let top = center.y - label.height / 2.0;
        if label.mode == LabelMode::Html {
            let texts = label.text_items(self.flow.fonts, center.x - label.width / 2.0, top, color);
            group.items.extend(texts.into_iter().map(SceneItem::Text));
            return;
        }
        let ascent = self.flow.fonts.primary_metrics(FONT_SIZE).ascent;
        for (index, line) in label.lines.iter().enumerate() {
            group.items.push(SceneItem::Text(TextItem {
                text: line.text.clone(),
                x: svg_left,
                baseline: top + ascent + FONT_SIZE * SVG_LINE_HEIGHT_EM * index as f64,
                font_size: FONT_SIZE,
                weight: FontWeight::Regular,
                color,
                rotation: None,
            }));
        }
    }
}
