//! Claude Code가 띄운 번호 선택 화면(AI의 질문, 도구 실행 허용 요청, 모델 전환 확인)을 읽고,
//! 대화 기록에 남은 질문 원문으로 라벨과 설명을 다듬는다.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

use super::screen_text::{as_rows, indent_of, row_at, without_whitespace};
use crate::javascript::{
    JS_WHITESPACE_CLASS, js_trim, js_trim_end, utf16_length, utf16_suffix_from,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatPromptOption {
    /// CLI 화면에 적힌 번호. 이 숫자 키를 누르면 CLI가 그 항목을 고른다
    pub number: u64,
    pub label: String,
    /// 선택지 아래에 딸린 글. 화면 너비를 넘겨 다음 줄로 넘어간 선택지의 나머지도 여기에 온다
    pub description: String,
    /// 다중 선택에서 체크했거나, 앞서 이 질문에 고른 답인 항목
    pub is_checked: bool,
    /// 고르면 그 줄에 글을 직접 입력받는 항목
    pub is_text_entry: bool,
}

/// question은 AI가 작업 중 던진 질문, permission은 도구 실행 허용 요청, confirmation은 CLI가 스스로 묻는 확인이다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneChatPromptKind {
    Question,
    Permission,
    Confirmation,
}

/// CLI가 답을 기다리며 띄운 선택 화면을 읽은 결과
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatPromptScreen {
    pub kind: PaneChatPromptKind,
    pub title: String,
    /// 제목 위에 CLI가 함께 보여 주는 내용. 권한 확인의 명령, 답 확인 화면의 고른 답 등이다
    pub detail: String,
    /// 질문이 여러 개일 때 맨 위에 나오는 단계 줄. 좌우 화살표 키로 단계를 옮길 수 있다는 표시다
    pub steps_line: Option<String>,
    pub options: Vec<PaneChatPromptOption>,
    /// CLI가 지금 강조하고 있는 선택지의 번호
    pub highlighted_number: u64,
    pub is_multi_select: bool,
    /// 강조된 줄이 글을 입력받는 중인지. 이때 숫자 키는 선택이 아니라 글자로 들어간다
    pub is_text_entry_active: bool,
    /// pane이 짧아 CLI가 선택지를 스크롤해 보여 주는 중이라, 화면에 없는 선택지가 더 있는지
    pub has_hidden_options: bool,
}

fn compile(pattern: &str) -> Regex {
    Regex::new(&pattern.replace("\\s", &format!("[{JS_WHITESPACE_CLASS}]")))
        .expect("선택 화면 패턴")
}

/// 선택지 한 줄. 강조된 줄은 번호 앞에 ❯가 붙고 나머지 줄은 그 자리가 비어 있다. 스크롤된 목록은 가려진 쪽 끝 줄의 그 자리에 화살표를 둔다
static OPTION_LINE: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^(\s*)(❯|↑|↓|\s) ([0-9]+)\. ([^\n\r\x{2028}\x{2029}]*)$"));
/// 선택 화면이 시작되는 위쪽 경계선. CLI가 이 줄에 안내 글을 넣으면 경계선은 줄 끝에만 남는다
static TOP_BORDER: LazyLock<Regex> = LazyLock::new(|| compile(r"^\s*[─▔]{10,}\s*$|\s[─▔]+\s*$"));
/// 선택 화면 안에서 내용을 나누는 가로줄
static DIVIDER: LazyLock<Regex> = LazyLock::new(|| compile(r"^\s*[─╌▔]{3,}\s*$"));
static CHECKBOX: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^\[([^\n\r\x{2028}\x{2029}])\] ([^\n\r\x{2028}\x{2029}]*)$"));
/// 앞서 고른 답 뒤에 붙는 표시. 뒤 조건(공백이나 줄 끝)은 [`find_chosen_mark`]가 따로 본다
static CHOSEN_MARK_BODY: LazyLock<Regex> = LazyLock::new(|| compile(r"\s+✔"));
/// AI의 질문 화면에만 있는 선택지
const QUESTION_ONLY_OPTION: &str = "Chat about this";
/// 도구 실행이나 계획 진행의 허용을 묻는 화면의 제목. Claude와 Codex가 같은 말로 묻는다
pub static PERMISSION_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^(?:Do you want|Would you like) to ").expect("권한 제목 패턴"));
static STEPS_LINE: LazyLock<Regex> = LazyLock::new(|| compile(r"^←\s[^\n\r\x{2028}\x{2029}]*\s→$"));
/// 글을 직접 입력받는 줄이 비어 있을 때 Claude가 보여 주는 자리 표시 글
static TEXT_ENTRY_PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^Type something\.?$").expect("입력 자리 표시 패턴"));
/// 번호 칸에서 선택지 글이나 설명이 시작하는 칸까지의 최소 들여쓰기
const OPTION_BODY_INDENT: usize = 3;
/// 선택지와 제목 사이에 허용하는 빈 줄 수
const MAX_BLANK_ROWS_BELOW_TITLE: usize = 1;

