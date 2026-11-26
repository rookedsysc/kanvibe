//! db.ts calculateTextProps: 상자 HTML 내용과 그 크기.
//!
//! 내용은 `<b>이름</b>`이고, 데이터가 있으면 `<br/><br/><code …>데이터</code>`를 붙인다. 데이터는 감싼 기호를
//! JavaScript `substring`으로 떼고(마지막 `}` 바로 앞 한 글자도 함께 떨어진다), 공백 단위로 430px 안에서 줄을
//! 바꾼 뒤 공백을 `&nbsp;`로 바꾼다. 크기는 calculateTextDimensions가 태그·엔티티까지 굵은 16px SVG 글자로 잰
//! 값이라 실제 글자보다 넓다. 데이터가 있으면 원본처럼 그 폭의 1/3을 쓴다.

use crate::sanitize::sanitize_text;
use crate::text::{FontBook, FontWeight};

use super::layout::entity_name;
use super::parser::{DataBlock, Frame};

const FONT_SIZE: f64 = 16.0;
/// diagramProps.textMaxWidth(450 − 2·10)
pub const TEXT_MAX_WIDTH: f64 = 430.0;
const CODE_OPEN: &str = "<code style=\"text-align: left; display: block;max-width:430px\">";

/// 상자 하나의 내용
#[derive(Clone, Debug, PartialEq)]
pub struct BoxContent {
    pub name: String,
    /// `<code>` 안 HTML. 데이터가 없으면 `None`
    pub code_html: Option<String>,
    /// textProps 폭·높이
    pub width: f64,
    pub height: f64,
}

pub fn box_content(fonts: &FontBook, frame: &Frame, data_blocks: &[DataBlock]) -> BoxContent {
    let name = sanitize_text(entity_name(&frame.identifier));
    let mut code_html = frame
        .inline_data
        .as_deref()
        .map(|value| data_html(fonts, value, "{", 1));
    let referenced = frame
        .data_reference
        .as_ref()
        .and_then(|reference| data_blocks.iter().find(|block| block.name == *reference));
    if let Some(block) = referenced {
        code_html = Some(data_html(fonts, &block.block, "{\n", 2) + "<br/>");
    }
    let mut content = format!("<b>{name}</b>");
    if let Some(html) = &code_html {
        content.push_str(&format!("<br/><br/>{CODE_OPEN}{html}</code>"));
    }
    let (measured_width, height) = text_dimensions(fonts, &content);
    let width = match code_html.is_some() {
        true => measured_width / 3.0,
        false => measured_width,
    };
    BoxContent {
        name,
        code_html,
        width,
        height,
    }
}

/// 감싼 기호를 떼고 줄을 감싼 뒤 공백을 `&nbsp;`로 바꾼 데이터 HTML
fn data_html(fonts: &FontBook, raw: &str, opening: &str, opening_length: i64) -> String {
    let start = js_index_of(raw, opening).map_or(0, |index| index + opening_length);
    let inner = js_substring(raw, start, i64::MAX);
    let end = js_last_index_of(&inner, "}").unwrap_or(-1) - 1;
    let inner = js_substring(&inner, 0, end);
    let wrapped = wrap_label(fonts, &sanitize_text(&inner));
    wrapped.replace(' ', "&nbsp;")
}

fn js_index_of(text: &str, pattern: &str) -> Option<i64> {
    let byte = text.find(pattern)?;
    Some(text[..byte].chars().count() as i64)
}

fn js_last_index_of(text: &str, pattern: &str) -> Option<i64> {
    let byte = text.rfind(pattern)?;
    Some(text[..byte].chars().count() as i64)
}

/// JavaScript `substring`: 음수는 0, 길이를 넘으면 길이로 자르고, 시작이 끝보다 크면 바꾼다
fn js_substring(text: &str, start: i64, end: i64) -> String {
    let length = text.chars().count() as i64;
    let start = start.clamp(0, length) as usize;
    let end = end.clamp(0, length) as usize;
    let (from, to) = (start.min(end), start.max(end));
    text.chars().skip(from).take(to - from).collect()
}

/// common.lineBreakRegex(`<br\s*/?>`, 대소문자 무시)로 줄을 나눈다
fn split_breaks(text: &str) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut rest = text;
    while let Some(character) = rest.chars().next() {
        let tag_length = break_length(rest);
        if tag_length > 0 {
            lines.push(String::new());
            rest = &rest[tag_length..];
            continue;
        }
        if let Some(line) = lines.last_mut() {
            line.push(character);
        }
        rest = &rest[character.len_utf8()..];
    }
    lines
}

/// `rest`가 `<br>`·`<br/>`·`<br />`(대소문자 무시)로 시작하면 그 길이
fn break_length(rest: &str) -> usize {
    let is_tag_start = rest
        .get(..3)
        .is_some_and(|head| head.eq_ignore_ascii_case("<br"));
    if !is_tag_start {
        return 0;
    }
    let after = &rest[3..];
    let spaces = after.len() - after.trim_start().len();
    let slash = usize::from(after[spaces..].starts_with('/'));
    match after[spaces + slash..].starts_with('>') {
        true => 3 + spaces + slash + 1,
        false => 0,
    }
}

