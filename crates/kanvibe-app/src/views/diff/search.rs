//! 찾기 위젯의 검색(Monaco `FindModel`). 대소문자 구분·단어 단위·정규식 세 토글을 따르고 결과는 줄별 바이트 범위다.
//! 정규식은 앱이 정규식 crate에 의존하지 않아 이 화면이 쓰는 JS 정규식 부분집합(글자·`.`·문자 집합·`\d\w\s\b`·`^$`·
//! 묶음·`|`·`* + ? {m,n}` 반복)을 줄 단위로 되추적해 맞춘다. 줄을 넘는 패턴(`\n`)은 지원하지 않는다.

use std::ops::Range;

use super::word_boundary::is_word_boundary_character;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub is_case_sensitive: bool,
    pub is_whole_word: bool,
    pub is_regex: bool,
}

/// 찾은 자리 하나
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindMatch {
    pub line: usize,
    pub range: Range<usize>,
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
    let pattern = if options.is_regex {
        parse_pattern(query)?
    } else {
        Node::Sequence(query.chars().map(Node::Literal).collect())
    };
    let mut matches = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        let characters: Vec<(usize, char)> = line.char_indices().collect();
        let matcher = Matcher {
            characters: &characters,
            is_case_sensitive: options.is_case_sensitive,
            line_len: line.len(),
        };
        for range in matcher.find_all(&pattern) {
            if !options.is_whole_word || is_whole_word(line, &range) {
                matches.push(FindMatch {
                    line: line_index,
                    range,
                });
            }
        }
    }
    Ok(matches)
}

/// Monaco 단어 단위: 앞뒤가 줄 경계·구분 기호이거나, 찾은 글자 자체가 구분 기호로 시작·끝나면 인정한다
fn is_whole_word(line: &str, range: &Range<usize>) -> bool {
    let before = line[..range.start].chars().next_back();
    let first = line[range.clone()].chars().next();
    let last = line[range.clone()].chars().next_back();
    let after = line[range.end..].chars().next();
    let is_left_ok = before.is_none_or(is_word_boundary_character)
        || first.is_some_and(is_word_boundary_character);
    let is_right_ok = after.is_none_or(is_word_boundary_character)
        || last.is_some_and(is_word_boundary_character);
    is_left_ok && is_right_ok
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Literal(char),
    AnyCharacter,
    Class {
        ranges: Vec<(char, char)>,
        is_negated: bool,
    },
    LineStart,
    LineEnd,
    WordBoundary {
        is_negated: bool,
    },
    Sequence(Vec<Node>),
    Alternation(Vec<Node>),
    Repeat {
        node: Box<Node>,
        min: usize,
        max: Option<usize>,
    },
}

fn parse_pattern(source: &str) -> Result<Node, String> {
    let characters: Vec<char> = source.chars().collect();
    let mut parser = Parser {
        characters,
        position: 0,
    };
    let node = parser.parse_alternation()?;
    if parser.position < parser.characters.len() {
        return Err(format!("Unmatched ')' at {}", parser.position));
    }
    Ok(node)
}

