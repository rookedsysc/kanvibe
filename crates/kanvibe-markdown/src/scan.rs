//! marked 인라인 확장(`latex`)과 GFM 자동 링크(주소·이메일)를 pulldown-cmark가 읽기 전에 자리표시자로 바꾼다.
//!
//! marked는 글자 토큰을 끊는 자리마다 `latex` 확장을 먼저, GFM `url`을 나중에 시도하고, 맞은 구간은 통째로 한
//! 토큰이 된다. 같은 순서로 원문을 왼쪽부터 훑어 처음 맞는 구간을 자리표시자(사용자 영역 문자)로 바꾸면,
//! pulldown-cmark는 그 구간 안의 `*`·`[`·`` ` ``를 문법으로 읽지 않는다.
//!
//! 시도하는 자리는 미리 한 번 파싱한 결과의 글자 사건 구간으로 한정한다. 코드·HTML·`<…>` 자동 링크는 marked에서도
//! 통째로 한 토큰이라 건너뛴다. 링크·그림 글자와 `<a …>`…`</a>` 사이에서는 marked `inLink`처럼 자동 링크를
//! 만들지 않는다. 구간은 그 글자가 속한 문단·제목·표 칸·목록 항목 끝을 넘지 않는다(marked는 블록마다 따로
//! 인라인을 읽고, 표는 칸을 먼저 나눈다). 강조·굵게·취소선·링크 글자 안이면 그 안쪽 끝도 넘지 않는다(marked는
//! 바깥에서 강조 범위를 먼저 정하고 안쪽을 따로 읽는다).

use std::ops::Range;

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

use crate::autolink::{self, Autolink};
use crate::latex::{self, LatexSpan};

pub const PLACEHOLDER_START: char = '\u{E000}';
pub const PLACEHOLDER_END: char = '\u{E001}';

/// 자리표시자 하나가 가리키는 marked 토큰
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineSpan {
    Latex(LatexSpan),
    Autolink(Autolink),
}

/// 자리표시자로 바꾼 본문과 자리표시자 목록
pub struct Substituted {
    pub text: String,
    pub spans: Vec<InlineSpan>,
}

/// 확장을 시도할 수 있는 글자 구간
struct ScanZone {
    range: Range<usize>,
    /// 구간이 속한 인라인 블록(또는 강조·링크 글자)의 안쪽 끝. 맞춘 토큰이 이 끝을 넘지 않는다
    limit: usize,
    can_autolink: bool,
    can_latex: bool,
}

/// 첫 파싱을 따라가며 지금 위치가 어떤 인라인 문맥 안인지 기억한다
#[derive(Default)]
struct ZoneTracker {
    zones: Vec<ScanZone>,
    /// 열린 링크·그림마다 수식도 막는지(그림·`<…>` 자동 링크)
    links: Vec<bool>,
    /// 열린 문단·제목·표 칸·목록 항목의 끝
    containers: Vec<usize>,
    /// 열린 강조·굵게·취소선·링크마다 그 안에서 처음 만든 구간 번호
    inline_containers: Vec<usize>,
    /// 바로 앞 사건의 끝. 인라인 묶음이 닫힐 때 안쪽 끝이다
    previous_end: usize,
    code_block_depth: usize,
    is_in_raw_anchor: bool,
}

impl ZoneTracker {
    fn event(&mut self, event: Event, range: Range<usize>) {
        let end = range.end;
        match event {
            Event::Start(tag) => self.start(tag, range),
            Event::End(tag) => self.end(tag),
            Event::InlineHtml(html) => self.raw_html(&html),
            Event::Text(_) if self.code_block_depth == 0 => self.text(range),
            _ => {}
        }
        self.previous_end = end;
    }

