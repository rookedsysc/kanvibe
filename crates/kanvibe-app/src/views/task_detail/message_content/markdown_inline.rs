//! Markdown 인라인 모델을 고를 수 있는 글자 구간으로 바꾼다(TS `.ai-session-markdown`의 인라인 규칙).
//!
//! 굵게는 `font-semibold text-text-primary`, 링크는 `text-brand-primary`, 인라인 코드는 `bg-bg-page font-mono`,
//! 수식은 KaTeX가 그린 글자를 한 줄로 옮긴 모양이다. GPUI 글자 요소는 한 덩어리 안에서 글자 크기를 바꿀 수 없어서
//! 인라인 코드는 본문과 같은 크기로 그린다. 그림과 블록 수식은 글자 흐름 밖에 따로 놓이므로 조각을 나눈다.

use gpui_kit::{FontStyle, Hsla, Rgba, StrikethroughStyle, TextRun, UnderlineStyle, px, rgb};
use kanvibe_markdown::math::MathNode;
use kanvibe_markdown::{Inline, Length};

use super::selectable_text::RichTextSpans;
use crate::fonts::{mono_font, sans_font};
use crate::theme::ThemeTokens;

/// 문단 안의 한 조각. 글자는 이어서 흐르고, 그림과 블록 수식은 줄을 바꿔 따로 놓인다
#[derive(Clone)]
pub enum InlinePiece {
    Text(RichTextSpans),
    Image {
        src: Option<String>,
        alt: String,
        width: Option<Length>,
        height: Option<Length>,
    },
    DisplayMath(String),
}

/// 글자 구간이 물려받는 모양
#[derive(Clone)]
pub struct InlineStyle {
    pub color: Rgba,
    pub css_weight: u16,
    pub is_italic: bool,
    /// 고정폭 글꼴(`code`·`kbd`)
    pub is_mono: bool,
    pub background: Option<Rgba>,
    pub is_strikethrough: bool,
    pub is_underlined: bool,
    pub link: Option<String>,
}

/// 브라우저 기본 `mark` 모양(UA 스타일 `Mark`·`MarkText`). 앱 CSS가 덮지 않는다
const MARK_BACKGROUND: u32 = 0xffff00;
const MARK_TEXT: u32 = 0x000000;

impl InlineStyle {
    pub fn plain(color: Rgba) -> Self {
        Self {
            color,
            css_weight: 400,
            is_italic: false,
            is_mono: false,
            background: None,
            is_strikethrough: false,
            is_underlined: false,
            link: None,
        }
    }

    fn text_run(&self) -> TextRun {
        let mut font = match self.is_mono {
            true => mono_font(400),
            false => sans_font(self.css_weight),
        };
        if self.is_italic {
            font.style = FontStyle::Italic;
        }
        let color: Hsla = self.color.into();
        TextRun {
            len: 0,
            font,
            color,
            background_color: self.background.map(Into::into),
            underline: self.is_underlined.then_some(UnderlineStyle {
                thickness: px(1.0),
                color: Some(color),
                wavy: false,
            }),
            strikethrough: self.is_strikethrough.then_some(StrikethroughStyle {
                thickness: px(1.0),
                color: Some(color),
            }),
        }
    }
}

/// 한 모양의 글자 구간
pub fn styled_spans(text: &str, style: &InlineStyle) -> RichTextSpans {
    let mut spans = RichTextSpans::default();
    spans.push(text, &style.text_run());
    spans
}

/// 인라인 목록을 조각으로 나눈다. 빈 글자 조각은 남기지 않는다
pub fn inline_pieces(
    inlines: &[Inline],
    base: &InlineStyle,
    theme: &ThemeTokens,
) -> Vec<InlinePiece> {
    let mut builder = PieceBuilder {
        pieces: Vec::new(),
        current: RichTextSpans::default(),
        theme,
    };
    builder.push_all(inlines, base);
    builder.finish()
}

struct PieceBuilder<'a> {
    pieces: Vec<InlinePiece>,
    current: RichTextSpans,
    theme: &'a ThemeTokens,
}

