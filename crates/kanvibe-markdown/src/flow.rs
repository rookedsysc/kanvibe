//! 정화한 요소 나무를 블록·인라인 모델로 옮긴다. 브라우저가 TS 화면에 그리는 모양을 따른다.
//!
//! - 블록 요소 사이에 놓인 글자·인라인 요소는 이름 없는 글자 줄([`Block::Inlines`])로 묶는다.
//! - 뜻이 없는 블록 요소(`div`, `section`, `dl` …)와 꾸밈 없는 인라인 요소(`span`, `font`, `title` 없는 `abbr` …)는
//!   태그만 벗기고 내용을 그 자리에 둔다. 블록을 품은 인라인 요소도 블록 자리에서 태그를 벗긴다. 다만 `align`을
//!   받은 `p`·`div`·제목과 `center`는 정렬 묶음([`Block::Aligned`])으로 감싼다.
//! - 그려지지 않는 요소(`template`, `audio`, `video`, `hidden` 속성, `open` 없는 `dialog` …)는 내용까지 뺀다.
//! - `pre` 밖 글자는 CSS `white-space: normal`처럼 공백을 하나로 접고 줄 머리·끝 공백을 지운다.

use crate::math::parse_tex;
use std::cell::Cell;

use crate::model::{
    Alignment, Block, Disclosure, Inline, Length, List, ListItem, Table, TableCaption, TableCell,
    TableRow,
};
use crate::sanitize::{HtmlElement, HtmlNode};
use crate::scan::{self, InlineSpan};

const BLOCK_ELEMENTS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "caption",
    "center",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dd",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "legend",
    "li",
    "main",
    "menu",
    "nav",
    "ol",
    "p",
    "pre",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
];

const HIDDEN_ELEMENTS: &[&str] = &[
    "area", "audio", "canvas", "col", "colgroup", "datalist", "meter", "progress", "rp", "select",
    "source", "template", "track", "video",
];

/// 정화한 마디 목록을 블록 모델로 옮긴다. `spans`는 글자 속 자리표시자가 가리키는 수식이다
pub fn blocks(nodes: &[HtmlNode], spans: &[InlineSpan]) -> Vec<Block> {
    FlowBuilder {
        spans,
        quote_depth: Cell::new(0),
    }
    .blocks(nodes)
}

/// 문서 언어 `ko`의 따옴표 쌍. 두 겹보다 깊은 인용은 마지막 쌍을 쓴다
const QUOTE_MARKS: [(char, char); 2] = [('\u{201C}', '\u{201D}'), ('\u{2018}', '\u{2019}')];

struct FlowBuilder<'a> {
    spans: &'a [InlineSpan],
    /// 지금 옮기는 글자를 감싼 `q` 수. CSS `open-quote`가 고르는 따옴표 쌍을 정한다
    quote_depth: Cell<usize>,
}

fn element(node: &HtmlNode) -> Option<&HtmlElement> {
    match node {
        HtmlNode::Element(element) => Some(element),
        HtmlNode::Text(_) => None,
    }
}

fn is_hidden(element: &HtmlElement) -> bool {
    HIDDEN_ELEMENTS.contains(&element.name.as_str())
        || element.attribute("hidden").is_some()
        || (element.name == "dialog" && element.attribute("open").is_none())
        || (element.name == "input" && !is_checkbox(element))
}

fn is_checkbox(element: &HtmlElement) -> bool {
    element
        .attribute("type")
        .is_some_and(|kind| kind.eq_ignore_ascii_case("checkbox"))
}

/// 블록 요소이거나, 블록 요소를 품은 인라인 요소(수식 제외)
fn is_block_level(node: &HtmlNode) -> bool {
    let Some(element) = element(node) else {
        return false;
    };
    if is_hidden(element) || element.name == "math" {
        return false;
    }
    BLOCK_ELEMENTS.contains(&element.name.as_str()) || element.children.iter().any(is_block_level)
}