/// SVG 글자 한 줄 getBBox 폭. 줄바꿈은 공백이 되고 이어진 공백은 하나로 접힌다
fn svg_line_width(fonts: &FontBook, line: &str) -> f64 {
    let collapsed = collapse_spaces(line);
    fonts
        .measure_svg_runs(&[collapsed.as_str()], FONT_SIZE, FontWeight::Bold)
        .width()
}

/// calculateTextDimensions(굵은 16px): 줄마다 반올림한 폭의 최댓값과 줄 높이(22px)의 합
fn text_dimensions(fonts: &FontBook, text: &str) -> (f64, f64) {
    if text.is_empty() {
        return (0.0, 0.0);
    }
    let metrics = fonts.primary_metrics(FONT_SIZE);
    let lines = split_breaks(text);
    let width = lines
        .iter()
        .map(|line| (svg_line_width(fonts, line) + 0.5).floor())
        .fold(0.0, f64::max);
    (
        width,
        lines.len() as f64 * (metrics.ascent + metrics.descent),
    )
}

/// wrapLabel(430px, 굵은 16px): 공백으로 나눈 낱말을 이어 붙이다 폭에 닿으면 `<br/>`로 줄을 바꾼다.
/// 430px보다 넓은 한 낱말을 글자 단위로 끊는 breakString은 옮기지 않았다
fn wrap_label(fonts: &FontBook, label: &str) -> String {
    if label.is_empty() || split_breaks(label).len() > 1 {
        return label.to_string();
    }
    let mut completed: Vec<String> = Vec::new();
    let mut next_line = String::new();
    for word in label.split(' ').filter(|word| !word.is_empty()) {
        let word_width = text_dimensions(fonts, &format!("{word} ")).0;
        let line_width = text_dimensions(fonts, &next_line).0;
        if line_width + word_width >= TEXT_MAX_WIDTH {
            completed.push(std::mem::replace(&mut next_line, word.to_string()));
        } else if next_line.is_empty() {
            next_line = word.to_string();
        } else {
            next_line = format!("{next_line} {word}");
        }
    }
    completed.push(next_line);
    completed.retain(|line| !line.is_empty());
    completed.join("<br/>")
}

/// `<code>` 안 HTML을 화면 줄로: `<br>`로 나눈 줄마다 엔티티를 풀고 공백을 접는다. 끝 `<br>` 뒤 빈 줄은 없다
pub fn code_lines(html: &str) -> Vec<String> {
    let mut lines: Vec<String> = split_breaks(html)
        .iter()
        .map(|line| collapse_spaces(&decode_entities(line)))
        .collect();
    if lines.len() > 1 && lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

fn decode_entities(text: &str) -> String {
    text.replace("&nbsp;", "\u{a0}")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// `white-space: normal`: 줄바꿈·탭은 공백이 되고 이어진 공백은 하나로 접히며 앞뒤 공백은 지운다(nbsp는 남긴다)
fn collapse_spaces(text: &str) -> String {
    let mut collapsed = String::with_capacity(text.len());
    for character in text.chars() {
        let character = match character {
            '\n' | '\t' | '\r' => ' ',
            other => other,
        };
        if character == ' ' && (collapsed.is_empty() || collapsed.ends_with(' ')) {
            continue;
        }
        collapsed.push(character);
    }
    collapsed.trim_end_matches(' ').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventmodeling::parser::parse;
    use crate::text::tests::bundled_fonts;

    fn contents() -> Vec<BoxContent> {
        let fonts = bundled_fonts();
        let model = parse(
            "eventmodeling\ntf 02 cmd AddItem { itemId: 42, quantity: 1 }\ntf 03 evt ItemAdded [[D]]\ndata D {\n  itemId: 42\n  quantity: 1\n}",
        )
        .unwrap();
        model
            .frames
            .iter()
            .map(|frame| box_content(&fonts, frame, &model.data_blocks))
            .collect()
    }

    #[test]
    fn 데이터_폭은_태그까지_잰_폭의_삼분의_일이다() {
        // eventmodeling-data 기준 SVG: 상자 폭 335.667·314.667, 높이 106·128
        let contents = contents();
        assert_eq!(
            contents[0].code_html.as_deref(),
            Some("itemId:&nbsp;42,&nbsp;quantity:&nbsp;1")
        );
        assert!(
            (contents[0].width - 887.0 / 3.0).abs() < 1e-9,
            "{}",
            contents[0].width
        );
        assert_eq!(contents[0].height, 66.0);
        assert!(
            (contents[1].width - 824.0 / 3.0).abs() < 1e-9,
            "{}",
            contents[1].width
        );
        assert_eq!(contents[1].height, 88.0);
    }

    #[test]
    fn 코드_줄은_공백을_접고_nbsp를_남긴다() {
        assert_eq!(
            code_lines("itemId:&nbsp;42\n&nbsp;quantity:&nbsp;1<br/>"),
            vec!["itemId:\u{a0}42 \u{a0}quantity:\u{a0}1"]
        );
    }
}
