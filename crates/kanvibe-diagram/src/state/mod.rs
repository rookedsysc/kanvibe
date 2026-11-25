//! stateDiagram·stateDiagram-v2 다이어그램(stateRenderer-v3-unified).
//!
//! Mermaid 11.15는 두 머리말 모두 통합 렌더러로 그린다. 문서 나무를 dataFetcher처럼 노드·간선 목록으로 편 뒤
//! flowchart와 같은 dagre·클러스터·간선 파이프라인([`crate::flowchart::render_layout`])에 넘기고, 상태 모양과
//! 복합 상태 상자는 노드 장식과 클러스터 그리기 함수로 덧입힌다.

mod graph;
mod parser;
mod shapes;
mod sketch;

use self::graph::{GraphEdge, GraphNode, StateShape, build_graph};
use self::shapes::{NeoLook, ROUGH_STROKE_WIDTH, STATE_PADDING, StateColors, first_color};
use crate::er::unified::{DiagramTitle, UnifiedGraph, render_unified};
use crate::flowchart::parser::{ArrowType, Direction, Stroke};
use crate::flowchart::render::ClusterPainter;
use crate::flowchart::{ElementStyle, FlowEdge, FlowNode, FlowRender};
use crate::label::{LabelBox, measure_label};
use crate::rough::RoughSet;
use crate::scene::{Color, Point, Scene, SceneItem};
use crate::theme::Palette;
use crate::theme_variables::ThemeVariables;
use crate::{DiagramError, LabelMode, RenderContext};

/// state.nodeSpacing·rankSpacing 기본값
const NODE_SPACING: f64 = 50.0;
const RANK_SPACING: f64 = 50.0;
/// `.statediagramTitleText`의 글자 크기
const TITLE_FONT_SIZE: f64 = 18.0;

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let root = parser::parse(definition)?;
    let graph = build_graph(&root)?;
    if graph.edges.len() > crate::MAX_EDGES {
        return Err(DiagramError::Syntax(format!(
            "간선이 {}개를 넘습니다",
            crate::MAX_EDGES
        )));
    }
    let config = crate::directive::read_config(definition);
    let variables = &context.theme_variables;
    let colors = StateColors::for_theme(context.palette.theme).with_theme_variables(variables);
    let palette = state_palette(context.palette, context.label_mode, variables, &colors);
    let is_neo = config.tree.get("look").and_then(|look| look.as_str()) == Some("neo");
    let styler = StateStyler {
        render: FlowRender {
            fonts: context.fonts,
            palette: &palette,
        },
        colors,
        edge_label_color: edge_label_color(variables, context.label_mode),
        mode: context.label_mode,
        neo: is_neo.then(|| NeoLook::for_theme(palette.theme)),
        sketch_seed: hand_drawn_seed(&config),
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
    let title = config.title.map(|text| DiagramTitle {
        text,
        font_size: TITLE_FONT_SIZE,
        color: palette.text,
    });
    let unified = UnifiedGraph {
        direction: graph.direction,
        nodes,
        edges,
        node_spacing: NODE_SPACING,
        rank_spacing: RANK_SPACING,
        title,
    };
    Ok(render_unified(&styler.render, unified))
}

/// state styles.js의 색. 상태 상자는 `stateBkg || mainBkg`·`stateBorder || nodeBorder`, 전이는 `transitionColor`,
/// SVG 글자는 뿌리의 `fill: textColor`를 물려받는다. 간선 라벨 바탕은 HTML 라벨이면 labelBkg 규칙이 없어 문단(p) 한 겹이고,
/// SVG 글자 라벨의 바탕 사각형은 `.edgeLabel .label rect`(labelBackgroundColor, opacity 0.5)다
fn state_palette(
    base: &Palette,
    mode: LabelMode,
    variables: &ThemeVariables,
    colors: &StateColors,
) -> Palette {
    let mut palette = base.clone();
    if let Some(color) = first_color(variables, &["stateBkg", "mainBkg"]) {
        palette.node_fill = color;
    }
    if let Some(color) = first_color(variables, &["stateBorder", "nodeBorder"]) {
        palette.node_stroke = color;
    }
    variables.read_color("transitionColor", &mut palette.line);
    variables.read_color("textColor", &mut palette.text);
    palette.edge_label_outer = Color::rgba(0, 0, 0, 0.0);
    if mode == LabelMode::SvgText {
        palette.edge_label_inner = palette.node_fill;
        variables.read_color("labelBackgroundColor", &mut palette.edge_label_inner);
    }
    colors.carry_in_palette(&mut palette, mode);
    palette
}

