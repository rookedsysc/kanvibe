//! AI 대화 본문(`AiSessionMessageContent`)과 릴리스 노트(`ReleaseUpdateDialog`)의 Markdown 처리.
//!
//! TS는 `marked`로 HTML을 만들고 DOMPurify로 거른 뒤 브라우저가 파싱한 요소를 highlight.js·KaTeX·Mermaid로
//! 꾸몄다. 여기서도 같은 길을 밟는다. 원문의 수식·GFM 자동 링크를 marked 순서대로 자리표시자로 바꾸고
//! ([`scan`]), pulldown-cmark 사건을 marked 렌더러와 같은 HTML로 쓴 뒤([`emit`]), html5ever로 파싱해
//! DOMPurify 규칙으로 거르고([`sanitize`]), 브라우저가 그리는 모양대로 모델로 옮긴다([`flow`]).
//! GPUI 뷰는 [`parse_message`]의 [`Segment`] 목록을 그리고, Mermaid 조각은 [`render_mermaid`]의 장면으로
//! 그린다(실패하면 TS처럼 정의 원문을 코드 블록으로 보인다).

mod autolink;
mod emit;
mod flow;
pub mod highlight;
mod latex;
pub mod math;
mod model;
mod sanitize;
mod scan;
mod segment;

pub use model::{
    Alignment, Block, CodeBlock, Disclosure, Inline, Length, List, ListItem, Segment, Table,
    TableCaption, TableCell, TableRow,
};

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

/// 대화 Markdown 한 조각을 블록 모델로 바꾼다(marked `breaks: true`와 `latex` 확장)
pub fn parse_markdown(markdown: &str) -> Vec<Block> {
    convert(markdown, true, true)
}

/// 릴리스 노트를 블록 모델로 바꾼다. TS `ReleaseUpdateDialog`는 기본 marked(`breaks: false`, 수식 없음)를 쓴다
pub fn parse_release_notes(markdown: &str) -> Vec<Block> {
    convert(markdown, false, false)
}

fn convert(markdown: &str, is_newline_break: bool, has_latex: bool) -> Vec<Block> {
    let substituted = scan::substitute(markdown, emit::markdown_options(), has_latex);
    let html = emit::markdown_to_html(&substituted.text, &substituted.spans, is_newline_break);
    flow::blocks(&sanitize::sanitize(&html), &substituted.spans)
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

/// Mermaid 조각을 그린다. 데스크톱은 앱 테마가 라이트면 Mermaid `default`, 아니면 `dark` 테마를 쓰고,
/// TS 데스크톱 렌더러의 `htmlLabels: false`처럼 라벨을 SVG 글자로 잰다
pub fn render_mermaid(
    definition: &str,
    theme: Theme,
    fonts: &FontBook,
) -> Result<Scene, kanvibe_diagram::DiagramError> {
    kanvibe_diagram::render_scene(
        definition,
        theme,
        kanvibe_diagram::LabelMode::SvgText,
        fonts,
    )
}

#[cfg(test)]
mod tests;
