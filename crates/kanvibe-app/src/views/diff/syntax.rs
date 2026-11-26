//! Monaco 라이트 테마(`vs`)로 칠하는 구문 강조. TS 앱은 Monaco Monarch 문법과 괄호 쌍 색칠(bracket pair colorization)을 썼다.
//! 확장자 → 언어 표(`detectLanguage`)는 TS와 같게 두고, 문법은 기준 이미지에 찍힌 TypeScript 계열을 Monarch 규칙대로 옮겼다.
//! 다른 언어는 같은 어휘 분석기에 주석·문자열·키워드 표만 바꿔 근사한다.

use std::ops::Range;

use gpui_kit::{Rgba, rgb};

/// TS `detectLanguage`가 돌려주던 Monaco 언어 이름
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    TypeScript,
    JavaScript,
    Json,
    Python,
    Rust,
    Go,
    Shell,
    CLike,
    PlainText,
}

/// 파일 확장자로 언어를 고른다. 표에 없는 확장자는 일반 텍스트다(TS `detectLanguage`의 `languageMap`)
pub fn detect_language(file_path: &str) -> Language {
    let extension = file_path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_lowercase();
    match extension.as_str() {
        "ts" | "tsx" => Language::TypeScript,
        "js" | "jsx" => Language::JavaScript,
        "json" => Language::Json,
        "py" => Language::Python,
        "rs" => Language::Rust,
        "go" => Language::Go,
        "sh" | "bash" | "zsh" => Language::Shell,
        "java" | "kt" | "swift" | "c" | "cpp" | "h" | "hpp" | "cs" | "php" | "dart" | "scss"
        | "less" | "css" => Language::CLike,
        _ => Language::PlainText,
    }
}

/// Monarch 토큰 종류. 색은 `vs` 테마 규칙에서 온다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Plain,
    Keyword,
    TypeIdentifier,
    String,
    Number,
    Comment,
    /// 괄호 쌍 색칠의 중첩 깊이
    Bracket(usize),
    UnexpectedBracket,
}

/// `vs` 테마 전경색과 `editorBracketHighlight.foreground1..3`
pub fn token_color(kind: TokenKind) -> Rgba {
    const BRACKET_COLORS: [u32; 3] = [0x0431FA, 0x319331, 0x7B3814];
    match kind {
        TokenKind::Plain => rgb(0x000000),
        TokenKind::Keyword => rgb(0x0000FF),
        TokenKind::TypeIdentifier => rgb(0x008080),
        TokenKind::String => rgb(0xA31515),
        TokenKind::Number => rgb(0x098658),
        TokenKind::Comment => rgb(0x008000),
        TokenKind::Bracket(depth) => rgb(BRACKET_COLORS[depth % BRACKET_COLORS.len()]),
        TokenKind::UnexpectedBracket => Rgba {
            r: 1.0,
            g: 18.0 / 255.0,
            b: 18.0 / 255.0,
            a: 0.8,
        },
    }
}

/// 한 줄 안의 토큰. `range`는 줄 텍스트의 바이트 범위다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub range: Range<usize>,
    pub kind: TokenKind,
}

struct Grammar {
    keywords: &'static [&'static str],
    line_comment: Option<&'static str>,
    block_comment: Option<(&'static str, &'static str)>,
    quotes: &'static [char],
    /// Monarch TS의 `/[A-Z][\w\$]*/` → `type.identifier`
    has_type_identifiers: bool,
}

const TYPESCRIPT_KEYWORDS: &[&str] = &[
    "abstract",
    "any",
    "as",
    "asserts",
    "bigint",
    "boolean",
    "break",
    "case",
    "catch",
    "class",
    "continue",
    "const",
    "constructor",
    "debugger",
    "declare",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "implements",
    "import",
    "in",
    "infer",
    "instanceof",
    "interface",
    "is",
    "keyof",
    "let",
    "module",
    "namespace",
    "never",
    "new",
    "null",
    "number",
    "object",
    "out",
    "package",
    "private",
    "protected",
    "public",
    "override",
    "readonly",
    "require",
    "global",
    "return",
    "satisfies",
    "set",
    "static",
    "string",
    "super",
    "switch",
    "symbol",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "unique",
    "unknown",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "async",
    "await",
    "of",
];

