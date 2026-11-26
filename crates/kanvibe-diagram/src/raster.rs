//! 장면을 tiny-skia로 그린다. 모바일 이미지는 diagram-worker.html과 같은 배치(좌우 8px 여백, 가운데 정렬,
//! 폭 `width - 16`과 높이 2384px 중 작은 배율, 위아래 8px)로 그리고 PNG·JPEG로 인코딩한다.

use tiny_skia::{
    BlendMode, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Mask, Paint, PathBuilder,
    Pixmap, PixmapPaint, Rect, SpreadMode, Stroke, StrokeDash, Transform,
};

use crate::hinting::VerticalHinting;
use crate::scene::{
    ClipItem, Color, MultiplyStrokeItem, PathCommand, PathItem, Rotation, Scene, SceneItem,
    TextItem, TextStrokeItem,
};
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
    // 워커는 viewBox.baseVal(float32)로 배율과 내림을 계산하므로 784/w·w가 783.999…(783)로 떨어지기도 한다
    let natural_width = f64::from(scene.width as f32);
    let natural_height = f64::from(scene.height as f32);
    let scale = (content_width / natural_width).min(MAX_DIAGRAM_HEIGHT / natural_height);
    let actual_width = (natural_width * scale).floor().max(1.0);
    let actual_height = (natural_height * scale).floor().max(1.0);
    let fit = (actual_width / scene.width).min(actual_height / scene.height);
    // Chromium은 margin:auto로 가운데 둔 SVG 상자를 장치 픽셀에 맞춘다(남는 폭이 홀수면 0.5px 대신 1px)
    let left = (PAGE_PADDING + (content_width - actual_width) / 2.0).round();
    // preserveAspectRatio: 보통은 xMidYMid meet로 가운데, journey처럼 xMinYMin이면 왼쪽 위에 둔다
    let (offset_x, offset_y) = if scene.is_start_aligned {
        (left, PAGE_PADDING)
    } else {
        (
            left + (actual_width - scene.width * fit) / 2.0,
            PAGE_PADDING + (actual_height - scene.height * fit) / 2.0,
        )
    };
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