impl FlowBuilder<'_> {
    fn blocks(&self, nodes: &[HtmlNode]) -> Vec<Block> {
        let mut blocks = Vec::new();
        let mut line: Vec<&HtmlNode> = Vec::new();
        for node in nodes {
            if element(node).is_some_and(is_hidden) {
                continue;
            }
            if !is_block_level(node) {
                line.push(node);
                continue;
            }
            self.flush_line(&mut line, &mut blocks);
            if let Some(element) = element(node) {
                blocks.extend(self.block(element));
            }
        }
        self.flush_line(&mut line, &mut blocks);
        blocks
    }

    fn flush_line(&self, line: &mut Vec<&HtmlNode>, blocks: &mut Vec<Block>) {
        let content = self.line_box(line.drain(..));
        if !content.is_empty() {
            blocks.push(Block::Inlines(content));
        }
    }

    fn block(&self, element: &HtmlElement) -> Vec<Block> {
        let blocks = self.element_blocks(element);
        match block_alignment(element) {
            Some(alignment) if !blocks.is_empty() => vec![Block::Aligned { alignment, blocks }],
            _ => blocks,
        }
    }

    fn element_blocks(&self, element: &HtmlElement) -> Vec<Block> {
        let block = match element.name.as_str() {
            "p" => Block::Paragraph(self.line_box(element.children.iter())),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Block::Heading {
                level: element.name[1..].parse().unwrap_or(1),
                content: self.line_box(element.children.iter()),
            },
            "blockquote" => Block::BlockQuote(self.blocks(&element.children)),
            "pre" => self.preformatted(element),
            "ul" | "ol" => Block::List(self.list(element)),
            "table" => Block::Table(self.table(element)),
            "details" => Block::Disclosure(self.disclosure(element)),
            "hr" => Block::ThematicBreak,
            _ => return self.blocks(&element.children),
        };
        let is_empty_text_block = matches!(&block, Block::Paragraph(content) | Block::Heading { content, .. } if content.is_empty());
        if is_empty_text_block {
            return Vec::new();
        }
        vec![block]
    }

    /// 첫 요소가 `<code>`면 TS `decorateCodeBlocks`처럼 코드 블록으로 꾸민다
    fn preformatted(&self, pre: &HtmlElement) -> Block {
        let first_element = pre.children.iter().find_map(element);
        let Some(code) = first_element.filter(|child| child.name == "code") else {
            let mut content = Vec::new();
            for child in &pre.children {
                self.inline(child, &mut content);
            }
            return Block::Preformatted(content);
        };
        let language = code.attribute("class").and_then(|class| {
            class
                .split_ascii_whitespace()
                .find_map(|name| name.strip_prefix("language-"))
                .map(str::to_string)
        });
        Block::CodeBlock(crate::code_block(
            language,
            self.text_content(&pre.children),
        ))
    }

    fn list(&self, element: &HtmlElement) -> List {
        let mut items: Vec<ListItem> = Vec::new();
        for child in &element.children {
            let blocks = match child {
                HtmlNode::Element(item) if item.name == "li" => {
                    items.push(ListItem {
                        blocks: self.blocks(&item.children),
                    });
                    continue;
                }
                other => self.blocks(std::slice::from_ref(other)),
            };
            if blocks.is_empty() {
                continue;
            }
            match items.last_mut() {
                Some(item) => item.blocks.extend(blocks),
                None => items.push(ListItem { blocks }),
            }
        }
        List {
            is_ordered: element.name == "ol",
            start: element
                .attribute("start")
                .and_then(parse_integer)
                .unwrap_or(1),
            items,
        }
    }

    fn table(&self, table: &HtmlElement) -> Table {
        let mut captions = Vec::new();
        let mut rows = Vec::new();
        for child in table.children.iter().filter_map(element) {
            match child.name.as_str() {
                "caption" => captions.push(TableCaption {
                    is_below: child
                        .attribute("align")
                        .is_some_and(|side| side.eq_ignore_ascii_case("bottom")),
                    blocks: self.blocks(&child.children),
                }),
                "thead" | "tbody" | "tfoot" => {
                    let section_alignment = alignment(child.attribute("align"));
                    rows.extend(
                        child
                            .children
                            .iter()
                            .filter_map(element)
                            .filter(|row| row.name == "tr")
                            .map(|row| self.table_row(row, section_alignment)),
                    )
                }
                "tr" => rows.push(self.table_row(child, Alignment::None)),
                _ => {}
            }
        }
        Table { captions, rows }
    }

    /// 칸은 자기 `align`이 없으면 행, 그다음 행 묶음(`section_alignment`)의 `align`을 물려받는다
    fn table_row(&self, row: &HtmlElement, section_alignment: Alignment) -> TableRow {
        let row_alignment = inherit_alignment(alignment(row.attribute("align")), section_alignment);
        let cells = row
            .children
            .iter()
            .filter_map(element)
            .filter(|cell| matches!(cell.name.as_str(), "td" | "th"))
            .map(|cell| TableCell {
                is_header: cell.name == "th",
                alignment: inherit_alignment(alignment(cell.attribute("align")), row_alignment),
                column_span: span_attribute(cell.attribute("colspan"), 1000),
                row_span: span_attribute(cell.attribute("rowspan"), 65534),
                blocks: self.blocks(&cell.children),
            })
            .collect();
        TableRow { cells }
    }

    /// 브라우저는 `details`의 첫 `summary` 자식만 제목으로 쓴다
    fn disclosure(&self, details: &HtmlElement) -> Disclosure {
        let summary_index = details
            .children
            .iter()
            .position(|child| element(child).is_some_and(|child| child.name == "summary"));
        let summary = summary_index
            .and_then(|index| element(&details.children[index]))
            .map(|summary| self.blocks(&summary.children));
        let rest: Vec<HtmlNode> = details
            .children
            .iter()
            .enumerate()
            .filter(|(index, _)| Some(*index) != summary_index)
            .map(|(_, child)| child.clone())
            .collect();
        Disclosure {
            summary,
            is_open: details.attribute("open").is_some(),
            blocks: self.blocks(&rest),
        }
    }
}