/// SVG 글자 간선 라벨: `.edgeLabel .label text { fill: transitionLabelColor || tertiaryTextColor }`. HTML 라벨은 바꾸지 않는다
fn edge_label_color(variables: &ThemeVariables, mode: LabelMode) -> Option<Color> {
    if mode != LabelMode::SvgText {
        return None;
    }
    first_color(variables, &["transitionLabelColor", "tertiaryTextColor"])
}

/// 상태 노드·간선을 flowchart 파이프라인의 노드·간선으로 옷 입힌다
struct StateStyler<'a> {
    render: FlowRender<'a>,
    colors: StateColors,
    /// SVG 글자 간선 라벨 색(없으면 flowchart처럼 palette.text)
    edge_label_color: Option<Color>,
    mode: LabelMode,
    /// `look: neo`면 테두리·그림자·화살촉을 바꾼다
    neo: Option<NeoLook>,
    /// `look: handDrawn`이면 roughjs 시드(handDrawnSeed)
    sketch_seed: Option<i32>,
}

/// `look: handDrawn`의 handDrawnSeed(없으면 0: 원본은 그릴 때마다 달라진다)
fn hand_drawn_seed(config: &crate::directive::DiagramConfig) -> Option<i32> {
    let look = config.tree.get("look").and_then(|look| look.as_str());
    if look != Some("handDrawn") {
        return None;
    }
    let seed = config
        .tree
        .get("handDrawnSeed")
        .and_then(|seed| seed.as_i64());
    Some(seed.unwrap_or(0) as i32)
}

