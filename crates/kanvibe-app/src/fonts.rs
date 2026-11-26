//! 기준 이미지와 같은 글꼴을 번들해 등록하고, CSS 글꼴 지정에 해당하는 GPUI `Font`를 만든다.
//!
//! 파일은 `parity/tools/fonts/build_app_fonts.py`가 `parity/screens/fonts.json`의 원본에서 만든다(라이선스는
//! `assets/fonts/LICENSE-*.txt`). gpui-pre-linux는 시스템 글꼴을 번들 글꼴보다 먼저 읽고, family 이름이 같으면
//! 먼저 읽힌 시스템 얼굴을 고른다. 그래서 모든 번들 얼굴의 family 이름에 `KanVibe ` 접두어를 붙여 시스템 글꼴과
//! 겹치지 않게 했고, 아래 대체 순서도 그 이름만 쓴다. 대체 순서 어디에도 없는 글자만 cosmic-text 자체 대체로 넘어간다.
//!
//! 앱 CSS가 요청한 `Inter`·`JetBrains Mono`는 기준 환경에 없어서 Chromium이 일반 이름(`sans-serif`, `monospace`)까지
//! 내려갔다. 여기서는 그 결과 family와 글자별 대체 순서를 바로 지정한다.

use std::borrow::Cow;

use gpui_kit::{App, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, SharedString};

pub const BUNDLED_FONT_FILES: [&[u8]; 11] = [
    include_bytes!("../assets/fonts/KanVibeNotoSans-Regular.ttf"),
    include_bytes!("../assets/fonts/KanVibeNotoSans-Bold.ttf"),
    // Mermaid 이탤릭 글자(class 추상 멤버·C4 등)는 Chromium이 이 얼굴을 골랐다. family는 Regular와 같은
    // `KanVibe Noto Sans`라서 GPUI `FontStyle::Italic`과 다이어그램 `FontBook` 모두 같은 family에서 고른다
    include_bytes!("../assets/fonts/KanVibeNotoSans-Italic.ttf"),
    include_bytes!("../assets/fonts/KanVibeNotoSans-BoldItalic.ttf"),
    include_bytes!("../assets/fonts/KanVibeNotoSansCJKkr-Regular.otf"),
    include_bytes!("../assets/fonts/KanVibeNotoSansCJKkr-SyntheticBold.otf"),
    include_bytes!("../assets/fonts/KanVibeNotoSansSymbols-Regular.ttf"),
    include_bytes!("../assets/fonts/KanVibeNotoSansSymbols2-Regular.ttf"),
    include_bytes!("../assets/fonts/KanVibeDejaVuSans.ttf"),
    include_bytes!("../assets/fonts/KanVibeDejaVuSansMono.ttf"),
    include_bytes!("../assets/fonts/KanVibeDejaVuSansMono-Bold.ttf"),
];

/// 터미널 프롬프트 아이콘(Powerline·Nerd Font 사설 영역 글자). TS 터미널은 JetBrainsMono Nerd Font Mono를 웹폰트로 받아 첫 순서에 썼다.
/// 기호만 든 얼굴이라 'm' 글리프가 없어 GPUI가 대체 순서에서는 빼지만(`SYMBOLS2_FAMILY`와 같다), 텍스트 시스템에 올려 두면
/// cosmic-text 자체 대체가 앞 얼굴들에 없는 글자를 이 얼굴로 그린다. 이름으로 고르지 않으므로 `KanVibe ` 접두어가 필요 없다.
/// 다이어그램·문서 미리보기 글꼴 묶음(`BUNDLED_FONT_FILES`)에는 넣지 않아 그쪽 기호 대체는 그대로다
pub const TERMINAL_ICON_FONT_FILE: &[u8] =
    include_bytes!("../assets/fonts/SymbolsNerdFontMono-Regular.ttf");

/// `--font-sans`가 기준 환경에서 풀린 Noto Sans
pub const SANS_FAMILY: &str = "KanVibe Noto Sans";
/// `--font-mono`·터미널·Monaco가 기준 환경에서 풀린 DejaVu Sans Mono
pub const MONO_FAMILY: &str = "KanVibe DejaVu Sans Mono";
/// 한글을 그리는 Noto Sans CJK KR Regular
pub const CJK_FAMILY: &str = "KanVibe Noto Sans CJK KR";
/// Chromium이 굵은 한글을 그린 방식(Regular 윤곽선을 글자 크기 1/24만큼 FT_Outline_Embolden)을 미리 적용한 얼굴.
/// 한글 블록·CJK 기호·전각 문자와 메시지 파일의 CJK 글자만 들어 있고, 나머지는 다음 대체 글꼴로 넘어간다.
pub const CJK_SYNTHETIC_BOLD_FAMILY: &str = "KanVibe Noto Sans CJK KR Synthetic Bold";
const SYMBOLS_FAMILY: &str = "KanVibe Noto Sans Symbols";
/// GPUI는 'm' 글리프가 없는 얼굴을 대체 순서에서 빼므로 이 얼굴은 실제로는 건너뛴다(`load_family`의 'm' 검사)
const SYMBOLS2_FAMILY: &str = "KanVibe Noto Sans Symbols2";
const DEJAVU_SANS_FAMILY: &str = "KanVibe DejaVu Sans";

