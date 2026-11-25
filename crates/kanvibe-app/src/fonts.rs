//! 기준 이미지와 같은 글꼴 파일을 번들해 등록하고, CSS 글꼴 지정에 해당하는 GPUI `Font`를 만든다.
//!
//! 파일은 `parity/screens/fonts.json`이 고정한 바이트 그대로다(라이선스는 `assets/fonts/LICENSE-*.txt`).
//! 기준 이미지에서 쓰이지 않은 Italic·CJK Bold·Noto Color Emoji는 넣지 않았다(`parity/screens/fonts.md` 조사).
//!
//! 앱 CSS가 요청한 `Inter`·`JetBrains Mono`는 기준 환경에 없어서 Chromium이 일반 이름(`sans-serif`, `monospace`)까지
//! 내려갔다. 여기서는 그 결과 family와 글자별 대체 순서를 바로 지정한다.

use std::borrow::Cow;

use gpui_kit::{App, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, SharedString};

const BUNDLED_FONT_FILES: [&[u8]; 8] = [
    include_bytes!("../assets/fonts/NotoSans-Regular.ttf"),
    include_bytes!("../assets/fonts/NotoSans-Bold.ttf"),
    include_bytes!("../assets/fonts/NotoSansCJK-Regular.ttc"),
    include_bytes!("../assets/fonts/NotoSansSymbols-Regular.ttf"),
    include_bytes!("../assets/fonts/NotoSansSymbols2-Regular.ttf"),
    include_bytes!("../assets/fonts/DejaVuSans.ttf"),
    include_bytes!("../assets/fonts/DejaVuSansMono.ttf"),
    include_bytes!("../assets/fonts/DejaVuSansMono-Bold.ttf"),
];

/// `--font-sans`가 기준 환경에서 풀린 family
pub const SANS_FAMILY: &str = "Noto Sans";
/// `--font-mono`·터미널·Monaco가 기준 환경에서 풀린 family
pub const MONO_FAMILY: &str = "DejaVu Sans Mono";

/// fontconfig `sans-serif` 별칭의 나머지 순서. 한글이 CJK JP 얼굴로 그려지지 않게 KR을 먼저 둔다.
const SANS_FALLBACKS: [&str; 4] = [
    "Noto Sans CJK KR",
    "Noto Sans Symbols",
    "Noto Sans Symbols2",
    "DejaVu Sans",
];
/// fontconfig `monospace` 별칭의 나머지 순서. Mono CJK는 기준 환경에도 없어 Sans CJK KR로 떨어진다.
const MONO_FALLBACKS: [&str; 3] = [
    "Noto Sans CJK KR",
    "Noto Sans Symbols",
    "Noto Sans Symbols2",
];

/// 번들 글꼴을 앱 텍스트 시스템에 올린다. 시스템 글꼴보다 먼저 이름이 맞는 번들 얼굴을 쓰게 하려면 창을 열기 전에 불러야 한다.
pub fn register_bundled_fonts(cx: &App) -> anyhow::Result<()> {
    let font_files = BUNDLED_FONT_FILES
        .iter()
        .map(|bytes| Cow::Borrowed(*bytes))
        .collect();
    cx.text_system().add_fonts(font_files)
}

/// CSS `font-weight`를 기준 환경이 실제로 고른 얼굴로 옮긴다.
/// 굵기 파일이 Regular(400)·Bold(700)뿐이라 Chromium은 500을 Regular로, 600 이상을 Bold로 그렸다.
pub fn resolve_css_weight(css_weight: u16) -> FontWeight {
    if css_weight >= 600 {
        FontWeight::BOLD
    } else {
        FontWeight::NORMAL
    }
}

/// 본문·UI 글꼴. `css_weight`는 Tailwind `font-*` 값(400·500·600·700) 그대로다.
pub fn sans_font(css_weight: u16) -> Font {
    font_with_fallbacks(SANS_FAMILY, &SANS_FALLBACKS, css_weight)
}

/// 코드·브랜치·단축키 글꼴
pub fn mono_font(css_weight: u16) -> Font {
    font_with_fallbacks(MONO_FAMILY, &MONO_FALLBACKS, css_weight)
}

fn font_with_fallbacks(family: &str, fallbacks: &[&str], css_weight: u16) -> Font {
    Font {
        family: SharedString::from(family.to_string()),
        features: FontFeatures::default(),
        fallbacks: Some(FontFallbacks::from_fonts(
            fallbacks.iter().map(|name| name.to_string()).collect(),
        )),
        weight: resolve_css_weight(css_weight),
        style: FontStyle::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn medium_weight_uses_the_regular_face_like_chromium() {
        assert_eq!(resolve_css_weight(400), FontWeight::NORMAL);
        assert_eq!(resolve_css_weight(500), FontWeight::NORMAL);
        assert_eq!(resolve_css_weight(600), FontWeight::BOLD);
        assert_eq!(resolve_css_weight(700), FontWeight::BOLD);
    }

    #[test]
    fn korean_falls_back_to_the_kr_face_before_symbols() {
        let font = sans_font(400);
        let fallbacks = font.fallbacks.expect("sans font must carry fallbacks");

        assert_eq!(font.family.as_ref(), SANS_FAMILY);
        assert_eq!(fallbacks.fallback_list()[0], "Noto Sans CJK KR");
    }
}