const PYTHON_KEYWORDS: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif",
    "else", "except", "False", "finally", "for", "from", "global", "if", "import", "in", "is",
    "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return", "True", "try", "while",
    "with", "yield", "self",
];

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

const GO_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
    "nil",
    "true",
    "false",
];

const SHELL_KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "until", "case",
    "esac", "function", "return", "local", "export", "echo", "exit",
];

const C_LIKE_KEYWORDS: &[&str] = &[
    "abstract",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "false",
    "final",
    "float",
    "for",
    "fun",
    "func",
    "if",
    "implements",
    "import",
    "int",
    "interface",
    "let",
    "long",
    "new",
    "null",
    "override",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "val",
    "var",
    "void",
    "while",
];

const JSON_KEYWORDS: &[&str] = &["true", "false", "null"];

fn grammar_for(language: Language) -> Grammar {
    let c_style = (Some("//"), Some(("/*", "*/")));
    let (keywords, (line_comment, block_comment), quotes, has_type_identifiers) = match language {
        Language::TypeScript | Language::JavaScript => {
            (TYPESCRIPT_KEYWORDS, c_style, &['"', '\'', '`'][..], true)
        }
        Language::Json => (JSON_KEYWORDS, c_style, &['"'][..], false),
        Language::Python => (PYTHON_KEYWORDS, (Some("#"), None), &['"', '\''][..], false),
        Language::Rust => (RUST_KEYWORDS, c_style, &['"'][..], false),
        Language::Go => (GO_KEYWORDS, c_style, &['"', '`'][..], false),
        Language::Shell => (SHELL_KEYWORDS, (Some("#"), None), &['"', '\''][..], false),
        Language::CLike => (C_LIKE_KEYWORDS, c_style, &['"', '\''][..], false),
        Language::PlainText => (&[][..], (None, None), &[][..], false),
    };
    Grammar {
        keywords,
        line_comment,
        block_comment,
        quotes,
        has_type_identifiers,
    }
}

/// 줄을 넘어 이어지는 어휘 상태(여러 줄 주석, 템플릿 문자열)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LexState {
    Code,
    BlockComment,
    MultilineString(char),
}

/// 문서 전체를 줄별 토큰으로 나눈다. 괄호 깊이는 문서 전체에서 이어 센다(Monaco bracket pair colorization)
pub fn tokenize_document(lines: &[impl AsRef<str>], language: Language) -> Vec<Vec<Token>> {
    tokenize_lines(lines, language).tokens
}

/// 한 줄을 다 읽은 뒤의 어휘 상태. 다음 줄은 이 상태에서 읽기 시작한다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEndLexState {
    state: LexState,
    open_brackets: Vec<char>,
}

impl LineEndLexState {
    fn document_start() -> Self {
        Self {
            state: LexState::Code,
            open_brackets: Vec::new(),
        }
    }
}

/// 줄별 토큰과 줄 끝 어휘 상태. 편집 뒤 다시 토큰화할 때 바뀌지 않은 줄을 건너뛰는 데 쓴다
#[derive(Debug, Clone, PartialEq)]
pub struct TokenizedLines {
    pub tokens: Vec<Vec<Token>>,
    pub line_end_states: Vec<LineEndLexState>,
}

/// 앞서 토큰화한 문서. 줄 글과 그 결과를 함께 넘긴다
pub struct PreviousTokenization<'a, L: AsRef<str>> {
    pub lines: &'a [L],
    pub tokens: &'a [Vec<Token>],
    pub line_end_states: &'a [LineEndLexState],
}

