//! stateDiagram·stateDiagram-v2 다이어그램(stateRenderer-v3-unified).
//!
//! Mermaid 11.15는 두 머리말 모두 통합 렌더러로 그린다. 문서 나무를 dataFetcher처럼 노드·간선 목록으로 편 뒤
//! flowchart와 같은 dagre·클러스터·간선 파이프라인([`crate::flowchart::render_layout`])에 넘기고, 상태 모양과
//! 복합 상태 상자는 노드 장식과 클러스터 그리기 함수로 덧입힌다.

mod graph;
mod parser;
mod shapes;

use self::graph::{GraphEdge, GraphNode, StateShape, build_graph};
use self::shapes::{ROUGH_STROKE_WIDTH, STATE_PADDING, StateColors};
use crate::flowchart::parser::{ArrowType, Direction, Stroke};
use crate::flowchart::render::ClusterPainter;
use crate::flowchart::{ElementStyle, FlowEdge, FlowNode, FlowRender, render_layout};
use crate::label::{LabelBox, measure_label};
use crate::scene::{Color, Scene};
use crate::theme::Palette;
use crate::{DiagramError, LabelMode, RenderContext};

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let root = parser::parse(definition)?;
    let graph = build_graph(&root)?;
    if graph.edges.len() > crate::MAX_EDGES {
        return Err(DiagramError::Syntax(format!(
            "간선이 {}개를 넘습니다",
            crate::MAX_EDGES
        )));
    }
    let palette = state_palette(context.palette, context.label_mode);
    let styler = StateStyler {
        render: FlowRender {
            fonts: context.fonts,
            palette: &palette,
        },
        colors: StateColors::for_theme(palette.theme),
        mode: context.label_mode,
    };
    let nodes = graph
        .nodes
        .iter()
        .map(|node| styler.flow_node(node))
        .collect();
    let edges = graph
        .edges
        .iter()
        .map(|edge| styler.flow_edge(edge))
        .collect();
    Ok(render_layout(&styler.render, graph.direction, nodes, edges))
}

/// 간선 라벨 바탕색. HTML 라벨은 labelBkg 규칙이 없어 문단(p) 한 겹이고, SVG 글자 라벨의 바탕 사각형은
/// `.edgeLabel .label rect`(mainBkg, opacity 0.5)다
fn state_palette(base: &Palette, mode: LabelMode) -> Palette {
    let mut palette = base.clone();
    palette.edge_label_outer = Color::rgba(0, 0, 0, 0.0);
    if mode == LabelMode::SvgText {
        palette.edge_label_inner = palette.node_fill;
    }
    palette
}

/// 상태 노드·간선을 flowchart 파이프라인의 노드·간선으로 옷 입힌다
struct StateStyler<'a> {
    render: FlowRender<'a>,
    colors: StateColors,
    mode: LabelMode,
}

