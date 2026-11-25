//! xychart 축(chartBuilder/components/axis: baseAxis.ts·bandAxis.ts·linearAxis.ts).
//!
//! 축은 놓일 자리(왼쪽·아래·위)에 따라 선, 눈금 라벨, 눈금, 제목이 차지할 폭이나 높이를 남은 공간에서 차례로
//! 떼어 가고, 들어가지 않는 요소는 숨긴다. 범주 축은 d3 `scaleBand`(paddingInner 1), 숫자 축은 d3
//! `scaleLinear`와 `ticks(10)`으로 위치와 눈금을 정한다.

use crate::chart_text::{AnchoredText, BaselineAnchor, TextAnchor, push_anchored_text};
use crate::scene::{Color, PathCommand, Point, Scene, StrokeStyle};
use crate::text::{FontBook, FontWeight};

use super::config::AxisConfig;
use super::parser::{AxisScale, format_number};
use super::ticks::linear_ticks;

const BAR_WIDTH_TO_TICK_WIDTH_RATIO: f64 = 0.7;
const MAX_OUTER_PADDING_PERCENT_FOR_WRT_LABEL: f64 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisPosition {
    Left,
    Bottom,
    Top,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// 글자 묶음의 최대 폭·높이(getMaxDimension). 높이는 반올림한 ascent + descent다
pub fn max_text_dimension(fonts: &FontBook, texts: &[String], font_size: f64) -> (f64, f64) {
    let metrics = fonts.primary_metrics(font_size);
    texts.iter().fold((0.0, 0.0), |(width, height), text| {
        let text_width = fonts.shape_line(text, font_size, FontWeight::Regular).width;
        (
            f64::max(width, text_width),
            f64::max(height, metrics.ascent + metrics.descent),
        )
    })
}

pub struct ChartAxis {
    data: AxisScale,
    pub position: AxisPosition,
    pub bounds: Bounds,
    range: (f64, f64),
    pub outer_padding: f64,
    title_text_height: f64,
    show_title: bool,
    show_label: bool,
    show_tick: bool,
    show_axis_line: bool,
    config: AxisConfig,
    /// 제목·라벨·눈금·선 색
    color: Color,
}

impl ChartAxis {
    pub fn new(data: AxisScale, position: AxisPosition, config: AxisConfig, color: Color) -> Self {
        Self {
            data,
            position,
            bounds: Bounds::default(),
            range: (0.0, 10.0),
            outer_padding: 0.0,
            title_text_height: 0.0,
            show_title: false,
            show_label: false,
            show_tick: false,
            show_axis_line: false,
            config,
            color,
        }
    }

    fn is_vertical(&self) -> bool {
        self.position == AxisPosition::Left
    }

    pub fn set_range(&mut self, start: f64, end: f64) {
        self.range = (start, end);
        if self.is_vertical() {
            self.bounds.height = end - start;
        } else {
            self.bounds.width = end - start;
        }
    }

    /// 바깥 여백을 뺀 그리기 범위
    fn padded_range(&self) -> (f64, f64) {
        (
            self.range.0 + self.outer_padding,
            self.range.1 - self.outer_padding,
        )
    }

    /// 숫자 축 정의역. 왼쪽 축은 위가 큰 값이 되도록 뒤집는다
    fn linear_domain(&self) -> Option<(f64, f64)> {
        match self.data {
            AxisScale::Linear { min, max, .. } if self.is_vertical() => Some((max, min)),
            AxisScale::Linear { min, max, .. } => Some((min, max)),
            AxisScale::Band { .. } => None,
        }
    }

    /// 눈금 글자와 그 위치
    pub fn ticks(&self) -> Vec<(String, f64)> {
        match (&self.data, self.linear_domain()) {
            (_, Some((start, end))) => linear_ticks(start, end, 10)
                .into_iter()
                .map(|tick| (format_number(tick), self.scale_number(tick)))
                .collect(),
            (AxisScale::Band { categories, .. }, None) => categories
                .iter()
                .enumerate()
                .map(|(index, category)| (category.clone(), self.band_position(index)))
                .collect(),
            (AxisScale::Linear { .. }, None) => Vec::new(),
        }
    }

    fn tick_count(&self) -> usize {
        match (&self.data, self.linear_domain()) {
            (_, Some((start, end))) => linear_ticks(start, end, 10).len(),
            (AxisScale::Band { categories, .. }, None) => categories.len(),
            (AxisScale::Linear { .. }, None) => 0,
        }
    }

    /// d3 scaleBand(paddingInner 1, paddingOuter 0, align 0.5)의 `index`번째 위치
    fn band_position(&self, index: usize) -> f64 {
        let (start, stop) = self.padded_range();
        let count = self.tick_count() as f64;
        let step = (stop - start) / f64::max(1.0, count - 1.0);
        let first = start + (stop - start - step * (count - 1.0)) * 0.5;
        first + step * index as f64
    }

    fn scale_number(&self, value: f64) -> f64 {
        let Some((domain_start, domain_end)) = self.linear_domain() else {
            return self.padded_range().0;
        };
        let (start, end) = self.padded_range();
        let span = domain_end - domain_start;
        let ratio = if span == 0.0 {
            0.5
        } else {
            (value - domain_start) / span
        };
        start * (1.0 - ratio) + end * ratio
    }

    /// 계열 값의 범주 글자 위치. 범주 축에 없는 글자는 범위 시작이다
    pub fn scale_category(&self, category: &str) -> f64 {
        match &self.data {
            AxisScale::Band { categories, .. } => categories
                .iter()
                .position(|known| known == category)
                .map_or(self.padded_range().0, |index| self.band_position(index)),
            AxisScale::Linear { .. } => self.scale_number(category.parse().unwrap_or(f64::NAN)),
        }
    }

    pub fn scale_value(&self, value: f64) -> f64 {
        self.scale_number(value)
    }

    pub fn tick_distance(&self) -> f64 {
        let (start, end) = self.padded_range();
        (start - end).abs() / self.tick_count() as f64
    }

    /// 막대가 있으면 눈금 간격의 70%가 들어가도록 바깥 여백을 늘린다
    pub fn widen_outer_padding_for_bars(&mut self) {
        let wanted = BAR_WIDTH_TO_TICK_WIDTH_RATIO * self.tick_distance();
        if wanted > self.outer_padding * 2.0 {
            self.outer_padding = (wanted / 2.0).floor();
        }
    }

    fn tick_labels(&self) -> Vec<String> {
        self.ticks().into_iter().map(|(label, _)| label).collect()
    }

    /// 축이 차지할 크기를 정하고 (폭, 높이)를 돌려준다
    pub fn calculate_space(&mut self, fonts: &FontBook, width: f64, height: f64) -> (f64, f64) {
        if self.is_vertical() {
            self.calculate_vertical_space(fonts, width, height);
        } else {
            self.calculate_horizontal_space(fonts, width, height);
        }
        (self.bounds.width, self.bounds.height)
    }

    fn calculate_horizontal_space(&mut self, fonts: &FontBook, width: f64, height: f64) {
        let mut available = self.reserve_axis_line(height);
        if self.config.show_label {
            let (label_width, label_height) =
                max_text_dimension(fonts, &self.tick_labels(), self.config.label_font_size);
            self.outer_padding = f64::min(
                label_width / 2.0,
                MAX_OUTER_PADDING_PERCENT_FOR_WRT_LABEL * width,
            );
            available = self.reserve_label(label_height, available);
        }
        available = self.reserve_tick(available);
        available = self.reserve_title(fonts, available);
        self.bounds.width = width;
        self.bounds.height = height - available;
    }

    fn calculate_vertical_space(&mut self, fonts: &FontBook, width: f64, height: f64) {
        let mut available = self.reserve_axis_line(width);
        if self.config.show_label {
            let (label_width, label_height) =
                max_text_dimension(fonts, &self.tick_labels(), self.config.label_font_size);
            self.outer_padding = f64::min(
                label_height / 2.0,
                MAX_OUTER_PADDING_PERCENT_FOR_WRT_LABEL * height,
            );
            available = self.reserve_label(label_width, available);
        }
        available = self.reserve_tick(available);
        available = self.reserve_title(fonts, available);
        self.bounds.width = width - available;
        self.bounds.height = height;
    }

    /// 축 선은 남은 공간이 선 굵기보다 클 때만 그린다
    fn reserve_axis_line(&mut self, available: f64) -> f64 {
        if self.config.show_axis_line && available > self.config.axis_line_width {
            self.show_axis_line = true;
            return available - self.config.axis_line_width;
        }
        available
    }

    /// 눈금 라벨은 라벨 크기와 양쪽 여백이 들어가야 보인다
    fn reserve_label(&mut self, label_size: f64, available: f64) -> f64 {
        let required = label_size + self.config.label_padding * 2.0;
        if required <= available {
            self.show_label = true;
            return available - required;
        }
        available
    }

    fn reserve_tick(&mut self, available: f64) -> f64 {
        if self.config.show_tick && available >= self.config.tick_length {
            self.show_tick = true;
            return available - self.config.tick_length;
        }
        available
    }

    /// 제목은 글자 높이와 위아래 여백만큼 자리를 뗀다
    fn reserve_title(&mut self, fonts: &FontBook, available: f64) -> f64 {
        let title = self.data.title().to_string();
        if !self.config.show_title || title.is_empty() {
            return available;
        }
        let (_, title_height) = max_text_dimension(fonts, &[title], self.config.title_font_size);
        self.title_text_height = title_height;
        let required = title_height + self.config.title_padding * 2.0;
        if required <= available {
            self.show_title = true;
            return available - required;
        }
        available
    }

    fn push_axis_line(&self, scene: &mut Scene, from: Point, to: Point) {
        push_line(scene, from, to, self.color, self.config.axis_line_width);
    }

    fn push_tick(&self, scene: &mut Scene, from: Point, to: Point) {
        push_line(scene, from, to, self.color, self.config.tick_width);
    }

    fn label_text<'a>(&self, text: &'a str, anchor: Point) -> AnchoredText<'a> {
        let (text_anchor, baseline) = match self.position {
            AxisPosition::Left => (TextAnchor::End, BaselineAnchor::Middle),
            AxisPosition::Bottom | AxisPosition::Top => {
                (TextAnchor::Middle, BaselineAnchor::TextBeforeEdge)
            }
        };
        AnchoredText {
            text,
            x: anchor.x,
            y: anchor.y,
            font_size: self.config.label_font_size,
            color: self.color,
            anchor: text_anchor,
            baseline,
            degrees: 0.0,
        }
    }

    pub fn draw(&self, scene: &mut Scene, fonts: &FontBook) {
        match self.position {
            AxisPosition::Left => self.draw_left(scene, fonts),
            AxisPosition::Bottom => self.draw_bottom(scene, fonts),
            AxisPosition::Top => self.draw_top(scene, fonts),
        }
    }

    fn line_width_if_shown(&self) -> f64 {
        if self.show_axis_line {
            self.config.axis_line_width
        } else {
            0.0
        }
    }

    fn tick_length_if_shown(&self) -> f64 {
        if self.show_tick {
            self.config.tick_length
        } else {
            0.0
        }
    }

    fn draw_left(&self, scene: &mut Scene, fonts: &FontBook) {
        let bounds = self.bounds;
        let right = bounds.x + bounds.width;
        if self.show_axis_line {
            let x = right - self.config.axis_line_width / 2.0;
            let bottom = bounds.y + bounds.height;
            self.push_axis_line(scene, Point::new(x, bounds.y), Point::new(x, bottom));
        }
        let ticks = self.ticks();
        if self.show_label {
            let x = right
                - self.config.label_padding
                - self.tick_length_if_shown()
                - self.line_width_if_shown();
            for (label, y) in &ticks {
                push_anchored_text(scene, fonts, &self.label_text(label, Point::new(x, *y)));
            }
        }
        if self.show_tick {
            let x = right - self.line_width_if_shown();
            for (_, y) in &ticks {
                let from = Point::new(x, *y);
                self.push_tick(scene, from, Point::new(x - self.config.tick_length, *y));
            }
        }
        if self.show_title {
            let anchor = Point::new(
                bounds.x + self.config.title_padding,
                bounds.y + bounds.height / 2.0,
            );
            self.push_title(scene, fonts, anchor, 270.0);
        }
    }

    fn draw_bottom(&self, scene: &mut Scene, fonts: &FontBook) {
        let bounds = self.bounds;
        if self.show_axis_line {
            let y = bounds.y + self.config.axis_line_width / 2.0;
            let right = bounds.x + bounds.width;
            self.push_axis_line(scene, Point::new(bounds.x, y), Point::new(right, y));
        }
        let ticks = self.ticks();
        if self.show_label {
            let y = bounds.y
                + self.config.label_padding
                + self.tick_length_if_shown()
                + self.line_width_if_shown();
            for (label, x) in &ticks {
                push_anchored_text(scene, fonts, &self.label_text(label, Point::new(*x, y)));
            }
        }
        if self.show_tick {
            let y = bounds.y + self.line_width_if_shown();
            for (_, x) in &ticks {
                let from = Point::new(*x, y);
                self.push_tick(scene, from, Point::new(*x, y + self.config.tick_length));
            }
        }
        if self.show_title {
            let x = self.range.0 + (self.range.1 - self.range.0) / 2.0;
            let y = bounds.y + bounds.height - self.config.title_padding - self.title_text_height;
            self.push_title(scene, fonts, Point::new(x, y), 0.0);
        }
    }

    fn draw_top(&self, scene: &mut Scene, fonts: &FontBook) {
        let bounds = self.bounds;
        let bottom = bounds.y + bounds.height;
        if self.show_axis_line {
            let y = bottom - self.config.axis_line_width / 2.0;
            let right = bounds.x + bounds.width;
            self.push_axis_line(scene, Point::new(bounds.x, y), Point::new(right, y));
        }
        let ticks = self.ticks();
        if self.show_label {
            let title_space = if self.show_title {
                self.title_text_height + self.config.title_padding * 2.0
            } else {
                0.0
            };
            let y = bounds.y + title_space + self.config.label_padding;
            for (label, x) in &ticks {
                push_anchored_text(scene, fonts, &self.label_text(label, Point::new(*x, y)));
            }
        }
        if self.show_tick {
            let from_y = bottom - self.line_width_if_shown();
            for (_, x) in &ticks {
                let from = Point::new(*x, from_y);
                let to = Point::new(*x, from_y - self.config.tick_length);
                self.push_tick(scene, from, to);
            }
        }
        if self.show_title {
            let anchor = Point::new(
                bounds.x + bounds.width / 2.0,
                bounds.y + self.config.title_padding,
            );
            self.push_title(scene, fonts, anchor, 0.0);
        }
    }

    fn push_title(&self, scene: &mut Scene, fonts: &FontBook, anchor: Point, degrees: f64) {
        let title = AnchoredText {
            text: self.data.title(),
            x: anchor.x,
            y: anchor.y,
            font_size: self.config.title_font_size,
            color: self.color,
            anchor: TextAnchor::Middle,
            baseline: BaselineAnchor::TextBeforeEdge,
            degrees,
        };
        push_anchored_text(scene, fonts, &title);
    }
}

fn push_line(scene: &mut Scene, from: Point, to: Point, color: Color, width: f64) {
    let commands = vec![PathCommand::MoveTo(from), PathCommand::LineTo(to)];
    let stroke = StrokeStyle {
        color,
        width,
        dash: Vec::new(),
    };
    scene.push_path(commands, None, Some(stroke));
}
