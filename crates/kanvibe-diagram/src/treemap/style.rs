//! treemap 설정(`treemap` 묶음)과 `classDef` 스타일. Mermaid `styles2String`은 글자 속성(`color`, `font-*` 등)을
//! 라벨 스타일로, 나머지를 상자 스타일로 나눠 `!important`로 붙인다. 같은 속성은 마지막 선언이 이긴다.

use std::collections::HashMap;

use serde_json::Value;

use super::value_format::ValueFormat;
use crate::directive::DiagramConfig;
use crate::scene::Color;
use crate::text::FontWeight;

/// renderer.ts가 읽는 `treemap` 설정. 칸 크기는 `nodeWidth·nodeHeight × 10`(0이면 960×500)이다
#[derive(Clone, Debug)]
pub struct TreemapConfig {
    pub inner_padding: f64,
    pub diagram_padding: f64,
    pub should_show_values: bool,
    pub width: f64,
    pub height: f64,
    pub value_format: ValueFormat,
}

impl TreemapConfig {
    pub fn from_config(config: &DiagramConfig) -> Self {
        let empty = serde_json::Map::new();
        let section = config.section("treemap").unwrap_or(&empty);
        let number = |key: &str| section.get(key).and_then(Value::as_f64);
        let canvas_size = |key: &str, default: f64, fallback: f64| match number(key) {
            Some(size) if size != 0.0 => size * 10.0,
            Some(_) => fallback,
            None => default * 10.0,
        };
        Self {
            inner_padding: number("padding").unwrap_or(10.0),
            diagram_padding: number("diagramPadding").unwrap_or(8.0),
            should_show_values: section.get("showValues").and_then(Value::as_bool) != Some(false),
            width: canvas_size("nodeWidth", 100.0, 960.0),
            height: canvas_size("nodeHeight", 40.0, 500.0),
            value_format: ValueFormat::new(
                section
                    .get("valueFormat")
                    .and_then(Value::as_str)
                    .filter(|format| !format.is_empty())
                    .unwrap_or(","),
            ),
        }
    }
}

/// `classDef`에서 읽은 상자·라벨 속성. 없는 속성은 Mermaid 기본값(속성값)을 쓴다
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassStyle {
    pub fill: Option<Color>,
    pub fill_opacity: Option<f32>,
    pub stroke: Option<Color>,
    pub stroke_width: Option<f64>,
    pub stroke_opacity: Option<f32>,
    pub dash: Vec<f64>,
    /// 상자 전체 `opacity`. 칠과 테두리 알파에 곱한다
    pub opacity: Option<f32>,
    /// 라벨 `color`(글자 `fill`이 된다)
    pub label_color: Option<Color>,
    pub font_size: Option<f64>,
    pub is_bold: Option<bool>,
    pub is_italic: Option<bool>,
}

impl ClassStyle {
    /// 라벨의 굵기·기울임. 클래스가 정하지 않으면 요소 기본값을 쓴다
    pub fn font_weight(&self, is_bold: bool, is_italic: bool) -> FontWeight {
        match (
            self.is_bold.unwrap_or(is_bold),
            self.is_italic.unwrap_or(is_italic),
        ) {
            (true, true) => FontWeight::BoldItalic,
            (true, false) => FontWeight::Bold,
            (false, true) => FontWeight::Italic,
            (false, false) => FontWeight::Regular,
        }
    }

    /// 칠 알파(`fill-opacity` × `opacity`)
    pub fn fill_alpha(&self, default_opacity: f32) -> f32 {
        self.fill_opacity.unwrap_or(default_opacity) * self.opacity.unwrap_or(1.0)
    }

    /// 테두리 알파(`stroke-opacity` × `opacity`)
    pub fn stroke_alpha(&self, default_opacity: f32) -> f32 {
        self.stroke_opacity.unwrap_or(default_opacity) * self.opacity.unwrap_or(1.0)
    }

    fn apply(&mut self, key: &str, value: &str) {
        let number = || value.trim_end_matches("px").trim().parse::<f64>().ok();
        match key {
            "fill" => self.fill = Color::parse_css(value),
            "fill-opacity" => self.fill_opacity = number().map(|value| value as f32),
            "stroke" => self.stroke = Color::parse_css(value),
            "stroke-width" => self.stroke_width = number(),
            "stroke-opacity" => self.stroke_opacity = number().map(|value| value as f32),
            "stroke-dasharray" => self.dash = parse_dash(value),
            "opacity" => self.opacity = number().map(|value| value as f32),
            "color" => self.label_color = Color::parse_css(value),
            "font-size" => self.font_size = number(),
            "font-weight" => self.is_bold = Some(is_bold_weight(value)),
            "font-style" => self.is_italic = Some(matches!(value, "italic" | "oblique")),
            _ => {}
        }
    }
}

fn is_bold_weight(value: &str) -> bool {
    match value {
        "bold" | "bolder" => true,
        other => other.parse::<f64>().is_ok_and(|weight| weight >= 600.0),
    }
}

fn parse_dash(value: &str) -> Vec<f64> {
    value
        .split([' ', ','])
        .filter_map(|part| part.trim_end_matches("px").parse::<f64>().ok())
        .collect()
}

/// `classDef` 선언 목록을 클래스별 스타일로 읽는다(`styles2Map`: `키:값`, 같은 키는 뒤가 이긴다)
pub fn read_class_styles(classes: &HashMap<String, Vec<String>>) -> HashMap<String, ClassStyle> {
    let mut styles = HashMap::new();
    for (name, declarations) in classes {
        let mut style = ClassStyle::default();
        for declaration in declarations {
            let mut parts = declaration.split(':');
            let key = parts.next().unwrap_or("").trim();
            let value = parts.next().unwrap_or("").trim();
            style.apply(key, value.trim_end_matches("!important").trim());
        }
        styles.insert(name.clone(), style);
    }
    styles
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directive::read_config;

    #[test]
    fn classdef_상자와_라벨_속성을_모두_읽는다() {
        let classes = HashMap::from([(
            "dashed".to_string(),
            [
                "stroke:#c00",
                "stroke-dasharray:6 3",
                "fill-opacity:0.2",
                "font-weight:bold",
                "font-size:20px",
                "color:#004 !important",
                "opacity:0.8",
            ]
            .map(String::from)
            .to_vec(),
        )]);
        let style = &read_class_styles(&classes)["dashed"];
        assert_eq!(style.dash, vec![6.0, 3.0]);
        assert_eq!(style.fill_alpha(0.6), 0.2 * 0.8);
        assert_eq!(style.font_size, Some(20.0));
        assert_eq!(style.font_weight(false, false), FontWeight::Bold);
        assert_eq!(style.label_color, Some(Color::rgb(0, 0, 0x44)));
    }

    #[test]
    fn 앞머리_treemap_설정을_읽는다() {
        let config = TreemapConfig::from_config(&read_config(
            "---\nconfig:\n  treemap:\n    padding: 4\n    nodeWidth: 80\n    showValues: false\n---\ntreemap",
        ));
        assert_eq!(config.inner_padding, 4.0);
        assert_eq!(config.width, 800.0);
        assert_eq!(config.height, 400.0);
        assert!(!config.should_show_values);
    }
}
