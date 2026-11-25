//! Markdown 블록 모델을 GPUI 요소로 그린다(TS `MarkdownContent`의 `.ai-session-markdown` 규칙과 `globals.css`).
//!
//! 블록 사이 간격은 CSS 세로 여백 겹침처럼 앞 블록의 아래 여백과 뒤 블록의 위 여백 중 큰 값이고, 첫 블록의 위·마지막
//! 블록의 아래 여백은 없다(`[&_*:first-child]:mt-0 [&_*:last-child]:mb-0`). 모든 글자는 창 글자 선택에 참여한다.

use gpui_kit::{
    AnyElement, App, Div, ElementId, Hsla, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, StyledImage,
    Window, div, img, px,
};
use kanvibe_markdown::highlight::SyntaxRole;
use kanvibe_markdown::{Alignment, Block, CodeBlock, Disclosure, Inline, Length, List};

use super::markdown_inline::{InlinePiece, InlineStyle, inline_pieces, styled_spans};
use super::markdown_table::MarkdownTable;
use super::selectable_text::{RichTextSpans, SelectableRichText};
use super::{CodeCopyButton, MessageCopyLabels};
use crate::components::TailwindText;
use crate::fonts::mono_font;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 본문 줄 높이(`leading-6`)
const BODY_LINE_HEIGHT: f32 = 24.0;
/// 작은 고정폭 글자(0.86em)가 본문 글자와 기준선을 맞추면 줄 상자가 1px 커진다. 인라인 코드가 있는 줄과 코드 블록 줄
const MIXED_CODE_LINE_HEIGHT: f32 = 25.0;
/// 코드 글자 크기(`[&_code]:text-[0.86em]`)
pub const CODE_FONT_SIZE: f32 = 12.04;

/// 그리는 데 필요한 공통 값
#[derive(Clone)]
pub struct MarkdownContext {
    pub theme: ThemeTokens,
    pub labels: MessageCopyLabels,
    /// 이 본문의 요소 id 앞부분(대화 행 키 등). 같은 화면의 두 본문이 겹치지 않게 한다
    pub scope: SharedString,
}

impl MarkdownContext {
    pub fn selection_color(&self) -> Hsla {
        self.theme.brand.muted.into()
    }

    fn id(&self, path: &str) -> ElementId {
        ElementId::Name(SharedString::from(format!("{}/{path}", self.scope)))
    }
}

/// 블록 위·아래 여백(px)
fn block_margins(block: &Block) -> (f32, f32) {
    match block {
        Block::Heading { level: 1, .. } => (20.0, 12.0),
        Block::Heading { level: 2 | 3, .. } => (16.0, 8.0),
        Block::Heading { .. } => (0.0, 0.0),
        // 문단 없는 글자 줄·원시 `pre`·`details`는 preflight가 여백을 없앤다
        Block::Inlines(_) | Block::Preformatted(_) | Block::Disclosure(_) => (0.0, 0.0),
        Block::ThematicBreak => (16.0, 16.0),
        // 정렬 묶음은 테두리·안쪽 여백이 없어 첫·끝 블록의 여백이 바깥으로 겹쳐 나온다(CSS 여백 겹침)
        Block::Aligned { blocks, .. } => (
            blocks.first().map_or(0.0, |first| block_margins(first).0),
            blocks.last().map_or(0.0, |last| block_margins(last).1),
        ),
        _ => (12.0, 12.0),
    }
}

/// 블록 목록. 느슨하지 않은 목록 항목 안의 문단은 여백 없이 붙는다
pub fn render_blocks(blocks: &[Block], context: &MarkdownContext, path: &str) -> Div {
    let mut column = div().flex().flex_col().min_w_0();
    let mut previous_bottom: Option<f32> = None;
    for (index, block) in blocks.iter().enumerate() {
        let (top, bottom) = block_margins(block);
        let gap = previous_bottom.map_or(0.0, |previous| previous.max(top));
        let element = render_block(block, context, &format!("{path}.{index}"));
        column = column.child(div().mt(px(gap)).min_w_0().child(element));
        previous_bottom = Some(bottom);
    }
    column
}