impl FlowBuilder<'_> {
    /// 한 줄 상자(문단·제목·이름 없는 글자 줄)의 인라인
    fn line_box<'n>(&self, nodes: impl Iterator<Item = &'n HtmlNode>) -> Vec<Inline> {
        let mut content = Vec::new();
        for node in nodes {
            self.inline(node, &mut content);
        }
        collapse_white_space(content)
    }

    fn inline(&self, node: &HtmlNode, output: &mut Vec<Inline>) {
        let element = match node {
            HtmlNode::Text(text) => return self.text(text, output),
            HtmlNode::Element(element) if is_hidden(element) => return,
            HtmlNode::Element(element) => element,
        };
        let children = || self.inline_children(element);
        let inline = match element.name.as_str() {
            "br" => Inline::LineBreak,
            "img" => image(element),
            "input" => Inline::Checkbox {
                is_checked: element.attribute("checked").is_some(),
            },
            "a" => Inline::Link {
                href: element.attribute("href").map(str::to_string),
                title: element.attribute("title").map(str::to_string),
                content: children(),
            },
            "code" => Inline::Code(self.text_content(&element.children)),
            "math" => Inline::Text(self.text_content(&element.children)),
            "em" | "i" | "cite" | "var" | "dfn" => Inline::Emphasis(children()),
            "strong" | "b" => Inline::Strong(children()),
            "del" | "s" | "strike" => Inline::Strikethrough(children()),
            "u" | "ins" => Inline::Underline(children()),
            "mark" => Inline::Highlight(children()),
            "sub" => Inline::Subscript(children()),
            "sup" => Inline::Superscript(children()),
            "kbd" | "samp" | "tt" => Inline::Monospace(children()),
            "small" => Inline::Small(children()),
            "q" => self.quote(element),
            "abbr" | "acronym" if element.attribute("title").is_some() => Inline::Abbreviation {
                title: element.attribute("title").unwrap_or("").to_string(),
                content: children(),
            },
            _ => {
                for child in &element.children {
                    self.inline(child, output);
                }
                return;
            }
        };
        output.push(inline);
    }

    /// 안쪽 `q`가 다음 따옴표 쌍을 고르도록 깊이를 하나 늘린 채 내용을 옮긴다
    fn quote(&self, element: &HtmlElement) -> Inline {
        let depth = self.quote_depth.get();
        let (open, close) = QUOTE_MARKS[depth.min(QUOTE_MARKS.len() - 1)];
        self.quote_depth.set(depth + 1);
        let content = self.inline_children(element);
        self.quote_depth.set(depth);
        Inline::Quote {
            open,
            close,
            content,
        }
    }

    fn inline_children(&self, element: &HtmlElement) -> Vec<Inline> {
        let mut content = Vec::new();
        for child in &element.children {
            self.inline(child, &mut content);
        }
        content
    }

    /// 수식 자리표시자는 KaTeX가 그리는 수식으로, 못 그리는 수식은 TS처럼 원문 코드로 바꾼다
    fn text(&self, text: &str, output: &mut Vec<Inline>) {
        for part in scan::split_placeholders(text) {
            let inline = match part.map_err(|index| self.spans.get(index)) {
                Ok(plain) => Inline::Text(plain.to_string()),
                Err(Some(InlineSpan::Latex(span))) => match parse_tex(&span.formula) {
                    Ok(node) => Inline::Math {
                        tex: span.formula.clone(),
                        display: span.is_display,
                        node,
                    },
                    Err(_) => Inline::Code(span.raw.clone()),
                },
                Err(Some(InlineSpan::Autolink(link))) => Inline::Text(link.text.clone()),
                Err(None) => continue,
            };
            output.push(inline);
        }
    }

    /// DOM `textContent`. 수식 자리표시자는 원문으로 되살린다
    fn text_content(&self, nodes: &[HtmlNode]) -> String {
        let mut content = String::new();
        for node in nodes {
            match node {
                HtmlNode::Text(text) => content.push_str(&self.restore_placeholders(text)),
                HtmlNode::Element(element) => {
                    content.push_str(&self.text_content(&element.children))
                }
            }
        }
        content
    }

    fn restore_placeholders(&self, text: &str) -> String {
        scan::split_placeholders(text)
            .into_iter()
            .map(|part| match part.map_err(|index| self.spans.get(index)) {
                Ok(plain) => plain.to_string(),
                Err(Some(InlineSpan::Latex(span))) => span.raw.clone(),
                Err(Some(InlineSpan::Autolink(link))) => link.text.clone(),
                Err(None) => String::new(),
            })
            .collect()
    }
}

