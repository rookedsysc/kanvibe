//! 릴리스 노트 Markdown(TS `ReleaseNotesContent`: marked + DOMPurify, `text-sm leading-6 text-text-secondary`).
//! `kanvibe-markdown`이 만든 블록을 그리고, 글자 꾸밈(굵게·기울임·코드·링크)은 한 줄 글자 안의 구간 스타일로 그린다.
//! HTML 원문은 받지 않으므로 스크립트·이벤트 속성이 끼어들 자리가 없다. 링크는 TS `target="_blank"`처럼 앱 링크 규칙으로 연다.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;

use gpui_kit::{
    AnyElement, Div, ElementId, FontStyle, Hsla, InteractiveText, IntoElement, ParentElement,
    SharedString, StrikethroughStyle, Styled, StyledText, TextRun, UnderlineStyle, div, img, px,
};
use kanvibe_markdown::{Block, Inline, List};

use crate::components::TailwindText;
use crate::fonts::{mono_font, sans_font};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// `text-sm leading-6`
const BODY_FONT_SIZE: f32 = 14.0;
const BODY_LINE_HEIGHT: f32 = 24.0;

pub(super) fn release_notes(theme: &ThemeTokens, markdown: &str) -> Div {
    let blocks = release_note_blocks(markdown);
    let count = blocks.len();
    div()
        .text_size(px(BODY_FONT_SIZE))
        .line_height(px(BODY_LINE_HEIGHT))
        .text_color(theme.text.secondary)
        .children(blocks.iter().enumerate().map(|(index, block)| {
            let is_first = index == 0;
            let is_last = index + 1 == count;
            render_block(theme, block, is_first, is_last)
        }))
}

/// 릴리스 노트는 대화용 파서가 아니라 TS 기본 marked와 같은 규칙(줄바꿈 유지 안 함, 수식 없음)으로 읽는다
fn release_note_blocks(markdown: &str) -> Vec<Block> {
    kanvibe_markdown::parse_release_notes(markdown)
}

/// 블록 위아래 여백(`[&_*:first-child]:mt-0 [&_*:last-child]:mb-0`)
fn spaced(element: Div, top: f32, bottom: f32, is_first: bool, is_last: bool) -> Div {
    element
        .mt(px(if is_first { 0.0 } else { top }))
        .mb(px(if is_last { 0.0 } else { bottom }))
}

fn render_block(theme: &ThemeTokens, block: &Block, is_first: bool, is_last: bool) -> AnyElement {
    match block {
        Block::Inlines(inlines) => div().child(inline_text(theme, inlines)).into_any_element(),
        Block::Preformatted(inlines) => spaced(
            div()
                .rounded(radius::MD)
                .border_1()
                .border_color(theme.border.subtle)
                .bg(theme.background.page)
                .p(spacing(3.0))
                .child(inline_text(theme, inlines)),
            16.0,
            16.0,
            is_first,
            is_last,
        )
        .into_any_element(),
        Block::Disclosure(disclosure) => {
            let shown: &[Block] = if disclosure.is_open {
                &disclosure.blocks
            } else {
                &[]
            };
            let summary = disclosure.summary.as_deref().unwrap_or(&[]);
            div()
                .children(
                    summary
                        .iter()
                        .chain(shown)
                        .map(|inner| render_block(theme, inner, false, false)),
                )
                .into_any_element()
        }
        Block::Paragraph(inlines) => spaced(
            div().child(inline_text(theme, inlines)),
            12.0,
            12.0,
            is_first,
            is_last,
        )
        .into_any_element(),
        Block::Heading { level, content } => heading(theme, *level, content, is_first, is_last),
        Block::CodeBlock(code) => spaced(
            div()
                .rounded(radius::MD)
                .border_1()
                .border_color(theme.border.subtle)
                .bg(theme.background.page)
                .p(spacing(3.0))
                .mono(400)
                .text_size(px(BODY_FONT_SIZE * 0.86))
                .child(code.code.trim_end_matches('\n').to_string()),
            16.0,
            16.0,
            is_first,
            is_last,
        )
        .into_any_element(),
        // 릴리스 노트 대화상자는 `align` 정렬을 따로 그리지 않고 안쪽 블록만 그린다
        Block::Aligned { blocks, .. } => div()
            .children(
                blocks
                    .iter()
                    .map(|inner| render_block(theme, inner, is_first, is_last)),
            )
            .into_any_element(),
        Block::BlockQuote(blocks) => spaced(
            div()
                .border_l_2()
                .border_color(theme.border.default)
                .pl(spacing(4.0))
                .text_color(theme.text.muted)
                .children(
                    blocks
                        .iter()
                        .map(|inner| render_block(theme, inner, false, false)),
                ),
            16.0,
            16.0,
            is_first,
            is_last,
        )
        .into_any_element(),
        Block::List(list) => {
            spaced(render_list(theme, list), 12.0, 12.0, is_first, is_last).into_any_element()
        }
        Block::Table(table) => spaced(
            div()
                .flex()
                .flex_col()
                .gap(spacing(2.0))
                .children(table.rows.iter().map(|row| {
                    div()
                        .flex()
                        .gap(spacing(2.0))
                        .children(row.cells.iter().map(|cell| {
                            div().flex_1().children(
                                cell.blocks
                                    .iter()
                                    .map(|inner| render_block(theme, inner, true, true)),
                            )
                        }))
                })),
            16.0,
            16.0,
            is_first,
            is_last,
        )
        .into_any_element(),
        Block::ThematicBreak => spaced(
            div().h(px(1.0)).bg(theme.border.subtle),
            20.0,
            20.0,
            is_first,
            is_last,
        )
        .into_any_element(),
    }
}

