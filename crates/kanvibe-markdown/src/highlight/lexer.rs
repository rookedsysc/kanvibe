//! 언어 계열별 토큰 규칙. highlight.js 출력(`node_modules/highlight.js`로 확인한 예)에 맞춘 근사다.

use super::{HighlightClass, HighlightSpan, LanguageDefinition};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    /// C 계열·스크립트 언어: 주석·문자열·숫자·키워드·제목
    Code,
    Markup,
    Json,
    Yaml,
    Ini,
    Css,
    Diff,
    Markdown,
    Plain,
}

struct Syntax {
    family: Family,
    line_comments: &'static [&'static str],
    block_comments: &'static [(&'static str, &'static str)],
    quotes: &'static [char],
    /// 이 낱말 뒤의 이름은 제목(함수·클래스 이름)이다
    title_after: &'static [&'static str],
    /// JS·TS: `이름:`은 attr, 대문자 이름은 class 제목, `이름(`은 함수 제목
    is_script: bool,
    /// Python 세 따옴표 문자열
    has_triple_quotes: bool,
}

const C_COMMENTS: &[&str] = &["//"];
const C_BLOCKS: &[(&str, &str)] = &[("/*", "*/")];
const HASH: &[&str] = &["#"];

fn code(
    line_comments: &'static [&'static str],
    block_comments: &'static [(&'static str, &'static str)],
    title_after: &'static [&'static str],
) -> Syntax {
    Syntax {
        family: Family::Code,
        line_comments,
        block_comments,
        quotes: &['"', '\''],
        title_after,
        is_script: false,
        has_triple_quotes: false,
    }
}

fn other(family: Family) -> Syntax {
    Syntax {
        family,
        line_comments: &[],
        block_comments: &[],
        quotes: &['"', '\''],
        title_after: &[],
        is_script: false,
        has_triple_quotes: false,
    }
}

/// C 계열·스크립트 언어. 없으면 None
fn code_syntax_for(name: &str) -> Option<Syntax> {
    let syntax = match name {
        "javascript" | "typescript" => Syntax {
            quotes: &['"', '\'', '`'],
            is_script: true,
            ..code(
                C_COMMENTS,
                C_BLOCKS,
                &[
                    "function",
                    "class",
                    "extends",
                    "implements",
                    "interface",
                    "new",
                ],
            )
        },
        "python" => Syntax {
            has_triple_quotes: true,
            ..code(HASH, &[], &["def", "class"])
        },
        "ruby" => code(HASH, &[], &["def", "class", "module"]),
        "rust" => code(
            C_COMMENTS,
            C_BLOCKS,
            &["fn", "struct", "enum", "trait", "impl", "mod", "union"],
        ),
        "go" => code(C_COMMENTS, C_BLOCKS, &["func"]),
        "java" | "csharp" | "kotlin" | "swift" | "objectivec" | "cpp" | "c" | "php" | "graphql" => {
            code(C_COMMENTS, C_BLOCKS, BRACE_TITLE_KEYWORDS)
        }
        _ => return script_syntax_for(name),
    };
    Some(syntax)
}

const BRACE_TITLE_KEYWORDS: &[&str] = &[
    "class",
    "interface",
    "enum",
    "struct",
    "fun",
    "func",
    "function",
    "protocol",
    "record",
];

/// 셸·스크립트·질의 언어
fn script_syntax_for(name: &str) -> Option<Syntax> {
    let syntax = match name {
        "bash" | "shell" | "makefile" | "r" => code(HASH, &[], &[]),
        "perl" => code(HASH, &[], &["sub"]),
        "lua" => code(&["--"], &[("--[[", "]]")], &["function"]),
        "sql" => code(&["--"], C_BLOCKS, &[]),
        "vbnet" => code(&["'"], &[], &["Sub", "Function", "Class"]),
        "wasm" => code(&[";;"], &[("(;", ";)")], &[]),
        _ => return None,
    };
    Some(syntax)
}

fn syntax_for(name: &str) -> Syntax {
    if let Some(syntax) = code_syntax_for(name) {
        return syntax;
    }
    match name {
        "xml" => other(Family::Markup),
        "json" => other(Family::Json),
        "yaml" => Syntax {
            line_comments: HASH,
            ..other(Family::Yaml)
        },
        "ini" => Syntax {
            line_comments: &[";", "#"],
            ..other(Family::Ini)
        },
        "css" => Syntax {
            block_comments: C_BLOCKS,
            ..other(Family::Css)
        },
        "scss" | "less" => Syntax {
            line_comments: C_COMMENTS,
            block_comments: C_BLOCKS,
            ..other(Family::Css)
        },
        "diff" => other(Family::Diff),
        "markdown" => other(Family::Markdown),
        _ => other(Family::Plain),
    }
}

