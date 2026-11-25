//! 배치 결과를 장면으로 옮긴다(svgDraw). 순서는 기준 SVG와 같다: 참여자 묶음, rect 배경, 아래 상자 모양 참여자,
//! 생명선과 위 상자 모양 참여자, 활성화·노트·블록, 위 그림 모양 참여자, 메시지, 아래 그림 모양 참여자, 제목.

use super::SequenceColors;
use super::arrows::draw_message;
use super::bounds::BlockFrame;
use super::config::{SequenceConfig, TextAlign};
use super::css_color::parse_css_color;
use super::document::{BlockKind, SequenceDocument};
use super::layout::{LayeredItem, PlacedGroup, PlacedTitle, SequenceLayout};
use super::measure::{FONT_SIZE, js_round, line_height, split_breaks};
use super::models::NoteModel;
use super::participants::{ParticipantPainter, is_boxed, lifeline_offset};
use crate::scene::{
    Color, PathCommand, Point, Scene, StrokeStyle, TextItem, polygon, rounded_rect,
};
use crate::text::{FontBook, FontWeight};

/// `dominant-baseline: middle`: 기준선이 x-height 절반만큼 아래다(Noto Sans x-height 536/1000)
const MIDDLE_BASELINE_SHIFT: f64 = FONT_SIZE * 0.536 / 2.0;
const LIFELINE_WIDTH: f64 = 2.0;
/// drawActor가 처음 긋는 생명선 끝 y
const UNMIRRORED_LIFELINE_END: f64 = 2000.0;
const BLOCK_LINE_WIDTH: f64 = 2.0;
/// 묶음 상자 그림자: 옮김, 흐림 표준편차(CSS drop-shadow의 흐림 길이), 그리는 범위와 층 수
const SHADOW_OFFSET: (f64, f64) = (1.0, 2.0);
const SHADOW_DEVIATION: f64 = 2.0;
const SHADOW_REACH: f64 = 6.5;
const SHADOW_STEPS: usize = 13;

pub(super) fn stroke(color: Color, width: f64, dash: &[f64]) -> Option<StrokeStyle> {
    Some(StrokeStyle {
        color,
        width,
        dash: dash.to_vec(),
    })
}

pub(super) fn line(from: Point, to: Point) -> Vec<PathCommand> {
    vec![PathCommand::MoveTo(from), PathCommand::LineTo(to)]
}

/// `text-anchor: middle` 한 줄
pub(super) fn centered_text(
    fonts: &FontBook,
    text: &str,
    center_x: f64,
    baseline: f64,
    font_size: f64,
    color: Color,
) -> TextItem {
    let width = fonts.shape_line(text, font_size, FontWeight::Regular).width;
    TextItem {
        text: text.to_string(),
        x: center_x - width / 2.0,
        baseline,
        font_size,
        weight: FontWeight::Regular,
        color,
        rotation: None,
    }
}

/// drawText의 가로 맞춤: 왼쪽은 여백만큼 안, 가운데는 폭 절반, 오른쪽은 끝에서 여백만큼 안이 글 기준점이다
pub(super) fn aligned_anchor(align: TextAlign, left: f64, width: f64, margin: f64) -> f64 {
    match align {
        TextAlign::Left => js_round(left + margin),
        TextAlign::Center => js_round(left + width / 2.0),
        TextAlign::Right => js_round(left + width - margin),
    }
}

/// `text-anchor`가 start·middle·end인 한 줄
pub(super) fn anchored_text(
    fonts: &FontBook,
    text: &str,
    (anchor_x, baseline): (f64, f64),
    align: TextAlign,
    color: Color,
) -> TextItem {
    let mut item = centered_text(fonts, text, anchor_x, baseline, FONT_SIZE, color);
    let width = fonts.shape_line(text, FONT_SIZE, FontWeight::Regular).width;
    item.x = match align {
        TextAlign::Left => anchor_x,
        TextAlign::Center => item.x,
        TextAlign::Right => anchor_x - width,
    };
    item
}

/// drawText의 세로 가운데 맞춤: i번째 줄 y는 앞 줄들 높이 합을 두 번 더한 값의 절반만큼 내려간다
pub(super) fn stacked_line_y(fonts: &FontBook, top: f64, index: usize, margin: f64) -> f64 {
    let above = line_height(fonts) * index as f64;
    js_round(top + (above + above + margin) / 2.0)
}

