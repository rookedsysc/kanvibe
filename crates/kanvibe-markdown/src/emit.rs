//! pulldown-cmark 사건을 marked 렌더러(`Renderer.ts`)가 만드는 것과 같은 HTML 문자열로 쓴다.
//!
//! 브라우저는 이 문자열 전체를 한 번에 파싱하므로, Markdown 사이에 섞인 원시 HTML(`<details>` 여는 태그와
//! 닫는 태그 사이의 Markdown 문단 등)도 TS와 같은 요소 나무가 된다. 수식 자리표시자는 글자로 그대로 두고,
//! 자동 링크 자리표시자는 marked처럼 `<a href>`로 쓴다.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};

use crate::scan::{self, InlineSpan};

pub fn markdown_options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS
}

/// marked `escape(html, true)`: 이미 푼 글자의 `&<>"'`를 모두 엔티티로 바꾼다
fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

/// marked `escape(html)`: 원문 글자에 이미 있는 엔티티(`&amp;`, `&#38;` …)는 그대로 둔다
fn escape_keeping_entities(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for (index, character) in text.char_indices() {
        if character == '&' && starts_entity(&text[index + 1..]) {
            escaped.push('&');
        } else {
            escaped.push_str(&escape_html(character.encode_utf8(&mut [0; 4])));
        }
    }
    escaped
}

