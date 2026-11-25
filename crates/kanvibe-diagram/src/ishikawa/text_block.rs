//! ishikawaRenderer.ts의 `drawMultilineText`·`wrapText`: 줄마다 `tspan`을 두는 SVG 글자와 그 `getBBox()`.
//!
//! 줄 간격은 글자 크기와 상관없이 설정 글자 크기(16px)의 1.05배이고, 묶음 가운데가 `y`에 오도록 첫 줄을
//! `(줄 수 − 1) · 간격 / 2`만큼 올린다. 세로 맞춤은 `dominant-baseline`(`middle`·`hanging`·기본)으로 정한다.

use crate::scene::{Color, TextItem};
use crate::text::{FontBook, FontWeight};

/// 설정 글자 크기(`fontSize: 16`). 머리 글자도 줄 간격은 이 크기로 잰다
pub const FONT_SIZE: f64 = 16.0;
pub const LINE_GAP: f64 = FONT_SIZE * 1.05;
/// Blink가 `dominant-baseline: hanging`에 쓰는 기준선 높이(ascent의 80%)
const HANGING_ASCENT_RATIO: f64 = 0.8;
/// Noto Sans ascender(1em 기준)
const ASCENT_EM: f64 = 1.069;
/// Noto Sans 보통·굵은 얼굴의 x-height(1em 기준). `middle`은 x-height 절반을 1/64px로 내림한 만큼 내린다
const REGULAR_X_HEIGHT_EM: f64 = 0.536;
const BOLD_X_HEIGHT_EM: f64 = 0.547;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Middle,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAlign {
    Alphabetic,
    Middle,
    Hanging,
}

/// SVG 사각형 범위(`getBBox()` 합집합)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Bounds {
    pub const EMPTY: Bounds = Bounds {
        left: f64::INFINITY,
        top: f64::INFINITY,
        right: f64::NEG_INFINITY,
        bottom: f64::NEG_INFINITY,
    };

    pub fn width(&self) -> f64 {
        self.right - self.left
    }

    pub fn height(&self) -> f64 {
        self.bottom - self.top
    }

    pub fn include(&mut self, other: &Bounds) {
        self.left = self.left.min(other.left);
        self.top = self.top.min(other.top);
        self.right = self.right.max(other.right);
        self.bottom = self.bottom.max(other.bottom);
    }

    pub fn include_point(&mut self, x: f64, y: f64) {
        self.include(&Bounds {
            left: x,
            top: y,
            right: x,
            bottom: y,
        });
    }
}

/// 글자 스타일: 크기·굵기·가로 기준·세로 맞춤
#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    pub font_size: f64,
    pub weight: FontWeight,
    pub anchor: Anchor,
    pub align: VerticalAlign,
}

/// 줄마다 자리를 잡은 글자와 그 `getBBox()`
#[derive(Clone, Debug)]
pub struct PlacedText {
    pub items: Vec<TextItem>,
    pub bounds: Bounds,
}

/// `<br>`·줄바꿈으로 나눈 줄을 (x, y) 기준으로 놓는다
pub fn place_text(
    fonts: &FontBook,
    text: &str,
    x: f64,
    y: f64,
    style: TextStyle,
    color: Color,
) -> PlacedText {
    let lines = split_lines(text);
    let first_y = y - (lines.len() as f64 - 1.0) * LINE_GAP / 2.0;
    let shift = baseline_shift(style);
    let metrics = fonts.primary_metrics(style.font_size);
    let mut placed = PlacedText {
        items: Vec::new(),
        bounds: Bounds::EMPTY,
    };
    for (index, line) in lines.into_iter().enumerate() {
        let baseline = first_y + index as f64 * LINE_GAP + shift;
        let extent = fonts.measure_svg_runs(&[line.as_str()], style.font_size, style.weight);
        let start = match style.anchor {
            Anchor::Middle => x - extent.advance / 2.0,
            Anchor::End => x - extent.advance,
        };
        placed.bounds.include(&Bounds {
            left: start + extent.left,
            top: baseline - metrics.ascent,
            right: start + extent.right,
            bottom: baseline + metrics.descent,
        });
        placed.items.push(TextItem {
            text: line,
            x: start,
            baseline,
            font_size: style.font_size,
            weight: style.weight,
            color,
            rotation: None,
        });
    }
    placed
}