struct Emitter {
    spans: Vec<HighlightSpan>,
}

impl Emitter {
    fn push(&mut self, text: &str, class: Option<HighlightClass>) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.spans.last_mut()
            && last.class == class
            && class.is_none()
        {
            last.text.push_str(text);
            return;
        }
        self.spans.push(HighlightSpan {
            text: text.to_string(),
            class,
        });
    }
}

/// `hljs.highlight(code, { language })` 결과를 구간으로 돌려준다
pub fn highlight(code: &str, language: &LanguageDefinition) -> Vec<HighlightSpan> {
    let syntax = syntax_for(language.name);
    let mut emitter = Emitter { spans: Vec::new() };
    match syntax.family {
        Family::Plain => emitter.push(code, None),
        Family::Diff => highlight_lines(code, &mut emitter, diff_line_class),
        Family::Markdown => highlight_lines(code, &mut emitter, markdown_line_class),
        Family::Markup => highlight_markup(code, &mut emitter),
        _ => CodeLexer {
            source: code,
            position: 0,
            syntax: &syntax,
            language,
            emitter: &mut emitter,
            previous_word: String::new(),
            brace_depth: 0,
        }
        .run(),
    }
    emitter.spans
}

fn highlight_lines(
    code: &str,
    emitter: &mut Emitter,
    classify: fn(&str) -> Option<HighlightClass>,
) {
    for line in code.split_inclusive('\n') {
        let body = line.trim_end_matches('\n');
        emitter.push(body, classify(body));
        emitter.push(&line[body.len()..], None);
    }
}

fn diff_line_class(line: &str) -> Option<HighlightClass> {
    if line.starts_with("@@") || line.starts_with("diff ") || line.starts_with("index ") {
        return Some(HighlightClass::Meta);
    }
    match line.chars().next() {
        Some('+') => Some(HighlightClass::Addition),
        Some('-') => Some(HighlightClass::Deletion),
        _ => None,
    }
}

fn markdown_line_class(line: &str) -> Option<HighlightClass> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        Some(HighlightClass::Section)
    } else {
        None
    }
}

fn highlight_markup(code: &str, emitter: &mut Emitter) {
    let mut rest = code;
    while let Some(start) = rest.find('<') {
        emitter.push(&rest[..start], None);
        let tail = &rest[start..];
        if let Some(comment) = tail.strip_prefix("<!--") {
            let end = comment.find("-->").map_or(tail.len(), |end| end + 7);
            emitter.push(&tail[..end], Some(HighlightClass::Comment));
            rest = &tail[end..];
            continue;
        }
        let end = tail.find('>').map_or(tail.len(), |end| end + 1);
        markup_tag(&tail[..end], emitter);
        rest = &tail[end..];
    }
    emitter.push(rest, None);
}

fn markup_tag(tag: &str, emitter: &mut Emitter) {
    let prefix_length = if tag.starts_with("</") { 2 } else { 1 };
    emitter.push(&tag[..prefix_length], None);
    let body = &tag[prefix_length..];
    let name_end = body
        .find(|character: char| character.is_whitespace() || character == '>' || character == '/')
        .unwrap_or(body.len());
    emitter.push(&body[..name_end], Some(HighlightClass::Name));
    let mut rest = &body[name_end..];
    while !rest.is_empty() {
        let character = rest.chars().next().unwrap_or(' ');
        if character == '"' || character == '\'' {
            let end = rest[1..].find(character).map_or(rest.len(), |end| end + 2);
            emitter.push(&rest[..end], Some(HighlightClass::String));
            rest = &rest[end..];
        } else if character.is_alphabetic() {
            let end = rest
                .find(|next: char| {
                    !(next.is_alphanumeric() || next == '-' || next == ':' || next == '_')
                })
                .unwrap_or(rest.len());
            emitter.push(&rest[..end], Some(HighlightClass::Attr));
            rest = &rest[end..];
        } else {
            emitter.push(&rest[..character.len_utf8()], None);
            rest = &rest[character.len_utf8()..];
        }
    }
}

