//! requirementDiagram 다이어그램(Mermaid 11 requirementRenderer: dagre 레이아웃, requirementBox 모양).
//!
//! `contains`는 시작 쪽 원+십자 표식의 실선, 나머지 관계는 끝 쪽 열린 화살표의 10,7 점선으로 그린다.

mod parser;
mod requirement_box;

use self::parser::{RelationKind, RequirementDiagram};
use self::requirement_box::{BoxStyle, element_node, requirement_node};
use crate::er::unified::{UnifiedGraph, edge_label_background, render_unified};
use crate::flowchart::parser::{ArrowType, Stroke};
use crate::flowchart::render::CustomMarker;
use crate::flowchart::{ElementStyle, FlowEdge, FlowNode, FlowRender};
use crate::label::measure_label;
use crate::scene::{Color, PathCommand, Point, Scene};
use crate::theme::{Palette, Theme, hsl};
use crate::{DiagramError, LabelMode, RenderContext};

/// requirement 설정이 없어 dagre 기본 간격을 쓴다
const NODE_SPACING: f64 = 50.0;
const RANK_SPACING: f64 = 50.0;
const RELATION_DASH: [f64; 2] = [10.0, 7.0];

/// markers.js requirement_contains: 원과 십자, refX 0
static CONTAINS_START: CustomMarker = CustomMarker {
    reference: Point::new(0.0, 10.0),
    circle: Some((Point::new(10.0, 10.0), 9.0, None)),
    strokes: &[
        PathCommand::MoveTo(Point::new(1.0, 10.0)),
        PathCommand::LineTo(Point::new(19.0, 10.0)),
        PathCommand::MoveTo(Point::new(10.0, 1.0)),
        PathCommand::LineTo(Point::new(10.0, 19.0)),
    ],
};

/// markers.js requirement_arrow: 열린 화살촉, refX 20
static ARROW_END: CustomMarker = CustomMarker {
    reference: Point::new(20.0, 10.0),
    circle: None,
    strokes: &[
        PathCommand::MoveTo(Point::new(0.0, 0.0)),
        PathCommand::LineTo(Point::new(20.0, 10.0)),
        PathCommand::MoveTo(Point::new(20.0, 10.0)),
        PathCommand::LineTo(Point::new(0.0, 20.0)),
    ],
};

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let diagram = parser::parse(definition)?;
    if diagram.relations.len() > crate::MAX_EDGES {
        return Err(DiagramError::Syntax(format!(
            "간선이 {}개를 넘습니다",
            crate::MAX_EDGES
        )));
    }
    let palette = requirement_palette(context.palette, context.label_mode);
    let render = FlowRender {
        fonts: context.fonts,
        palette: &palette,
    };
    let graph = UnifiedGraph {
        direction: diagram.direction,
        nodes: box_nodes(&diagram, context),
        edges: relation_edges(&diagram, context)?,
        node_spacing: NODE_SPACING,
        rank_spacing: RANK_SPACING,
    };
    Ok(render_unified(&render, graph))
}

/// requirementDiagram 스타일의 간선 라벨 바탕(labelBkg와 `.edgeLabel .label rect`)
fn requirement_palette(base: &Palette, mode: LabelMode) -> Palette {
    let background = match base.theme {
        Theme::Light => Color::rgba(232, 232, 232, 0.8),
        Theme::Dark => hsl(0.0, 0.0, 34.411_764_705_9),
    };
    let mut palette = base.clone();
    palette.edge_label_outer = background;
    palette.edge_label_inner = edge_label_background(background, mode);
    palette
}

/// RequirementDB.getData: 요구사항 다음 요소 순서
fn box_nodes(diagram: &RequirementDiagram, context: &RenderContext) -> Vec<FlowNode> {
    let style = BoxStyle {
        fonts: context.fonts,
        mode: context.label_mode,
        fill: context.palette.node_fill,
        border: context.palette.node_stroke,
        text: context.palette.text,
    };
    let requirements = diagram
        .requirements
        .iter()
        .map(|requirement| requirement_node(requirement, &style));
    let elements = diagram
        .elements
        .iter()
        .map(|element| element_node(element, &style));
    requirements.chain(elements).collect()
}

fn is_known(diagram: &RequirementDiagram, name: &str) -> bool {
    diagram.requirements.iter().any(|node| node.name == name)
        || diagram.elements.iter().any(|node| node.name == name)
}

/// SVG 글자 라벨은 `.edgeLabel .label text{fill}`(라이트 검정, 다크 lightgrey)을 쓴다
fn edge_text_color(context: &RenderContext) -> Option<Color> {
    match (context.label_mode, context.palette.theme) {
        (LabelMode::Html, _) => None,
        (LabelMode::SvgText, Theme::Light) => Some(Color::rgb(0, 0, 0)),
        (LabelMode::SvgText, Theme::Dark) => Some(Color::rgb(211, 211, 211)),
    }
}

fn relation_edges(
    diagram: &RequirementDiagram,
    context: &RenderContext,
) -> Result<Vec<FlowEdge>, DiagramError> {
    let mut edges = Vec::new();
    for relation in &diagram.relations {
        if !is_known(diagram, &relation.source) || !is_known(diagram, &relation.target) {
            return Err(DiagramError::Syntax(format!(
                "정의되지 않은 관계 끝: {} → {}",
                relation.source, relation.target
            )));
        }
        let is_contains = relation.kind == RelationKind::Contains;
        let label = format!("&lt;&lt;{}&gt;&gt;", relation.kind.name());
        edges.push(FlowEdge {
            id: format!("{}-{}-0", relation.source, relation.target),
            start: relation.source.clone(),
            end: relation.target.clone(),
            label: measure_label(context.fonts, &label, context.label_mode),
            arrow_start: if is_contains {
                ArrowType::Custom(&CONTAINS_START)
            } else {
                ArrowType::None
            },
            arrow_end: if is_contains {
                ArrowType::None
            } else {
                ArrowType::Custom(&ARROW_END)
            },
            stroke: Stroke::Normal,
            minlen: 1,
            style: ElementStyle {
                dash: if is_contains {
                    Vec::new()
                } else {
                    RELATION_DASH.to_vec()
                },
                text_color: edge_text_color(context),
                ..ElementStyle::default()
            },
        });
    }
    Ok(edges)
}