struct Parser {
    characters: Vec<char>,
    position: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.characters.get(self.position).copied()
    }

    fn next(&mut self) -> Option<char> {
        let character = self.peek();
        self.position += 1;
        character
    }

    fn parse_alternation(&mut self) -> Result<Node, String> {
        let mut branches = vec![self.parse_sequence()?];
        while self.peek() == Some('|') {
            self.position += 1;
            branches.push(self.parse_sequence()?);
        }
        Ok(if branches.len() == 1 {
            branches.remove(0)
        } else {
            Node::Alternation(branches)
        })
    }

    fn parse_sequence(&mut self) -> Result<Node, String> {
        let mut items = Vec::new();
        while let Some(character) = self.peek() {
            if character == '|' || character == ')' {
                break;
            }
            let atom = self.parse_atom()?;
            items.push(self.parse_quantifier(atom)?);
        }
        Ok(Node::Sequence(items))
    }

    fn parse_atom(&mut self) -> Result<Node, String> {
        match self.next() {
            Some('(') => {
                if self.characters[self.position..].starts_with(&['?', ':']) {
                    self.position += 2;
                }
                let inner = self.parse_alternation()?;
                if self.next() != Some(')') {
                    return Err("Unterminated group".to_string());
                }
                Ok(inner)
            }
            Some('[') => self.parse_class(),
            Some('.') => Ok(Node::AnyCharacter),
            Some('^') => Ok(Node::LineStart),
            Some('$') => Ok(Node::LineEnd),
            Some('\\') => self.parse_escape(),
            Some('*' | '+' | '?') => Err("Nothing to repeat".to_string()),
            Some(character) => Ok(Node::Literal(character)),
            None => Err("Unexpected end of pattern".to_string()),
        }
    }

    fn parse_escape(&mut self) -> Result<Node, String> {
        let class = |ranges: &[(char, char)], is_negated: bool| Node::Class {
            ranges: ranges.to_vec(),
            is_negated,
        };
        Ok(match self.next().ok_or("\\ at end of pattern")? {
            'd' => class(&DIGIT, false),
            'D' => class(&DIGIT, true),
            'w' => class(&WORD, false),
            'W' => class(&WORD, true),
            's' => class(&SPACE, false),
            'S' => class(&SPACE, true),
            'b' => Node::WordBoundary { is_negated: false },
            'B' => Node::WordBoundary { is_negated: true },
            't' => Node::Literal('\t'),
            other => Node::Literal(other),
        })
    }

    fn parse_class(&mut self) -> Result<Node, String> {
        let is_negated = self.peek() == Some('^');
        if is_negated {
            self.position += 1;
        }
        let mut ranges = Vec::new();
        loop {
            let start = match self.next().ok_or("Unterminated character class")? {
                ']' if !ranges.is_empty() => break,
                '\\' => self.class_escape(&mut ranges)?,
                character => character,
            };
            let is_range = self.peek() == Some('-')
                && self
                    .characters
                    .get(self.position + 1)
                    .is_some_and(|next| *next != ']');
            if is_range {
                self.position += 1;
                let end = self.next().ok_or("Unterminated character class")?;
                ranges.push((start, end));
            } else if start != '\0' {
                ranges.push((start, start));
            }
        }
        Ok(Node::Class { ranges, is_negated })
    }

    /// 문자 집합 안의 `\d` 같은 묶음은 범위에 바로 넣고 `\0`을 돌려 하나짜리 글자로 넣지 않게 한다
    fn class_escape(&mut self, ranges: &mut Vec<(char, char)>) -> Result<char, String> {
        let escaped = self.next().ok_or("\\ at end of pattern")?;
        let group: &[(char, char)] = match escaped {
            'd' => &DIGIT,
            'w' => &WORD,
            's' => &SPACE,
            't' => return Ok('\t'),
            other => return Ok(other),
        };
        ranges.extend_from_slice(group);
        Ok('\0')
    }

    fn parse_quantifier(&mut self, atom: Node) -> Result<Node, String> {
        let (min, max) = match self.peek() {
            Some('*') => (0, None),
            Some('+') => (1, None),
            Some('?') => (0, Some(1)),
            Some('{') => match self.parse_braces() {
                Some(bounds) => bounds,
                None => return Ok(atom),
            },
            _ => return Ok(atom),
        };
        if matches!(self.peek(), Some('*' | '+' | '?')) {
            self.position += 1;
        }
        // 게으른 반복(`*?`)은 찾는 범위만 달라지므로 같은 탐욕 반복으로 다룬다
        if self.peek() == Some('?') {
            self.position += 1;
        }
        Ok(Node::Repeat {
            node: Box::new(atom),
            min,
            max,
        })
    }

    /// `{m}`, `{m,}`, `{m,n}`. 숫자가 아니면 글자 `{`로 둔다(JS와 같다)
    fn parse_braces(&mut self) -> Option<(usize, Option<usize>)> {
        let rest: String = self.characters[self.position + 1..].iter().collect();
        let close = rest.find('}')?;
        let body = &rest[..close];
        let (min, max) = match body.split_once(',') {
            Some((min, "")) => (min.parse().ok()?, None),
            Some((min, max)) => (min.parse().ok()?, Some(max.parse().ok()?)),
            None => (body.parse().ok()?, Some(body.parse().ok()?)),
        };
        self.position += close + 2;
        Some((min, max))
    }
}

const DIGIT: [(char, char); 1] = [('0', '9')];
const WORD: [(char, char); 4] = [('a', 'z'), ('A', 'Z'), ('0', '9'), ('_', '_')];
const SPACE: [(char, char); 4] = [
    (' ', ' '),
    ('\t', '\t'),
    ('\u{a0}', '\u{a0}'),
    ('\u{3000}', '\u{3000}'),
];

struct Matcher<'a> {
    characters: &'a [(usize, char)],
    is_case_sensitive: bool,
    line_len: usize,
}

