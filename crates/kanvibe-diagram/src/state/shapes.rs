//! stateDiagram 노드 모양(rendering-elements/shapes의 stateStart·stateEnd·choice·forkJoin·note·roundedRect·
//! rectWithTitle)과 클러스터(clusters.js의 roundedWithTitle·noteGroup·divider), 기준 SVG `<style>`의 색.

use crate::LabelMode;
use crate::flowchart::effects::{drop_shadow, gradient_stroke};
use crate::flowchart::geometry::BoundaryBox;
use crate::flowchart::render::Group;
use crate::flowchart::shapes::{Intersect, NodeGeometry};
use crate::flowchart::{ElementStyle, FlowNode, FlowRender};
use crate::label::LabelBox;
use crate::scene::{
    Color, PathCommand, PathItem, Point, SceneItem, StrokeStyle, TextItem, ellipse, polygon,
    rounded_rect,
};
use crate::text::FontBook;
use crate::theme::{Palette, Theme, hsl};

/// 상태 상자 padding(dataFetcher의 nodeData.padding)
pub const STATE_PADDING: f64 = 8.0;
/// 메모 padding(flowchart.padding)
pub const NOTE_PADDING: f64 = 15.0;
/// 메모 그룹 padding
pub const NOTE_GROUP_PADDING: f64 = 16.0;
/// rough 모양(choice·fork·note)의 기본 선 굵기
pub const ROUGH_STROKE_WIDTH: f64 = 1.3;
const START_RADIUS: f64 = 7.0;
const END_INNER_RADIUS: f64 = 2.5;
const CHOICE_SIZE: f64 = 28.0;
const BAR_LENGTH: f64 = 70.0;
const BAR_THICKNESS: f64 = 10.0;

/// `look: neo` 테두리와 그림자. 어두운 테마는 테두리가 `url(#gradient)` 가로 그라디언트다
#[derive(Clone, Copy, Debug)]
pub struct NeoLook {
    pub stroke: Color,
    pub stroke_gradient: Option<(Color, Color)>,
    pub shadow: Color,
}

impl NeoLook {
    pub fn for_theme(theme: Theme) -> Self {
        let shadow = Color::rgb(185, 185, 185);
        match theme {
            Theme::Light => Self {
                stroke: Color::rgb(0x93, 0x70, 0xDB),
                stroke_gradient: None,
                shadow,
            },
            Theme::Dark => Self {
                stroke: Color::rgb(0xCC, 0xCC, 0xCC),
                stroke_gradient: Some((
                    Color::rgb(0xCC, 0xCC, 0xCC),
                    hsl(180.0, 0.0, 18.352_941_176_5),
                )),
                shadow,
            },
        }
    }
}

/// 기준 SVG에서 읽은 stateDiagram 전용 색
pub struct StateColors {
    /// HTML 라벨의 `.nodeLabel`·`.cluster-label` 색
    pub html_label: Color,
    pub start: Color,
    pub end_inner: Color,
    pub note_fill: Color,
    pub note_stroke: Color,
    pub note_text: Color,
    pub composite_inner: Color,
    pub composite_alt_inner: Color,
    pub divider_fill: Color,
}

impl StateColors {
    pub fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self {
                html_label: Color::rgb(0x13, 0x13, 0x00),
                start: Color::rgb(0x33, 0x33, 0x33),
                end_inner: Color::rgb(0x93, 0x70, 0xDB),
                note_fill: Color::rgb(0xFF, 0xF5, 0xAD),
                note_stroke: Color::rgb(0xAA, 0xAA, 0x33),
                note_text: Color::rgb(0, 0, 0),
                composite_inner: Color::rgb(255, 255, 255),
                composite_alt_inner: Color::rgb(0xF0, 0xF0, 0xF0),
                divider_fill: Color::rgb(0xF0, 0xF0, 0xF0),
            },
            Theme::Dark => Self {
                html_label: Color::rgb(0xE0, 0xDF, 0xDF),
                start: Color::rgb(0xF4, 0xF4, 0xF4),
                end_inner: Color::rgb(0xCC, 0xCC, 0xCC),
                note_fill: hsl(180.0, 1.587_301_587_3, 28.352_941_176_5),
                note_stroke: hsl(180.0, 0.0, 18.352_941_176_5),
                note_text: Color::rgb(184, 182, 182),
                composite_inner: Color::rgb(0x33, 0x33, 0x33),
                composite_alt_inner: Color::rgb(0x55, 0x55, 0x55),
                divider_fill: Color::rgb(0x55, 0x55, 0x55),
            },
        }
    }

    /// 상태·클러스터 라벨 색. SVG 글자 라벨은 루트의 `fill`(palette.text)을 물려받는다
    pub fn label(&self, palette: &Palette, mode: LabelMode) -> Color {
        match mode {
            LabelMode::Html => self.html_label,
            LabelMode::SvgText => palette.text,
        }
    }
}

