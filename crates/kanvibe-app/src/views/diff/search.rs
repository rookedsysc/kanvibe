//! 찾기 위젯의 검색(Monaco `TextModelSearch`). 대소문자 구분·단어 단위·정규식 세 토글을 따르고, 결과는 시작·끝 위치와 묶음 글자다.
//!
//! Monaco는 찾을 글자를 JS `RegExp`로 만들어(정규식이 아니면 특수 문자를 이스케이프) 플래그 `u`, 대소문자 무시면 `i`를 단다.
//! 정규식 원문에 줄바꿈·`\n`·`\r`·`\W`가 있으면 줄을 `\n`으로 이은 문서 전체에서 `m` 플래그로 찾고, 아니면 줄마다 따로 찾는다.
//! 여기서는 ECMAScript 정규식 문법과 의미(역참조, 앞·뒤 보기, ASCII `\w`·`\b`, `u`의 대소문자 접기)를 그대로 구현한
//! `regress` crate로 같은 정규식을 만든다. 남은 차이: JS는 UTF-16 단위로 세고 여기서는 UTF-8 바이트로 세므로 위치 값만 다르고
//! 찾는 글자는 같다. `u` 플래그가 늘 켜져 있어 UTF-16 대리 쌍을 반쪽씩 맞추는 JS 동작은 Monaco에서도 일어나지 않는다.

use std::ops::Range;

use regress::{Flags, Regex};

use super::line_diff::TextPoint;
use super::word_boundary::is_word_boundary_character;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub is_case_sensitive: bool,
    pub is_whole_word: bool,
    pub is_regex: bool,
}

/// 찾은 자리 하나. 여러 줄 정규식이면 시작과 끝 줄이 다를 수 있다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindMatch {
    pub start: TextPoint,
    pub end: TextPoint,
    /// `$0`(찾은 글자 전체)부터 묶음 번호 순서의 찾은 글자. 맞지 않은 묶음은 `None`(JS `undefined`)
    pub captures: Vec<Option<String>>,
}

/// 문서 전체에서 찾는다. 정규식이 틀리면 사유를 돌려준다(Monaco는 입력란 테두리를 붉게 한다)
pub fn find_matches(
    lines: &[String],
    query: &str,
    options: FindOptions,
) -> Result<Vec<FindMatch>, String> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let is_multiline = if options.is_regex {
        is_multiline_regex_source(query)
    } else {
        query.contains('\n')
    };
    let regex = build_regex(query, options, is_multiline)?;
    if is_multiline {
        return Ok(find_in_document(&regex, lines, options.is_whole_word));
    }
    let mut matches = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        for (range, captures) in search_text(&regex, line, options.is_whole_word) {
            matches.push(FindMatch {
                start: TextPoint {
                    line: line_index,
                    column: range.start,
                },
                end: TextPoint {
                    line: line_index,
                    column: range.end,
                },
                captures,
            });
        }
    }
    Ok(matches)
}

/// Monaco `createRegExp(searchString, isRegex, { matchCase, multiline, global: true, unicode: true })`
fn build_regex(query: &str, options: FindOptions, is_multiline: bool) -> Result<Regex, String> {
    let source = if options.is_regex {
        query.to_string()
    } else {
        regress::escape(query)
    };
    let flags = Flags {
        icase: !options.is_case_sensitive,
        multiline: is_multiline,
        unicode: true,
        ..Flags::default()
    };
    Regex::with_flags(&source, flags).map_err(|error| error.to_string())
}

/// Monaco `isMultilineRegexSource`: 줄바꿈 글자나 `\n`·`\r`·`\W`가 있으면 줄을 넘어 찾는다
fn is_multiline_regex_source(source: &str) -> bool {
    let mut characters = source.chars();
    while let Some(character) = characters.next() {
        match character {
            '\n' => return true,
            '\\' => match characters.next() {
                Some('n' | 'r' | 'W') => return true,
                Some(_) => {}
                None => return false,
            },
            _ => {}
        }
    }
    false
}

/// 줄을 `\n`으로 이은 글에서 찾고, 바이트 위치를 줄·칸으로 되돌린다(Monaco `_doFindMatchesMultiline`)
fn find_in_document(regex: &Regex, lines: &[String], is_whole_word: bool) -> Vec<FindMatch> {
    let text = lines.join("\n");
    let mut line_starts = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in lines {
        line_starts.push(offset);
        offset += line.len() + 1;
    }
    let point_at = |offset: usize| {
        let line = line_starts.partition_point(|start| *start <= offset) - 1;
        TextPoint {
            line,
            column: offset - line_starts[line],
        }
    };
    search_text(regex, &text, is_whole_word)
        .into_iter()
        .map(|(range, captures)| FindMatch {
            start: point_at(range.start),
            end: point_at(range.end),
            captures,
        })
        .collect()
}