impl StateStyler<'_> {
    fn measure(&self, text: &str) -> LabelBox {
        measure_label(self.render.fonts, text, self.mode)
    }

    fn label_color(&self) -> Color {
        self.colors.label(self.render.palette, self.mode)
    }

    fn flow_edge(&self, edge: &GraphEdge) -> FlowEdge {
        // neo 간선은 인라인 stroke-dasharray(0 0 길이 0)가 `.note-edge` 점선을 덮어 실선이 된다
        let (arrow_end, stroke) = match (edge.is_note, self.neo.is_some()) {
            (true, false) => (ArrowType::None, Stroke::Dashed),
            (true, true) => (ArrowType::None, Stroke::Normal),
            (false, false) => (ArrowType::Barb, Stroke::Normal),
            (false, true) => (ArrowType::BarbNeo, Stroke::Normal),
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
            style: ElementStyle {
                sketch_seed: self.sketch_seed,
                text_color: self.edge_label_color,
                ..ElementStyle::default()
            },
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
            _ if self.sketch_seed.is_some() => self.dress_sketch(node, &mut flow),
            StateShape::Note => self.dress_note(node, &mut flow),
            StateShape::RectWithTitle => self.dress_titled(node, &mut flow),
            _ => self.dress_shape(node, &mut flow),
        }
        if let Some(neo) = &self.neo {
            self.dress_neo(node, &mut flow, neo);
        }
        apply_user_styles(&mut flow.style, &node.styles);
        flow
    }

    /// `[data-look="neo"]` 규칙: 상태 모양과 복합 상태 상자는 그림자와 neo 테두리를 받고, path로 그린 모양
    /// (끝 원·choice·fork·메모)은 테두리가 1px로 얇아진다
    fn dress_neo(&self, node: &GraphNode, flow: &mut FlowNode, neo: &NeoLook) {
        let style = &mut flow.style;
        match node.shape {
            StateShape::NoteGroup | StateShape::Divider => return,
            StateShape::Composite => {}
            StateShape::End => {
                style.stroke_width = Some(1.0);
                flow.decorations = shapes::neo_end_inner_circle(self.colors.end_inner, neo);
            }
            StateShape::Choice | StateShape::Fork | StateShape::Join | StateShape::Note => {
                style.stroke_width = Some(1.0);
            }
            _ => {}
        }
        style.stroke = Some(neo.stroke);
        style.stroke_gradient = neo.stroke_gradient;
        style.drop_shadow = Some(neo.shadow);
    }

    fn sketch_colors(&self) -> sketch::SketchColors {
        let palette = self.render.palette;
        sketch::SketchColors {
            line: self.colors.line,
            fill: palette.node_fill,
            border: palette.node_stroke,
            end_inner: self.colors.end_inner,
        }
    }

    /// handDrawn 상태 모양: 보통 모양으로 크기·라벨을 정한 뒤 흔들린 경로로 바꿔 그린다
    fn dress_sketch(&self, node: &GraphNode, flow: &mut FlowNode) {
        let seed = self.sketch_seed.unwrap_or_default();
        let colors = self.sketch_colors();
        let (geometry, decorations) = match node.shape {
            StateShape::Start => sketch::start(&colors, seed),
            StateShape::End => sketch::end(&colors, seed),
            StateShape::Note => return self.sketch_note(node, flow, seed),
            StateShape::RectWithTitle => return self.sketch_titled(node, flow, seed),
            _ => return self.sketch_shape(node, flow, seed),
        };
        flow.geometry = Some(geometry);
        flow.decorations = decorations;
    }

    /// 상태 상자(반지름 5)·choice(크기 28 고정)·fork/join 막대(경계 + padding/2)
    fn sketch_shape(&self, node: &GraphNode, flow: &mut FlowNode, seed: i32) {
        self.dress_shape(node, flow);
        let colors = self.sketch_colors();
        let Some(geometry) = flow.geometry.as_mut() else {
            return;
        };
        let grow = STATE_PADDING / 2.0;
        let sets = match node.shape {
            StateShape::Choice => {
                geometry.outlines.clear();
                flow.decorations = sketch::choice(&colors, seed);
                return;
            }
            StateShape::Fork | StateShape::Join => {
                let size = (geometry.width - grow, geometry.height - grow);
                sketch::bar(size.0, size.1, &colors, seed)
            }
            _ => sketch::rounded_box((geometry.width, geometry.height), 5.0, &colors, seed),
        };
        let is_bar = matches!(node.shape, StateShape::Fork | StateShape::Join);
        self.cover_with_sketch(node, flow, sets, Vec::new());
        if let Some(geometry) = flow.geometry.as_mut().filter(|_| is_bar) {
            geometry.width += grow;
            geometry.height += grow;
        }
    }

    fn sketch_note(&self, node: &GraphNode, flow: &mut FlowNode, seed: i32) {
        self.dress_note(node, flow);
        let Some(geometry) = &flow.geometry else {
            return;
        };
        let colors = (self.colors.note_fill, self.colors.note_stroke);
        let sets = sketch::note(geometry.width, geometry.height, colors, seed);
        self.cover_with_sketch(node, flow, sets, Vec::new());
    }

    /// rectWithTitle: 반지름 10 상자와 rc.line 구분선(노드 크기는 상자 경계만)
    fn sketch_titled(&self, node: &GraphNode, flow: &mut FlowNode, seed: i32) {
        self.dress_titled(node, flow);
        let colors = self.sketch_colors();
        let Some(geometry) = &flow.geometry else {
            return;
        };
        let (width, height) = (geometry.width, geometry.height);
        let mut extra = std::mem::take(&mut flow.decorations);
        let divider_y = -height / 2.0 + STATE_PADDING / 2.0 + flow.label.height;
        let from = Point::new(-width / 2.0, divider_y);
        let to = Point::new(width / 2.0, divider_y);
        let mut line = sketch::divider_line(from, to, &colors, seed);
        // 보통 모양의 첫 장식(구분선)을 흔들린 선으로 바꾼다
        line.extend(extra.drain(1..));
        let sets = sketch::rounded_box((width, height), 10.0, &colors, seed);
        self.cover_with_sketch(node, flow, sets, line);
    }

    /// 흔들린 경로가 라벨 아래에 오도록 라벨 글자도 장식 끝으로 옮기고, 노드 크기를 경로 경계로 바꾼다
    fn cover_with_sketch(
        &self,
        node: &GraphNode,
        flow: &mut FlowNode,
        sets: Vec<RoughSet>,
        extra: Vec<SceneItem>,
    ) {
        let Some(geometry) = flow.geometry.as_mut() else {
            return;
        };
        let label = std::mem::take(&mut flow.label);
        let user = ElementStyle::from_declarations(node.styles.iter());
        let color = user
            .text_color
            .or(flow.style.text_color)
            .unwrap_or(self.label_color());
        let top = -label.height / 2.0 + geometry.label_offset_y;
        let texts = label.text_items(self.render.fonts, -label.width / 2.0, top, color);
        (geometry.width, geometry.height) = sketch::sketch_size(&sets);
        geometry.outlines.clear();
        geometry.label_offset_y = 0.0;
        let mut items = sketch::into_items(sets);
        items.extend(extra);
        items.extend(texts.into_iter().map(SceneItem::Text));
        flow.decorations = items;
    }

    fn dress_cluster(&self, node: &GraphNode, flow: &mut FlowNode) {
        let is_sketched = self.sketch_seed.is_some();
        flow.style.sketch_seed = self.sketch_seed;
        let painter: ClusterPainter = match node.shape {
            StateShape::Divider if is_sketched => sketch::paint_divider,
            StateShape::Divider => shapes::paint_divider,
            StateShape::NoteGroup => shapes::paint_note_group,
            _ if node.is_alt && is_sketched => sketch::paint_alt_composite,
            _ if is_sketched => sketch::paint_composite,
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
            flow.style.fill = Some(self.colors.composite_title);
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
                    paint_style(palette.node_fill, colors.line, 2.0),
                )
            }
            StateShape::Choice => (
                shapes::choice_geometry(),
                paint_style(palette.node_fill, palette.node_stroke, ROUGH_STROKE_WIDTH),
            ),
            StateShape::Fork | StateShape::Join => (
                shapes::bar_geometry(node.layout_dir == Direction::LeftRight),
                paint_style(colors.line, colors.line, ROUGH_STROKE_WIDTH),
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
        let scene = crate::render_scene(
            source,
            Theme::Light,
            LabelMode::Html,
            crate::theme_variables::ThemeVariablesPolicy::Ignore,
            &fonts,
        )
        .expect("scene");
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
            let scene = crate::render_scene(
                source,
                Theme::Dark,
                mode,
                crate::theme_variables::ThemeVariablesPolicy::Ignore,
                &fonts,
            )
            .expect("scene");
            assert!(scene.width > 500.0 && scene.height > 200.0, "{mode:?}");
        }
    }

    #[test]
    fn 앞머리_제목을_그림_위에_두고_떠_있는_메모와_클릭은_그리지_않는다() {
        let fonts = bundled_fonts();
        let source = include_str!("../../../../parity/diagrams/baseline/state-notes-title.mmd");
        let scene = crate::render_scene(
            source,
            Theme::Light,
            LabelMode::Html,
            crate::theme_variables::ThemeVariablesPolicy::Ignore,
            &fonts,
        )
        .expect("scene");
        // state-notes-title 기준 SVG viewBox: 0 -52 157.546875 350
        assert!((scene.width - 157.546_875).abs() < 0.01, "{}", scene.width);
        assert!((scene.height - 350.0).abs() < 0.01, "{}", scene.height);
        let texts: Vec<&str> = scene
            .items
            .iter()
            .filter_map(|item| match item {
                crate::scene::SceneItem::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&"Order states") && texts.contains(&"N1"));
        assert!(!texts.contains(&"floating hint"));
    }
}