fn geometry(
    width: f64,
    height: f64,
    outline: Vec<PathCommand>,
    intersect: Intersect,
) -> NodeGeometry {
    NodeGeometry {
        width,
        height,
        outlines: vec![outline],
        intersect,
        label_offset_y: 0.0,
    }
}

/// stateStart 원(`<circle r=7>`, getBBox 14×14)
pub fn start_geometry() -> NodeGeometry {
    let outline = ellipse(Point::new(0.0, 0.0), START_RADIUS, START_RADIUS);
    let size = START_RADIUS * 2.0;
    geometry(size, size, outline, Intersect::Circle(START_RADIUS))
}

/// stateEnd 바깥 원. roughjs 원 경로의 마지막 3차 곡선이 제어점 x 7.0177까지 나가서 getBBox 폭이
/// 7 + 0.75·0.0177 만큼 넓고, 교점 반지름도 그 폭의 절반이다
pub fn end_geometry() -> NodeGeometry {
    const ROUGH_CLOSING_BULGE: f64 = 0.75 * 0.017_724_288_152_425;
    let outline = ellipse(Point::new(0.0, 0.0), START_RADIUS, START_RADIUS);
    let width = START_RADIUS * 2.0 + ROUGH_CLOSING_BULGE;
    geometry(
        width,
        START_RADIUS * 2.0,
        outline,
        Intersect::Circle(width / 2.0),
    )
}

/// stateEnd 안쪽 원(지름 14·5/14 = 5)
pub fn end_inner_circle(color: Color) -> SceneItem {
    SceneItem::Path(PathItem {
        commands: ellipse(Point::new(0.0, 0.0), END_INNER_RADIUS, END_INNER_RADIUS),
        fill: Some(color),
        stroke: Some(StrokeStyle {
            color,
            width: 2.0,
            dash: Vec::new(),
        }),
    })
}

/// neo stateEnd 안쪽 원: 채움은 그대로, 테두리는 neo 1px(그라디언트면 띠로 나눠 긋는다)
pub fn neo_end_inner_circle(fill: Color, neo: &NeoLook) -> Vec<SceneItem> {
    let commands = ellipse(Point::new(0.0, 0.0), END_INNER_RADIUS, END_INNER_RADIUS);
    match neo.stroke_gradient {
        Some(colors) => {
            let mut items = vec![SceneItem::Path(PathItem {
                commands: commands.clone(),
                fill: Some(fill),
                stroke: None,
            })];
            items.extend(gradient_stroke(&commands, 1.0, colors));
            items
        }
        None => vec![SceneItem::Path(PathItem {
            commands,
            fill: Some(fill),
            stroke: Some(solid(neo.stroke, 1.0)),
        })],
    }
}

/// roundedRect(padding 8, 반지름 5)
pub fn rect_geometry(label: &LabelBox) -> NodeGeometry {
    let width = label.width + STATE_PADDING * 2.0;
    let height = label.height + STATE_PADDING * 2.0;
    let outline = rounded_rect(-width / 2.0, -height / 2.0, width, height, 5.0);
    geometry(width, height, outline, Intersect::Rect)
}

/// choice: 한 변 28의 마름모
pub fn choice_geometry() -> NodeGeometry {
    let half = CHOICE_SIZE / 2.0;
    let points = vec![
        Point::new(0.0, half),
        Point::new(half, 0.0),
        Point::new(0.0, -half),
        Point::new(-half, 0.0),
    ];
    geometry(
        CHOICE_SIZE,
        CHOICE_SIZE,
        polygon(&points),
        Intersect::Polygon {
            points,
            correction: 0.0,
        },
    )
}

