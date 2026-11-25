//! `look: handDrawn` 상태 모양과 복합 상태 상자(roughjs 갈래). 노드 크기는 Mermaid updateNodeBounds처럼
//! 흔들린 경로의 `getBBox()`이고, 모양은 그 크기와 상관없이 노드 중심 기준으로 그린다.

use crate::flowchart::geometry::{BoundaryBox, path_bounds};
use crate::flowchart::render::Group;
use crate::flowchart::shapes::{Intersect, NodeGeometry};
use crate::flowchart::{FlowNode, FlowRender};
use crate::rough::{self, FillStyle, RoughOptions, RoughSet, Segment};
use crate::scene::{Color, Point, SceneItem};

use super::shapes::{StateColors, push_cluster_label};

const START_DIAMETER: f64 = 14.0;
/// choice 마름모 한 변(노드 크기도 이 값으로 고정된다)
const CHOICE_SIZE: f64 = 28.0;
/// 복합 상태 바깥 상자 반지름
const COMPOSITE_RADIUS: f64 = 10.0;

/// 손그림 모양에 쓰는 테마 색
pub struct SketchColors {
    /// lineColor
    pub line: Color,
    /// mainBkg
    pub fill: Color,
    /// nodeBorder
    pub border: Color,
    /// stateBorder ?? nodeBorder(끝 원 안쪽)
    pub end_inner: Color,
}

/// userNodeOverrides: 거칠기 0.7, mainBkg hachure(굵기 4, 간격 5.2), nodeBorder 1.3px 테두리
fn node_options(colors: &SketchColors, seed: i32) -> RoughOptions {
    RoughOptions {
        roughness: 0.7,
        fill: Some(colors.fill),
        fill_weight: 4.0,
        hachure_gap: 5.2,
        stroke: Some(colors.border),
        stroke_width: 1.3,
        ..RoughOptions::new(seed)
    }
}

/// 흔들린 경로들의 경계 상자 크기(getBBox 폭·높이)
pub fn sketch_size(sets: &[RoughSet]) -> (f64, f64) {
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for bounds in sets.iter().filter_map(|set| path_bounds(&set.commands)) {
        min_x = min_x.min(bounds.0);
        min_y = min_y.min(bounds.1);
        max_x = max_x.max(bounds.2);
        max_y = max_y.max(bounds.3);
    }
    (max_x - min_x, max_y - min_y)
}

pub fn into_items(sets: Vec<RoughSet>) -> Vec<SceneItem> {
    sets.into_iter().map(RoughSet::into_item).collect()
}

/// 흔들린 경로들을 장식으로 두고, 그 경계 상자를 노드 크기로 삼는다
fn sketched_geometry(
    sets: Vec<RoughSet>,
    intersect: fn(f64) -> Intersect,
) -> (NodeGeometry, Vec<SceneItem>) {
    let (width, height) = sketch_size(&sets);
    let geometry = NodeGeometry {
        width,
        height,
        outlines: Vec::new(),
        intersect: intersect(width),
        label_offset_y: 0.0,
    };
    (geometry, into_items(sets))
}

/// stateStart: solidStateFill(lineColor) 원(hachure 120°, 간격 4, 굵기 2)
pub fn start(colors: &SketchColors, seed: i32) -> (NodeGeometry, Vec<SceneItem>) {
    let options = RoughOptions {
        roughness: 0.7,
        fill: Some(colors.line),
        stroke: Some(colors.line),
        fill_weight: 2.0,
        hachure_angle: 120.0,
        hachure_gap: 4.0,
        ..RoughOptions::new(seed)
    };
    let sets = rough::circle(Point::new(0.0, 0.0), START_DIAMETER, &options);
    sketched_geometry(sets, |width| Intersect::Circle(width / 2.0))
}

