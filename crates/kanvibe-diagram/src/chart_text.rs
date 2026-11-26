//! 차트형 다이어그램(xychart·radar·venn)이 `<text>` 하나를 놓는 방식.
//!
//! 세 다이어그램은 글자를 `text-anchor`(가로)와 `dominant-baseline`(세로)로 앵커 점에 맞추고, 필요하면 그 점을
//! 중심으로 돌린다. HTML 라벨을 쓰지 않으므로 라벨 방식과 상관없이 같은 장면이 나온다.

use crate::scene::{Color, Point, Rotation, Scene, TextItem};
use crate::text::{FontBook, FontWeight};

/// Noto Sans ascender(1em 기준)
const ASCENT_EM: f64 = 1.069;
/// Blink가 `dominant-baseline: hanging`에 쓰는 기준선 높이(ascent의 80%)
const HANGING_ASCENT_RATIO: f64 = 0.8;
/// Noto Sans x-height(1em 기준). `middle`은 x-height의 절반만큼 내린다
const X_HEIGHT_EM: f64 = 0.536;

/// `dominant-baseline`
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BaselineAnchor {
    /// `auto`: 알파벳 기준선
    Alphabetic,
    Hanging,
    Middle,
    /// `text-before-edge`: 글꼴 ascent 위 끝
    TextBeforeEdge,
    /// `dy` 만큼 글자 크기 비율로 내린다(`dy=".35em"`)
    Shifted(f64),
}

/// `text-anchor`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAnchor {
    Start,
    Middle,
    End,
}

/// 앵커 점 (x, y)에 놓을 글자 한 줄
#[derive(Clone, Debug)]
pub struct AnchoredText<'a> {
    pub text: &'a str,
    pub x: f64,
    pub y: f64,
    pub font_size: f64,
    pub color: Color,
    pub anchor: TextAnchor,
    pub baseline: BaselineAnchor,
    /// 앵커 점을 중심으로 시계 방향 각도. 0이면 돌리지 않는다
    pub degrees: f64,
}

impl BaselineAnchor {
    /// 앵커 y에서 알파벳 기준선까지 내리는 거리
    fn shift(self, font_size: f64) -> f64 {
        match self {
            BaselineAnchor::Alphabetic => 0.0,
            BaselineAnchor::Hanging => ASCENT_EM * font_size * HANGING_ASCENT_RATIO,
            BaselineAnchor::Middle => X_HEIGHT_EM * font_size / 2.0,
            BaselineAnchor::TextBeforeEdge => ASCENT_EM * font_size,
            BaselineAnchor::Shifted(ratio) => ratio * font_size,
        }
    }
}

/// 글자를 장면에 넣는다. 빈 글자는 건너뛴다
pub fn push_anchored_text(scene: &mut Scene, fonts: &FontBook, text: &AnchoredText) {
    if text.text.is_empty() {
        return;
    }
    let width = fonts
        .shape_line(text.text, text.font_size, FontWeight::Regular)
        .width;
    let left = match text.anchor {
        TextAnchor::Start => text.x,
        TextAnchor::Middle => text.x - width / 2.0,
        TextAnchor::End => text.x - width,
    };
    let rotation = (text.degrees != 0.0).then_some(Rotation {
        degrees: text.degrees,
        center: Point::new(text.x, text.y),
    });
    scene.push_text(TextItem {
        text: text.text.to_string(),
        x: left,
        baseline: text.y + text.baseline.shift(text.font_size),
        font_size: text.font_size,
        weight: FontWeight::Regular,
        color: text.color,
        rotation,
    });
}
