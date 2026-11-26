//! Codex 입력란과 그 아래 대화 ID footer 읽기, 붙여넣은 글이 입력란에 그대로 보이는지 확인.
//!
//! 빈 입력란 판정은 화면 글자만으로는 할 수 없어 터미널 셀의 dim 속성을 본다. 셀은 [`TerminalCellRow`]로 받으며,
//! 터미널 계층(M3, alacritty_terminal)이 이 trait을 구현한다. CLAUDE.md의 Codex `/statusline thread-id` 예외가 이 판정에 기댄다.

use regex::Regex;
use std::sync::LazyLock;

use super::screen_text::as_rows;
use crate::javascript::{js_trim, js_trim_end, utf16_suffix_from};

/// headless 터미널 버퍼의 한 줄. xterm `IBufferLine`에서 이 판정이 쓰는 부분만 옮겼다
pub trait TerminalCellRow {
    /// 줄의 칸 수(xterm `line.length`)
    fn column_count(&self) -> usize;
    /// 그 칸의 글자(xterm `cell.getChars()`). 빈 칸과 넓은 글자의 뒤 칸은 빈 문자열이고, 줄 밖이면 `None`이다
    fn cell_chars(&self, column: usize) -> Option<String>;
    /// 그 칸이 dim(SGR 2)으로 그려졌는지
    fn is_cell_dim(&self, column: usize) -> bool;
    /// 줄 끝 공백을 뗀 화면 글(xterm `translateToString(true)`)
    fn trimmed_text(&self) -> String;
}

/// 화면의 footer 배치를 실제 TUI에서 확인한 버전. 상태 조회는 표시 설정을 바꾸지 않는다
const THREAD_FOOTER_VERIFIED_VERSIONS: [&str; 1] = ["0.169.0"];
const COMPOSER_MARKER: &str = "›";
const COMPOSER_TEXT_COLUMN: usize = 2;
/// 입력란이 비어 있을 때 Codex가 그 자리에 보여 주는 안내 문구. 사용자가 같은 글을 직접 입력할 수도 있어 글자만으로는 빈 입력란임이 증명되지 않는다
const EMPTY_COMPOSER_HINT: &str = "Ask Codex to do anything";
/// 줄 전체가 대화 ID 하나이거나, 그 뒤에 메인 대화임을 알리는 Codex의 표시만 붙은 footer
static THREAD_FOOTER_ROW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ {2}([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})(?: · Main \[default\])?$")
        .expect("footer 패턴")
});
/// 메인 대화의 빈 입력란 아래에서 실제로 확인한 단축키 안내 줄. side 대화나 연결이 끊긴 화면의 안내 줄은 이와 다르다
static MAIN_SHORTCUT_HINT_ROW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ {2}← for agents(?: · \? for shortcuts(?: · ⌃/ for side)?)?(?: +⚠ [0-9]+ warnings? · f2 to view)?$")
        .expect("단축키 안내 패턴")
});
/// 실제 Codex에서 확인한 ASCII 한 칸과 완성형 한글 두 칸만 접힘 계산에 쓴다
static KNOWN_COLUMN_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\x20-\x7e\x{ac00}-\x{d7a3}]*$").expect("칸 계산 패턴"));
/// 다음 줄로 넘어갈 수 있는 가장 짧은 조각
static SHORTEST_NEXT_PIECE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[^ \-/|!?}]*[-/|!?}]?").expect("다음 조각 패턴"));
/// 앞에 공백이 있어도 그 앞에서는 줄을 나누지 않는 글자
static NO_BREAK_BEFORE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[)\]}!?/,.:;]").expect("앞 줄바꿈 금지 패턴"));
/// 뒤에 공백이 있어도 그 뒤에서는 줄을 나누지 않는 여는 괄호
static NO_BREAK_AFTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[(\[{]$").expect("뒤 줄바꿈 금지 패턴"));
static HYPHEN_AT_WORD_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9]-$").expect("붙임표 패턴"));
static LETTER_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z]").expect("영문 시작 패턴"));
static HANGUL_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\x{ac00}-\x{d7a3}]$").expect("한글 끝 패턴"));
static HANGUL_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\x{ac00}-\x{d7a3}]").expect("한글 시작 패턴"));
static CODEX_VERSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "^codex-cli ([^{}]+)$",
        crate::javascript::JS_WHITESPACE_CLASS
    ))
    .expect("버전 패턴")
});
/// 빈 입력란에 안내문이나 추천 문구를 dim으로 그리는 CLI의 입력란 시작. Codex는 `› `, Claude Code 2.1.325는 `❯` 뒤에 no-break space를 그린다
static DIM_HINT_COMPOSER_PROMPT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[›❯][ \x{A0}]").expect("입력란 시작 패턴"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexDisplay {
    /// 입력란에 보이는 글. 입력란이 비어 있으면 Codex의 안내 문구가 그대로 담기며, 사용자가 직접 입력한 같은 글과 구분되지 않는다
    pub composer_text: String,
    /// footer 자리에 보이는 대화 ID. 화면의 글자만 읽은 값이라 이 값만으로 새 대화를 받아들이지 않는다.
    /// 처음 보는 ID를 받아들이거나 표시 명령을 치기 전에, 같은 캡처의 dim 빈 입력란 표시로 입력란이 실제로 비어 있음을 확인해야 한다
    pub thread_id: Option<String>,
}

