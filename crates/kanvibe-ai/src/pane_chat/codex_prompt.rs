//! Codex CLI가 띄운 번호 선택 화면(명령 승인, 사용량 경고의 모델 전환, 모델 목록) 읽기.

use regex::Regex;
use std::sync::LazyLock;

use super::claude_prompt::{
    PERMISSION_TITLE, PaneChatPromptKind, PaneChatPromptOption, PaneChatPromptScreen,
};
use super::screen_text::{as_rows, indent_of};
use crate::javascript::{JS_WHITESPACE_CLASS, js_trim, utf16_length};

/// 선택지 한 줄. 강조된 줄은 첫 칸에 ›가 붙는다
static OPTION_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(›| ) ([0-9]+)\. ([^\n\r\x{2028}\x{2029}]+)$").expect("선택지 패턴")
});
/// 선택 화면 맨 아래의 조작 안내. 예: "enter select · esc back", "Press enter to confirm or esc to cancel"
static KEY_HINT_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^ {{2}}[^{JS_WHITESPACE_CLASS}][^\n\r\x{{2028}}\x{{2029}}]*(?-u:\b)esc(?-u:\b)"
    ))
    .expect("조작 안내 패턴")
});
/// 선택 화면의 제목과 상세는 두 칸 들여쓴다. 대화 내용은 첫 칸의 기호로 시작하거나 더 깊이 들여쓴다
static DIALOG_TEXT_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!("^ {{2}}[^{JS_WHITESPACE_CLASS}]")).expect("본문 줄 패턴")
});
/// 선택지와 같은 줄에서 설명을 떼어 놓는 간격
static INLINE_DESCRIPTION_GAP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!("[{JS_WHITESPACE_CLASS}]{{2,}}")).expect("설명 간격 패턴")
});
/// pane이 좁아 넘어간 선택지의 나머지 줄. 라벨이나 설명이 시작한 칸에 맞춰 들여쓴다
static WRAPPED_OPTION_ROW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!("^ {{5,}}[^{JS_WHITESPACE_CLASS}]")).expect("넘어간 줄 패턴")
});
/// Codex가 대화를 시작할 때 찍는 머리글의 첫 줄. 넓은 pane에서는 상자 안에 그린다
static HEADER_TITLE_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^[{JS_WHITESPACE_CLASS}│]*>_ OpenAI Codex(?-u:\b)"
    ))
    .expect("머리글 패턴")
});
static PARAGRAPH_BREAK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\n{2,}").expect("문단 구분 패턴"));

fn is_blank(line: Option<&str>) -> bool {
    js_trim(line.unwrap_or_default()).is_empty()
}

fn row_at<'a>(lines: &[&'a str], row: isize) -> Option<&'a str> {
    usize::try_from(row)
        .ok()
        .and_then(|row| lines.get(row).copied())
}

/// 선택 화면과, 그 위에 보이는 대화 내용의 줄
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexPromptWithConversation {
    pub screen: PaneChatPromptScreen,
    pub conversation_rows: Vec<String>,
}

/// Codex CLI가 띄운 번호 선택 화면을 읽는다.
///
/// Codex는 선택 화면을 화면 아래에 붙여 그리고 커서를 선택지 위에 두지 않는다. 그래서 맨 아래의 조작 안내 줄,
/// 그 위의 빈 줄, 그 위의 번호 목록이라는 배치로 판별한다. 선택 화면 위쪽은 빈 줄 둘로 대화 내용과 떨어져 있다.
/// Codex CLI 0.169.0의 실제 화면으로 확인한 배치이고, 읽지 못하는 화면은 `None`이다
pub fn read_codex_prompt<S: AsRef<str>>(lines: &[S]) -> Option<PaneChatPromptScreen> {
    read_codex_prompt_with_conversation(lines).map(|found| found.screen)
}

