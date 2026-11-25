//! classDiagram·classDiagram-v2 다이어그램(Mermaid 11 classRenderer-v3-unified, dagre 레이아웃).
//!
//! ClassDB.getData 순서(namespace → 클래스 → 노트 → 인터페이스, 노트 간선 → 관계)로 노드·간선을 만들고,
//! flowchart의 dagre 단계([`render_graph`]: 클러스터 정리·단계별 dagre·노드 교점·라벨 자리)를 그대로 쓴다.
//! 그리는 쪽은 클래스 상자([`class_box`])와 관계선·표식·다중성 라벨([`edges`])만 [`painter`]가 따로 맡는다.
//! 색은 기준 SVG `<style>`에서 읽은 classDiagram 값이다.

mod class_box;
mod edges;
mod markdown;
mod measure;
mod member;
mod painter;
mod parser;
mod statements;
mod underline;

use std::collections::HashMap;

use self::class_box::{LineBox, layout_class_box};
use self::edges::{ClassEdge, EdgePainter, LinePattern, TERMINAL_FONT_SIZE};
use self::measure::{MeasuredText, measure_markdown, measure_text};
use self::painter::{ClassPainter, NodeDrawing};
use self::parser::{ClassDiagram, Relation};
use crate::flowchart::parser::{ArrowType, Stroke};
use crate::flowchart::shapes::{Intersect, NodeGeometry};
use crate::flowchart::{ElementStyle, FlowEdge, FlowNode, FlowRender, render_layout};
use crate::label::{FONT_SIZE, LabelBox, LabelLine, measure_label};
use crate::scene::{Color, Scene};
use crate::text::FontWeight;
use crate::theme::{Theme, hsl};
use crate::{DiagramError, RenderContext};

/// namespace 묶음 padding(config.class.padding ?? 16)
const NAMESPACE_PADDING: f64 = 16.0;

/// classDiagram 스타일의 색
#[derive(Clone, Debug)]
pub struct ClassColors {
    pub node_fill: Color,
    pub node_stroke: Color,
    /// `.nodeLabel`·`.edgeLabel`·`.label text`(classText)
    pub class_text: Color,
    pub line: Color,
    /// `.labelBkg`·`.edgeLabel .label rect`
    pub label_background: Color,
    /// SVG 다중성 라벨이 물려받는 svg 루트 글자색
    pub root_text: Color,
    pub note_fill: Color,
    pub note_stroke: Color,
    pub note_text: Color,
}

impl ClassColors {
    fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self {
                node_fill: Color::rgb(0xEC, 0xEC, 0xFF),
                node_stroke: Color::rgb(0x93, 0x70, 0xDB),
                class_text: Color::rgb(0x13, 0x13, 0x00),
                line: Color::rgb(0x33, 0x33, 0x33),
                label_background: Color::rgb(0xEC, 0xEC, 0xFF),
                root_text: Color::rgb(0x33, 0x33, 0x33),
                note_fill: Color::rgb(0xFF, 0xF5, 0xAD),
                note_stroke: Color::rgb(0xAA, 0xAA, 0x33),
                note_text: Color::rgb(0, 0, 0),
            },
            // theme-dark: noteBkgColor = lighten(#1f2020, 16), 테두리 = 그 색의 명도 −10·채도 0, 글자 = invert
            Theme::Dark => Self {
                node_fill: Color::rgb(0x1F, 0x20, 0x20),
                node_stroke: Color::rgb(0xCC, 0xCC, 0xCC),
                class_text: Color::rgb(0xE0, 0xDF, 0xDF),
                line: Color::rgb(211, 211, 211),
                label_background: Color::rgb(0x1F, 0x20, 0x20),
                root_text: Color::rgb(0xCC, 0xCC, 0xCC),
                note_fill: hsl(180.0, 1.587_301_587_3, 28.352_941_176_5),
                note_stroke: hsl(180.0, 0.0, 18.352_941_176_5),
                note_text: Color::rgb(184, 182, 182),
            },
        }
    }
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let diagram = parser::parse(definition)?;
    if diagram.relations.len() + diagram.notes.len() > crate::MAX_EDGES {
        return Err(DiagramError::Syntax(format!(
            "간선이 {}개를 넘습니다",
            crate::MAX_EDGES
        )));
    }
    let colors = ClassColors::for_theme(context.palette.theme);
    let drawings = node_drawings(&diagram, context);
    let nodes = layout_nodes(&diagram, context, &drawings);
    let (flow_edges, relations) = build_edges(&diagram, context);
    let painter = ClassPainter {
        flow: FlowRender {
            fonts: context.fonts,
            palette: context.palette,
        },
        edges: EdgePainter {
            fonts: context.fonts,
            colors: &colors,
            mode: context.label_mode,
        },
        colors: &colors,
        drawings,
        relations,
    };
    let scene = render_layout(&painter, diagram.direction, nodes, flow_edges);
    Ok(scene)
}