/// stateEnd: lineColor 2px 바깥 원(mainBkg hachure)과 지름 5의 solid 안쪽 원
pub fn end(colors: &SketchColors, seed: i32) -> (NodeGeometry, Vec<SceneItem>) {
    let base = node_options(colors, seed);
    let outer = RoughOptions {
        stroke: Some(colors.line),
        stroke_width: 2.0,
        ..base.clone()
    };
    let inner = RoughOptions {
        fill: Some(colors.end_inner),
        stroke: Some(colors.end_inner),
        stroke_width: 2.0,
        fill_style: FillStyle::Solid,
        ..base
    };
    let center = Point::new(0.0, 0.0);
    let mut sets = rough::circle(center, START_DIAMETER, &outer);
    sets.extend(rough::circle(center, START_DIAMETER * 5.0 / 14.0, &inner));
    sketched_geometry(sets, |width| Intersect::Circle(width / 2.0))
}

/// choice: createPathFromPoints 마름모(`Z`로 닫는다). 노드 크기는 28로 고정이다
pub fn choice(colors: &SketchColors, seed: i32) -> Vec<SceneItem> {
    let half = CHOICE_SIZE / 2.0;
    let segments = [
        Segment::MoveTo(Point::new(0.0, half)),
        Segment::LineTo(Point::new(half, 0.0)),
        Segment::LineTo(Point::new(0.0, -half)),
        Segment::LineTo(Point::new(-half, 0.0)),
        Segment::Close,
    ];
    into_items(rough::path(&segments, &node_options(colors, seed)))
}

/// forkJoin: lineColor로 채운 막대 rectangle
pub fn bar(width: f64, height: f64, colors: &SketchColors, seed: i32) -> Vec<RoughSet> {
    let options = RoughOptions {
        fill: Some(colors.line),
        stroke: Some(colors.line),
        ..node_options(colors, seed)
    };
    let origin = Point::new(-width / 2.0, -height / 2.0);
    rough::rectangle(origin, width, height, &options)
}

/// note: noteBkgColor·noteBorderColor rectangle
pub fn note(width: f64, height: f64, (fill, stroke): (Color, Color), seed: i32) -> Vec<RoughSet> {
    let colors = SketchColors {
        line: stroke,
        fill,
        border: stroke,
        end_inner: stroke,
    };
    let origin = Point::new(-width / 2.0, -height / 2.0);
    rough::rectangle(origin, width, height, &node_options(&colors, seed))
}

/// 상태 상자(반지름 5)·설명 상자(반지름 10): createRoundedRectPathD 경로를 roughjs path로 그린다
pub fn rounded_box(
    (width, height): (f64, f64),
    radius: f64,
    colors: &SketchColors,
    seed: i32,
) -> Vec<RoughSet> {
    let segments = rounded_rect_segments(-width / 2.0, -height / 2.0, (width, height), radius);
    rough::path(&segments, &node_options(colors, seed))
}

/// rectWithTitle 구분선(rc.line, 노드 옵션)
pub fn divider_line(from: Point, to: Point, colors: &SketchColors, seed: i32) -> Vec<SceneItem> {
    into_items(rough::line(from, to, &node_options(colors, seed)))
}

/// createRoundedRectPathD: 네 모서리를 원호로 돌고 `Z`로 닫는다
fn rounded_rect_segments(x: f64, y: f64, (width, height): (f64, f64), radius: f64) -> Vec<Segment> {
    let corner = |end: Point| Segment::Arc {
        radius: Point::new(radius, radius),
        rotation: 0.0,
        is_large: false,
        is_sweep: true,
        end,
    };
    vec![
        Segment::MoveTo(Point::new(x + radius, y)),
        Segment::Horizontal(x + width - radius),
        corner(Point::new(x + width, y + radius)),
        Segment::Vertical(y + height - radius),
        corner(Point::new(x + width - radius, y + height)),
        Segment::Horizontal(x + radius),
        corner(Point::new(x, y + height - radius)),
        Segment::Vertical(y + radius),
        corner(Point::new(x + radius, y)),
        Segment::Close,
    ]
}

/// 흔들린 경로를 클러스터 그룹에 넣고 경계를 넓힌다
fn push_sets(group: &mut Group, sets: Vec<RoughSet>) {
    for set in sets {
        group.bounds.include_path(&set.commands);
        group.items.push(set.into_item());
    }
}

/// roundedWithTitle(handDrawn): 반지름 10의 solid 바깥 상자, 제목, 안쪽 rectangle(alt면 hachure)
pub fn paint_composite(
    render: &FlowRender,
    group: &mut Group,
    flow: &FlowNode,
    frame: &BoundaryBox,
) {
    paint_titled(render, group, flow, frame, false);
}