fn image(element: &HtmlElement) -> Inline {
    Inline::Image {
        src: element
            .attribute("src")
            .filter(|src| !src.is_empty())
            .map(str::to_string),
        alt: element.attribute("alt").unwrap_or("").to_string(),
        title: element.attribute("title").map(str::to_string),
        width: element.attribute("width").and_then(parse_length),
        height: element.attribute("height").and_then(parse_length),
    }
}

/// Chromium이 `text-align`으로 바꾸는 `align` 값. 그 밖의 값은 무시한다
fn alignment(value: Option<&str>) -> Alignment {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("left") => Alignment::Left,
        Some("center" | "middle") => Alignment::Center,
        Some("right") => Alignment::Right,
        Some("justify") => Alignment::Justify,
        _ => Alignment::None,
    }
}

fn inherit_alignment(own: Alignment, parent: Alignment) -> Alignment {
    if own == Alignment::None { parent } else { own }
}

/// 블록 요소 가운데 `align`을 `text-align`으로 받는 것(`p`·`div`·제목)과, UA CSS가 늘 가운데 맞추는 `center`
fn block_alignment(element: &HtmlElement) -> Option<Alignment> {
    let alignment = match element.name.as_str() {
        "center" => Alignment::Center,
        "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            alignment(element.attribute("align"))
        }
        _ => return None,
    };
    (alignment != Alignment::None).then_some(alignment)
}