impl PieceBuilder<'_> {
    fn push_all(&mut self, inlines: &[Inline], style: &InlineStyle) {
        for inline in inlines {
            self.push(inline, style);
        }
    }

    fn push(&mut self, inline: &Inline, style: &InlineStyle) {
        match inline {
            Inline::Text(text) => self.push_text(text, style),
            Inline::Code(code) => self.push_text(code, &code_style(style, self.theme)),
            Inline::Link { href, content, .. } => self.push_link(href.as_deref(), content, style),
            Inline::Image {
                src,
                alt,
                width,
                height,
                ..
            } => self.push_block(InlinePiece::Image {
                src: src.clone(),
                alt: alt.clone(),
                width: *width,
                height: *height,
            }),
            Inline::Checkbox { is_checked } => {
                self.push_text(if *is_checked { "☑ " } else { "☐ " }, style)
            }
            Inline::LineBreak => self.push_text("\n", style),
            Inline::Math {
                display: true,
                node,
                ..
            } => self.push_block(InlinePiece::DisplayMath(math_text(node))),
            Inline::Math { node, .. } => self.push_math(node, style),
            Inline::Subscript(children) => self.push_script(children, '_', style),
            Inline::Superscript(children) => self.push_script(children, '^', style),
            // `q`의 따옴표는 Chromium이 생성 내용으로 그린다
            Inline::Quote {
                open,
                close,
                content,
            } => {
                self.push_text(&open.to_string(), style);
                self.push_all(content, style);
                self.push_text(&close.to_string(), style);
            }
            container => {
                let children = container.children().unwrap_or_default();
                let decorated = decorated_style(container, style, self.theme);
                self.push_all(children, &decorated);
            }
        }
    }

    /// 위·아래 첨자. 글자 크기를 바꿀 수 없어 첨자 글자가 있으면 그것으로, 없으면 그대로 쓴다
    fn push_script(&mut self, children: &[Inline], marker: char, style: &InlineStyle) {
        let text = Inline::plain_text(children);
        let converted: Option<String> = text
            .chars()
            .map(|character| script_char(character, marker))
            .collect();
        match converted {
            Some(script) => self.push_text(&script, style),
            None => self.push_all(children, style),
        }
    }

    fn push_text(&mut self, text: &str, style: &InlineStyle) {
        let start = self.current.text.len();
        self.current.push(text, &style.text_run());
        if let Some(href) = &style.link {
            let end = self.current.text.len();
            self.current.links.push((start..end, href.clone()));
        }
    }

    /// DOMPurify가 막은 주소(`None`)도 링크 색은 그대로다
    fn push_link(&mut self, href: Option<&str>, content: &[Inline], style: &InlineStyle) {
        let link = InlineStyle {
            color: self.theme.brand.primary,
            link: href.map(str::to_string),
            ..style.clone()
        };
        self.push_all(content, &link);
    }

    /// 수식은 KaTeX처럼 한 글자 변수만 기울인다
    fn push_math(&mut self, node: &MathNode, style: &InlineStyle) {
        let mut parts = Vec::new();
        collect_math(node, &mut parts);
        for (text, is_italic) in parts {
            self.push_text(
                &text,
                &InlineStyle {
                    is_italic,
                    ..style.clone()
                },
            );
        }
    }

    fn push_block(&mut self, piece: InlinePiece) {
        self.flush();
        self.pieces.push(piece);
    }

    fn flush(&mut self) {
        let spans = std::mem::take(&mut self.current);
        if !spans.text.is_empty() {
            self.pieces.push(InlinePiece::Text(spans));
        }
    }

    fn finish(mut self) -> Vec<InlinePiece> {
        self.flush();
        self.pieces
    }
}

/// `[&_code]:bg-bg-page [&_code]:font-mono`
fn code_style(style: &InlineStyle, theme: &ThemeTokens) -> InlineStyle {
    InlineStyle {
        is_mono: true,
        background: Some(theme.background.page),
        ..style.clone()
    }
}

/// 강조·굵게·취소선·밑줄·형광·고정폭 묶음의 모양
fn decorated_style(container: &Inline, style: &InlineStyle, theme: &ThemeTokens) -> InlineStyle {
    let style = style.clone();
    match container {
        Inline::Emphasis(_) => InlineStyle {
            is_italic: true,
            ..style
        },
        Inline::Strong(_) => strong(&style, theme),
        Inline::Strikethrough(_) => InlineStyle {
            is_strikethrough: true,
            ..style
        },
        Inline::Underline(_) => InlineStyle {
            is_underlined: true,
            ..style
        },
        Inline::Highlight(_) => InlineStyle {
            background: Some(rgb(MARK_BACKGROUND)),
            color: rgb(MARK_TEXT),
            ..style
        },
        Inline::Monospace(_) => InlineStyle {
            is_mono: true,
            ..style
        },
        // `abbr[title]`은 점선 밑줄이지만 GPUI `UnderlineStyle`은 실선·물결뿐이라 실선으로 긋는다
        Inline::Abbreviation { .. } => InlineStyle {
            is_underlined: true,
            ..style
        },
        // `small`(80%)은 구간마다 글자 크기를 바꿀 수 없어(`TextRun`에 크기 칸이 없다) 본문 크기로 그린다
        _ => style,
    }
}

