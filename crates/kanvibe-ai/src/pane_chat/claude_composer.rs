//! Claude Code 입력란 읽기와, 붙여넣은 글이 입력란에 그대로 보이는지 확인.

use regex::Regex;
use std::sync::LazyLock;

use super::screen_text::{as_rows, row_at, without_whitespace};
use crate::javascript::{js_trim, js_trim_end, utf16_length, utf16_suffix_from};

/// 입력란의 첫 줄. 표시 기호(셸 모드에서는 !)와 줄바꿈 없는 공백 뒤에 글이 온다
static COMPOSER_FIRST_ROW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([❯!])[ \x{A0}]([^\n\r\x{2028}\x{2029}]*)$").expect("입력란 첫 줄 패턴")
});
/// 입력란에서 이어지는 줄의 들여쓰기
const COMPOSER_CONTINUATION_INDENT: &str = "  ";
/// 입력란 위쪽의 경계선
static COMPOSER_BORDER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "^─{{10,}}[{}]*$",
        crate::javascript::JS_WHITESPACE_CLASS
    ))
    .expect("경계선 패턴")
});
/// 붙여넣은 이미지 하나를 대신하는 표시 `[Image #N]`. 공백을 뺀 글에서 찾는다
static IMAGE_PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[Image#[0-9]+\]").expect("이미지 표시 패턴"));
/// 붙여넣은 글을 접어 보여 주는 표시 `[Pasted text #N +M lines]`. 공백을 뺀 글에서 찾는다
static FOLDED_PASTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\[Pastedtext#[0-9]+(?:\+([0-9]+)lines)?\]$").expect("접은 붙여넣기 패턴")
});
/// 한 줄짜리 글을 접기 시작하는 길이의 하한. 700자는 그대로 보였고 1,100자는 접혔다
const FOLDED_SINGLE_LINE_MIN_LENGTH: usize = 700;
/// 여러 줄 글을 접기 시작하는 줄 수의 하한. 3줄은 그대로 보였고 4줄부터 접혔다
const FOLDED_MIN_LINE_COUNT: usize = 4;

/// 이어지는 줄은 두 칸 들여쓰고, 문단 사이나 글 끝의 빈 줄은 화면에서도 빈 줄이다
fn is_continuation_row(line: &str) -> bool {
    js_trim(line).is_empty() || line.starts_with(COMPOSER_CONTINUATION_INDENT)
}

/// Claude Code 입력란에 지금 들어 있는 글을 화면에서 읽는다. 커서가 입력란의 마지막 줄에 있을 때만 읽고, 아니면 `None`이다.
///
/// 입력란은 경계선 바로 아래에서 시작한다. 대화 본문에 남은 지난 메시지나 선택 화면의 선택지도 같은 기호로 시작하므로,
/// 첫 줄 위가 경계선인지를 함께 본다. Claude Code 2.1.326의 실제 화면으로 확인한 배치다
pub fn read_claude_composer_text<S: AsRef<str>>(lines: &[S], cursor_row: usize) -> Option<String> {
    let lines = as_rows(lines);
    let mut first_row = cursor_row;
    while first_row > 0
        && !COMPOSER_FIRST_ROW.is_match(row_at(&lines, first_row))
        && is_continuation_row(row_at(&lines, first_row))
    {
        first_row -= 1;
    }
    let first = COMPOSER_FIRST_ROW.captures(row_at(&lines, first_row))?;
    if first_row < 1 || !COMPOSER_BORDER.is_match(lines[first_row - 1]) {
        return None;
    }
    let prefix = if &first[1] == "!" { "!" } else { "" };
    let mut rows = vec![format!("{prefix}{}", &first[2])];
    let end = (cursor_row + 1).min(lines.len());
    if first_row + 1 < end {
        rows.extend(
            lines[first_row + 1..end].iter().map(|line| {
                utf16_suffix_from(line, COMPOSER_CONTINUATION_INDENT.len()).to_string()
            }),
        );
    }
    Some(js_trim_end(&rows.join("\n")).to_string())
}

/// 입력란의 글이 방금 붙여넣은 글 그대로인지 확인한다.
///
/// Claude는 긴 글을 화면 너비에 맞춰 여러 줄로 그리므로 공백과 줄바꿈은 견주지 않는다. 4줄 이상이거나 아주 긴 글은
/// `[Pasted text #N +M lines]`로 접어 보여 주고 이미지는 `[Image #N]`으로 바꾸므로, 그 표시가 붙여넣은 글의 모양과 맞는지를 본다
pub fn is_claude_composer_showing_paste(
    composer_text: &str,
    pasted_text: &str,
    image_count: usize,
) -> bool {
    let mut shown = without_whitespace(composer_text);
    for _ in 0..image_count {
        let Some(placeholder) = IMAGE_PLACEHOLDER.find(&shown) else {
            return false;
        };
        shown = shown[placeholder.end()..].to_string();
    }
    if IMAGE_PLACEHOLDER.is_match(&shown) {
        return false;
    }
    if shown == without_whitespace(pasted_text) {
        return true;
    }
    let Some(folded) = FOLDED_PASTE.captures(&shown) else {
        return false;
    };
    let line_count = pasted_text.split('\n').count();
    let is_long = utf16_length(pasted_text) >= FOLDED_SINGLE_LINE_MIN_LENGTH;
    match folded.get(1) {
        None => line_count == 1 && is_long,
        Some(folded_lines) => {
            folded_lines.as_str().parse::<f64>().ok() == Some((line_count - 1) as f64)
                && (line_count >= FOLDED_MIN_LINE_COUNT || is_long)
        }
    }
}

#[cfg(test)]
mod tests;
