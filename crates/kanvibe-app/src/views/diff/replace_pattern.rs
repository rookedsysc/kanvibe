//! 바꿀 글자 해석(Monaco `replacePattern.ts`의 `parseReplaceString`·`ReplacePattern`)과 대소문자 보존
//! (`buildReplaceStringWithCasePreserved`). 정규식 모드에서만 `$&`·`$0`·`$1`~`$99`·`$$`와 `\n`·`\t`·`\\`,
//! 다음 묶음 글자의 대소문자를 바꾸는 `\u`·`\U`·`\l`·`\L`을 해석한다. 정규식이 아니면 바꿀 글자를 그대로 쓴다.
//! 대소문자 보존은 바꿀 글자에 묶음 참조가 없을 때만 적용된다(Monaco와 같다).

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplacePiece {
    Static(String),
    /// 묶음 번호와, 그 묶음 글자의 앞에서부터 적용할 대소문자 연산(`u`·`U`·`l`·`L`)
    Capture {
        index: usize,
        case_operations: Vec<char>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplacePattern {
    Static(String),
    Dynamic(Vec<ReplacePiece>),
}

impl ReplacePattern {
    /// 위젯의 바꿀 글자와 정규식 토글로 만든다(Monaco `FindModel._getReplacePattern`)
    pub fn new(replacement: &str, is_regex: bool) -> Self {
        if is_regex {
            parse_replace_string(replacement)
        } else {
            Self::Static(replacement.to_string())
        }
    }

    fn from_pieces(pieces: Vec<ReplacePiece>) -> Self {
        match pieces.as_slice() {
            [] => Self::Static(String::new()),
            [ReplacePiece::Static(value)] => Self::Static(value.clone()),
            _ => Self::Dynamic(pieces),
        }
    }

    /// 결과 하나를 바꿀 글자를 만든다. `captures[0]`은 찾은 글자 전체다
    pub fn build(&self, captures: &[Option<String>], is_preserving_case: bool) -> String {
        let pieces = match self {
            Self::Static(value) if is_preserving_case => {
                return build_with_case_preserved(captures.first().cloned().flatten(), value);
            }
            Self::Static(value) => return value.clone(),
            Self::Dynamic(pieces) => pieces,
        };
        pieces
            .iter()
            .map(|piece| match piece {
                ReplacePiece::Static(value) => value.clone(),
                ReplacePiece::Capture {
                    index,
                    case_operations,
                } => apply_case_operations(&substitute(*index, captures), case_operations),
            })
            .collect()
    }
}

/// Monaco `ReplacePattern._substitute`: 없는 두 자리 묶음 번호는 앞자리 묶음 + 남은 숫자로, 그것도 없으면 `$n` 그대로다
fn substitute(index: usize, captures: &[Option<String>]) -> String {
    if index == 0 {
        return captures.first().cloned().flatten().unwrap_or_default();
    }
    let mut remainder = String::new();
    let mut index = index;
    while index > 0 {
        if index < captures.len() {
            let captured = captures[index].clone().unwrap_or_default();
            return captured + &remainder;
        }
        remainder = format!("{}{remainder}", index % 10);
        index /= 10;
    }
    format!("${remainder}")
}

/// `u`·`l`은 글자 하나만, `U`·`L`은 남은 글자 모두를 바꾼다. 연산이 끝나면 나머지는 그대로 붙인다
fn apply_case_operations(captured: &str, operations: &[char]) -> String {
    if operations.is_empty() {
        return captured.to_string();
    }
    let mut result = String::new();
    let mut operation_index = 0;
    for (byte_index, character) in captured.char_indices() {
        let Some(operation) = operations.get(operation_index) else {
            result.push_str(&captured[byte_index..]);
            break;
        };
        match operation {
            'U' => result.extend(character.to_uppercase()),
            'u' => {
                result.extend(character.to_uppercase());
                operation_index += 1;
            }
            'L' => result.extend(character.to_lowercase()),
            _ => {
                result.extend(character.to_lowercase());
                operation_index += 1;
            }
        }
    }
    result
}

/// Monaco `ReplacePieceBuilder`: 원문에서 아직 내보내지 않은 구간을 정적 글자로 모으다가 묶음 참조를 만나면 끊는다
struct PieceBuilder<'a> {
    source: &'a [char],
    last_index: usize,
    pieces: Vec<ReplacePiece>,
    current_static: String,
}

impl PieceBuilder<'_> {
    fn emit_unchanged(&mut self, to_index: usize) {
        let unchanged: String = self.source[self.last_index..to_index].iter().collect();
        self.current_static.push_str(&unchanged);
        self.last_index = to_index;
    }

    fn emit_static(&mut self, value: &str, to_index: usize) {
        self.current_static.push_str(value);
        self.last_index = to_index;
    }

    fn emit_capture(&mut self, index: usize, to_index: usize, case_operations: &mut Vec<char>) {
        if !self.current_static.is_empty() {
            let value = std::mem::take(&mut self.current_static);
            self.pieces.push(ReplacePiece::Static(value));
        }
        self.pieces.push(ReplacePiece::Capture {
            index,
            case_operations: std::mem::take(case_operations),
        });
        self.last_index = to_index;
    }

    fn finish(mut self) -> ReplacePattern {
        self.emit_unchanged(self.source.len());
        if !self.current_static.is_empty() {
            self.pieces.push(ReplacePiece::Static(self.current_static));
        }
        ReplacePattern::from_pieces(self.pieces)
    }
}