/// JS `/\s+✔(?=\s|$)/`의 첫 일치 범위. Rust regex에 앞보기가 없어 뒤 글자를 따로 확인한다
fn find_chosen_mark(text: &str) -> Option<std::ops::Range<usize>> {
    CHOSEN_MARK_BODY
        .find_iter(text)
        .find(|found| {
            text[found.end()..]
                .chars()
                .next()
                .is_none_or(crate::javascript::is_js_whitespace)
        })
        .map(|found| found.range())
}

/// 직접 입력 줄에 아직 아무 글도 넣지 않았는지
pub fn is_blank_claude_text_entry(option: &PaneChatPromptOption) -> bool {
    option.is_text_entry && TEXT_ENTRY_PLACEHOLDER.is_match(&option.label)
}

/// 화면 줄이 선택지 줄이면 그 들여쓰기·표시·번호·글
struct OptionLine<'a> {
    indent: &'a str,
    marker: &'a str,
    number: u64,
    text: &'a str,
}

fn parse_option_line(line: &str) -> Option<OptionLine<'_>> {
    let found = OPTION_LINE.captures(line)?;
    Some(OptionLine {
        indent: found.get(1)?.as_str(),
        marker: found.get(2)?.as_str(),
        number: found[3].parse().ok()?,
        text: found.get(4)?.as_str(),
    })
}

/// 화면을 읽는 동안 쓰는 줄 목록과 강조된 선택지의 표시 칸
struct PromptRows<'a> {
    lines: Vec<&'a str>,
    marker_column: usize,
}