    fn start(&mut self, tag: Tag, range: Range<usize>) {
        if matches!(
            tag,
            Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. }
        ) {
            self.inline_containers.push(self.zones.len());
        }
        match tag {
            Tag::Link { link_type, .. } => self
                .links
                .push(matches!(link_type, LinkType::Autolink | LinkType::Email)),
            Tag::Image { .. } => self.links.push(true),
            Tag::CodeBlock(_) => self.code_block_depth += 1,
            Tag::Paragraph | Tag::Heading { .. } | Tag::TableCell | Tag::Item => {
                self.containers.push(range.end)
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        if matches!(
            tag,
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link
        ) {
            self.clip_inline_container();
        }
        match tag {
            TagEnd::Link | TagEnd::Image => {
                self.links.pop();
            }
            TagEnd::CodeBlock => self.code_block_depth -= 1,
            TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::TableCell | TagEnd::Item => {
                self.containers.pop();
            }
            _ => {}
        }
    }

    fn clip_inline_container(&mut self) {
        let first_zone = self.inline_containers.pop().unwrap_or(self.zones.len());
        let inner_end = self.previous_end;
        for zone in &mut self.zones[first_zone..] {
            zone.limit = zone.limit.min(inner_end).max(zone.range.end);
        }
    }

    /// marked `tag` 토크나이저처럼 `<a `로 여는 태그부터 `</a>`까지를 링크 안으로 본다
    fn raw_html(&mut self, html: &str) {
        let lower = html.to_ascii_lowercase();
        if lower.starts_with("<a ") {
            self.is_in_raw_anchor = true;
        } else if lower.starts_with("</a>") {
            self.is_in_raw_anchor = false;
        }
    }

    fn text(&mut self, range: Range<usize>) {
        let limit = self.containers.last().copied().unwrap_or(range.end);
        self.zones.push(ScanZone {
            limit: limit.max(range.end),
            range,
            can_autolink: self.links.is_empty() && !self.is_in_raw_anchor,
            can_latex: !self.links.iter().any(|blocks_latex| *blocks_latex),
        });
    }
}

fn scan_zones(markdown: &str, options: Options) -> Vec<ScanZone> {
    let mut tracker = ZoneTracker::default();
    for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
        tracker.event(event, range);
    }
    tracker.zones
}

fn match_span(markdown: &str, index: usize, zone: &ScanZone) -> Option<(usize, InlineSpan)> {
    let rest = &markdown[index..zone.limit];
    let is_latex_candidate =
        rest.starts_with('$') || rest.starts_with("\\[") || rest.starts_with("\\(");
    if zone.can_latex
        && is_latex_candidate
        && !latex::is_escaped(markdown, index)
        && let Some((length, span)) = latex::match_latex(rest)
    {
        return Some((length, InlineSpan::Latex(span)));
    }
    if !zone.can_autolink {
        return None;
    }
    let found = autolink::match_url(rest).or_else(|| {
        autolink::is_email_attempt_position(rest)
            .then(|| autolink::match_email(rest))
            .flatten()
    });
    found.map(|(length, link)| (length, InlineSpan::Autolink(link)))
}

/// 시도 위치: 구간 안 글자와, 구간 바로 앞의 백슬래시(`\[`·`\(`는 pulldown-cmark가 이스케이프로 먹어 글자 구간이
/// 백슬래시 다음부터 시작한다)
fn zone_start(markdown: &str, zone: &ScanZone) -> usize {
    let start = zone.range.start;
    if start > 0 && markdown.as_bytes()[start - 1] == b'\\' {
        start - 1
    } else {
        start
    }
}

/// `has_latex`가 꺼지면(릴리스 노트처럼 `latex` 확장이 없는 marked) 자동 링크만 바꾼다
pub fn substitute(markdown: &str, options: Options, has_latex: bool) -> Substituted {
    let mut text = String::with_capacity(markdown.len());
    let mut spans = Vec::new();
    let mut cursor = 0;
    for mut zone in scan_zones(markdown, options) {
        zone.can_latex &= has_latex;
        let mut index = zone_start(markdown, &zone).max(cursor);
        while index < zone.range.end {
            let Some((length, span)) = match_span(markdown, index, &zone) else {
                index += markdown[index..].chars().next().map_or(1, char::len_utf8);
                continue;
            };
            text.push_str(&markdown[cursor..index]);
            text.push_str(&placeholder(spans.len()));
            spans.push(span);
            index += length;
            cursor = index;
        }
    }
    text.push_str(&markdown[cursor..]);
    Substituted { text, spans }
}