/// Monaco `parseReplaceString`. 인식하지 못한 `\x`·`$x`는 그대로 남는다
fn parse_replace_string(replacement: &str) -> ReplacePattern {
    let source: Vec<char> = replacement.chars().collect();
    let mut builder = PieceBuilder {
        source: &source,
        last_index: 0,
        pieces: Vec::new(),
        current_static: String::new(),
    };
    let mut case_operations = Vec::new();
    let mut index = 0;
    while index < source.len() {
        let skipped = match source[index] {
            '\\' => parse_backslash(&mut builder, index, &mut case_operations),
            '$' => parse_dollar(&mut builder, index, &mut case_operations),
            _ => Some(0),
        };
        let Some(skipped) = skipped else {
            break;
        };
        index += 1 + skipped;
    }
    builder.finish()
}

/// `\` 다음 글자를 해석하고 건너뛴 글자 수를 돌려준다. 원문이 `\`로 끝나면 `None`
fn parse_backslash(
    builder: &mut PieceBuilder,
    index: usize,
    case_operations: &mut Vec<char>,
) -> Option<usize> {
    let next = *builder.source.get(index + 1)?;
    let escaped = match next {
        '\\' => Some("\\"),
        'n' => Some("\n"),
        't' => Some("\t"),
        'u' | 'U' | 'l' | 'L' => {
            case_operations.push(next);
            Some("")
        }
        _ => None,
    };
    if let Some(escaped) = escaped {
        builder.emit_unchanged(index);
        builder.emit_static(escaped, index + 2);
    }
    Some(1)
}

/// `$` 다음 글자를 해석하고 건너뛴 글자 수를 돌려준다. 원문이 `$`로 끝나면 `None`
fn parse_dollar(
    builder: &mut PieceBuilder,
    index: usize,
    case_operations: &mut Vec<char>,
) -> Option<usize> {
    let next = *builder.source.get(index + 1)?;
    match next {
        '$' => {
            builder.emit_unchanged(index);
            builder.emit_static("$", index + 2);
        }
        '0' | '&' => {
            builder.emit_unchanged(index);
            builder.emit_capture(0, index + 2, case_operations);
        }
        '1'..='9' => {
            let first = next as usize - '0' as usize;
            builder.emit_unchanged(index);
            if let Some(second) = builder.source.get(index + 2).and_then(|c| c.to_digit(10)) {
                builder.emit_capture(first * 10 + second as usize, index + 3, case_operations);
                return Some(2);
            }
            builder.emit_capture(first, index + 2, case_operations);
        }
        _ => {}
    }
    Some(1)
}

