//! `AiSessionMessageContent.test.tsx` 중 파싱·정화·색칠 동작을 옮긴 시험. 복사 버튼·선택 유지·가로 스크롤처럼
//! 화면 동작인 행은 GPUI 작업 상세 화면 시험이 맡는다.

use super::*;
use crate::highlight::HighlightClass;

fn markdown_blocks(text: &str) -> Vec<Block> {
    parse_message(text)
        .into_iter()
        .flat_map(|segment| match segment {
            Segment::Markdown(blocks) => blocks,
            Segment::Mermaid { .. } => Vec::new(),
        })
        .collect()
}

fn code_blocks(text: &str) -> Vec<CodeBlock> {
    markdown_blocks(text)
        .into_iter()
        .filter_map(|block| match block {
            Block::CodeBlock(code) => Some(code),
            _ => None,
        })
        .collect()
}

fn collect_inlines(blocks: &[Block], found: &mut Vec<Inline>) {
    for block in blocks {
        match block {
            Block::Paragraph(content) | Block::Heading { content, .. } => {
                found.extend(content.iter().cloned())
            }
            Block::BlockQuote(inner) => collect_inlines(inner, found),
            _ => {}
        }
    }
}

// parity: TS-renderer-AiSessionMessageContent-815249
#[test]
fn 언어가_붙은_코드_블록에_언어_이름을_보이고_글자에_색을_입힌다() {
    let blocks = code_blocks("```ts\nconst total = 3;\n```");

    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].header_label, "ts");
    let spans = blocks[0].highlight.as_ref().expect("ts는 아는 언어다");
    let keyword = spans
        .iter()
        .find(|span| span.class == Some(HighlightClass::Keyword))
        .expect("keyword");
    assert_eq!(keyword.text, "const");
    assert_eq!(blocks[0].code, "const total = 3;\n");
}

// parity: TS-renderer-AiSessionMessageContent-ff456d
#[test]
fn 언어가_없거나_모르는_언어인_코드_블록은_색_없이_원문_그대로_둔다() {
    let blocks = code_blocks("```\n<b>plain</b>\n```\n\n```nosuchlang\nvalue = 1\n```");

    let labels: Vec<&str> = blocks
        .iter()
        .map(|block| block.header_label.as_str())
        .collect();
    let codes: Vec<&str> = blocks.iter().map(|block| block.code.as_str()).collect();
    assert_eq!(labels, ["text", "nosuchlang"]);
    assert_eq!(codes, ["<b>plain</b>\n", "value = 1\n"]);
    assert!(blocks.iter().all(|block| block.highlight.is_none()));
}

// parity: TS-renderer-AiSessionMessageContent-86db00
#[test]
fn 닫히지_않은_코드_블록도_끝까지_코드로_그린다() {
    let blocks = code_blocks("설명\n\n```py\nprint('still code')");

    assert_eq!(blocks[0].header_label, "py");
    assert!(blocks[0].code.contains("print('still code')"));
}

// parity: TS-renderer-AiSessionMessageContent-2bc393
#[test]
fn 표를_옆으로_넘길_수_있는_틀로_감싼다() {
    let blocks = markdown_blocks("| 이름 | 값 |\n| --- | --- |\n| a | 1 |");

    let Some(Block::Table(table)) = blocks.first() else {
        panic!("표가 필요합니다: {blocks:?}")
    };
    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0].len(), 2);
    assert_eq!(Inline::plain_text(&table.header[0]), "이름");
}

// parity: TS-renderer-AiSessionMessageContent-0d5248
#[test]
fn 본문에_섞인_스크립트와_인라인_스타일은_그리지_않는다() {
    let blocks = markdown_blocks(
        "<script>window.hacked = true</script><p style=\"color:red\">안전한 문단</p>",
    );

    assert_eq!(
        blocks,
        [Block::Paragraph(vec![Inline::Text(
            "안전한 문단".to_string()
        )])]
    );
}

// parity: TS-renderer-AiSessionMessageContent-4db3dd
#[test]
fn 수식을_mathml로_그리고_코드_안의_달러는_글자로_둔다() {
    let blocks = markdown_blocks("Formula $E=mc^2$ and \\[\\frac{a}{b}\\] plus `$literal$`");

    let mut inlines = Vec::new();
    collect_inlines(&blocks, &mut inlines);
    let math: Vec<(&str, bool)> = inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Math { tex, display, .. } => Some((tex.as_str(), *display)),
            _ => None,
        })
        .collect();
    assert_eq!(math, [("E=mc^2", false), ("\\frac{a}{b}", true)]);
    assert!(inlines.contains(&Inline::Code("$literal$".to_string())));
}

#[test]
fn 머메이드_조각은_정의_원문을_넘긴다() {
    let segments = parse_message("앞 문단\n\n```mermaid\ngraph LR\n  A --> B\n```");

    assert!(matches!(&segments[0], Segment::Markdown(blocks) if blocks.len() == 1));
    assert_eq!(
        segments[1],
        Segment::Mermaid {
            definition: "graph LR\n  A --> B".to_string()
        }
    );
}

#[test]
fn 한_줄_바꿈도_줄을_바꾸고_작업_목록을_읽는다() {
    let blocks = markdown_blocks("첫 줄\n둘째 줄\n\n- [x] 끝남\n- [ ] 남음");

    assert_eq!(
        blocks[0],
        Block::Paragraph(vec![
            Inline::Text("첫 줄".to_string()),
            Inline::LineBreak,
            Inline::Text("둘째 줄".to_string())
        ])
    );
    let Block::List(list) = &blocks[1] else {
        panic!("목록")
    };
    assert_eq!(
        list.items.iter().map(|item| item.task).collect::<Vec<_>>(),
        [Some(true), Some(false)]
    );
    assert!(list.is_tight);
}

#[test]
fn 위험한_링크는_주소를_지우고_url은_자동_링크로_바꾼다() {
    let blocks = markdown_blocks("[x](javascript:alert(1)) 그리고 https://example.com.");

    let mut inlines = Vec::new();
    collect_inlines(&blocks, &mut inlines);
    let hrefs: Vec<&str> = inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Link { href, .. } => Some(href.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(hrefs, ["", "https://example.com"]);
}

#[test]
fn 인라인_html은_허용_태그만_모델로_옮긴다() {
    let blocks =
        markdown_blocks("a <b>굵게</b> <span onclick=\"x()\">보통</span> <script>bad()</script>끝");

    let Some(Block::Paragraph(content)) = blocks.first() else {
        panic!("문단")
    };
    assert!(content.contains(&Inline::Strong(vec![Inline::Text("굵게".to_string())])));
    assert_eq!(Inline::plain_text(content), "a 굵게 보통 끝");
}

#[test]
fn 지원하지_않는_수식은_원문을_코드로_보인다() {
    let blocks = markdown_blocks("값 $\\begin{matrix}a\\end{matrix}$");

    let mut inlines = Vec::new();
    collect_inlines(&blocks, &mut inlines);
    assert!(inlines.contains(&Inline::Code("$\\begin{matrix}a\\end{matrix}$".to_string())));
}