/// forkJoin: 70×10 막대(LR이면 세로). 레이아웃 크기는 state.padding(8)의 절반만큼 크다
pub fn bar_geometry(is_vertical: bool) -> NodeGeometry {
    let (width, height) = if is_vertical {
        (BAR_THICKNESS, BAR_LENGTH)
    } else {
        (BAR_LENGTH, BAR_THICKNESS)
    };
    let outline = rounded_rect(-width / 2.0, -height / 2.0, width, height, 0.0);
    let grow = STATE_PADDING / 2.0;
    geometry(width + grow, height + grow, outline, Intersect::Rect)
}

/// note: 라벨 + padding 15. SVG 글자 라벨은 getBBox의 y(기준선 16 − ascent)만큼 아래로 내려 놓는다
pub fn note_geometry(label: &LabelBox, fonts: &FontBook, mode: LabelMode) -> NodeGeometry {
    let width = label.width + NOTE_PADDING * 2.0;
    let height = label.height + NOTE_PADDING * 2.0;
    let outline = rounded_rect(-width / 2.0, -height / 2.0, width, height, 0.0);
    let mut note = geometry(width, height, outline, Intersect::Rect);
    if mode == LabelMode::SvgText {
        note.label_offset_y =
            fonts.primary_metrics(crate::label::FONT_SIZE).ascent - crate::label::FONT_SIZE;
    }
    note
}

/// rectWithTitle: 제목 아래 구분선과 설명 줄. 설명 글자와 구분선은 장식으로 돌려준다
pub fn titled_geometry(
    title: &LabelBox,
    description: &LabelBox,
    fonts: &FontBook,
    style: (Color, Color),
) -> (NodeGeometry, Vec<SceneItem>) {
    let (line_color, text_color) = style;
    let half_padding = STATE_PADDING / 2.0;
    let width = title.width.max(description.width);
    let height = title.height + half_padding + 5.0 + description.height;
    let top = -height / 2.0 - 1.0;
    let outer_width = width + STATE_PADDING;
    let outer_height = height + STATE_PADDING;
    let outline = rounded_rect(
        -outer_width / 2.0,
        -outer_height / 2.0,
        outer_width,
        outer_height,
        5.0,
    );
    let mut titled = geometry(outer_width, outer_height, outline, Intersect::Rect);
    titled.label_offset_y = top + title.height / 2.0;
    let divider_y = -height / 2.0 + title.height;
    let mut decorations = vec![SceneItem::Path(PathItem {
        commands: vec![
            PathCommand::MoveTo(Point::new(-outer_width / 2.0, divider_y)),
            PathCommand::LineTo(Point::new(outer_width / 2.0, divider_y)),
        ],
        fill: None,
        stroke: Some(solid(line_color, 1.0)),
    })];
    let description_top = top + title.height + half_padding + 5.0;
    let texts =
        description.text_items(fonts, -description.width / 2.0, description_top, text_color);
    decorations.extend(texts.into_iter().map(SceneItem::Text));
    (titled, decorations)
}

fn solid(color: Color, width: f64) -> StrokeStyle {
    StrokeStyle {
        color,
        width,
        dash: Vec::new(),
    }
}

fn push_rect(
    group: &mut Group,
    frame: (f64, f64, f64, f64),
    fill: Color,
    stroke: StrokeStyle,
    radius: f64,
) {
    let (x, y, width, height) = frame;
    let commands = rounded_rect(x, y, width, height, radius);
    group.bounds.include_path(&commands);
    group.items.push(SceneItem::Path(PathItem {
        commands,
        fill: Some(fill),
        stroke: Some(stroke),
    }));
}

/// neo 복합 상태 상자: 그림자(바깥 상자만)와 그라디언트 테두리는 경계에 넣지 않는다
fn push_neo_rect(
    group: &mut Group,
    (frame, radius): ((f64, f64, f64, f64), f64),
    fill: Color,
    stroke: StrokeStyle,
    style: &ElementStyle,
) {
    let (x, y, width, height) = frame;
    let commands = rounded_rect(x, y, width, height, radius);
    if let Some(shadow) = style.drop_shadow.filter(|_| radius > 0.0) {
        group.items.extend(drop_shadow(&commands, shadow));
    }
    let Some(colors) = style.stroke_gradient else {
        push_rect(group, frame, fill, stroke, radius);
        return;
    };
    group.bounds.include_path(&commands);
    group.items.push(SceneItem::Path(PathItem {
        commands: commands.clone(),
        fill: Some(fill),
        stroke: None,
    }));
    group
        .items
        .extend(gradient_stroke(&commands, stroke.width, colors));
}

