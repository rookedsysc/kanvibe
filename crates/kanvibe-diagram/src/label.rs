//! Mermaid `createText()` 라벨 크기 계산. 두 방식이 있다.
//!
//! - HTML 라벨(`htmlLabels: true`, 모바일 워커): foreignObject 안의 div를 `getBoundingClientRect()`로 잰다.
//!   줄 높이 1.5(24px), `white-space: nowrap`, 최대 너비 200px. 자연 폭이 200px에 닿으면 `break-spaces`로 줄을 바꾼다.
//! - SVG 글자 라벨(`htmlLabels: false`, 데스크톱 화면): `createFormattedText()`가 낱말을 공백으로 이어 붙이며
//!   200px을 넘기 전에 줄을 바꾸고, 줄마다 `tspan`(y = (1.1·i − 0.1)em, dy = 1.1em)을 둔다. 크기는 `getBBox()`라
//!   높이가 `ascent + descent + 1.1em·(줄 수 − 1)`이다.

use crate::LabelMode;
use crate::scene::{Color, TextItem};
use crate::text::{FontBook, FontWeight, SvgTextExtent, snap_layout_unit};

pub const FONT_SIZE: f64 = 16.0;
pub const LINE_HEIGHT: f64 = 24.0;
pub const WRAPPING_WIDTH: f64 = 200.0;
/// SVG 글자 라벨의 줄 간격(em)
pub const SVG_LINE_HEIGHT_EM: f64 = 1.1;

#[derive(Clone, Debug, Default)]
pub struct LabelLine {
    pub text: String,
    pub width: f64,
}

#[derive(Clone, Debug)]
pub struct LabelBox {
    pub lines: Vec<LabelLine>,
    pub width: f64,
    pub height: f64,
    pub mode: LabelMode,
    /// 글자 크기(px). CSS로 라벨 글자를 줄이는 다이어그램(erDiagram 간선 라벨 14px 등)이 쓴다
    pub font_size: f64,
}

impl Default for LabelBox {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            width: 0.0,
            height: 0.0,
            mode: LabelMode::default(),
            font_size: FONT_SIZE,
        }
    }
}

impl LabelBox {
    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|line| line.text.is_empty())
    }

    /// 라벨 묶음 원점이 `top`일 때 getBBox 상자의 위 끝. SVG 글자는 첫 기준선이 원점에서 1em 아래라서
    /// 글자 상자가 `1em − ascent`만큼 내려가거나 올라간다
    pub fn box_top(&self, fonts: &FontBook, top: f64) -> f64 {
        match self.mode {
            LabelMode::Html => top,
            LabelMode::SvgText => {
                top + self.font_size - fonts.primary_metrics(self.font_size).ascent
            }
        }
    }

    /// 라벨 상자 왼쪽 위가 (left, top)일 때 줄마다 가운데 맞춘 글자
    pub fn text_items(&self, fonts: &FontBook, left: f64, top: f64, color: Color) -> Vec<TextItem> {
        match self.mode {
            LabelMode::Html => self.html_text_items(fonts, left, top, color),
            LabelMode::SvgText => self.svg_text_items(left, top, color),
        }
    }

    /// SVG 글자 라벨: 라벨 묶음을 상자 위 끝(translate(0, −높이/2))에 두므로 i번째 줄 기준선이 위 끝에서
    /// (1 + 1.1·i)em 아래다. 가로는 `text-anchor: middle`
    fn svg_text_items(&self, left: f64, top: f64, color: Color) -> Vec<TextItem> {
        let center = left + self.width / 2.0;
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, line)| !line.text.is_empty())
            .map(|(index, line)| TextItem {
                text: line.text.clone(),
                x: center - line.width / 2.0,
                baseline: top + self.font_size * (1.0 + SVG_LINE_HEIGHT_EM * index as f64),
                font_size: self.font_size,
                weight: FontWeight::Regular,
                color,
                rotation: None,
            })
            .collect()
    }

    fn html_text_items(
        &self,
        fonts: &FontBook,
        left: f64,
        top: f64,
        color: Color,
    ) -> Vec<TextItem> {
        let metrics = fonts.primary_metrics(self.font_size);
        let line_height = html_line_height(self.font_size);
        let half_leading = ((line_height - (metrics.ascent + metrics.descent)) / 2.0).floor();
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, line)| !line.text.is_empty())
            .map(|(index, line)| TextItem {
                text: line.text.clone(),
                x: left + snap_layout_unit((self.width - line.width) / 2.0),
                baseline: top + index as f64 * line_height + half_leading + metrics.ascent,
                font_size: self.font_size,
                weight: FontWeight::Regular,
                color,
                rotation: None,
            })
            .collect()
    }
}

/// `<br>` 줄바꿈과 몇 가지 엔티티만 남기고 나머지 태그는 글자로 쓰지 않는다
pub fn label_lines(label: &str) -> Vec<String> {
    let normalized = label
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<br>", "\n");
    normalized
        .split('\n')
        .map(|line| decode_entities(&strip_tags(line)))
        .collect()
}