fn render_block(block: &Block, context: &MarkdownContext, path: &str) -> AnyElement {
    let theme = &context.theme;
    match block {
        Block::Paragraph(inlines) | Block::Inlines(inlines) => {
            render_paragraph(inlines, context, path, theme.text.primary)
        }
        Block::Preformatted(inlines) => render_preformatted(inlines, context, path),
        Block::Disclosure(disclosure) => {
            MarkdownDisclosure::new(disclosure.clone(), context.clone(), path).into_any_element()
        }
        Block::Heading { level, content } => render_heading(*level, content, context, path),
        Block::CodeBlock(code) => render_code_block(code, context, path),
        Block::BlockQuote(children) => div()
            .border_l_2()
            .border_color(theme.border.default)
            .pl(spacing(3.0))
            .child(render_quote_blocks(children, context, path))
            .into_any_element(),
        Block::List(list) => render_list(list, context, path),
        Block::Table(table) => {
            MarkdownTable::new(table.clone(), context.clone(), path).into_any_element()
        }
        Block::ThematicBreak => div().h(px(1.0)).bg(theme.border.subtle).into_any_element(),
        Block::Aligned { alignment, blocks } => {
            render_aligned_blocks(*alignment, blocks, context, path)
        }
    }
}

/// `align` 정렬 묶음. Chromium은 값을 `text-align`으로 안쪽 글자에 물려준다. GPUI 글자 정렬에는 양쪽 맞춤이
/// 없어(`TextAlign`은 Left·Center·Right뿐) `justify`는 왼쪽 맞춤으로 그린다
fn render_aligned_blocks(
    alignment: Alignment,
    blocks: &[Block],
    context: &MarkdownContext,
    path: &str,
) -> AnyElement {
    let text_align = match alignment {
        Alignment::Center => gpui_kit::TextAlign::Center,
        Alignment::Right => gpui_kit::TextAlign::Right,
        Alignment::None | Alignment::Left | Alignment::Justify => gpui_kit::TextAlign::Left,
    };
    render_blocks(blocks, context, &format!("{path}a"))
        .text_align(text_align)
        .into_any_element()
}

/// 인용문 안 글자는 `text-text-muted`
fn render_quote_blocks(blocks: &[Block], context: &MarkdownContext, path: &str) -> Div {
    let muted = MarkdownContext {
        theme: ThemeTokens {
            text: muted_text_colors(&context.theme),
            ..context.theme
        },
        ..context.clone()
    };
    render_blocks(blocks, &muted, &format!("{path}q"))
}

fn muted_text_colors(theme: &ThemeTokens) -> crate::theme::TextColors {
    crate::theme::TextColors {
        primary: theme.text.muted,
        ..theme.text
    }
}

/// 문단 하나. 그림과 블록 수식은 글자 흐름에서 떨어져 따로 놓인다
pub fn render_paragraph(
    inlines: &[Inline],
    context: &MarkdownContext,
    path: &str,
    color: gpui_kit::Rgba,
) -> AnyElement {
    let pieces = inline_pieces(inlines, &InlineStyle::plain(color), &context.theme);
    let mut column = div().flex().flex_col().min_w_0();
    if has_inline_code(inlines) {
        column = column.line_height(px(MIXED_CODE_LINE_HEIGHT));
    }
    for (index, piece) in pieces.into_iter().enumerate() {
        let id = context.id(&format!("{path}:{index}"));
        column = column.child(render_piece(piece, id, context));
    }
    column.into_any_element()
}

fn has_inline_code(inlines: &[Inline]) -> bool {
    inlines.iter().any(|inline| match inline {
        Inline::Code(_) => true,
        other => other.children().is_some_and(has_inline_code),
    })
}

/// 원시 `<pre>`(`[&_pre]:overflow-x-auto [&_pre]:p-3`, preflight 고정폭 글꼴). 공백과 줄바꿈을 그대로 둔다
fn render_preformatted(inlines: &[Inline], context: &MarkdownContext, path: &str) -> AnyElement {
    let style = InlineStyle {
        is_mono: true,
        ..InlineStyle::plain(context.theme.text.primary)
    };
    let mut column = div()
        .id(context.id(&format!("{path}pre")))
        .overflow_x_scroll()
        .p(spacing(3.0))
        .whitespace_nowrap()
        .flex()
        .flex_col();
    for (index, piece) in inline_pieces(inlines, &style, &context.theme)
        .into_iter()
        .enumerate()
    {
        column = column.child(render_piece(
            piece,
            context.id(&format!("{path}pre:{index}")),
            context,
        ));
    }
    column.into_any_element()
}

fn render_piece(piece: InlinePiece, id: ElementId, context: &MarkdownContext) -> AnyElement {
    match piece {
        InlinePiece::Text(spans) => {
            SelectableRichText::new(id, spans, context.selection_color()).into_any_element()
        }
        InlinePiece::Image {
            src,
            alt,
            width,
            height,
        } => render_image(src, alt, width, height, context),
        InlinePiece::DisplayMath(text) => {
            let style = InlineStyle {
                is_italic: true,
                ..InlineStyle::plain(context.theme.text.primary)
            };
            let spans = styled_spans(&text, &style);
            // KaTeX 블록 수식: 가운데 정렬, 글자 1.21em
            div()
                .flex()
                .justify_center()
                .my(spacing(2.0))
                .text_size(px(16.94))
                .child(SelectableRichText::new(
                    id,
                    spans,
                    context.selection_color(),
                ))
                .into_any_element()
        }
    }
}

