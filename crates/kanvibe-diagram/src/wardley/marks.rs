//! 주석(번호 원과 설명 상자), 노트, 가속·감속 화살표 층.

use crate::scene::{Color, Point, polygon, rounded_rect};
use crate::text::FontWeight;

use super::parser::{PlacedMark, WardleyMap};
use super::{Anchor, Baseline, Frame, PADDING, Painter, TextStyle};

const WHITE: Color = Color::rgb(255, 255, 255);
const BOX_PADDING: f64 = 10.0;
const BOX_LINE_HEIGHT: f64 = 16.0;
const BOX_FONT_SIZE: f64 = 11.0;
/// getBBox 높이(Noto Sans 11px ascent 12 + descent 3)
const BOX_TEXT_HEIGHT: f64 = 15.0;
const ARROW_WIDTH: f64 = 60.0;
const ARROW_HEIGHT: f64 = 30.0;
const ARROW_HEAD_WIDTH: f64 = 20.0;

pub fn draw_annotations(painter: &mut Painter, map: &WardleyMap, frame: Frame) {
    if map.annotations.is_empty() {
        return;
    }
    let colors = painter.colors;
    let number_style = TextStyle {
        font_size: 10.0,
        weight: FontWeight::Bold,
        anchor: Anchor::Middle,
        baseline: Baseline::Central,
        color: colors.axis_text,
    };
    for annotation in &map.annotations {
        let center = Point::new(frame.project_x(annotation.x), frame.project_y(annotation.y));
        painter.circle(center, 10.0, WHITE, colors.axis, 1.5);
        painter.text(&annotation.number.to_string(), center, number_style, None);
    }
    if let Some((x, y)) = map.annotations_box {
        draw_annotations_box(
            painter,
            map,
            frame,
            Point::new(frame.project_x(x), frame.project_y(y)),
        );
    }
}

/// 설명 상자: 번호 순 `번호. 글` 줄을 담고, 판 안쪽 여백 안으로 밀어 넣는다
fn draw_annotations_box(painter: &mut Painter, map: &WardleyMap, frame: Frame, origin: Point) {
    let mut entries: Vec<_> = map
        .annotations
        .iter()
        .filter(|annotation| !annotation.text.is_empty())
        .collect();
    if entries.is_empty() {
        return;
    }
    entries.sort_by_key(|annotation| annotation.number);
    let lines: Vec<String> = entries
        .iter()
        .map(|annotation| format!("{}. {}", annotation.number, annotation.text))
        .collect();
    let max_width = lines
        .iter()
        .map(|line| {
            painter
                .fonts
                .measure_svg_runs(&[line.as_str()], BOX_FONT_SIZE, FontWeight::Regular)
                .advance
        })
        .fold(0.0, f64::max);
    let width = max_width + BOX_PADDING * 2.0 + 105.0;
    let height = lines.len() as f64 * BOX_LINE_HEIGHT + BOX_PADDING * 2.0 + BOX_TEXT_HEIGHT / 2.0;
    let left = origin.x.min(frame.width - PADDING - width).max(PADDING);
    let top = origin.y.min(frame.height - PADDING - height).max(PADDING);
    let colors = painter.colors;
    let outline = rounded_rect(left, top, width, height, 4.0);
    let stroke = Painter::stroke(colors.axis, 1.5, &[]);
    painter.scene.push_path(outline, Some(WHITE), stroke);
    let style = TextStyle {
        font_size: BOX_FONT_SIZE,
        weight: FontWeight::Regular,
        anchor: Anchor::Start,
        baseline: Baseline::Middle,
        color: colors.axis_text,
    };
    for (index, line) in lines.iter().enumerate() {
        let at = Point::new(
            left + BOX_PADDING,
            top + BOX_PADDING + (index as f64 + 1.0) * BOX_LINE_HEIGHT,
        );
        painter.text(line, at, style, None);
    }
}

pub fn draw_notes_and_accelerators(painter: &mut Painter, map: &WardleyMap, frame: Frame) {
    let text_color = painter.colors.axis_text;
    let bold = |font_size: f64, anchor: Anchor| TextStyle {
        font_size,
        weight: FontWeight::Bold,
        anchor,
        baseline: Baseline::Alphabetic,
        color: text_color,
    };
    for note in &map.notes {
        let at = Point::new(frame.project_x(note.x), frame.project_y(note.y));
        painter.text(&note.text, at, bold(11.0, Anchor::Start), None);
    }
    for (marks, is_reversed) in [(&map.accelerators, false), (&map.deaccelerators, true)] {
        for mark in marks {
            draw_block_arrow(painter, mark, frame, is_reversed);
            let left = frame.project_x(mark.x);
            let at = Point::new(
                left + ARROW_WIDTH / 2.0,
                frame.project_y(mark.y) + ARROW_HEIGHT / 2.0 + 15.0,
            );
            painter.text(&mark.text, at, bold(10.0, Anchor::Middle), None);
        }
    }
}

/// 가속(오른쪽)·감속(왼쪽) 굵은 화살표. 몸통 60×30, 머리 폭 20, 머리는 몸통보다 위아래로 8 크다
fn draw_block_arrow(painter: &mut Painter, mark: &PlacedMark, frame: Frame, is_reversed: bool) {
    let (x, y) = (frame.project_x(mark.x), frame.project_y(mark.y));
    let half = ARROW_HEIGHT / 2.0;
    let outline = [
        (0.0, -half),
        (ARROW_WIDTH - ARROW_HEAD_WIDTH, -half),
        (ARROW_WIDTH - ARROW_HEAD_WIDTH, -half - 8.0),
        (ARROW_WIDTH, 0.0),
        (ARROW_WIDTH - ARROW_HEAD_WIDTH, half + 8.0),
        (ARROW_WIDTH - ARROW_HEAD_WIDTH, half),
        (0.0, half),
    ];
    let corners: Vec<Point> = outline
        .iter()
        .map(|(dx, dy)| match is_reversed {
            true => Point::new(x + ARROW_WIDTH - dx, y + dy),
            false => Point::new(x + dx, y + dy),
        })
        .collect();
    let stroke = Painter::stroke(painter.colors.component_stroke, 1.0, &[]);
    painter
        .scene
        .push_path(polygon(&corners), Some(WHITE), stroke);
}
