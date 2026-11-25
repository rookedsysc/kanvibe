//! 본문에 섞인 HTML을 DOMPurify(`USE_PROFILES: { html, mathMl }`, `FORBID_TAGS: style`,
//! `FORBID_ATTR: style`)를 거친 결과와 같은 뜻으로 옮긴다.
//!
//! - 내용까지 지우는 태그: script, style, template, iframe, noembed, noframes, noscript, plaintext, xmp, title,
//!   svg, audio, video, head, object, textarea, select(DOMPurify FORBID_CONTENTS와 허용 목록 밖 요소)
//! - 태그만 지우고 내용은 남기는 태그: 그 밖의 모르는 태그와 모델에 대응이 없는 허용 태그(span, div, u 등)
//! - 속성: `on*`·`style`은 버린다. 링크·이미지 주소는 DOMPurify ALLOWED_URI_REGEXP를 통과할 때만 남긴다
//!   (이미지는 `data:` 허용). 막힌 링크는 주소 없는 링크로 남는다(DOMPurify가 href만 지운 `<a>`)

use crate::model::{Block, Inline};

const DROPPED_WITH_CONTENT: [&str; 17] = [
    "script",
    "style",
    "template",
    "iframe",
    "noembed",
    "noframes",
    "noscript",
    "plaintext",
    "xmp",
    "title",
    "svg",
    "audio",
    "video",
    "head",
    "object",
    "textarea",
    "select",
];

const VOID_TAGS: [&str; 8] = ["br", "img", "hr", "input", "wbr", "source", "embed", "meta"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HtmlToken {
    Start {
        name: String,
        attributes: Vec<(String, String)>,
        is_self_closing: bool,
    },
    End {
        name: String,
    },
    Text(String),
}

pub fn is_dropped_with_content(name: &str) -> bool {
    DROPPED_WITH_CONTENT.contains(&name)
}

fn is_void(name: &str) -> bool {
    VOID_TAGS.contains(&name)
}

/// 이름·숫자 HTML 엔티티 일부를 푼다
pub fn decode_entities(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        result.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find(';').filter(|end| *end <= 10) else {
            result.push('&');
            rest = &tail[1..];
            continue;
        };
        match decode_entity(&tail[1..end]) {
            Some(character) => result.push(character),
            None => result.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    result.push_str(rest);
    result
}

fn decode_entity(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
        return u32::from_str_radix(number, 16)
            .ok()
            .and_then(char::from_u32);
    }
    if let Some(number) = name.strip_prefix('#') {
        return number.parse::<u32>().ok().and_then(char::from_u32);
    }
    let character = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '©',
        "middot" => '·',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        _ => return None,
    };
    Some(character)
}

fn read_attributes(source: &str) -> Vec<(String, String)> {
    let mut attributes = Vec::new();
    let mut rest = source.trim();
    while !rest.is_empty() {
        let name_end = rest
            .find(|character: char| {
                character.is_whitespace() || character == '=' || character == '/'
            })
            .unwrap_or(rest.len());
        let name = rest[..name_end].to_ascii_lowercase();
        rest = rest[name_end..].trim_start();
        let mut value = String::new();
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let (raw, remaining) = match after.chars().next() {
                Some(quote @ ('"' | '\'')) => {
                    let end = after[1..].find(quote).map_or(after.len(), |end| end + 1);
                    (
                        &after[1..end.min(after.len())],
                        after.get(end + 1..).unwrap_or(""),
                    )
                }
                _ => {
                    let end = after.find(char::is_whitespace).unwrap_or(after.len());
                    (&after[..end], &after[end..])
                }
            };
            value = decode_entities(raw);
            rest = remaining.trim_start();
        } else if rest.starts_with('/') {
            rest = rest[1..].trim_start();
        }
        if !name.is_empty() {
            attributes.push((name, value));
        }
    }
    attributes
}

fn read_tag(source: &str) -> Option<(HtmlToken, usize)> {
    let end = source.find('>')?;
    let inner = &source[1..end];
    if let Some(name) = inner.strip_prefix('/') {
        let name = name.trim().to_ascii_lowercase();
        return Some((HtmlToken::End { name }, end + 1));
    }
    let name_end = inner
        .find(|character: char| character.is_whitespace() || character == '/')
        .unwrap_or(inner.len());
    let name = inner[..name_end].to_ascii_lowercase();
    if name.is_empty() || !name.starts_with(|character: char| character.is_ascii_alphabetic()) {
        return None;
    }
    let is_self_closing = inner.trim_end().ends_with('/');
    let attributes = read_attributes(inner[name_end..].trim_end_matches('/'));
    Some((
        HtmlToken::Start {
            name,
            attributes,
            is_self_closing,
        },
        end + 1,
    ))
}

fn flush_text(tokens: &mut Vec<HtmlToken>, text: &mut String) {
    if !text.is_empty() {
        tokens.push(HtmlToken::Text(decode_entities(&std::mem::take(text))));
    }
}

