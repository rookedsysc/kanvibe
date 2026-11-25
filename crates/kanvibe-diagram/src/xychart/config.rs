//! xychart 설정(`config.xyChart`, defaultConfig.xyChart)과 테마 색(themeVariables.xyChart 기본값).
//!
//! 앞머리 `config`나 `init` 지시문에서 형이 맞는 값만 기본값을 덮는다. 색은 테마 기본값만 쓴다(TS 모바일 워커가
//! `themeVariables`를 `secure`로 막는다).

use crate::chart_config::ConfigSection;
use crate::directive::DiagramConfig;
use crate::scene::Color;
use crate::theme::Theme;

/// x축·y축 설정(XYChartAxisConfig)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisConfig {
    pub show_label: bool,
    pub label_font_size: f64,
    pub label_padding: f64,
    pub show_title: bool,
    pub title_font_size: f64,
    pub title_padding: f64,
    pub show_tick: bool,
    pub tick_length: f64,
    pub tick_width: f64,
    pub show_axis_line: bool,
    pub axis_line_width: f64,
}

impl Default for AxisConfig {
    fn default() -> Self {
        Self {
            show_label: true,
            label_font_size: 14.0,
            label_padding: 5.0,
            show_title: true,
            title_font_size: 16.0,
            title_padding: 5.0,
            show_tick: true,
            tick_length: 5.0,
            tick_width: 2.0,
            show_axis_line: true,
            axis_line_width: 2.0,
        }
    }
}

impl AxisConfig {
    fn read(section: ConfigSection) -> Self {
        let mut axis = Self::default();
        section.read_flag("showLabel", &mut axis.show_label);
        section.read_number("labelFontSize", &mut axis.label_font_size);
        section.read_number("labelPadding", &mut axis.label_padding);
        section.read_flag("showTitle", &mut axis.show_title);
        section.read_number("titleFontSize", &mut axis.title_font_size);
        section.read_number("titlePadding", &mut axis.title_padding);
        section.read_flag("showTick", &mut axis.show_tick);
        section.read_number("tickLength", &mut axis.tick_length);
        section.read_number("tickWidth", &mut axis.tick_width);
        section.read_flag("showAxisLine", &mut axis.show_axis_line);
        section.read_number("axisLineWidth", &mut axis.axis_line_width);
        axis
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChartConfig {
    pub width: f64,
    pub height: f64,
    pub title_font_size: f64,
    pub title_padding: f64,
    pub show_title: bool,
    pub show_data_label: bool,
    pub show_data_label_outside_bar: bool,
    /// `chartOrientation: horizontal`. 머리줄의 `vertical`·`horizontal`이 이긴다
    pub is_horizontal: bool,
    pub plot_reserved_space_percent: f64,
    pub x_axis: AxisConfig,
    pub y_axis: AxisConfig,
}

impl ChartConfig {
    pub fn read(config: &DiagramConfig) -> Self {
        let section = ConfigSection::of(config, "xyChart");
        let mut chart = Self {
            width: 700.0,
            height: 500.0,
            title_font_size: 20.0,
            title_padding: 10.0,
            show_title: true,
            show_data_label: false,
            show_data_label_outside_bar: false,
            is_horizontal: section.text("chartOrientation") == Some("horizontal"),
            plot_reserved_space_percent: 50.0,
            x_axis: AxisConfig::read(section.child("xAxis")),
            y_axis: AxisConfig::read(section.child("yAxis")),
        };
        section.read_number("width", &mut chart.width);
        section.read_number("height", &mut chart.height);
        section.read_number("titleFontSize", &mut chart.title_font_size);
        section.read_number("titlePadding", &mut chart.title_padding);
        section.read_flag("showTitle", &mut chart.show_title);
        section.read_flag("showDataLabel", &mut chart.show_data_label);
        section.read_flag(
            "showDataLabelOutsideBar",
            &mut chart.show_data_label_outside_bar,
        );
        section.read_number(
            "plotReservedSpacePercent",
            &mut chart.plot_reserved_space_percent,
        );
        chart
    }
}

/// themeVariables.xyChart 기본값. TS 워커는 `themeVariables`를 `secure`로 막아 정의에서 바꿀 수 없다
#[derive(Clone, Debug, PartialEq)]
pub struct ChartColors {
    pub background: Color,
    /// 제목·축·데이터 라벨 글자와 선(primaryTextColor)
    pub text: Color,
    pub plots: [Color; 10],
}

fn hex(value: u32) -> Color {
    Color::rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

impl ChartColors {
    pub fn for_theme(theme: Theme) -> Self {
        let (background, text, plots) = match theme {
            Theme::Light => (
                0xFFFFFF,
                0x131300,
                [
                    0xECECFF, 0x8493A6, 0xFFC3A0, 0xDCDDE1, 0xB8E994, 0xD1A36F, 0xC3CDE6, 0xFFB6C1,
                    0x496078, 0xF8F3E3,
                ],
            ),
            Theme::Dark => (
                0x333333,
                0xE0DFDF,
                [
                    0x3498DB, 0x2ECC71, 0xE74C3C, 0xF1C40F, 0xBDC3C7, 0xFFFFFF, 0x34495E, 0x9B59B6,
                    0x1ABC9C, 0xE67E22,
                ],
            ),
        };
        Self {
            background: hex(background),
            text: hex(text),
            plots: plots.map(hex),
        }
    }

    /// getPlotColorFromPalette
    pub fn plot(&self, index: usize) -> Color {
        self.plots[index % self.plots.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directive::read_config;

    #[test]
    fn 앞머리의_축_설정을_읽는다() {
        let config = read_config(
            "---\nconfig:\n  xyChart:\n    width: 600\n    showDataLabel: true\n    chartOrientation: horizontal\n    xAxis:\n      showTick: false\n---\nxychart",
        );
        let chart = ChartConfig::read(&config);
        assert_eq!(chart.width, 600.0);
        assert!(chart.show_data_label && chart.is_horizontal);
        assert!(!chart.x_axis.show_tick && chart.y_axis.show_tick);
    }
}
