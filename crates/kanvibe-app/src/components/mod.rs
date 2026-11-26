//! 여러 화면이 함께 쓰는 UI 기본 요소. TS `src/components`·`src/desktop/renderer/components`에서 한 화면에 묶이지 않은
//! Tailwind 클래스 조합만 옮겼다. 색·그림자는 모두 `ThemeTokens`에서 읽고, 크기는 Tailwind 4px 간격과 `text_scale`을 쓴다.
//!
//! TS에는 공용 UI 라이브러리가 없어서 같은 조합이 화면마다 반복돼 있었다. 각 요소의 문서 주석에 기준으로 삼은 TS 클래스 문자열을 적는다.
//! 상태(열림·선택·강조)는 화면이 갖고, 여기 요소는 받은 상태대로 그리기만 한다.

pub mod badge;
pub mod button;
pub mod context_menu;
pub mod create_task;
pub mod dialog;
pub mod done_confirm;
pub mod icon;
pub mod notification_center;
pub mod popover;
pub mod scroll_area;
pub mod segmented_control;
pub(crate) mod session_dependency_prompt;
pub mod spinner;
pub mod terminal_tabs;
pub mod terminal_view;
pub mod text_context_menu;
pub mod toast;
pub mod tooltip;

use gpui_kit::{BoxShadow, Rgba, Styled, point, px};

use crate::fonts::{mono_font, sans_font};
use crate::theme::{ShadowLayer, TextScale, ThemeTokens};

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

    /// `font-mono`
    fn mono(self, css_weight: u16) -> Self {
        self.font(mono_font(css_weight))
    }
}

impl<E: Styled> TailwindText for E {}

/// CSS `box-shadow` 겹을 GPUI 그림자로 옮긴다
pub fn box_shadows(layers: &[ShadowLayer]) -> Vec<BoxShadow> {
    layers
        .iter()
        .map(|layer| BoxShadow {
            color: layer.color.into(),
            offset: point(px(0.0), px(layer.offset_y)),
            blur_radius: px(layer.blur),
            spread_radius: px(layer.spread),
            inset: false,
        })
        .collect()
}

/// Tailwind 불투명도 접미사(`bg-brand-primary/10`). 토큰이 이미 알파를 가지면 곱한다
pub fn fade(color: Rgba, opacity: f32) -> Rgba {
    Rgba {
        a: color.a * opacity,
        ..color
    }
}

/// `focus-visible:outline-2 focus-visible:outline-brand-primary`. GPUI에는 outline이 없어 테두리 바깥 2px 그림자로 그린다
pub fn focus_ring(theme: &ThemeTokens) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: theme.brand.primary.into(),
        offset: point(px(0.0), px(0.0)),
        blur_radius: px(0.0),
        spread_radius: px(2.0),
        inset: false,
    }]
}

/// `disabled:opacity-50`
pub const DISABLED_OPACITY: f32 = 0.5;

/// Chromium 기본 포커스 링(`:focus-visible`): 2px 링과 바깥 1px 테두리. 다크 화면은 흰 링, 라이트 화면은 주황 링이다.
/// 링이 없을 때는 패널 기본 테두리(`border border-border-default shadow-lg`)를 그린다
pub trait FocusRing: Styled + Sized {
    fn when_focus_ring(self, is_visible: bool, theme: &ThemeTokens) -> Self {
        if !is_visible {
            return self
                .border_1()
                .border_color(theme.border.default)
                .shadow(box_shadows(&theme.shadow.lg));
        }
        let (ring, halo) = match theme.mode {
            crate::theme::ThemeMode::Dark => (gpui_kit::rgb(0xffffff), gpui_kit::rgb(0x101010)),
            crate::theme::ThemeMode::Light => (gpui_kit::rgb(0xe59700), gpui_kit::rgb(0xffffff)),
        };
        let mut shadows = vec![BoxShadow {
            color: halo.into(),
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(0.0),
            spread_radius: px(1.0),
            inset: false,
        }];
        shadows.extend(box_shadows(&theme.shadow.lg));
        self.border_2().border_color(ring).shadow(shadows)
    }
}

impl<E: Styled> FocusRing for E {}