struct CodeLexer<'a> {
    source: &'a str,
    position: usize,
    syntax: &'a Syntax,
    language: &'a LanguageDefinition,
    emitter: &'a mut Emitter,
    previous_word: String,
    brace_depth: i32,
}

fn is_identifier_start(character: char) -> bool {
    character.is_alphabetic() || character == '_' || character == '$'
}

fn is_identifier_part(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '$'
}

impl CodeLexer<'_> {
    fn rest(&self) -> &str {
        &self.source[self.position..]
    }

    fn emit(&mut self, length: usize, class: Option<HighlightClass>) {
        let text = &self.source[self.position..self.position + length];
        self.emitter.push(text, class);
        self.position += length;
    }

    fn run(&mut self) {
        while self.position < self.source.len() {
            if self.comment() || self.string() || self.number() || self.word() {
                continue;
            }
            let character = self.rest().chars().next().unwrap_or(' ');
            match character {
                '{' => self.brace_depth += 1,
                '}' => self.brace_depth -= 1,
                _ => {}
            }
            if !character.is_whitespace() {
                self.previous_word.clear();
            }
            self.emit(character.len_utf8(), None);
        }
    }

    fn comment(&mut self) -> bool {
        let rest = self.rest();
        if let Some((_, close)) = self
            .syntax
            .block_comments
            .iter()
            .find(|(open, _)| rest.starts_with(open))
        {
            let end = rest[2..]
                .find(close)
                .map_or(rest.len(), |end| end + 2 + close.len());
            self.emit(end, Some(HighlightClass::Comment));
            return true;
        }
        if self
            .syntax
            .line_comments
            .iter()
            .any(|prefix| rest.starts_with(prefix))
        {
            let end = rest.find('\n').unwrap_or(rest.len());
            self.emit(end, Some(HighlightClass::Comment));
            return true;
        }
        false
    }

    fn string(&mut self) -> bool {
        let rest = self.rest();
        let Some(quote) = rest
            .chars()
            .next()
            .filter(|character| self.syntax.quotes.contains(character))
        else {
            return false;
        };
        if self.syntax.has_triple_quotes {
            let triple: String = std::iter::repeat_n(quote, 3).collect();
            if rest.starts_with(&triple) {
                let end = rest[3..].find(&triple).map_or(rest.len(), |end| end + 6);
                self.emit(end, Some(HighlightClass::String));
                return true;
            }
        }
        let end = string_end(rest, quote);
        let class = if self.syntax.family == Family::Json && self.is_followed_by_colon(end) {
            HighlightClass::Attr
        } else {
            HighlightClass::String
        };
        self.emit(end, Some(class));
        true
    }

    fn is_followed_by_colon(&self, length: usize) -> bool {
        let after = self.rest()[length..].trim_start_matches([' ', '\t']);
        after.starts_with(':') && !after.starts_with("::")
    }

    fn number(&mut self) -> bool {
        let rest = self.rest();
        let first = rest.chars().next().unwrap_or(' ');
        let is_hex_color = self.syntax.family == Family::Css && first == '#';
        let previous = self.source[..self.position].chars().next_back();
        if !(first.is_ascii_digit() || is_hex_color) || previous.is_some_and(is_identifier_part) {
            return false;
        }
        let mut end = first.len_utf8();
        let bytes = rest.as_bytes();
        while end < bytes.len() {
            let byte = bytes[end];
            let is_exponent_sign = matches!(byte, b'+' | b'-')
                && matches!(bytes[end - 1], b'e' | b'E')
                && !rest.starts_with("0x");
            if byte.is_ascii_alphanumeric()
                || byte == b'_'
                || byte == b'.' && bytes.get(end + 1).is_some_and(u8::is_ascii_digit)
                || is_exponent_sign
            {
                end += 1;
            } else {
                break;
            }
        }
        if self.syntax.family == Family::Css {
            end += rest[end..]
                .find(|character: char| !(character.is_ascii_alphabetic() || character == '%'))
                .unwrap_or(rest.len() - end);
        }
        self.emit(end, Some(HighlightClass::Number));
        true
    }

    fn word(&mut self) -> bool {
        let rest = self.rest();
        if !rest.chars().next().is_some_and(is_identifier_start) {
            return false;
        }
        let end = rest
            .find(|character: char| !is_identifier_part(character))
            .unwrap_or(rest.len());
        let word = rest[..end].to_string();
        let mut length = end;
        if self.language.name == "rust" && rest[end..].starts_with('!') {
            length += 1;
        }
        let class = self.classify_word(&word, end);
        self.emit(length, class);
        self.previous_word = word;
        true
    }

    fn table_contains(&self, table: &[&str], word: &str) -> bool {
        if self.language.is_case_insensitive {
            let lowered = word.to_ascii_lowercase();
            table
                .iter()
                .any(|entry| entry.eq_ignore_ascii_case(&lowered))
        } else {
            table.contains(&word)
        }
    }

    fn classify_word(&self, word: &str, end: usize) -> Option<HighlightClass> {
        match self.syntax.family {
            Family::Yaml | Family::Ini
                if self.is_followed_by_colon(end)
                    || self.rest()[end..].trim_start().starts_with('=') =>
            {
                return Some(HighlightClass::Attr);
            }
            Family::Css if self.brace_depth > 0 && self.is_followed_by_colon(end) => {
                return Some(HighlightClass::Attr);
            }
            Family::Css if self.brace_depth == 0 => return Some(HighlightClass::Keyword),
            _ => {}
        }
        if self
            .syntax
            .title_after
            .contains(&self.previous_word.as_str())
            && !self.table_contains(self.language.keywords, word)
        {
            return Some(HighlightClass::Title);
        }
        if self.table_contains(self.language.literals, word) {
            return Some(HighlightClass::Literal);
        }
        if self.table_contains(self.language.keywords, word) {
            return Some(HighlightClass::Keyword);
        }
        if self.table_contains(self.language.types, word) {
            return Some(HighlightClass::Type);
        }
        if self.table_contains(self.language.built_ins, word) {
            return Some(HighlightClass::BuiltIn);
        }
        if self.syntax.family == Family::Yaml {
            return Some(HighlightClass::String);
        }
        if self.syntax.is_script {
            return self.classify_script_word(word, end);
        }
        None
    }

    fn classify_script_word(&self, word: &str, end: usize) -> Option<HighlightClass> {
        let after = &self.rest()[end..];
        if self.is_followed_by_colon(end) {
            return Some(HighlightClass::Attr);
        }
        if after.starts_with('(') {
            return Some(HighlightClass::Title);
        }
        let mut characters = word.chars();
        let is_class_reference = characters.next().is_some_and(char::is_uppercase)
            && characters.next().is_some_and(char::is_lowercase);
        is_class_reference.then_some(HighlightClass::Title)
    }
}

