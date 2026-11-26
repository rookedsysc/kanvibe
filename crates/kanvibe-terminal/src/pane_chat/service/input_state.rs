//! 화면에서 읽는 입력란 상태와 에이전트 작업 여부. CLI마다 실제 화면에서 확인한 배치만 판정하고 나머지는 unknown이다.

use std::sync::LazyLock;

use kanvibe_ai::javascript::{
    JS_WHITESPACE_CLASS, js_trim, utf16_length, utf16_prefix, utf16_suffix_from,
};
use kanvibe_ai::pane_chat::codex_display::is_codex_main_composer_ready;
use kanvibe_ai::pane_chat::process::{PaneChatProcess, PaneChatProvider};
use kanvibe_ai::pane_chat::snapshot::{PaneChatAgentActivity, PaneChatInputState};
use regex::Regex;

use super::codex_display::codex_display;
use crate::pane_chat::surface::PaneChatSurface;

/// 입력란이 비어 있을 때 Codex가 그 자리에 보여 주는 안내 문구
pub(super) const EMPTY_CODEX_COMPOSER_HINT: &str = "Ask Codex to do anything";
/// Codex 상태 줄과 입력란 사이에는 사용량 경고 같은 줄이 끼어들 수 있어 입력란 위로 이만큼 찾는다
const CODEX_WORKING_STATUS_SEARCH_ROWS: usize = 6;

/// JS 정규식의 `\s`를 JS 공백 글자로 바꿔 컴파일한다
fn compile(pattern: &str) -> Regex {
    Regex::new(&pattern.replace(r"\s", &format!("[{JS_WHITESPACE_CLASS}]"))).expect("입력란 패턴")
}

static GEMINI_MARKER: LazyLock<Regex> = LazyLock::new(|| compile(r"^\s*> "));
static OPENCODE_MARKER: LazyLock<Regex> = LazyLock::new(|| compile(r"^\s*┃ {2}"));
static GEMINI_PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^Type your message(?: or @path/to/file)?$"));
static OPENCODE_PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| compile(r"^Ask anything…(?:\s|$)"));
static GEMINI_BORDER: LazyLock<Regex> = LazyLock::new(|| compile(r"^─{3,}\s*$"));
static CLAUDE_TRY_HINT: LazyLock<Regex> = LazyLock::new(|| compile(r"^Try\s+"));
/// Claude가 작업 중일 때만 입력란 아래 footer에 붙이는 중단 안내
static CLAUDE_WORKING_FOOTER_HINT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u:\b)esc to interrupt(?-u:\b)").expect("작업 중 안내 패턴"));
/// Codex가 작업 중일 때만 입력란 위에 띄우는 상태 줄. 예: "• Working (3s • esc to interrupt)"
static CODEX_WORKING_STATUS_LINE: LazyLock<Regex> = LazyLock::new(|| {
    compile(&format!(
        r"^•\s{dot}*\({dot}*(?-u:\b)esc to interrupt\)$",
        dot = kanvibe_ai::javascript::JS_DOT
    ))
});

/// JS `line.indexOf(pattern)`의 UTF-16 위치
fn utf16_index_of(line: &str, pattern: &str) -> Option<usize> {
    line.find(pattern)
        .map(|byte_index| utf16_length(&line[..byte_index]))
}

/// 입력란이 비어 있어 바로 붙여넣어도 되는지. `verify_codex_empty`를 켜면 Codex 표시 대상은 Codex 고유의 빈 입력란 표시가 있어야만 ready다
pub(crate) fn read_input_state(
    surface: &PaneChatSurface,
    process: &PaneChatProcess,
    verify_codex_empty: bool,
) -> PaneChatInputState {
    let (Some(line), Some(cursor_column)) = (surface.input_line.as_deref(), surface.cursor_column)
    else {
        return PaneChatInputState::Unknown;
    };
    match process.provider {
        PaneChatProvider::Gemini | PaneChatProvider::Opencode => {
            read_bordered_input_state(surface, process.provider, line, cursor_column)
        }
        PaneChatProvider::Codex => read_codex_composer_state(surface, verify_codex_empty)
            .unwrap_or_else(|| {
                read_marker_input_state(surface, process.provider, line, cursor_column)
            }),
        PaneChatProvider::Claude => {
            read_marker_input_state(surface, process.provider, line, cursor_column)
        }
    }
}

