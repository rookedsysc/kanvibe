//! rendering-util classBox(shapes/classBox.ts)와 class/shapeUtil.ts textHelper의 크기·배치.
//!
//! 주석(«…»)·제목(굵게)·속성·메서드 묶음을 원본처럼 먼저 쌓아 getBBox를 구하고, 그 상자에 padding 12를 두른
//! 사각형과 구분선 두 줄을 그린다. 원본은 묶음 위치를 bbox 기준으로 다시 옮기므로 그 식을 그대로 따른다.
//! 속성·메서드 묶음은 x = 0에서 시작하고 제목은 가운데에 놓이므로, 상자 폭은 `제목/2 + 가장 긴 줄`이 된다.

use super::ClassColors;
use super::measure::{MeasuredText, measure_markdown};
use super::member::{ClassMember, Classifier};
use super::parser::ClassNode;
use super::underline::{ink_intercepts, underline_pieces};
use crate::LabelMode;
use crate::flowchart::ElementStyle;
use crate::label::{FONT_SIZE, LINE_HEIGHT};
use crate::scene::{Color, PathCommand, PathItem, Point, SceneItem, StrokeStyle, rounded_rect};
use crate::text::{FontBook, FontWeight};

/// config.class.padding 기본값. classBox는 같은 값을 GAP으로도 쓴다
const PADDING: f64 = 12.0;
const GAP: f64 = PADDING;
/// htmlLabels가 꺼지면 묶음을 4px 위로 올린다
const SVG_TEXT_LIFT: f64 = 4.0;
/// htmlLabels가 꺼지면 같은 묶음의 줄 사이에 3px을 더 둔다
const SVG_LINE_PADDING: f64 = 3.0;
/// 밑줄 윗변의 기준선 아래 거리와 두께. 기준 이미지(class-features)의 밑줄 덮임에서 잰 값이다:
/// HTML 라벨은 약 1px 아래 1.1px 두께, SVG 글자는 약 1.1px 아래 1.4px 두께(글자 크기의 1/10 근처)다
const HTML_UNDERLINE_OFFSET: f64 = 1.0;
const HTML_UNDERLINE_THICKNESS: f64 = 1.1;
const SVG_UNDERLINE_OFFSET: f64 = 1.1;
const SVG_UNDERLINE_THICKNESS: f64 = 1.4;

/// 라벨 `g` 원점 기준 한 줄의 세로 지표
#[derive(Clone, Copy, Debug)]
pub struct LineBox {
    /// getBBox(SVG 글자)나 div 높이(HTML)
    pub height: f64,
    /// 글자 상자 위 끝
    pub top: f64,
    pub baseline: f64,
    /// 같은 묶음의 줄 사이 여백(TEXT_PADDING)
    pub spacing: f64,
    /// 밑줄 윗변의 기준선 아래 거리와 두께
    pub underline_offset: f64,
    pub underline_thickness: f64,
}

impl LineBox {
    pub fn for_mode(fonts: &FontBook, mode: LabelMode) -> Self {
        let metrics = fonts.primary_metrics(FONT_SIZE);
        let content = metrics.ascent + metrics.descent;
        match mode {
            LabelMode::Html => Self {
                height: LINE_HEIGHT,
                top: 0.0,
                baseline: ((LINE_HEIGHT - content) / 2.0).floor() + metrics.ascent,
                spacing: 0.0,
                underline_offset: HTML_UNDERLINE_OFFSET,
                underline_thickness: HTML_UNDERLINE_THICKNESS,
            },
            LabelMode::SvgText => Self {
                height: content,
                top: FONT_SIZE - metrics.ascent,
                baseline: FONT_SIZE,
                spacing: SVG_LINE_PADDING,
                underline_offset: SVG_UNDERLINE_OFFSET,
                underline_thickness: SVG_UNDERLINE_THICKNESS,
            },
        }
    }

    /// 줄 `count`개를 쌓은 묶음의 bbox 높이
    fn stack_height(&self, count: usize) -> f64 {
        if count == 0 {
            return 0.0;
        }
        count as f64 * self.height + (count - 1) as f64 * self.spacing
    }

    /// 묶음 원점이 `group_y`일 때 줄 `count`개가 차지하는 세로 범위
    fn stack_range(&self, group_y: f64, count: usize) -> (f64, f64) {
        let first_top = group_y - self.height / 2.0 + self.top;
        (first_top, first_top + self.stack_height(count))
    }
}

#[derive(Clone, Debug)]
struct PlacedText {
    text: MeasuredText,
    left: f64,
    baseline: f64,
}

/// 노드 중심 기준으로 배치를 마친 클래스 상자
#[derive(Clone, Debug)]
pub struct ClassBoxLayout {
    /// dagre에 넘기는 사각형 크기
    pub width: f64,
    pub height: f64,
    texts: Vec<PlacedText>,
    divider_ys: [f64; 2],
    line: LineBox,
}

struct Sections {
    annotation: Option<MeasuredText>,
    title: MeasuredText,
    members: Vec<MeasuredText>,
    methods: Vec<MeasuredText>,
    line: LineBox,
}