pub fn tokenize_lines(lines: &[impl AsRef<str>], language: Language) -> TokenizedLines {
    let no_previous_lines: &[&str] = &[];
    let empty = PreviousTokenization {
        lines: no_previous_lines,
        tokens: &[],
        line_end_states: &[],
    };
    retokenize_lines(&empty, lines, language)
}

/// 앞 문서와 같은 앞쪽 줄은 그대로 두고, 달라진 줄부터 다시 읽는다. 달라진 줄을 지난 뒤 줄 끝 어휘 상태가
/// 앞 문서의 같은 줄과 같아지면 그 뒤 줄의 토큰은 앞 문서 것을 옮긴다. 결과는 처음부터 다시 토큰화한 것과 같다
pub fn retokenize_lines<L: AsRef<str>>(
    previous: &PreviousTokenization<'_, L>,
    lines: &[impl AsRef<str>],
    language: Language,
) -> TokenizedLines {
    let previous_count = previous.lines.len();
    let unchanged_prefix = lines
        .iter()
        .zip(previous.lines)
        .take_while(|(line, previous_line)| line.as_ref() == previous_line.as_ref())
        .count();
    let unchanged_suffix = lines[unchanged_prefix..]
        .iter()
        .rev()
        .zip(previous.lines[unchanged_prefix..].iter().rev())
        .take_while(|(line, previous_line)| line.as_ref() == previous_line.as_ref())
        .count();
    let start = match unchanged_prefix.checked_sub(1) {
        Some(last_unchanged) => previous.line_end_states[last_unchanged].clone(),
        None => LineEndLexState::document_start(),
    };
    let grammar = grammar_for(language);
    let mut lexer = Lexer {
        grammar: &grammar,
        state: start.state,
        open_brackets: start.open_brackets,
    };
    let mut tokenized = TokenizedLines {
        tokens: previous.tokens[..unchanged_prefix].to_vec(),
        line_end_states: previous.line_end_states[..unchanged_prefix].to_vec(),
    };
    let changed_end = lines.len() - unchanged_suffix;
    let previous_changed_end = previous_count - unchanged_suffix;
    let mut index = unchanged_prefix;
    loop {
        if index >= changed_end {
            let previous_index = index - changed_end + previous_changed_end;
            if lexer.starts_like(previous, previous_index) {
                tokenized
                    .tokens
                    .extend_from_slice(&previous.tokens[previous_index..]);
                tokenized
                    .line_end_states
                    .extend_from_slice(&previous.line_end_states[previous_index..]);
                break;
            }
        }
        let Some(line) = lines.get(index) else {
            break;
        };
        tokenized.tokens.push(lexer.tokenize_line(line.as_ref()));
        tokenized.line_end_states.push(lexer.line_end_state());
        index += 1;
    }
    tokenized
}

struct Lexer<'a> {
    grammar: &'a Grammar,
    state: LexState,
    /// 아직 닫히지 않은 여는 괄호들
    open_brackets: Vec<char>,
}

