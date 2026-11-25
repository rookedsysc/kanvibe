//! 글자 폭 측정과 글리프 윤곽선.
//!
//! 기준 이미지는 Mermaid 기본 글꼴 지정(`"trebuchet ms", verdana, arial, sans-serif`)이 고정 fontconfig에서
//! Noto Sans로 풀리고, 한글은 Noto Sans CJK KR로 대체된 결과다. 여기서도 같은 파일을 같은 순서로 쓰고,
//! Chromium처럼 HarfBuzz(rustybuzz)로 글자를 이어 붙여 폭을 잰다. 글꼴 바이트는 호출하는 쪽이 넘긴다.

use std::path::Path;
use std::sync::Arc;

use rustybuzz::{Face, UnicodeBuffer};
use tiny_skia::{Path as SkiaPath, PathBuilder};
use ttf_parser::OutlineBuilder;

/// Chromium 레이아웃 단위. 글자 폭은 1/64px 단위로 올림된다
const LAYOUT_UNIT_SCALE: f64 = 64.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontWeight {
    Regular,
    Bold,
    /// 보통 굵기의 이탤릭 얼굴(`font-style: italic`). 이탤릭 얼굴이 없는 family는 보통 얼굴을 쓴다
    Italic,
    /// 굵은 이탤릭 얼굴
    BoldItalic,
}

#[derive(Clone)]
struct FontFace {
    data: Arc<Vec<u8>>,
    index: u32,
    rank: usize,
    is_bold: bool,
    is_italic: bool,
}

/// 글자별 대체 순서대로 쌓은 글꼴 묶음
#[derive(Clone, Default)]
pub struct FontBook {
    faces: Vec<FontFace>,
}

/// 한 글꼴로 이어 그릴 글리프 하나
#[derive(Clone, Debug)]
pub struct PositionedGlyph {
    pub face_index: usize,
    pub glyph_id: u16,
    /// 줄 시작에서 잰 글리프 원점(px)
    pub x: f64,
}

/// 한 줄을 모양 잡은 결과
#[derive(Clone, Debug, Default)]
pub struct ShapedLine {
    pub glyphs: Vec<PositionedGlyph>,
    pub width: f64,
}

/// 글꼴 세로 지표(px). Chromium처럼 ascent·descent를 정수로 반올림한 값이다
#[derive(Clone, Copy, Debug)]
pub struct LineMetrics {
    pub ascent: f64,
    pub descent: f64,
}

/// SVG `<text>`가 `getBBox()`로 돌려주는 가로 범위(px, 글자 시작점 기준).
///
/// Chromium은 SVG 글자 상자를 글자 진행 폭과 글리프 잉크 상자의 합집합으로 잡는다. 잉크 상자는 Skia가 글리프를
/// 그릴 때 쓰는 정수 픽셀 경계(왼쪽 내림, 오른쪽 올림)라서 'A'·'y'·'r'처럼 끝 글자 잉크가 진행 폭에 붙은 줄은
/// 진행 폭보다 최대 1px 넓어진다. 진행 폭은 `tspan` 글자 노드마다 레이아웃 단위로 올림해 더한다
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SvgTextExtent {
    /// `getComputedTextLength()`와 같은 진행 폭
    pub advance: f64,
    pub left: f64,
    pub right: f64,
}

