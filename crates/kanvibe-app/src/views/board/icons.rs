//! 보드 화면 전용 인라인 SVG 아이콘. TS 컴포넌트의 인라인 `<svg>`를 그대로 옮기고, `currentColor` 자리에 그릴 색을 넣어
//! 이미지로 그린다. 같은 아이콘·색 조합은 한 번만 만든다.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::{Image, ImageFormat, Img, Rgba, Styled, img, px};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoardIcon {
    /// `TaskCardContent` CrownIcon(기본 브랜치 표시)
    Crown,
    /// `TaskCardContent` BellIcon(미읽음 배지)
    Bell,
    /// `TaskCardContent` PullRequestIcon
    PullRequest,
    /// `TaskDiffStats` FileDiffIcon
    FileDiff,
    /// `DesktopFilter` 체크 표시
    Check,
    /// 선택 상자 펼침 표시, 닫기
    ChevronDown,
    Close,
    /// `BoardSortPicker` 방향 표시
    ArrowUp,
    ArrowDown,
}

impl BoardIcon {
    fn markup(self) -> &'static str {
        match self {
            Self::Crown => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M2.2 5.1 5.3 8l2.7-4.1L10.7 8l3.1-2.9-1.2 6.6H3.4L2.2 5.1Z" fill="currentColor"/><path d="M3.7 13h8.6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>"#
            }
            Self::Bell => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 17h5l-1.4-1.4A2 2 0 0 1 18 14.2V11a6 6 0 1 0-12 0v3.2a2 2 0 0 1-.6 1.4L4 17h5"/><path d="M9 17a3 3 0 0 0 6 0"/></svg>"#
            }
            Self::PullRequest => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="18" r="3"/><circle cx="18" cy="6" r="3"/><path d="M6 15V6"/><path d="M18 9v1.5A5.5 5.5 0 0 1 12.5 16H9"/><path d="m12 13-3 3 3 3"/></svg>"#
            }
            Self::FileDiff => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5v4c0 2 1.5 3.5 3.5 3.5H14"/><path d="M15 13l3 3-3 3"/></svg>"#
            }
            Self::Check => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="8" viewBox="0 0 10 8" fill="none"><path d="M1 4L3.5 6.5L9 1" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"#
            }
            Self::ChevronDown => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>"#
            }
            Self::Close => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>"#
            }
            Self::ArrowUp => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5"/><path d="m5 12 7-7 7 7"/></svg>"#
            }
            Self::ArrowDown => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14"/><path d="m19 12-7 7-7-7"/></svg>"#
            }
        }
    }
}

fn css_hex(color: Rgba) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(color.r),
        channel(color.g),
        channel(color.b)
    )
}

thread_local! {
    static ICON_IMAGES: RefCell<HashMap<(BoardIcon, String, u8), Arc<Image>>> = RefCell::new(HashMap::new());
}

fn icon_image(icon: BoardIcon, color: Rgba) -> Arc<Image> {
    let hex = css_hex(color);
    let alpha = (color.a.clamp(0.0, 1.0) * 255.0).round() as u8;
    ICON_IMAGES.with(|images| {
        images
            .borrow_mut()
            .entry((icon, hex.clone(), alpha))
            .or_insert_with(|| {
                let markup = icon.markup().replace("currentColor", &hex).replacen(
                    "<svg ",
                    &format!("<svg opacity=\"{}\" ", color.a),
                    1,
                );
                Arc::new(Image::from_bytes(ImageFormat::Svg, markup.into_bytes()))
            })
            .clone()
    })
}

/// `size` 정사각형 칸에 아이콘을 그린다
pub fn board_icon(icon: BoardIcon, color: impl Into<Rgba>, size: f32) -> Img {
    img(icon_image(icon, color.into()))
        .size(px(size))
        .flex_none()
}

/// 가로·세로가 다른 아이콘(체크 표시 10x8)
pub fn board_icon_sized(icon: BoardIcon, color: impl Into<Rgba>, width: f32, height: f32) -> Img {
    img(icon_image(icon, color.into()))
        .w(px(width))
        .h(px(height))
        .flex_none()
}