/// Codex와 Claude Code는 기본 안내문과 응답 뒤 추천 문구를 모두 빈 입력란에 dim으로 그린다. 같은 글을 직접 쓴 입력은 이 조건을 충족하지 않는다
pub fn is_dim_hint_empty_composer(
    line: Option<&dyn TerminalCellRow>,
    cursor_column: usize,
) -> bool {
    let Some(line) = line.filter(|_| cursor_column == COMPOSER_TEXT_COLUMN) else {
        return false;
    };
    if !DIM_HINT_COMPOSER_PROMPT.is_match(js_trim_end(&line.trimmed_text())) {
        return false;
    }
    let has_visible_text = |column: usize| {
        line.cell_chars(column)
            .is_some_and(|chars| !js_trim(&chars).is_empty())
    };
    let Some(last_text_column) = (COMPOSER_TEXT_COLUMN..line.column_count())
        .rev()
        .find(|column| has_visible_text(*column))
    else {
        return false;
    };
    let text_columns: Vec<usize> = (COMPOSER_TEXT_COLUMN..=last_text_column)
        .filter(|column| {
            line.cell_chars(*column)
                .is_some_and(|chars| !chars.is_empty())
        })
        .collect();
    !text_columns.is_empty() && text_columns.iter().all(|column| line.is_cell_dim(*column))
}

pub fn is_codex_thread_footer_supported(version_output: &str) -> bool {
    CODEX_VERSION
        .captures(js_trim(version_output))
        .is_some_and(|found| THREAD_FOOTER_VERIFIED_VERSIONS.contains(&&found[1]))
}

fn row_or_empty<'a>(lines: &[&'a str], row: isize) -> &'a str {
    usize::try_from(row)
        .ok()
        .and_then(|row| lines.get(row).copied())
        .unwrap_or("")
}

fn is_blank_row(line: &str) -> bool {
    js_trim(line).is_empty()
}

fn is_composer_continuation_row(line: &str) -> bool {
    is_blank_row(line) || line.starts_with(&" ".repeat(COMPOSER_TEXT_COLUMN))
}

fn last_content_row(lines: &[&str]) -> isize {
    lines
        .iter()
        .rposition(|line| !is_blank_row(line))
        .map_or(-1, |row| row as isize)
}

/// 커서 줄에서 위로 올라가며 입력란의 첫 줄을 찾는다. 첫 줄이 화면 밖으로 밀렸으면 찾지 못한다
fn find_composer_first_row(lines: &[&str], cursor_row: usize) -> Option<usize> {
    for row in (0..=cursor_row).rev() {
        let line = row_or_empty(lines, row as isize);
        if lines
            .get(row)
            .is_some_and(|line| line.starts_with(COMPOSER_MARKER))
        {
            return Some(row);
        }
        if !is_composer_continuation_row(line) {
            return None;
        }
    }
    None
}

fn read_footer_thread_id(line: &str) -> Option<String> {
    THREAD_FOOTER_ROW
        .captures(js_trim_end(line))
        .map(|found| found[1].to_string())
}

/// 실제 빈 입력란의 idle 안내만 허용한다. 실행 중인 메시지 대기 안내는 자동 명령/새 입력을 허용하지 않는다
pub fn is_codex_main_composer_ready<S: AsRef<str>>(
    viewport_lines: &[S],
    cursor_row: usize,
) -> bool {
    let lines = as_rows(viewport_lines);
    let last_row = last_content_row(&lines);
    let cursor_row = cursor_row as isize;
    // 초기 상태줄은 설정 항목과 pane 너비에 따라 여러 줄로 접힌다. 빈 입력란과 마지막 안내 줄 사이의 연속된 상태줄을 확인한다
    last_row >= cursor_row + 3
        && is_blank_row(row_or_empty(&lines, cursor_row + 1))
        && ((cursor_row + 2)..last_row).all(|row| {
            let line = row_or_empty(&lines, row);
            line.starts_with("  ") && !is_blank_row(line)
        })
        && MAIN_SHORTCUT_HINT_ROW.is_match(js_trim_end(row_or_empty(&lines, last_row)))
}