/// 클래스 상자·노트·인터페이스의 크기와 그림(노드 id별)
fn node_drawings(diagram: &ClassDiagram, context: &RenderContext) -> HashMap<String, NodeDrawing> {
    let (fonts, mode) = (context.fonts, context.label_mode);
    let mut drawings = HashMap::new();
    for class in &diagram.classes {
        let style = ElementStyle::from_declarations(class.styles.iter());
        let layout = layout_class_box(fonts, class, mode);
        drawings.insert(class.id.clone(), NodeDrawing::ClassBox(layout, style));
    }
    for (index, note) in diagram.notes.iter().enumerate() {
        let label = measure_label(fonts, &note.text, mode);
        drawings.insert(format!("note{index}"), NodeDrawing::Note(label));
    }
    for (index, label) in diagram.interfaces.iter().enumerate() {
        let label = measure_label(fonts, label, mode);
        drawings.insert(format!("interface{index}"), NodeDrawing::Interface(label));
    }
    drawings
}

/// getData의 노드 순서: namespace 묶음, 클래스, 노트, 인터페이스
fn layout_nodes(
    diagram: &ClassDiagram,
    context: &RenderContext,
    drawings: &HashMap<String, NodeDrawing>,
) -> Vec<FlowNode> {
    let mut nodes: Vec<FlowNode> = diagram
        .namespaces
        .iter()
        .map(|namespace| FlowNode {
            id: namespace.id.clone(),
            label: measure_label(context.fonts, &namespace.label, context.label_mode),
            is_hidden: false,
            parent: namespace.parent.clone(),
            geometry: None,
            style: ElementStyle::default(),
            dir: None,
            padding: NAMESPACE_PADDING,
            decorations: Vec::new(),
            cluster_painter: None,
        })
        .collect();
    let classes = diagram
        .classes
        .iter()
        .map(|class| (class.id.clone(), class.parent.clone()));
    let notes = diagram
        .notes
        .iter()
        .enumerate()
        .map(|(index, note)| (format!("note{index}"), note.parent.clone()));
    let interfaces = (0..diagram.interfaces.len()).map(|index| (format!("interface{index}"), None));
    for (id, parent) in classes.chain(notes).chain(interfaces) {
        let size = drawings[&id].size();
        nodes.push(leaf_node(id, parent, size));
    }
    nodes
}

/// dagre와 교점 계산에 쓰는 잎 노드: 사각형 크기만 넘기고 그리기는 [`ClassPainter`]가 맡는다
fn leaf_node(id: String, parent: Option<String>, (width, height): (f64, f64)) -> FlowNode {
    FlowNode {
        id,
        label: LabelBox::default(),
        is_hidden: true,
        parent,
        geometry: Some(NodeGeometry {
            width,
            height,
            outlines: Vec::new(),
            intersect: Intersect::Rect,
            label_offset_y: 0.0,
        }),
        style: ElementStyle::default(),
        dir: None,
        padding: 0.0,
        decorations: Vec::new(),
        cluster_painter: None,
    }
}

