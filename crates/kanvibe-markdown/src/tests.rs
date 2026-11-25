//! `AiSessionMessageContent.test.tsx` 중 파싱·정화·색칠 동작을 옮긴 시험과, 같은 입력을 실제 marked 18.0.3 +
//! DOMPurify 3.4.2(jsdom)에 넣어 얻은 요소 나무를 모델로 옮겨 적은 시험. 복사 버튼·선택 유지·가로 스크롤처럼
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

/// 모든 블록(목록·표·접기 상자 안 포함)의 글자 줄 인라인을 문서 순서로 모은다
fn collect_inlines(blocks: &[Block], found: &mut Vec<Inline>) {
    for block in blocks {
        match block {
            Block::Paragraph(content)
            | Block::Inlines(content)
            | Block::Preformatted(content)
            | Block::Heading { content, .. } => found.extend(content.iter().cloned()),
            Block::BlockQuote(inner) => collect_inlines(inner, found),
            Block::List(list) => list
                .items
                .iter()
                .for_each(|item| collect_inlines(&item.blocks, found)),
            Block::Table(table) => table
                .rows
                .iter()
                .flat_map(|row| &row.cells)
                .for_each(|cell| collect_inlines(&cell.blocks, found)),
            Block::Disclosure(disclosure) => {
                collect_inlines(disclosure.summary.as_deref().unwrap_or(&[]), found);
                collect_inlines(&disclosure.blocks, found);
            }
            Block::CodeBlock(_) | Block::ThematicBreak => {}
        }
    }
}

fn inlines_of(blocks: &[Block]) -> Vec<Inline> {
    let mut found = Vec::new();
    collect_inlines(blocks, &mut found);
    found
}

/// 링크마다 (주소, 글자)
fn links(blocks: &[Block]) -> Vec<(Option<String>, String)> {
    inlines_of(blocks).iter().flat_map(links_in).collect()
}

fn links_in(inline: &Inline) -> Vec<(Option<String>, String)> {
    match inline {
        Inline::Link { href, content, .. } => vec![(href.clone(), Inline::plain_text(content))],
        other => other
            .children()
            .unwrap_or(&[])
            .iter()
            .flat_map(links_in)
            .collect(),
    }
}

fn text(value: &str) -> Inline {
    Inline::Text(value.to_string())
}

fn link(href: &str, content: &str) -> (Option<String>, String) {
    (Some(href.to_string()), content.to_string())
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
    assert_eq!(blocks[0].code, "print('still code')\n");
}

#[test]
fn 펜스_코드는_끝_줄바꿈을_하나로_맞추고_들여쓴_코드와_원시_pre도_읽는다() {
    let blocks = code_blocks(
        "```\na\n\n```\n\n    indented\n    code\n\n~~~py title\nx\n~~~\n\n\
         <pre><code class=\"language-rust\">fn main() {}</code></pre>",
    );

    let codes: Vec<(&str, Option<&str>)> = blocks
        .iter()
        .map(|block| (block.code.as_str(), block.language.as_deref()))
        .collect();
    assert_eq!(
        codes,
        [
            ("a\n", None),
            ("indented\ncode\n", None),
            ("x\n", Some("py")),
            ("fn main() {}", Some("rust"))
        ]
    );
}

#[test]
fn 코드로_시작하지_않는_원시_pre는_공백을_그대로_둔_글자다() {
    let blocks = markdown_blocks("<pre>  raw\npre <b>b</b></pre>");

    assert_eq!(
        blocks,
        [Block::Preformatted(vec![
            text("  raw\npre "),
            Inline::Strong(vec![text("b")])
        ])]
    );
}

// parity: TS-renderer-AiSessionMessageContent-2bc393
#[test]
fn 표를_옆으로_넘길_수_있는_틀로_감싼다() {
    let blocks = markdown_blocks("| 이름 | 값 |\n| --- | --- |\n| a | 1 |");

    let Some(Block::Table(table)) = blocks.first() else {
        panic!("표가 필요합니다: {blocks:?}")
    };
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[1].cells.len(), 2);
    assert!(table.rows[0].cells[0].is_header);
    assert_eq!(
        table.rows[0].cells[0].blocks,
        [Block::Inlines(vec![text("이름")])]
    );
}

