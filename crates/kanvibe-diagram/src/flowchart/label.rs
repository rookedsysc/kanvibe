//! HTML 라벨(`htmlLabels: true`) 크기 계산. Mermaid는 라벨을 foreignObject 안의 div로 그리고
//! `getBoundingClientRect()`로 잰다: 줄 높이 1.5(24px), `white-space: nowrap`, 최대 너비 200px.
//! 자연 폭이 200px에 닿으면 `break-spaces`로 200px 안에서 줄을 바꾼다.

use crate::scene::{Color, TextItem};
use crate::text::{FontBook, FontWeight, snap_layout_unit};

pub const FONT_SIZE: f64 = 16.0;
pub const LINE_HEIGHT: f64 = 24.0;
pub const WRAPPING_WIDTH: f64 = 200.0;

#[derive(Clone, Debug, Default)]
pub struct LabelLine {
    pub text: String,
    pub width: f64,
}

#[derive(Clone, Debug, Default)]
pub struct LabelBox {
    pub lines: Vec<LabelLine>,
    pub width: f64,
    pub height: f64,
}

impl LabelBox {
    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|line| line.text.is_empty())
    }

    /// 라벨 상자 왼쪽 위가 (left, top)일 때 줄마다 가운데 맞춘 글자
    pub fn text_items(&self, fonts: &FontBook, left: f64, top: f64, color: Color) -> Vec<TextItem> {
        let metrics = fonts.primary_metrics(FONT_SIZE);
        let half_leading = ((LINE_HEIGHT - (metrics.ascent + metrics.descent)) / 2.0).floor();
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, line)| !line.text.is_empty())
            .map(|(index, line)| TextItem {
                text: line.text.clone(),
                x: left + snap_layout_unit((self.width - line.width) / 2.0),
                baseline: top + index as f64 * LINE_HEIGHT + half_leading + metrics.ascent,
                font_size: FONT_SIZE,
                weight: FontWeight::Regular,
                color,
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

pub fn measure_label(fonts: &FontBook, label: &str) -> LabelBox {
    let raw_lines = label_lines(label);
    let measured: Vec<LabelLine> = raw_lines
        .into_iter()
        .map(|text| {
            let width = fonts.measure_line(&text, FONT_SIZE, FontWeight::Regular);
            LabelLine { text, width }
        })
        .collect();
    let natural = measured.iter().map(|line| line.width).fold(0.0, f64::max);
    if natural < WRAPPING_WIDTH {
        let height = if label.is_empty() {
            0.0
        } else {
            measured.len() as f64 * LINE_HEIGHT
        };
        return LabelBox {
            width: natural,
            height,
            lines: measured,
        };
    }
    let lines: Vec<LabelLine> = measured
        .iter()
        .flat_map(|line| wrap_line(fonts, &line.text))
        .collect();
    LabelBox {
        width: WRAPPING_WIDTH,
        height: lines.len() as f64 * LINE_HEIGHT,
        lines,
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

fn wrap_line(fonts: &FontBook, text: &str) -> Vec<LabelLine> {
    let mut lines = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let rest = &text[start..];
        let mut end = rest.len();
        if fonts.measure_line(rest.trim_end(), FONT_SIZE, FontWeight::Regular) > WRAPPING_WIDTH {
            let candidates = break_opportunities(rest);
            let fitting = candidates
                .iter()
                .copied()
                .take_while(|index| {
                    fonts.measure_line(rest[..*index].trim_end(), FONT_SIZE, FontWeight::Regular)
                        <= WRAPPING_WIDTH
                })
                .last();
            end = fitting
                .or_else(|| candidates.first().copied())
                .unwrap_or(rest.len());
        }
        let line = rest[..end].trim_end().to_string();
        let width = fonts.measure_line(&line, FONT_SIZE, FontWeight::Regular);
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
        let label = measure_label(&fonts, "Valid?");
        assert_eq!(label.width, 43.296875);
        assert_eq!(label.height, 24.0);
        assert_eq!(measure_label(&fonts, "").height, 0.0);
    }

    #[test]
    fn br은_줄을_나눈다() {
        let fonts = bundled_fonts();
        let label = measure_label(&fonts, "local·CI<br/>PostgreSQL 검증");
        assert_eq!(label.lines.len(), 2);
        assert_eq!(label.height, 48.0);
    }
}