impl Matcher<'_> {
    fn byte_offset(&self, index: usize) -> usize {
        self.characters
            .get(index)
            .map_or(self.line_len, |(offset, _)| *offset)
    }

    /// 앞에서부터 겹치지 않게 찾는다. 빈 일치는 건너뛴다
    fn find_all(&self, pattern: &Node) -> Vec<Range<usize>> {
        let mut found = Vec::new();
        let mut start = 0;
        while start <= self.characters.len() {
            let mut end = None;
            self.match_node(pattern, start, &mut |position| {
                end = Some(position);
                true
            });
            match end {
                Some(end) if end > start => {
                    found.push(self.byte_offset(start)..self.byte_offset(end));
                    start = end;
                }
                _ => start += 1,
            }
        }
        found
    }

    fn equals(&self, left: char, right: char) -> bool {
        left == right || (!self.is_case_sensitive && left.to_lowercase().eq(right.to_lowercase()))
    }

    fn is_word_character(&self, index: usize) -> bool {
        self.characters
            .get(index)
            .is_some_and(|(_, character)| character.is_alphanumeric() || *character == '_')
    }

    fn in_class(&self, character: char, ranges: &[(char, char)]) -> bool {
        let variants = [
            character,
            character.to_lowercase().next().unwrap_or(character),
            character.to_uppercase().next().unwrap_or(character),
        ];
        let candidates = if self.is_case_sensitive {
            &variants[..1]
        } else {
            &variants[..]
        };
        candidates.iter().any(|candidate| {
            ranges
                .iter()
                .any(|(low, high)| (*low..=*high).contains(candidate))
        })
    }

    /// `position`에서 `node`를 맞추고, 맞으면 끝 위치로 `next`를 부른다. `next`가 참이면 거기서 멈춘다
    fn match_node(
        &self,
        node: &Node,
        position: usize,
        next: &mut dyn FnMut(usize) -> bool,
    ) -> bool {
        match node {
            Node::Sequence(items) => self.match_sequence(items, position, next),
            Node::Alternation(branches) => branches
                .iter()
                .any(|branch| self.match_node(branch, position, next)),
            Node::Repeat { node, min, max } => {
                self.match_repeat(node, *min, *max, 0, position, next)
            }
            _ => self.match_single(node, position).is_some_and(next),
        }
    }

    fn match_single(&self, node: &Node, position: usize) -> Option<usize> {
        let current = self
            .characters
            .get(position)
            .map(|(_, character)| *character);
        let is_match = match node {
            Node::Literal(expected) => {
                current.is_some_and(|character| self.equals(character, *expected))
            }
            Node::AnyCharacter => current.is_some(),
            Node::Class { ranges, is_negated } => {
                current.is_some_and(|character| self.in_class(character, ranges) != *is_negated)
            }
            Node::LineStart => return (position == 0).then_some(position),
            Node::LineEnd => return (position == self.characters.len()).then_some(position),
            Node::WordBoundary { is_negated } => {
                let is_boundary = position
                    .checked_sub(1)
                    .is_some_and(|before| self.is_word_character(before))
                    != self.is_word_character(position);
                return (is_boundary != *is_negated).then_some(position);
            }
            _ => false,
        };
        is_match.then_some(position + 1)
    }

    fn match_sequence(
        &self,
        items: &[Node],
        position: usize,
        next: &mut dyn FnMut(usize) -> bool,
    ) -> bool {
        let Some((first, rest)) = items.split_first() else {
            return next(position);
        };
        self.match_node(first, position, &mut |end| {
            self.match_sequence(rest, end, next)
        })
    }

    /// 탐욕 반복: 더 많이 맞추는 쪽을 먼저 해 보고 안 되면 줄인다. 빈 반복으로 제자리를 돌지 않게 한다
    fn match_repeat(
        &self,
        node: &Node,
        min: usize,
        max: Option<usize>,
        count: usize,
        position: usize,
        next: &mut dyn FnMut(usize) -> bool,
    ) -> bool {
        let can_repeat = max.is_none_or(|max| count < max);
        if can_repeat {
            let mut try_more = |end: usize| {
                end != position && self.match_repeat(node, min, max, count + 1, end, next)
            };
            if self.match_node(node, position, &mut try_more) {
                return true;
            }
        }
        count >= min && next(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.split('\n').map(str::to_string).collect()
    }

    fn ranges(text: &str, query: &str, options: FindOptions) -> Vec<(usize, Range<usize>)> {
        find_matches(&lines(text), query, options)
            .unwrap()
            .into_iter()
            .map(|found| (found.line, found.range))
            .collect()
    }

    #[test]
    fn plain_search_ignores_case_unless_asked() {
        let text = "Card card\nCARD";

        assert_eq!(ranges(text, "card", FindOptions::default()).len(), 3);
        let case_sensitive = FindOptions {
            is_case_sensitive: true,
            ..FindOptions::default()
        };
        assert_eq!(ranges(text, "card", case_sensitive), vec![(0, 5..9)]);
    }

    #[test]
    fn whole_word_skips_matches_inside_identifiers() {
        let whole_word = FindOptions {
            is_whole_word: true,
            ..FindOptions::default()
        };

        assert_eq!(ranges("cards card", "card", whole_word), vec![(0, 6..10)]);
    }

    #[test]
    fn regex_supports_classes_groups_and_quantifiers() {
        let regex = FindOptions {
            is_regex: true,
            ..FindOptions::default()
        };

        assert_eq!(
            ranges("countOpen countDone", r"count(Open|Done)", regex).len(),
            2
        );
        assert_eq!(
            ranges("a1 b22 c333", r"\d{2,}", regex),
            vec![(0, 4..6), (0, 8..11)]
        );
        assert_eq!(ranges("  return x;", r"^\s*\w+", regex), vec![(0, 0..8)]);
        assert_eq!(ranges("x = [1, 2]", r"[^\w\s]", regex).len(), 4);
        assert!(find_matches(&lines("a"), "(a", regex).is_err());
    }
}