/// `dominant-baseline: middle`과 `dy` 뒤 기준선
pub(super) fn middle_baseline(y: f64, dy: f64) -> f64 {
    y + dy + MIDDLE_BASELINE_SHIFT
}

pub(super) fn draw(
    document: &SequenceDocument,
    layout: &SequenceLayout,
    fonts: &FontBook,
    colors: &SequenceColors,
    config: &SequenceConfig,
) -> Scene {
    let mut scene = empty_scene(layout);
    for group in &layout.groups {
        draw_group(&mut scene, fonts, colors, group, view_bounds(layout));
    }
    for frame in &layout.backgrounds {
        draw_background(&mut scene, frame);
    }
    let painters = |is_footer: bool| {
        document
            .participants
            .iter()
            .zip(&layout.actors)
            .map(move |(participant, actor)| ParticipantPainter {
                fonts,
                colors,
                participant,
                actor,
                is_footer,
            })
    };
    let is_mirrored = config.should_mirror_actors;
    if is_mirrored {
        paint_boxed(&mut scene, colors, painters(true).rev(), is_mirrored);
    }
    paint_boxed(&mut scene, colors, painters(false).rev(), is_mirrored);
    for item in &layout.layered {
        draw_layered(&mut scene, fonts, colors, config, item);
    }
    paint_figures(&mut scene, painters(false));
    for message in &layout.messages {
        draw_message(&mut scene, fonts, colors, config, &layout.actors, message);
    }
    if is_mirrored {
        paint_figures(&mut scene, painters(true));
    }
    if let Some(title) = &layout.title {
        draw_title(&mut scene, fonts, colors, title);
    }
    scene.translate(-layout.view_x, -layout.view_y);
    scene
}

fn empty_scene(layout: &SequenceLayout) -> Scene {
    Scene {
        is_start_aligned: false,
        width: layout.view_width,
        height: layout.view_height,
        items: Vec::new(),
    }
}

/// 노트·블록 아래로 내려 그리는 상자 모양 참여자. 위 참여자는 생명선을 먼저 긋는다
fn paint_boxed<'a>(
    scene: &mut Scene,
    colors: &SequenceColors,
    painters: impl Iterator<Item = ParticipantPainter<'a>>,
    is_mirrored: bool,
) {
    for painter in painters {
        if !painter.is_footer {
            draw_lifeline(scene, colors, &painter, is_mirrored);
        }
        if is_boxed(painter.participant.kind) {
            painter.paint(scene);
        }
    }
}

/// 노트·블록 위에 그리는 그림 모양 참여자
fn paint_figures<'a>(scene: &mut Scene, painters: impl Iterator<Item = ParticipantPainter<'a>>) {
    for painter in painters {
        if !is_boxed(painter.participant.kind) {
            painter.paint(scene);
        }
    }
}

fn draw_background(scene: &mut Scene, frame: &BlockFrame) {
    let (Some(start_x), Some(stop_x), Some(stop_y)) = (frame.start_x, frame.stop_x, frame.stop_y)
    else {
        return;
    };
    let fill = frame.fill.as_deref().and_then(parse_css_color);
    let corners = [
        Point::new(start_x, frame.start_y),
        Point::new(stop_x, frame.start_y),
        Point::new(stop_x, stop_y),
        Point::new(start_x, stop_y),
    ];
    scene.push_path(polygon(&corners), fill, None);
}

/// 생명선: 위 참여자 아래 끝부터 아래 참여자(또는 소멸 자리)까지. 거울 참여자가 없으면 소멸 자리 가운데나
/// 처음 그은 끝(y = 2000, viewBox 밖은 잘린다)까지다
fn draw_lifeline(
    scene: &mut Scene,
    colors: &SequenceColors,
    painter: &ParticipantPainter,
    is_mirrored: bool,
) {
    let actor = painter.actor;
    let center = actor.center();
    let top = actor.start_y + lifeline_offset(painter.participant.kind, actor);
    let bottom = match actor.stop_y {
        Some(stop_y) if is_mirrored => stop_y,
        Some(stop_y) => stop_y + actor.height / 2.0,
        None if is_mirrored => top,
        None => UNMIRRORED_LIFELINE_END,
    };
    scene.push_path(
        line(Point::new(center, top), Point::new(center, bottom)),
        None,
        stroke(colors.actor_line, LIFELINE_WIDTH, &[]),
    );
}