pub fn paint_alt_composite(
    render: &FlowRender,
    group: &mut Group,
    flow: &FlowNode,
    frame: &BoundaryBox,
) {
    paint_titled(render, group, flow, frame, true);
}

fn paint_titled(
    render: &FlowRender,
    group: &mut Group,
    flow: &FlowNode,
    frame: &BoundaryBox,
    is_alt: bool,
) {
    let seed = flow.style.sketch_seed.unwrap_or_default();
    let palette = render.palette;
    let colors = StateColors::for_theme(palette.theme);
    let x = frame.center.x - frame.width / 2.0;
    let y = frame.center.y - frame.height / 2.0;
    let outer = RoughOptions {
        roughness: 0.7,
        fill: Some(palette.node_fill),
        fill_style: FillStyle::Solid,
        stroke: Some(palette.node_stroke),
        ..RoughOptions::new(seed)
    };
    let segments = rounded_rect_segments(x, y, (frame.width, frame.height), COMPOSITE_RADIUS);
    push_sets(group, rough::path(&segments, &outer));
    push_cluster_label(render, group, flow, frame, y);
    let label_height = flow.label.height;
    let inner = RoughOptions {
        fill: Some(if is_alt {
            colors.composite_alt_inner
        } else {
            colors.composite_inner
        }),
        fill_style: if is_alt {
            FillStyle::Hachure
        } else {
            FillStyle::Solid
        },
        stroke: Some(palette.node_stroke),
        ..RoughOptions::new(seed)
    };
    let origin = Point::new(x, y + label_height + 2.0);
    let inner_height = frame.height - label_height - 6.0;
    push_sets(
        group,
        rough::rectangle(origin, frame.width, inner_height, &inner),
    );
}

/// `--` 동시 영역(handDrawn): lightgrey hachure, 거칠기 0.5, 점선(5) 테두리 rectangle
pub fn paint_divider(render: &FlowRender, group: &mut Group, flow: &FlowNode, frame: &BoundaryBox) {
    let options = RoughOptions {
        roughness: 0.5,
        fill: Some(Color::rgb(211, 211, 211)),
        stroke: Some(render.palette.node_stroke),
        stroke_dash: vec![5.0, 5.0],
        ..RoughOptions::new(flow.style.sketch_seed.unwrap_or_default())
    };
    let origin = Point::new(
        frame.center.x - frame.width / 2.0,
        frame.center.y - frame.height / 2.0,
    );
    push_sets(
        group,
        rough::rectangle(origin, frame.width, frame.height, &options),
    );
}

#[cfg(test)]
mod tests {
    use crate::text::tests::bundled_fonts;
    use crate::theme::Theme;
    use crate::{LabelMode, render_scene};

    #[test]
    fn 시드를_준_손그림_다이어그램이_기준_svg_viewbox와_같다() {
        let fonts = bundled_fonts();
        let source = include_str!("../../../../parity/diagrams/baseline/state-look-handdrawn.mmd");
        let scene = render_scene(
            source,
            Theme::Light,
            LabelMode::Html,
            crate::theme_variables::ThemeVariablesPolicy::Ignore,
            &fonts,
        )
        .unwrap();
        // 기준 SVG viewBox: -0.13153076171875 0.0988302230834961 74.14773559570312 397.78594970703125
        assert!((scene.width - 74.147_735).abs() < 0.001, "{}", scene.width);
        assert!(
            (scene.height - 397.785_95).abs() < 0.001,
            "{}",
            scene.height
        );
    }

    #[test]
    fn 손그림_모양_모음의_폭이_기준_이미지의_원래_폭과_같다() {
        let fonts = bundled_fonts();
        let source =
            include_str!("../../../../parity/diagrams/baseline/state-look-handdrawn-shapes.mmd");
        let scene = render_scene(
            source,
            Theme::Light,
            LabelMode::Html,
            crate::theme_variables::ThemeVariablesPolicy::Ignore,
            &fonts,
        )
        .unwrap();
        // 기준 PNG를 만든 렌더의 naturalWidth(viewBox 폭 올림)가 562다
        assert_eq!(scene.width.ceil(), 562.0, "{}", scene.width);
    }
}
