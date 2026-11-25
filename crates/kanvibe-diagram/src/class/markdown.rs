//! createText의 markdown 처리 중 화면에 드러나는 인라인 강조만 옮긴다.
//!
//! marked(GFM)의 구분자 규칙대로 `**굵게**`·`__굵게__`는 strong, `*기울임*`·`_기울임_`은 em이 되고, `\*`처럼
//! 백슬래시로 막은 문장 부호는 글자로 남는다. 밑줄은 낱말 안에서 강조를 열거나 닫지 못한다(`snake_case`).
//! 목록·제목처럼 줄 단위 문법이나 코드·링크는 원본도 원문을 그대로 쓰므로 건드리지 않는다.

/// 같은 강조가 이어진 글자 조각
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledSpan {
    pub text: String,
    pub is_strong: bool,
    pub is_emphasis: bool,
    /// strong과 em이 겹칠 때 strong이 안쪽 요소인지(`***x***`는 `<em><strong>x</strong></em>`)
    pub is_strong_innermost: bool,
}

impl StyledSpan {
    /// createFormattedText(markdownToLines)는 글자를 감싼 가장 안쪽 강조 하나만 tspan에 옮긴다.
    /// 그래서 SVG 글자에서는 겹친 강조 중 안쪽 것만 남는다
    pub fn innermost_only(&self) -> StyledSpan {
        let is_both = self.is_strong && self.is_emphasis;
        StyledSpan {
            text: self.text.clone(),
            is_strong: self.is_strong && (!is_both || self.is_strong_innermost),
            is_emphasis: self.is_emphasis && (!is_both || !self.is_strong_innermost),
            is_strong_innermost: self.is_strong_innermost,
        }
    }
}

/// 글자 하나 또는 `*`·`_` 구분자 묶음
#[derive(Clone, Debug)]
enum Token {
    Character(char),
    Delimiter(DelimiterRun),
}

#[derive(Clone, Debug)]
struct DelimiterRun {
    mark: char,
    remaining: usize,
    can_open: bool,
    can_close: bool,
}

/// 토큰마다 감싼 strong·em 깊이
#[derive(Clone, Copy, Debug, Default)]
struct Depth {
    strong: usize,
    emphasis: usize,
    /// 안쪽 짝부터 맞추므로 처음 더한 강조가 가장 안쪽 요소다
    is_strong_innermost: Option<bool>,
}

pub fn parse_inline(text: &str) -> Vec<StyledSpan> {
    let mut tokens = tokenize(text);
    let mut depths = vec![Depth::default(); tokens.len()];
    match_delimiters(&mut tokens, &mut depths);
    collect_spans(&tokens, &depths)
}

fn tokenize(text: &str) -> Vec<Token> {
    let characters: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        let next = characters.get(index + 1).copied();
        if character == '\\' && next.is_some_and(|next| next.is_ascii_punctuation()) {
            tokens.push(Token::Character(next.unwrap_or(character)));
            index += 2;
            continue;
        }
        if character != '*' && character != '_' {
            tokens.push(Token::Character(character));
            index += 1;
            continue;
        }
        let end = characters[index..]
            .iter()
            .position(|other| *other != character)
            .map_or(characters.len(), |offset| index + offset);
        let previous = index.checked_sub(1).map(|before| characters[before]);
        tokens.push(Token::Delimiter(delimiter_run(
            character,
            end - index,
            previous,
            characters.get(end).copied(),
        )));
        index = end;
    }
    tokens
}

/// CommonMark 왼쪽·오른쪽 기댐(flanking) 규칙. 줄 끝은 공백으로 친다
fn delimiter_run(
    mark: char,
    length: usize,
    previous: Option<char>,
    next: Option<char>,
) -> DelimiterRun {
    let is_space = |character: Option<char>| character.is_none_or(char::is_whitespace);
    let is_punctuation = |character: Option<char>| {
        character
            .is_some_and(|character| !character.is_alphanumeric() && !character.is_whitespace())
    };
    let is_left = !is_space(next)
        && (!is_punctuation(next) || is_space(previous) || is_punctuation(previous));
    let is_right = !is_space(previous)
        && (!is_punctuation(previous) || is_space(next) || is_punctuation(next));
    let (can_open, can_close) = if mark == '_' {
        (
            is_left && (!is_right || is_punctuation(previous)),
            is_right && (!is_left || is_punctuation(next)),
        )
    } else {
        (is_left, is_right)
    };
    DelimiterRun {
        mark,
        remaining: length,
        can_open,
        can_close,
    }
}