/// 그라디언트 선을 곱하기 섞기로 그린다. 그라디언트 좌표는 경로와 같은 장면 좌표다
fn draw_multiply_stroke(pixmap: &mut Pixmap, item: &MultiplyStrokeItem, transform: Transform) {
    let Some(path) = build_path(&item.commands) else {
        return;
    };
    let stop = |offset: f32, color: Color| {
        GradientStop::new(
            offset,
            tiny_skia::Color::from_rgba8(
                color.red,
                color.green,
                color.blue,
                (color.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            ),
        )
    };
    let Some(shader) = LinearGradient::new(
        tiny_skia::Point::from_xy(item.start_x as f32, 0.0),
        tiny_skia::Point::from_xy(item.end_x as f32, 0.0),
        vec![stop(0.0, item.start_color), stop(1.0, item.end_color)],
        SpreadMode::Pad,
        Transform::identity(),
    ) else {
        return;
    };
    let paint = Paint {
        shader,
        blend_mode: BlendMode::Multiply,
        anti_alias: true,
        ..Paint::default()
    };
    let stroke = Stroke {
        width: item.width as f32,
        line_cap: LineCap::Butt,
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, transform, None);
}

/// 한글처럼 대체 얼굴로 넘어간 글리프는 FreeType CJK autofit이 라틴 청색 영역을 쓰지 않으므로 기준선만
/// 장치 픽셀에 맞춘다. 가로 글리프 위치는 부분 픽셀 그대로 둔다(맞추면 기준 이미지와 더 멀어진다)
fn fallback_glyph_transform(item: &TextItem, glyph_x: f64, placement: Placement) -> Transform {
    let baseline = (item.baseline * placement.scale + placement.offset_y).round();
    let origin_x = (item.x + glyph_x) * placement.scale + placement.offset_x;
    let scale = placement.scale as f32;
    Transform::from_row(scale, 0.0, 0.0, scale, origin_x as f32, baseline as f32)
}

/// 라틴 글리프는 장치 크기로 키운 윤곽선에 autofit 세로 사상을 입힌 뒤 기준선(장치 픽셀)에 둔다
fn draw_hinted_glyph(
    pixmap: &mut Pixmap,
    outline: &tiny_skia::Path,
    paint: &Paint,
    item: &TextItem,
    glyph_x: f64,
    placement: Placement,
) {
    let scale = placement.scale as f32;
    let Some(device) = outline
        .clone()
        .transform(Transform::from_scale(scale, scale))
    else {
        return;
    };
    let hinting = VerticalHinting::for_size(item.font_size * placement.scale);
    let Some(hinted) = hinting.apply(&device) else {
        return;
    };
    let baseline = (item.baseline * placement.scale + placement.offset_y).round();
    let origin_x = (item.x + glyph_x) * placement.scale + placement.offset_x;
    let origin = Transform::from_translate(origin_x as f32, baseline as f32);
    pixmap.fill_path(&hinted, paint, FillRule::Winding, origin, None);
}

/// 돌린 글자는 힌팅 없이 장면 좌표에서 돌린 뒤 배치한다
fn rotated_glyph_transform(
    item: &TextItem,
    glyph_x: f64,
    rotation: Rotation,
    placement: Placement,
) -> Transform {
    let glyph_origin = Transform::from_translate((item.x + glyph_x) as f32, item.baseline as f32);
    let turn = Transform::from_rotate_at(
        rotation.degrees as f32,
        rotation.center.x as f32,
        rotation.center.y as f32,
    );
    glyph_origin
        .post_concat(turn)
        .post_concat(placement.transform())
}

/// 글리프 윤곽선을 힌팅 없이 장면 좌표에서 긋는다
fn draw_text_stroke(
    pixmap: &mut Pixmap,
    fonts: &FontBook,
    item: &TextStrokeItem,
    placement: Placement,
) {
    let text = &item.text;
    let line = fonts.shape_line(&text.text, text.font_size, text.weight);
    let paint = paint_for(item.color);
    let stroke = Stroke {
        width: item.width as f32,
        line_join: if item.is_round_join {
            LineJoin::Round
        } else {
            LineJoin::Miter
        },
        miter_limit: 4.0,
        ..Stroke::default()
    };
    for glyph in &line.glyphs {
        let Some(outline) = fonts.glyph_outline(glyph, text.font_size) else {
            continue;
        };
        let transform = match text.rotation {
            Some(rotation) => rotated_glyph_transform(text, glyph.x, rotation, placement),
            None => Transform::from_translate((text.x + glyph.x) as f32, text.baseline as f32)
                .post_concat(placement.transform()),
        };
        pixmap.stroke_path(&outline, &paint, &stroke, transform, None);
    }
}

fn draw_text(pixmap: &mut Pixmap, fonts: &FontBook, item: &TextItem, placement: Placement) {
    let line = fonts.shape_line(&item.text, item.font_size, item.weight);
    let paint = paint_for(item.color);
    for glyph in &line.glyphs {
        let Some(outline) = fonts.glyph_outline(glyph, item.font_size) else {
            continue;
        };
        let transform = match item.rotation {
            Some(rotation) => rotated_glyph_transform(item, glyph.x, rotation, placement),
            None if fonts.is_primary_face(glyph.face_index) => {
                draw_hinted_glyph(pixmap, &outline, &paint, item, glyph.x, placement);
                continue;
            }
            None => fallback_glyph_transform(item, glyph.x, placement),
        };
        pixmap.fill_path(&outline, &paint, FillRule::Winding, transform, None);
    }
}

/// 장면을 배치대로 이미 배경을 칠한 픽스맵 위에 그린다
pub fn draw_scene(pixmap: &mut Pixmap, scene: &Scene, fonts: &FontBook, placement: Placement) {
    draw_items(pixmap, &scene.items, fonts, placement);
}

fn draw_items(pixmap: &mut Pixmap, items: &[SceneItem], fonts: &FontBook, placement: Placement) {
    let transform = placement.transform();
    for item in items {
        match item {
            SceneItem::Path(path) => draw_path(pixmap, path, transform),
            SceneItem::Text(text) => draw_text(pixmap, fonts, text, placement),
            SceneItem::Clip(clip) => draw_clipped(pixmap, clip, fonts, placement),
            SceneItem::MultiplyStroke(stroke) => draw_multiply_stroke(pixmap, stroke, transform),
            SceneItem::TextStroke(stroke) => draw_text_stroke(pixmap, fonts, stroke, placement),
        }
    }
}

/// 잘라 낼 사각형을 덮는 장치 픽셀 영역에 따로 그린 뒤, 사각형(가장자리는 부분 덮임) 밖을 지우고 합친다
fn draw_clipped(pixmap: &mut Pixmap, clip: &ClipItem, fonts: &FontBook, placement: Placement) {
    let device_x = clip.x * placement.scale + placement.offset_x;
    let device_y = clip.y * placement.scale + placement.offset_y;
    let (left, top) = (device_x.floor(), device_y.floor());
    let right = (device_x + clip.width * placement.scale).ceil();
    let bottom = (device_y + clip.height * placement.scale).ceil();
    let (Some(mut layer), Some(mut mask)) = (
        Pixmap::new((right - left) as u32, (bottom - top) as u32),
        Mask::new((right - left) as u32, (bottom - top) as u32),
    ) else {
        return;
    };
    let local = Placement {
        offset_x: placement.offset_x - left,
        offset_y: placement.offset_y - top,
        ..placement
    };
    draw_items(&mut layer, &clip.items, fonts, local);
    let Some(rect) = Rect::from_xywh(
        (device_x - left) as f32,
        (device_y - top) as f32,
        (clip.width * placement.scale) as f32,
        (clip.height * placement.scale) as f32,
    ) else {
        return;
    };
    let mut builder = PathBuilder::new();
    builder.push_rect(rect);
    if let Some(path) = builder.finish() {
        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    }
    layer.apply_mask(&mask);
    let paint = PixmapPaint::default();
    pixmap.draw_pixmap(
        left as i32,
        top as i32,
        layer.as_ref(),
        &paint,
        Transform::identity(),
        None,
    );
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