/// HTML 조각을 토큰으로 나눈다. 주석과 `<!…>`·`<?…>`는 버린다
pub fn tokenize(source: &str) -> Vec<HtmlToken> {
    let mut tokens = Vec::new();
    let mut text = String::new();
    let mut rest = source;
    while let Some(start) = rest.find('<') {
        text.push_str(&rest[..start]);
        let tail = &rest[start..];
        if let Some(comment) = tail.strip_prefix("<!--") {
            rest = comment.find("-->").map_or("", |end| &comment[end + 3..]);
            continue;
        }
        if tail.starts_with("<!") || tail.starts_with("<?") {
            rest = tail.find('>').map_or("", |end| &tail[end + 1..]);
            continue;
        }
        let Some((token, length)) = read_tag(tail) else {
            text.push('<');
            rest = &tail[1..];
            continue;
        };
        flush_text(&mut tokens, &mut text);
        tokens.push(token);
        rest = &tail[length..];
    }
    text.push_str(rest);
    if !text.is_empty() {
        tokens.push(HtmlToken::Text(decode_entities(&text)));
    }
    tokens
}

/// DOMPurify ALLOWED_URI_REGEXP. `allow_data`는 이미지(DATA_URI_TAGS)일 때만 켠다
pub fn is_safe_url(url: &str, allow_data: bool) -> bool {
    let cleaned: String = url
        .chars()
        .filter(|character| !character.is_whitespace() && !character.is_control())
        .collect();
    let lower = cleaned.to_ascii_lowercase();
    if allow_data && lower.starts_with("data:") {
        return true;
    }
    const SCHEMES: [&str; 11] = [
        "http:", "https:", "ftp:", "ftps:", "mailto:", "tel:", "callto:", "sms:", "cid:", "xmpp:",
        "matrix:",
    ];
    if SCHEMES.iter().any(|scheme| lower.starts_with(scheme)) {
        return true;
    }
    match lower.find(|character: char| {
        !(character.is_ascii_lowercase() || matches!(character, '+' | '.' | '-'))
    }) {
        None => true,
        Some(0) => true,
        Some(index) => !lower[index..].starts_with(':'),
    }
}

fn attribute<'a>(attributes: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// 인라인 HTML 태그가 만드는 모델
#[derive(Clone, Debug, PartialEq)]
pub enum InlineTag {
    Strong,
    Emphasis,
    Strikethrough,
    Code,
    Link {
        href: String,
        title: Option<String>,
    },
    /// 내용만 남기는 태그
    Transparent,
}

pub fn inline_tag(name: &str, attributes: &[(String, String)]) -> InlineTag {
    match name {
        "b" | "strong" => InlineTag::Strong,
        "i" | "em" | "cite" | "var" | "dfn" => InlineTag::Emphasis,
        "s" | "del" | "strike" => InlineTag::Strikethrough,
        "code" | "kbd" | "samp" | "tt" => InlineTag::Code,
        "a" => {
            let href = attribute(attributes, "href")
                .filter(|href| is_safe_url(href, false))
                .unwrap_or("")
                .to_string();
            InlineTag::Link {
                href,
                title: attribute(attributes, "title").map(str::to_string),
            }
        }
        _ => InlineTag::Transparent,
    }
}

/// 빈 태그 하나가 만드는 인라인(줄바꿈·이미지)
pub fn void_inline(name: &str, attributes: &[(String, String)]) -> Option<Inline> {
    match name {
        "br" => Some(Inline::LineBreak),
        "img" => {
            let src = attribute(attributes, "src").filter(|src| is_safe_url(src, true))?;
            Some(Inline::Image {
                src: src.to_string(),
                alt: attribute(attributes, "alt").unwrap_or("").to_string(),
                title: attribute(attributes, "title").map(str::to_string),
            })
        }
        _ => None,
    }
}

pub fn wrap_inline(tag: &InlineTag, children: Vec<Inline>) -> Vec<Inline> {
    match tag {
        InlineTag::Strong => vec![Inline::Strong(children)],
        InlineTag::Emphasis => vec![Inline::Emphasis(children)],
        InlineTag::Strikethrough => vec![Inline::Strikethrough(children)],
        InlineTag::Code => vec![Inline::Code(Inline::plain_text(&children))],
        InlineTag::Link { href, title } => vec![Inline::Link {
            href: href.clone(),
            title: title.clone(),
            content: children,
        }],
        InlineTag::Transparent => children,
    }
}

/// HTML 블록 하나를 블록 모델로 옮긴다
pub fn html_block_to_blocks(source: &str) -> Vec<Block> {
    let mut builder = HtmlBlockBuilder::default();
    for token in tokenize(source) {
        builder.push(token);
    }
    builder.finish()
}

#[derive(Default)]
struct HtmlBlockBuilder {
    blocks: Vec<Block>,
    /// 지금 모으는 문단 인라인
    inline_stack: Vec<(InlineTag, Vec<Inline>)>,
    current: Vec<Inline>,
    dropped_depth: usize,
    pre_depth: usize,
    pre_text: String,
    heading: Option<u8>,
}

