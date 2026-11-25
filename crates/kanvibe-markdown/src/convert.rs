//! pulldown-cmark 사건 → 블록·인라인 모델. marked(`gfm: true`, `breaks: true`)와 결과를 맞춘다.
//!
//! - 줄바꿈 하나(soft break)도 `<br>`처럼 줄을 바꾼다
//! - GFM 표·작업 목록·취소선(`~`, `~~`)과 `https://`·`http://`·`www.` 자동 링크
//! - 닫히지 않은 코드 펜스는 끝까지 코드이고, 코드 끝에는 marked처럼 줄바꿈 하나가 붙는다
//! - 인라인 HTML 태그는 [`crate::html`] 규칙으로 걸러 모델로 옮긴다

use std::iter::Peekable;

use pulldown_cmark::{
    Alignment as CmarkAlignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd,
};

use crate::html::{self, HtmlToken, InlineTag};
use crate::latex::{self, LatexSpan};
use crate::math::parse_tex;
use crate::model::{Alignment, Block, Inline, List, ListItem, Table};

pub fn markdown_options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS
}

struct Converter<'a, I: Iterator<Item = Event<'a>>> {
    events: Peekable<I>,
    spans: Vec<LatexSpan>,
    /// `<p>`로 감싼 문단 수. 목록 항목 안에서 늘면 느슨한 목록이다
    explicit_paragraphs: usize,
}

pub fn convert(markdown: &str) -> Vec<Block> {
    let options = markdown_options();
    let substituted = latex::substitute_latex(markdown, options);
    let parser = Parser::new_ext(&substituted.text, options);
    let mut converter = Converter {
        events: parser.peekable(),
        spans: substituted.spans,
        explicit_paragraphs: 0,
    };
    converter.blocks(None)
}

fn is_inline_event(event: &Event) -> bool {
    match event {
        Event::Text(_)
        | Event::Code(_)
        | Event::InlineHtml(_)
        | Event::SoftBreak
        | Event::HardBreak
        | Event::InlineMath(_) => true,
        Event::Start(tag) => matches!(
            tag,
            Tag::Emphasis
                | Tag::Strong
                | Tag::Strikethrough
                | Tag::Link { .. }
                | Tag::Image { .. }
                | Tag::Superscript
                | Tag::Subscript
        ),
        _ => false,
    }
}

