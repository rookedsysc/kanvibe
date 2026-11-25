//! venn 설정(`config.venn`, defaultConfig.venn)과 테마 색(themeVariables venn1..8 등 기본값).
//!
//! 그림 크기는 글자·선 배율(폭 ÷ 1600)도 정한다. `padding`은 원 배치(venn.js 기본 15)가 아니라 글자 상자
//! 배치에만 쓰인다. 색은 테마 기본값(venn1..8, vennTitleTextColor, vennSetTextColor)에 정의가 덮은 `themeVariables`
//! (데스크톱 경로만, TS 모바일 워커는 `secure`로 막는다)를 겹치고, 원 라벨은 어두운 `background`(다크 테마 #333)에서
//! 밝히고 밝은 배경에서 어둡게 한다.

use crate::chart_config::ConfigSection;
use crate::directive::DiagramConfig;
use crate::scene::Color;
use crate::theme::{Theme, hsl};
use crate::theme_variables::ThemeVariables;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VennConfig {
    pub width: f64,
    pub height: f64,
    /// 글자 상자 배치 여백
    pub padding: f64,
    /// `useDebugLayout`: 글자 상자 안쪽 원과 칸을 점선으로 그린다
    pub is_debug_layout: bool,
}

impl VennConfig {
    pub fn read(config: &DiagramConfig) -> Self {
        let section = ConfigSection::of(config, "venn");
        let mut venn = Self {
            width: 800.0,
            height: 450.0,
            padding: 8.0,
            is_debug_layout: false,
        };
        section.read_number("width", &mut venn.width);
        section.read_number("height", &mut venn.height);
        section.read_number("padding", &mut venn.padding);
        section.read_flag("useDebugLayout", &mut venn.is_debug_layout);
        venn
    }
}

/// venn 색
pub struct VennPalette {
    pub sets: [Color; 8],
    pub title: Color,
    pub set_text: Color,
    /// khroma `isDark(background)`: 다크 테마 배경 #333만 어둡다
    pub is_dark: bool,
}

/// 테마별 (venn1..8, titleColor, textColor)
fn theme_defaults(theme: Theme) -> ([Color; 8], Color, Color) {
    match theme {
        Theme::Light => {
            let primary = |hue: f64| hsl(hue, 100.0, 66.274_509_803_9);
            let secondary = |hue: f64| hsl(hue, 100.0, 63.529_411_764_7);
            let sets = [
                primary(240.0),
                secondary(60.0),
                hsl(80.0, 100.0, 56.274_509_803_9),
                primary(300.0),
                primary(180.0),
                secondary(120.0),
                primary(0.0),
                secondary(180.0),
            ];
            let text = Color::rgb(0x33, 0x33, 0x33);
            (sets, text, text)
        }
        Theme::Dark => {
            // cScale0..7을 30만큼 밝힌 색
            let sets = [
                0x1f2020, 0x0b0000, 0x4d1037, 0x3f5258, 0x4f2f1b, 0x6e0a0a, 0x3b0048, 0x995a01,
            ]
            .map(|value: u32| {
                let base = Color::rgb((value >> 16) as u8, (value >> 8) as u8, value as u8);
                adjust_lightness(base, 30.0)
            });
            let title = Color::rgb(0xF9, 0xFF, 0xFE);
            (sets, title, Color::rgb(0xCC, 0xCC, 0xCC))
        }
    }
}

impl VennPalette {
    pub fn for_theme(theme: Theme) -> Self {
        let (sets, title, set_text) = theme_defaults(theme);
        Self {
            sets,
            title,
            set_text,
            is_dark: theme == Theme::Dark,
        }
    }

    /// vennRenderer.ts가 읽는 `venn1..8`·`vennTitleTextColor`·`vennSetTextColor`·`background`
    pub fn with_theme_variables(mut self, variables: &ThemeVariables) -> Self {
        for (index, set) in self.sets.iter_mut().enumerate() {
            variables.read_color(&format!("venn{}", index + 1), set);
        }
        variables.read_color("vennTitleTextColor", &mut self.title);
        variables.read_color("vennSetTextColor", &mut self.set_text);
        if let Some(background) = variables.color("background") {
            self.is_dark = !is_light(background);
        }
        self
    }
}

/// khroma `isLight`: WCAG 상대 휘도가 0.5 이상이면 밝다
fn is_light(color: Color) -> bool {
    let linear = |channel: u8| {
        let value = f64::from(channel) / 255.0;
        if value <= 0.039_28 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance =
        0.2126 * linear(color.red) + 0.7152 * linear(color.green) + 0.0722 * linear(color.blue);
    luminance >= 0.5
}

/// khroma `lighten`·`darken`: HSL 밝기를 `delta`(백분율)만큼 바꾼다
pub fn adjust_lightness(color: Color, delta: f64) -> Color {
    let channel = |value: u8| f64::from(value) / 255.0;
    let (red, green, blue) = (
        channel(color.red),
        channel(color.green),
        channel(color.blue),
    );
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let lightness = (max + min) / 2.0;
    let chroma = max - min;
    let (hue, saturation) = if chroma == 0.0 {
        (0.0, 0.0)
    } else {
        let saturation = chroma / (1.0 - (2.0 * lightness - 1.0).abs());
        let hue = if max == red {
            ((green - blue) / chroma).rem_euclid(6.0)
        } else if max == green {
            (blue - red) / chroma + 2.0
        } else {
            (red - green) / chroma + 4.0
        };
        (hue * 60.0, saturation)
    };
    let adjusted = (lightness * 100.0 + delta).clamp(0.0, 100.0);
    Color {
        alpha: color.alpha,
        ..hsl(hue, saturation * 100.0, adjusted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directive::read_config;

    #[test]
    fn 다크_테마_집합_색은_cscale을_30만큼_밝힌다() {
        let colors = VennPalette::for_theme(Theme::Dark);
        assert_eq!(colors.sets[0], Color::rgb(106, 110, 110));
        assert_eq!(colors.sets[1], Color::rgb(164, 0, 0));
        assert!(colors.is_dark);
    }

    #[test]
    fn 앞머리의_크기와_디버그_배치를_읽는다() {
        let config = read_config(
            "---\nconfig:\n  venn:\n    width: 600\n    useDebugLayout: true\n---\nvenn-beta",
        );
        let venn = VennConfig::read(&config);
        assert_eq!((venn.width, venn.height), (600.0, 450.0));
        assert!(venn.is_debug_layout);
    }
}