impl PromptRows<'_> {
    fn option_number_at(&self, row: usize) -> Option<u64> {
        let option = parse_option_line(row_at(&self.lines, row))?;
        (utf16_length(option.indent) == self.marker_column).then_some(option.number)
    }

    fn is_inside_options(&self, row: usize) -> bool {
        let line = row_at(&self.lines, row);
        self.option_number_at(row).is_some()
            || DIVIDER.is_match(line)
            || (!js_trim(line).is_empty()
                && indent_of(line) >= self.marker_column + OPTION_BODY_INDENT)
    }

    /// 커서 줄을 둘러싼 선택지 목록의 첫 줄과 끝 줄
    fn option_bounds(&self, cursor_row: usize) -> (usize, usize) {
        let mut first_row = cursor_row;
        while first_row > 0 && self.is_inside_options(first_row - 1) {
            first_row -= 1;
        }
        // 위로 더듬다 닿은 딸린 줄과 가로줄은 선택지가 아니다. 목록은 선택지 줄에서 시작한다
        while self.option_number_at(first_row).is_none() {
            first_row += 1;
        }
        let mut last_row = cursor_row;
        while last_row + 1 < self.lines.len() && self.is_inside_options(last_row + 1) {
            last_row += 1;
        }
        (first_row, last_row)
    }

    /// 선택지 바로 위 제목의 첫 줄과 끝 줄. 제목이 없으면 `None`이다
    fn title_bounds(&self, first_row: usize) -> Option<(usize, usize)> {
        let mut title_end = first_row as isize - 1;
        while title_end >= 0
            && js_trim(self.lines[title_end as usize]).is_empty()
            && (first_row as isize - 1 - title_end) < MAX_BLANK_ROWS_BELOW_TITLE as isize
        {
            title_end -= 1;
        }
        let mut title_start = title_end + 1;
        while title_start > 0 {
            let line = self.lines[title_start as usize - 1];
            if js_trim(line).is_empty() || DIVIDER.is_match(line) || TOP_BORDER.is_match(line) {
                break;
            }
            title_start -= 1;
        }
        (title_start <= title_end).then_some((title_start as usize, title_end as usize))
    }

    /// 제목 위, 위쪽 경계선 아래의 줄. 경계선이 화면 밖으로 밀려나 있으면 비어 있다
    fn rows_above_title(&self, title_start: usize) -> Vec<&str> {
        let border_row = (0..title_start)
            .rev()
            .find(|row| TOP_BORDER.is_match(self.lines[*row]));
        border_row.map_or_else(Vec::new, |row| self.lines[row + 1..title_start].to_vec())
    }

    /// 선택지 위의 제목, 단계 줄, 상세. 제목이 없으면 `None`이다
    fn read_heading(&self, first_row: usize) -> Option<PromptHeading<'_>> {
        let (title_start, title_end) = self.title_bounds(first_row)?;
        let above = self.rows_above_title(title_start);
        let steps_line = above
            .iter()
            .map(|line| js_trim(line))
            .find(|line| STEPS_LINE.is_match(line))
            .map(str::to_string);
        let detail_lines: Vec<&str> = above
            .iter()
            .copied()
            .filter(|line| Some(js_trim(line)) != steps_line.as_deref())
            .collect();
        Some(PromptHeading {
            title_lines: self.lines[title_start..=title_end]
                .iter()
                .map(|line| js_trim(line))
                .collect(),
            detail: format_detail(&detail_lines),
            steps_line,
        })
    }
}

/// 선택지 위에 놓인 글
struct PromptHeading<'a> {
    title_lines: Vec<&'a str>,
    steps_line: Option<String>,
    detail: String,
}

/// 선택지 한 개가 화면에서 차지한 줄들. 첫 줄의 글과, 그 아래 딸린 줄을 읽은 그대로 담는다
struct OptionRows {
    number: u64,
    label: String,
    is_checked: bool,
    /// 선택지 글이 시작하는 칸. 딸린 줄은 이 칸 이상으로 들여쓴다
    body_column: usize,
    first_row_length: usize,
    continuation_rows: Vec<String>,
}

/// 강조된 줄과 커서 위치. 커서가 표시 칸이 아니면 그 줄에 글을 입력받는 중이다
struct Highlight {
    marker_column: usize,
    number: u64,
    is_text_entry_active: bool,
}

fn read_highlight(cursor_line: &str, cursor_column: usize) -> Option<Highlight> {
    let highlighted = parse_option_line(cursor_line)?;
    if highlighted.marker != "❯" {
        return None;
    }
    let marker_column = utf16_length(highlighted.indent);
    let label_column = utf16_length(cursor_line) - utf16_length(highlighted.text);
    let is_text_entry_active = cursor_column != marker_column;
    if is_text_entry_active && cursor_column < label_column {
        return None;
    }
    Some(Highlight {
        marker_column,
        number: highlighted.number,
        is_text_entry_active,
    })
}

