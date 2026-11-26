//! `iconText` 서비스의 foreignObject 글자(svgDraw.ts drawServices).
//!
//! `div.node-icon-text`(flex, 높이 iconSize, `align-items: center`) 안의 div가 흰 글자를 가운데 맞춤으로 쓴다.
//! 안쪽 div는 flex 항목이라 폭이 한 줄로 늘어놓은 글자 폭(최대 iconSize − 여백 2)이고 왼쪽에 붙는다. 줄 높이는
//! `normal`(반올림한 ascent + descent)이고, `-webkit-line-clamp: ⌊(iconSize − 2) / 16⌋`줄을 넘으면 마지막 줄 끝을
//! 말줄임표로 바꾸며, 넘치는 글자는 `overflow: hidden`으로 잘린다. HTML이라 라벨 방식과 상관없이 같다.

use crate::scene::{ClipItem, Color, SceneItem, TextItem};
use crate::text::{FontBook, FontWeight};

/// 글자 크기는 CSS(16px)를 물려받는다. `architecture.fontSize`와 상관없다
const FONT_SIZE: f64 = 16.0;
/// 안쪽 div `margin: 1px`
const MARGIN: f64 = 1.0;
const ELLIPSIS: char = '\u{2026}';
const WHITE: Color = Color::rgb(0xFF, 0xFF, 0xFF);

/// (left, top)에 놓인 `size`×`size` 아이콘 위에 쓸 글자 묶음(잘라 내기 포함)
pub(crate) fn icon_text_block(
    fonts: &FontBook,
    text: &str,
    (left, top): (f64, f64),
    size: f64,
) -> SceneItem {
    let available = size - 2.0 * MARGIN;
    let measure = |line: &str| fonts.measure_line(line, FONT_SIZE, FontWeight::Regular);
    let words: Vec<&str> = text.split_whitespace().collect();
    let clamp = ((size - 2.0) / FONT_SIZE).floor().max(1.0) as usize;
    let lines = clamped_lines(&words, available, clamp, &measure);
    let metrics = fonts.primary_metrics(FONT_SIZE);
    let line_height = metrics.ascent + metrics.descent;
    let block_width = measure(&words.join(" ")).min(available);
    let block_height = line_height * lines.len() as f64;
    let block_left = left + MARGIN;
    let block_top = top + (size - block_height) / 2.0;
    let items = lines
        .iter()
        .enumerate()
        .map(|(index, (line, width))| {
            SceneItem::Text(TextItem {
                text: line.clone(),
                x: block_left + (block_width - width) / 2.0,
                baseline: block_top + line_height * index as f64 + metrics.ascent,
                font_size: FONT_SIZE,
                weight: FontWeight::Regular,
                color: WHITE,
                rotation: None,
            })
        })
        .collect();
    SceneItem::Clip(ClipItem {
        x: block_left,
        y: block_top,
        width: block_width,
        height: block_height,
        items,
    })
}

/// (그릴 글자, 가운데 맞춤에 쓰는 폭) 줄 목록. 말줄임표는 맞춘 줄 끝에 붙어 가운데 맞춤 폭에 들지 않는다
fn clamped_lines(
    words: &[&str],
    available: f64,
    clamp: usize,
    measure: &dyn Fn(&str) -> f64,
) -> Vec<(String, f64)> {
    let mut lines: Vec<(String, f64)> = wrap_words(words, available, measure)
        .into_iter()
        .map(|line| {
            let width = measure(&line);
            (line, width)
        })
        .collect();
    if lines.len() > clamp {
        lines.truncate(clamp);
        let (last, _) = lines.pop().unwrap_or_default();
        let kept = kept_before_ellipsis(&last, available, measure);
        let width = measure(&kept);
        lines.push((format!("{kept}{ELLIPSIS}"), width));
    }
    lines
}

/// 공백에서만 줄을 바꾼다. 폭보다 긴 낱말은 그 줄에 그대로 둔다(넘친 부분은 잘린다)
fn wrap_words(words: &[&str], available: f64, measure: &dyn Fn(&str) -> f64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in words {
        match lines.last_mut() {
            Some(line) if measure(&format!("{line} {word}")) <= available => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push((*word).to_string()),
        }
    }
    lines
}

/// 말줄임표가 폭 안에 들어올 때까지 끝 글자를 지운 앞부분
fn kept_before_ellipsis(line: &str, available: f64, measure: &dyn Fn(&str) -> f64) -> String {
    let mut kept: String = line.to_string();
    loop {
        let trimmed = kept.trim_end().to_string();
        if trimmed.is_empty() || measure(&format!("{trimmed}{ELLIPSIS}")) <= available {
            return trimmed;
        }
        kept = trimmed;
        kept.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tests::bundled_fonts;

    fn texts(block: SceneItem) -> Vec<String> {
        let SceneItem::Clip(clip) = block else {
            panic!("잘라 내기 묶음");
        };
        clip.items
            .into_iter()
            .filter_map(|item| match item {
                SceneItem::Text(text) => Some(text.text),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn 줄_수를_넘으면_마지막_줄을_말줄임표로_끝낸다() {
        let fonts = bundled_fonts();
        let block = icon_text_block(
            &fonts,
            "A much longer icon text that clamps after a few lines",
            (0.0, 0.0),
            60.0,
        );
        // 기준 이미지(architecture-config, iconSize 60): 세 줄 "A much / longer / icon…"
        assert_eq!(texts(block), vec!["A much", "longer", "icon\u{2026}"]);
    }
}