pub fn placeholder(index: usize) -> String {
    format!("{PLACEHOLDER_START}{index}{PLACEHOLDER_END}")
}

/// 글자 조각을 일반 글자와 자리표시자 번호로 나눈다
pub fn split_placeholders(text: &str) -> Vec<Result<&str, usize>> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(PLACEHOLDER_START) {
        let body = &rest[start + PLACEHOLDER_START.len_utf8()..];
        let Some(end) = body.find(PLACEHOLDER_END) else {
            break;
        };
        let Ok(number) = body[..end].parse::<usize>() else {
            break;
        };
        if start > 0 {
            parts.push(Ok(&rest[..start]));
        }
        parts.push(Err(number));
        rest = &body[end + PLACEHOLDER_END.len_utf8()..];
    }
    if !rest.is_empty() {
        parts.push(Ok(rest));
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emit::markdown_options;

    fn spans(markdown: &str) -> Vec<InlineSpan> {
        substitute(markdown, markdown_options(), true).spans
    }

    fn formulas(markdown: &str) -> Vec<String> {
        spans(markdown)
            .into_iter()
            .filter_map(|span| match span {
                InlineSpan::Latex(latex) => Some(latex.formula),
                InlineSpan::Autolink(_) => None,
            })
            .collect()
    }

    fn link_texts(markdown: &str) -> Vec<String> {
        spans(markdown)
            .into_iter()
            .filter_map(|span| match span {
                InlineSpan::Autolink(link) => Some(link.text),
                InlineSpan::Latex(_) => None,
            })
            .collect()
    }

    #[test]
    fn 코드와_이스케이프와_링크_주소_안은_수식이_아니다() {
        assert!(formulas("`$literal$` costs $5 and $10").is_empty());
        assert!(formulas("\\$x$ stays").is_empty());
        assert!(formulas("```\n$a$\n```").is_empty());
        assert!(formulas("[a](http://x.com/$a$)").is_empty());
        assert_eq!(formulas("[$x$](http://x.com) \\(y\\)"), ["x", "y"]);
    }

    #[test]
    fn 링크_안과_원시_a_태그_안은_자동_링크를_만들지_않는다() {
        assert!(link_texts("[https://a.com](https://b.com)").is_empty());
        assert!(link_texts("<a href=\"x\">see https://y.com</a>").is_empty());
        assert!(link_texts("<https://x.com>").is_empty());
        assert_eq!(
            link_texts("x*y@a.com foohttps://x.com"),
            ["y@a.com", "https://x.com"]
        );
    }

    #[test]
    fn 강조_안의_자동_링크는_강조_끝에서_멈춘다() {
        assert_eq!(link_texts("_a@b.com_"), ["a@b.com"]);
        assert_eq!(
            link_texts("**bold https://x.com/a**b**"),
            ["https://x.com/a"]
        );
        assert_eq!(
            link_texts("https://example.com/*foo*"),
            ["https://example.com/*foo"]
        );
    }

    #[test]
    fn 수식이_자동_링크보다_먼저이고_주소는_수식을_삼킨다() {
        assert_eq!(formulas("$https://a.com$"), ["https://a.com"]);
        assert_eq!(link_texts("www.a.com/$5$"), ["www.a.com/$5$"]);
    }

    #[test]
    fn 표_칸_끝을_넘지_않는다() {
        assert_eq!(
            link_texts("|a|b|\n|-|-|\n|https://a.com|b|"),
            ["https://a.com"]
        );
    }
}