fn read_composer_text(lines: &[&str], first_row: usize, last_row: usize) -> String {
    let end = (last_row + 1).min(lines.len());
    lines[first_row.min(end)..end]
        .iter()
        .map(|line| utf16_suffix_from(line, COMPOSER_TEXT_COLUMN))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 커서가 놓인 Codex 입력란과 그 아래 footer를 같은 화면에서 읽는다.
///
/// 안내 문구가 보이는 입력란에서는 입력란 한 줄, 빈 줄, footer, 확인된 단축키 안내 줄이 차례로 화면 끝까지 이어져야 하고,
/// 그 밖의 입력란에서는 입력란 끝에서 빈 줄 하나를 건넌 footer가 화면의 마지막 내용 줄이어야 한다.
/// 커서가 입력란에 있지 않으면 `None`이다
pub fn read_codex_display<S: AsRef<str>>(
    viewport_lines: &[S],
    cursor_row: usize,
    is_native_empty: bool,
) -> Option<CodexDisplay> {
    let lines = as_rows(viewport_lines);
    let first_row = find_composer_first_row(&lines, cursor_row)?;
    let first_line = js_trim_end(utf16_suffix_from(lines[first_row], COMPOSER_TEXT_COLUMN));
    let last_content = last_content_row(&lines);
    let cursor = cursor_row as isize;

    if is_native_empty || first_line == EMPTY_COMPOSER_HINT {
        let hint_row = cursor + 3;
        let is_empty_composer_layout = first_row == cursor_row
            && is_blank_row(row_or_empty(&lines, cursor + 1))
            && last_content == hint_row
            && (is_codex_main_composer_ready(&lines, cursor_row)
                || js_trim_end(row_or_empty(&lines, hint_row)) == "  tab to queue message");
        return Some(CodexDisplay {
            composer_text: if is_native_empty {
                String::new()
            } else {
                read_composer_text(&lines, first_row, cursor_row)
            },
            thread_id: is_empty_composer_layout
                .then(|| read_footer_thread_id(row_or_empty(&lines, cursor + 2)))
                .flatten(),
        });
    }

    let footer_row = last_content;
    let is_footer_below_draft = !first_line.is_empty()
        && footer_row - 2 >= cursor
        && is_blank_row(row_or_empty(&lines, footer_row - 1))
        && ((cursor + 1)..(footer_row - 1))
            .all(|row| is_composer_continuation_row(row_or_empty(&lines, row)));
    let thread_id = is_footer_below_draft
        .then(|| read_footer_thread_id(row_or_empty(&lines, footer_row)))
        .flatten();
    let composer_last_row = if thread_id.is_some() {
        (footer_row - 2) as usize
    } else {
        cursor_row
    };
    Some(CodexDisplay {
        composer_text: read_composer_text(&lines, first_row, composer_last_row),
        thread_id,
    })
}

/// 실제 Codex에서 확인한 칸 수. ASCII는 한 칸, 나머지(완성형 한글)는 두 칸이다
fn display_columns(text: &str) -> isize {
    text.chars()
        .map(|character| if (character as u32) < 0x80 { 1 } else { 2 })
        .sum()
}

/// 기대한 줄의 `fold_at`(UTF-16) 자리에서 화면의 줄이 바뀐 것이 Codex가 줄을 접은 자리인지 입력란의 글 너비로 확인한다.
/// 접으면서 지운 공백 수를 돌려주고, 접힌 자리로 확인되지 않으면 `None`이다.
///
/// 줄은 다음 조각이 그 줄에 들어갈 수 없을 때만 접힌다. 낱말 접힘과, 다음 글자가 들어갈 수 없는 강제 분할만 인정한다
fn count_spaces_dropped_at_fold(
    expected_line: &[u16],
    fold_at: usize,
    row: &str,
    text_columns: isize,
) -> Option<usize> {
    let rest = String::from_utf16_lossy(&expected_line[fold_at.min(expected_line.len())..]);
    let after_spaces = rest.trim_start_matches(' ');
    let spaces = rest.len() - after_spaces.len();
    let next_piece = SHORTEST_NEXT_PIECE
        .find(after_spaces)
        .map_or("", |found| found.as_str());
    if !KNOWN_COLUMN_TEXT.is_match(&format!("{row}{next_piece}")) {
        return None;
    }
    let row_columns = display_columns(row);
    if row_columns > text_columns {
        return None;
    }
    let could_next_piece_fit =
        row_columns + spaces as isize + display_columns(next_piece) <= text_columns;
    if spaces > 0 {
        let is_fold = !could_next_piece_fit
            && !NO_BREAK_AFTER.is_match(row)
            && !NO_BREAK_BEFORE.is_match(next_piece);
        return is_fold.then_some(spaces);
    }
    let is_after_hyphen_inside_word =
        HYPHEN_AT_WORD_END.is_match(row) && LETTER_START.is_match(next_piece);
    let is_hangul_forced_split = HANGUL_END.is_match(row)
        && HANGUL_START.is_match(next_piece)
        && row_columns + 2 > text_columns;
    let is_fold = (is_after_hyphen_inside_word && !could_next_piece_fit)
        || row_columns == text_columns
        || is_hangul_forced_split;
    is_fold.then_some(0)
}

/// 화면의 입력란 줄들이 기대한 글을 그대로 보여 주는지 견준다.
///
/// 화면의 한 줄은 기대한 글의 한 줄과 글자 그대로 같아야 한다. 터미널이 지우는 줄 끝 공백만 예외다.
/// `viewport_columns`로 같은 캡처의 화면 너비를 주면, Codex가 긴 줄을 접어 여러 줄로 그린 것을 그 너비로 확인해 한 줄로 잇는다
pub fn is_same_composer_text(
    composer_text: &str,
    expected_text: &str,
    viewport_columns: Option<usize>,
) -> bool {
    let displayed_rows: Vec<&str> = composer_text.split('\n').map(js_trim_end).collect();
    let text_columns =
        viewport_columns.map(|columns| columns as isize - COMPOSER_TEXT_COLUMN as isize);
    let mut next_row = 0;
    for expected_line in expected_text.split('\n').map(js_trim_end) {
        let Some(consumed_rows) = match_expected_line(
            &displayed_rows[next_row.min(displayed_rows.len())..],
            expected_line,
            text_columns,
        ) else {
            return false;
        };
        next_row += consumed_rows;
    }
    next_row == displayed_rows.len()
}

/// 기대한 한 줄이 화면의 앞 몇 줄로 그려졌는지. 맞지 않으면 `None`이다
fn match_expected_line(
    rows: &[&str],
    expected_line: &str,
    text_columns: Option<isize>,
) -> Option<usize> {
    let expected_units: Vec<u16> = expected_line.encode_utf16().collect();
    let mut shown_length = 0;
    let mut consumed_rows = 0;
    loop {
        let row = *rows.get(consumed_rows)?;
        consumed_rows += 1;
        let row_units: Vec<u16> = row.encode_utf16().collect();
        let is_shown_here = expected_units
            .get(shown_length..)
            .is_some_and(|rest| rest.starts_with(&row_units));
        if row.is_empty() != expected_line.is_empty() || !is_shown_here {
            return None;
        }
        shown_length += row_units.len();
        if shown_length == expected_units.len() {
            return Some(consumed_rows);
        }
        shown_length +=
            count_spaces_dropped_at_fold(&expected_units, shown_length, row, text_columns?)?;
        if shown_length >= expected_units.len() {
            return Some(consumed_rows);
        }
    }
}

/// 프롬프트를 붙여넣은 뒤 Codex 입력란에 보여야 하는 글의 후보. [`is_same_composer_text`]로 견줄 수 있게 화면 표기 그대로 만든다.
///
/// Codex는 붙여넣은 이미지 경로를 순번이 붙은 자리표시로 바꾸고 뒤에 공백 하나를 두며, 긴 글은 글자 수만 적힌 자리표시로 접는다.
/// 어느 길이부터 접는지는 Codex가 정하므로 두 형태를 모두 후보로 둔다
pub fn expect_codex_composer_texts(body: &str, image_count: usize) -> Vec<String> {
    let image_placeholders: String = (1..=image_count)
        .map(|index| format!("[Image #{index}] "))
        .collect();
    if body.is_empty() {
        return vec![image_placeholders];
    }
    vec![
        format!("{image_placeholders}{body}"),
        format!(
            "{image_placeholders}[Pasted Content {} chars]",
            body.chars().count()
        ),
    ]
}

#[cfg(test)]
mod tests;