#[test]
fn markdown_표는_정렬과_칸_안_인라인을_남기고_빈_칸을_채운다() {
    let blocks = markdown_blocks(
        "| a | b | c |\n|:--|:-:|--:|\n| `x` | **y** | [z](u) |\n| 1 |\n\npara\n| a |\n| - |\n| c |",
    );

    let Block::Table(table) = &blocks[0] else {
        panic!("표: {blocks:?}")
    };
    let alignments: Vec<Alignment> = table.rows[0]
        .cells
        .iter()
        .map(|cell| cell.alignment)
        .collect();
    assert_eq!(
        alignments,
        [Alignment::Left, Alignment::Center, Alignment::Right]
    );
    let body: Vec<Vec<Block>> = table.rows[1]
        .cells
        .iter()
        .map(|cell| cell.blocks.clone())
        .collect();
    assert_eq!(
        body[0],
        [Block::Inlines(vec![Inline::Code("x".to_string())])]
    );
    assert_eq!(
        body[1],
        [Block::Inlines(vec![Inline::Strong(vec![text("y")])])]
    );
    assert_eq!(links(&body[2]), [link("u", "z")]);
    assert_eq!(table.rows[2].cells.len(), 3);
    assert!(table.rows[2].cells[1].blocks.is_empty());
    assert_eq!(blocks[1], Block::Paragraph(vec![text("para")]));
    assert!(matches!(&blocks[2], Block::Table(_)));
}

#[test]
fn 원시_표는_행과_머리_칸과_칸_병합을_남기고_안의_markdown은_읽지_않는다() {
    let blocks = markdown_blocks(
        "<table>\n<tr><th align=\"center\">H</th><td colspan=\"2\">x</td></tr>\n\
         <tr><td rowspan=\"3\">**md?**</td></tr>\n</table>",
    );

    let Some(Block::Table(table)) = blocks.first() else {
        panic!("표: {blocks:?}")
    };
    let header = &table.rows[0].cells[0];
    assert!(header.is_header);
    assert_eq!(header.alignment, Alignment::Center);
    assert_eq!(table.rows[0].cells[1].column_span, 2);
    let second = &table.rows[1].cells[0];
    assert_eq!(second.row_span, 3);
    assert_eq!(second.blocks, [Block::Inlines(vec![text("**md?**")])]);
}

// parity: TS-renderer-AiSessionMessageContent-0d5248
#[test]
fn 본문에_섞인_스크립트와_인라인_스타일은_그리지_않는다() {
    let blocks = markdown_blocks(
        "<script>window.hacked = true</script><p style=\"color:red\">안전한 문단</p>",
    );

    assert_eq!(blocks, [Block::Paragraph(vec![text("안전한 문단")])]);
}

#[test]
fn 그려지지_않거나_dompurify가_지우는_요소는_내용까지_뺀다() {
    let blocks = markdown_blocks(
        "<script>alert(1)</script>\n<style>p{}</style>\n<iframe src=\"x\"></iframe>\n\
         <svg><script>bad</script><text>t</text></svg>\n<template><p>t</p></template>\n\
         <noscript>ns</noscript>\n<select><option>o1</option></select>\n\
         <video><p>fallback</p></video>\n<form><input type=\"text\" value=\"v\"><button>btn</button></form>\n\
         <math><mi>x</mi></math>\n\n<p hidden>hidden</p><dialog>dlg</dialog><dialog open>open dlg</dialog>",
    );

    assert_eq!(
        Inline::plain_text(&inlines_of(&blocks)),
        "btnxopen dlg",
        "{blocks:?}"
    );
}

