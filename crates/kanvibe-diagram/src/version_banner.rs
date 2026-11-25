//! info 다이어그램(infoRenderer.ts). Mermaid 판 번호 `v11.15.0` 한 줄만 그린다.
//!
//! 원본 SVG는 viewBox 없이 `configureSvgSize(svg, 100, 400, true)`로 최대 폭 400px만 정한다. 모바일 워커는
//! 이 400×150 상자(SVG 기본 높이 150)를 폭 784px에 맞춰 1.96배로 늘린 784×294 크기로 놓지만, viewBox가 없으니
//! 글자는 늘지 않고 CSS px 좌표 그대로 그려진다. 그래서 장면은 그 784×294 상자를 viewBox처럼 쓴다.

use crate::scene::{Scene, TextItem};
use crate::text::FontWeight;
use crate::{DiagramError, RenderContext};

/// infoDb의 판 번호(package.json version)
const MERMAID_VERSION: &str = "11.15.0";
/// 워커가 400×150 상자를 폭 784px에 맞춘 크기
const BOX_WIDTH: f64 = 784.0;
const BOX_HEIGHT: f64 = 294.0;
/// `<text x="100" y="40" font-size="32" text-anchor: middle>`
const TEXT_CENTER_X: f64 = 100.0;
const TEXT_BASELINE: f64 = 40.0;
const FONT_SIZE: f64 = 32.0;

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    validate(definition)?;
    let text = format!("v{MERMAID_VERSION}");
    let width = context
        .fonts
        .shape_line(&text, FONT_SIZE, FontWeight::Regular)
        .width;
    let mut scene = Scene {
        width: BOX_WIDTH,
        height: BOX_HEIGHT,
        items: Vec::new(),
        is_start_aligned: false,
    };
    scene.push_text(TextItem {
        text,
        x: TEXT_CENTER_X - width / 2.0,
        baseline: TEXT_BASELINE,
        font_size: FONT_SIZE,
        weight: FontWeight::Regular,
        color: context.palette.text,
        rotation: None,
    });
    Ok(scene)
}

/// info 문법: `info` 뒤에 `showInfo`가 하나 올 수 있고, 그 뒤에는 title·accTitle·accDescr 문장만 온다
fn validate(definition: &str) -> Result<(), DiagramError> {
    let body = crate::directive::diagram_body(definition);
    let mut words = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"))
        .flat_map(str::split_whitespace);
    if words.next() != Some("info") {
        return Err(DiagramError::Syntax("info 머리줄".to_string()));
    }
    let rest: Vec<&str> = words.collect();
    let statements = match rest.first() {
        Some(&"showInfo") => &rest[1..],
        _ => &rest[..],
    };
    let is_accessibility = |word: &&str| {
        ["title", "accTitle", "accDescr"]
            .iter()
            .any(|keyword| word.starts_with(keyword))
    };
    match statements.first() {
        Some(word) if !is_accessibility(word) => {
            Err(DiagramError::Syntax(format!("info 문법: {word}")))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LabelMode;
    use crate::scene::SceneItem;
    use crate::text::tests::bundled_fonts;
    use crate::theme::{Palette, Theme};

    fn render_text(definition: &str) -> Result<Scene, DiagramError> {
        let fonts = bundled_fonts();
        let palette = Palette::for_theme(Theme::Light);
        let context = RenderContext {
            fonts: &fonts,
            palette: &palette,
            label_mode: LabelMode::Html,
            theme_variables: Default::default(),
        };
        render(definition, &context)
    }

    #[test]
    fn 판_번호를_가운데_맞춰_한_줄로_그린다() {
        let scene = render_text("info").unwrap();
        assert_eq!((scene.width, scene.height), (784.0, 294.0));
        let SceneItem::Text(text) = &scene.items[0] else {
            panic!("글자가 있어야 합니다");
        };
        assert_eq!(text.text, "v11.15.0");
        assert_eq!(text.baseline, 40.0);
        assert!(text.x > 30.0 && text.x < 50.0, "{}", text.x);
    }

    #[test]
    fn show_info는_받고_다른_낱말은_문법_오류다() {
        assert!(render_text("info showInfo").is_ok());
        assert!(render_text("info\n  title 판").is_ok());
        assert!(render_text("info nonsense").is_err());
    }
}