impl<'a, I: Iterator<Item = Event<'a>>> Converter<'a, I> {
    /// `end`가 나올 때까지 블록을 모은다. 느슨하지 않은 목록 항목처럼 문단 없이 온 인라인은 문단으로 묶는다
    fn blocks(&mut self, end: Option<TagEnd>) -> Vec<Block> {
        let mut blocks = Vec::new();
        while let Some(event) = self.events.peek() {
            if let (Event::End(found), Some(expected)) = (event, end)
                && *found == expected
            {
                self.events.next();
                return blocks;
            }
            if is_inline_event(event) {
                let inlines = self.inlines(None);
                if !inlines.is_empty() {
                    blocks.push(Block::Paragraph(inlines));
                }
                continue;
            }
            let event = self.events.next().expect("peeked");
            if let Some(block) = self.block(event) {
                blocks.extend(block);
            }
        }
        blocks
    }

    fn block(&mut self, event: Event<'a>) -> Option<Vec<Block>> {
        let block = match event {
            Event::Start(Tag::Paragraph) => {
                self.explicit_paragraphs += 1;
                Block::Paragraph(self.inlines(Some(TagEnd::Paragraph)))
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let content = self.inlines(Some(TagEnd::Heading(level)));
                Block::Heading {
                    level: level as u8,
                    content,
                }
            }
            Event::Start(Tag::BlockQuote(kind)) => {
                Block::BlockQuote(self.blocks(Some(TagEnd::BlockQuote(kind))))
            }
            Event::Start(Tag::CodeBlock(kind)) => Block::CodeBlock(self.code_block(kind)),
            Event::Start(Tag::List(start)) => Block::List(self.list(start)),
            Event::Start(Tag::Table(alignments)) => Block::Table(self.table(&alignments)),
            Event::Start(Tag::HtmlBlock) => {
                return Some(html::html_block_to_blocks(&self.html_block()));
            }
            Event::Rule => Block::ThematicBreak,
            Event::Start(tag) => {
                let end = tag.to_end();
                return Some(self.blocks(Some(end)));
            }
            _ => return None,
        };
        Some(vec![block])
    }

    fn html_block(&mut self) -> String {
        let mut source = String::new();
        for event in self.events.by_ref() {
            match event {
                Event::End(TagEnd::HtmlBlock) => break,
                Event::Html(text) | Event::Text(text) => source.push_str(&text),
                _ => {}
            }
        }
        source
    }

    fn code_block(&mut self, kind: CodeBlockKind<'a>) -> crate::model::CodeBlock {
        let mut code = String::new();
        for event in self.events.by_ref() {
            match event {
                Event::End(TagEnd::CodeBlock) => break,
                Event::Text(text) => code.push_str(&text),
                _ => {}
            }
        }
        let language = match kind {
            CodeBlockKind::Fenced(info) => info.split_whitespace().next().map(str::to_string),
            CodeBlockKind::Indented => None,
        };
        let code = format!("{}\n", code.strip_suffix('\n').unwrap_or(&code));
        crate::code_block(language, restore_placeholders(&code, &self.spans))
    }

    fn list(&mut self, start: Option<u64>) -> List {
        let mut items = Vec::new();
        let paragraphs_before = self.explicit_paragraphs;
        while let Some(event) = self.events.next() {
            match event {
                Event::End(TagEnd::List(_)) => break,
                Event::Start(Tag::Item) => {
                    let task = match self.events.peek() {
                        Some(Event::TaskListMarker(checked)) => {
                            let checked = *checked;
                            self.events.next();
                            Some(checked)
                        }
                        _ => None,
                    };
                    let blocks = self.blocks(Some(TagEnd::Item));
                    items.push(ListItem { task, blocks });
                }
                _ => {}
            }
        }
        let is_tight = self.explicit_paragraphs == paragraphs_before;
        List {
            is_ordered: start.is_some(),
            start: start.unwrap_or(1),
            is_tight,
            items,
        }
    }

    fn table(&mut self, alignments: &[CmarkAlignment]) -> Table {
        let alignments = alignments
            .iter()
            .map(|alignment| match alignment {
                CmarkAlignment::None => Alignment::None,
                CmarkAlignment::Left => Alignment::Left,
                CmarkAlignment::Center => Alignment::Center,
                CmarkAlignment::Right => Alignment::Right,
            })
            .collect();
        let mut table = Table {
            alignments,
            header: Vec::new(),
            rows: Vec::new(),
        };
        let mut row: Vec<Vec<Inline>> = Vec::new();
        while let Some(event) = self.events.next() {
            match event {
                Event::End(TagEnd::Table) => break,
                Event::Start(Tag::TableCell) => row.push(self.inlines(Some(TagEnd::TableCell))),
                Event::End(TagEnd::TableHead) => table.header = std::mem::take(&mut row),
                Event::End(TagEnd::TableRow) => table.rows.push(std::mem::take(&mut row)),
                _ => {}
            }
        }
        table
    }

    /// `end`가 나올 때까지 인라인을 모은다. `end`가 없으면 다음 블록 사건 앞에서 멈춘다
    fn inlines(&mut self, end: Option<TagEnd>) -> Vec<Inline> {
        let mut builder = InlineBuilder::default();
        while let Some(event) = self.events.peek() {
            if let Event::End(found) = event
                && Some(*found) == end
            {
                self.events.next();
                break;
            }
            if end.is_none() && !is_inline_event(event) {
                break;
            }
            let event = self.events.next().expect("peeked");
            self.inline_event(&mut builder, event);
        }
        finish_inlines(builder.finish(), &self.spans)
    }

    fn inline_event(&mut self, builder: &mut InlineBuilder, event: Event<'a>) {
        match event {
            Event::Text(text) => builder.text(&text),
            Event::Code(code) => {
                builder.push(Inline::Code(restore_placeholders(&code, &self.spans)))
            }
            Event::SoftBreak | Event::HardBreak => builder.push(Inline::LineBreak),
            Event::InlineHtml(source) | Event::Html(source) => builder.html(&source),
            Event::Start(tag) => self.inline_container(builder, tag),
            _ => {}
        }
    }

    fn inline_container(&mut self, builder: &mut InlineBuilder, tag: Tag<'a>) {
        let end = tag.to_end();
        let children = self.raw_inlines(end);
        let inline = match tag {
            Tag::Emphasis => Inline::Emphasis(children),
            Tag::Strong => Inline::Strong(children),
            Tag::Strikethrough => Inline::Strikethrough(children),
            Tag::Link {
                dest_url, title, ..
            } => {
                let href = if html::is_safe_url(&dest_url, false) {
                    dest_url.to_string()
                } else {
                    String::new()
                };
                let title = (!title.is_empty()).then(|| title.to_string());
                Inline::Link {
                    href,
                    title,
                    content: children,
                }
            }
            Tag::Image {
                dest_url, title, ..
            } => {
                let src = if html::is_safe_url(&dest_url, true) {
                    dest_url.to_string()
                } else {
                    String::new()
                };
                let title = (!title.is_empty()).then(|| title.to_string());
                Inline::Image {
                    src,
                    alt: Inline::plain_text(&children),
                    title,
                }
            }
            _ => {
                builder.extend(children);
                return;
            }
        };
        builder.push(inline);
    }

    /// 강조·링크 안쪽: 자리표시자·자동 링크 처리는 바깥에서 한 번에 한다
    fn raw_inlines(&mut self, end: TagEnd) -> Vec<Inline> {
        let mut builder = InlineBuilder::default();
        while let Some(event) = self.events.next() {
            if matches!(&event, Event::End(found) if *found == end) {
                break;
            }
            self.inline_event(&mut builder, event);
        }
        builder.finish()
    }
}