/// `<img loading=lazy referrerpolicy=no-referrer>`(preflight `display: block; max-width: 100%`).
/// 주소가 막혔거나 불러오지 못하면 대체 글자를 보인다
fn render_image(
    src: Option<String>,
    alt: String,
    width: Option<Length>,
    height: Option<Length>,
    context: &MarkdownContext,
) -> AnyElement {
    let muted = context.theme.text.muted;
    let Some(src) = src else {
        return div().text_color(muted).child(alt).into_any_element();
    };
    let image = img(SharedString::from(src))
        .max_w_full()
        .object_fit(ObjectFit::Contain)
        .with_fallback(move || {
            div()
                .text_color(muted)
                .child(alt.clone())
                .into_any_element()
        });
    let image = match width {
        Some(Length::Pixels(value)) => image.w(px(value)),
        Some(Length::Percent(value)) => image.w(gpui_kit::relative(value / 100.0)),
        None => image,
    };
    let image = match height {
        Some(Length::Pixels(value)) => image.h(px(value)),
        Some(Length::Percent(value)) => image.h(gpui_kit::relative(value / 100.0)),
        None => image,
    };
    image.into_any_element()
}

/// h1 `text-lg`, h2 `text-base`, h3 `text-sm`, 모두 `font-semibold text-text-primary`. h4 이하는 본문 모양이다
fn render_heading(
    level: u8,
    content: &[Inline],
    context: &MarkdownContext,
    path: &str,
) -> AnyElement {
    let theme = &context.theme;
    let scale = match level {
        1 => text_scale::LG,
        2 => text_scale::BASE,
        3 => text_scale::SM,
        _ => crate::theme::TextScale {
            font_size: px(14.0),
            line_height: px(BODY_LINE_HEIGHT),
        },
    };
    let style = InlineStyle {
        css_weight: if level <= 3 { 600 } else { 400 },
        ..InlineStyle::plain(theme.text.primary)
    };
    let pieces = inline_pieces(content, &style, theme);
    let mut column = div().flex().flex_col().text_scale(scale);
    for (index, piece) in pieces.into_iter().enumerate() {
        column = column.child(render_piece(
            piece,
            context.id(&format!("{path}:{index}")),
            context,
        ));
    }
    column.into_any_element()
}

/// `ul list-disc pl-5`·`ol list-decimal pl-5`, 항목 `my-1`. 꽉 찬 항목은 문단 없는 글자 줄이라 여백이 없다
fn render_list(list: &List, context: &MarkdownContext, path: &str) -> AnyElement {
    let mut column = div().flex().flex_col();
    for (index, item) in list.items.iter().enumerate() {
        let marker = match list.is_ordered {
            true => format!("{}.", list.start + index as i64),
            false => "•".to_string(),
        };
        let body = render_blocks(&item.blocks, context, &format!("{path}l{index}"));
        // 항목 `my-1`은 이웃 항목과 겹쳐 4px이다
        let gap = if index == 0 { 0.0 } else { 4.0 };
        let row = div()
            .mt(px(gap))
            .flex()
            .child(list_marker(marker, &context.theme))
            .child(div().min_w_0().flex_1().child(body));
        column = column.child(row);
    }
    column.into_any_element()
}

/// 표시 기호는 글자 바깥 20px 칸의 오른쪽에 붙는다(브라우저 `list-style-position: outside`)
fn list_marker(marker: String, theme: &ThemeTokens) -> AnyElement {
    div()
        .flex_none()
        .w(px(20.0))
        .flex()
        .justify_end()
        .pr(px(10.0))
        .text_color(theme.text.primary)
        .child(marker)
        .into_any_element()
}

/// `kv-chat-code`: 언어 이름과 복사 버튼이 놓인 머리줄 + 옆으로 넘기는 코드
fn render_code_block(code: &CodeBlock, context: &MarkdownContext, path: &str) -> AnyElement {
    let theme = &context.theme;
    let copy = CodeCopyButton {
        id: context.id(&format!("{path}copy")),
        code: code.code.clone(),
        labels: context.labels.clone(),
        theme: *theme,
    };
    div()
        .overflow_hidden()
        .rounded(radius::LG)
        .border_1()
        .border_color(theme.border.subtle)
        .bg(theme.background.page)
        .child(
            div()
                .flex()
                .min_h(px(32.0))
                .items_center()
                .justify_between()
                .pl(spacing(3.0))
                .pr(spacing(1.0))
                .border_b_1()
                .border_color(theme.border.subtle)
                .text_size(px(12.0))
                .line_height(px(BODY_LINE_HEIGHT))
                .text_color(theme.text.secondary)
                .child(code.header_label.clone())
                .child(copy),
        )
        .child(
            div()
                .id(context.id(&format!("{path}scroll")))
                .overflow_x_scroll()
                .p(spacing(3.0))
                .text_size(px(CODE_FONT_SIZE))
                .line_height(px(MIXED_CODE_LINE_HEIGHT))
                .whitespace_nowrap()
                .child(SelectableRichText::new(
                    context.id(&format!("{path}code")),
                    highlighted_code_spans(code, theme),
                    context.selection_color(),
                )),
        )
        .into_any_element()
}

