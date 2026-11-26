//! 코드 블록 색칠. highlight.js `lib/common`과 같은 언어 이름·별칭으로 언어를 고르고, 같은 이름의 토큰 분류
//! (`hljs-keyword`, `hljs-string` …)를 돌려준다.
//!
//! highlight.js 문법 엔진을 그대로 옮기지 않고, 언어마다 키워드 표(highlight.js에서 뽑은 생성 파일)와
//! 주석·문자열·숫자·제목 규칙으로 근사한다. 앱 CSS가 색을 주는 분류만 의미가 있다:
//! comment·quote, keyword·selector-tag·literal·type, string·regexp·addition, number·symbol·attr·attribute,
//! title·section·name·built_in, deletion. 모르는 언어는 None을 돌려 원문 그대로 둔다.

mod languages;
mod lexer;

pub use lexer::highlight;

/// highlight.js 언어 정의에서 뽑은 키워드 표
#[derive(Debug)]
pub struct LanguageDefinition {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub is_case_insensitive: bool,
    pub keywords: &'static [&'static str],
    pub literals: &'static [&'static str],
    pub types: &'static [&'static str],
    pub built_ins: &'static [&'static str],
}

/// highlight.js 토큰 분류(`hljs-<이름>`)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HighlightClass {
    Keyword,
    Literal,
    Type,
    BuiltIn,
    Title,
    String,
    Number,
    Comment,
    Attr,
    Meta,
    Section,
    Name,
    Addition,
    Deletion,
    Variable,
}

/// 앱 테마의 `--color-syntax-*` 역할
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxRole {
    Comment,
    Keyword,
    String,
    Number,
    Title,
    Deletion,
}

impl HighlightClass {
    pub fn hljs_class(self) -> &'static str {
        match self {
            Self::Keyword => "hljs-keyword",
            Self::Literal => "hljs-literal",
            Self::Type => "hljs-type",
            Self::BuiltIn => "hljs-built_in",
            Self::Title => "hljs-title",
            Self::String => "hljs-string",
            Self::Number => "hljs-number",
            Self::Comment => "hljs-comment",
            Self::Attr => "hljs-attr",
            Self::Meta => "hljs-meta",
            Self::Section => "hljs-section",
            Self::Name => "hljs-name",
            Self::Addition => "hljs-addition",
            Self::Deletion => "hljs-deletion",
            Self::Variable => "hljs-variable",
        }
    }

    /// globals.css `.kv-chat-code .hljs-*` 규칙이 주는 색 역할. 규칙이 없으면 기본 글자색이다
    pub fn syntax_role(self) -> Option<SyntaxRole> {
        match self {
            Self::Comment => Some(SyntaxRole::Comment),
            Self::Keyword | Self::Literal | Self::Type => Some(SyntaxRole::Keyword),
            Self::String | Self::Addition => Some(SyntaxRole::String),
            Self::Number | Self::Attr => Some(SyntaxRole::Number),
            Self::Title | Self::Section | Self::Name | Self::BuiltIn => Some(SyntaxRole::Title),
            Self::Deletion => Some(SyntaxRole::Deletion),
            Self::Meta | Self::Variable => None,
        }
    }
}

/// 색칠한 구간. 모든 구간의 `text`를 이으면 원문이다
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HighlightSpan {
    pub text: String,
    pub class: Option<HighlightClass>,
}

/// `hljs.getLanguage(name)`: 이름과 별칭을 대소문자 없이 찾는다
pub fn find_language(name: &str) -> Option<&'static LanguageDefinition> {
    let lowered = name.to_ascii_lowercase();
    languages::LANGUAGES
        .iter()
        .find(|language| language.name == lowered || language.aliases.contains(&lowered.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 별칭으로_언어를_찾는다() {
        assert_eq!(
            find_language("ts").map(|language| language.name),
            Some("typescript")
        );
        assert_eq!(
            find_language("PY").map(|language| language.name),
            Some("python")
        );
        assert!(find_language("nosuchlang").is_none());
    }
}