/// `h1 text-xl`, `h2 text-base`, `h3 text-sm`, 모두 `font-semibold text-text-primary`
fn heading(
    theme: &ThemeTokens,
    level: u8,
    content: &[Inline],
    is_first: bool,
    is_last: bool,
) -> AnyElement {
    let (scale, top, bottom) = match level {
        1 => (text_scale::XL, 24.0, 12.0),
        2 => (text_scale::BASE, 24.0, 12.0),
        _ => (text_scale::SM, 20.0, 8.0),
    };
    spaced(
        div()
            .text_scale(scale)
            .css_weight(600)
            .text_color(theme.text.primary)
            .child(Inline::plain_text(content)),
        top,
        bottom,
        is_first,
        is_last,
    )
    .into_any_element()
}

/// `ul list-disc pl-5`, `ol list-decimal pl-5`, `li my-1`
fn render_list(theme: &ThemeTokens, list: &List) -> Div {
    let items = list.items.iter().enumerate().map(|(index, item)| {
        let marker = if list.is_ordered {
            format!("{}.", list.start + index as i64)
        } else {
            "•".to_string()
        };
        div()
            .relative()
            .my(spacing(1.0))
            .child(list_marker(theme, list.is_ordered, marker))
            .children(item.blocks.iter().map(|block| match block {
                Block::Paragraph(inlines) => {
                    div().child(inline_text(theme, inlines)).into_any_element()
                }
                other => render_block(theme, other, true, true),
            }))
    });
    div().pl(spacing(5.0)).children(items)
}

/// `list-disc` 점(14px 글자 기준 5px 원)이나 `list-decimal` 번호
fn list_marker(theme: &ThemeTokens, is_ordered: bool, number: String) -> AnyElement {
    if is_ordered {
        return div()
            .absolute()
            .left(px(-20.0))
            .text_color(theme.text.secondary)
            .child(number)
            .into_any_element();
    }
    div()
        .absolute()
        .left(px(-17.0))
        .top(px(10.0))
        .size(px(5.0))
        .rounded(radius::FULL)
        .bg(theme.text.secondary)
        .into_any_element()
}

/// 글자 꾸밈 상태
#[derive(Clone, Copy)]
struct RunStyle {
    weight: u16,
    is_italic: bool,
    is_code: bool,
    is_link: bool,
    is_struck: bool,
}

/// 한 문단의 글자와 구간 스타일, 누를 수 있는 링크 구간과 그 주소
struct StyledRuns {
    text: String,
    runs: Vec<TextRun>,
    links: Vec<(Range<usize>, String)>,
}

fn styled_runs(theme: &ThemeTokens, inlines: &[Inline]) -> StyledRuns {
    let mut styled = StyledRuns {
        text: String::new(),
        runs: Vec::new(),
        links: Vec::new(),
    };
    let base = RunStyle {
        weight: 400,
        is_italic: false,
        is_code: false,
        is_link: false,
        is_struck: false,
    };
    push_inlines(theme, inlines, base, &mut styled);
    styled
}

fn inline_text(theme: &ThemeTokens, inlines: &[Inline]) -> AnyElement {
    let images: Vec<AnyElement> = collect_images(theme, inlines);
    let text = div().child(clickable_text(styled_runs(theme, inlines)));
    if images.is_empty() {
        text.into_any_element()
    } else {
        div().child(text).children(images).into_any_element()
    }
}

/// 링크가 있으면 링크 구간을 눌렀을 때 그 주소를 연다
fn clickable_text(styled: StyledRuns) -> AnyElement {
    let StyledRuns { text, runs, links } = styled;
    let element_id = links_element_id(&text, &links);
    let styled_text = StyledText::new(SharedString::from(text)).with_runs(runs);
    if links.is_empty() {
        return styled_text.into_any_element();
    }
    let (ranges, hrefs): (Vec<Range<usize>>, Vec<String>) = links.into_iter().unzip();
    InteractiveText::new(element_id, styled_text)
        .on_click(ranges, move |link_index, window, cx| {
            let origin_window = window.window_handle().window_id();
            crate::windows::open_href(&hrefs[link_index], Some(origin_window), cx);
        })
        .into_any_element()
}