/// `&` 뒤가 `#\d{1,7};`·`#[Xx][a-fA-F0-9]{1,6};`·`\w+;`인지
fn starts_entity(rest: &str) -> bool {
    let Some(end) = rest.find(';') else {
        return false;
    };
    let name = &rest[..end];
    if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
        return (1..=6).contains(&hex.len()) && hex.bytes().all(|byte| byte.is_ascii_hexdigit());
    }
    if let Some(digits) = name.strip_prefix('#') {
        return (1..=7).contains(&digits.len()) && digits.bytes().all(|byte| byte.is_ascii_digit());
    }
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// marked `cleanUrl`: `encodeURI` 뒤 `%25`를 `%`로 되돌린다
fn clean_url(url: &str) -> String {
    const URI_UNESCAPED: &[u8] = b";,/?:@&=+$-_.!~*'()#";
    let mut encoded = String::with_capacity(url.len());
    for character in url.chars() {
        let is_kept = character.is_ascii_alphanumeric()
            || (character.is_ascii() && URI_UNESCAPED.contains(&(character as u8)));
        if is_kept {
            encoded.push(character);
            continue;
        }
        for byte in character.encode_utf8(&mut [0; 4]).bytes() {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded.replace("%25", "%")
}

/// pulldown-cmark가 이미 푼 주소를 속성 값으로 쓴다(`&`만 다시 엔티티로 바꾼다)
fn url_attribute(url: &str) -> String {
    clean_url(url).replace('&', "&amp;")
}

/// 대체 글자를 모으는 중인 그림
struct PendingImage {
    src: String,
    title: String,
    alt: String,
}

struct HtmlWriter<'a> {
    html: String,
    spans: &'a [InlineSpan],
    /// marked `breaks: true`면 줄바꿈 하나도 `<br>`이다
    is_newline_break: bool,
    image: Option<PendingImage>,
    code_text: Option<String>,
    is_fenced_code: bool,
    table_alignments: Vec<Alignment>,
    table_cell_index: usize,
    is_table_head: bool,
    has_table_body: bool,
}

/// 자리표시자를 바꾼 Markdown을 marked와 같은 HTML로 쓴다
pub fn markdown_to_html(markdown: &str, spans: &[InlineSpan], is_newline_break: bool) -> String {
    let mut writer = HtmlWriter {
        html: String::with_capacity(markdown.len() * 2),
        spans,
        is_newline_break,
        image: None,
        code_text: None,
        is_fenced_code: false,
        table_alignments: Vec::new(),
        table_cell_index: 0,
        is_table_head: false,
        has_table_body: false,
    };
    for event in Parser::new_ext(markdown, markdown_options()) {
        writer.event(event);
    }
    writer.html
}

impl HtmlWriter<'_> {
    fn event(&mut self, event: Event) {
        if self.image.is_some() && !matches!(event, Event::End(TagEnd::Image)) {
            self.image_alt_event(event);
            return;
        }
        if let (Some(code), Event::Text(text)) = (self.code_text.as_mut(), &event) {
            code.push_str(text);
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => {
                self.html
                    .push_str(&format!("<code>{}</code>", escape_html(&code)));
            }
            Event::Html(html) | Event::InlineHtml(html) => self.html.push_str(&html),
            Event::SoftBreak if !self.is_newline_break => self.html.push('\n'),
            Event::SoftBreak | Event::HardBreak => self.html.push_str("<br>"),
            Event::Rule => self.html.push_str("<hr>\n"),
            Event::TaskListMarker(is_checked) => {
                let checked = if is_checked { "checked=\"\" " } else { "" };
                self.html.push_str(&format!(
                    "<input {checked}disabled=\"\" type=\"checkbox\"> "
                ));
            }
            _ => {}
        }
    }

    /// marked는 그림 대체 글자를 글자 렌더러로 만든다(강조·코드는 글자만 남는다)
    fn image_alt_event(&mut self, event: Event) {
        let Some(image) = self.image.as_mut() else {
            return;
        };
        match event {
            Event::Text(text) | Event::Code(text) => image.alt.push_str(&text),
            Event::SoftBreak | Event::HardBreak => image.alt.push('\n'),
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        let html = match tag {
            Tag::Paragraph => "<p>".to_string(),
            Tag::Heading { level, .. } => format!("<{level}>"),
            Tag::BlockQuote(_) => "<blockquote>\n".to_string(),
            Tag::CodeBlock(kind) => {
                self.code_text = Some(String::new());
                self.is_fenced_code = matches!(kind, CodeBlockKind::Fenced(_));
                code_block_open(&kind)
            }
            Tag::List(Some(start)) if start != 1 => format!("<ol start=\"{start}\">\n"),
            Tag::List(Some(_)) => "<ol>\n".to_string(),
            Tag::List(None) => "<ul>\n".to_string(),
            Tag::Item => "<li>".to_string(),
            Tag::Table(alignments) => {
                self.table_alignments = alignments;
                self.has_table_body = false;
                "<table>\n".to_string()
            }
            Tag::TableHead => {
                self.is_table_head = true;
                self.table_cell_index = 0;
                "<thead>\n<tr>\n".to_string()
            }
            Tag::TableRow => self.table_row_open(),
            Tag::TableCell => self.table_cell_open(),
            Tag::Emphasis => "<em>".to_string(),
            Tag::Strong => "<strong>".to_string(),
            Tag::Strikethrough => "<del>".to_string(),
            Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            } => link_open(link_type, &dest_url, &title),
            Tag::Image {
                dest_url, title, ..
            } => {
                self.image = Some(PendingImage {
                    src: dest_url.to_string(),
                    title: title.to_string(),
                    alt: String::new(),
                });
                return;
            }
            _ => return,
        };
        self.html.push_str(&html);
    }

    fn end(&mut self, tag: TagEnd) {
        let html = match tag {
            TagEnd::Paragraph => "</p>\n".to_string(),
            TagEnd::Heading(level) => format!("</{level}>\n"),
            TagEnd::BlockQuote(_) => "</blockquote>\n".to_string(),
            TagEnd::CodeBlock => code_block_close(
                self.code_text.take().unwrap_or_default(),
                self.is_fenced_code,
            ),
            TagEnd::List(true) => "</ol>\n".to_string(),
            TagEnd::List(false) => "</ul>\n".to_string(),
            TagEnd::Item => "</li>\n".to_string(),
            TagEnd::Table if self.has_table_body => "</tbody></table>\n".to_string(),
            TagEnd::Table => "</table>\n".to_string(),
            TagEnd::TableHead => {
                self.is_table_head = false;
                "</tr>\n</thead>\n".to_string()
            }
            TagEnd::TableRow => "</tr>\n".to_string(),
            TagEnd::TableCell => {
                self.table_cell_index += 1;
                if self.is_table_head {
                    "</th>\n"
                } else {
                    "</td>\n"
                }
                .to_string()
            }
            TagEnd::Emphasis => "</em>".to_string(),
            TagEnd::Strong => "</strong>".to_string(),
            TagEnd::Strikethrough => "</del>".to_string(),
            TagEnd::Link => "</a>".to_string(),
            TagEnd::Image => self.image_tag(),
            _ => return,
        };
        self.html.push_str(&html);
    }

    fn table_row_open(&mut self) -> String {
        self.table_cell_index = 0;
        if self.has_table_body {
            return "<tr>\n".to_string();
        }
        self.has_table_body = true;
        "<tbody><tr>\n".to_string()
    }

    fn table_cell_open(&self) -> String {
        let name = if self.is_table_head { "th" } else { "td" };
        let alignment = match self.table_alignments.get(self.table_cell_index) {
            Some(Alignment::Left) => " align=\"left\"",
            Some(Alignment::Center) => " align=\"center\"",
            Some(Alignment::Right) => " align=\"right\"",
            _ => "",
        };
        format!("<{name}{alignment}>")
    }

    fn image_tag(&mut self) -> String {
        let Some(image) = self.image.take() else {
            return String::new();
        };
        let title = if image.title.is_empty() {
            String::new()
        } else {
            format!(" title=\"{}\"", escape_html(&image.title))
        };
        format!(
            "<img src=\"{}\" alt=\"{}\"{title}>",
            url_attribute(&image.src),
            escape_html(&image.alt)
        )
    }

    /// 수식 자리표시자는 글자로 남기고, 자동 링크 자리표시자는 marked `url` 토큰처럼 원문으로 링크를 쓴다
    fn text(&mut self, text: &str) {
        for part in scan::split_placeholders(text) {
            let html = match part {
                Ok(plain) => escape_html(plain),
                Err(index) => match self.spans.get(index) {
                    Some(InlineSpan::Autolink(link)) => format!(
                        "<a href=\"{}\">{}</a>",
                        clean_url(&link.href),
                        escape_keeping_entities(&link.text)
                    ),
                    Some(span) => scan::placeholder(index, span),
                    None => String::new(),
                },
            };
            self.html.push_str(&html);
        }
    }
}