/// 인라인 HTML 태그 짝을 맞추며 인라인을 쌓는다
#[derive(Default)]
struct InlineBuilder {
    current: Vec<Inline>,
    stack: Vec<(String, InlineTag, Vec<Inline>)>,
    dropped: Vec<String>,
}

impl InlineBuilder {
    fn push(&mut self, inline: Inline) {
        if self.dropped.is_empty() {
            self.current.push(inline);
        }
    }

    fn extend(&mut self, inlines: Vec<Inline>) {
        for inline in inlines {
            self.push(inline);
        }
    }

    fn text(&mut self, text: &str) {
        self.push(Inline::Text(text.to_string()));
    }

    fn html(&mut self, source: &str) {
        for token in html::tokenize(source) {
            self.html_token(token);
        }
    }

    fn html_token(&mut self, token: HtmlToken) {
        if let Some(open) = self.dropped.last().cloned() {
            match token {
                HtmlToken::End { name } if name == open => {
                    self.dropped.pop();
                }
                HtmlToken::Start {
                    name,
                    is_self_closing: false,
                    ..
                } if name == open => self.dropped.push(name),
                _ => {}
            }
            return;
        }
        match token {
            HtmlToken::Text(text) => self.text(&text),
            HtmlToken::Start {
                name,
                attributes,
                is_self_closing,
            } => self.open_tag(name, &attributes, is_self_closing),
            HtmlToken::End { name } => self.close(&name),
        }
    }

    fn open_tag(&mut self, name: String, attributes: &[(String, String)], is_self_closing: bool) {
        if html::is_dropped_with_content(&name) {
            if !is_self_closing {
                self.dropped.push(name);
            }
            return;
        }
        if let Some(inline) = html::void_inline(&name, attributes) {
            self.push(inline);
            return;
        }
        if !is_self_closing {
            let tag = html::inline_tag(&name, attributes);
            self.stack
                .push((name, tag, std::mem::take(&mut self.current)));
        }
    }

    fn close(&mut self, name: &str) {
        let Some(position) = self.stack.iter().rposition(|(open, _, _)| open == name) else {
            return;
        };
        while self.stack.len() > position {
            let (_, tag, outer) = self.stack.pop().expect("checked");
            let children = std::mem::replace(&mut self.current, outer);
            self.current.extend(html::wrap_inline(&tag, children));
        }
    }

    /// 닫히지 않은 태그는 브라우저처럼 끝에서 닫는다
    fn finish(mut self) -> Vec<Inline> {
        while let Some((_, tag, outer)) = self.stack.pop() {
            let children = std::mem::replace(&mut self.current, outer);
            self.current.extend(html::wrap_inline(&tag, children));
        }
        self.current
    }
}

fn restore_placeholders(text: &str, spans: &[LatexSpan]) -> String {
    latex::split_placeholders(text)
        .into_iter()
        .map(|part| match part {
            Ok(text) => text,
            Err(index) => spans
                .get(index)
                .map(|span| span.raw.clone())
                .unwrap_or_default(),
        })
        .collect()
}

fn math_inline(span: &LatexSpan) -> Inline {
    match parse_tex(&span.formula) {
        Ok(node) => Inline::Math {
            tex: span.formula.clone(),
            display: span.is_display,
            node,
        },
        Err(_) => Inline::Code(span.raw.clone()),
    }
}

/// 이웃한 글자를 합치고, 수식 자리표시자를 되살리고, 링크 밖 글자에 자동 링크를 건다
fn finish_inlines(inlines: Vec<Inline>, spans: &[LatexSpan]) -> Vec<Inline> {
    let mut merged: Vec<Inline> = Vec::new();
    for inline in inlines {
        let inline = match inline {
            Inline::Emphasis(children) => Inline::Emphasis(finish_inlines(children, spans)),
            Inline::Strong(children) => Inline::Strong(finish_inlines(children, spans)),
            Inline::Strikethrough(children) => {
                Inline::Strikethrough(finish_inlines(children, spans))
            }
            Inline::Link {
                href,
                title,
                content,
            } => Inline::Link {
                href,
                title,
                content: finish_link_text(content, spans),
            },
            other => other,
        };
        match (merged.last_mut(), inline) {
            (Some(Inline::Text(previous)), Inline::Text(next)) => previous.push_str(&next),
            (_, inline) => merged.push(inline),
        }
    }
    merged
        .into_iter()
        .flat_map(|inline| expand_text(inline, spans))
        .collect()
}

fn finish_link_text(content: Vec<Inline>, spans: &[LatexSpan]) -> Vec<Inline> {
    finish_inlines(content, spans)
        .into_iter()
        .map(|inline| match inline {
            Inline::Link { content, .. } => Inline::Text(Inline::plain_text(&content)),
            other => other,
        })
        .collect()
}

fn expand_text(inline: Inline, spans: &[LatexSpan]) -> Vec<Inline> {
    let Inline::Text(text) = inline else {
        return vec![inline];
    };
    let mut result = Vec::new();
    for part in latex::split_placeholders(&text) {
        match part {
            Ok(plain) => result.extend(crate::autolink::link_urls(&plain)),
            Err(index) => result.extend(spans.get(index).map(math_inline)),
        }
    }
    result
}