/// Claude Code가 띄운 번호 선택 화면을 읽는다. AI의 질문, 도구 실행 허용 요청, 모델 전환 확인이 모두 같은 모양이다.
///
/// Claude는 선택 화면이 떠 있는 동안 커서를 강조된 선택지의 ❯ 위에 두고, 그 줄에 글을 입력받는 동안에는 글 위에 둔다.
/// 입력란이나 대화 본문에 번호 목록이 있어도 속지 않도록 이 커서 위치와, 선택지 바로 위에 제목이 있는지를 함께 본다.
/// Claude Code 2.1.326의 실제 화면으로 확인한 배치이고, 읽지 못하는 화면은 `None`이다.
/// 화면 너비를 주면 너비를 넘겨 다음 줄로 넘어간 선택지를 한 라벨로 이어 붙인다
pub fn read_claude_prompt<S: AsRef<str>>(
    lines: &[S],
    cursor_row: usize,
    cursor_column: usize,
    viewport_columns: Option<usize>,
) -> Option<PaneChatPromptScreen> {
    let lines = as_rows(lines);
    let highlight = read_highlight(row_at(&lines, cursor_row), cursor_column)?;
    let rows = PromptRows {
        lines,
        marker_column: highlight.marker_column,
    };
    let (first_row, last_row) = rows.option_bounds(cursor_row);
    let option_lines = &rows.lines[first_row..=last_row];
    let has_hidden_options =
        has_option_marked(option_lines, |option| matches!(option.marker, "↑" | "↓"));
    let option_rows = read_option_rows(option_lines, highlight.marker_column);
    if option_rows.len() < 2 || !is_numbered_in_order(&option_rows, has_hidden_options) {
        return None;
    }
    let heading = rows.read_heading(first_row)?;
    let is_question = heading.steps_line.is_some()
        || option_rows
            .iter()
            .any(|option| option.label == QUESTION_ONLY_OPTION);
    let layout = OptionLayout::new(viewport_columns, is_question, &highlight);
    Some(PaneChatPromptScreen {
        kind: prompt_kind(
            is_question,
            heading.title_lines.last().copied().unwrap_or_default(),
        ),
        title: heading.title_lines.join("\n"),
        detail: heading.detail,
        steps_line: heading.steps_line,
        options: option_rows
            .iter()
            .map(|option| describe_option(option, &layout))
            .collect(),
        highlighted_number: highlight.number,
        is_multi_select: has_option_marked(option_lines, |option| CHECKBOX.is_match(option.text)),
        is_text_entry_active: highlight.is_text_entry_active,
        has_hidden_options,
    })
}

