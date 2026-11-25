//! 화면 요소. Tailwind 클래스 하나하나를 GPUI 스타일로 옮길 때 쓰는 작은 도우미도 여기 둔다.

pub mod board;
pub mod not_found;
pub mod page_scrollbar;
pub mod window_controls;

use gpui_kit::Styled;

use crate::fonts::sans_font;
use crate::theme::TextScale;

/// Tailwind `text-*`와 `font-*`를 함께 거는 도우미
pub trait TailwindText: Styled + Sized {
    /// `text-xs`처럼 글자 크기와 줄 높이를 함께 정한다
    fn text_scale(self, scale: TextScale) -> Self {
        self.text_size(scale.font_size)
            .line_height(scale.line_height)
    }

    /// `font-medium`(500)·`font-semibold`(600) 같은 CSS 굵기
    fn css_weight(self, css_weight: u16) -> Self {
        self.font(sans_font(css_weight))
    }
}

impl<E: Styled> TailwindText for E {}