impl Lexer<'_> {
    /// 지금 상태가 앞 문서 `previous_index`번 줄을 읽기 시작할 때의 상태와 같은지
    fn starts_like<L: AsRef<str>>(
        &self,
        previous: &PreviousTokenization<'_, L>,
        previous_index: usize,
    ) -> bool {
        let previous_start = match previous_index.checked_sub(1) {
            Some(previous_line) => &previous.line_end_states[previous_line],
            None => &LineEndLexState::document_start(),
        };
        self.state == previous_start.state && self.open_brackets == previous_start.open_brackets
    }

    fn line_end_state(&self) -> LineEndLexState {
        LineEndLexState {
            state: self.state,
            open_brackets: self.open_brackets.clone(),
        }
    }

    fn tokenize_line(&mut self, line: &str) -> Vec<Token> {
        let mut tokens = Vec::new();
        let mut offset = 0;
        while offset < line.len() {
            let (end, kind) = match self.state {
                LexState::BlockComment => self.read_block_comment(line, offset),
                LexState::MultilineString(quote) => self.read_string(line, offset, quote),
                LexState::Code => self.read_code(line, offset),
            };
            push_token(&mut tokens, offset..end, kind);
            offset = end;
        }
        // 한 줄 문자열은 줄 끝에서 닫힌 것으로 본다(Monarch도 다음 줄로 넘기지 않는다)
        if let LexState::MultilineString(quote) = self.state
            && quote != '`'
        {
            self.state = LexState::Code;
        }
        tokens
    }

    fn read_block_comment(&mut self, line: &str, offset: usize) -> (usize, TokenKind) {
        let close = self
            .grammar
            .block_comment
            .map(|(_, close)| close)
            .unwrap_or("*/");
        match line[offset..].find(close) {
            Some(found) => {
                self.state = LexState::Code;
                (offset + found + close.len(), TokenKind::Comment)
            }
            None => (line.len(), TokenKind::Comment),
        }
    }

    fn read_string(&mut self, line: &str, offset: usize, quote: char) -> (usize, TokenKind) {
        let mut characters = line[offset..].char_indices();
        while let Some((index, character)) = characters.next() {
            if character == '\\' {
                characters.next();
            } else if character == quote {
                self.state = LexState::Code;
                return (offset + index + character.len_utf8(), TokenKind::String);
            }
        }
        (line.len(), TokenKind::String)
    }

    fn read_code(&mut self, line: &str, offset: usize) -> (usize, TokenKind) {
        let rest = &line[offset..];
        let first = rest.chars().next().unwrap_or(' ');
        if let Some(prefix) = self.grammar.line_comment
            && rest.starts_with(prefix)
        {
            return (line.len(), TokenKind::Comment);
        }
        if let Some((open, _)) = self.grammar.block_comment
            && rest.starts_with(open)
        {
            self.state = LexState::BlockComment;
            let (end, kind) = self.read_block_comment(line, offset + open.len());
            return (end, kind);
        }
        if self.grammar.quotes.contains(&first) {
            self.state = LexState::MultilineString(first);
            return self.read_string(line, offset + 1, first);
        }
        if first.is_ascii_digit() {
            return (offset + scan_while(rest, is_number_char), TokenKind::Number);
        }
        if is_identifier_start(first) {
            let end = offset + scan_while(rest, is_identifier_char);
            return (end, self.classify_word(&line[offset..end]));
        }
        let end = offset + first.len_utf8();
        (end, self.classify_symbol(first))
    }

    fn classify_word(&self, word: &str) -> TokenKind {
        if self.grammar.keywords.contains(&word) {
            TokenKind::Keyword
        } else if self.grammar.has_type_identifiers
            && word.starts_with(|c: char| c.is_ascii_uppercase())
        {
            TokenKind::TypeIdentifier
        } else {
            TokenKind::Plain
        }
    }

    fn classify_symbol(&mut self, symbol: char) -> TokenKind {
        if self.grammar.keywords.is_empty() {
            return TokenKind::Plain;
        }
        match symbol {
            '(' | '[' | '{' => {
                self.open_brackets.push(symbol);
                TokenKind::Bracket(self.open_brackets.len() - 1)
            }
            ')' | ']' | '}' => self.close_bracket(symbol),
            _ => TokenKind::Plain,
        }
    }

    fn close_bracket(&mut self, symbol: char) -> TokenKind {
        let expected_open = match symbol {
            ')' => '(',
            ']' => '[',
            _ => '{',
        };
        if self.open_brackets.last() == Some(&expected_open) {
            self.open_brackets.pop();
            TokenKind::Bracket(self.open_brackets.len())
        } else {
            TokenKind::UnexpectedBracket
        }
    }
}

