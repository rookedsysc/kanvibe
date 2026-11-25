//! quadrantChart 설정(`config.quadrantChart`, quadrantBuilder getDefaultConfig). 앞머리나 `init` 지시문으로
//! 바꾼 값만 기본값을 덮는다.

use serde_json::{Map, Value};

use crate::directive::DiagramConfig;

#[derive(Clone, Debug, PartialEq)]
pub struct ChartConfig {
    pub chart_width: f64,
    pub chart_height: f64,
    pub show_x_axis: bool,
    pub show_y_axis: bool,
    pub show_title: bool,
    pub title_padding: f64,
    pub title_font_size: f64,
    pub quadrant_padding: f64,
    pub x_axis_label_padding: f64,
    pub y_axis_label_padding: f64,
    pub x_axis_label_font_size: f64,
    pub y_axis_label_font_size: f64,
    pub quadrant_label_font_size: f64,
    pub quadrant_text_top_padding: f64,
    pub point_text_padding: f64,
    pub point_label_font_size: f64,
    pub point_radius: f64,
    /// `xAxisPosition: "top"`(점이 있으면 무시하고 아래)
    pub is_x_axis_top: bool,
    /// `yAxisPosition: "left"`
    pub is_y_axis_left: bool,
    pub internal_border_width: f64,
    pub external_border_width: f64,
}

impl Default for ChartConfig {
    /// Mermaid defaultConfig.quadrantChart
    fn default() -> Self {
        Self {
            chart_width: 500.0,
            chart_height: 500.0,
            show_x_axis: true,
            show_y_axis: true,
            show_title: true,
            title_padding: 10.0,
            title_font_size: 20.0,
            quadrant_padding: 5.0,
            x_axis_label_padding: 5.0,
            y_axis_label_padding: 5.0,
            x_axis_label_font_size: 16.0,
            y_axis_label_font_size: 16.0,
            quadrant_label_font_size: 16.0,
            quadrant_text_top_padding: 5.0,
            point_text_padding: 5.0,
            point_label_font_size: 12.0,
            point_radius: 5.0,
            is_x_axis_top: true,
            is_y_axis_left: true,
            internal_border_width: 1.0,
            external_border_width: 2.0,
        }
    }
}

impl ChartConfig {
    /// 형이 맞지 않는 값은 무시하고 기본값을 둔다
    pub fn from_config(config: &DiagramConfig) -> Self {
        let mut chart = Self::default();
        let Some(section) = config.section("quadrantChart") else {
            return chart;
        };
        let numbers: [(&str, &mut f64); 16] = [
            ("chartWidth", &mut chart.chart_width),
            ("chartHeight", &mut chart.chart_height),
            ("titlePadding", &mut chart.title_padding),
            ("titleFontSize", &mut chart.title_font_size),
            ("quadrantPadding", &mut chart.quadrant_padding),
            ("xAxisLabelPadding", &mut chart.x_axis_label_padding),
            ("yAxisLabelPadding", &mut chart.y_axis_label_padding),
            ("xAxisLabelFontSize", &mut chart.x_axis_label_font_size),
            ("yAxisLabelFontSize", &mut chart.y_axis_label_font_size),
            ("quadrantLabelFontSize", &mut chart.quadrant_label_font_size),
            (
                "quadrantTextTopPadding",
                &mut chart.quadrant_text_top_padding,
            ),
            ("pointTextPadding", &mut chart.point_text_padding),
            ("pointLabelFontSize", &mut chart.point_label_font_size),
            ("pointRadius", &mut chart.point_radius),
            (
                "quadrantInternalBorderStrokeWidth",
                &mut chart.internal_border_width,
            ),
            (
                "quadrantExternalBorderStrokeWidth",
                &mut chart.external_border_width,
            ),
        ];
        for (key, target) in numbers {
            if let Some(value) = section.get(key).and_then(Value::as_f64) {
                *target = value;
            }
        }
        read_flags(section, &mut chart);
        chart
    }
}

fn read_flags(section: &Map<String, Value>, chart: &mut ChartConfig) {
    let flag = |key: &str| section.get(key).and_then(Value::as_bool);
    chart.show_x_axis = flag("showXAxis").unwrap_or(chart.show_x_axis);
    chart.show_y_axis = flag("showYAxis").unwrap_or(chart.show_y_axis);
    chart.show_title = flag("showTitle").unwrap_or(chart.show_title);
    if let Some(position) = section.get("xAxisPosition").and_then(Value::as_str) {
        chart.is_x_axis_top = position == "top";
    }
    if let Some(position) = section.get("yAxisPosition").and_then(Value::as_str) {
        chart.is_y_axis_left = position == "left";
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directive::read_config;

    #[test]
    fn 앞머리의_차트_크기와_축_위치를_읽는다() {
        let config = read_config(
            "---\nconfig:\n  quadrantChart:\n    chartWidth: 400\n    yAxisPosition: right\n    pointRadius: \"8\"\n---\nquadrantChart",
        );
        let chart = ChartConfig::from_config(&config);
        assert_eq!(chart.chart_width, 400.0);
        assert_eq!(chart.chart_height, 500.0);
        assert!(!chart.is_y_axis_left);
        assert_eq!(chart.point_radius, 5.0);
    }
}
