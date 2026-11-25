//! 글자 기준선을 Chromium과 같은 픽셀 행에 맞춘다. diff 화면에서 시작해 모든 화면이 함께 쓰도록 옮겼다.
//! GPUI는 줄 상자 안 기준선을 `(줄 높이 - 글꼴 높이) / 2 + ascent`로 소수점까지 두고 반올림하며, 한 줄에 한글이 섞이면
//! 더 큰 CJK 글꼴 지표로 잰다. Chromium은 첫 글꼴(Noto Sans)의 ascent·descent를 각각 반올림한 뒤 반 행간을 내림한다.
//! 두 계산이 다른 크기에서는 글자가 1px 아래에 찍히므로, 글자만 그 차이만큼 올린다(배경·테두리는 그대로).
//!
//! 폭도 다르다. GPUI 글자 요소는 폭을 올림한 정수로 잡고 Chromium은 소수 폭을 그대로 쓴다. 글자 뒤에 오는 요소와 오른쪽 정렬
//! 글자가 1px씩 밀리므로, 자르지 않는 짧은 글자는 소수 폭으로 자리를 잡는 `ChromiumText`로 그린다.

use gpui_kit::{
    App, Bounds, Div, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId,
    ParentElement, Pixels, SharedString, Style, Styled, TextAlign, Window, div, point, px, size,
};

use crate::fonts::MONO_FAMILY;
use crate::theme::TextScale;

/// 글꼴 지표(em 비율)
#[derive(Clone, Copy)]
struct VerticalMetrics {
    ascent: f32,
    descent: f32,
}

const NOTO_SANS: VerticalMetrics = VerticalMetrics {
    ascent: 1.069,
    descent: 0.293,
};
const NOTO_SANS_CJK: VerticalMetrics = VerticalMetrics {
    ascent: 1.16,
    descent: 0.288,
};
const DEJAVU_SANS_MONO: VerticalMetrics = VerticalMetrics {
    ascent: 1901.0 / 2048.0,
    descent: 483.0 / 2048.0,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFamily {
    Sans,
    Mono,
}

fn is_cjk(character: char) -> bool {
    matches!(character as u32, 0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF | 0x4E00..=0x9FFF)
}

fn gpui_baseline(scale: TextScale, metrics: VerticalMetrics) -> f32 {
    let (font_size, line_height) = (f32::from(scale.font_size), f32::from(scale.line_height));
    ((line_height - (metrics.ascent + metrics.descent) * font_size) / 2.0
        + metrics.ascent * font_size)
        .round()
}

fn chromium_baseline(scale: TextScale, metrics: VerticalMetrics) -> f32 {
    let (font_size, line_height) = (f32::from(scale.font_size), f32::from(scale.line_height));
    let ascent = (metrics.ascent * font_size).round();
    let descent = (metrics.descent * font_size).round();
    ascent + ((line_height - ascent - descent) / 2.0).floor()
}

/// GPUI가 Chromium보다 기준선을 몇 px 아래에 두는지
pub fn baseline_lift(text: &str, scale: TextScale, family: TextFamily) -> f32 {
    let primary = match family {
        TextFamily::Sans => NOTO_SANS,
        TextFamily::Mono => DEJAVU_SANS_MONO,
    };
    let line_metrics = if text.chars().any(is_cjk) {
        VerticalMetrics {
            ascent: primary.ascent.max(NOTO_SANS_CJK.ascent),
            descent: primary.descent.max(NOTO_SANS_CJK.descent),
        }
    } else {
        primary
    };
    gpui_baseline(scale, line_metrics) - chromium_baseline(scale, primary)
}

/// 글자 한 줄. 부모의 글자 모양을 물려받고 기준선만 Chromium 행으로 옮긴다
pub fn aligned_text(text: impl Into<SharedString>, scale: TextScale, family: TextFamily) -> Div {
    let text = text.into();
    let lift = baseline_lift(&text, scale, family);
    div().relative().top(px(-lift)).child(text)
}

/// Chromium처럼 소수 폭으로 자리를 잡고 기준선을 맞춘 한 줄 글자. 글자 모양(글꼴·크기·색·줄 높이)은 부모에서 물려받는다
pub struct ChromiumText {
    text: SharedString,
}

pub fn chromium_text(text: impl Into<SharedString>) -> ChromiumText {
    ChromiumText { text: text.into() }
}

impl Element for ChromiumText {
    type RequestLayoutState = gpui_kit::ShapedLine;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = text_style.line_height_in_pixels(window.rem_size());
        let run = text_style.to_run(self.text.len());
        let shaped = window
            .text_system()
            .shape_line(self.text.clone(), font_size, &[run], None);
        let style = Style {
            size: size(shaped.width.into(), line_height.into()),
            flex_shrink: 0.0,
            ..Style::default()
        };
        (window.request_layout(style, [], cx), shaped)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        shaped: &mut Self::RequestLayoutState,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let text_style = window.text_style();
        let family = if text_style.font_family.as_ref() == MONO_FAMILY {
            TextFamily::Mono
        } else {
            TextFamily::Sans
        };
        let scale = TextScale {
            font_size: text_style.font_size.to_pixels(window.rem_size()),
            line_height: bounds.size.height,
        };
        let lift = baseline_lift(&self.text, scale, family);
        let origin = point(bounds.origin.x, bounds.origin.y - px(lift));
        let _ = shaped.paint(
            origin,
            bounds.size.height,
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::text_scale;

    #[test]
    fn lifts_only_the_sizes_measured_one_pixel_low_against_the_baseline_images() {
        assert_eq!(
            baseline_lift("board.ts", text_scale::XS, TextFamily::Sans),
            1.0
        );
        assert_eq!(
            baseline_lift("작업으로 돌아가기", text_scale::SM, TextFamily::Sans),
            1.0
        );
        assert_eq!(
            baseline_lift("Loading...", text_scale::BODY, TextFamily::Sans),
            0.0
        );
        assert_eq!(
            baseline_lift("+14", text_scale::PX_10, TextFamily::Sans),
            0.0
        );
        assert_eq!(baseline_lift("main", text_scale::XS, TextFamily::Mono), 0.0);
    }
}