impl HtmlBlockBuilder {
    fn push(&mut self, token: HtmlToken) {
        if self.dropped_depth > 0 {
            match &token {
                HtmlToken::Start {
                    name,
                    is_self_closing,
                    ..
                } if is_dropped_with_content(name) && !is_self_closing => self.dropped_depth += 1,
                HtmlToken::End { name } if is_dropped_with_content(name) => self.dropped_depth -= 1,
                _ => {}
            }
            return;
        }
        match token {
            HtmlToken::Text(text) => self.text(text),
            HtmlToken::Start {
                name,
                attributes,
                is_self_closing,
            } => self.start(&name, &attributes, is_self_closing),
            HtmlToken::End { name } => self.end(&name),
        }
    }

    fn text(&mut self, text: String) {
        if self.pre_depth > 0 {
            self.pre_text.push_str(&text);
            return;
        }
        let collapsed = collapse_whitespace(&text);
        if !collapsed.trim().is_empty() || !self.current.is_empty() {
            self.current.push(Inline::Text(collapsed));
        }
    }

    fn start(&mut self, name: &str, attributes: &[(String, String)], is_self_closing: bool) {
        if is_dropped_with_content(name) {
            if !is_self_closing {
                self.dropped_depth += 1;
            }
            return;
        }
        if let Some(inline) = void_inline(name, attributes) {
            self.current.push(inline);
            return;
        }
        match name {
            "p" | "div" | "section" | "article" | "li" | "ul" | "ol" | "blockquote" | "table"
            | "tr" | "details" | "summary" => self.flush(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush();
                self.heading = name[1..].parse().ok();
            }
            "hr" => {
                self.flush();
                self.blocks.push(Block::ThematicBreak);
            }
            "pre" => {
                self.flush();
                self.pre_depth += 1;
            }
            _ if is_void(name) || is_self_closing => {}
            _ if self.pre_depth == 0 => self.inline_stack.push((
                inline_tag(name, attributes),
                std::mem::take(&mut self.current),
            )),
            _ => {}
        }
    }

    fn end(&mut self, name: &str) {
        if name == "pre" && self.pre_depth > 0 {
            self.pre_depth -= 1;
            let code = std::mem::take(&mut self.pre_text);
            self.blocks
                .push(Block::CodeBlock(crate::code_block(None, code)));
            return;
        }
        if matches!(
            name,
            "p" | "div"
                | "section"
                | "article"
                | "li"
                | "blockquote"
                | "td"
                | "th"
                | "tr"
                | "summary"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
        ) {
            self.flush();
            return;
        }
        if let Some((tag, outer)) = self.inline_stack.pop() {
            let children = std::mem::replace(&mut self.current, outer);
            self.current.extend(wrap_inline(&tag, children));
        }
    }

    fn flush(&mut self) {
        while let Some((tag, outer)) = self.inline_stack.pop() {
            let children = std::mem::replace(&mut self.current, outer);
            self.current.extend(wrap_inline(&tag, children));
        }
        let content = std::mem::take(&mut self.current);
        let heading = self.heading.take();
        if content
            .iter()
            .all(|inline| matches!(inline, Inline::Text(text) if text.trim().is_empty()))
        {
            return;
        }
        let content = trim_inlines(content);
        self.blocks.push(match heading {
            Some(level) => Block::Heading { level, content },
            None => Block::Paragraph(content),
        });
    }

    fn finish(mut self) -> Vec<Block> {
        if self.pre_depth > 0 {
            let code = std::mem::take(&mut self.pre_text);
            self.blocks
                .push(Block::CodeBlock(crate::code_block(None, code)));
        }
        self.flush();
        self.blocks
    }
}

fn collapse_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut was_space = false;
    for character in text.chars() {
        if character.is_whitespace() && character != '\u{a0}' {
            if !was_space {
                result.push(' ');
            }
            was_space = true;
        } else {
            result.push(character);
            was_space = false;
        }
    }
    result
}

fn trim_inlines(mut content: Vec<Inline>) -> Vec<Inline> {
    if let Some(Inline::Text(first)) = content.first_mut() {
        *first = first.trim_start().to_string();
    }
    if let Some(Inline::Text(last)) = content.last_mut() {
        *last = last.trim_end().to_string();
    }
    content.retain(|inline| !matches!(inline, Inline::Text(text) if text.is_empty()));
    content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 스크립트는_내용까지_지우고_문단은_남긴다() {
        let blocks = html_block_to_blocks(
            "<script>window.hacked = true</script><p style=\"color:red\">안전한 문단</p>",
        );
        assert_eq!(
            blocks,
            [Block::Paragraph(vec![Inline::Text(
                "안전한 문단".to_string()
            )])]
        );
    }

    #[test]
    fn 위험한_주소는_버린다() {
        assert!(!is_safe_url("javascript:alert(1)", false));
        assert!(!is_safe_url(" jav\tascript:alert(1)", false));
        assert!(!is_safe_url("data:text/html,x", false));
        assert!(is_safe_url("data:image/png;base64,AAAA", true));
        assert!(is_safe_url("https://example.com", false));
        assert!(is_safe_url("/relative/path", false));
        assert!(is_safe_url("docs/readme.md", false));
    }
}