/// Codex 화면 판정으로 결론이 나는 경우만 돌려준다
fn read_codex_composer_state(
    surface: &PaneChatSurface,
    verify_codex_empty: bool,
) -> Option<PaneChatInputState> {
    let text = codex_display(surface).map(|display| display.composer_text);
    let is_native_empty = surface.is_input_placeholder
        && surface.cursor_column == Some(2)
        && surface
            .input_line
            .as_deref()
            .is_some_and(|line| line.starts_with("› "))
        && match (&surface.viewport_lines, surface.cursor_row) {
            (Some(lines), Some(row)) => is_codex_main_composer_ready(lines, row),
            _ => false,
        };
    if verify_codex_empty && is_native_empty {
        return Some(PaneChatInputState::Ready);
    }
    let is_hint_or_empty = |text: &str| text.is_empty() || text == EMPTY_CODEX_COMPOSER_HINT;
    match text.as_deref() {
        Some(text) if !is_hint_or_empty(text) => Some(PaneChatInputState::Occupied),
        Some(_) if verify_codex_empty => Some(PaneChatInputState::Occupied),
        _ => None,
    }
}

/// 입력란 표시(`❯`, `›`) 바로 뒤에 커서가 있고 글이 없거나 안내 문구뿐이면 ready다
fn read_marker_input_state(
    surface: &PaneChatSurface,
    provider: PaneChatProvider,
    line: &str,
    cursor_column: usize,
) -> PaneChatInputState {
    let marker = if provider == PaneChatProvider::Claude {
        "❯"
    } else {
        "›"
    };
    let Some(start) = utf16_index_of(line, marker) else {
        return PaneChatInputState::Unknown;
    };
    if !js_trim(utf16_prefix(line, start)).is_empty() {
        return PaneChatInputState::Unknown;
    }
    let value = js_trim(utf16_suffix_from(line, start + 1));
    let is_placeholder = surface.is_input_placeholder
        || match provider {
            PaneChatProvider::Codex => value == EMPTY_CODEX_COMPOSER_HINT,
            _ => CLAUDE_TRY_HINT.is_match(value),
        };
    if cursor_column == start + 2 && (value.is_empty() || is_placeholder) {
        PaneChatInputState::Ready
    } else {
        PaneChatInputState::Occupied
    }
}

/// Gemini는 위아래 경계선 사이, OpenCode는 `┃` 테두리 블록 안의 글을 모두 본다. 커서 줄만 비어 있고 다른 줄에 draft가 남은 경우를 막는다
fn read_bordered_input_state(
    surface: &PaneChatSurface,
    provider: PaneChatProvider,
    line: &str,
    cursor_column: usize,
) -> PaneChatInputState {
    let is_gemini = provider == PaneChatProvider::Gemini;
    let marker = if is_gemini {
        &GEMINI_MARKER
    } else {
        &OPENCODE_MARKER
    };
    let Some(found) = marker.find(line) else {
        return PaneChatInputState::Unknown;
    };
    let input_column = match is_gemini {
        true => utf16_index_of(line, ">").map_or(1, |index| index + 2),
        false => utf16_length(found.as_str()),
    };
    let value = js_trim(utf16_suffix_from(line, input_column));
    let is_placeholder = match is_gemini {
        true => GEMINI_PLACEHOLDER.is_match(value),
        false => OPENCODE_PLACEHOLDER.is_match(value),
    };
    let (Some(lines), Some(row)) = (surface.viewport_lines.as_ref(), surface.cursor_row) else {
        return PaneChatInputState::Unknown;
    };
    if lines.get(row).map(String::as_str) != Some(line) {
        return PaneChatInputState::Unknown;
    }
    let block = match is_gemini {
        true => read_gemini_block(lines, row),
        false => read_opencode_block(lines, row, line, input_column, is_placeholder),
    };
    if let Some(state) = block {
        return state;
    }
    if cursor_column == input_column && (value.is_empty() || is_placeholder) {
        PaneChatInputState::Ready
    } else {
        PaneChatInputState::Occupied
    }
}

