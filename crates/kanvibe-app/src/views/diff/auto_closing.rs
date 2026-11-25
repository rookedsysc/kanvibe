//! 괄호·따옴표 자동 닫기와 감싸기(Monaco 기본값 `autoClosingBrackets`·`autoClosingQuotes`·`autoSurround`: `languageDefined`,
//! `autoClosingOvertype`·`autoClosingDelete`: `auto`). 쌍은 Monaco TypeScript 언어 설정을 따른다.
//! `/**` → ` */` 쌍은 문서 주석 템플릿이라 옮기지 않았다.

use super::syntax::Language;
use super::word_boundary::is_word_boundary_character;

/// 다음 글자가 이 중 하나이거나 줄 끝일 때만 닫는다(Monaco 기본 `autoCloseBefore`)
const AUTO_CLOSE_BEFORE: &str = ";:.,=}])> \t";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClosingPair {
    open: char,
    close: char,
    /// 문자열·주석 안에서는 닫지 않는다
    is_skipped_in_string: bool,
    is_skipped_in_comment: bool,
}

const fn pair(
    open: char,
    close: char,
    is_skipped_in_string: bool,
    is_skipped_in_comment: bool,
) -> ClosingPair {
    ClosingPair {
        open,
        close,
        is_skipped_in_string,
        is_skipped_in_comment,
    }
}

const BRACKETS: [ClosingPair; 3] = [
    pair('{', '}', false, false),
    pair('[', ']', false, false),
    pair('(', ')', false, false),
];
const TYPESCRIPT_QUOTES: [ClosingPair; 3] = [
    pair('"', '"', true, false),
    pair('\'', '\'', true, true),
    pair('`', '`', true, true),
];

fn closing_pairs(language: Language) -> Vec<ClosingPair> {
    let quotes: &[char] = match language {
        Language::PlainText => return Vec::new(),
        Language::TypeScript | Language::JavaScript => {
            return BRACKETS
                .iter()
                .chain(TYPESCRIPT_QUOTES.iter())
                .copied()
                .collect();
        }
        Language::Json | Language::Rust => &['"'],
        Language::Go => &['"', '`'],
        Language::Python | Language::Shell | Language::CLike => &['"', '\''],
    };
    let quote_pairs = quotes.iter().map(|quote| pair(*quote, *quote, true, true));
    BRACKETS.iter().copied().chain(quote_pairs).collect()
}

/// 커서 자리가 문자열·주석 안인지
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenContext {
    pub is_in_string: bool,
    pub is_in_comment: bool,
}

/// 글자 하나를 쳤을 때 할 일
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypingAction {
    Insert,
    /// 여는 글자와 닫는 글자를 함께 넣고 커서를 사이에 둔다
    InsertPair {
        close: char,
    },
    /// 자동으로 넣은 닫는 글자 위를 지나간다
    Overtype,
    /// 선택을 감싼다
    Surround {
        close: char,
    },
}

pub struct TypingContext<'a> {
    pub line: &'a str,
    pub column: usize,
    pub has_selection: bool,
    pub tokens: TokenContext,
    /// 바로 뒤 글자를 편집기가 자동으로 넣었는지
    pub is_next_auto_closed: bool,
}

pub fn decide_typing(language: Language, typed: char, context: &TypingContext) -> TypingAction {
    let pairs = closing_pairs(language);
    let next = context.line[context.column..].chars().next();
    if !context.has_selection
        && context.is_next_auto_closed
        && next == Some(typed)
        && pairs.iter().any(|pair| pair.close == typed)
    {
        return TypingAction::Overtype;
    }
    let Some(pair) = pairs.iter().find(|pair| pair.open == typed) else {
        return TypingAction::Insert;
    };
    if context.has_selection {
        return TypingAction::Surround { close: pair.close };
    }
    let is_blocked_by_token = (pair.is_skipped_in_string && context.tokens.is_in_string)
        || (pair.is_skipped_in_comment && context.tokens.is_in_comment);
    let is_next_allowed = next.is_none_or(|next| AUTO_CLOSE_BEFORE.contains(next));
    let previous = context.line[..context.column].chars().next_back();
    let is_quote = pair.open == pair.close;
    // 따옴표는 단어 글자 바로 뒤에서 닫지 않는다(`don't`)
    let is_after_word =
        is_quote && previous.is_some_and(|previous| !is_word_boundary_character(previous));
    if is_blocked_by_token || !is_next_allowed || is_after_word {
        return TypingAction::Insert;
    }
    TypingAction::InsertPair { close: pair.close }
}