fn string_end(rest: &str, quote: char) -> usize {
    let mut is_escaped = false;
    for (index, character) in rest.char_indices().skip(1) {
        if is_escaped {
            is_escaped = false;
            continue;
        }
        match character {
            '\\' => is_escaped = true,
            '\n' if quote != '`' => return index,
            _ if character == quote => return index + 1,
            _ => {}
        }
    }
    rest.len()
}

#[cfg(test)]
mod tests {
    use super::super::find_language;
    use super::*;

    fn classes(language: &str, code: &str) -> Vec<(String, &'static str)> {
        let definition = find_language(language).expect("language");
        highlight(code, definition)
            .into_iter()
            .filter_map(|span| span.class.map(|class| (span.text, class.hljs_class())))
            .collect()
    }

    #[test]
    fn 타입스크립트는_highlight_js와_같은_분류를_낸다() {
        let found = classes(
            "ts",
            "export const onlyOpen: CardFilter = (card) => card.status !== \"done\";",
        );
        assert_eq!(
            found,
            [
                ("export".to_string(), "hljs-keyword"),
                ("const".to_string(), "hljs-keyword"),
                ("onlyOpen".to_string(), "hljs-attr"),
                ("CardFilter".to_string(), "hljs-title"),
                ("\"done\"".to_string(), "hljs-string"),
            ]
        );
    }

    #[test]
    fn 파이썬_주석과_내장_함수를_가른다() {
        let found = classes(
            "python",
            "def total(items):\n    return sum(items)  # c\nprint(True)",
        );
        assert!(found.contains(&("total".to_string(), "hljs-title")));
        assert!(found.contains(&("sum".to_string(), "hljs-built_in")));
        assert!(found.contains(&("# c".to_string(), "hljs-comment")));
        assert!(found.contains(&("True".to_string(), "hljs-literal")));
    }

    #[test]
    fn 구간을_이으면_원문이다() {
        let code = "fn main() { let x: u32 = 5; println!(\"{}\", x); }";
        let joined: String = highlight(code, find_language("rs").expect("rust"))
            .into_iter()
            .map(|span| span.text)
            .collect();
        assert_eq!(joined, code);
    }
}
