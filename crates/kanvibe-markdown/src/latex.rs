//! marked `latex` 인라인 확장. TS 토크나이저 정규식
//! `^(?:\$\$([\s\S]+?)\$\$|\\\[([\s\S]+?)\\\]|\\\(([\s\S]+?)\\\)|\$(?![\s$])([^$\n]+?)\$(?!\d))`
//! 를 같은 우선순위로 따른다.
//!
//! pulldown-cmark는 `\[`를 이스케이프로 먹고 `$`를 모르므로, 파싱 전에 코드·HTML 블록 밖의 수식 구간을
//! 자리표시자(사용자 영역 문자)로 바꿔 두었다가 글자 조각에서 되살린다. marked처럼 백슬래시로 이스케이프한
//! `\$`·`\\(`는 수식으로 보지 않고, 빈 줄(문단 경계)을 넘는 구간도 만들지 않는다.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

pub const PLACEHOLDER_START: char = '\u{E000}';
pub const PLACEHOLDER_END: char = '\u{E001}';

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LatexSpan {
    /// 구분자를 포함한 원문
    pub raw: String,
    pub formula: String,
    pub is_display: bool,
}

/// 수식을 자리표시자로 바꾼 본문과 수식 목록
pub struct Substituted {
    pub text: String,
    pub spans: Vec<LatexSpan>,
}

/// 코드 블록·코드 구간·HTML 블록 범위(수식을 찾지 않는 곳)
fn excluded_ranges(markdown: &str, options: Options) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut code_block_start: Option<usize> = None;
    for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => code_block_start = Some(range.start),
            Event::End(TagEnd::CodeBlock) => {
                ranges.push(code_block_start.take().unwrap_or(range.start)..range.end);
            }
            Event::Code(_) | Event::Html(_) => ranges.push(range),
            _ => {}
        }
    }
    ranges
}

fn is_escaped(text: &str, index: usize) -> bool {
    let backslashes = text[..index]
        .chars()
        .rev()
        .take_while(|character| *character == '\\')
        .count();
    backslashes % 2 == 1
}

fn crosses_blank_line(content: &str) -> bool {
    let mut lines = content.split('\n');
    lines.next();
    let mut iter = lines.peekable();
    while let Some(line) = iter.next() {
        if line.trim().is_empty() && iter.peek().is_some() {
            return true;
        }
    }
    false
}

/// 구분자 쌍 사이 내용을 게으르게(가장 가까운 닫는 구분자) 찾는다
fn delimited(rest: &str, open: &str, close: &str) -> Option<(usize, String)> {
    let body = rest.strip_prefix(open)?;
    let end = body.get(1..)?.find(close)? + 1;
    let content = &body[..end];
    if crosses_blank_line(content) {
        return None;
    }
    Some((open.len() + end + close.len(), content.to_string()))
}

/// `$…$`: 여는 `$` 뒤는 공백·`$`가 아니고, 줄바꿈 없이 닫히며, 닫는 `$` 뒤에 숫자가 오지 않는다
fn inline_dollar(rest: &str) -> Option<(usize, String)> {
    let body = rest.strip_prefix('$')?;
    let first = body.chars().next()?;
    if first.is_whitespace() || first == '$' {
        return None;
    }
    let end = body.find(['$', '\n'])?;
    if body[end..].starts_with('\n') || end == 0 {
        return None;
    }
    let after = body[end + 1..].chars().next();
    if after.is_some_and(|character| character.is_ascii_digit()) {
        return None;
    }
    Some((end + 2, body[..end].to_string()))
}

fn match_latex(rest: &str) -> Option<(usize, LatexSpan)> {
    let candidates: [(Option<(usize, String)>, bool); 4] = [
        (delimited(rest, "$$", "$$"), true),
        (delimited(rest, "\\[", "\\]"), true),
        (delimited(rest, "\\(", "\\)"), false),
        (inline_dollar(rest), false),
    ];
    candidates.into_iter().find_map(|(found, is_display)| {
        found.map(|(length, formula)| {
            (
                length,
                LatexSpan {
                    raw: rest[..length].to_string(),
                    formula,
                    is_display,
                },
            )
        })
    })
}

pub fn substitute_latex(markdown: &str, options: Options) -> Substituted {
    let excluded = excluded_ranges(markdown, options);
    let is_excluded = |index: usize| excluded.iter().any(|range| range.contains(&index));
    let mut text = String::with_capacity(markdown.len());
    let mut spans = Vec::new();
    let mut cursor = 0;
    let mut index = 0;
    while index < markdown.len() {
        let rest = &markdown[index..];
        let is_candidate =
            rest.starts_with('$') || rest.starts_with("\\[") || rest.starts_with("\\(");
        if is_candidate
            && !is_excluded(index)
            && !is_escaped(markdown, index)
            && let Some((length, span)) = match_latex(rest)
            && !is_excluded(index + length - 1)
        {
            text.push_str(&markdown[cursor..index]);
            text.push(PLACEHOLDER_START);
            text.push_str(&spans.len().to_string());
            text.push(PLACEHOLDER_END);
            spans.push(span);
            index += length;
            cursor = index;
            continue;
        }
        index += rest.chars().next().map_or(1, char::len_utf8);
    }
    text.push_str(&markdown[cursor..]);
    Substituted { text, spans }
}

/// 글자 조각을 일반 글자와 수식 번호로 나눈다
pub fn split_placeholders(text: &str) -> Vec<Result<String, usize>> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(PLACEHOLDER_START) {
        let Some(end) = rest[start..]
            .find(PLACEHOLDER_END)
            .map(|offset| start + offset)
        else {
            break;
        };
        let Ok(number) = rest[start + PLACEHOLDER_START.len_utf8()..end].parse::<usize>() else {
            break;
        };
        if start > 0 {
            parts.push(Ok(rest[..start].to_string()));
        }
        parts.push(Err(number));
        rest = &rest[end + PLACEHOLDER_END.len_utf8()..];
    }
    if !rest.is_empty() {
        parts.push(Ok(rest.to_string()));
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formulas(markdown: &str) -> Vec<(String, bool)> {
        substitute_latex(markdown, Options::empty())
            .spans
            .into_iter()
            .map(|span| (span.formula, span.is_display))
            .collect()
    }

    #[test]
    fn 네_가지_구분자를_찾는다() {
        assert_eq!(
            formulas("Formula $E=mc^2$ and \\[\\frac{a}{b}\\] then \\(x\\) and $$y$$"),
            [
                ("E=mc^2".to_string(), false),
                ("\\frac{a}{b}".to_string(), true),
                ("x".to_string(), false),
                ("y".to_string(), true)
            ]
        );
    }

    #[test]
    fn 코드와_가격과_이스케이프는_수식이_아니다() {
        assert!(formulas("`$literal$` costs $5 and $10").is_empty());
        assert!(formulas("\\$x$ stays").is_empty());
        assert!(formulas("```\n$a$\n```").is_empty());
    }
}