/// `dominant-baseline`이 기준선을 `y`에서 아래로 내리는 거리
fn baseline_shift(style: TextStyle) -> f64 {
    match style.align {
        VerticalAlign::Alphabetic => 0.0,
        VerticalAlign::Hanging => HANGING_ASCENT_RATIO * ASCENT_EM * style.font_size,
        VerticalAlign::Middle => {
            let x_height = match style.weight {
                FontWeight::Bold | FontWeight::BoldItalic => BOLD_X_HEIGHT_EM,
                FontWeight::Regular | FontWeight::Italic | FontWeight::Monospace => {
                    REGULAR_X_HEIGHT_EM
                }
            };
            (x_height * style.font_size / 2.0 * 64.0).floor() / 64.0
        }
    }
}

/// `text.split(/<br\s*\/?>|\n/)`
fn split_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut rest = text;
    while let Some(character) = rest.chars().next() {
        let tag_length = line_break_length(rest);
        if tag_length > 0 {
            lines.push(std::mem::take(&mut current));
            rest = &rest[tag_length..];
            continue;
        }
        current.push(character);
        rest = &rest[character.len_utf8()..];
    }
    lines.push(current);
    lines
}

/// `rest`가 줄바꿈(`\n`·`<br>`·`<br/>`·`<br />`)으로 시작하면 그 길이
fn line_break_length(rest: &str) -> usize {
    if rest.starts_with('\n') {
        return 1;
    }
    let Some(after) = rest.strip_prefix("<br") else {
        return 0;
    };
    let spaces = after.len() - after.trim_start().len();
    let after = &after[spaces..];
    let slash = usize::from(after.starts_with('/'));
    match after[slash..].starts_with('>') {
        true => 3 + spaces + slash + 1,
        false => 0,
    }
}

/// `wrapText`: UTF-16 길이가 `max_chars`를 넘으면 공백으로 나눈 낱말을 `max_chars` 안에서 이어 붙인다
pub fn wrap_text(text: &str, max_chars: usize) -> String {
    let length = |value: &str| value.encode_utf16().count();
    if length(text) <= max_chars {
        return text.to_string();
    }
    let mut lines: Vec<String> = Vec::new();
    for word in text
        .split(char::is_whitespace)
        .filter(|word| !word.is_empty())
    {
        match lines.last_mut() {
            Some(last) if length(last) + 1 + length(word) <= max_chars => {
                last.push(' ');
                last.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 긴_글자는_낱말_단위로_줄을_바꾼다() {
        assert_eq!(wrap_text("Server Outage", 11), "Server\nOutage");
        assert_eq!(
            wrap_text("Late Delivery of Customer Orders", 11),
            "Late\nDelivery of\nCustomer\nOrders"
        );
        assert_eq!(wrap_text("Shaky hands", 15), "Shaky hands");
    }

    #[test]
    fn br_태그와_줄바꿈으로_줄을_나눈다() {
        assert_eq!(split_lines("a<br>b<br />c\nd"), vec!["a", "b", "c", "d"]);
        assert_eq!(split_lines("a<bra>"), vec!["a<bra>"]);
    }

    #[test]
    fn middle_기준선은_x_height_절반만큼_내린다() {
        let style = TextStyle {
            font_size: 16.0,
            weight: FontWeight::Regular,
            anchor: Anchor::Middle,
            align: VerticalAlign::Middle,
        };
        // ishikawa-simple 기준 SVG: 원인 상자 y(59.2366) + 2 + ascent(17) − 글자 y(73.9553)
        assert_eq!(baseline_shift(style), 4.28125);
        let bold = TextStyle {
            font_size: 14.0,
            weight: FontWeight::Bold,
            ..style
        };
        assert_eq!(baseline_shift(bold), 3.828125);
    }
}