impl SvgTextExtent {
    /// `getBBox().width`
    pub fn width(&self) -> f64 {
        self.right - self.left
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FontLoadError {
    #[error("글꼴 파일을 읽지 못했습니다: {0}")]
    Read(#[from] std::io::Error),
    #[error("Noto Sans와 Noto Sans CJK KR 글꼴이 모두 있어야 합니다")]
    MissingRequiredFace,
}

/// family 이름에서 대체 순서를 정한다. 앱 번들 글꼴의 `KanVibe ` 접두어는 무시한다
fn fallback_rank(family: &str) -> Option<usize> {
    let name = family.trim_start_matches("KanVibe ");
    match name {
        "Noto Sans" => Some(0),
        "Noto Sans CJK KR" => Some(1),
        "Noto Sans Symbols" => Some(2),
        "Noto Sans Symbols2" => Some(3),
        "DejaVu Sans" => Some(4),
        _ => None,
    }
}

fn read_family_name(face: &ttf_parser::Face) -> Option<String> {
    let names = face.names();
    let preferred = names
        .into_iter()
        .find(|name| name.name_id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY && name.is_unicode());
    let family = names
        .into_iter()
        .find(|name| name.name_id == ttf_parser::name_id::FAMILY && name.is_unicode());
    preferred.or(family).and_then(|name| name.to_string())
}

impl FontBook {
    /// 글꼴 파일 바이트(ttf·otf·ttc)를 받아 대체 순서대로 정리한다. 모르는 family는 버린다
    pub fn from_font_files(files: Vec<Vec<u8>>) -> Result<Self, FontLoadError> {
        let mut ranked: Vec<(usize, FontFace)> = Vec::new();
        for bytes in files {
            let data = Arc::new(bytes);
            let count = ttf_parser::fonts_in_collection(&data).unwrap_or(1);
            for index in 0..count {
                let Ok(face) = ttf_parser::Face::parse(&data, index) else {
                    continue;
                };
                let Some(rank) = read_family_name(&face).as_deref().and_then(fallback_rank) else {
                    continue;
                };
                let is_bold = face.is_bold();
                let is_italic = face.is_italic();
                ranked.push((
                    rank,
                    FontFace {
                        data: data.clone(),
                        index,
                        rank,
                        is_bold,
                        is_italic,
                    },
                ));
            }
        }
        ranked.sort_by_key(|(rank, face)| (*rank, face.is_bold, face.is_italic));
        let has_sans = ranked.iter().any(|(rank, _)| *rank == 0);
        let has_cjk = ranked.iter().any(|(rank, _)| *rank == 1);
        if !has_sans || !has_cjk {
            return Err(FontLoadError::MissingRequiredFace);
        }
        let faces = ranked.into_iter().map(|(_, face)| face).collect();
        Ok(Self { faces })
    }

    /// 디렉터리 안의 글꼴 파일을 모두 읽는다(앱 번들 `assets/fonts` 등)
    pub fn load_dir(dir: &Path) -> Result<Self, FontLoadError> {
        let mut files = Vec::new();
        let mut entries: Vec<_> = std::fs::read_dir(dir)?
            .flatten()
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for file_path in entries {
            let extension = file_path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if matches!(extension, "ttf" | "otf" | "ttc") {
                files.push(std::fs::read(&file_path)?);
            }
        }
        Self::from_font_files(files)
    }

    fn parse_face(&self, face_index: usize) -> Option<Face<'_>> {
        let face = &self.faces[face_index];
        Face::from_slice(&face.data, face.index)
    }

    /// 굵기·기울임에 맞는 얼굴만 남긴 대체 순서. 맞는 얼굴이 없는 family는 보통 얼굴을 쓴다
    fn candidates(&self, weight: FontWeight) -> Vec<usize> {
        let wants_bold = matches!(weight, FontWeight::Bold | FontWeight::BoldItalic);
        let wants_italic = matches!(weight, FontWeight::Italic | FontWeight::BoldItalic);
        let mut chosen: Vec<usize> = Vec::new();
        for (index, face) in self.faces.iter().enumerate() {
            match chosen.last_mut() {
                Some(last)
                    if self.faces[*last].rank == face.rank
                        && face.is_bold == wants_bold
                        && face.is_italic == wants_italic =>
                {
                    *last = index
                }
                Some(last) if self.faces[*last].rank == face.rank => {}
                _ => chosen.push(index),
            }
        }
        chosen
    }

    /// 글자마다 그 글자를 가진 첫 얼굴을 고른다
    fn pick_face(&self, candidates: &[usize], character: char) -> usize {
        for &index in candidates {
            let face = &self.faces[index];
            let Ok(parsed) = ttf_parser::Face::parse(&face.data, face.index) else {
                continue;
            };
            if parsed.glyph_index(character).is_some() {
                return index;
            }
        }
        candidates[0]
    }

    /// 한 줄을 얼굴별 구간으로 나눠 모양 잡는다
    pub fn shape_line(&self, text: &str, font_size: f64, weight: FontWeight) -> ShapedLine {
        let candidates = self.candidates(weight);
        let mut line = ShapedLine::default();
        for (face_index, segment) in self.split_by_face(&candidates, text) {
            self.append_segment(&mut line, face_index, &segment, font_size);
        }
        line
    }

    fn split_by_face(&self, candidates: &[usize], text: &str) -> Vec<(usize, String)> {
        let mut segments: Vec<(usize, String)> = Vec::new();
        for character in text.chars() {
            // 공백도 다른 글자처럼 그 글자를 가진 첫 얼굴(Noto Sans)로 간다. 한글 사이 공백도 마찬가지다
            let face_index = self.pick_face(candidates, character);
            match segments.last_mut() {
                Some((index, segment)) if *index == face_index => segment.push(character),
                _ => segments.push((face_index, character.to_string())),
            }
        }
        segments
    }

    fn append_segment(
        &self,
        line: &mut ShapedLine,
        face_index: usize,
        segment: &str,
        font_size: f64,
    ) {
        let Some(face) = self.parse_face(face_index) else {
            return;
        };
        let scale = freetype_scale(font_size, face.units_per_em());
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(segment);
        let output = rustybuzz::shape(&face, &[], buffer);
        for (info, position) in output.glyph_infos().iter().zip(output.glyph_positions()) {
            let glyph_id = u16::try_from(info.glyph_id).unwrap_or(0);
            let x = line.width + f64::from(position.x_offset) * scale;
            line.glyphs.push(PositionedGlyph {
                face_index,
                glyph_id,
                x,
            });
            line.width += f64::from(position.x_advance) * scale;
        }
    }

    /// 한 줄의 폭(px)을 Chromium 레이아웃 단위로 올림해 돌려준다
    pub fn measure_line(&self, text: &str, font_size: f64, weight: FontWeight) -> f64 {
        snap_layout_unit(self.shape_line(text, font_size, weight).width)
    }

    /// 이어 붙은 글자 노드(`tspan`)들을 SVG 글자로 잰다. 글자 노드 하나짜리 `<text>`면 `runs`가 한 개다
    pub fn measure_svg_runs(
        &self,
        runs: &[&str],
        font_size: f64,
        weight: FontWeight,
    ) -> SvgTextExtent {
        let mut extent = SvgTextExtent::default();
        for run in runs {
            let line = self.shape_line(run, font_size, weight);
            for glyph in &line.glyphs {
                let Some(bounds) = self
                    .glyph_outline(glyph, font_size)
                    .map(|path| path.bounds())
                else {
                    continue;
                };
                let origin = extent.advance + glyph.x;
                extent.left = extent.left.min(origin + f64::from(bounds.left()).floor());
                extent.right = extent.right.max(origin + f64::from(bounds.right()).ceil());
            }
            extent.advance += snap_layout_unit(line.width);
        }
        extent.right = extent.right.max(extent.advance);
        extent
    }

    /// 첫 얼굴(Noto Sans)의 세로 지표. 줄 높이가 고정이면 대체 글꼴은 줄 상자를 바꾸지 않는다
    pub fn primary_metrics(&self, font_size: f64) -> LineMetrics {
        let Some(face) = self.parse_face(0) else {
            return LineMetrics {
                ascent: font_size,
                descent: 0.0,
            };
        };
        let scale = font_size / f64::from(face.units_per_em());
        LineMetrics {
            ascent: (f64::from(face.ascender()) * scale).round(),
            descent: (f64::from(-face.descender()) * scale).round(),
        }
    }

    /// 글리프가 첫 얼굴(Noto Sans)의 것인지. 라틴 x-height 맞춤은 이 얼굴 글리프에만 쓴다
    pub fn is_primary_face(&self, face_index: usize) -> bool {
        self.faces[face_index].rank == 0
    }

    /// 글리프 윤곽선을 원점 기준 px 좌표(y는 아래로 증가)로 돌려준다
    pub fn glyph_outline(&self, glyph: &PositionedGlyph, font_size: f64) -> Option<SkiaPath> {
        let face = self.parse_face(glyph.face_index)?;
        let scale = (font_size / f64::from(face.units_per_em())) as f32;
        let mut builder = OutlineToPath {
            builder: PathBuilder::new(),
            scale,
        };
        face.outline_glyph(ttf_parser::GlyphId(glyph.glyph_id), &mut builder)?;
        builder.builder.finish()
    }
}

/// FreeType이 글자 크기를 26.6px로 받아 `FT_DivFix`로 반올림한 배율(px/글꼴 단위).
/// Chromium의 글자 폭은 정확한 `크기 / upem`보다 이 반올림만큼 크고, 1/64px 올림 경계에서 그 차이가 드러난다
fn freetype_scale(font_size: f64, units_per_em: i32) -> f64 {
    const FIXED_ONE: f64 = 65_536.0;
    let x_scale = (font_size * 64.0 * FIXED_ONE / f64::from(units_per_em)).round();
    x_scale / FIXED_ONE / 64.0
}

/// 소수 px 폭을 1/64px 단위로 올림한다(LayoutUnit::FromFloatCeil)
pub fn snap_layout_unit(width: f64) -> f64 {
    (width * LAYOUT_UNIT_SCALE - 1e-6).ceil() / LAYOUT_UNIT_SCALE
}

struct OutlineToPath {
    builder: PathBuilder,
    scale: f32,
}

impl OutlineBuilder for OutlineToPath {
    fn move_to(&mut self, x: f32, y: f32) {
        self.builder.move_to(x * self.scale, -y * self.scale);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.builder.line_to(x * self.scale, -y * self.scale);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let scale = self.scale;
        self.builder
            .quad_to(x1 * scale, -y1 * scale, x * scale, -y * scale);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let scale = self.scale;
        self.builder.cubic_to(
            x1 * scale,
            -y1 * scale,
            x2 * scale,
            -y2 * scale,
            x * scale,
            -y * scale,
        );
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 시험은 앱이 번들한 기준 글꼴을 그대로 읽는다
    pub(crate) fn bundled_fonts() -> FontBook {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../kanvibe-app/assets/fonts");
        FontBook::load_dir(&dir).expect("앱 번들 글꼴이 있어야 합니다")
    }

    #[test]
    fn 기준_svg의_라벨_폭과_같게_잰다() {
        let fonts = bundled_fonts();
        assert_eq!(
            fonts.measure_line("A", 16.0, FontWeight::Regular),
            10.234375
        );
        assert_eq!(fonts.measure_line("B", 16.0, FontWeight::Regular), 10.40625);
    }

    #[test]
    fn svg_글자_상자는_끝_글자_잉크를_정수_픽셀로_넣는다() {
        let fonts = bundled_fonts();
        // fc-*--desktop 기준 SVG의 라벨 getBBox 폭
        let single = fonts.measure_svg_runs(&["A"], 16.0, FontWeight::Regular);
        assert_eq!(single.width(), 11.0);
        let words = fonts.measure_svg_runs(
            &["Istio", " gateway", " Service"],
            16.0,
            FontWeight::Regular,
        );
        assert_eq!(words.width(), 157.40625);
        let overhang = fonts.measure_svg_runs(&["Start"], 16.0, FontWeight::Regular);
        assert!(
            (overhang.width() - 36.144_058).abs() < 0.02,
            "{}",
            overhang.width()
        );
    }

    #[test]
    fn 한글은_cjk_얼굴로_넘긴다() {
        let fonts = bundled_fonts();
        let line = fonts.shape_line("a가", 16.0, FontWeight::Regular);
        assert_eq!(line.glyphs.len(), 2);
        assert_ne!(line.glyphs[0].face_index, line.glyphs[1].face_index);
    }
}