// parity: TS-renderer-AiSessionMessageContent-4db3dd
#[test]
fn 수식을_mathml로_그리고_코드_안의_달러는_글자로_둔다() {
    let blocks = markdown_blocks("Formula $E=mc^2$ and \\[\\frac{a}{b}\\] plus `$literal$`");

    let inlines = inlines_of(&blocks);
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
fn 지원하지_않는_수식은_원문을_코드로_보인다() {
    let blocks = markdown_blocks("값 $\\begin{matrix}a\\end{matrix}$");

    assert!(
        inlines_of(&blocks).contains(&Inline::Code("$\\begin{matrix}a\\end{matrix}$".to_string()))
    );
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

// parity: TS-renderer-TaskDetailRoute-b9924e
#[test]
fn 채팅_상세_메시지는_markdown과_mermaid를_나누고_위험한_html을_지운다() {
    let message = [
        "## 작업 요약",
        "",
        "- 첫 번째 항목",
        "",
        "```ts",
        "const value = 1;",
        "```",
        "",
        "```mermaid",
        "graph TD",
        "  A[Start] --> B[Done]",
        "```",
        "",
        "<script>alert(\"xss\")</script>",
        "<img src=\"x\" style=\"color: red\" onerror=\"alert(1)\" alt=\"unsafe image\">",
        "[위험 링크](javascript:alert(\"link\"))",
        "<svg><script>markdown-svg-xss</script></svg>",
    ]
    .join("\n");

    let segments = parse_message(&message);

    let Segment::Markdown(before) = &segments[0] else {
        panic!("Markdown 조각: {segments:?}")
    };
    assert_eq!(
        before[0],
        Block::Heading {
            level: 2,
            content: vec![text("작업 요약")]
        }
    );
    let Block::List(list) = &before[1] else {
        panic!("목록: {before:?}")
    };
    assert_eq!(
        list.items[0].blocks,
        [Block::Inlines(vec![text("첫 번째 항목")])]
    );
    let Block::CodeBlock(code) = &before[2] else {
        panic!("코드: {before:?}")
    };
    assert_eq!(code.code, "const value = 1;\n");
    assert_eq!(
        segments[1],
        Segment::Mermaid {
            definition: "graph TD\n  A[Start] --> B[Done]".to_string()
        }
    );
    let Segment::Markdown(after) = &segments[2] else {
        panic!("Markdown 조각: {segments:?}")
    };
    // `<img>` 줄이 HTML 블록(7형)을 열어 빈 줄까지 이어지므로, marked도 링크 줄을 원문 글자로 둔다
    assert_eq!(
        after,
        &[Block::Inlines(vec![
            Inline::Image {
                src: Some("x".to_string()),
                alt: "unsafe image".to_string(),
                title: None,
                width: None,
                height: None,
            },
            text(" [위험 링크](javascript:alert(\"link\"))"),
        ])]
    );
}

// parity: TS-renderer-ReleaseUpdateDialog-f7ee72
#[test]
fn 릴리스_노트는_제목과_표_안_그림을_그리고_위험한_html을_지운다() {
    let notes = [
        "# KanVibe 0.0.1 Release Notes",
        "",
        "- **From prompt to review, fully visible:** 5-stage Kanban flow",
        "",
        "<table><tr><td><img src=\"https://raw.githubusercontent.com/rookedsysc/kanvibe/main/docs/images/main-page.png\" alt=\"Kanban Board\" width=\"100%\"></td></tr></table>",
        "",
        "<script>alert('xss')</script>",
        "[bad link](javascript:alert('xss'))",
    ]
    .join("\n");

    let blocks = parse_release_notes(&notes);

    assert_eq!(
        blocks[0],
        Block::Heading {
            level: 1,
            content: vec![text("KanVibe 0.0.1 Release Notes")]
        }
    );
    let item_inlines = inlines_of(&blocks[1..2]);
    assert_eq!(
        item_inlines[0],
        Inline::Strong(vec![text("From prompt to review, fully visible:")])
    );
    let Block::Table(table) = &blocks[2] else {
        panic!("표: {blocks:?}")
    };
    assert_eq!(
        table.rows[0].cells[0].blocks,
        [Block::Inlines(vec![Inline::Image {
            src: Some(
                "https://raw.githubusercontent.com/rookedsysc/kanvibe/main/docs/images/main-page.png"
                    .to_string()
            ),
            alt: "Kanban Board".to_string(),
            title: None,
            width: Some(Length::Percent(100.0)),
            height: None,
        }])]
    );
    assert_eq!(
        blocks[3],
        Block::Paragraph(vec![Inline::Link {
            href: None,
            title: None,
            content: vec![text("bad link")]
        }])
    );
    assert_eq!(blocks.len(), 4);
}

#[test]
fn 릴리스_노트는_marked_기본값처럼_줄바꿈을_공백으로_두고_수식을_읽지_않는다() {
    let blocks = parse_release_notes("line one\nline two $x$ https://auto.com");

    assert_eq!(
        blocks,
        [Block::Paragraph(vec![
            text("line one line two $x$ "),
            Inline::Link {
                href: Some("https://auto.com".to_string()),
                title: None,
                content: vec![text("https://auto.com")]
            }
        ])]
    );
}

#[test]
fn 위험한_링크는_주소를_지우고_url은_자동_링크로_바꾼다() {
    let blocks = markdown_blocks(
        "[x](javascript:alert(1)) 그리고 https://example.com. \
         <a href=\"https://ok.com\" onclick=\"x\" title=\"t\">ok</a> <a>noh</a> \
         <a href=\"data:text/html,x\">d</a> <a href=\"#frag\">frag</a>",
    );

    assert_eq!(
        links(&blocks),
        [
            (None, "x".to_string()),
            link("https://example.com", "https://example.com"),
            link("https://ok.com", "ok"),
            (None, "noh".to_string()),
            (None, "d".to_string()),
            link("#frag", "frag"),
        ]
    );
}

#[test]
fn 이메일을_marked_gfm처럼_자동_링크로_바꾼다() {
    let blocks = markdown_blocks(
        "hi user@example.com. and <a@b.co> mailto:x@y.org foo_bar@baz.io! x@y.ab_ x*y@a.com \
         _a@b.com_ a@b",
    );

    assert_eq!(
        links(&blocks),
        [
            link("mailto:user@example.com", "user@example.com"),
            link("mailto:a@b.co", "a@b.co"),
            link("mailto:x@y.org", "x@y.org"),
            link("mailto:foo_bar@baz.io", "foo_bar@baz.io"),
            link("mailto:x@y.a", "x@y.a"),
            link("mailto:y@a.com", "y@a.com"),
            link("mailto:a@b.com", "a@b.com"),
        ]
    );
}

#[test]
fn 주소_자동_링크는_marked처럼_끝을_다듬고_인코딩한다() {
    let blocks = markdown_blocks(
        "https://example.com/*foo* foohttps://x.com foowww.x.com **https://x.com** \
         https://a.com). https://a.com/(x) [https://a.com](https://b.com) \
         https://예시.com/경로 https://x.com&amp;y https://x.com/a\"b `https://code.com`",
    );

    assert_eq!(
        links(&blocks),
        [
            link("https://example.com/*foo", "https://example.com/*foo"),
            link("https://x.com", "https://x.com"),
            link("http://www.x.com", "www.x.com"),
            link("https://x.com", "https://x.com"),
            link("https://a.com", "https://a.com"),
            link("https://a.com/(x)", "https://a.com/(x)"),
            link("https://b.com", "https://a.com"),
            link("https://x.com&y", "https://x.com&y"),
            link("https://x.com/a%22b", "https://x.com/a\"b"),
        ]
    );
}

#[test]
fn details와_summary는_접기_상자가_되고_빈_줄_뒤의_markdown을_읽는다() {
    let blocks = markdown_blocks(
        "<details>\n<summary>Click **me**</summary>\n\ninner **bold**\n\n</details>\n\n\
         <details open><summary>S</summary>\n\n- a\n\n</details>\n\n<details>no summary</details>",
    );

    assert_eq!(
        blocks[0],
        Block::Disclosure(Disclosure {
            summary: Some(vec![Block::Inlines(vec![text("Click **me**")])]),
            is_open: false,
            blocks: vec![Block::Paragraph(vec![
                text("inner "),
                Inline::Strong(vec![text("bold")])
            ])],
        })
    );
    let Block::Disclosure(open) = &blocks[1] else {
        panic!("접기 상자: {blocks:?}")
    };
    assert!(open.is_open);
    assert!(matches!(open.blocks.as_slice(), [Block::List(_)]));
    let Block::Disclosure(untitled) = &blocks[2] else {
        panic!("접기 상자: {blocks:?}")
    };
    assert_eq!(untitled.summary, None);
    assert_eq!(untitled.blocks, [Block::Inlines(vec![text("no summary")])]);
}

#[test]
fn 원시_html_꾸밈_태그를_모델로_옮기고_뜻없는_태그는_벗긴다() {
    let blocks = markdown_blocks(
        "<b>b</b> <i>i</i> <u>u</u> <s>s</s> <mark>m</mark> <sub>2</sub> <sup>3</sup> \
         <ins>n</ins> <del>d</del> <small>sm</small> <span style=\"x\">sp</span> <code>c</code> \
         <kbd><kbd>Ctrl</kbd>+<kbd>C</kbd></kbd> <cite>ct</cite>",
    );

    let content: Vec<Inline> = inlines_of(&blocks)
        .into_iter()
        .filter(|inline| *inline != text(" "))
        .collect();
    let one = |value: &str| vec![text(value)];
    assert_eq!(
        content,
        [
            Inline::Strong(one("b")),
            Inline::Emphasis(one("i")),
            Inline::Underline(one("u")),
            Inline::Strikethrough(one("s")),
            Inline::Highlight(one("m")),
            Inline::Subscript(one("2")),
            Inline::Superscript(one("3")),
            Inline::Underline(one("n")),
            Inline::Strikethrough(one("d")),
            text(" sm sp "),
            Inline::Code("c".to_string()),
            Inline::Monospace(vec![
                Inline::Monospace(one("Ctrl")),
                text("+"),
                Inline::Monospace(one("C"))
            ]),
            Inline::Emphasis(one("ct")),
        ]
    );
}

#[test]
fn 그림은_안전한_주소와_크기만_남긴다() {
    let blocks = markdown_blocks(
        "<img src=\"javascript:alert(1)\" alt=\"x\"> <img src=\"data:image/png;base64,AA\" \
         width=\"10\" height=\"20px\" style=\"a\" onerror=\"x\"> ![alt *text*](https://x.com/i.png \"T\") \
         ![](javascript:x) ![a](<x y.png>)",
    );

    let image = |src: Option<&str>, alt: &str| Inline::Image {
        src: src.map(str::to_string),
        alt: alt.to_string(),
        title: None,
        width: None,
        height: None,
    };
    let images: Vec<Inline> = inlines_of(&blocks)
        .into_iter()
        .filter(|inline| matches!(inline, Inline::Image { .. }))
        .collect();
    assert_eq!(
        images,
        [
            image(None, "x"),
            Inline::Image {
                src: Some("data:image/png;base64,AA".to_string()),
                alt: String::new(),
                title: None,
                width: Some(Length::Pixels(10.0)),
                height: Some(Length::Pixels(20.0)),
            },
            Inline::Image {
                src: Some("https://x.com/i.png".to_string()),
                alt: "alt text".to_string(),
                title: Some("T".to_string()),
                width: None,
                height: None,
            },
            image(None, ""),
            image(Some("x%20y.png"), "a"),
        ]
    );
}

#[test]
fn 꽉_찬_목록은_글자_줄을_느슨한_목록은_문단을_담고_작업_상자를_읽는다() {
    let blocks = markdown_blocks(
        "- [ ] todo\n- [x] done\n  - [ ] nested\n\n- [x] loose task\n\n---\n\n3) x\n4) y",
    );

    let Block::List(loose) = &blocks[0] else {
        panic!("목록: {blocks:?}")
    };
    let unchecked = Inline::Checkbox { is_checked: false };
    let checked = Inline::Checkbox { is_checked: true };
    assert_eq!(
        loose.items[0].blocks,
        [Block::Paragraph(vec![unchecked.clone(), text(" todo")])]
    );
    let Block::List(nested) = &loose.items[1].blocks[1] else {
        panic!("안쪽 목록: {loose:?}")
    };
    assert_eq!(
        nested.items[0].blocks,
        [Block::Inlines(vec![unchecked, text(" nested")])]
    );
    assert_eq!(
        loose.items[2].blocks,
        [Block::Paragraph(vec![checked, text(" loose task")])]
    );
    assert_eq!(blocks[1], Block::ThematicBreak);
    let Block::List(ordered) = &blocks[2] else {
        panic!("번호 목록: {blocks:?}")
    };
    assert!(ordered.is_ordered);
    assert_eq!(ordered.start, 3);
    assert_eq!(ordered.items[1].blocks, [Block::Inlines(vec![text("y")])]);
}

#[test]
fn 원시_목록과_인용과_정의_목록을_블록으로_옮긴다() {
    let blocks = markdown_blocks(
        "<dl><dt>term</dt><dd>def</dd></dl>\n<hr>\n<h3>raw h3</h3>\n<blockquote>raw bq</blockquote>\n\
         <ol start=\"3\"><li>three</li></ol>\n<ul><li>x</li>stray</ul>",
    );

    let inline_block = |value: &str| Block::Inlines(vec![text(value)]);
    assert_eq!(
        blocks,
        [
            inline_block("term"),
            inline_block("def"),
            Block::ThematicBreak,
            Block::Heading {
                level: 3,
                content: vec![text("raw h3")]
            },
            Block::BlockQuote(vec![inline_block("raw bq")]),
            Block::List(List {
                is_ordered: true,
                start: 3,
                items: vec![ListItem {
                    blocks: vec![inline_block("three")]
                }],
            }),
            Block::List(List {
                is_ordered: false,
                start: 1,
                items: vec![ListItem {
                    blocks: vec![inline_block("x"), inline_block("stray")]
                }],
            }),
        ]
    );
}

#[test]
fn 제목은_atx와_setext를_읽고_일곱_개_샵은_문단이다() {
    let blocks =
        markdown_blocks("# H1\n## H2 ##\n###### H6\n####### not\n\nSetext\n======\n\nSetext2\n---");

    let heading = |level: u8, value: &str| Block::Heading {
        level,
        content: vec![text(value)],
    };
    assert_eq!(
        blocks,
        [
            heading(1, "H1"),
            heading(2, "H2"),
            heading(6, "H6"),
            Block::Paragraph(vec![text("####### not")]),
            heading(1, "Setext"),
            heading(2, "Setext2"),
        ]
    );
}

#[test]
fn 강조와_취소선과_참조_링크와_엔티티를_marked처럼_읽는다() {
    let blocks = markdown_blocks(
        "~~strike~~ ~single~ ***bi*** a*b*c a_b_c [ref link][r] [nope] \
         &copy; &amp; &#xA9; &foo; &copy \\* \\_\n\n[r]: https://example.com \"Title\"",
    );

    let content = inlines_of(&blocks);
    assert_eq!(
        content,
        [
            Inline::Strikethrough(vec![text("strike")]),
            text(" "),
            Inline::Strikethrough(vec![text("single")]),
            text(" "),
            Inline::Emphasis(vec![Inline::Strong(vec![text("bi")])]),
            text(" a"),
            Inline::Emphasis(vec![text("b")]),
            text("c a_b_c "),
            Inline::Link {
                href: Some("https://example.com".to_string()),
                title: Some("Title".to_string()),
                content: vec![text("ref link")]
            },
            text(" [nope] © & © &foo; &copy * _"),
        ]
    );
}

#[test]
fn 줄바꿈_하나와_두_칸_공백과_백슬래시와_br이_모두_줄을_바꾼다() {
    let blocks = markdown_blocks("line1\nline2  \nline3\\\nline4\n\nHello<br>world<br/>");

    assert_eq!(
        blocks,
        [
            Block::Paragraph(vec![
                text("line1"),
                Inline::LineBreak,
                text("line2"),
                Inline::LineBreak,
                text("line3"),
                Inline::LineBreak,
                text("line4")
            ]),
            Block::Paragraph(vec![text("Hello"), Inline::LineBreak, text("world")]),
        ]
    );
}

#[test]
fn 공백은_css처럼_접고_줄_끝과_br_앞뒤에서_지운다() {
    let blocks = markdown_blocks("<p>  a  <b> b </b>  c <br>  d  </p>");

    assert_eq!(
        blocks,
        [Block::Paragraph(vec![
            text("a "),
            Inline::Strong(vec![text("b ")]),
            text("c"),
            Inline::LineBreak,
            text("d")
        ])]
    );
}

#[test]
fn html_블록_안은_markdown을_읽지_않고_빈_줄_뒤와_인라인_태그_안은_읽는다() {
    let blocks = markdown_blocks(
        "<div>\n*not md*\n</div>\n\n<div>\n\n*md*\n\n</div>\n\n<span>*inline md*</span> works\n\n\
         Paragraph\n<div>interrupt</div>",
    );

    assert_eq!(
        blocks,
        [
            Block::Inlines(vec![text("*not md*")]),
            Block::Paragraph(vec![Inline::Emphasis(vec![text("md")])]),
            Block::Paragraph(vec![
                Inline::Emphasis(vec![text("inline md")]),
                text(" works")
            ]),
            Block::Paragraph(vec![text("Paragraph")]),
            Block::Inlines(vec![text("interrupt")]),
        ]
    );
}