/// 같은 종류가 이어지면 한 토큰으로 합쳐 글자 모양 묶음(run)을 줄인다
fn push_token(tokens: &mut Vec<Token>, range: Range<usize>, kind: TokenKind) {
    let is_mergeable = !matches!(kind, TokenKind::Bracket(_) | TokenKind::UnexpectedBracket);
    if let Some(last) = tokens.last_mut()
        && last.kind == kind
        && last.range.end == range.start
        && is_mergeable
    {
        last.range.end = range.end;
        return;
    }
    tokens.push(Token { range, kind });
}

fn scan_while(text: &str, predicate: impl Fn(char) -> bool) -> usize {
    text.char_indices()
        .find(|(_, character)| !predicate(*character))
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

fn is_identifier_start(character: char) -> bool {
    character.is_alphabetic() || character == '_' || character == '$'
}

fn is_identifier_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '$'
}

fn is_number_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '.' || character == '_'
}

/// 커서를 둘러싼 괄호 쌍(Monaco `matchBrackets`). 커서 바로 앞뒤 괄호가 있으면 그 쌍을, 없으면 커서를 감싼 가장 안쪽 쌍을 고른다
pub fn matching_bracket_pair(
    tokens: &[Vec<Token>],
    cursor: (usize, usize),
) -> Option<((usize, usize), (usize, usize))> {
    let mut open_stack: Vec<(usize, usize)> = Vec::new();
    let mut pairs = Vec::new();
    for (line, line_tokens) in tokens.iter().enumerate() {
        for token in line_tokens {
            let TokenKind::Bracket(depth) = token.kind else {
                continue;
            };
            let position = (line, token.range.start);
            if depth == open_stack.len() {
                open_stack.push(position);
            } else if let Some(open) = open_stack.pop() {
                pairs.push((open, position));
            }
        }
    }
    let is_adjacent = |point: (usize, usize)| {
        point.0 == cursor.0 && (point.1 == cursor.1 || point.1 + 1 == cursor.1)
    };
    pairs
        .iter()
        .find(|(open, close)| is_adjacent(*open) || is_adjacent(*close))
        .or_else(|| {
            pairs
                .iter()
                .filter(|(open, close)| *open < cursor && cursor <= *close)
                .max_by_key(|(open, _)| *open)
        })
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str, language: Language) -> Vec<(String, TokenKind)> {
        tokenize_document(&[line], language)[0]
            .iter()
            .filter(|token| !line[token.range.clone()].trim().is_empty())
            .map(|token| (line[token.range.clone()].trim().to_string(), token.kind))
            .collect()
    }

    fn retokenized_after_edit(before: &[&str], after: &[&str]) -> TokenizedLines {
        let previous = tokenize_lines(before, Language::TypeScript);
        let previous = PreviousTokenization {
            lines: before,
            tokens: &previous.tokens,
            line_end_states: &previous.line_end_states,
        };
        retokenize_lines(&previous, after, Language::TypeScript)
    }

    #[test]
    fn retokenizing_after_an_edit_matches_tokenizing_from_scratch() {
        let before = [
            "const a = (1);",
            "let s = `x`;",
            "function f() {",
            "  return [a];",
            "}",
            "const z = \"tail\";",
        ];
        let edits: [&[&str]; 7] = [
            &[
                "const a = (1);",
                "/* let s = `x`;",
                "function f() {",
                "  return [a];",
                "}",
                "const z = \"tail\";",
            ],
            &[
                "const a = (1);",
                "let s = `x",
                "function f() {",
                "  return [a];",
                "}",
                "const z = \"tail\";",
            ],
            &[
                "const a = ((1);",
                "let s = `x`;",
                "function f() {",
                "  return [a];",
                "}",
                "const z = \"tail\";",
            ],
            &[
                "const a = (1);",
                "let s = `x`;",
                "function f() {",
                "  return [a];",
                "}",
                "const z = \"tail\";",
                "x();",
            ],
            &[
                "const a = (1);",
                "function f() {",
                "  return [a];",
                "}",
                "const z = \"tail\";",
            ],
            &[
                "const a = (1);",
                "let s = `x`;",
                "let t = 2;",
                "function f() {",
                "  return [a];",
                "}",
                "const z = \"tail\";",
            ],
            &[],
        ];
        for after in edits {
            assert_eq!(
                retokenized_after_edit(&before, after),
                tokenize_lines(after, Language::TypeScript),
                "{after:?}"
            );
        }
    }

    #[test]
    fn an_edit_that_ends_in_the_same_state_reuses_the_following_lines() {
        let before = ["let a = 1;", "let b = 2;", "let c = 3;"];
        let after = ["let a = 1;", "let bb = 2;", "let c = 3;"];
        let previous = tokenize_lines(&before, Language::TypeScript);
        let mut stale_tokens = previous.tokens.clone();
        stale_tokens[2] = Vec::new();
        let reused = retokenize_lines(
            &PreviousTokenization {
                lines: &before,
                tokens: &stale_tokens,
                line_end_states: &previous.line_end_states,
            },
            &after,
            Language::TypeScript,
        );
        assert!(
            reused.tokens[2].is_empty(),
            "셋째 줄은 다시 읽지 않고 옮긴다"
        );
        assert_eq!(
            reused.tokens[1],
            tokenize_document(&after, Language::TypeScript)[1]
        );
    }

    #[test]
    fn typescript_extensions_map_like_the_monaco_language_table() {
        assert_eq!(detect_language("src/board.ts"), Language::TypeScript);
        assert_eq!(detect_language("App.JSX"), Language::JavaScript);
        assert_eq!(detect_language("notes.txt"), Language::PlainText);
        assert_eq!(detect_language("Makefile"), Language::PlainText);
    }

    #[test]
    fn keywords_types_and_strings_get_the_vs_theme_colors() {
        let tokens = kinds("  status: \"todo\" | Card;", Language::TypeScript);

        assert!(tokens.contains(&("\"todo\"".into(), TokenKind::String)));
        assert!(tokens.contains(&("Card".into(), TokenKind::TypeIdentifier)));
        assert!(
            tokens
                .iter()
                .any(|(text, kind)| text.starts_with("status") && *kind == TokenKind::Plain)
        );
        let keyword = kinds("export interface Card {", Language::TypeScript);
        assert_eq!(keyword[0], ("export".into(), TokenKind::Keyword));
    }

    #[test]
    fn bracket_colors_follow_nesting_depth_across_lines() {
        let lines = [
            "function f(cards: Card[]) {",
            "  return [x].map((a) => a);",
            "}",
        ];
        let tokens = tokenize_document(&lines, Language::TypeScript);
        let bracket_depths = |line_index: usize| -> Vec<usize> {
            tokens[line_index]
                .iter()
                .filter_map(|token| match token.kind {
                    TokenKind::Bracket(depth) => Some(depth),
                    _ => None,
                })
                .collect()
        };

        assert_eq!(bracket_depths(0), vec![0, 1, 1, 0, 0]);
        assert_eq!(bracket_depths(1), vec![1, 1, 1, 2, 2, 1]);
        assert_eq!(bracket_depths(2), vec![0]);
    }

    #[test]
    fn the_innermost_block_around_the_cursor_is_matched() {
        let lines = ["function a(): number { // TODO", "  return b(c);", "}"];
        let tokens = tokenize_document(&lines, Language::TypeScript);

        assert_eq!(
            matching_bracket_pair(&tokens, (0, 30)),
            Some(((0, 21), (2, 0)))
        );
        assert_eq!(
            matching_bracket_pair(&tokens, (1, 11)),
            Some(((1, 10), (1, 12)))
        );
    }

    #[test]
    fn block_comments_continue_on_the_next_line() {
        let tokens = tokenize_document(&["/* start", "end */ let"], Language::TypeScript);

        assert_eq!(tokens[0][0].kind, TokenKind::Comment);
        assert_eq!(tokens[1][0].kind, TokenKind::Comment);
        assert_eq!(tokens[1].last().unwrap().kind, TokenKind::Keyword);
    }
}