fn code_block_open(kind: &CodeBlockKind) -> String {
    let language = match kind {
        CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or(""),
        CodeBlockKind::Indented => "",
    };
    if language.is_empty() {
        return "<pre><code>".to_string();
    }
    format!("<pre><code class=\"language-{}\">", escape_html(language))
}

/// marked 펜스 토큰은 닫는 펜스 앞 줄바꿈 하나를 빼고 렌더러가 끝 줄바꿈 하나를 더 빼고 다시 붙인다.
/// 들여쓴 코드는 끝 줄바꿈을 모두 빼고 하나를 붙인다
fn code_block_close(code: String, is_fenced: bool) -> String {
    let code = if is_fenced {
        let without_fence_newline = code.strip_suffix('\n').unwrap_or(&code);
        without_fence_newline
            .strip_suffix('\n')
            .unwrap_or(without_fence_newline)
            .to_string()
    } else {
        code.trim_end_matches('\n').to_string()
    };
    format!("{}\n</code></pre>\n", escape_html(&code))
}

fn link_open(link_type: LinkType, dest_url: &str, title: &str) -> String {
    let href = match link_type {
        LinkType::Email => format!("mailto:{dest_url}"),
        _ => dest_url.to_string(),
    };
    let title = if title.is_empty() {
        String::new()
    } else {
        format!(" title=\"{}\"", escape_html(title))
    };
    format!("<a href=\"{}\"{title}>", url_attribute(&href))
}
