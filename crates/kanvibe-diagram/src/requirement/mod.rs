//! requirementDiagram 다이어그램(Mermaid 11 requirementRenderer: dagre 레이아웃, requirementBox 모양).
//!
//! `contains`는 시작 쪽 원+십자 표식의 실선, 나머지 관계는 끝 쪽 열린 화살표의 10,7 점선으로 그린다.
//! classDef·style 선언은 상자 채움·테두리·구분선·글자색을 덮고, 앞머리 `title`은 그림 위 가운데에 놓는다.

mod parser;
mod requirement_box;

use self::parser::{RelationKind, RequirementDiagram, StyleAssignment};
use self::requirement_box::{BoxStyle, element_node, requirement_node};
use crate::er::unified::{
    DiagramTitle, UnifiedGraph, edge_label_background, render_unified, rough_pen,
};
use crate::flowchart::parser::{ArrowType, Stroke};
use crate::flowchart::render::CustomMarker;
use crate::flowchart::{ElementStyle, FlowEdge, FlowNode, FlowRender};
use crate::label::{FONT_SIZE, measure_label};
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
        title: crate::directive::read_config(definition)
            .title
            .map(|text| DiagramTitle {
                text,
                font_size: FONT_SIZE,
                color: context.palette.text,
            }),
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
    let requirements = diagram
        .requirements
        .iter()
        .map(|requirement| requirement_node(requirement, &box_style(context, &requirement.style)));
    let elements = diagram
        .elements
        .iter()
        .map(|element| element_node(element, &box_style(context, &element.style)));
    requirements.chain(elements).collect()
}

/// 테마 색 위에 노드의 스타일 선언(userNodeOverrides·labelStyles)을 덮는다
fn box_style<'a>(context: &RenderContext<'a>, assignment: &StyleAssignment) -> BoxStyle<'a> {
    let palette = context.palette;
    let user = ElementStyle::from_declarations(assignment.declarations.iter());
    BoxStyle {
        fonts: context.fonts,
        mode: context.label_mode,
        fill: user.fill.unwrap_or(palette.node_fill),
        border: rough_pen(
            user.stroke.unwrap_or(palette.node_stroke),
            user.stroke_width,
        ),
        text: user.text_color.unwrap_or(palette.text),
    }
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

#[cfg(test)]
mod tests {
    use crate::scene::SceneItem;
    use crate::text::tests::bundled_fonts;
    use crate::theme::Theme;
    use crate::{LabelMode, render_scene};

    #[test]
    fn 스타일_선언으로_테두리_굵기와_채움을_바꾸고_제목을_더한다() {
        let fonts = bundled_fonts();
        let source = include_str!("../../../../parity/diagrams/baseline/requirement-styles.mmd");
        let scene = render_scene(source, Theme::Light, LabelMode::Html, &fonts).unwrap();
        // requirement-styles 기준 SVG viewBox: 0 -50 170.40625 436
        assert!((scene.width - 170.406_25).abs() < 0.01, "{}", scene.width);
        assert!((scene.height - 436.0).abs() < 0.01, "{}", scene.height);
        let widths: Vec<f64> = scene
            .items
            .iter()
            .filter_map(|item| match item {
                SceneItem::Path(path) => path.stroke.as_ref().map(|stroke| stroke.width),
                _ => None,
            })
            .collect();
        assert!(widths.contains(&3.0), "{widths:?}");
    }
}