/// 노트 간선(점선, 표식 없음) 다음에 관계 간선
fn build_edges(
    diagram: &ClassDiagram,
    context: &RenderContext,
) -> (Vec<FlowEdge>, HashMap<String, ClassEdge>) {
    let mut flow_edges = Vec::new();
    let mut edges = HashMap::new();
    let class_notes = diagram
        .notes
        .iter()
        .enumerate()
        .filter_map(|(index, note)| note.class_id.as_ref().map(|class_id| (index, class_id)))
        .filter(|(_, class_id)| diagram.classes.iter().any(|class| &class.id == *class_id));
    for (index, class_id) in class_notes {
        let id = format!("edgeNote{index}");
        flow_edges.push(layout_edge(&id, &format!("note{index}"), class_id, None));
        edges.insert(id, note_edge());
    }
    for (index, relation) in diagram.relations.iter().enumerate() {
        let id = format!("id_{}_{}_{}", relation.start, relation.end, index + 1);
        let edge = relation_edge(relation, context);
        let label = edge
            .label
            .as_ref()
            .map(|label| label_layout_box(label, context));
        flow_edges.push(layout_edge(&id, &relation.start, &relation.end, label));
        edges.insert(id, edge);
    }
    (flow_edges, edges)
}

fn layout_edge(id: &str, start: &str, end: &str, label: Option<LabelBox>) -> FlowEdge {
    FlowEdge {
        id: id.to_string(),
        start: start.to_string(),
        end: end.to_string(),
        label: label.unwrap_or_default(),
        arrow_start: ArrowType::None,
        arrow_end: ArrowType::None,
        stroke: Stroke::Normal,
        minlen: 1,
        style: ElementStyle::default(),
    }
}

/// 관계 라벨의 글자 상자. SVG 글자면 FlowEdge::label_layout_size가 바탕 사각형 몫을 더해 dagre에 넘긴다
fn label_layout_box(label: &MeasuredText, context: &RenderContext) -> LabelBox {
    let mode = context.label_mode;
    LabelBox {
        lines: vec![LabelLine {
            text: label.text(),
            width: label.width,
        }],
        width: label.width,
        height: LineBox::for_mode(context.fonts, mode).height,
        mode,
        font_size: FONT_SIZE,
    }
}

fn note_edge() -> ClassEdge {
    ClassEdge {
        start_kind: None,
        end_kind: None,
        pattern: LinePattern::Dotted,
        label: None,
        start_label: None,
        end_label: None,
    }
}

/// 관계 라벨은 markdown 라벨이고, 다중성은 markdown 없는 createLabel이다
fn relation_edge(relation: &Relation, context: &RenderContext) -> ClassEdge {
    let (fonts, mode) = (context.fonts, context.label_mode);
    let is_shown = |text: &str| !text.is_empty() && text != "none";
    let terminal = |text: &str| {
        is_shown(text)
            .then(|| measure_text(fonts, text, TERMINAL_FONT_SIZE, FontWeight::Regular, mode))
    };
    ClassEdge {
        start_kind: relation.start_kind,
        end_kind: relation.end_kind,
        pattern: if relation.is_dashed {
            LinePattern::Dashed
        } else {
            LinePattern::Solid
        },
        label: is_shown(&relation.title).then(|| {
            measure_markdown(fonts, &relation.title, FONT_SIZE, FontWeight::Regular, mode)
        }),
        start_label: terminal(&relation.start_label),
        end_label: terminal(&relation.end_label),
    }
}

#[cfg(test)]
mod tests {
    use crate::text::tests::bundled_fonts;
    use crate::theme::Theme;
    use crate::{LabelMode, render_scene};

    #[test]
    fn 노트_막대사탕_자기_관계를_함께_그린다() {
        let source = "classDiagram\n direction LR\n class Shape {\n <<abstract>>\n +area()* double\n }\n class Empty\n Shape <|-- Circle\n Circle --> Circle : self\n Circle ..|> Drawable\n Shape \"1\" o-- \"*\" Point : vertices\n Circle --() Printable\n note for Circle \"round<br>shape\"\n note \"free note\"";
        let fonts = bundled_fonts();
        for mode in [LabelMode::Html, LabelMode::SvgText] {
            let scene = render_scene(source, Theme::Dark, mode, &fonts).expect("render");
            assert!(scene.width > 300.0 && scene.height > 100.0, "{mode:?}");
            assert!(scene.width.is_finite() && scene.height.is_finite());
        }
    }
}