/// 선택 화면을 읽고, 그 위에 보이는 대화 내용의 줄도 함께 돌려준다
pub fn read_codex_prompt_with_conversation<S: AsRef<str>>(
    lines: &[S],
) -> Option<CodexPromptWithConversation> {
    let lines = as_rows(lines);
    let hint_row = lines.iter().rposition(|line| !is_blank(Some(line)))? as isize;
    if !KEY_HINT_LINE.is_match(lines[hint_row as usize]) || !is_blank(row_at(&lines, hint_row - 1))
    {
        return None;
    }
    let last_option_row = hint_row - 2;
    let mut first_option_row = last_option_row + 1;
    while first_option_row > 0 {
        let line = row_at(&lines, first_option_row - 1).unwrap_or_default();
        if !OPTION_LINE.is_match(line) && !WRAPPED_OPTION_ROW.is_match(line) {
            break;
        }
        first_option_row -= 1;
    }
    let option_lines = slice_rows(&lines, first_option_row, last_option_row + 1);
    let (options, highlighted_number) = read_option_block(&option_lines)?;
    let text_start = dialog_text_start(&lines, first_option_row);
    let paragraphs = read_paragraphs(&lines, text_start, first_option_row);
    let title = paragraphs.first()?.clone();
    let screen = PaneChatPromptScreen {
        kind: if PERMISSION_TITLE.is_match(&title) {
            PaneChatPromptKind::Permission
        } else {
            PaneChatPromptKind::Confirmation
        },
        detail: paragraphs[1..].join("\n\n"),
        title,
        steps_line: None,
        options,
        highlighted_number,
        is_multi_select: false,
        is_text_entry_active: false,
        has_hidden_options: false,
    };
    let conversation_rows = without_header(&lines[..text_start.max(0) as usize]);
    Some(CodexPromptWithConversation {
        screen,
        conversation_rows,
    })
}

/// 선택지 목록과 강조된 번호. 선택지가 둘 이상이고, 1부터 이어지고, 강조된 줄이 하나여야 한다
fn read_option_block(option_lines: &[&str]) -> Option<(Vec<PaneChatPromptOption>, u64)> {
    let options = read_options(option_lines);
    let highlighted: Vec<&HighlightedOption> = options
        .iter()
        .filter(|option| option.is_highlighted)
        .collect();
    let is_numbered_in_order = options
        .iter()
        .enumerate()
        .all(|(index, option)| option.option.number == index as u64 + 1);
    if options.len() < 2 || highlighted.len() != 1 || !is_numbered_in_order {
        return None;
    }
    let highlighted_number = highlighted[0].option.number;
    Some((
        options.into_iter().map(|option| option.option).collect(),
        highlighted_number,
    ))
}

fn slice_rows<'a>(lines: &[&'a str], start: isize, end: isize) -> Vec<&'a str> {
    let start = start.clamp(0, lines.len() as isize) as usize;
    let end = end.clamp(start as isize, lines.len() as isize) as usize;
    lines[start..end].to_vec()
}

/// 선택지 위 제목·상세가 시작하는 줄. 빈 줄 하나는 선택 화면 안의 문단 사이, 빈 줄 둘은 선택 화면의 위쪽 끝이다
fn dialog_text_start(lines: &[&str], first_option_row: isize) -> isize {
    let mut text_end = first_option_row - 1;
    while text_end >= 0 && is_blank(row_at(lines, text_end)) {
        text_end -= 1;
    }
    let mut text_start = text_end + 1;
    while text_start > 0 {
        let previous = row_at(lines, text_start - 1).unwrap_or_default();
        let before_previous = row_at(lines, text_start - 2).unwrap_or_default();
        let is_dialog_row = DIALOG_TEXT_LINE.is_match(previous)
            || (is_blank(Some(previous)) && DIALOG_TEXT_LINE.is_match(before_previous));
        if !is_dialog_row {
            break;
        }
        text_start -= 1;
    }
    text_start
}

