//! 마인드맵 가지 색. 값은 기준 SVG `<style>`의 `.section-N`·`.section-root` 규칙에서 읽었다.

use crate::scene::Color;
use crate::theme::{Theme, hsl};

/// 라이트 테마 `cScale1..11`의 색상(채도 100%, 밝기 76.27%). 0번 가지만 밝기 73.53%다
const LIGHT_SECTION_HUES: [f64; 11] = [
    60.0, 80.0, 270.0, 300.0, 330.0, 0.0, 30.0, 90.0, 150.0, 180.0, 210.0,
];
/// 라이트 테마 `cScaleInv1..11`(밑줄 색)의 색상
const LIGHT_LINE_HUES: [f64; 11] = [
    240.0, 260.0, 90.0, 120.0, 150.0, 180.0, 210.0, 270.0, 330.0, 0.0, 30.0,
];
/// 다크 테마 `cScale1..11`
const DARK_SECTION_FILLS: [(u8, u8, u8); 11] = [
    (0x0b, 0x00, 0x00),
    (0x4d, 0x10, 0x37),
    (0x3f, 0x52, 0x58),
    (0x4f, 0x2f, 0x1b),
    (0x6e, 0x0a, 0x0a),
    (0x3b, 0x00, 0x48),
    (0x99, 0x5a, 0x01),
    (0x15, 0x47, 0x06),
    (0x16, 0x17, 0x22),
    (0x00, 0x29, 0x6f),
    (0x01, 0x62, 0x9c),
];

pub(crate) struct SectionColors {
    pub fill: Color,
    pub text: Color,
    /// 기본 노드 밑줄. 다크 테마는 그라데이션 규칙이 `stroke-width: 0`으로 지운다
    pub line: Option<Color>,
}

impl SectionColors {
    /// 루트(`section-root`)면 `None`, 나머지는 가지 번호
    pub(crate) fn for_node(theme: Theme, section: Option<usize>) -> Self {
        match (theme, section) {
            (Theme::Light, None) => Self {
                fill: hsl(240.0, 100.0, 46.274_509_803_9),
                text: Color::rgb(255, 255, 255),
                line: None,
            },
            (Theme::Dark, None) => Self {
                fill: hsl(180.0, 1.587_301_587_3, 48.352_941_176_5),
                text: Color::rgb(0x2c, 0x2c, 0x2c),
                line: None,
            },
            (Theme::Light, Some(section)) => light_section(section % LIGHT_SECTION_HUES.len()),
            (Theme::Dark, Some(section)) => {
                let (red, green, blue) = DARK_SECTION_FILLS[section % DARK_SECTION_FILLS.len()];
                Self {
                    fill: Color::rgb(red, green, blue),
                    text: Color::rgb(211, 211, 211),
                    line: None,
                }
            }
        }
    }
}

fn light_section(section: usize) -> SectionColors {
    let fill_lightness = if section == 0 {
        73.529_411_764_7
    } else {
        76.274_509_803_9
    };
    let line_lightness = if section == 0 {
        83.529_411_764_7
    } else {
        86.274_509_803_9
    };
    // cScaleLabel: 2번 가지만 흰 글자다
    let text = if section == 2 {
        Color::rgb(255, 255, 255)
    } else {
        Color::rgb(0, 0, 0)
    };
    SectionColors {
        fill: hsl(LIGHT_SECTION_HUES[section], 100.0, fill_lightness),
        text,
        line: Some(hsl(LIGHT_LINE_HUES[section], 100.0, line_lightness)),
    }
}
