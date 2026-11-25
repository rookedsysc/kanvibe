//! timeline `styles.js`가 기본·dark 테마에서 낸 구역 색(`section--1`..`section-10`). 값은 기준 SVG `<style>`에서
//! 읽었다.

use crate::scene::Color;
use crate::theme::{Theme, hsl};
use crate::theme_variables::ThemeVariables;

/// 구역 색 개수(THEME_COLOR_LIMIT 12 → 구역 번호 −1..=10)
pub const SECTION_COLOR_COUNT: usize = 12;
/// `.eventWrapper{filter: brightness(120%)}`
const EVENT_BRIGHTNESS: f64 = 1.2;

/// 구역 하나의 바탕·글자·밑줄 색
#[derive(Clone, Copy, Debug)]
pub struct SectionColors {
    pub fill: Color,
    pub text: Color,
    pub line: Color,
}

impl SectionColors {
    /// 사건 상자는 `brightness(120%)` 필터로 모든 색이 밝아진다
    pub fn brightened(self) -> Self {
        Self {
            fill: brighten(self.fill),
            text: brighten(self.text),
            line: brighten(self.line),
        }
    }
}

fn brighten(color: Color) -> Color {
    let channel = |value: u8| (f64::from(value) * EVENT_BRIGHTNESS).round().min(255.0) as u8;
    Color {
        red: channel(color.red),
        green: channel(color.green),
        blue: channel(color.blue),
        alpha: color.alpha,
    }
}

#[derive(Clone, Debug)]
pub struct TimelineColors {
    /// 구역 번호 −1부터 10까지
    sections: [SectionColors; SECTION_COLOR_COUNT],
    /// 제목과 화살촉(SVG 기본 `fill`)
    pub title: Color,
    /// `.lineWrapper line` 점선·가로 화살표
    pub connector: Color,
}

impl TimelineColors {
    pub fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => light(),
            Theme::Dark => dark(),
        }
    }

    /// timeline styles.js `genSections`: 구역 `i − 1`은 `cScale{i}`·`cScaleLabel{i}`·`cScaleInv{i}`(밑줄)이고, 반복마다
    /// 다시 쓰는 `.lineWrapper line`은 마지막 `cScaleLabel11`이 남는다. 제목·화살촉은 루트 `fill`(`textColor`)이다
    pub fn with_theme_variables(mut self, variables: &ThemeVariables) -> Self {
        for (index, section) in self.sections.iter_mut().enumerate() {
            variables.read_color(&format!("cScale{index}"), &mut section.fill);
            variables.read_color(&format!("cScaleLabel{index}"), &mut section.text);
            variables.read_color(&format!("cScaleInv{index}"), &mut section.line);
        }
        let last = SECTION_COLOR_COUNT - 1;
        variables.read_color(&format!("cScaleLabel{last}"), &mut self.connector);
        variables.read_color("textColor", &mut self.title);
        self
    }

    /// drawNode의 구역 번호(`번호 % 12 − 1`, −1..=10)에 맞는 색
    pub fn section(&self, section: i64) -> SectionColors {
        self.sections[(section + 1) as usize % SECTION_COLOR_COUNT]
    }
}

fn light() -> TimelineColors {
    let white = Color::rgb(255, 255, 255);
    let black = Color::rgb(0, 0, 0);
    let fill_hues = [
        240.0, 60.0, 80.0, 270.0, 300.0, 330.0, 0.0, 30.0, 90.0, 150.0, 180.0, 210.0,
    ];
    let line_hues = [
        60.0, 240.0, 260.0, 90.0, 120.0, 150.0, 180.0, 210.0, 270.0, 330.0, 0.0, 30.0,
    ];
    let sections = std::array::from_fn(|index| {
        let fill_lightness = if index == 1 {
            73.529_411_764_7
        } else {
            76.274_509_803_9
        };
        let line_lightness = if index == 1 {
            83.529_411_764_7
        } else {
            86.274_509_803_9
        };
        SectionColors {
            fill: hsl(fill_hues[index], 100.0, fill_lightness),
            text: if index == 0 || index == 3 {
                white
            } else {
                black
            },
            line: hsl(line_hues[index], 100.0, line_lightness),
        }
    });
    TimelineColors {
        sections,
        title: Color::rgb(0x33, 0x33, 0x33),
        connector: black,
    }
}

fn dark() -> TimelineColors {
    let light_grey = Color::rgb(211, 211, 211);
    let fills = [
        0x1f2020, 0x0b0000, 0x4d1037, 0x3f5258, 0x4f2f1b, 0x6e0a0a, 0x3b0048, 0x995a01, 0x154706,
        0x161722, 0x00296f, 0x01629c,
    ];
    let lines = [
        0xe0dfdf, 0xf4ffff, 0xb2efc8, 0xc0ada7, 0xb0d0e4, 0x91f5f5, 0xc4ffb7, 0x66a5fe, 0xeab8f9,
        0xe9e8dd, 0xffd690, 0xfe9d63,
    ];
    let sections = std::array::from_fn(|index| SectionColors {
        fill: hex(fills[index]),
        text: light_grey,
        line: hex(lines[index]),
    });
    TimelineColors {
        sections,
        title: Color::rgb(0xcc, 0xcc, 0xcc),
        connector: light_grey,
    }
}

fn hex(value: u32) -> Color {
    Color::rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}
