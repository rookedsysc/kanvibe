//! 글자 크기 계산(utils.calculateTextDimensions·wrapLabel). 시퀀스 다이어그램은 모든 글자가 16px SVG 글자다.
//!
//! 크기는 줄마다 `getBBox()`를 정수로 반올림한 값이다. 폭은 가장 넓은 줄, 높이는 줄 높이(ascent + descent)의 합이다.

use crate::text::{FontBook, FontWeight};

pub(super) const FONT_SIZE: f64 = 16.0;
/// wrapLabel이 줄을 이을 때 넣는 줄바꿈
const JOIN_WITH: &str = "<br/>";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct TextSize {
    pub width: f64,
    pub height: f64,
}

/// JavaScript `Math.round`(0.5는 늘 위로)
pub(super) fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// common.splitBreaks: `<br>`, `<br/>`, `<br />`(대소문자 무시)로 줄을 나눈다
pub(super) fn split_breaks(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut line_start = 0;
    let mut search_from = 0;
    let lower = text.to_ascii_lowercase();
    while let Some(offset) = lower[search_from..].find("<br") {
        let tag_start = search_from + offset;
        match line_break_end(&lower[tag_start + 3..]) {
            Some(length) => {
                lines.push(&text[line_start..tag_start]);
                line_start = tag_start + 3 + length;
                search_from = line_start;
            }
            None => search_from = tag_start + 3,
        }
    }
    lines.push(&text[line_start..]);
    lines
}

/// `<br` 뒤의 `\s*\/?>` 길이
fn line_break_end(rest: &str) -> Option<usize> {
    let spaces = rest.len() - rest.trim_start().len();
    let after_spaces = &rest[spaces..];
    let slash = usize::from(after_spaces.starts_with('/'));
    after_spaces[slash..]
        .starts_with('>')
        .then_some(spaces + slash + 1)
}

/// 한 줄 글자의 getBBox 폭(반올림 전)
fn line_width(fonts: &FontBook, line: &str) -> f64 {
    fonts
        .measure_svg_runs(&[line], FONT_SIZE, FontWeight::Regular)
        .width()
}

/// getBBox 높이(글꼴 ascent + descent)
pub(super) fn line_height(fonts: &FontBook) -> f64 {
    let metrics = fonts.primary_metrics(FONT_SIZE);
    metrics.ascent + metrics.descent
}

/// calculateTextDimensions: 빈 글은 0, 빈 줄도 한 줄 높이를 차지한다
pub(super) fn text_size(fonts: &FontBook, text: &str) -> TextSize {
    if text.is_empty() {
        return TextSize::default();
    }
    let lines = split_breaks(text);
    let width = lines
        .iter()
        .map(|line| js_round(line_width(fonts, line)))
        .fold(0.0, f64::max);
    TextSize {
        width,
        height: lines.len() as f64 * js_round(line_height(fonts)),
    }
}

fn rounded_width(fonts: &FontBook, text: &str) -> f64 {
    text_size(fonts, text).width
}

/// wrapLabel: 낱말을 이어 붙이다 폭에 닿으면 줄을 바꾸고 `<br/>`로 잇는다. 이미 줄바꿈이 있으면 그대로 둔다
pub(super) fn wrap_label(fonts: &FontBook, label: &str, max_width: f64) -> String {
    if label.is_empty() || split_breaks(label).len() > 1 {
        return label.to_string();
    }
    let words: Vec<&str> = label.split(' ').filter(|word| !word.is_empty()).collect();
    let mut completed: Vec<String> = Vec::new();
    let mut next_line = String::new();
    for word in &words {
        // 끝 공백은 SVG 글자에서 접히므로 낱말 폭에 들어가지 않는다
        let word_width = rounded_width(fonts, word);
        let next_width = rounded_width(fonts, &next_line);
        if word_width > max_width {
            let (pieces, remaining) = break_string(fonts, word, max_width);
            completed.push(std::mem::take(&mut next_line));
            completed.extend(pieces);
            next_line = remaining;
        } else if next_width + word_width >= max_width {
            completed.push(std::mem::replace(&mut next_line, (*word).to_string()));
        } else if next_line.is_empty() {
            next_line = (*word).to_string();
        } else {
            next_line = format!("{next_line} {word}");
        }
    }
    completed.push(next_line);
    completed.retain(|line| !line.is_empty());
    completed.join(JOIN_WITH)
}

/// breakString: 한 낱말을 폭에 맞게 끊고 끊은 자리에 `-`를 붙인다. 남은 꼬리를 따로 돌려준다
fn break_string(fonts: &FontBook, word: &str, max_width: f64) -> (Vec<String>, String) {
    let characters: Vec<char> = word.chars().collect();
    let mut pieces = Vec::new();
    let mut current = String::new();
    for (index, character) in characters.iter().enumerate() {
        let next = format!("{current}{character}");
        if rounded_width(fonts, &next) < max_width {
            current = next;
            continue;
        }
        let is_last = index + 1 == characters.len();
        pieces.push(if is_last { next } else { format!("{next}-") });
        current.clear();
    }
    (pieces, current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tests::bundled_fonts;

    #[test]
    fn br로_줄을_나눈다() {
        assert_eq!(
            split_breaks("a<br>b<BR/>c<br />d<bx"),
            ["a", "b", "c", "d<bx"]
        );
    }

    #[test]
    fn 기준_svg의_글자_크기와_같다() {
        let fonts = bundled_fonts();
        // seq-basic: "Split mermaid blocks" 폭 157, 한 줄 높이 22
        let size = text_size(&fonts, "Split mermaid blocks");
        assert_eq!(
            size,
            TextSize {
                width: 157.0,
                height: 22.0
            }
        );
        assert_eq!(text_size(&fonts, "a<br>b").height, 44.0);
    }

    #[test]
    fn 블록_제목을_폭에_맞춰_감싼다() {
        let fonts = bundled_fonts();
        assert_eq!(
            wrap_label(&fonts, "[every message]", 107.0),
            "[every<br/>message]"
        );
        assert_eq!(wrap_label(&fonts, "[ok]", 107.0), "[ok]");
    }
}
