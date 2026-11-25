//! Mermaid 11 `default`(라이트)·`dark` 테마에서 실제 SVG에 들어간 색.
//!
//! 값은 기준 SVG의 `<style>`에서 읽은 그대로다(parity/diagrams/baseline/*.svg).

use crate::scene::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Theme {
    Light,
    Dark,
}

/// CSS `hsl()`을 Chromium처럼 0..255 정수로 바꾼다
pub fn hsl(hue: f64, saturation: f64, lightness: f64) -> Color {
    let saturation = saturation / 100.0;
    let lightness = lightness / 100.0;
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let hue_section = (hue / 60.0) % 6.0;
    let x = chroma * (1.0 - (hue_section % 2.0 - 1.0).abs());
    let (red, green, blue) = match hue_section as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let offset = lightness - chroma / 2.0;
    let channel = |value: f64| ((value + offset) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color::rgb(channel(red), channel(green), channel(blue))
}

#[derive(Clone, Debug)]
pub struct Palette {
    /// 모바일 이미지 배경(diagram-worker body)
    pub page_background: Color,
    pub text: Color,
    pub node_fill: Color,
    pub node_stroke: Color,
    pub line: Color,
    pub cluster_fill: Color,
    pub cluster_stroke: Color,
    pub cluster_text: Color,
    /// 간선 라벨 바탕: 바깥 div(labelBkg)와 안쪽 문단(p) 두 겹
    pub edge_label_outer: Color,
    pub edge_label_inner: Color,
    pub pie: PiePalette,
}

#[derive(Clone, Debug)]
pub struct PiePalette {
    pub slices: [Color; 12],
    pub stroke: Color,
    pub outer_stroke: Color,
    pub title: Color,
    pub section_text: Color,
    pub legend_text: Color,
}

impl Palette {
    pub fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => light_palette(),
            Theme::Dark => dark_palette(),
        }
    }
}

fn light_palette() -> Palette {
    Palette {
        page_background: Color::rgb(255, 255, 255),
        text: Color::rgb(0x33, 0x33, 0x33),
        node_fill: Color::rgb(0xEC, 0xEC, 0xFF),
        node_stroke: Color::rgb(0x93, 0x70, 0xDB),
        line: Color::rgb(0x33, 0x33, 0x33),
        cluster_fill: Color::rgb(0xFF, 0xFF, 0xDE),
        cluster_stroke: Color::rgb(0xAA, 0xAA, 0x33),
        cluster_text: Color::rgb(0x33, 0x33, 0x33),
        edge_label_outer: Color::rgba(232, 232, 232, 0.5),
        edge_label_inner: Color::rgba(232, 232, 232, 0.8),
        pie: light_pie_palette(),
    }
}

fn dark_palette() -> Palette {
    let light_grey = Color::rgb(211, 211, 211);
    Palette {
        page_background: Color::rgb(0x11, 0x12, 0x16),
        text: Color::rgb(0xCC, 0xCC, 0xCC),
        node_fill: Color::rgb(0x1F, 0x20, 0x20),
        node_stroke: Color::rgb(0xCC, 0xCC, 0xCC),
        line: light_grey,
        cluster_fill: hsl(180.0, 1.587_301_587_3, 28.352_941_176_5),
        cluster_stroke: Color::rgba(255, 255, 255, 0.25),
        cluster_text: Color::rgb(0xF9, 0xFF, 0xFE),
        edge_label_outer: Color::rgba(88, 88, 88, 0.5),
        edge_label_inner: hsl(0.0, 0.0, 34.411_764_705_9),
        pie: dark_pie_palette(),
    }
}

fn light_pie_palette() -> PiePalette {
    PiePalette {
        slices: [
            Color::rgb(0xEC, 0xEC, 0xFF),
            Color::rgb(0xFF, 0xFF, 0xDE),
            hsl(80.0, 100.0, 56.274_509_803_9),
            hsl(240.0, 100.0, 86.274_509_803_9),
            hsl(60.0, 100.0, 63.529_411_764_7),
            hsl(80.0, 100.0, 76.274_509_803_9),
            hsl(300.0, 100.0, 76.274_509_803_9),
            hsl(180.0, 100.0, 56.274_509_803_9),
            hsl(0.0, 100.0, 56.274_509_803_9),
            hsl(300.0, 100.0, 56.274_509_803_9),
            hsl(150.0, 100.0, 56.274_509_803_9),
            hsl(0.0, 100.0, 66.274_509_803_9),
        ],
        stroke: Color::rgb(0, 0, 0),
        outer_stroke: Color::rgb(0, 0, 0),
        title: Color::rgb(0, 0, 0),
        section_text: Color::rgb(0x33, 0x33, 0x33),
        legend_text: Color::rgb(0, 0, 0),
    }
}

fn dark_pie_palette() -> PiePalette {
    let light_grey = Color::rgb(211, 211, 211);
    PiePalette {
        // theme-dark.js cScale1..12
        slices: [
            Color::rgb(0x0b, 0x00, 0x00),
            Color::rgb(0x4d, 0x10, 0x37),
            Color::rgb(0x3f, 0x52, 0x58),
            Color::rgb(0x4f, 0x2f, 0x1b),
            Color::rgb(0x6e, 0x0a, 0x0a),
            Color::rgb(0x3b, 0x00, 0x48),
            Color::rgb(0x99, 0x5a, 0x01),
            Color::rgb(0x15, 0x47, 0x06),
            Color::rgb(0x16, 0x17, 0x22),
            Color::rgb(0x00, 0x29, 0x6f),
            Color::rgb(0x01, 0x62, 0x9c),
            Color::rgb(0x01, 0x00, 0x29),
        ],
        stroke: Color::rgb(0, 0, 0),
        outer_stroke: Color::rgb(0, 0, 0),
        title: light_grey,
        section_text: Color::rgb(0xCC, 0xCC, 0xCC),
        legend_text: light_grey,
    }
}