/// HTML 정수 읽기 규칙: 앞 공백, 부호, 숫자까지만 읽고 나머지는 버린다
fn parse_integer(value: &str) -> Option<i64> {
    let trimmed = value.trim_start_matches(|character: char| character.is_ascii_whitespace());
    let (sign, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let length = digits.bytes().take_while(u8::is_ascii_digit).count();
    digits[..length]
        .parse::<i64>()
        .ok()
        .map(|number| sign * number)
}

/// `colspan`·`rowspan`: 읽지 못하거나 0이면 1, 브라우저 최댓값에서 자른다
fn span_attribute(value: Option<&str>, max: i64) -> u32 {
    value
        .and_then(parse_integer)
        .filter(|number| *number > 0)
        .map_or(1, |number| number.min(max) as u32)
}

/// HTML 크기 값 읽기 규칙: 숫자(소수 포함) 뒤가 `%`면 백분율, 아니면 픽셀
fn parse_length(value: &str) -> Option<Length> {
    let trimmed = value.trim_start_matches(|character: char| character.is_ascii_whitespace());
    let length = trimmed
        .bytes()
        .take_while(|byte| byte.is_ascii_digit() || *byte == b'.')
        .count();
    let number = trimmed[..length].parse::<f32>().ok()?;
    if trimmed[length..].starts_with('%') {
        Some(Length::Percent(number))
    } else {
        Some(Length::Pixels(number))
    }
}

fn is_collapsible_space(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}

/// CSS `white-space: normal`: 공백 묶음을 한 칸으로 접고, 줄 머리·`<br>` 앞뒤·줄 끝 공백을 지운다.
/// 블록 끝의 `<br>`은 브라우저처럼 빈 줄을 더 만들지 않는다
fn collapse_white_space(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut is_after_space = true;
    let mut collapsed = collapse_sequence(inlines, &mut is_after_space);
    trim_line_end(&mut collapsed);
    if collapsed.len() > 1 && collapsed.last() == Some(&Inline::LineBreak) {
        collapsed.pop();
        trim_line_end(&mut collapsed);
    }
    collapsed
}

fn collapse_text(text: &str, is_after_space: &mut bool) -> String {
    let mut collapsed = String::with_capacity(text.len());
    for character in text.chars() {
        if !is_collapsible_space(character) {
            collapsed.push(character);
            *is_after_space = false;
        } else if !*is_after_space {
            collapsed.push(' ');
            *is_after_space = true;
        }
    }
    collapsed
}

fn collapse_sequence(inlines: Vec<Inline>, is_after_space: &mut bool) -> Vec<Inline> {
    let mut output = Vec::new();
    for inline in inlines {
        match inline {
            Inline::Text(text) => push_text(&mut output, collapse_text(&text, is_after_space)),
            Inline::Code(text) => output.push(Inline::Code(collapse_text(&text, is_after_space))),
            Inline::LineBreak => {
                trim_line_end(&mut output);
                output.push(Inline::LineBreak);
                *is_after_space = true;
            }
            Inline::Quote {
                open,
                close,
                content,
            } => output.push(collapse_quote(open, close, content, is_after_space)),
            mut container if container.children().is_some() => {
                let is_empty = container.children_mut().is_none_or(|children| {
                    *children = collapse_sequence(std::mem::take(children), is_after_space);
                    children.is_empty()
                });
                if !is_empty {
                    output.push(container);
                }
            }
            atomic => {
                output.push(atomic);
                *is_after_space = false;
            }
        }
    }
    output
}

/// 따옴표는 공백이 아닌 글자라서 앞뒤 공백을 지우지 못한다. 비어 있어도 따옴표는 남는다
fn collapse_quote(
    open: char,
    close: char,
    content: Vec<Inline>,
    is_after_space: &mut bool,
) -> Inline {
    *is_after_space = false;
    let content = collapse_sequence(content, is_after_space);
    *is_after_space = false;
    Inline::Quote {
        open,
        close,
        content,
    }
}

/// 지운 주석·벗긴 태그 때문에 나뉜 글자 마디를 하나로 붙인다
fn push_text(output: &mut Vec<Inline>, text: String) {
    if text.is_empty() {
        return;
    }
    match output.last_mut() {
        Some(Inline::Text(previous)) => previous.push_str(&text),
        _ => output.push(Inline::Text(text)),
    }
}

/// 줄 끝 공백을 안쪽 꾸밈 묶음까지 지운다. 비게 된 글자·묶음은 뺀다(코드 상자는 비어도 남는다)
fn trim_line_end(inlines: &mut Vec<Inline>) {
    while let Some(last) = inlines.last_mut() {
        let is_emptied = match last {
            Inline::Text(text) => {
                text.truncate(text.trim_end_matches(' ').len());
                text.is_empty()
            }
            Inline::Code(text) => {
                text.truncate(text.trim_end_matches(' ').len());
                false
            }
            Inline::Quote { .. } => false,
            other => other.children_mut().is_some_and(|children| {
                trim_line_end(children);
                children.is_empty()
            }),
        };
        if !is_emptied {
            return;
        }
        inlines.pop();
    }
}