pub fn layout_class_box(fonts: &FontBook, class: &ClassNode, mode: LabelMode) -> ClassBoxLayout {
    let measure =
        |text: &str, weight: FontWeight| measure_markdown(fonts, text, FONT_SIZE, weight, mode);
    // SVG 글자는 createFormattedText가 tspan마다 font-style="normal"을 달아 기울임이 사라진다
    let is_italic_shown = mode == LabelMode::Html;
    let measure_member = |member: &ClassMember| {
        let weight = match member.classifier {
            Some(Classifier::Abstract) if is_italic_shown => FontWeight::Italic,
            _ => FontWeight::Regular,
        };
        let measured = measure(&member.text, weight);
        if member.classifier != Some(Classifier::Static) {
            return measured;
        }
        let line = LineBox::for_mode(fonts, mode);
        let band = (
            line.underline_offset,
            line.underline_offset + line.underline_thickness,
        );
        let intercepts: Vec<(f64, f64)> = ink_intercepts(fonts, &measured, FONT_SIZE, band)
            .into_iter()
            .map(|(left, right)| (left - measured.left, right - measured.left))
            .collect();
        MeasuredText {
            underline: underline_pieces(measured.width, &intercepts, line.underline_thickness),
            ..measured
        }
    };
    let sections = Sections {
        annotation: class
            .annotations
            .first()
            .map(|annotation| measure(&format!("«{annotation}»"), FontWeight::Regular)),
        title: measure(&class.title(), FontWeight::Bold),
        members: class.members.iter().map(measure_member).collect(),
        methods: class.methods.iter().map(measure_member).collect(),
        line: LineBox::for_mode(fonts, mode),
    };
    sections.place(mode == LabelMode::SvgText)
}

impl Sections {
    fn annotation_height(&self) -> f64 {
        if self.annotation.is_some() {
            self.line.height
        } else {
            0.0
        }
    }

    /// textHelper가 속성 묶음을 놓는 y
    fn members_y(&self) -> f64 {
        self.annotation_height() + self.line.height + GAP * 2.0
    }

    /// textHelper가 메서드 묶음을 놓는 y(속성이 없으면 GAP/2 높이로 친다)
    fn methods_y(&self) -> f64 {
        let members_height = self.line.stack_height(self.members.len());
        let members_height = if members_height > 0.0 {
            members_height
        } else {
            GAP / 2.0
        };
        self.annotation_height() + self.line.height + members_height + GAP * 4.0
    }

    /// textHelper 직후 shapeSvg.getBBox()의 폭·높이
    fn content_extent(&self) -> (f64, f64) {
        let mut ranges = vec![self.line.stack_range(self.annotation_height(), 1)];
        if self.annotation.is_some() {
            ranges.push(self.line.stack_range(0.0, 1));
        }
        if !self.members.is_empty() {
            ranges.push(self.line.stack_range(self.members_y(), self.members.len()));
        }
        if !self.methods.is_empty() {
            ranges.push(self.line.stack_range(self.methods_y(), self.methods.len()));
        }
        let top = ranges
            .iter()
            .map(|range| range.0)
            .fold(f64::INFINITY, f64::min);
        let bottom = ranges
            .iter()
            .map(|range| range.1)
            .fold(f64::NEG_INFINITY, f64::max);
        let centered = self
            .annotation
            .iter()
            .chain(std::iter::once(&self.title))
            .map(|text| (text.left - text.width / 2.0, text.left + text.width / 2.0));
        let lines = self
            .members
            .iter()
            .chain(&self.methods)
            .map(|text| (text.left, text.left + text.width));
        let (left, right) = centered.chain(lines).fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(left, right), range| (left.min(range.0), right.max(range.1)),
        );
        (right - left, bottom - top)
    }

    /// classBox: 사각형 크기를 정하고 묶음·구분선을 bbox 기준 자리로 옮긴다
    fn place(self, is_svg: bool) -> ClassBoxLayout {
        let is_empty = self.members.is_empty() && self.methods.is_empty();
        let (content_width, mut content_height) = self.content_extent();
        content_height += match (self.members.is_empty(), self.methods.is_empty()) {
            (true, true) => GAP,
            (false, true) => GAP * 2.0,
            _ => 0.0,
        };
        let (x, y) = (-content_width / 2.0, -content_height / 2.0);
        let extra_padding = if is_empty { PADDING } else { 0.0 };
        let shrink = if is_empty { PADDING / 2.0 } else { 0.0 };
        let lift = if is_svg { SVG_TEXT_LIFT } else { 0.0 };
        let shift = y + PADDING - extra_padding - lift;
        let annotation_height = self.annotation_height() - shrink;
        let label_height = self.line.height - shrink;
        let members_height = self.line.stack_height(self.members.len()) - shrink;
        let methods_y = annotation_height
            + label_height
            + members_height.max(GAP / 2.0)
            + y
            + GAP * 4.0
            + PADDING
            - lift;
        let mut texts = Vec::new();
        if let Some(annotation) = &self.annotation {
            self.push_group(
                &mut texts,
                std::slice::from_ref(annotation),
                -annotation.width / 2.0,
                shift,
            );
        }
        let title_y = self.annotation_height() + shift;
        self.push_group(
            &mut texts,
            std::slice::from_ref(&self.title),
            -self.title.width / 2.0,
            title_y,
        );
        self.push_group(&mut texts, &self.members, x, self.members_y() + shift);
        self.push_group(&mut texts, &self.methods, x, methods_y);
        let first_divider = annotation_height + label_height + y + PADDING;
        ClassBoxLayout {
            width: content_width + PADDING * 2.0,
            height: content_height + PADDING * 2.0 + extra_padding * 2.0,
            texts,
            divider_ys: [first_divider, first_divider + members_height + GAP * 2.0],
            line: self.line,
        }
    }

    /// addText: 묶음 원점 (left, group_y)에서 i번째 줄은 `−높이/2 + i·(높이 + 여백)`에 놓인다
    fn push_group(
        &self,
        texts: &mut Vec<PlacedText>,
        lines: &[MeasuredText],
        left: f64,
        group_y: f64,
    ) {
        for (index, text) in lines.iter().enumerate() {
            let offset = index as f64 * (self.line.height + self.line.spacing);
            texts.push(PlacedText {
                text: text.clone(),
                left,
                baseline: group_y - self.line.height / 2.0 + offset + self.line.baseline,
            });
        }
    }
}