/// 찾은 범위와 묶음 글자
type TextMatch = (Range<usize>, Vec<Option<String>>);

/// Monaco `Searcher.next`를 끝까지 되풀이한다. 빈 결과도 결과로 세고, 같은 빈 결과가 다시 나오면 한 글자 건너뛴다.
/// 직전 결과가 글 끝에서 끝나면 더 찾지 않는다(그래서 `x*`는 글 끝의 빈 결과를 내지 않는다)
fn search_text(regex: &Regex, text: &str, is_whole_word: bool) -> Vec<TextMatch> {
    let mut found = Vec::new();
    let mut last_index = 0;
    let mut previous: Option<Range<usize>> = None;
    loop {
        if previous
            .as_ref()
            .is_some_and(|range| range.end == text.len())
            || last_index > text.len()
        {
            break;
        }
        let Some(candidate) = regex.find_from(text, last_index).next() else {
            break;
        };
        let range = candidate.range();
        if previous.as_ref() == Some(&range) {
            if !range.is_empty() {
                break;
            }
            last_index = next_character_boundary(text, last_index);
            continue;
        }
        last_index = range.end;
        previous = Some(range.clone());
        if !is_whole_word || is_whole_word_match(text, &range) {
            let captures = candidate
                .groups()
                .map(|group| group.map(|group| text[group].to_string()))
                .collect();
            found.push((range, captures));
        }
    }
    found
}

/// JS는 빈 결과 뒤 `lastIndex`를 코드 포인트 하나만큼 옮긴다. 글 끝이면 끝을 넘겨 멈추게 한다
fn next_character_boundary(text: &str, index: usize) -> usize {
    text[index..]
        .chars()
        .next()
        .map_or(index + 1, |character| index + character.len_utf8())
}