/// highlight.js 분류마다 `--color-syntax-*`를 입힌 코드 글자. `pre`는 마지막 줄바꿈을 빈 줄로 그리지 않는다
pub fn highlighted_code_spans(code: &CodeBlock, theme: &ThemeTokens) -> RichTextSpans {
    let base = code_text_run(theme.text.primary);
    let mut spans = RichTextSpans::default();
    let Some(highlight) = code.highlight.as_ref() else {
        spans.push(code.code.strip_suffix('\n').unwrap_or(&code.code), &base);
        return spans;
    };
    let last = highlight.len().saturating_sub(1);
    for (index, span) in highlight.iter().enumerate() {
        let text = match index == last {
            true => span.text.strip_suffix('\n').unwrap_or(&span.text),
            false => span.text.as_str(),
        };
        let color = span
            .class
            .and_then(|class| class.syntax_role())
            .map(|role| syntax_color(role, theme))
            .unwrap_or(theme.text.primary);
        spans.push(text, &code_text_run(color));
    }
    spans
}

fn code_text_run(color: gpui_kit::Rgba) -> gpui_kit::TextRun {
    gpui_kit::TextRun {
        len: 0,
        font: mono_font(400),
        color: color.into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

pub fn syntax_color(role: SyntaxRole, theme: &ThemeTokens) -> gpui_kit::Rgba {
    match role {
        SyntaxRole::Comment => theme.syntax.comment,
        SyntaxRole::Keyword => theme.syntax.keyword,
        SyntaxRole::String => theme.syntax.string,
        SyntaxRole::Number => theme.syntax.number,
        SyntaxRole::Title => theme.syntax.title,
        SyntaxRole::Deletion => theme.syntax.deletion,
    }
}

/// Markdown 본문 한 조각(`text-sm leading-6`)
#[derive(IntoElement)]
pub struct MarkdownBlocks {
    pub blocks: std::sync::Arc<Vec<Block>>,
    pub context: MarkdownContext,
    pub path: String,
}

impl RenderOnce for MarkdownBlocks {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .min_w_0()
            .text_size(px(14.0))
            .line_height(px(BODY_LINE_HEIGHT))
            .text_color(self.context.theme.text.primary)
            .child(render_blocks(&self.blocks, &self.context, &self.path))
    }
}

/// `<details>`(브라우저 기본 모양: 접힘 삼각형과 요약). 누르면 펼치고 접는다
#[derive(IntoElement)]
pub struct MarkdownDisclosure {
    disclosure: Disclosure,
    context: MarkdownContext,
    path: String,
}

impl MarkdownDisclosure {
    pub fn new(disclosure: Disclosure, context: MarkdownContext, path: &str) -> Self {
        Self {
            disclosure,
            context,
            path: path.to_string(),
        }
    }
}

impl RenderOnce for MarkdownDisclosure {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = self.context.id(&format!("{}details", self.path));
        let is_open_initially = self.disclosure.is_open;
        let is_open = window.use_keyed_state(id.clone(), cx, |_, _| is_open_initially);
        let is_expanded = *is_open.read(cx);
        let summary = match &self.disclosure.summary {
            Some(blocks) => render_blocks(blocks, &self.context, &format!("{}sum", self.path)),
            None => div().child("Details"),
        };
        let header = div()
            .id(id)
            .flex()
            .cursor_pointer()
            .child(
                div()
                    .flex_none()
                    .w(px(16.0))
                    .child(if is_expanded { "▾" } else { "▸" }),
            )
            .child(div().min_w_0().flex_1().child(summary))
            .on_click(move |_, _, cx| {
                is_open.update(cx, |state, cx| {
                    *state = !*state;
                    cx.notify();
                })
            });
        let body = is_expanded.then(|| {
            render_blocks(
                &self.disclosure.blocks,
                &self.context,
                &format!("{}body", self.path),
            )
        });
        div().flex().flex_col().child(header).children(body)
    }
}
