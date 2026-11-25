//! 렌더러와 무관한 Markdown 결과 모델. GPUI 뷰는 이 모델만 보고 요소를 만든다.
//!
//! 모델은 TS가 DOMPurify로 거른 뒤 브라우저가 만든 요소 나무를 따른다. 모양을 정하는 CSS(여백, 표 머리 칸
//! 정렬, 그림의 `display: block` 등)는 화면마다 다르므로 뷰가 맡는다.

use crate::highlight::HighlightSpan;
use crate::math::MathNode;

/// 메시지 하나를 이루는 조각(Markdown 본문 또는 Mermaid 다이어그램)
#[derive(Clone, Debug, PartialEq)]
pub enum Segment {
    Markdown(Vec<Block>),
    /// 펜스 안의 정의(앞뒤 공백 제거). 그리기 실패 시 TS처럼 원문 코드 블록으로 보여 준다
    Mermaid {
        definition: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// `<p>` 문단. 위아래 여백이 있다
    Paragraph(Vec<Inline>),
    /// 문단 태그 없이 블록 안에 바로 놓인 글자 줄(꽉 찬 목록 항목, 표 칸, `<div>` 안 글자)
    Inlines(Vec<Inline>),
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    /// 첫 요소가 `<code>`인 `<pre>`. TS는 언어 이름·복사 버튼 머리줄을 붙인다
    CodeBlock(CodeBlock),
    /// 첫 요소가 `<code>`가 아닌 원시 `<pre>`. 머리줄 없이 공백과 줄바꿈(`\n`)을 그대로 둔다
    Preformatted(Vec<Inline>),
    BlockQuote(Vec<Block>),
    List(List),
    /// TS 대화 화면은 `kv-chat-table` 틀(가로 스크롤) 안에 그린다
    Table(Table),
    /// `<details>` 접기 상자
    Disclosure(Disclosure),
    ThematicBreak,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CodeBlock {
    /// `<code>`의 `language-*` 분류 이름. 없으면 None
    pub language: Option<String>,
    /// 머리줄에 보이는 이름(`language` 또는 "text")
    pub header_label: String,
    /// 복사 버튼이 복사하는 코드(TS `pre.textContent`와 같다)
    pub code: String,
    /// highlight.js가 아는 언어일 때만 있는 색 구간
    pub highlight: Option<Vec<HighlightSpan>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub is_ordered: bool,
    /// `<ol start>`(없으면 1)
    pub start: i64,
    pub items: Vec<ListItem>,
}

/// 느슨한 목록 항목은 [`Block::Paragraph`], 꽉 찬 목록 항목은 [`Block::Inlines`]를 담는다
#[derive(Clone, Debug, PartialEq)]
pub struct ListItem {
    pub blocks: Vec<Block>,
}

/// 칸의 `align` 속성. 대화 화면 CSS는 머리 칸(`th`)을 이 값과 상관없이 왼쪽으로 맞춘다
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    None,
    Left,
    Center,
    Right,
}

/// `thead`·`tbody`·`tfoot`의 행을 문서 순서대로 담는다
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub rows: Vec<TableRow>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableCell {
    /// `th`면 참
    pub is_header: bool,
    pub alignment: Alignment,
    pub column_span: u32,
    pub row_span: u32,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Disclosure {
    /// 첫 `<summary>` 내용. 없으면 브라우저 기본 이름("Details")을 보인다
    pub summary: Option<Vec<Block>>,
    /// `open` 속성. 닫혀 있으면 `blocks`를 숨긴다
    pub is_open: bool,
    pub blocks: Vec<Block>,
}

/// `<img width|height>` 값
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Pixels(f32),
    Percent(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text(String),
    Code(String),
    /// `em`·`i`·`cite`·`var`·`dfn`
    Emphasis(Vec<Inline>),
    /// `strong`·`b`
    Strong(Vec<Inline>),
    /// `del`·`s`·`strike`
    Strikethrough(Vec<Inline>),
    /// `u`·`ins`
    Underline(Vec<Inline>),
    /// `mark`
    Highlight(Vec<Inline>),
    Subscript(Vec<Inline>),
    Superscript(Vec<Inline>),
    /// `kbd`·`samp`·`tt`: 고정폭 글꼴만 바꾼다(코드 배경 없음)
    Monospace(Vec<Inline>),
    /// 새 창(`target=_blank`, `rel=noopener noreferrer`)으로 연다. DOMPurify가 막은 주소는 None
    Link {
        href: Option<String>,
        title: Option<String>,
        content: Vec<Inline>,
    },
    /// `loading=lazy`, `referrerpolicy=no-referrer`. Tailwind preflight 때문에 TS에서는 `display: block`이라
    /// 줄을 혼자 차지한다. DOMPurify가 막았거나 없는 주소는 None(대체 글자만 보인다)
    Image {
        src: Option<String>,
        alt: String,
        title: Option<String>,
        width: Option<Length>,
        height: Option<Length>,
    },
    /// `<input type="checkbox">`(작업 목록 `- [ ]`). 읽기 전용
    Checkbox {
        is_checked: bool,
    },
    LineBreak,
    /// KaTeX MathML에 해당하는 수식. `display`면 블록 수식이다
    Math {
        tex: String,
        display: bool,
        node: MathNode,
    },
}

impl Inline {
    /// 글자만 모은다(테스트와 선택·복사용)
    pub fn plain_text(inlines: &[Inline]) -> String {
        let mut text = String::new();
        for inline in inlines {
            match inline {
                Inline::Text(value) | Inline::Code(value) => text.push_str(value),
                Inline::Link { content, .. } => text.push_str(&Self::plain_text(content)),
                Inline::Image { alt, .. } => text.push_str(alt),
                Inline::LineBreak => text.push('\n'),
                Inline::Math { tex, .. } => text.push_str(tex),
                Inline::Checkbox { .. } => {}
                container => text.push_str(&Self::plain_text(container.children().unwrap_or(&[]))),
            }
        }
        text
    }

    /// 꾸밈 묶음(강조·굵게·밑줄 …)의 안쪽
    pub fn children(&self) -> Option<&[Inline]> {
        match self {
            Inline::Emphasis(children)
            | Inline::Strong(children)
            | Inline::Strikethrough(children)
            | Inline::Underline(children)
            | Inline::Highlight(children)
            | Inline::Subscript(children)
            | Inline::Superscript(children)
            | Inline::Monospace(children) => Some(children),
            Inline::Link { content, .. } => Some(content),
            _ => None,
        }
    }

    pub(crate) fn children_mut(&mut self) -> Option<&mut Vec<Inline>> {
        match self {
            Inline::Emphasis(children)
            | Inline::Strong(children)
            | Inline::Strikethrough(children)
            | Inline::Underline(children)
            | Inline::Highlight(children)
            | Inline::Subscript(children)
            | Inline::Superscript(children)
            | Inline::Monospace(children) => Some(children),
            Inline::Link { content, .. } => Some(content),
            _ => None,
        }
    }
}
