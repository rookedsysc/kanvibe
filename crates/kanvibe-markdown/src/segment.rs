//! 본문을 Markdown 조각과 Mermaid 조각으로 나눈다(`splitMarkdownAndMermaidBlocks`).
//!
//! TS 정규식 `/```[ \t]*mermaid[^\r\n]*(?:\r?\n)([\s\S]*?)```/gi`를 그대로 따른다. 여는 펜스는 줄 머리가
//! 아니어도 되고, `mermaid`는 대소문자를 가리지 않으며, 닫는 펜스가 없으면 Mermaid 조각이 아니다.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawSegment {
    Markdown(String),
    Mermaid(String),
}

struct MermaidMatch {
    start: usize,
    end: usize,
    body: String,
}

fn match_at(text: &str, start: usize) -> Option<MermaidMatch> {
    let rest = text.get(start..)?.strip_prefix("```")?;
    let after_blank = rest.trim_start_matches([' ', '\t']);
    let keyword = after_blank.get(..7)?;
    if !keyword.eq_ignore_ascii_case("mermaid") {
        return None;
    }
    let after_keyword = &after_blank[7..];
    let line_end = after_keyword
        .find(['\r', '\n'])
        .unwrap_or(after_keyword.len());
    let after_line = &after_keyword[line_end..];
    let body = after_line
        .strip_prefix("\r\n")
        .or_else(|| after_line.strip_prefix('\n'))?;
    let close = body.find("```")?;
    let body_start = text.len() - body.len();
    Some(MermaidMatch {
        start,
        end: body_start + close + 3,
        body: body[..close].to_string(),
    })
}

fn find_matches(text: &str) -> Vec<MermaidMatch> {
    let mut matches = Vec::new();
    let mut cursor = 0;
    while let Some(offset) = text[cursor..].find("```") {
        let start = cursor + offset;
        match match_at(text, start) {
            Some(found) => {
                cursor = found.end;
                matches.push(found);
            }
            None => cursor = start + 1,
        }
    }
    matches
}

pub fn split_markdown_and_mermaid(text: &str) -> Vec<RawSegment> {
    let mut segments = Vec::new();
    let mut cursor = 0;
    for found in find_matches(text) {
        let markdown = &text[cursor..found.start];
        if !markdown.trim().is_empty() {
            segments.push(RawSegment::Markdown(markdown.to_string()));
        }
        let definition = found.body.trim();
        if !definition.is_empty() {
            segments.push(RawSegment::Mermaid(definition.to_string()));
        }
        cursor = found.end;
    }
    let remaining = &text[cursor..];
    if !remaining.trim().is_empty() {
        segments.push(RawSegment::Markdown(remaining.to_string()));
    }
    if segments.is_empty() {
        segments.push(RawSegment::Markdown(text.to_string()));
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 머메이드_블록을_떼어_낸다() {
        let segments =
            split_markdown_and_mermaid("앞\n\n```Mermaid title\ngraph LR\n  A --> B\n```\n뒤");
        assert_eq!(
            segments,
            [
                RawSegment::Markdown("앞\n\n".to_string()),
                RawSegment::Mermaid("graph LR\n  A --> B".to_string()),
                RawSegment::Markdown("\n뒤".to_string()),
            ]
        );
    }

    #[test]
    fn 닫히지_않은_머메이드는_markdown으로_둔다() {
        let segments = split_markdown_and_mermaid("```mermaid\ngraph LR");
        assert_eq!(
            segments,
            [RawSegment::Markdown("```mermaid\ngraph LR".to_string())]
        );
    }
}
