//! 강조(`**굵게**`·`*기울임*`)가 섞인 마인드맵 markdown 라벨의 줄바꿈과 크기.
//!
//! HTML 라벨은 `markdown-node-label` div가 자연 폭이 200px에 닿으면 폭 200px·`break-spaces`로 바뀌어 공백에서
//! 줄을 바꾸고 줄마다 가운데 맞춘다. SVG 글자 라벨은 createFormattedText가 낱말을 하나씩 붙여 보며
//! `getComputedTextLength()`(진행 폭)가 200px을 넘기 전에 줄을 바꾸고, 줄마다 원점(x = 0)에서 시작한다.

use crate::LabelMode;
use crate::class::markdown::{StyledSpan, parse_inline};
use crate::class::measure::{MeasuredText, measure_spans};
use crate::label::{
    FONT_SIZE, LINE_HEIGHT, LabelBox, LabelLine, SVG_LINE_HEIGHT_EM, WRAPPING_WIDTH,
};
use crate::text::{FontBook, FontWeight};

/// 강조 라벨: 일반 라벨처럼 쓰는 상자(줄 글자·폭)와 줄마다 굵기별로 잰 조각
pub(super) struct StyledLabel {
    pub label: LabelBox,
    pub lines: Vec<MeasuredText>,
}

/// 강조가 하나도 없으면 `None`(일반 라벨 경로를 쓴다)
pub(super) fn measure_styled_label(
    fonts: &FontBook,
    text: &str,
    mode: LabelMode,
) -> Option<StyledLabel> {
    let spans = parse_inline(text);
    if spans
        .iter()
        .all(|span| !span.is_strong && !span.is_emphasis)
    {
        return None;
    }
    let words = split_words(&spans);
    let measure =
        |line: &[StyledSpan]| measure_spans(fonts, line, FONT_SIZE, FontWeight::Regular, mode);
    let whole = measure(&join_words(&words));
    let line_words = match mode {
        LabelMode::Html if whole.width < WRAPPING_WIDTH => vec![words],
        LabelMode::Html => wrap_words(words, |line| measure(line).width),
        LabelMode::SvgText => wrap_words(words, |line| svg_advance(fonts, line)),
    };
    let is_html_wrapped = mode == LabelMode::Html && whole.width >= WRAPPING_WIDTH;
    let last_index = line_words.len().saturating_sub(1);
    let lines: Vec<MeasuredText> = line_words
        .iter()
        .enumerate()
        .map(|(index, words)| {
            let line = join_words(words);
            if !is_html_wrapped || index == last_index {
                return measure(&line);
            }
            with_trailing_space(&line, measure)
        })
        .collect();
    Some(StyledLabel {
        label: label_box(fonts, &lines, mode, is_html_wrapped),
        lines,
    })
}

/// `break-spaces` 줄은 줄 끝 공백을 걸치지 않고 그 폭을 차지해 가운데 맞춤이 공백 반만큼 왼쪽으로 간다.
/// 공백까지 넣어 폭을 넘으면 공백 없이 잰다
fn with_trailing_space(
    line: &[StyledSpan],
    measure: impl Fn(&[StyledSpan]) -> MeasuredText,
) -> MeasuredText {
    let mut spaced = line.to_vec();
    if let Some(last) = spaced.last_mut() {
        last.text.push(' ');
    }
    let measured = measure(&spaced);
    if measured.width <= WRAPPING_WIDTH {
        measured
    } else {
        measure(line)
    }
}

/// 낱말마다 강조를 붙인 조각. 공백은 버리고 줄을 만들 때 다시 넣는다
fn split_words(spans: &[StyledSpan]) -> Vec<StyledSpan> {
    spans
        .iter()
        .flat_map(|span| {
            span.text
                .split(' ')
                .filter(|word| !word.is_empty())
                .map(move |word| StyledSpan {
                    text: word.to_string(),
                    ..span.clone()
                })
        })
        .collect()
}

/// 낱말들을 공백으로 잇는다. 공백은 앞 낱말 조각 끝에 붙인다(marked가 강조 앞 공백을 앞 글자에 남기는 것과 같다)
fn join_words(words: &[StyledSpan]) -> Vec<StyledSpan> {
    let mut joined: Vec<StyledSpan> = Vec::new();
    for word in words {
        if let Some(last) = joined.last_mut() {
            last.text.push(' ');
            if is_same_style(last, word) {
                last.text.push_str(&word.text);
                continue;
            }
        }
        joined.push(word.clone());
    }
    joined
}

fn is_same_style(first: &StyledSpan, second: &StyledSpan) -> bool {
    first.is_strong == second.is_strong
        && first.is_emphasis == second.is_emphasis
        && first.is_strong_innermost == second.is_strong_innermost
}