/// Backspace가 자동으로 넣은 빈 쌍(`(|)`)을 함께 지워야 하는지
pub fn is_deleting_auto_closed_pair(
    language: Language,
    line: &str,
    column: usize,
    is_next_auto_closed: bool,
) -> bool {
    let (Some(previous), Some(next)) = (
        line[..column].chars().next_back(),
        line[column..].chars().next(),
    ) else {
        return false;
    };
    is_next_auto_closed
        && closing_pairs(language)
            .iter()
            .any(|pair| pair.open == previous && pair.close == next)
}

/// Enter를 눌렀을 때 바로 앞이 여는 괄호인지, 바로 뒤가 그 닫는 괄호인지(Monaco `onEnterRules`의 괄호 들여쓰기)
pub fn bracket_around(line: &str, column: usize) -> (bool, bool) {
    let previous = line[..column].trim_end().chars().next_back();
    let next = line[column..].trim_start().chars().next();
    let open = BRACKETS.iter().find(|pair| Some(pair.open) == previous);
    (
        open.is_some(),
        open.is_some_and(|pair| Some(pair.close) == next),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(line: &str, column: usize) -> TypingContext<'_> {
        TypingContext {
            line,
            column,
            has_selection: false,
            tokens: TokenContext::default(),
            is_next_auto_closed: false,
        }
    }

    #[test]
    fn brackets_close_before_whitespace_or_closers_only() {
        assert_eq!(
            decide_typing(Language::TypeScript, '(', &context("f", 1)),
            TypingAction::InsertPair { close: ')' }
        );
        assert_eq!(
            decide_typing(Language::TypeScript, '(', &context("fx", 1)),
            TypingAction::Insert
        );
        assert_eq!(
            decide_typing(Language::PlainText, '(', &context("", 0)),
            TypingAction::Insert
        );
    }

    #[test]
    fn quotes_skip_strings_and_word_characters() {
        let in_string = TypingContext {
            tokens: TokenContext {
                is_in_string: true,
                is_in_comment: false,
            },
            ..context("", 0)
        };

        assert_eq!(
            decide_typing(Language::TypeScript, '"', &in_string),
            TypingAction::Insert
        );
        assert_eq!(
            decide_typing(Language::TypeScript, '\'', &context("don", 3)),
            TypingAction::Insert
        );
        assert_eq!(
            decide_typing(Language::TypeScript, '\'', &context("x = ", 4)),
            TypingAction::InsertPair { close: '\'' }
        );
    }

    #[test]
    fn auto_closed_closers_are_typed_over_and_selections_are_surrounded() {
        let auto_closed = TypingContext {
            is_next_auto_closed: true,
            ..context("f()", 2)
        };
        let selected = TypingContext {
            has_selection: true,
            ..context("abc", 0)
        };

        assert_eq!(
            decide_typing(Language::TypeScript, ')', &auto_closed),
            TypingAction::Overtype
        );
        assert_eq!(
            decide_typing(Language::TypeScript, ')', &context("f()", 2)),
            TypingAction::Insert
        );
        assert_eq!(
            decide_typing(Language::TypeScript, '[', &selected),
            TypingAction::Surround { close: ']' }
        );
        assert!(is_deleting_auto_closed_pair(
            Language::TypeScript,
            "f()",
            2,
            true
        ));
    }
}