fn read_paragraphs(lines: &[&str], text_start: isize, first_option_row: isize) -> Vec<String> {
    let mut text_end = first_option_row - 1;
    while text_end >= 0 && is_blank(row_at(lines, text_end)) {
        text_end -= 1;
    }
    let joined = slice_rows(lines, text_start, text_end + 1)
        .iter()
        .map(|line| js_trim(line))
        .collect::<Vec<_>>()
        .join("\n");
    PARAGRAPH_BREAK
        .split(&joined)
        .filter(|paragraph| !paragraph.is_empty())
        .map(str::to_string)
        .collect()
}

/// 머리글이 화면에 보이면 그 아래의 줄만 남긴다. 머리글의 작업 경로 줄은 pane 너비에 맞춰 줄여 다시 그려져, 같은 대화에서도 글자가 달라진다.
/// 머리글은 빈 줄에서 끝난다. 이전 대화의 내용이 화면 위쪽에 남아 있으면 마지막 머리글이 지금 대화의 시작이다
fn without_header(rows: &[&str]) -> Vec<String> {
    let Some(title_row) = rows.iter().rposition(|row| HEADER_TITLE_LINE.is_match(row)) else {
        return rows.iter().map(|row| row.to_string()).collect();
    };
    let first_row_below_header = rows
        .iter()
        .enumerate()
        .position(|(index, row)| index > title_row && is_blank(Some(row)));
    first_row_below_header.map_or_else(Vec::new, |row| {
        rows[row..].iter().map(|row| row.to_string()).collect()
    })
}

struct HighlightedOption {
    option: PaneChatPromptOption,
    is_highlighted: bool,
}

/// UTF-16 위치 `from_units`부터 `needle`을 찾는다(JS `indexOf`). 위치도 UTF-16로 돌려준다
fn utf16_index_of(haystack: &str, needle: &str, from_units: usize) -> Option<usize> {
    let start_byte = crate::javascript::utf16_prefix(haystack, from_units).len();
    let found = haystack[start_byte..].find(needle)?;
    Some(utf16_length(&haystack[..start_byte + found]))
}

/// 선택지 줄과, pane이 좁아 그 아래로 넘어간 줄을 묶는다.
/// 넘어간 줄이 설명 칸에서 시작하면 설명의 나머지이고, 그보다 왼쪽이면 라벨의 나머지다. Codex는 붙임표 뒤에서도 줄을 나누므로 그때는 공백 없이 잇는다
fn read_options(option_lines: &[&str]) -> Vec<HighlightedOption> {
    let mut options: Vec<HighlightedOption> = Vec::new();
    let mut description_column: Option<usize> = None;
    for line in option_lines {
        if let Some(found) = OPTION_LINE.captures(line) {
            let Ok(number) = found[2].parse::<u64>() else {
                continue;
            };
            let trimmed = js_trim(&found[3]);
            let mut parts = INLINE_DESCRIPTION_GAP.split(trimmed);
            let label = parts.next().unwrap_or_default().to_string();
            let description: Vec<&str> = parts.collect();
            description_column = description.first().and_then(|first| {
                let label_end = utf16_index_of(line, &label, 0).unwrap_or(0) + utf16_length(&label);
                utf16_index_of(line, first, label_end)
            });
            options.push(HighlightedOption {
                option: PaneChatPromptOption {
                    number,
                    label,
                    description: description.join(" "),
                    is_checked: false,
                    is_text_entry: false,
                },
                is_highlighted: &found[1] == "›",
            });
            continue;
        }
        let Some(current) = options.last_mut() else {
            continue;
        };
        append_wrapped_row(&mut current.option, line, description_column);
    }
    options
}

fn append_wrapped_row(
    option: &mut PaneChatPromptOption,
    line: &str,
    description_column: Option<usize>,
) {
    let row_text = js_trim(line);
    if description_column.is_some_and(|column| indent_of(line) >= column) {
        option.description = format!("{} {row_text}", option.description);
    } else {
        let separator = if option.label.ends_with('-') { "" } else { " " };
        option.label = format!("{}{separator}{row_text}", option.label);
    }
}

#[cfg(test)]
mod tests;
