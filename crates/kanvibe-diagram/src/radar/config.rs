//! radar 설정(`config.radar`, defaultConfig.radar)과 테마 색·굵기(themeVariables.radar·cScale0..11 기본값).
//!
//! 앞머리 `config`나 `init` 지시문에서 형이 맞는 값만 기본값을 덮는다. 색과 굵기는 테마 기본값에 정의가 덮은
//! `themeVariables`(데스크톱 경로만)를 겹친다.

use crate::chart_config::ConfigSection;
use crate::directive::DiagramConfig;
use crate::scene::Color;
use crate::theme::{Theme, hsl};
use crate::theme_variables::ThemeVariables;

/// defaultConfig.radar
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadarConfig {
    pub width: f64,
    pub height: f64,
    pub margin_top: f64,
    pub margin_right: f64,
    pub margin_bottom: f64,
    pub margin_left: f64,
    pub axis_scale_factor: f64,
    pub axis_label_factor: f64,
    pub curve_tension: f64,
}

impl RadarConfig {
    pub fn read(config: &DiagramConfig) -> Self {
        let section = ConfigSection::of(config, "radar");
        let mut radar = Self {
            width: 600.0,
            height: 600.0,
            margin_top: 50.0,
            margin_right: 50.0,
            margin_bottom: 50.0,
            margin_left: 50.0,
            axis_scale_factor: 1.0,
            axis_label_factor: 1.05,
            curve_tension: 0.17,
        };
        let numbers: [(&str, &mut f64); 9] = [
            ("width", &mut radar.width),
            ("height", &mut radar.height),
            ("marginTop", &mut radar.margin_top),
            ("marginRight", &mut radar.margin_right),
            ("marginBottom", &mut radar.margin_bottom),
            ("marginLeft", &mut radar.margin_left),
            ("axisScaleFactor", &mut radar.axis_scale_factor),
            ("axisLabelFactor", &mut radar.axis_label_factor),
            ("curveTension", &mut radar.curve_tension),
        ];
        for (key, target) in numbers {
            section.read_number(key, target);
        }
        radar
    }
}

/// themeVariables.radar 기본값과 곡선 색. TS 워커는 `themeVariables`를 `secure`로 막아 정의에서 바꿀 수 없다
#[derive(Clone, Debug, PartialEq)]
pub struct RadarStyle {
    /// SVG 루트 `fill`(라벨·범례·제목 글자, textColor)
    pub text: Color,
    pub axis: Color,
    pub axis_stroke_width: f64,
    pub axis_label_font_size: f64,
    pub curve_opacity: f32,
    pub curve_stroke_width: f64,
    pub graticule: Color,
    pub graticule_stroke_width: f64,
    pub graticule_opacity: f32,
    pub legend_font_size: f64,
    /// cScale0..11
    pub curves: [Color; 12],
}

/// 테마별 (textColor, lineColor, cScale0..11)
fn theme_defaults(theme: Theme) -> (Color, Color, [Color; 12]) {
    match theme {
        Theme::Light => {
            let scale = |hue: f64| hsl(hue, 100.0, 76.274_509_803_9);
            let curves = [
                scale(240.0),
                hsl(60.0, 100.0, 73.529_411_764_7),
                scale(80.0),
                scale(270.0),
                scale(300.0),
                scale(330.0),
                scale(0.0),
                scale(30.0),
                scale(90.0),
                scale(150.0),
                scale(180.0),
                scale(210.0),
            ];
            let text = Color::rgb(0x33, 0x33, 0x33);
            (text, text, curves)
        }
        Theme::Dark => {
            let curves = [
                0x1f2020, 0x0b0000, 0x4d1037, 0x3f5258, 0x4f2f1b, 0x6e0a0a, 0x3b0048, 0x995a01,
                0x154706, 0x161722, 0x00296f, 0x01629c,
            ]
            .map(|value: u32| Color::rgb((value >> 16) as u8, (value >> 8) as u8, value as u8));
            (
                Color::rgb(0xCC, 0xCC, 0xCC),
                Color::rgb(211, 211, 211),
                curves,
            )
        }
    }
}

impl RadarStyle {
    pub fn for_theme(theme: Theme) -> Self {
        let (text, line, curves) = theme_defaults(theme);
        Self {
            text,
            axis: line,
            axis_stroke_width: 2.0,
            axis_label_font_size: 12.0,
            curve_opacity: 0.5,
            curve_stroke_width: 2.0,
            graticule: Color::rgb(0xDE, 0xDE, 0xDE),
            graticule_stroke_width: 1.0,
            graticule_opacity: 0.3,
            legend_font_size: 12.0,
            curves,
        }
    }

    /// radar styles.ts: 곡선은 `cScale0..11`, 선·글자 크기·투명도는 `themeVariables.radar`, 글자는 문서 `fill`(`textColor`)
    pub fn with_theme_variables(mut self, variables: &ThemeVariables) -> Self {
        variables.read_color("textColor", &mut self.text);
        for (index, curve) in self.curves.iter_mut().enumerate() {
            variables.read_color(&format!("cScale{index}"), curve);
        }
        let radar = variables.section("radar");
        radar.read_color("axisColor", &mut self.axis);
        radar.read_number("axisStrokeWidth", &mut self.axis_stroke_width);
        radar.read_number("axisLabelFontSize", &mut self.axis_label_font_size);
        radar.read_number("curveStrokeWidth", &mut self.curve_stroke_width);
        radar.read_color("graticuleColor", &mut self.graticule);
        radar.read_number("graticuleStrokeWidth", &mut self.graticule_stroke_width);
        radar.read_number("legendFontSize", &mut self.legend_font_size);
        for (key, target) in [
            ("curveOpacity", &mut self.curve_opacity),
            ("graticuleOpacity", &mut self.graticule_opacity),
        ] {
            if let Some(opacity) = radar.number(key) {
                *target = opacity as f32;
            }
        }
        self
    }

    pub fn curve(&self, index: usize) -> Color {
        self.curves[index % self.curves.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directive::read_config;

    #[test]
    fn 지시문의_크기와_여백을_읽는다() {
        let config =
            read_config("%%{init: {\"radar\": {\"width\": 400, \"marginTop\": 80}}}%%\nradar-beta");
        let radar = RadarConfig::read(&config);
        assert_eq!(
            (radar.width, radar.height, radar.margin_top),
            (400.0, 600.0, 80.0)
        );
    }
}
