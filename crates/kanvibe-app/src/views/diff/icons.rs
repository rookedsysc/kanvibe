//! diff 화면 인라인 SVG(TS `DiffRoute`·`DiffPageClient`·`DiffFileTree`의 `<svg>` 경로 그대로). GPUI svg 요소는 부모 글자색을
//! 물려받지 않고 단색 마스크만 그리므로, 색과 불투명도를 넣은 SVG 문서를 이미지로 그린다(채움 0.15와 선을 겹친 폴더 아이콘 때문).

use std::sync::Arc;

use gpui_kit::{Image, ImageFormat, Img, Rgba, Styled, img, px};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffIcon {
    /// 헤더 "작업으로 돌아가기" 화살표(16)
    Back,
    /// 헤더 base → branch 사이 화살표(14)
    BranchArrow,
    /// 폴더 펼침 화살표(12). 펼친 폴더는 90도 돌린 모양
    FolderChevron {
        is_expanded: bool,
    },
    Folder {
        is_open: bool,
    },
    File,
    /// 확인한 파일 체크(16)
    Viewed,
    /// 파일이 없을 때 가운데 문서 아이콘(48)
    EmptyDocument,
}

impl DiffIcon {
    fn size(self) -> f32 {
        match self {
            Self::BranchArrow => 14.0,
            Self::FolderChevron { .. } => 12.0,
            Self::EmptyDocument => 48.0,
            _ => 16.0,
        }
    }

    fn body(self, color: &str) -> String {
        let stroke = |path: &str, width: &str| {
            format!(
                r#"<path d="{path}" stroke="{color}" stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#
            )
        };
        match self {
            Self::Back => stroke("M10 12L6 8L10 4", "1.5"),
            Self::BranchArrow => stroke("M5 3l4 4-4 4", "1.2"),
            Self::FolderChevron { is_expanded: false } => stroke("M4.5 2.5L8 6L4.5 9.5", "1.2"),
            Self::FolderChevron { is_expanded: true } => stroke("M2.5 4.5L6 8L9.5 4.5", "1.2"),
            Self::Folder { is_open } => {
                // TS `opacity` 속성은 채움과 선을 함께 흐리게 한다
                let opacity = if is_open { "0.2" } else { "0.15" };
                let path = "M1.5 3.5C1.5 2.67 2.17 2 3 2h3.38a1.5 1.5 0 011.12.5L8.5 3.5H13c.83 0 1.5.67 1.5 1.5v7c0 .83-.67 1.5-1.5 1.5H3c-.83 0-1.5-.67-1.5-1.5v-8.5z";
                format!(
                    r#"<path d="{path}" fill="{color}" opacity="{opacity}" stroke="{color}" stroke-width="1"/>"#
                )
            }
            Self::File => format!(
                r#"<path d="M4 1.5h5.5L13 5v8.5a1 1 0 01-1 1H4a1 1 0 01-1-1v-12a1 1 0 011-1z" stroke="{color}" stroke-width="1" fill="none"/><path d="M9.5 1.5V5H13" stroke="{color}" stroke-width="1" fill="none"/>"#
            ),
            Self::Viewed => stroke("M3 8l3.5 3.5L13 5", "1.5"),
            Self::EmptyDocument => format!(
                r#"<path d="M12 8h16l8 8v24a4 4 0 01-4 4H12a4 4 0 01-4-4V12a4 4 0 014-4z" stroke="{color}" stroke-width="2" fill="none"/><path d="M28 8v8h8" stroke="{color}" stroke-width="2" fill="none"/><path d="M16 24h16M16 30h12" stroke="{color}" stroke-width="2" stroke-linecap="round"/>"#
            ),
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

/// 색과 불투명도(`opacity-30` 같은 Tailwind 접미사 포함)를 넣은 아이콘 이미지
pub fn diff_icon(icon: DiffIcon, color: Rgba) -> Img {
    let icon_size = icon.size();
    let markup = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{icon_size}" height="{icon_size}" viewBox="0 0 {icon_size} {icon_size}" fill="none" opacity="{opacity}">{body}</svg>"#,
        opacity = color.a,
        body = icon.body(&css_hex(color)),
    );
    let image = Arc::new(Image::from_bytes(ImageFormat::Svg, markup.into_bytes()));
    img(image).size(px(icon_size)).flex_none()
}