/// `[&_strong]:font-semibold [&_strong]:text-text-primary`
fn strong(style: &InlineStyle, theme: &ThemeTokens) -> InlineStyle {
    InlineStyle {
        css_weight: 600,
        color: match style.link {
            Some(_) => style.color,
            None => theme.text.primary,
        },
        ..style.clone()
    }
}

/// 수식 트리를 한 줄 글자로 옮긴다(블록 수식용)
pub fn math_text(node: &MathNode) -> String {
    let mut parts = Vec::new();
    collect_math(node, &mut parts);
    parts.into_iter().map(|(text, _)| text).collect()
}

/// (글자, 기울임) 조각. 분수는 `a/b`, 첨자는 위·아래 첨자 글자가 있으면 그것으로, 없으면 `^(…)`·`_(…)`로 쓴다
fn collect_math(node: &MathNode, parts: &mut Vec<(String, bool)>) {
    match node {
        MathNode::Row(children) => children.iter().for_each(|child| collect_math(child, parts)),
        MathNode::Identifier { text, is_italic } => parts.push((text.clone(), *is_italic)),
        MathNode::Number(text) | MathNode::Text(text) => parts.push((text.clone(), false)),
        MathNode::Operator { text, .. } => parts.push((operator_text(text), false)),
        MathNode::Fraction {
            numerator,
            denominator,
        } => {
            push_grouped(numerator, parts);
            parts.push(("/".into(), false));
            push_grouped(denominator, parts);
        }
        MathNode::Scripts {
            base,
            subscript,
            superscript,
        } => {
            collect_math(base, parts);
            push_script(subscript.as_deref(), '_', parts);
            push_script(superscript.as_deref(), '^', parts);
        }
        MathNode::Sqrt { index, radicand } => {
            if let Some(index) = index {
                push_script(Some(index), '^', parts);
            }
            parts.push(("√".into(), false));
            push_grouped(radicand, parts);
        }
        MathNode::Accent { base, accent } => {
            collect_math(base, parts);
            parts.push((accent.to_string(), false));
        }
        MathNode::Space(_) => parts.push(("\u{2009}".into(), false)),
    }
}

/// 이항·관계 연산자는 KaTeX처럼 양옆을 띄운다
fn operator_text(text: &str) -> String {
    const SPACED: [&str; 12] = ["=", "+", "−", "-", "×", "÷", "<", ">", "≤", "≥", "≠", "→"];
    match SPACED.contains(&text) {
        true => format!(" {text} "),
        false => text.to_string(),
    }
}

fn push_grouped(node: &MathNode, parts: &mut Vec<(String, bool)>) {
    let mut inner = Vec::new();
    collect_math(node, &mut inner);
    let is_single = inner
        .iter()
        .map(|(text, _)| text.chars().count())
        .sum::<usize>()
        <= 1;
    if !is_single {
        parts.push(("(".into(), false));
    }
    parts.extend(inner);
    if !is_single {
        parts.push((")".into(), false));
    }
}

fn push_script(node: Option<&MathNode>, marker: char, parts: &mut Vec<(String, bool)>) {
    let Some(node) = node else {
        return;
    };
    let text = math_text(node);
    let converted: Option<String> = text
        .chars()
        .map(|character| script_char(character, marker))
        .collect();
    match converted {
        Some(script) => parts.push((script, false)),
        None => {
            parts.push((marker.to_string(), false));
            push_grouped(node, parts);
        }
    }
}

fn script_char(character: char, marker: char) -> Option<char> {
    const DIGITS_UP: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    const DIGITS_DOWN: [char; 10] = ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉'];
    let digits = match marker {
        '^' => DIGITS_UP,
        _ => DIGITS_DOWN,
    };
    match (character, marker) {
        ('0'..='9', _) => Some(digits[character as usize - '0' as usize]),
        ('+', '^') => Some('⁺'),
        ('−' | '-', '^') => Some('⁻'),
        ('n', '^') => Some('ⁿ'),
        ('i', '^') => Some('ⁱ'),
        ('+', '_') => Some('₊'),
        ('−' | '-', '_') => Some('₋'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_markdown::math::parse_tex;

    #[test]
    fn formulas_read_as_one_line_of_text() {
        let node = parse_tex("E = mc^2").expect("수식");
        assert_eq!(math_text(&node), "E = mc²");
        let fraction = parse_tex("\\frac{a+b}{2}").expect("수식");
        assert_eq!(math_text(&fraction), "(a + b)/2");
    }
}
