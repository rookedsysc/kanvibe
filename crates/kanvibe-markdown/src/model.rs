//! 렌더러와 무관한 Markdown 결과 모델. GPUI 뷰는 이 모델만 보고 요소를 만든다.

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
    Paragraph(Vec<Inline>),
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    CodeBlock(CodeBlock),
    BlockQuote(Vec<Block>),
    List(List),
    /// `kv-chat-table` 틀(가로 스크롤) 안에 그리는 표
    Table(Table),
    ThematicBreak,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CodeBlock {
    /// 펜스 정보 문자열의 첫 낱말. 없으면 None
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
    pub start: u64,
    pub is_tight: bool,
    pub items: Vec<ListItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListItem {
    /// GFM 작업 목록 체크 상자(읽기 전용)
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    None,
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub alignments: Vec<Alignment>,
    pub header: Vec<Vec<Inline>>,
    pub rows: Vec<Vec<Vec<Inline>>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text(String),
    Code(String),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Strikethrough(Vec<Inline>),
    /// 새 창(`target=_blank`, `rel=noopener noreferrer`)으로 연다
    Link {
        href: String,
        title: Option<String>,
        content: Vec<Inline>,
    },
    /// `loading=lazy`, `referrerpolicy=no-referrer`
    Image {
        src: String,
        alt: String,
        title: Option<String>,
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
                Inline::Emphasis(children)
                | Inline::Strong(children)
                | Inline::Strikethrough(children) => {
                    text.push_str(&Self::plain_text(children));
                }
                Inline::Link { content, .. } => text.push_str(&Self::plain_text(content)),
                Inline::Image { alt, .. } => text.push_str(alt),
                Inline::LineBreak => text.push('\n'),
                Inline::Math { tex, .. } => text.push_str(tex),
            }
        }
        text
    }
}
