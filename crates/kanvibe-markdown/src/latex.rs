//! marked `latex` 인라인 확장. TS 토크나이저 정규식
//! `^(?:\$\$([\s\S]+?)\$\$|\\\[([\s\S]+?)\\\]|\\\(([\s\S]+?)\\\)|\$(?![\s$])([^$\n]+?)\$(?!\d))`
//! 를 같은 우선순위로 따른다. 어느 자리에서 시도할지는 [`crate::scan`]이 정한다.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LatexSpan {
    /// 구분자를 포함한 원문
    pub raw: String,
    pub formula: String,
    pub is_display: bool,
}

pub fn is_escaped(text: &str, index: usize) -> bool {
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

pub fn match_latex(rest: &str) -> Option<(usize, LatexSpan)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn formula(source: &str) -> Option<(String, bool)> {
        match_latex(source).map(|(_, span)| (span.formula, span.is_display))
    }

    #[test]
    fn 네_가지_구분자를_찾는다() {
        assert_eq!(formula("$E=mc^2$ x"), Some(("E=mc^2".to_string(), false)));
        assert_eq!(
            formula("\\[\\frac{a}{b}\\]"),
            Some(("\\frac{a}{b}".to_string(), true))
        );
        assert_eq!(formula("\\(x\\)"), Some(("x".to_string(), false)));
        assert_eq!(formula("$$y$$"), Some(("y".to_string(), true)));
    }

    #[test]
    fn 가격과_빈_줄을_넘는_구간은_수식이_아니다() {
        assert_eq!(formula("$5 and $10"), None);
        assert_eq!(formula("$ x$"), None);
        assert_eq!(formula("$$a\n\nb$$"), None);
    }
}
