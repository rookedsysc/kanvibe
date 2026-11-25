//! 장면을 tiny-skia로 그린다. 모바일 이미지는 diagram-worker.html과 같은 배치(좌우 8px 여백, 가운데 정렬,
//! 폭 `width - 16`과 높이 2384px 중 작은 배율, 위아래 8px)로 그리고 PNG·JPEG로 인코딩한다.

use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, StrokeDash, Transform,
};

use crate::scene::{Color, PathCommand, PathItem, Scene, SceneItem, TextItem};
use crate::text::FontBook;

/// 워커 body padding
const PAGE_PADDING: f64 = 8.0;
/// 워커가 그림에 허락하는 최대 높이(2400 - 16)
const MAX_DIAGRAM_HEIGHT: f64 = 2384.0;
pub const MAX_IMAGE_HEIGHT: u32 = 2400;

/// 장면 좌표 → 이미지 픽셀 좌표
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub scale: f64,
    pub offset_x: f64,
    pub offset_y: f64,
}

impl Placement {
    fn transform(&self) -> Transform {
        Transform::from_row(
            self.scale as f32,
            0.0,
            0.0,
            self.scale as f32,
            self.offset_x as f32,
            self.offset_y as f32,
        )
    }
}

/// 모바일 워커 배치 계산 결과
#[derive(Clone, Copy, Debug)]
pub struct MobileLayout {
    pub placement: Placement,
    pub image_height: u32,
    pub natural_width: u32,
}

/// diagramWorker/main.ts의 배율·크기 계산
pub fn mobile_layout(scene: &Scene, width: u32) -> MobileLayout {
    let content_width = f64::from(width) - PAGE_PADDING * 2.0;
    let scale = (content_width / scene.width).min(MAX_DIAGRAM_HEIGHT / scene.height);
    let actual_width = (scene.width * scale).floor().max(1.0);
    let actual_height = (scene.height * scale).floor().max(1.0);
    // preserveAspectRatio xMidYMid meet: viewBox를 실제 크기 안에 가운데 맞춘다
    let fit = (actual_width / scene.width).min(actual_height / scene.height);
    let left = PAGE_PADDING + (content_width - actual_width) / 2.0;
    let offset_x = left + (actual_width - scene.width * fit) / 2.0;
    let offset_y = PAGE_PADDING + (actual_height - scene.height * fit) / 2.0;
    let image_height = ((actual_height + PAGE_PADDING * 2.0).ceil() as u32).min(MAX_IMAGE_HEIGHT);
    MobileLayout {
        placement: Placement {
            scale: fit,
            offset_x,
            offset_y,
        },
        image_height,
        natural_width: scene.width.ceil() as u32,
    }
}

