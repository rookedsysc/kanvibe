//! AI 대화 본문(`AiSessionMessageContent`)의 Markdown 처리.
//!
//! TS는 `marked`로 HTML을 만들고 DOMPurify로 거른 뒤 highlight.js·KaTeX·Mermaid로 꾸몄다. 여기서는 같은 결과를
//! 렌더러와 무관한 모델로 만든다. GPUI 뷰는 [`parse_message`]의 [`Segment`] 목록을 그리고, Mermaid 조각은
//! [`render_mermaid`]의 장면으로 그린다(실패하면 TS처럼 정의 원문을 코드 블록으로 보인다).

mod autolink;
mod convert;
pub mod highlight;
mod html;
mod latex;
pub mod math;
mod model;
mod segment;

pub use model::{Alignment, Block, CodeBlock, Inline, List, ListItem, Segment, Table};

use kanvibe_diagram::scene::Scene;
use kanvibe_diagram::text::FontBook;
use kanvibe_diagram::theme::Theme;

/// 언어 없는 코드 블록 머리줄 이름
pub const PLAIN_CODE_LABEL: &str = "text";

/// 메시지 원문을 Markdown·Mermaid 조각으로 나누고 각 Markdown 조각을 모델로 바꾼다
pub fn parse_message(text: &str) -> Vec<Segment> {
    segment::split_markdown_and_mermaid(text)
        .into_iter()
        .map(|raw| match raw {
            segment::RawSegment::Markdown(markdown) => Segment::Markdown(parse_markdown(&markdown)),
            segment::RawSegment::Mermaid(definition) => Segment::Mermaid { definition },
        })
        .collect()
}

/// Markdown 한 조각을 블록 모델로 바꾼다
pub fn parse_markdown(markdown: &str) -> Vec<Block> {
    convert::convert(markdown)
}

/// 코드 블록 모델. 아는 언어면 색 구간을 채운다
pub fn code_block(language: Option<String>, code: String) -> CodeBlock {
    let language = language.filter(|name| !name.is_empty());
    let highlight = language
        .as_deref()
        .and_then(highlight::find_language)
        .map(|definition| highlight::highlight(&code, definition));
    let header_label = language
        .clone()
        .unwrap_or_else(|| PLAIN_CODE_LABEL.to_string());
    CodeBlock {
        language,
        header_label,
        code,
        highlight,
    }
}

/// Mermaid 조각을 그린다. 데스크톱은 앱 테마가 라이트면 Mermaid `default`, 아니면 `dark` 테마를 쓴다
pub fn render_mermaid(
    definition: &str,
    theme: Theme,
    fonts: &FontBook,
) -> Result<Scene, kanvibe_diagram::DiagramError> {
    kanvibe_diagram::render_scene(definition, theme, fonts)
}

#[cfg(test)]
mod tests;
