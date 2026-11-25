//! createText 한 줄의 크기와 글자 조각.
//!
//! HTML 라벨은 div 안 인라인 조각(strong·em)의 폭을 이어 붙이고, SVG 글자는 createFormattedText처럼 낱말마다
//! tspan(두 번째 낱말부터 앞에 공백)을 두어 getBBox(진행 폭과 잉크 상자의 합)를 잰다.

use super::markdown::{StyledSpan, parse_inline};
use crate::LabelMode;
use crate::scene::{Color, TextItem};
use crate::text::{FontBook, FontWeight};

/// 같은 굵기·기울임으로 이어 그리는 조각. `offset`은 줄 왼쪽에서 조각 시작까지의 거리다
#[derive(Clone, Debug)]
pub struct TextRun {
    pub text: String,
    pub weight: FontWeight,
    pub offset: f64,
}

/// 한 줄 글자와 그 bbox(글자 x 원점 기준 왼쪽 끝과 폭)
#[derive(Clone, Debug)]
pub struct MeasuredText {
    pub runs: Vec<TextRun>,
    pub left: f64,
    pub width: f64,
    /// 정적 멤버(`$`) 밑줄의 조각(bbox 왼쪽 기준 x 범위). 밑줄이 없으면 비어 있다
    pub underline: Vec<(f64, f64)>,
}

impl MeasuredText {
    pub fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }

    /// 줄 왼쪽 x와 기준선에 맞춘 조각별 글자
    pub fn text_items(&self, x: f64, baseline: f64, font_size: f64, color: Color) -> Vec<TextItem> {
        self.runs
            .iter()
            .map(|run| TextItem {
                text: run.text.clone(),
                x: x + run.offset,
                baseline,
                font_size,
                weight: run.weight,
                color,
                rotation: None,
            })
            .collect()
    }
}

/// markdown 없이 그대로 잰다(다중성 라벨의 createLabel)
pub fn measure_text(
    fonts: &FontBook,
    text: &str,
    font_size: f64,
    weight: FontWeight,
    mode: LabelMode,
) -> MeasuredText {
    let span = StyledSpan {
        text: text.to_string(),
        is_strong: false,
        is_emphasis: false,
    };
    measure_spans(fonts, &[span], font_size, weight, mode)
}

/// markdown 인라인 강조를 풀어 잰다(클래스 상자 글자·관계 라벨)
pub fn measure_markdown(
    fonts: &FontBook,
    text: &str,
    font_size: f64,
    weight: FontWeight,
    mode: LabelMode,
) -> MeasuredText {
    measure_spans(fonts, &parse_inline(text), font_size, weight, mode)
}

/// 바탕 굵기·기울임에 strong(굵게)·em(기울임)을 더한다
fn span_weight(base: FontWeight, span: &StyledSpan) -> FontWeight {
    let is_bold = span.is_strong || matches!(base, FontWeight::Bold | FontWeight::BoldItalic);
    let is_italic = span.is_emphasis || matches!(base, FontWeight::Italic | FontWeight::BoldItalic);
    match (is_bold, is_italic) {
        (true, true) => FontWeight::BoldItalic,
        (true, false) => FontWeight::Bold,
        (false, true) => FontWeight::Italic,
        (false, false) => FontWeight::Regular,
    }
}

fn measure_spans(
    fonts: &FontBook,
    spans: &[StyledSpan],
    font_size: f64,
    base: FontWeight,
    mode: LabelMode,
) -> MeasuredText {
    let mut runs = Vec::new();
    let (mut advance, mut left, mut right) = (0.0, 0.0_f64, 0.0_f64);
    for (text, weight) in pieces(spans, base, mode) {
        let (run_advance, run_left, run_right) = match mode {
            LabelMode::Html => {
                let width = fonts.measure_line(&text, font_size, weight);
                (width, 0.0, width)
            }
            LabelMode::SvgText => {
                let extent = fonts.measure_svg_runs(&[text.as_str()], font_size, weight);
                (extent.advance, extent.left, extent.right)
            }
        };
        left = left.min(advance + run_left);
        right = right.max(advance + run_right);
        runs.push(TextRun {
            text,
            weight,
            offset: advance,
        });
        advance += run_advance;
    }
    MeasuredText {
        runs,
        left,
        width: right.max(advance) - left,
        underline: Vec::new(),
    }
}

/// 잴 조각: HTML은 강조 조각 그대로, SVG 글자는 낱말 tspan(`+String`, ` name`)
fn pieces(spans: &[StyledSpan], base: FontWeight, mode: LabelMode) -> Vec<(String, FontWeight)> {
    if mode == LabelMode::Html {
        return spans
            .iter()
            .map(|span| (span.text.clone(), span_weight(base, span)))
            .collect();
    }
    let mut words = Vec::new();
    for span in spans {
        for word in span.text.split(' ').filter(|word| !word.is_empty()) {
            let text = if words.is_empty() {
                word.to_string()
            } else {
                format!(" {word}")
            };
            words.push((text, span_weight(base, span)));
        }
    }
    words
}