/// Monaco 단어 단위(`isValidMatch`): 앞뒤가 글 경계·구분 기호·줄바꿈이거나, 찾은 글자 자체가 구분 기호로 시작·끝나면 인정한다
fn is_whole_word_match(text: &str, range: &Range<usize>) -> bool {
    let before = text[..range.start].chars().next_back();
    let first = text[range.clone()].chars().next();
    let last = text[range.clone()].chars().next_back();
    let after = text[range.end..].chars().next();
    let is_left_ok = before.is_none_or(is_word_boundary_character)
        || first.is_some_and(is_word_boundary_character);
    let is_right_ok = after.is_none_or(is_word_boundary_character)
        || last.is_some_and(is_word_boundary_character);
    is_left_ok && is_right_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.split('\n').map(str::to_string).collect()
    }

    /// (시작 줄, 시작 칸, 끝 줄, 끝 칸)
    fn spans(text: &str, query: &str, options: FindOptions) -> Vec<(usize, usize, usize, usize)> {
        find_matches(&lines(text), query, options)
            .unwrap()
            .into_iter()
            .map(|found| {
                (
                    found.start.line,
                    found.start.column,
                    found.end.line,
                    found.end.column,
                )
            })
            .collect()
    }

    fn texts(text: &str, query: &str, options: FindOptions) -> Vec<String> {
        find_matches(&lines(text), query, options)
            .unwrap()
            .into_iter()
            .map(|found| found.captures[0].clone().unwrap())
            .collect()
    }

    const REGEX: FindOptions = FindOptions {
        is_case_sensitive: false,
        is_whole_word: false,
        is_regex: true,
    };
    const CASE_SENSITIVE_REGEX: FindOptions = FindOptions {
        is_case_sensitive: true,
        ..REGEX
    };

    #[test]
    fn plain_search_ignores_case_unless_asked() {
        let text = "Card card\nCARD";

        assert_eq!(spans(text, "card", FindOptions::default()).len(), 3);
        let case_sensitive = FindOptions {
            is_case_sensitive: true,
            ..FindOptions::default()
        };
        assert_eq!(spans(text, "card", case_sensitive), vec![(0, 5, 0, 9)]);
        assert_eq!(texts("a.b axb", "a.b", FindOptions::default()), ["a.b"]);
    }

    #[test]
    fn whole_word_skips_matches_inside_identifiers() {
        let whole_word = FindOptions {
            is_whole_word: true,
            ..FindOptions::default()
        };

        assert_eq!(spans("cards card", "card", whole_word), vec![(0, 6, 0, 10)]);
    }

    #[test]
    fn regex_supports_classes_groups_and_quantifiers() {
        assert_eq!(
            spans("countOpen countDone", r"count(Open|Done)", REGEX).len(),
            2
        );
        assert_eq!(
            spans("a1 b22 c333", r"\d{2,}", REGEX),
            vec![(0, 4, 0, 6), (0, 8, 0, 11)]
        );
        assert_eq!(spans("  return x;", r"^\s*\w+", REGEX), vec![(0, 0, 0, 8)]);
        assert_eq!(spans("x = [1, 2]", r"[^\w\s]", REGEX).len(), 4);
        assert!(find_matches(&lines("a"), "(a", REGEX).is_err());
    }

    #[test]
    fn line_anchors_match_every_line_including_empty_matches() {
        let text = "alpha\nbeta\n\ngamma";

        // Monaco는 빈 결과도 센다: `^`는 줄마다 하나, `$`도 줄마다 하나
        assert_eq!(spans(text, "^", REGEX).len(), 4);
        assert_eq!(spans(text, "$", REGEX).len(), 4);
        assert_eq!(texts(text, "a$", REGEX), ["a", "a", "a"]);
        // 앞 결과가 줄 끝에서 끝나면 그 줄의 끝 빈 결과는 더 찾지 않는다
        assert_eq!(spans("ax", "x*", REGEX), vec![(0, 0, 0, 0), (0, 1, 0, 2)]);
    }

    #[test]
    fn newline_escapes_switch_to_multiline_search_with_the_m_flag() {
        let text = "const a = 1;\nconst b = 2;\nlet c;";

        assert_eq!(spans(text, r";\nconst", REGEX), vec![(0, 11, 1, 5)]);
        // `m` 플래그: `$`와 `^`가 줄바꿈 앞뒤에서 맞는다
        assert_eq!(spans(text, r"1;$\n^const", REGEX), vec![(0, 10, 1, 5)]);
        // `.`은 줄바꿈을 넘지 않는다(JS `s` 플래그 없음)
        assert_eq!(texts(text, r"b.*\n?", REGEX), ["b = 2;\n"]);
        // 줄마다 찾는 모드에서 `\s`는 줄을 넘지 않는다(Monaco와 같다)
        assert!(spans(text, r"1;\s+const", REGEX).is_empty());
    }

    #[test]
    fn word_boundary_and_word_class_stay_ascii_like_javascript() {
        // JS `\w`·`\b`는 `u` 플래그에서도 ASCII 글자만 단어 글자로 본다
        assert_eq!(texts("café naïve", r"\b\w+\b", REGEX), ["caf", "na", "ve"]);
        assert_eq!(texts("작업 board", r"\bboard\b", REGEX), ["board"]);
        assert!(spans("작업", r"\b작업\b", REGEX).is_empty());
        assert_eq!(texts("ÉCOLE école", "école", REGEX), ["ÉCOLE", "école"]);
        assert_eq!(
            texts("ÉCOLE école", "école", CASE_SENSITIVE_REGEX),
            ["école"]
        );
    }

    #[test]
    fn backreferences_and_named_groups_match_repeated_text() {
        assert_eq!(texts("aa ab bb", r"(\w)\1", REGEX), ["aa", "bb"]);
        assert_eq!(
            texts("<b>x</b> <i>y</b>", r"<(?<tag>\w)>\w</\k<tag>>", REGEX),
            ["<b>x</b>"]
        );
    }

    #[test]
    fn lookahead_and_variable_length_lookbehind_work() {
        assert_eq!(texts("count countOpen", r"count(?=Open)", REGEX), ["count"]);
        assert_eq!(texts("count countOpen", r"count(?!Open)", REGEX), ["count"]);
        assert_eq!(texts("$12 €34 56", r"(?<=\$|€)\d+", REGEX), ["12", "34"]);
        assert_eq!(texts("ab12 b34", r"(?<=a\w+)\d", REGEX), ["1", "2"]);
        assert_eq!(texts("ab12 b34", r"(?<!a)b\d+", REGEX), ["b34"]);
    }

    #[test]
    fn unicode_flag_rejects_identity_escapes_like_monaco() {
        // `u` 플래그에서는 뜻 없는 이스케이프와 홀로 선 `{`가 문법 오류다
        assert!(find_matches(&lines("a"), r"\a", REGEX).is_err());
        assert!(find_matches(&lines("a{"), r"a{", REGEX).is_err());
        assert_eq!(texts("😀x", r"\u{1F600}", REGEX), ["😀"]);
        assert_eq!(texts("😀x", r"^.", REGEX), ["😀"]);
    }

    #[test]
    fn captures_keep_unmatched_groups_as_none() {
        let found = find_matches(&lines("ab"), "(a)(x)?(b)", REGEX).unwrap();

        assert_eq!(
            found[0].captures,
            vec![
                Some("ab".to_string()),
                Some("a".to_string()),
                None,
                Some("b".to_string())
            ]
        );
    }
}