fn push_label(
    render: &FlowRender,
    group: &mut Group,
    label: &LabelBox,
    left: f64,
    top: f64,
    color: Color,
) {
    let box_top = label.box_top(render.fonts, top);
    group
        .bounds
        .include_rect(left, box_top, label.width, label.height);
    let texts: Vec<TextItem> = label.text_items(render.fonts, left, top, color);
    group.items.extend(texts.into_iter().map(SceneItem::Text));
}

/// roundedWithTitle: 둥근 바깥 상자, 제목 아래 안쪽 상자, 위쪽 가운데 제목
pub fn paint_composite(
    render: &FlowRender,
    group: &mut Group,
    flow: &FlowNode,
    frame: &BoundaryBox,
) {
    paint_titled_cluster(render, group, flow, frame, false);
}

/// 중첩된 복합 상태(statediagram-cluster-alt): 안쪽 상자 색만 다르다
pub fn paint_alt_composite(
    render: &FlowRender,
    group: &mut Group,
    flow: &FlowNode,
    frame: &BoundaryBox,
) {
    paint_titled_cluster(render, group, flow, frame, true);
}

fn paint_titled_cluster(
    render: &FlowRender,
    group: &mut Group,
    flow: &FlowNode,
    frame: &BoundaryBox,
    is_alt: bool,
) {
    let colors = StateColors::for_theme(render.palette.theme);
    let palette = render.palette;
    let x = frame.center.x - frame.width / 2.0;
    let y = frame.center.y - frame.height / 2.0;
    let stroke = solid(flow.style.stroke.unwrap_or(palette.node_stroke), 1.0);
    let fill = flow.style.fill.unwrap_or(palette.node_fill);
    let outer = ((x, y, frame.width, frame.height), 5.0);
    push_neo_rect(group, outer, fill, stroke.clone(), &flow.style);
    let label = &flow.label;
    let inner_y = y + label.height + 2.0;
    let inner_height = frame.height - label.height - 6.0;
    let inner_fill = if is_alt {
        colors.composite_alt_inner
    } else {
        colors.composite_inner
    };
    let inner = ((x, inner_y, frame.width, inner_height), 0.0);
    push_neo_rect(group, inner, inner_fill, stroke, &flow.style);
    push_cluster_label(render, group, flow, frame, y);
}

/// 복합 상태 제목: 바깥 상자 위 끝(SVG 글자는 3px 위) 가운데
pub fn push_cluster_label(
    render: &FlowRender,
    group: &mut Group,
    flow: &FlowNode,
    frame: &BoundaryBox,
    y: f64,
) {
    let colors = StateColors::for_theme(render.palette.theme);
    let label = &flow.label;
    let label_top = match label.mode {
        LabelMode::Html => y + 1.0,
        LabelMode::SvgText => y - 2.0,
    };
    let color = flow
        .style
        .text_color
        .unwrap_or(colors.label(render.palette, label.mode));
    let left = frame.center.x - label.width / 2.0;
    push_label(render, group, label, left, label_top, color);
}

/// `--` 동시 영역: 점선 테두리 상자
pub fn paint_divider(
    render: &FlowRender,
    group: &mut Group,
    _flow: &FlowNode,
    frame: &BoundaryBox,
) {
    let colors = StateColors::for_theme(render.palette.theme);
    let x = frame.center.x - frame.width / 2.0;
    let y = frame.center.y - frame.height / 2.0;
    let stroke = StrokeStyle {
        color: render.palette.node_stroke,
        width: 1.0,
        dash: vec![10.0, 10.0],
    };
    push_rect(
        group,
        (x, y, frame.width, frame.height),
        colors.divider_fill,
        stroke,
        0.0,
    );
}

/// noteGroup은 `fill="none"` 상자라 보이는 것이 없다. 경계만 그룹 크기에 넣는다
pub fn paint_note_group(
    _render: &FlowRender,
    group: &mut Group,
    _flow: &FlowNode,
    frame: &BoundaryBox,
) {
    let x = frame.center.x - frame.width / 2.0;
    let y = frame.center.y - frame.height / 2.0;
    group.bounds.include_rect(x, y, frame.width, frame.height);
}