/// 선택지 줄 가운데 조건에 맞는 줄이 있는지. 들여쓰기 칸은 따지지 않는다
fn has_option_marked(option_lines: &[&str], is_marked: impl Fn(&OptionLine<'_>) -> bool) -> bool {
    option_lines
        .iter()
        .any(|line| parse_option_line(line).is_some_and(|option| is_marked(&option)))
}

fn prompt_kind(is_question: bool, last_title_line: &str) -> PaneChatPromptKind {
    if is_question {
        PaneChatPromptKind::Question
    } else if PERMISSION_TITLE.is_match(last_title_line) {
        PaneChatPromptKind::Permission
    } else {
        PaneChatPromptKind::Confirmation
    }
}

fn is_numbered_in_order(options: &[OptionRows], has_hidden_options: bool) -> bool {
    options.iter().enumerate().all(|(index, option)| {
        if has_hidden_options {
            index == 0 || option.number > options[index - 1].number
        } else {
            option.number == index as u64 + 1
        }
    })
}

/// 선택지 줄과 그 아래 딸린 줄을 묶는다. 선택지 글보다 덜 들여쓴 줄(다중 선택의 Submit 줄)은 어느 선택지에도 속하지 않는다
fn read_option_rows(option_lines: &[&str], marker_column: usize) -> Vec<OptionRows> {
    let mut options: Vec<OptionRows> = Vec::new();
    for line in option_lines {
        match parse_option_line(line) {
            Some(option) if utf16_length(option.indent) == marker_column => {
                options.push(new_option_rows(line, &option));
            }
            _ => {
                if let Some(current) = options.last_mut()
                    && !DIVIDER.is_match(line)
                    && indent_of(line) >= current.body_column
                {
                    current.continuation_rows.push(line.to_string());
                }
            }
        }
    }
    options
}

fn new_option_rows(line: &str, option: &OptionLine<'_>) -> OptionRows {
    let checkbox = CHECKBOX.captures(option.text);
    let text = checkbox
        .as_ref()
        .and_then(|found| found.get(2))
        .map_or(option.text, |found| found.as_str());
    let chosen_mark = find_chosen_mark(text);
    let unmarked = match &chosen_mark {
        Some(range) => format!("{}{}", &text[..range.start], &text[range.end..]),
        None => text.to_string(),
    };
    OptionRows {
        number: option.number,
        label: js_trim(&unmarked).to_string(),
        is_checked: match &checkbox {
            Some(found) => &found[1] != " ",
            None => chosen_mark.is_some(),
        },
        body_column: utf16_length(line) - utf16_length(text),
        first_row_length: utf16_length(js_trim_end(line)),
        continuation_rows: Vec::new(),
    }
}

/// 딸린 줄을 가르는 데 쓰는 화면 조건
struct OptionLayout {
    wrap_column: Option<isize>,
    has_description_rows: bool,
    text_entry_number: Option<u64>,
}

impl OptionLayout {
    fn new(viewport_columns: Option<usize>, is_question: bool, highlight: &Highlight) -> Self {
        Self {
            // Claude는 선택 화면의 글을 pane 너비보다 한 칸 좁게 접는다
            wrap_column: viewport_columns
                .filter(|columns| *columns > 0)
                .map(|columns| columns as isize - 1),
            // 선택지 아래에 설명 줄이 따로 붙는 것은 AI의 질문뿐이다
            has_description_rows: is_question,
            text_entry_number: highlight.is_text_entry_active.then_some(highlight.number),
        }
    }
}

struct ContinuationRow {
    text: String,
    length: usize,
}

fn first_word(text: &str) -> &str {
    text.split(' ').next().unwrap_or_default()
}

/// 딸린 줄 가운데 앞에서부터 몇 줄이 라벨의 나머지인지.
///
/// 너비를 넘겨 넘어온 줄은 앞줄에 그 첫 낱말이 들어갈 자리가 없었던 줄이다. 자리가 있었는데도 줄을 바꿨다면 거기서부터가 설명이다.
/// 모든 줄이 자리가 없었던 경우는 화면만으로 가를 수 없다. 설명 줄이 붙는 화면이면 첫 줄만 라벨로 보고, 아니면 전부 라벨로 본다
fn count_label_rows(option: &OptionRows, rows: &[ContinuationRow], layout: &OptionLayout) -> usize {
    let mut previous_row_length = option.first_row_length as isize;
    let first_separate_row = rows.iter().position(|row| {
        let next_word_length = utf16_length(first_word(&row.text)) as isize;
        let had_room = layout
            .wrap_column
            .is_none_or(|wrap| previous_row_length + 1 + next_word_length <= wrap);
        previous_row_length = row.length as isize;
        had_room
    });
    match first_separate_row {
        Some(row) => row,
        None if layout.has_description_rows => 0,
        None => rows.len(),
    }
}

fn describe_option(option: &OptionRows, layout: &OptionLayout) -> PaneChatPromptOption {
    let rows: Vec<ContinuationRow> = option
        .continuation_rows
        .iter()
        .map(|line| ContinuationRow {
            text: js_trim(line).to_string(),
            length: utf16_length(js_trim_end(line)),
        })
        .collect();
    let label_row_count = count_label_rows(option, &rows, layout);
    let wrap_column = layout.wrap_column.unwrap_or(0);
    let mut label = option.label.clone();
    let mut previous_row_length = option.first_row_length as isize;
    for row in &rows[..label_row_count] {
        // 한 줄에 다 들어가지 않는 낱말만 가운데에서 끊긴다. 그때는 끊긴 두 조각 사이에 공백이 없었다
        let last_word = label.split(' ').next_back().unwrap_or_default();
        let broken_word_length = utf16_length(last_word) + utf16_length(first_word(&row.text));
        let is_broken_inside_word = previous_row_length >= wrap_column
            && broken_word_length as isize > wrap_column - option.body_column as isize;
        let separator = if is_broken_inside_word { "" } else { " " };
        label = format!("{label}{separator}{}", row.text);
        previous_row_length = row.length as isize;
    }
    let description = rows[label_row_count..]
        .iter()
        .map(|row| row.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    PaneChatPromptOption {
        number: option.number,
        is_text_entry: TEXT_ENTRY_PLACEHOLDER.is_match(&label)
            || Some(option.number) == layout.text_entry_number,
        label,
        description,
        is_checked: option.is_checked,
    }
}

static TRIPLE_NEWLINES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\n{3,}").expect("빈 줄 묶음 패턴"));

/// 가로줄을 빈 줄로 바꾸고, 앞뒤 빈 줄과 공통 들여쓰기를 걷어 낸다
fn format_detail(detail_lines: &[&str]) -> String {
    let lines: Vec<&str> = detail_lines
        .iter()
        .map(|line| {
            if DIVIDER.is_match(line) {
                ""
            } else {
                js_trim_end(line)
            }
        })
        .collect();
    let indent = lines
        .iter()
        .filter(|line| !line.is_empty())
        .map(|line| indent_of(line))
        .min();
    let joined = lines
        .iter()
        .map(|line| indent.map_or("", |indent| utf16_suffix_from(line, indent)))
        .collect::<Vec<_>>()
        .join("\n");
    js_trim(&TRIPLE_NEWLINES.replace_all(&joined, "\n\n")).to_string()
}

/// 대화 기록에 남은 AI의 질문 하나. 화면에서는 가를 수 없는 라벨과 설명의 경계가 여기에 있다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeAskedQuestion {
    pub question: String,
    pub options: Vec<ClaudeAskedOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeAskedOption {
    pub label: String,
    pub description: String,
}

/// Claude가 사용자에게 질문할 때 부르는 도구
const QUESTION_TOOL: &str = "AskUserQuestion";

/// 대화 기록 끝에서 가장 최근의 질문 도구 호출을 찾아 질문과 선택지를 꺼낸다. 없거나 형식이 다르면 빈 목록이다
pub fn read_claude_asked_questions(records: &[Value]) -> Vec<ClaudeAskedQuestion> {
    for record in records.iter().rev() {
        let Some(content) = record
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        let Some(call) = content.iter().rev().find(|part| {
            part.get("type").and_then(Value::as_str) == Some("tool_use")
                && part.get("name").and_then(Value::as_str) == Some(QUESTION_TOOL)
        }) else {
            continue;
        };
        let questions = call.get("input").and_then(|input| input.get("questions"));
        return questions
            .and_then(Value::as_array)
            .map_or_else(Vec::new, |questions| {
                questions.iter().filter_map(read_asked_question).collect()
            });
    }
    Vec::new()
}

fn read_asked_question(asked: &Value) -> Option<ClaudeAskedQuestion> {
    let question = asked.get("question")?.as_str()?;
    let options = asked.get("options")?.as_array()?;
    Some(ClaudeAskedQuestion {
        question: question.to_string(),
        options: options
            .iter()
            .filter_map(|option| {
                Some(ClaudeAskedOption {
                    label: option.get("label")?.as_str()?.to_string(),
                    description: option
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                })
            })
            .collect(),
    })
}

/// 화면에서 읽은 AI의 질문에, 대화 기록에 남은 질문 원문의 라벨과 설명을 입힌다.
///
/// 원문의 질문이 화면의 제목과 같고, 선택지의 글이 화면의 그 번호 선택지와 공백만 다를 때에만 원문으로 바꾼다.
/// 맞지 않으면 화면에서 읽은 그대로 둔다
pub fn apply_asked_questions(
    mut screen: PaneChatPromptScreen,
    asked_questions: &[ClaudeAskedQuestion],
) -> PaneChatPromptScreen {
    if screen.kind != PaneChatPromptKind::Question {
        return screen;
    }
    let title = without_whitespace(&screen.title);
    let Some(asked) = asked_questions
        .iter()
        .find(|candidate| without_whitespace(&candidate.question) == title)
    else {
        return screen;
    };
    for option in &mut screen.options {
        let original = (option.number as usize)
            .checked_sub(1)
            .and_then(|index| asked.options.get(index));
        let Some(original) = original else {
            continue;
        };
        let is_same_text =
            without_whitespace(&format!("{}{}", original.label, original.description))
                == without_whitespace(&format!("{}{}", option.label, option.description));
        if is_same_text {
            option.label = js_trim(&original.label).to_string();
            option.description = js_trim(&original.description).to_string();
        }
    }
    screen
}

#[cfg(test)]
mod tests;