fn paint_for(color: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(
        color.red,
        color.green,
        color.blue,
        (color.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    );
    paint.anti_alias = true;
    paint
}

fn build_path(commands: &[PathCommand]) -> Option<tiny_skia::Path> {
    let mut builder = PathBuilder::new();
    for command in commands {
        match command {
            PathCommand::MoveTo(point) => builder.move_to(point.x as f32, point.y as f32),
            PathCommand::LineTo(point) => builder.line_to(point.x as f32, point.y as f32),
            PathCommand::QuadTo(control, point) => builder.quad_to(
                control.x as f32,
                control.y as f32,
                point.x as f32,
                point.y as f32,
            ),
            PathCommand::CubicTo(first, second, point) => builder.cubic_to(
                first.x as f32,
                first.y as f32,
                second.x as f32,
                second.y as f32,
                point.x as f32,
                point.y as f32,
            ),
            PathCommand::Close => builder.close(),
        }
    }
    builder.finish()
}

fn draw_path(pixmap: &mut Pixmap, item: &PathItem, transform: Transform) {
    let Some(path) = build_path(&item.commands) else {
        return;
    };
    if let Some(fill) = item.fill {
        pixmap.fill_path(&path, &paint_for(fill), FillRule::Winding, transform, None);
    }
    let Some(style) = &item.stroke else {
        return;
    };
    if style.width <= 0.0 {
        return;
    }
    let mut stroke = Stroke {
        width: style.width as f32,
        line_cap: LineCap::Butt,
        line_join: LineJoin::Miter,
        ..Stroke::default()
    };
    stroke.miter_limit = 4.0;
    if !style.dash.is_empty() {
        let dashes: Vec<f32> = style.dash.iter().map(|value| *value as f32).collect();
        stroke.dash = StrokeDash::new(dashes, 0.0);
    }
    pixmap.stroke_path(&path, &paint_for(style.color), &stroke, transform, None);
}

/// Noto Sans x-height(글꼴 단위 1000 기준)
const X_HEIGHT_RATIO: f64 = 0.536;

/// FreeType hintslight처럼 세로 방향만 맞춘다: 기준선을 장치 픽셀에 두고 x-height가 정수 픽셀이 되게
/// 세로 배율을 고친다. 가로 글리프 위치는 부분 픽셀 그대로 둔다(맞추면 기준 이미지와 더 멀어진다)
fn glyph_transform(item: &TextItem, glyph_x: f64, placement: Placement) -> Transform {
    let baseline = (item.baseline * placement.scale + placement.offset_y).round();
    let origin_x = (item.x + glyph_x) * placement.scale + placement.offset_x;
    let x_height = X_HEIGHT_RATIO * item.font_size * placement.scale;
    let vertical = if x_height >= 1.0 {
        x_height.round() / x_height
    } else {
        1.0
    };
    let scale = placement.scale;
    Transform::from_row(
        scale as f32,
        0.0,
        0.0,
        (scale * vertical) as f32,
        origin_x as f32,
        baseline as f32,
    )
}

fn draw_text(pixmap: &mut Pixmap, fonts: &FontBook, item: &TextItem, placement: Placement) {
    let line = fonts.shape_line(&item.text, item.font_size, item.weight);
    let paint = paint_for(item.color);
    for glyph in &line.glyphs {
        let Some(outline) = fonts.glyph_outline(glyph, item.font_size) else {
            continue;
        };
        pixmap.fill_path(
            &outline,
            &paint,
            FillRule::Winding,
            glyph_transform(item, glyph.x, placement),
            None,
        );
    }
}

/// 장면을 배치대로 이미 배경을 칠한 픽스맵 위에 그린다
pub fn draw_scene(pixmap: &mut Pixmap, scene: &Scene, fonts: &FontBook, placement: Placement) {
    let transform = placement.transform();
    for item in &scene.items {
        match item {
            SceneItem::Path(path) => draw_path(pixmap, path, transform),
            SceneItem::Text(text) => draw_text(pixmap, fonts, text, placement),
        }
    }
}

/// 모바일 이미지 크기의 픽스맵에 배경과 장면을 그린다
pub fn rasterize_mobile(
    scene: &Scene,
    fonts: &FontBook,
    background: Color,
    width: u32,
) -> Option<(Pixmap, MobileLayout)> {
    let layout = mobile_layout(scene, width);
    let mut pixmap = Pixmap::new(width, layout.image_height)?;
    pixmap.fill(tiny_skia::Color::from_rgba8(
        background.red,
        background.green,
        background.blue,
        255,
    ));
    draw_scene(&mut pixmap, scene, fonts, layout.placement);
    Some((pixmap, layout))
}

fn to_rgb_image(pixmap: &Pixmap) -> image::RgbImage {
    let mut rgb = image::RgbImage::new(pixmap.width(), pixmap.height());
    for (pixel, target) in pixmap.pixels().iter().zip(rgb.pixels_mut()) {
        let color = pixel.demultiply();
        *target = image::Rgb([color.red(), color.green(), color.blue()]);
    }
    rgb
}

/// 불투명 그림을 JPEG(품질 100)로 인코딩한다. TS 워커도 Android PNG 디코더 문제를 피하려고 JPEG를 쓴다
pub fn encode_jpeg(pixmap: &Pixmap) -> Result<Vec<u8>, image::ImageError> {
    let rgb = to_rgb_image(pixmap);
    let mut bytes = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 100);
    rgb.write_with_encoder(encoder)?;
    Ok(bytes)
}

pub fn encode_png(pixmap: &Pixmap) -> Result<Vec<u8>, image::ImageError> {
    let rgb = to_rgb_image(pixmap);
    let mut bytes = Vec::new();
    rgb.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )?;
    Ok(bytes)
}