/// Monaco `buildReplaceStringWithCasePreserved`: 찾은 글자가 모두 대문자·소문자·첫 글자 대문자인지에 맞춰 바꿀 글자를 고친다.
/// `-`나 `_`로 나뉜 조각 수가 같으면 조각마다 따로 맞춘다
fn build_with_case_preserved(matched: Option<String>, pattern: &str) -> String {
    let Some(matched) = matched.filter(|matched| !matched.is_empty()) else {
        return pattern.to_string();
    };
    let has_hyphens = has_same_separated_parts(&matched, pattern, '-');
    let has_underscores = has_same_separated_parts(&matched, pattern, '_');
    if has_hyphens != has_underscores {
        let separator = if has_hyphens { '-' } else { '_' };
        let parts: Vec<String> = pattern
            .split(separator)
            .zip(matched.split(separator))
            .map(|(pattern_part, matched_part)| {
                build_with_case_preserved(Some(matched_part.to_string()), pattern_part)
            })
            .collect();
        return parts.join(&separator.to_string());
    }
    if matched.to_uppercase() == matched {
        return pattern.to_uppercase();
    }
    if matched.to_lowercase() == matched {
        return pattern.to_lowercase();
    }
    let first = matched.chars().next().unwrap_or_default().to_string();
    let mut pattern_characters = pattern.chars();
    let Some(pattern_first) = pattern_characters.next() else {
        return pattern.to_string();
    };
    let rest = pattern_characters.as_str();
    if first.to_lowercase() != first {
        format!("{}{rest}", pattern_first.to_uppercase())
    } else if first.to_uppercase() != first {
        format!("{}{rest}", pattern_first.to_lowercase())
    } else {
        pattern.to_string()
    }
}

fn has_same_separated_parts(matched: &str, pattern: &str, separator: char) -> bool {
    matched.contains(separator)
        && pattern.contains(separator)
        && matched.split(separator).count() == pattern.split(separator).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captures(values: &[Option<&str>]) -> Vec<Option<String>> {
        values
            .iter()
            .map(|value| value.map(str::to_string))
            .collect()
    }

    fn regex_replace(replacement: &str, values: &[Option<&str>]) -> String {
        ReplacePattern::new(replacement, true).build(&captures(values), false)
    }

    #[test]
    fn dollar_references_follow_monaco_substitution() {
        let found = [Some("foo-bar"), Some("foo"), Some("bar")];

        assert_eq!(regex_replace("$2_$1", &found), "bar_foo");
        assert_eq!(regex_replace("[$&]", &found), "[foo-bar]");
        assert_eq!(regex_replace("[$0]", &found), "[foo-bar]");
        assert_eq!(regex_replace("$$1", &found), "$1");
        // 없는 두 자리 번호는 앞자리 묶음 + 남은 숫자, 그것도 없으면 그대로
        assert_eq!(regex_replace("$12", &found), "foo2");
        assert_eq!(regex_replace("$5", &found), "$5");
        assert_eq!(regex_replace("a$", &found), "a$");
        assert_eq!(regex_replace("$x", &found), "$x");
    }

    #[test]
    fn unmatched_groups_become_empty() {
        assert_eq!(regex_replace("<$1>", &[Some("b"), None]), "<>");
    }

    #[test]
    fn backslash_escapes_and_case_operations_apply_in_regex_mode() {
        let found = [Some("hello world"), Some("hello"), Some("world")];

        assert_eq!(regex_replace(r"$1\n\t$2\\", &found), "hello\n\tworld\\");
        assert_eq!(regex_replace(r"\u$1 \U$2", &found), "Hello WORLD");
        assert_eq!(regex_replace(r"\l\U$1", &found), "hELLO");
        // `\U`는 연산 자리를 넘기지 않아 뒤의 `\l`은 쓰이지 않는다
        assert_eq!(regex_replace(r"\U\l$1", &found), "HELLO");
        assert_eq!(regex_replace(r"\x$1", &found), r"\xhello");
        // 정규식이 아니면 모두 글자 그대로다
        let literal = ReplacePattern::new(r"$1\n", false).build(&captures(&found), false);
        assert_eq!(literal, r"$1\n");
    }

    #[test]
    fn preserve_case_matches_the_found_text_shape() {
        let preserve = |matched: &str, pattern: &str| {
            ReplacePattern::new(pattern, false).build(&captures(&[Some(matched)]), true)
        };

        assert_eq!(preserve("FOO", "bar"), "BAR");
        assert_eq!(preserve("foo", "BAR"), "bar");
        assert_eq!(preserve("Foo", "bar"), "Bar");
        assert_eq!(preserve("fOO", "Bar"), "bar");
        assert_eq!(preserve("Foo-Bar", "baz-qux"), "Baz-Qux");
        assert_eq!(preserve("FOO_bar", "baz_qux"), "BAZ_qux");
        assert_eq!(preserve("", "bar"), "bar");
        // 묶음 참조가 있으면 대소문자 보존을 건너뛴다(Monaco와 같다)
        let dynamic = ReplacePattern::new("x$0", true).build(&captures(&[Some("FOO")]), true);
        assert_eq!(dynamic, "xFOO");
    }
}