/// fontconfig `sans-serif` 별칭의 나머지 순서. 한글이 CJK JP 얼굴로 그려지지 않게 KR을 먼저 둔다.
const SANS_FALLBACKS: [&str; 4] = [
    CJK_FAMILY,
    SYMBOLS_FAMILY,
    SYMBOLS2_FAMILY,
    DEJAVU_SANS_FAMILY,
];
/// fontconfig `monospace` 별칭의 나머지 순서. Mono CJK는 기준 환경에도 없어 Sans CJK KR로 떨어진다.
const MONO_FALLBACKS: [&str; 3] = [CJK_FAMILY, SYMBOLS_FAMILY, SYMBOLS2_FAMILY];

/// 번들 글꼴을 앱 텍스트 시스템에 올린다. 창을 열기 전에 불러야 첫 레이아웃부터 번들 얼굴을 쓴다.
pub fn register_bundled_fonts(cx: &App) -> anyhow::Result<()> {
    let font_files = BUNDLED_FONT_FILES
        .iter()
        .chain([&TERMINAL_ICON_FONT_FILE])
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

/// 굵은 글씨면 CJK 자리 앞에 합성 bold 얼굴을 끼운다. 합성 bold에 없는 CJK 글자는 바로 뒤 Regular로 그린다.
fn fallback_chain(fallbacks: &[&str], weight: FontWeight) -> Vec<String> {
    let mut chain = Vec::with_capacity(fallbacks.len() + 1);
    for family in fallbacks {
        if *family == CJK_FAMILY && weight == FontWeight::BOLD {
            chain.push(CJK_SYNTHETIC_BOLD_FAMILY.to_string());
        }
        chain.push((*family).to_string());
    }
    chain
}

fn font_with_fallbacks(family: &str, fallbacks: &[&str], css_weight: u16) -> Font {
    let weight = resolve_css_weight(css_weight);
    Font {
        family: SharedString::from(family.to_string()),
        features: FontFeatures::default(),
        fallbacks: Some(FontFallbacks::from_fonts(fallback_chain(fallbacks, weight))),
        weight,
        style: FontStyle::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terminal_icon_font_is_the_nerd_font_symbols_face() {
        let mut database = resvg::usvg::fontdb::Database::new();
        database.load_font_data(TERMINAL_ICON_FONT_FILE.to_vec());
        let families: Vec<String> = database
            .faces()
            .flat_map(|face| face.families.iter().map(|(family, _)| family.clone()))
            .collect();
        assert_eq!(families, ["Symbols Nerd Font Mono"]);
    }

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
        assert_eq!(fallbacks.fallback_list()[0], CJK_FAMILY);
    }

    #[test]
    fn bold_text_draws_korean_with_the_synthetic_bold_face_first() {
        for font in [sans_font(600), mono_font(700)] {
            let fallbacks = font.fallbacks.expect("font must carry fallbacks");
            assert_eq!(
                &fallbacks.fallback_list()[..2],
                [CJK_SYNTHETIC_BOLD_FAMILY, CJK_FAMILY]
            );
        }
        let regular = sans_font(500)
            .fallbacks
            .expect("sans font must carry fallbacks");
        assert!(
            !regular
                .fallback_list()
                .iter()
                .any(|family| family == CJK_SYNTHETIC_BOLD_FAMILY)
        );
    }

    #[test]
    fn every_family_the_app_names_is_bundled_under_a_kanvibe_name() {
        for family in [SANS_FAMILY, MONO_FAMILY, CJK_SYNTHETIC_BOLD_FAMILY]
            .into_iter()
            .chain(SANS_FALLBACKS)
            .chain(MONO_FALLBACKS)
        {
            assert!(family.starts_with("KanVibe "), "{family}");
            let utf16_name: Vec<u8> = family.encode_utf16().flat_map(u16::to_be_bytes).collect();
            assert!(
                BUNDLED_FONT_FILES.iter().any(|bytes| name_table(bytes)
                    .windows(utf16_name.len())
                    .any(|window| window == utf16_name)),
                "{family} is not the name of any bundled font"
            );
        }
    }

    /// sfnt 표 디렉터리에서 `name` 표 바이트를 찾는다
    fn name_table(font: &[u8]) -> &[u8] {
        let read_u32 =
            |at: usize| u32::from_be_bytes(font[at..at + 4].try_into().unwrap()) as usize;
        let table_count = u16::from_be_bytes([font[4], font[5]]) as usize;
        (0..table_count)
            .map(|index| 12 + 16 * index)
            .find(|&entry| &font[entry..entry + 4] == b"name")
            .map(|entry| &font[read_u32(entry + 8)..read_u32(entry + 8) + read_u32(entry + 12)])
            .expect("bundled font has a name table")
    }
}
