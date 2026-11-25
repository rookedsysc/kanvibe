//! rendering-elements/shapes/requirementBox.ts: 종류(`<<Requirement>>`)·굵은 이름을 가운데에, 속성 줄을 왼쪽에
//! 맞춰 쌓고 이름 아래에 구분선을 긋는다.

use super::parser::{Element, Requirement};
use crate::LabelMode;
use crate::er::unified::{
    decorated_node, label_items, measure_single_line, rough_line, rough_rect,
};
use crate::flowchart::FlowNode;
use crate::label::{FONT_SIZE, LabelBox};
use crate::scene::{Color, Point, SceneItem};
use crate::text::{FontBook, FontWeight};

/// 상자 여백과 이름 아래 간격
const PADDING: f64 = 20.0;
const GAP: f64 = 20.0;
/// SVG 글자 방식에서 addText가 getBBox 높이에 더하는 값
const SVG_EXTRA_HEIGHT: f64 = 6.0;

pub struct BoxStyle<'a> {
    pub fonts: &'a FontBook,
    pub mode: LabelMode,
    pub fill: Color,
    pub border: Color,
    pub text: Color,
}

/// addText로 넣은 글자 하나: 라벨, 굵기, 높이(addText 반환값), 위로부터의 자리(yOffset)
struct BoxLine {
    label: LabelBox,
    weight: FontWeight,
    height: f64,
    offset: f64,
}

pub fn requirement_node(requirement: &Requirement, style: &BoxStyle) -> FlowNode {
    let details = [
        prefixed("ID", &requirement.requirement_id),
        prefixed("Text", &requirement.text),
        prefixed("Risk", &requirement.risk),
        prefixed("Verification", &requirement.verify_method),
    ];
    let kind = format!("<<{}>>", requirement.kind);
    box_node(&requirement.name, &kind, &details, style)
}

pub fn element_node(element: &Element, style: &BoxStyle) -> FlowNode {
    let details = [
        prefixed("Type", &element.element_type),
        prefixed("Doc Ref", &element.doc_ref),
    ];
    box_node(&element.name, "<<Element>>", &details, style)
}

fn prefixed(prefix: &str, value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        format!("{prefix}: {value}")
    }
}

/// 위에서부터 쌓은 글자 줄들. 마지막 속성 줄은 누적 높이에 넣지 않는다(원본 그대로)
struct BoxLines {
    lines: Vec<BoxLine>,
    /// 마지막 속성 줄을 뺀 누적 높이
    stacked_height: f64,
    /// 종류·이름·간격까지의 높이(구분선 자리)
    header_height: f64,
}

fn stack_lines(name: &str, kind: &str, details: &[String], style: &BoxStyle) -> BoxLines {
    let mut lines = Vec::new();
    let mut offset = 0.0;
    let mut push = |text: &str, weight: FontWeight, offset: f64| -> f64 {
        if text.is_empty() {
            return 0.0;
        }
        let label = measure_single_line(style.fonts, text, style.mode, weight);
        let height = match style.mode {
            LabelMode::Html => label.height,
            LabelMode::SvgText => label.height + SVG_EXTRA_HEIGHT,
        };
        lines.push(BoxLine {
            label,
            weight,
            height,
            offset,
        });
        height
    };
    offset += push(kind, FontWeight::Regular, offset);
    offset += push(name, FontWeight::Bold, offset) + GAP;
    let header_height = offset;
    let (last, stacked) = details.split_last().expect("속성 줄");
    for detail in stacked {
        offset += push(detail, FontWeight::Regular, offset);
    }
    push(last, FontWeight::Regular, offset);
    BoxLines {
        lines,
        stacked_height: offset,
        header_height,
    }
}

/// 처음 둔 자리(가운데 맞춤, 위 끝 = yOffset − 높이/2)에서 글자 상자의 세로 범위
fn ink_range(line: &BoxLine, mode: LabelMode, fonts: &FontBook) -> (f64, f64) {
    let top = line.offset - line.height / 2.0;
    match mode {
        LabelMode::Html => (top, top + line.label.height),
        LabelMode::SvgText => {
            let ascent = fonts.primary_metrics(FONT_SIZE).ascent;
            let box_top = top + FONT_SIZE - ascent;
            (box_top, box_top + line.label.height)
        }
    }
}

fn box_node(name: &str, kind: &str, details: &[String], style: &BoxStyle) -> FlowNode {
    let stacked = stack_lines(name, kind, details, style);
    let (min_y, max_y) = stacked
        .lines
        .iter()
        .map(|line| ink_range(line, style.mode, style.fonts))
        .fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(low, high), (top, bottom)| (low.min(top), high.max(bottom)),
        );
    let widest = stacked
        .lines
        .iter()
        .map(|line| line.label.width)
        .fold(0.0, f64::max);
    let width = widest + PADDING;
    let height = max_y - min_y + PADDING;
    let (x, y) = (-width / 2.0, -height / 2.0);
    let mut items: Vec<SceneItem> =
        rough_rect(x, y, width, height, style.fill, style.border).into();
    for (index, line) in stacked.lines.iter().enumerate() {
        let left = if index < 2 {
            -line.label.width / 2.0
        } else {
            x + PADDING / 2.0
        };
        let top = line.offset - line.height / 2.0 - height / 2.0 + PADDING;
        let origin = Point::new(left, top);
        items.extend(label_items(
            style.fonts,
            &line.label,
            origin,
            style.text,
            line.weight,
        ));
    }
    if stacked.stacked_height > stacked.header_height {
        let line_y = y + stacked.header_height;
        items.push(rough_line(
            Point::new(x, line_y),
            Point::new(x + width, line_y),
            style.border,
        ));
    }
    decorated_node(name.to_string(), width, height, items)
}
