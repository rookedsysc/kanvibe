//! xychart 막대 값 라벨(`showDataLabel`, xychartRenderer.ts의 rect 분기).
//!
//! 라벨 글자는 어느 막대 계열이든 첫 계열의 값이다(Mermaid 그대로). 글자 폭을 `글자 수 × 크기 × 0.7`로
//! 어림해 모든 막대에 들어가는 가장 큰 정수 크기를 하나 고른다. 세로 차트는 막대 위 끝 안쪽(또는 바깥 위),
//! 가로 차트는 막대 오른쪽 끝 안쪽(또는 바깥)에 둔다.

use crate::chart_text::{AnchoredText, BaselineAnchor, TextAnchor, push_anchored_text};
use crate::scene::{Color, Scene};
use crate::text::FontBook;

use super::axis::Bounds;
use super::parser::{XyChart, format_number};

const CHAR_WIDTH_FACTOR: f64 = 0.7;
/// 세로 차트에서 막대 위 끝과 라벨 사이
const VERTICAL_OFFSET: f64 = 10.0;
/// 가로 차트에서 막대 오른쪽 끝과 라벨 사이
const RIGHT_MARGIN: f64 = 10.0;

pub struct DataLabelStyle {
    pub is_horizontal: bool,
    pub is_outside: bool,
    pub color: Color,
}

/// 첫 계열 값의 JS 숫자 글자
pub fn label_texts(chart: &XyChart) -> Vec<String> {
    chart.plots.first().map_or_else(Vec::new, |plot| {
        plot.points
            .iter()
            .map(|(_, value)| format_number(*value))
            .collect()
    })
}

/// 어림 폭으로 막대 안에 들어가는지
fn fits(bar: &Bounds, label: &str, font_size: f64, is_horizontal: bool) -> bool {
    let text_width = font_size * label.chars().count() as f64 * CHAR_WIDTH_FACTOR;
    if is_horizontal {
        return text_width <= bar.width - RIGHT_MARGIN;
    }
    let center = bar.x + bar.width / 2.0;
    let is_inside_width =
        center - text_width / 2.0 >= bar.x && center + text_width / 2.0 <= bar.x + bar.width;
    is_inside_width && bar.y + VERTICAL_OFFSET + font_size <= bar.y + bar.height
}

/// 막대 하나에 들어가는 가장 큰 크기(1px씩 줄여 찾는다)
fn candidate_font_size(bar: &Bounds, label: &str, is_horizontal: bool) -> f64 {
    let mut font_size = if is_horizontal {
        bar.height * CHAR_WIDTH_FACTOR
    } else {
        bar.width / (label.chars().count() as f64 * CHAR_WIDTH_FACTOR)
    };
    while !fits(bar, label, font_size, is_horizontal) && font_size > 0.0 {
        font_size -= 1.0;
    }
    font_size
}

/// 폭·높이가 양수인 막대에만 라벨을 둔다. 라벨 번호는 걸러 내기 전 막대 순서다
pub fn draw_data_labels(
    scene: &mut Scene,
    fonts: &FontBook,
    bars: &[Bounds],
    labels: &[String],
    style: &DataLabelStyle,
) {
    let visible: Vec<(&Bounds, &String)> = bars
        .iter()
        .zip(labels)
        .filter(|(bar, _)| bar.width > 0.0 && bar.height > 0.0)
        .collect();
    let font_size = visible
        .iter()
        .map(|(bar, label)| candidate_font_size(bar, label, style.is_horizontal))
        .fold(f64::INFINITY, f64::min)
        .floor();
    if !font_size.is_finite() || font_size <= 0.0 {
        return;
    }
    for (bar, label) in visible {
        let text = label_placement(bar, label, font_size, style);
        push_anchored_text(scene, fonts, &text);
    }
}

fn label_placement<'a>(
    bar: &Bounds,
    label: &'a str,
    font_size: f64,
    style: &DataLabelStyle,
) -> AnchoredText<'a> {
    let (x, y, anchor, baseline) = if style.is_horizontal {
        let right = bar.x + bar.width;
        let (x, anchor) = if style.is_outside {
            (right + RIGHT_MARGIN, TextAnchor::Start)
        } else {
            (right - RIGHT_MARGIN, TextAnchor::End)
        };
        (x, bar.y + bar.height / 2.0, anchor, BaselineAnchor::Middle)
    } else {
        let (y, baseline) = if style.is_outside {
            (bar.y - VERTICAL_OFFSET, BaselineAnchor::Alphabetic)
        } else {
            (bar.y + VERTICAL_OFFSET, BaselineAnchor::Hanging)
        };
        (bar.x + bar.width / 2.0, y, TextAnchor::Middle, baseline)
    };
    AnchoredText {
        text: label,
        x,
        y,
        font_size,
        color: style.color,
        anchor,
        baseline,
        degrees: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 세로_막대_라벨은_폭과_높이에_맞는_크기를_고른다() {
        let bar = Bounds {
            x: 0.0,
            y: 0.0,
            width: 47.5,
            height: 66.0,
        };
        assert_eq!(candidate_font_size(&bar, "5000", false).floor(), 16.0);
    }
}