/// drawBox: 묶음 상자(`g rect.rect`라 테두리 색과 그림자가 붙는다)와 가운데 위 제목
/// viewBox 경계(왼쪽 위, 오른쪽 아래). SVG 뷰포트 밖은 잘린다
fn view_bounds(layout: &SequenceLayout) -> ViewBounds {
    ViewBounds {
        left: layout.view_x,
        top: layout.view_y,
        right: layout.view_x + layout.view_width,
        bottom: layout.view_y + layout.view_height,
    }
}

#[derive(Clone, Copy)]
struct ViewBounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl ViewBounds {
    /// 사각형을 뷰포트 안으로 자른다
    fn clipped_rect(&self, left: f64, top: f64, right: f64, bottom: f64) -> Vec<PathCommand> {
        let (left, top) = (left.max(self.left), top.max(self.top));
        let (right, bottom) = (right.min(self.right), bottom.min(self.bottom));
        rounded_rect(
            left,
            top,
            (right - left).max(0.0),
            (bottom - top).max(0.0),
            0.0,
        )
    }
}

fn draw_group(
    scene: &mut Scene,
    fonts: &FontBook,
    colors: &SequenceColors,
    group: &PlacedGroup,
    view: ViewBounds,
) {
    let (x, y) = (group.start_x, group.start_y);
    let (width, height) = (group.stop_x - x, group.stop_y - y);
    let fill = parse_css_color(&group.fill).filter(|color| color.alpha > 0.0);
    let shadow = Shadow {
        color: colors.shadow,
        view,
    };
    shadow.draw(scene, (x, y, width, height), fill.is_some());
    scene.push_path(
        rounded_rect(x, y, width, height, 0.0),
        fill,
        stroke(colors.group_stroke, 1.0, &[]),
    );
    let Some(title) = &group.title else {
        return;
    };
    let metrics = fonts.primary_metrics(FONT_SIZE);
    let central_shift = (metrics.ascent - metrics.descent) / 2.0;
    let lines = split_breaks(title);
    let middle = (lines.len() as f64 - 1.0) / 2.0;
    for (index, text) in lines.iter().enumerate() {
        let y = group.title_center_y + (index as f64 - middle) * FONT_SIZE;
        scene.push_text(centered_text(
            fonts,
            text,
            group.title_center_x,
            y + central_shift,
            FONT_SIZE,
            colors.root_fill,
        ));
    }
}

/// `drop-shadow(1px 2px 2px)`: (1, 2)만큼 옮긴 모양을 표준편차 2px로 흐린 그림자. 흐린 가장자리를 1px 간격으로
/// 겹친 반투명 경로로 나눠 그리고, 뷰포트 밖은 자른다
struct Shadow {
    color: Color,
    view: ViewBounds,
}

impl Shadow {
    fn draw(
        &self,
        scene: &mut Scene,
        (x, y, width, height): (f64, f64, f64, f64),
        is_filled: bool,
    ) {
        let (x, y) = (x + SHADOW_OFFSET.0, y + SHADOW_OFFSET.1);
        if is_filled {
            self.draw_filled(scene, (x, y, width, height));
        } else {
            self.draw_outline(scene, (x, y, width, height));
        }
    }

    /// 테두리 반폭(0.5)까지 채운 모양의 가장자리에서 바깥으로 멀수록 옅어진다
    fn draw_filled(&self, scene: &mut Scene, (x, y, width, height): (f64, f64, f64, f64)) {
        let spreads: Vec<f64> = (0..SHADOW_STEPS)
            .map(|step| SHADOW_REACH - step as f64)
            .collect();
        let coverages = spreads
            .iter()
            .map(|spread| 1.0 - normal_cdf((spread - 0.5) / SHADOW_DEVIATION));
        for (spread, alpha) in spreads.iter().zip(layer_alphas(coverages)) {
            let grown = spread + 0.5;
            let layer =
                self.view
                    .clipped_rect(x - grown, y - grown, x + width + grown, y + height + grown);
            scene.push_path(
                layer,
                Some(Color {
                    alpha,
                    ..self.color
                }),
                None,
            );
        }
    }