/// 앞에서부터 낱말을 붙여 보다가 폭을 넘기면 새 줄을 연다. 한 낱말만으로 넘치면 그 낱말 혼자 한 줄이다
fn wrap_words(
    words: Vec<StyledSpan>,
    line_width: impl Fn(&[StyledSpan]) -> f64,
) -> Vec<Vec<StyledSpan>> {
    let mut lines: Vec<Vec<StyledSpan>> = Vec::new();
    let mut current: Vec<StyledSpan> = Vec::new();
    for word in words {
        current.push(word);
        let joined = join_words(&current);
        if current.len() > 1 && line_width(&joined) > WRAPPING_WIDTH {
            let overflow = current.pop().into_iter().collect();
            lines.push(std::mem::replace(&mut current, overflow));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// createFormattedText의 `getComputedTextLength()`: 낱말 tspan(두 번째부터 앞 공백)의 진행 폭 합
fn svg_advance(fonts: &FontBook, line: &[StyledSpan]) -> f64 {
    let pieces = measure_spans(
        fonts,
        line,
        FONT_SIZE,
        FontWeight::Regular,
        LabelMode::SvgText,
    );
    pieces
        .runs
        .iter()
        .map(|run| {
            fonts
                .measure_svg_runs(&[run.text.as_str()], FONT_SIZE, run.weight)
                .advance
        })
        .sum()
}

/// 일반 라벨과 같은 줄 높이 규칙으로 상자를 만든다. HTML은 줄을 바꾸면 폭이 200px로 고정된다
fn label_box(
    fonts: &FontBook,
    lines: &[MeasuredText],
    mode: LabelMode,
    is_html_wrapped: bool,
) -> LabelBox {
    let widest = lines.iter().map(|line| line.width).fold(0.0, f64::max);
    let count = lines.len() as f64;
    let (width, height) = match mode {
        LabelMode::Html if is_html_wrapped => (WRAPPING_WIDTH, count * LINE_HEIGHT),
        LabelMode::Html => (widest, LINE_HEIGHT),
        LabelMode::SvgText => {
            let metrics = fonts.primary_metrics(FONT_SIZE);
            let extra = FONT_SIZE * SVG_LINE_HEIGHT_EM * (count - 1.0);
            (widest, metrics.ascent + metrics.descent + extra)
        }
    };
    LabelBox {
        lines: lines
            .iter()
            .map(|line| LabelLine {
                text: line.text(),
                width: line.width,
            })
            .collect(),
        width,
        height,
        mode,
        font_size: FONT_SIZE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tests::bundled_fonts;

    const LONG_LABEL: &str =
        "A long label with **bold words** in the middle and *italic tail text*";

    fn line_texts(label: &StyledLabel) -> Vec<String> {
        label.lines.iter().map(MeasuredText::text).collect()
    }

    #[test]
    fn svg_글자는_진행_폭_200을_넘기_전에_낱말_단위로_줄을_바꾼다() {
        let fonts = bundled_fonts();
        let label = measure_styled_label(&fonts, LONG_LABEL, LabelMode::SvgText).unwrap();
        // 기준 SVG: 세 줄, 높이 57.2(22 + 1.1em × 2), 상자 폭 184.64
        assert_eq!(
            line_texts(&label),
            vec![
                "A long label with bold",
                "words in the middle and",
                "italic tail text"
            ]
        );
        assert!(
            (label.label.height - 57.2).abs() < 0.01,
            "{}",
            label.label.height
        );
        assert!(
            (label.label.width - 184.640_625).abs() < 0.5,
            "{}",
            label.label.width
        );
        // 기준 SVG 사각형 폭 234.015625 − 여백 40
        let rect_text = "**Strong start** then a long plain sentence that has to wrap *twice over*";
        let rect_label = measure_styled_label(&fonts, rect_text, LabelMode::SvgText).unwrap();
        assert_eq!(
            line_texts(&rect_label),
            vec![
                "Strong start then a long",
                "plain sentence that has to",
                "wrap twice over"
            ]
        );
        let width = rect_label.label.width;
        assert!((width - 194.015_625).abs() < 0.5, "{width}");
    }

    #[test]
    fn html_라벨은_폭_200에서_줄을_바꾼다() {
        let fonts = bundled_fonts();
        let label = measure_styled_label(&fonts, LONG_LABEL, LabelMode::Html).unwrap();
        assert_eq!(label.lines.len(), 3);
        assert_eq!((label.label.width, label.label.height), (200.0, 72.0));
        assert!(measure_styled_label(&fonts, "plain words", LabelMode::Html).is_none());
    }
}