/// 누를 수 있는 문단마다 다른 요소 id. 같은 대화상자 안 문단끼리 클릭 상태가 섞이지 않게 글과 주소로 만든다
fn links_element_id(text: &str, links: &[(Range<usize>, String)]) -> ElementId {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    links.hash(&mut hasher);
    ElementId::NamedInteger("release-note-links".into(), hasher.finish())
}

fn push_inlines(theme: &ThemeTokens, inlines: &[Inline], style: RunStyle, out: &mut StyledRuns) {
    for inline in inlines {
        match inline {
            Inline::Text(value) => push_run(theme, value, style, out),
            Inline::Code(value) => push_run(
                theme,
                value,
                RunStyle {
                    is_code: true,
                    ..style
                },
                out,
            ),
            Inline::Emphasis(inner) => push_inlines(
                theme,
                inner,
                RunStyle {
                    is_italic: true,
                    ..style
                },
                out,
            ),
            Inline::Strong(inner) => push_inlines(
                theme,
                inner,
                RunStyle {
                    weight: 600,
                    ..style
                },
                out,
            ),
            Inline::Monospace(inner) => push_inlines(
                theme,
                inner,
                RunStyle {
                    is_code: true,
                    ..style
                },
                out,
            ),
            Inline::Underline(inner)
            | Inline::Highlight(inner)
            | Inline::Subscript(inner)
            | Inline::Superscript(inner)
            | Inline::Small(inner)
            | Inline::Abbreviation { content: inner, .. } => push_inlines(theme, inner, style, out),
            Inline::Quote {
                open,
                close,
                content,
            } => {
                push_run(theme, &open.to_string(), style, out);
                push_inlines(theme, content, style, out);
                push_run(theme, &close.to_string(), style, out);
            }
            Inline::Strikethrough(inner) => push_inlines(
                theme,
                inner,
                RunStyle {
                    is_struck: true,
                    ..style
                },
                out,
            ),
            Inline::Link { href, content, .. } => {
                let start = out.text.len();
                let link_style = RunStyle {
                    is_link: true,
                    ..style
                };
                push_inlines(theme, content, link_style, out);
                if let Some(href) = href {
                    out.links.push((start..out.text.len(), href.clone()));
                }
            }
            Inline::LineBreak => push_run(theme, "\n", style, out),
            Inline::Image { .. } | Inline::Math { .. } | Inline::Checkbox { .. } => {}
        }
    }
}

fn push_run(theme: &ThemeTokens, value: &str, style: RunStyle, out: &mut StyledRuns) {
    if value.is_empty() {
        return;
    }
    let mut font = if style.is_code {
        mono_font(style.weight)
    } else {
        sans_font(style.weight)
    };
    if style.is_italic {
        font.style = FontStyle::Italic;
    }
    let color: Hsla = if style.is_link {
        theme.brand.primary.into()
    } else if style.weight >= 600 {
        theme.text.primary.into()
    } else {
        theme.text.secondary.into()
    };
    out.runs.push(TextRun {
        len: value.len(),
        font,
        color,
        background_color: style.is_code.then(|| theme.background.page.into()),
        underline: None::<UnderlineStyle>,
        strikethrough: style.is_struck.then(|| StrikethroughStyle {
            thickness: px(1.0),
            color: Some(color),
        }),
    });
    out.text.push_str(value);
}

/// 문단 안 이미지(`max-h-[420px] w-full rounded-md border object-contain`)
fn collect_images(theme: &ThemeTokens, inlines: &[Inline]) -> Vec<AnyElement> {
    inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Image { src: Some(src), .. } => Some(
                img(SharedString::from(src.clone()))
                    .w_full()
                    .max_h(px(420.0))
                    .rounded(radius::MD)
                    .border_1()
                    .border_color(theme.border.subtle)
                    .into_any_element(),
            ),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeMode;

    fn first_paragraph_runs(markdown: &str) -> StyledRuns {
        let theme = ThemeTokens::for_mode(ThemeMode::Dark);
        let blocks = release_note_blocks(markdown);
        let Some(Block::Paragraph(inlines)) = blocks.first() else {
            panic!("첫 블록은 문단이어야 한다: {blocks:?}");
        };
        styled_runs(&theme, inlines)
    }

    #[test]
    fn dollar_signs_in_release_notes_stay_as_text() {
        let runs = first_paragraph_runs("set $HOME and $PATH before running");

        assert_eq!(runs.text, "set $HOME and $PATH before running");
    }

    #[test]
    fn release_note_links_keep_their_address_on_the_link_text() {
        let runs = first_paragraph_runs("See [PR #509](https://github.com/o/r/pull/509) now");

        let (range, href) = runs.links.first().expect("링크 구간");
        assert_eq!(&runs.text[range.clone()], "PR #509");
        assert_eq!(href, "https://github.com/o/r/pull/509");
    }
}