/// 경계선 배치가 맞지 않으면 unknown, 첫 줄 아래에 글이 남았으면 occupied다. 판정을 이어 가야 하면 `None`이다
fn read_gemini_block(lines: &[String], row: usize) -> Option<PaneChatInputState> {
    if row < 1 || !GEMINI_BORDER.is_match(&lines[row - 1]) {
        return Some(PaneChatInputState::Unknown);
    }
    let Some(end) = (row + 1..lines.len()).find(|index| GEMINI_BORDER.is_match(&lines[*index]))
    else {
        return Some(PaneChatInputState::Unknown);
    };
    let has_draft = lines[row + 1..end]
        .iter()
        .any(|content| !js_trim(content).is_empty());
    has_draft.then_some(PaneChatInputState::Occupied)
}

fn read_opencode_block(
    lines: &[String],
    row: usize,
    line: &str,
    input_column: usize,
    is_placeholder: bool,
) -> Option<PaneChatInputState> {
    let prefix_length = utf16_index_of(line, "┃").map_or(0, |index| index + 1);
    let prefix = utf16_prefix(line, prefix_length);
    let mut start = row;
    let mut end = row;
    while start > 0 && lines[start - 1].starts_with(prefix) {
        start -= 1;
    }
    while end + 1 < lines.len() && lines[end + 1].starts_with(prefix) {
        end += 1;
    }
    let bottom = format!(
        "{}╹▀",
        utf16_prefix(prefix, prefix_length.saturating_sub(1))
    );
    let has_bottom = lines
        .get(end + 1)
        .is_some_and(|next| next.starts_with(&bottom));
    if !has_bottom || end <= start || !lines[end].contains(" · ") {
        return Some(PaneChatInputState::Unknown);
    }
    let is_occupied = (start..end).any(|index| {
        let text = js_trim(utf16_suffix_from(&lines[index], input_column));
        !text.is_empty() && !(index == row && is_placeholder)
    });
    (is_occupied || row == end).then_some(PaneChatInputState::Occupied)
}

/// 에이전트가 응답을 만들거나 도구를 실행하는 중인지 화면 표시로 판정한다.
/// Claude 2.1.325와 Codex 0.169에서 작업 중에만 나타나고 끝나면 사라지는 것을 확인한 자리만 본다.
/// 대화 본문에 같은 문구가 있어도 속지 않게 입력란 기준 위치로 좁히고, 확인하지 않은 CLI나 화면 좌표가 없는 surface는 unknown이다
pub(super) fn read_agent_activity(
    surface: &PaneChatSurface,
    process: &PaneChatProcess,
) -> PaneChatAgentActivity {
    let (Some(lines), Some(input_row)) = (surface.viewport_lines.as_ref(), surface.cursor_row)
    else {
        return PaneChatAgentActivity::Unknown;
    };
    if input_row >= lines.len() {
        return PaneChatAgentActivity::Unknown;
    }
    let is_working = match process.provider {
        PaneChatProvider::Claude => lines[input_row + 1..]
            .iter()
            .any(|line| CLAUDE_WORKING_FOOTER_HINT.is_match(line)),
        PaneChatProvider::Codex => lines
            [input_row.saturating_sub(CODEX_WORKING_STATUS_SEARCH_ROWS)..input_row]
            .iter()
            .any(|line| CODEX_WORKING_STATUS_LINE.is_match(js_trim(line))),
        _ => return PaneChatAgentActivity::Unknown,
    };
    match is_working {
        true => PaneChatAgentActivity::Working,
        false => PaneChatAgentActivity::Idle,
    }
}
