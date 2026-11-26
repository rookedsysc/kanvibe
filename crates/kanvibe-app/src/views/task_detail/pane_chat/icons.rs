//! pane 채팅 전용 인라인 SVG(모델 팝오버의 체크 표시). 작업 상세 아이콘과 같은 방식으로 색을 넣은 문서를 이미지로 그린다.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::{AnyElement, Image, ImageFormat, IntoElement, Pixels, Rgba, Styled, img};

/// 색(RGBA 비트)과 크기(비트)
type CheckMarkKey = ([u32; 4], u32);

thread_local! {
    static CHECK_MARKS: RefCell<HashMap<CheckMarkKey, Arc<Image>>> = RefCell::new(HashMap::new());
}

fn color_hex(color: Rgba) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(color.r),
        channel(color.g),
        channel(color.b)
    )
}

/// TS 모델 선택지의 `<path d="m3 8 3 3 7-7" stroke-width="1.75"/>`
pub fn check_mark(color: Rgba, size: Pixels) -> AnyElement {
    let size_value = f32::from(size);
    let key = (
        [color.r, color.g, color.b, color.a].map(f32::to_bits),
        size_value.to_bits(),
    );
    let image = CHECK_MARKS.with(|marks| {
        marks
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                let document = format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size_value}" height="{size_value}" viewBox="0 0 16 16" fill="none"><path d="m3 8 3 3 7-7" stroke="{hex}" stroke-opacity="{opacity}" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/></svg>"#,
                    hex = color_hex(color),
                    opacity = color.a,
                );
                Arc::new(Image::from_bytes(ImageFormat::Svg, document.into_bytes()))
            })
            .clone()
    });
    img(image).size(size).flex_none().into_any_element()
}
