//! user journey `styles.js`가 기본·dark 테마에서 낸 색. 값은 기준 SVG `<style>`에서 읽었다.

use crate::scene::Color;
use crate::theme::{Theme, hsl};
use crate::theme_variables::ThemeVariables;

/// journey 설정 `actorColours`
const ACTOR_COLORS: [Color; 6] = [
    Color::rgb(0x8F, 0xBC, 0x8F),
    Color::rgb(0x7C, 0xFC, 0x00),
    Color::rgb(0x00, 0xFF, 0xFF),
    Color::rgb(0x20, 0xB2, 0xAA),
    Color::rgb(0xB0, 0xE0, 0xE6),
    Color::rgb(0xFF, 0xFF, 0xE0),
];

#[derive(Clone, Debug)]
pub struct JourneyColors {
    /// `.task-type-N`·`.section-type-N` 바탕(설정 `sectionFills` 7개를 돈다)
    section_fills: [Color; 7],
    /// 범례 글자(`.legend`)
    pub legend: Color,
    /// 상자 HTML 글자(`.label`)
    pub label: Color,
    /// 제목과 입 모양 칠(SVG 기본 `fill`)
    pub title: Color,
    pub mouth_fill: Color,
    /// 점선과 가로 화살표(`line{stroke}`)
    pub line: Color,
    /// 화살촉(SVG 기본 `fill`)
    pub arrow_head: Color,
    pub box_stroke: Color,
    pub face_fill: Color,
    pub face_stroke: Color,
    /// 눈과 입 선(`#666`)
    pub feature: Color,
}

impl JourneyColors {
    pub fn for_theme(theme: Theme) -> Self {
        let (text, section_fills) = match theme {
            Theme::Light => (Color::rgb(0x33, 0x33, 0x33), light_section_fills()),
            Theme::Dark => (Color::rgb(0xcc, 0xcc, 0xcc), dark_section_fills()),
        };
        Self {
            section_fills,
            legend: text,
            label: text,
            title: text,
            mouth_fill: text,
            line: text,
            arrow_head: text,
            box_stroke: Color::rgb(0x66, 0x66, 0x66),
            face_fill: Color::rgb(0xFF, 0xF8, 0xDC),
            face_stroke: Color::rgb(0x99, 0x99, 0x99),
            feature: Color::rgb(0x66, 0x66, 0x66),
        }
    }

    /// user-journey styles.js: 글자·선·제목은 textColor(뿌리 `fill`도 같다), 상자 바탕은 `fillType0`이 있을 때 `fillTypeN`
    pub fn with_theme_variables(mut self, variables: &ThemeVariables) -> Self {
        if let Some(text) = variables.color("textColor") {
            self.legend = text;
            self.label = text;
            self.title = text;
            self.mouth_fill = text;
            self.line = text;
            self.arrow_head = text;
        }
        if variables.flag("fillType0") {
            for (index, fill) in self.section_fills.iter_mut().enumerate() {
                variables.read_color(&format!("fillType{index}"), fill);
            }
        }
        self
    }

    pub fn section_fill(&self, section_number: usize) -> Color {
        self.section_fills[section_number % self.section_fills.len()]
    }

    pub fn actor(&self, position: usize) -> Color {
        ACTOR_COLORS[position % ACTOR_COLORS.len()]
    }
}

fn light_section_fills() -> [Color; 7] {
    [
        Color::rgb(0xEC, 0xEC, 0xFF),
        Color::rgb(0xFF, 0xFF, 0xDE),
        hsl(304.0, 100.0, 96.274_509_803_9),
        hsl(124.0, 100.0, 93.529_411_764_7),
        hsl(176.0, 100.0, 96.274_509_803_9),
        hsl(356.0, 100.0, 93.529_411_764_7),
        hsl(8.0, 100.0, 96.274_509_803_9),
    ]
}

fn dark_section_fills() -> [Color; 7] {
    let saturation = 1.587_301_587_3;
    [
        Color::rgb(0x1f, 0x20, 0x20),
        hsl(180.0, saturation, 28.352_941_176_5),
        hsl(244.0, saturation, 12.352_941_176_5),
        hsl(244.0, saturation, 28.352_941_176_5),
        hsl(116.0, saturation, 12.352_941_176_5),
        hsl(116.0, saturation, 28.352_941_176_5),
        hsl(308.0, saturation, 12.352_941_176_5),
    ]
}