impl StateStyler<'_> {
    fn measure(&self, text: &str) -> LabelBox {
        measure_label(self.render.fonts, text, self.mode)
    }

    fn label_color(&self) -> Color {
        self.colors.label(self.render.palette, self.mode)
    }

    fn flow_edge(&self, edge: &GraphEdge) -> FlowEdge {
        let (arrow_end, stroke) = if edge.is_note {
            (ArrowType::None, Stroke::Dashed)
        } else {
            (ArrowType::Barb, Stroke::Normal)
        };
        FlowEdge {
            id: edge.id.clone(),
            start: edge.start.clone(),
            end: edge.end.clone(),
            label: self.measure(&edge.label),
            arrow_start: ArrowType::None,
            arrow_end,
            stroke,
            minlen: 1,
            style: ElementStyle::default(),
        }
    }

    /// 노드 하나의 크기·그림·색. 복합 상태와 메모 그룹은 클러스터가 된다
    fn flow_node(&self, node: &GraphNode) -> FlowNode {
        let mut flow = FlowNode {
            id: node.id.clone(),
            label: LabelBox::default(),
            is_hidden: false,
            parent: node.parent.clone(),
            geometry: None,
            style: ElementStyle::default(),
            dir: node.dir,
            padding: STATE_PADDING,
            decorations: Vec::new(),
            cluster_painter: None,
        };
        match node.shape {
            StateShape::Composite | StateShape::Divider | StateShape::NoteGroup => {
                self.dress_cluster(node, &mut flow);
            }
            StateShape::Note => self.dress_note(node, &mut flow),
            StateShape::RectWithTitle => self.dress_titled(node, &mut flow),
            _ => self.dress_shape(node, &mut flow),
        }
        apply_user_styles(&mut flow.style, &node.styles);
        flow
    }

    fn dress_cluster(&self, node: &GraphNode, flow: &mut FlowNode) {
        let painter: ClusterPainter = match node.shape {
            StateShape::Divider => shapes::paint_divider,
            StateShape::NoteGroup => shapes::paint_note_group,
            _ if node.is_alt => shapes::paint_alt_composite,
            _ => shapes::paint_composite,
        };
        flow.cluster_painter = Some(painter);
        if node.shape == StateShape::NoteGroup {
            flow.padding = shapes::NOTE_GROUP_PADDING;
        }
        if node.shape == StateShape::Composite {
            flow.label = self.measure(node.label());
            flow.style.text_color = Some(self.label_color());
        }
    }

    /// 시작·끝 원, choice, fork·join 막대, 보통 상태 상자
    fn dress_shape(&self, node: &GraphNode, flow: &mut FlowNode) {
        let palette = self.render.palette;
        let colors = &self.colors;
        let (geometry, style) = match node.shape {
            StateShape::Start => (
                shapes::start_geometry(),
                paint_style(colors.start, colors.start, 1.0),
            ),
            StateShape::End => {
                flow.decorations = vec![shapes::end_inner_circle(colors.end_inner)];
                (
                    shapes::end_geometry(),
                    paint_style(palette.node_fill, palette.line, 2.0),
                )
            }
            StateShape::Choice => (
                shapes::choice_geometry(),
                paint_style(palette.node_fill, palette.node_stroke, ROUGH_STROKE_WIDTH),
            ),
            StateShape::Fork | StateShape::Join => (
                shapes::bar_geometry(node.layout_dir == Direction::LeftRight),
                paint_style(palette.line, palette.line, ROUGH_STROKE_WIDTH),
            ),
            _ => {
                flow.label = self.measure(node.label());
                (
                    shapes::rect_geometry(&flow.label),
                    paint_style(palette.node_fill, palette.node_stroke, 1.0),
                )
            }
        };
        flow.geometry = Some(geometry);
        flow.style = ElementStyle {
            text_color: Some(self.label_color()),
            ..style
        };
    }

    fn dress_note(&self, node: &GraphNode, flow: &mut FlowNode) {
        flow.label = self.measure(node.label());
        flow.geometry = Some(shapes::note_geometry(
            &flow.label,
            self.render.fonts,
            self.mode,
        ));
        let colors = &self.colors;
        flow.style = ElementStyle {
            text_color: Some(colors.note_text),
            ..paint_style(colors.note_fill, colors.note_stroke, ROUGH_STROKE_WIDTH)
        };
    }

    fn dress_titled(&self, node: &GraphNode, flow: &mut FlowNode) {
        let palette = self.render.palette;
        let label_color = self.label_color();
        flow.label = self.measure(node.label());
        let description = self.measure(&node.descriptions().join("<br/>"));
        let (geometry, decorations) = shapes::titled_geometry(
            &flow.label,
            &description,
            self.render.fonts,
            (palette.node_stroke, label_color),
        );
        flow.geometry = Some(geometry);
        flow.decorations = decorations;
        flow.style = ElementStyle {
            text_color: Some(label_color),
            ..paint_style(palette.node_fill, palette.node_stroke, 1.0)
        };
    }
}

fn paint_style(fill: Color, stroke: Color, stroke_width: f64) -> ElementStyle {
    ElementStyle {
        fill: Some(fill),
        stroke: Some(stroke),
        stroke_width: Some(stroke_width),
        ..ElementStyle::default()
    }
}

/// classDef·style 선언은 기본 색 위에 덮어쓴다
fn apply_user_styles(style: &mut ElementStyle, declarations: &[String]) {
    if declarations.is_empty() {
        return;
    }
    let user = ElementStyle::from_declarations(declarations.iter());
    style.fill = user.fill.or(style.fill);
    style.stroke = user.stroke.or(style.stroke);
    style.stroke_width = user.stroke_width.or(style.stroke_width);
    style.text_color = user.text_color.or(style.text_color);
    if !user.dash.is_empty() {
        style.dash = user.dash;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tests::bundled_fonts;
    use crate::theme::Theme;

    #[test]
    fn 기본_상태_다이어그램의_viewbox_크기를_맞춘다() {
        let fonts = bundled_fonts();
        let source = "stateDiagram-v2\n[*] --> Still\nStill --> Moving\nMoving --> Still\n\
                      Moving --> Crash\nCrash --> [*]";
        let scene =
            crate::render_scene(source, Theme::Light, LabelMode::Html, &fonts).expect("scene");
        // state-v1 기준 SVG viewBox: 87.859375 × 364
        assert!((scene.width - 87.859_375).abs() < 0.01, "{}", scene.width);
        assert!((scene.height - 364.0).abs() < 0.01, "{}", scene.height);
    }

    #[test]
    fn 동시_영역·메모·설명·선택·합류를_두_라벨_방식으로_그린다() {
        let fonts = bundled_fonts();
        let source = "stateDiagram-v2\ndirection LR\nclassDef hot fill:#f96\n[*] --> Idle\n\
                      Idle --> Busy : go\nstate \"Long\" as Busy\nBusy : first\nBusy : second\n\
                      state P {\n[*] --> A\n--\n[*] --> C\n}\nBusy --> P\nstate G <<choice>>\n\
                      P --> G\nG --> Out:::hot\nstate J <<join>>\nOut --> J\nJ --> [*]\n\
                      note left of Idle\nwaiting\nend note";
        for mode in [LabelMode::Html, LabelMode::SvgText] {
            let scene = crate::render_scene(source, Theme::Dark, mode, &fonts).expect("scene");
            assert!(scene.width > 500.0 && scene.height > 200.0, "{mode:?}");
        }
    }
}