impl ClassBoxLayout {
    /// 중심이 `center`인 상자: 채운 사각형, 구분선 두 줄, 글자. style·classDef 선언이 색과 선을 덮는다
    pub fn draw(
        &self,
        center: Point,
        colors: &ClassColors,
        style: &ElementStyle,
    ) -> Vec<SceneItem> {
        let stroke = StrokeStyle {
            color: style.stroke.unwrap_or(colors.node_stroke),
            width: style.stroke_width.unwrap_or(1.0),
            dash: style.dash.clone(),
        };
        let left = center.x - self.width / 2.0;
        let mut items = vec![SceneItem::Path(PathItem {
            commands: rounded_rect(
                left,
                center.y - self.height / 2.0,
                self.width,
                self.height,
                0.0,
            ),
            fill: Some(style.fill.unwrap_or(colors.node_fill)),
            stroke: Some(stroke.clone()),
        })];
        for divider_y in self.divider_ys {
            let y = center.y + divider_y;
            items.push(SceneItem::Path(PathItem {
                commands: vec![
                    PathCommand::MoveTo(Point::new(left, y)),
                    PathCommand::LineTo(Point::new(left + self.width, y + 0.001)),
                ],
                fill: None,
                stroke: Some(stroke.clone()),
            }));
        }
        let color = style.text_color.unwrap_or(colors.class_text);
        for placed in &self.texts {
            let (x, baseline) = (center.x + placed.left, center.y + placed.baseline);
            for (start, end) in &placed.text.underline {
                items.push(self.underline(
                    x + placed.text.left + start,
                    end - start,
                    baseline,
                    color,
                ));
            }
            let texts = placed.text.text_items(x, baseline, FONT_SIZE, color);
            items.extend(texts.into_iter().map(SceneItem::Text));
        }
        items
    }

    /// 밑줄 조각: 기준선 아래 `underline_offset`에서 `underline_thickness` 두께
    fn underline(&self, left: f64, width: f64, baseline: f64, color: Color) -> SceneItem {
        SceneItem::Path(PathItem {
            commands: rounded_rect(
                left,
                baseline + self.line.underline_offset,
                width,
                self.line.underline_thickness,
                0.0,
            ),
            fill: Some(color),
            stroke: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::class::parser::parse;
    use crate::text::tests::bundled_fonts;

    fn class_box(definition: &str, name: &str, mode: LabelMode) -> ClassBoxLayout {
        let diagram = parse(definition).expect("parse");
        let class = diagram
            .classes
            .iter()
            .find(|class| class.id == name)
            .expect("class");
        layout_class_box(&bundled_fonts(), class, mode)
    }

    #[test]
    fn 기준_svg의_상자_크기와_구분선() {
        // class-basic--light.svg: DiagramImage 221.03×168, 구분선 −36·60
        let source = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../parity/diagrams/baseline/class-basic.mmd"
        ))
        .expect("mmd");
        let image = class_box(&source, "DiagramImage", LabelMode::Html);
        assert!((image.width - 221.031_25).abs() < 0.05, "{}", image.width);
        assert_eq!(image.height, 168.0);
        assert_eq!(image.divider_ys, [-36.0, 60.0]);
        // 주석이 있는 DiagramRenderer: 높이 150, 데스크톱(SVG 글자) 144
        assert_eq!(
            class_box(&source, "DiagramRenderer", LabelMode::Html).height,
            150.0
        );
        let desktop = class_box(&source, "DiagramRenderer", LabelMode::SvgText);
        assert_eq!(desktop.height, 144.0);
        assert_eq!(desktop.divider_ys, [-4.0, 20.0]);
    }
}