/// 닫는 구분자마다 가장 가까운 같은 종류의 여는 구분자와 짝을 짓고, 둘 다 두 개 이상이면 strong이다
fn match_delimiters(tokens: &mut [Token], depths: &mut [Depth]) {
    for closer in 0..tokens.len() {
        while let Some(opener) = find_opener(tokens, closer) {
            let used = match (&tokens[opener], &tokens[closer]) {
                (Token::Delimiter(open), Token::Delimiter(close))
                    if open.remaining >= 2 && close.remaining >= 2 =>
                {
                    2
                }
                _ => 1,
            };
            mark_between(
                &tokens[opener + 1..closer],
                &mut depths[opener + 1..closer],
                used,
            );
            consume(&mut tokens[opener], used);
            consume(&mut tokens[closer], used);
        }
    }
}

/// 짝 사이 글자에 strong(두 개) 또는 em(한 개) 깊이를 더한다
fn mark_between(tokens: &[Token], depths: &mut [Depth], used: usize) {
    for (token, depth) in tokens.iter().zip(depths) {
        if !matches!(token, Token::Character(_)) {
            continue;
        }
        if used == 2 {
            depth.strong += 1;
        } else {
            depth.emphasis += 1;
        }
        depth.is_strong_innermost.get_or_insert(used == 2);
    }
}

fn find_opener(tokens: &[Token], closer: usize) -> Option<usize> {
    let Token::Delimiter(close) = &tokens[closer] else {
        return None;
    };
    if !close.can_close || close.remaining == 0 {
        return None;
    }
    (0..closer).rev().find(|index| {
        matches!(&tokens[*index], Token::Delimiter(open)
            if open.can_open && open.remaining > 0 && open.mark == close.mark)
    })
}

fn consume(token: &mut Token, used: usize) {
    if let Token::Delimiter(run) = token {
        run.remaining -= used;
    }
}

/// 짝을 못 찾은 구분자 글자는 그 자리의 글자로 남긴다
fn collect_spans(tokens: &[Token], depths: &[Depth]) -> Vec<StyledSpan> {
    let mut spans: Vec<StyledSpan> = Vec::new();
    for (token, depth) in tokens.iter().zip(depths) {
        let text: String = match token {
            Token::Character(character) => character.to_string(),
            Token::Delimiter(run) => std::iter::repeat_n(run.mark, run.remaining).collect(),
        };
        if text.is_empty() {
            continue;
        }
        let span = StyledSpan {
            text,
            is_strong: depth.strong > 0,
            is_emphasis: depth.emphasis > 0,
            is_strong_innermost: depth.is_strong_innermost.unwrap_or(false),
        };
        match spans.last_mut() {
            Some(last) if is_same_style(last, &span) => last.text.push_str(&span.text),
            _ => spans.push(span),
        }
    }
    spans
}

fn is_same_style(first: &StyledSpan, second: &StyledSpan) -> bool {
    first.is_strong == second.is_strong
        && first.is_emphasis == second.is_emphasis
        && first.is_strong_innermost == second.is_strong_innermost
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(text: &str) -> String {
        parse_inline(text)
            .iter()
            .map(|span| match (span.is_strong, span.is_emphasis) {
                (true, true) => format!("[BI:{}]", span.text),
                (true, false) => format!("[B:{}]", span.text),
                (false, true) => format!("[I:{}]", span.text),
                (false, false) => span.text.clone(),
            })
            .collect()
    }

    #[test]
    fn 강조와_굵게를_가린다() {
        assert_eq!(render("+__init__()"), "+[B:init]()");
        assert_eq!(render("**bold** and *em*"), "[B:bold] and [I:em]");
        assert_eq!(render("+snake_case_name"), "+snake_case_name");
        assert_eq!(render("+area()* double"), "+area()* double");
        assert_eq!(render("a \\*b\\* c"), "a *b* c");
        assert_eq!(render("List<T> items"), "List<T> items");
    }

    #[test]
    fn 겹친_강조는_svg_글자에서_안쪽_것만_남는다() {
        let innermost = |text: &str| {
            let span = parse_inline(text)[0].innermost_only();
            (span.is_strong, span.is_emphasis)
        };
        assert_eq!(render("***both***"), "[BI:both]");
        assert_eq!(innermost("***both***"), (true, false));
        assert_eq!(innermost("**_both_**"), (false, true));
        assert_eq!(innermost("*_x_*"), (false, true));
        assert_eq!(innermost("**x**"), (true, false));
    }
}