    /// 1px 테두리를 흐리면 테두리에서 거리 d의 덮임은 Φ((d+0.5)/σ) − Φ((d−0.5)/σ)이다
    fn draw_outline(&self, scene: &mut Scene, (x, y, width, height): (f64, f64, f64, f64)) {
        let distances: Vec<f64> = (0..=SHADOW_REACH.floor() as usize)
            .rev()
            .map(|distance| distance as f64)
            .collect();
        let coverages = distances.iter().map(|distance| {
            normal_cdf((distance + 0.5) / SHADOW_DEVIATION)
                - normal_cdf((distance - 0.5) / SHADOW_DEVIATION)
        });
        for (distance, alpha) in distances.iter().zip(layer_alphas(coverages)) {
            let ring = stroke(
                Color {
                    alpha,
                    ..self.color
                },
                2.0 * distance + 1.0,
                &[],
            );
            scene.push_path(rounded_rect(x, y, width, height, 0.0), None, ring);
        }
    }
}

/// 바깥부터 겹쳐 칠할 때 각 띠가 목표 덮임에 닿도록 하는 층별 불투명도
fn layer_alphas(coverages: impl Iterator<Item = f64>) -> Vec<f32> {
    let mut covered = 0.0;
    coverages
        .map(|target: f64| {
            let target = target.clamp(covered, 0.999);
            let alpha = 1.0 - (1.0 - target) / (1.0 - covered);
            covered = target;
            alpha as f32
        })
        .collect()
}

/// 표준 정규 누적 분포(Abramowitz–Stegun 7.1.26 erf 근사)
fn normal_cdf(value: f64) -> f64 {
    let z = value.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let polynomial = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - polynomial * (-z * z).exp();
    if value >= 0.0 {
        0.5 * (1.0 + erf)
    } else {
        0.5 * (1.0 - erf)
    }
}

/// 그림 제목: viewBox 위쪽 여백 안, 왼쪽 정렬
fn draw_title(scene: &mut Scene, fonts: &FontBook, colors: &SequenceColors, title: &PlacedTitle) {
    let width = fonts
        .shape_line(&title.text, FONT_SIZE, FontWeight::Regular)
        .width;
    let mut item = centered_text(
        fonts,
        &title.text,
        title.x + width / 2.0,
        title.baseline,
        FONT_SIZE,
        colors.root_fill,
    );
    item.x = title.x;
    scene.push_text(item);
}

fn draw_layered(
    scene: &mut Scene,
    fonts: &FontBook,
    colors: &SequenceColors,
    config: &SequenceConfig,
    item: &LayeredItem,
) {
    match item {
        LayeredItem::Activation {
            x,
            y,
            width,
            height,
        } => scene.push_path(
            rounded_rect(*x, *y, *width, *height, 0.0),
            Some(colors.activation_fill),
            stroke(colors.activation_stroke, 1.0, &[]),
        ),
        LayeredItem::Note {
            model,
            start_y,
            height,
        } => draw_note(scene, fonts, colors, config, model, (*start_y, *height)),
        LayeredItem::Block { frame, kind } => {
            draw_block(scene, fonts, colors, config, frame, *kind)
        }
    }
}

/// drawNote: 줄마다 noteAlign 맞춤, `dy = 1em`, `dominant-baseline: middle`
fn draw_note(
    scene: &mut Scene,
    fonts: &FontBook,
    colors: &SequenceColors,
    config: &SequenceConfig,
    model: &NoteModel,
    (start_y, height): (f64, f64),
) {
    scene.push_path(
        rounded_rect(model.start_x, start_y, model.width, height, 0.0),
        Some(colors.note_fill),
        stroke(colors.note_stroke, 1.0, &[]),
    );
    let (align, margin) = (config.note_align, config.note_margin);
    let anchor_x = aligned_anchor(align, model.start_x, model.width, margin);
    for (index, line_text) in split_breaks(&model.text).iter().enumerate() {
        let y = stacked_line_y(fonts, start_y, index, margin);
        let position = (anchor_x, middle_baseline(y, FONT_SIZE));
        scene.push_text(anchored_text(
            fonts,
            line_text,
            position,
            align,
            colors.note_text,
        ));
    }
}