fn strip_tags(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut is_in_tag = false;
    for character in line.chars() {
        match character {
            '<' => is_in_tag = true,
            '>' if is_in_tag => is_in_tag = false,
            _ if !is_in_tag => result.push(character),
            _ => {}
        }
    }
    result
}

fn decode_entities(line: &str) -> String {
    line.replace("#quot;", "\"")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", "\u{a0}")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// 호출한 쪽의 라벨 방식으로 잰다(글자 16px, 줄바꿈 폭 200px)
pub fn measure_label(fonts: &FontBook, label: &str, mode: LabelMode) -> LabelBox {
    measure_label_with_font_size(fonts, label, mode, FONT_SIZE)
}

/// [`measure_label`]과 같되 글자 크기를 정한다(줄 높이는 HTML이면 1.5배)
pub fn measure_label_with_font_size(
    fonts: &FontBook,
    label: &str,
    mode: LabelMode,
    font_size: f64,
) -> LabelBox {
    match mode {
        LabelMode::Html => measure_html_label(fonts, label, font_size, WRAPPING_WIDTH),
        LabelMode::SvgText => measure_svg_label(fonts, label, font_size, WRAPPING_WIDTH),
    }
}

/// HTML 라벨 div의 `line-height: 1.5`
fn html_line_height(font_size: f64) -> f64 {
    LINE_HEIGHT * font_size / FONT_SIZE
}

/// SVG 글자 라벨 크기. `wrap_width`가 무한대면 줄을 바꾸지 않는다(createText의 `width`).
/// 줄의 `width`는 가운데 맞춤에 쓰는 진행 폭이고, 상자 폭은 잉크까지 넣은 `getBBox()` 폭이다
pub fn measure_svg_label(
    fonts: &FontBook,
    label: &str,
    font_size: f64,
    wrap_width: f64,
) -> LabelBox {
    let lines: Vec<LabelLine> = label_lines(label)
        .iter()
        .flat_map(|line| wrap_words(fonts, line, font_size, wrap_width))
        .collect();
    let box_width = lines
        .iter()
        .map(|line| svg_line_extent(fonts, &line.text, font_size).width())
        .fold(0.0, f64::max);
    let metrics = fonts.primary_metrics(font_size);
    let is_blank = lines.iter().all(|line| line.text.is_empty());
    let height = if is_blank {
        0.0
    } else {
        metrics.ascent
            + metrics.descent
            + font_size * SVG_LINE_HEIGHT_EM * (lines.len() as f64 - 1.0)
    };
    LabelBox {
        width: box_width,
        height,
        lines,
        mode: LabelMode::SvgText,
        font_size,
    }
}

/// updateTextContentAndStyles: 낱말마다 안쪽 `tspan`을 두고 두 번째 낱말부터 앞에 공백을 붙인다
fn svg_line_extent(fonts: &FontBook, line: &str, font_size: f64) -> SvgTextExtent {
    let runs: Vec<String> = line
        .split(' ')
        .filter(|word| !word.is_empty())
        .enumerate()
        .map(|(index, word)| {
            if index == 0 {
                word.to_string()
            } else {
                format!(" {word}")
            }
        })
        .collect();
    let runs: Vec<&str> = runs.iter().map(String::as_str).collect();
    fonts.measure_svg_runs(&runs, font_size, FontWeight::Regular)
}

/// splitLineToFitWidth: 공백으로 나눈 낱말을 폭 안에서 이어 붙이고, 한 낱말이 넘치면 글자 단위로 자른다
fn wrap_words(fonts: &FontBook, line: &str, font_size: f64, wrap_width: f64) -> Vec<LabelLine> {
    let measure = |text: &str| svg_line_extent(fonts, text, font_size).advance;
    let mut lines: Vec<LabelLine> = Vec::new();
    let mut current = String::new();
    for word in line.split(' ').filter(|word| !word.is_empty()) {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if measure(&candidate) <= wrap_width {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(LabelLine {
                width: measure(&current),
                text: std::mem::take(&mut current),
            });
        }
        current = split_long_word(word, wrap_width, &measure, &mut lines);
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(LabelLine {
            width: measure(&current),
            text: current,
        });
    }
    lines
}

/// splitWordToFitWidth: 넘치는 낱말을 폭에 맞는 앞부분들로 잘라 넣고 남은 꼬리를 돌려준다
fn split_long_word(
    word: &str,
    wrap_width: f64,
    measure: &dyn Fn(&str) -> f64,
    lines: &mut Vec<LabelLine>,
) -> String {
    let mut rest = word.to_string();
    while measure(&rest) > wrap_width {
        let mut head = String::new();
        for character in rest.chars() {
            let mut next = head.clone();
            next.push(character);
            if !head.is_empty() && measure(&next) > wrap_width {
                break;
            }
            head = next;
        }
        rest = rest[head.len()..].to_string();
        lines.push(LabelLine {
            width: measure(&head),
            text: head,
        });
        if rest.is_empty() {
            break;
        }
    }
    rest
}

/// HTML 라벨 크기를 div `max-width`가 `wrap_width`일 때로 잰다(kanban 카드는 175px)
pub fn measure_html_label(
    fonts: &FontBook,
    label: &str,
    font_size: f64,
    wrap_width: f64,
) -> LabelBox {
    let raw_lines = label_lines(label);
    let measured: Vec<LabelLine> = raw_lines
        .into_iter()
        .map(|text| {
            let width = fonts.measure_line(&text, font_size, FontWeight::Regular);
            LabelLine { text, width }
        })
        .collect();
    let natural = measured.iter().map(|line| line.width).fold(0.0, f64::max);
    let line_height = html_line_height(font_size);
    if natural < wrap_width {
        let height = if label.is_empty() {
            0.0
        } else {
            measured.len() as f64 * line_height
        };
        return LabelBox {
            width: natural,
            height,
            lines: measured,
            mode: LabelMode::Html,
            font_size,
        };
    }
    let lines: Vec<LabelLine> = measured
        .iter()
        .flat_map(|line| wrap_line(fonts, &line.text, font_size, wrap_width))
        .collect();
    LabelBox {
        width: wrap_width,
        height: lines.len() as f64 * line_height,
        lines,
        mode: LabelMode::Html,
        font_size,
    }
}

/// 한글·한자 사이와 공백 뒤에서 줄을 바꿀 수 있다(Chromium 기본 줄바꿈 규칙의 근사)
fn break_opportunities(text: &str) -> Vec<usize> {
    let characters: Vec<(usize, char)> = text.char_indices().collect();
    let mut breaks = Vec::new();
    for window in characters.windows(2) {
        let (_, previous) = window[0];
        let (index, current) = window[1];
        let is_space_break = previous == ' ' && current != ' ';
        let is_ideographic = is_cjk(previous) || is_cjk(current);
        if is_space_break || (is_ideographic && current != ' ' && !is_closing_punctuation(current))
        {
            breaks.push(index);
        }
    }
    breaks
}

fn is_cjk(character: char) -> bool {
    matches!(character as u32, 0x1100..=0x11FF | 0x3040..=0x30FF | 0x3130..=0x318F | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF)
}

fn is_closing_punctuation(character: char) -> bool {
    matches!(
        character,
        '.' | ',' | ')' | ']' | '}' | '!' | '?' | ':' | ';' | '·'
    )
}

fn wrap_line(fonts: &FontBook, text: &str, font_size: f64, wrap_width: f64) -> Vec<LabelLine> {
    let mut lines = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let rest = &text[start..];
        let mut end = rest.len();
        if fonts.measure_line(rest.trim_end(), font_size, FontWeight::Regular) > wrap_width {
            let candidates = break_opportunities(rest);
            let fitting = candidates
                .iter()
                .copied()
                .take_while(|index| {
                    fonts.measure_line(rest[..*index].trim_end(), font_size, FontWeight::Regular)
                        <= wrap_width
                })
                .last();
            end = fitting
                .or_else(|| candidates.first().copied())
                .unwrap_or(rest.len());
        }
        let line = rest[..end].trim_end().to_string();
        let width = fonts.measure_line(&line, font_size, FontWeight::Regular);
        lines.push(LabelLine { text: line, width });
        start += end;
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tests::bundled_fonts;

    #[test]
    fn 기준_라벨_폭과_높이를_잰다() {
        let fonts = bundled_fonts();
        let label = measure_label(&fonts, "Valid?", LabelMode::Html);
        assert_eq!(label.width, 43.296875);
        assert_eq!(label.height, 24.0);
        assert_eq!(measure_label(&fonts, "", LabelMode::Html).height, 0.0);
    }

    #[test]
    fn br은_줄을_나눈다() {
        let fonts = bundled_fonts();
        let label = measure_label(&fonts, "local·CI<br/>PostgreSQL 검증", LabelMode::Html);
        assert_eq!(label.lines.len(), 2);
        assert_eq!(label.height, 48.0);
    }

    #[test]
    fn svg_글자_라벨은_getbbox_높이를_쓴다() {
        let fonts = bundled_fonts();
        // fc-korean-lr--desktop 기준 SVG: 한 줄 22, 두 줄 39.6, "prod 설정 변경 PR 승인" 폭 160.28
        let single = measure_label(&fonts, "prod 설정 변경 PR 승인", LabelMode::SvgText);
        assert_eq!(single.height, 22.0);
        assert!((single.width - 160.281_25).abs() < 0.02, "{}", single.width);
        let double = measure_label(&fonts, "local·CI<br/>PostgreSQL 검증", LabelMode::SvgText);
        assert!((double.height - 39.6).abs() < 1e-9);
    }
}
