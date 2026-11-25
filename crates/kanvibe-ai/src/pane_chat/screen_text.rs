//! 화면 줄을 다루는 JS 문자열 연산. 칸 계산은 TS와 같이 UTF-16 단위로 센다.

use crate::javascript::{is_js_whitespace, js_trim_start, utf16_length};

/// JS `text.replace(/\s/g, "")`
pub fn without_whitespace(text: &str) -> String {
    text.chars()
        .filter(|character| !is_js_whitespace(*character))
        .collect()
}

/// 줄 앞 공백의 UTF-16 길이
pub fn indent_of(line: &str) -> usize {
    utf16_length(line) - utf16_length(js_trim_start(line))
}

/// JS `lines[row] ?? ""`
pub fn row_at<'a>(lines: &[&'a str], row: usize) -> &'a str {
    lines.get(row).copied().unwrap_or("")
}

/// `&[String]`이든 `&[&str]`이든 같은 줄 목록으로 본다
pub fn as_rows<S: AsRef<str>>(lines: &[S]) -> Vec<&str> {
    lines.iter().map(AsRef::as_ref).collect()
}