/// drawLoop: 점선 테두리, 구역 점선, 왼쪽 위 이름표, 가운데 제목들
fn draw_block(
    scene: &mut Scene,
    fonts: &FontBook,
    colors: &SequenceColors,
    config: &SequenceConfig,
    frame: &BlockFrame,
    kind: BlockKind,
) {
    let (Some(start_x), Some(stop_x), Some(stop_y)) = (frame.start_x, frame.stop_x, frame.stop_y)
    else {
        return;
    };
    let start_y = frame.start_y;
    let border = stroke(colors.block_line, BLOCK_LINE_WIDTH, &[2.0, 2.0]);
    let edges = [
        (Point::new(start_x, start_y), Point::new(stop_x, start_y)),
        (Point::new(stop_x, start_y), Point::new(stop_x, stop_y)),
        (Point::new(start_x, stop_y), Point::new(stop_x, stop_y)),
        (Point::new(start_x, start_y), Point::new(start_x, stop_y)),
    ];
    for (from, to) in edges {
        scene.push_path(line(from, to), None, border.clone());
    }
    let divider = stroke(colors.block_line, BLOCK_LINE_WIDTH, &[3.0, 3.0]);
    for section in &frame.sections {
        let (from, to) = (
            Point::new(start_x, section.y),
            Point::new(stop_x, section.y),
        );
        scene.push_path(line(from, to), None, divider.clone());
    }
    let corner = Point::new(start_x, start_y);
    draw_block_label(scene, fonts, colors, config, corner, kind);
    let title_x = start_x + config.label_box_width / 2.0 + (stop_x - start_x) / 2.0;
    let title_anchor = Point::new(title_x, start_y);
    draw_block_title(scene, fonts, colors, config, &frame.title, title_anchor);
    let section_x = start_x + (stop_x - start_x) / 2.0;
    for section in &frame.sections {
        let anchor = Point::new(section_x, section.y);
        draw_block_title(scene, fonts, colors, config, &section.title, anchor);
    }
}

/// drawLabel: 오른쪽 아래 모서리를 깎은 이름표와 그 가운데 블록 낱말
fn draw_block_label(
    scene: &mut Scene,
    fonts: &FontBook,
    colors: &SequenceColors,
    config: &SequenceConfig,
    corner: Point,
    kind: BlockKind,
) {
    let (x, y) = (corner.x, corner.y);
    let (width, height, cut) = (config.label_box_width, config.label_box_height, 7.0);
    let points = [
        Point::new(x, y),
        Point::new(x + width, y),
        Point::new(x + width, y + height - cut),
        Point::new(x + width - cut * 1.2, y + height),
        Point::new(x, y + height),
    ];
    scene.push_path(
        polygon(&points),
        Some(colors.label_box_fill),
        stroke(colors.label_box_stroke, 1.0, &[]),
    );
    let text_y = js_round(y + height / 2.0 + config.box_text_margin / 2.0);
    scene.push_text(centered_text(
        fonts,
        kind.keyword(),
        js_round(x + width / 2.0),
        middle_baseline(text_y, 0.0),
        FONT_SIZE,
        colors.block_label_text,
    ));
}

/// 블록·구역 제목: 위 끝에서 boxMargin + boxTextMargin 아래, 알파벳 기준선
fn draw_block_title(
    scene: &mut Scene,
    fonts: &FontBook,
    colors: &SequenceColors,
    config: &SequenceConfig,
    title: &str,
    anchor: Point,
) {
    let top = anchor.y + config.box_margin + config.box_text_margin;
    for (index, text) in split_breaks(title).iter().enumerate() {
        if text.is_empty() {
            continue;
        }
        let baseline = stacked_line_y(fonts, top, index, config.box_text_margin);
        scene.push_text(centered_text(
            fonts,
            text,
            anchor.x,
            baseline,
            FONT_SIZE,
            colors.block_text,
        ));
    }
}
