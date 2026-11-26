//! 편집 모드 편집기의 글 내용과 커서·선택, 실행 취소. TS 앱은 Monaco `Editor`가 이 일을 했다.
//! 위치는 줄 번호와 줄 안 바이트 위치(`TextPoint`)이고, 언제나 글자 경계에 놓인다.

use std::ops::Range;

use super::auto_closing::bracket_around;
use super::line_diff::TextPoint;
use super::syntax::Language;
use super::typescript_config::{EnterAction, enter_action};
use super::word_boundary::{next_word_end, previous_word_start, word_range_at};

const DEFAULT_INDENT_WIDTH: usize = 4;
/// 실행 취소 기록은 문서 전체 사본이라, 개수와 합친 크기 둘 다 넘지 않게 오래된 것부터 버린다
const MAX_UNDO_SNAPSHOTS: usize = 1000;
const MAX_UNDO_HISTORY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    lines: Vec<String>,
    cursor: TextPoint,
    anchor: TextPoint,
    /// 줄 글자 바이트 합. 기록 상한을 잴 때 줄을 다시 세지 않으려고 만들 때 한 번 센다
    byte_size: usize,
}

/// 마지막 편집이 이어 붙일 수 있는 종류였는지. 연속 입력과 연속 지우기는 실행 취소 한 번에 함께 되돌린다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Typing,
    Deleting,
    Other,
}

#[derive(Debug, Clone)]
pub struct TextBuffer {
    lines: Vec<String>,
    cursor: TextPoint,
    /// 선택의 반대쪽 끝. 커서와 같으면 선택이 없다
    anchor: TextPoint,
    /// 위아래로 움직일 때 지키는 줄 안 글자 수
    preferred_column: Option<usize>,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    last_edit: Option<EditKind>,
    /// 내용이 바뀔 때마다 1씩 는다. 저장 중 더 편집했는지 판단한다(TS `editRevisionRef`)
    revision: u64,
    indent_unit: String,
    /// 저장할 때 되돌릴 원래 줄바꿈(Monaco 모델의 EOL)
    line_ending: &'static str,
}

impl TextBuffer {
    pub fn new(text: &str) -> Self {
        let line_ending = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let lines: Vec<String> = text
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
            .collect();
        let indent_unit = " ".repeat(detect_indent_width(&lines));
        let start = TextPoint { line: 0, column: 0 };
        Self {
            lines,
            cursor: start,
            anchor: start,
            preferred_column: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_edit: None,
            revision: 0,
            indent_unit,
            line_ending,
        }
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn text(&self) -> String {
        self.lines.join(self.line_ending)
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        Some(self.text_between(start, end, self.line_ending))
    }

    /// 입력기가 묻는 UTF-16 범위의 글과, 문서 안으로 맞춘 범위. 줄바꿈은 `utf16_offset`처럼 한 단위로 센다
    pub fn text_for_utf16_range(&self, range: Range<usize>) -> (String, Range<usize>) {
        let (start, end) = ordered(
            self.point_for_utf16_offset(range.start),
            self.point_for_utf16_offset(range.end),
        );
        let adjusted_range = self.utf16_offset(start)..self.utf16_offset(end);
        (self.text_between(start, end, "\n"), adjusted_range)
    }

    fn text_between(&self, start: TextPoint, end: TextPoint, line_separator: &str) -> String {
        if start.line == end.line {
            return self.lines[start.line][start.column..end.column].to_string();
        }
        let mut parts = vec![&self.lines[start.line][start.column..]];
        parts.extend(
            self.lines[start.line + 1..end.line]
                .iter()
                .map(String::as_str),
        );
        parts.push(&self.lines[end.line][..end.column]);
        parts.join(line_separator)
    }

    pub fn cursor(&self) -> TextPoint {
        self.cursor
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// 선택 범위(앞, 뒤). 선택이 없으면 `None`
    pub fn selection(&self) -> Option<(TextPoint, TextPoint)> {
        (self.cursor != self.anchor).then(|| ordered(self.cursor, self.anchor))
    }

    pub fn set_cursor(&mut self, point: TextPoint, is_extending: bool) {
        self.cursor = self.clamp(point);
        if !is_extending {
            self.anchor = self.cursor;
        }
        self.preferred_column = None;
        self.last_edit = None;
    }

    pub fn select_all(&mut self) {
        self.anchor = TextPoint { line: 0, column: 0 };
        let last_line = self.lines.len() - 1;
        self.cursor = TextPoint {
            line: last_line,
            column: self.lines[last_line].len(),
        };
    }

    pub fn insert_text(&mut self, text: &str) {
        let kind = if text.chars().count() == 1 && text != "\n" {
            EditKind::Typing
        } else {
            EditKind::Other
        };
        self.record_undo(kind);
        self.delete_selection();
        self.insert_raw(text);
        self.finish_edit();
    }

    /// 커서 자리에 글자를 넣고 커서를 그 뒤로 옮긴다. 실행 취소 기록은 부르는 쪽이 맡는다
    fn insert_raw(&mut self, text: &str) {
        let start = self.cursor;
        let line = &self.lines[start.line];
        let (head, tail) = (
            line[..start.column].to_string(),
            line[start.column..].to_string(),
        );
        let mut new_lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        new_lines[0] = head + &new_lines[0];
        let last_index = new_lines.len() - 1;
        let cursor_column = new_lines[last_index].len();
        new_lines[last_index].push_str(&tail);
        self.lines.splice(start.line..=start.line, new_lines);
        self.cursor = TextPoint {
            line: start.line + last_index,
            column: cursor_column,
        };
    }

    /// Enter. 지금 줄의 들여쓰기를 이어 가고, 여는 괄호 뒤면 한 단계 더 들인다. `{|}` 사이면 닫는 괄호를 다음 줄로 내린다
    /// (Monaco `autoIndent: full`과 괄호 `onEnterRules`)
    /// TypeScript 계열은 문서 주석 규칙(`onEnterRules`)을 괄호 규칙보다 먼저 본다
    pub fn insert_newline(&mut self, language: Language) {
        let (start, end) = self.selection().unwrap_or((self.cursor, self.cursor));
        let before = &self.lines[start.line][..start.column];
        let after = &self.lines[end.line][end.column..];
        if let Some(action) = enter_action(language, before, after) {
            self.apply_enter_action(action, start);
            return;
        }
        let line = &self.lines[self.cursor.line];
        let indentation: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let (is_after_open, is_before_close) = bracket_around(line, self.cursor.column);
        if !is_after_open || self.selection().is_some() {
            self.insert_text(&format!("\n{indentation}"));
            return;
        }
        let inner = format!("{indentation}{}", self.indent_unit);
        if !is_before_close {
            self.insert_text(&format!("\n{inner}"));
            return;
        }
        self.insert_text(&format!("\n{inner}\n{indentation}"));
        self.cursor = TextPoint {
            line: self.cursor.line - 1,
            column: inner.len(),
        };
        self.anchor = self.cursor;
    }

    /// Monaco `TypeOperations._enter`: 새 줄 들여쓰기는 커서 줄의 앞 공백(커서까지, `removeText`만큼 덜어 냄)에 `appendText`를 붙인다.
    /// `IndentOutdent`면 커서 뒤 글자를 원래 들여쓰기로 한 줄 더 내리고 커서는 가운데 줄 끝에 둔다
    fn apply_enter_action(&mut self, action: EnterAction, start: TextPoint) {
        let leading: String = self.lines[start.line][..start.column]
            .chars()
            .take_while(|character| matches!(character, ' ' | '\t'))
            .collect();
        let kept = leading.chars().count().saturating_sub(action.remove_text);
        let indentation: String = leading.chars().take(kept).collect();
        let inner = format!("{indentation}{}", action.append_text);
        if !action.is_indent_outdent {
            self.insert_text(&format!("\n{inner}"));
            return;
        }
        self.insert_text(&format!("\n{inner}\n{indentation}"));
        self.cursor = TextPoint {
            line: self.cursor.line - 1,
            column: inner.len(),
        };
        self.anchor = self.cursor;
    }

    /// 자동 닫기: 여는 글자와 닫는 글자를 넣고 커서를 사이에 둔다
    pub fn insert_pair(&mut self, open: char, close: &str) {
        self.insert_text(&format!("{open}{close}"));
        self.cursor.column -= close.len();
        self.anchor = self.cursor;
    }

    /// 자동 감싸기: 선택을 여는·닫는 글자로 감싸고 안쪽 글자를 다시 고른다
    pub fn surround_selection(&mut self, open: char, close: char) {
        let Some((start, end)) = self.selection() else {
            return;
        };
        let selected = self.selected_text().unwrap_or_default();
        self.insert_text(&format!("{open}{selected}{close}"));
        let inner_start = TextPoint {
            line: start.line,
            column: start.column + open.len_utf8(),
        };
        let inner_end = if start.line == end.line {
            TextPoint {
                line: end.line,
                column: end.column + open.len_utf8(),
            }
        } else {
            end
        };
        self.anchor = inner_start;
        self.cursor = inner_end;
    }

    /// 커서 앞뒤 글자를 한 번에 지운다(자동으로 넣은 빈 쌍 지우기)
    pub fn delete_surrounding_pair(&mut self) {
        let start = self.step_left(self.cursor);
        let end = self.step_right(self.cursor);
        self.record_undo(EditKind::Other);
        self.anchor = start;
        self.cursor = end;
        self.delete_selection();
        self.finish_edit();
    }

    /// 단어 단위 이동. 줄 처음·끝에서는 이웃 줄로 넘어간다
    pub fn move_word(&mut self, is_forward: bool, is_extending: bool) {
        let target = self.word_target(is_forward);
        self.set_cursor(target, is_extending);
    }

    /// Ctrl+Backspace·Ctrl+Delete(macOS Alt). 선택이 있으면 선택을 지운다
    pub fn delete_word(&mut self, is_forward: bool) {
        if self.selection().is_none() {
            let target = self.word_target(is_forward);
            if target == self.cursor {
                return;
            }
            self.anchor = target;
        }
        self.record_undo(EditKind::Other);
        self.delete_selection();
        self.finish_edit();
    }

    fn word_target(&self, is_forward: bool) -> TextPoint {
        let line = &self.lines[self.cursor.line];
        let column = self.cursor.column;
        let is_at_edge = if is_forward {
            line[column..].trim().is_empty() && column == line.len()
        } else {
            column == 0
        };
        if is_at_edge {
            return if is_forward {
                self.step_right(self.cursor)
            } else {
                self.step_left(self.cursor)
            };
        }
        let column = if is_forward {
            next_word_end(line, column)
        } else {
            previous_word_start(line, column)
        };
        TextPoint {
            line: self.cursor.line,
            column,
        }
    }

    /// 두 번 누르기: 누른 자리의 단어를 고른다
    pub fn select_word_at(&mut self, point: TextPoint) {
        let point = self.clamp(point);
        let range = word_range_at(&self.lines[point.line], point.column);
        self.anchor = TextPoint {
            line: point.line,
            column: range.start,
        };
        self.cursor = TextPoint {
            line: point.line,
            column: range.end,
        };
        self.preferred_column = None;
    }

    /// 세 번 누르기: 줄 전체를 줄바꿈까지 고른다
    pub fn select_line(&mut self, line: usize) {
        let line = line.min(self.lines.len() - 1);
        self.anchor = TextPoint { line, column: 0 };
        self.cursor = if line + 1 < self.lines.len() {
            TextPoint {
                line: line + 1,
                column: 0,
            }
        } else {
            TextPoint {
                line,
                column: self.lines[line].len(),
            }
        };
        self.preferred_column = None;
    }

    /// 선택을 문서 위치 둘로 바꾼다(찾기 결과 고르기)
    pub fn select_range(&mut self, start: TextPoint, end: TextPoint) {
        self.anchor = self.clamp(start);
        self.cursor = self.clamp(end);
        self.preferred_column = None;
    }

    /// 찾아 바꾸기. 범위마다 바꿀 글자가 따로 있고(정규식 `$1` 치환), 여러 곳을 한 번의 실행 취소로 되돌린다.
    /// 범위는 뒤에서부터 바꿔 앞 범위의 위치가 흔들리지 않게 한다
    pub fn replace_ranges(&mut self, replacements: &[(TextPoint, TextPoint, String)]) {
        if replacements.is_empty() {
            return;
        }
        self.record_undo(EditKind::Other);
        let mut sorted = replacements.to_vec();
        sorted.sort();
        for (start, end, replacement) in sorted.into_iter().rev() {
            self.anchor = start;
            self.cursor = end;
            self.delete_selection();
            self.insert_raw(&replacement);
        }
        self.finish_edit();
    }

    pub fn insert_indent(&mut self) {
        let unit = self.indent_unit.clone();
        self.insert_text(&unit);
    }

    /// Tab(Monaco `TypeOperations.tab`). 여러 줄이나 한 줄 전체를 골랐으면 고른 줄을 들여 쓰고 선택을 남긴다.
    /// 선택이 없거나 한 줄의 일부만 골랐으면 그 자리에 들여쓰기 한 단위를 넣는다
    pub fn indent(&mut self) {
        match self.is_whole_line_selection() {
            true => self.shift_selected_lines(true),
            false => self.insert_indent(),
        }
    }

    /// Shift+Tab(Monaco 내어쓰기). 선택이 없어도 커서 줄을 한 단계 내어 쓴다
    pub fn outdent(&mut self) {
        self.shift_selected_lines(false);
    }

    fn is_whole_line_selection(&self) -> bool {
        let Some((start, end)) = self.selection() else {
            return false;
        };
        start.line != end.line || (start.column == 0 && end.column == self.lines[end.line].len())
    }

    /// 선택이 걸친 줄. 여러 줄 선택이 다음 줄 맨 앞에서 끝나면 그 줄은 빼다(Monaco `ShiftCommand`)
    fn selected_lines(&self) -> std::ops::RangeInclusive<usize> {
        let (start, end) = self.selection().unwrap_or((self.cursor, self.cursor));
        let last_line = match end.line > start.line && end.column == 0 {
            true => end.line - 1,
            false => end.line,
        };
        start.line..=last_line
    }

    /// 고른 줄마다 앞 공백을 다음(들여쓰기)·이전(내어쓰기) 들여쓰기 자리로 맞춘다. 여러 줄을 들일 때 빈 줄은 건너뛴다.
    /// 바뀐 줄이 없으면 실행 취소 기록도 남기지 않는다
    fn shift_selected_lines(&mut self, is_indenting: bool) {
        let lines = self.selected_lines();
        let is_multi_line = lines.start() != lines.end();
        let indent_width = self.indent_unit.len();
        let leading_changes: Vec<(usize, isize)> = lines
            .filter_map(|line| {
                let text = &self.lines[line];
                let change = match is_indenting {
                    true if is_multi_line && text.is_empty() => 0,
                    true => indent_change(text, indent_width),
                    false => outdent_change(text, indent_width),
                };
                (change != 0).then_some((line, change))
            })
            .collect();
        if leading_changes.is_empty() {
            return;
        }
        self.record_undo(EditKind::Other);
        for (line, change) in &leading_changes {
            let text = &mut self.lines[*line];
            match *change > 0 {
                true => text.insert_str(0, &" ".repeat(change.unsigned_abs())),
                false => {
                    text.drain(..change.unsigned_abs());
                }
            }
        }
        self.cursor = shifted_point(self.cursor, &leading_changes);
        self.anchor = shifted_point(self.anchor, &leading_changes);
        self.preferred_column = None;
        self.revision += 1;
    }

    pub fn delete_backward(&mut self) {
        let mut kind = EditKind::Other;
        if self.selection().is_none() {
            let previous = self.step_left(self.cursor);
            if previous == self.cursor {
                return;
            }
            self.anchor = previous;
            kind = EditKind::Deleting;
        }
        self.record_undo(kind);
        self.delete_selection();
        self.finish_edit();
    }

    pub fn delete_forward(&mut self) {
        let mut kind = EditKind::Other;
        if self.selection().is_none() {
            let next = self.step_right(self.cursor);
            if next == self.cursor {
                return;
            }
            self.anchor = next;
            kind = EditKind::Deleting;
        }
        self.record_undo(kind);
        self.delete_selection();
        self.finish_edit();
    }

    pub fn move_horizontally(&mut self, is_forward: bool, is_extending: bool) {
        let target = match (self.selection(), is_extending) {
            (Some((start, end)), false) => {
                if is_forward {
                    end
                } else {
                    start
                }
            }
            _ if is_forward => self.step_right(self.cursor),
            _ => self.step_left(self.cursor),
        };
        self.set_cursor(target, is_extending);
    }

    /// 위아래 이동의 목표 줄로 간다. 줄 안 글자 수는 처음 움직이기 시작한 자리를 지킨다(접힌 줄을 건너뛸 때도 쓴다)
    pub fn move_to_line(&mut self, target_line: usize, is_extending: bool) {
        let column_chars = self.preferred_column.unwrap_or_else(|| {
            self.lines[self.cursor.line][..self.cursor.column]
                .chars()
                .count()
        });
        let target_line = target_line.min(self.lines.len() - 1);
        let target_column = byte_index_for_char(&self.lines[target_line], column_chars);
        self.set_cursor(
            TextPoint {
                line: target_line,
                column: target_column,
            },
            is_extending,
        );
        self.preferred_column = Some(column_chars);
    }

    /// Home은 Monaco처럼 들여쓰기 끝과 줄 처음을 오간다
    pub fn move_to_line_start(&mut self, is_extending: bool) {
        let line = &self.lines[self.cursor.line];
        let indentation_end = line.len() - line.trim_start().len();
        let column = if self.cursor.column == indentation_end {
            0
        } else {
            indentation_end
        };
        self.set_cursor(
            TextPoint {
                line: self.cursor.line,
                column,
            },
            is_extending,
        );
    }

    pub fn move_to_line_end(&mut self, is_extending: bool) {
        let column = self.lines[self.cursor.line].len();
        self.set_cursor(
            TextPoint {
                line: self.cursor.line,
                column,
            },
            is_extending,
        );
    }

    pub fn undo(&mut self) -> bool {
        self.swap_history(true)
    }

    pub fn redo(&mut self) -> bool {
        self.swap_history(false)
    }

    /// 문서 전체를 UTF-16 단위로 센 위치(입력기 연동)
    pub fn utf16_offset(&self, point: TextPoint) -> usize {
        let before: usize = self.lines[..point.line]
            .iter()
            .map(|line| line.encode_utf16().count() + 1)
            .sum();
        before
            + self.lines[point.line][..point.column]
                .encode_utf16()
                .count()
    }

    pub fn point_for_utf16_offset(&self, offset: usize) -> TextPoint {
        let mut remaining = offset;
        for (line_index, line) in self.lines.iter().enumerate() {
            let line_length = line.encode_utf16().count();
            if remaining <= line_length {
                return TextPoint {
                    line: line_index,
                    column: byte_index_for_utf16(line, remaining),
                };
            }
            remaining -= line_length + 1;
        }
        let last_line = self.lines.len() - 1;
        TextPoint {
            line: last_line,
            column: self.lines[last_line].len(),
        }
    }

    /// 입력기가 넘긴 UTF-16 범위를 선택으로 바꾼다
    pub fn select_utf16_range(&mut self, range: Range<usize>) {
        self.anchor = self.point_for_utf16_offset(range.start);
        self.cursor = self.point_for_utf16_offset(range.end);
    }

    fn record_undo(&mut self, kind: EditKind) {
        // 지우기는 한 글자 선택을 만든 뒤 부르므로, 선택 여부는 글자 입력에서만 본다
        let is_continuing_group = match kind {
            EditKind::Typing => {
                self.last_edit == Some(EditKind::Typing) && self.selection().is_none()
            }
            EditKind::Deleting => self.last_edit == Some(EditKind::Deleting),
            EditKind::Other => false,
        };
        if !is_continuing_group {
            self.undo_stack.push(self.snapshot());
            trim_oldest_snapshots(
                &mut self.undo_stack,
                MAX_UNDO_SNAPSHOTS,
                MAX_UNDO_HISTORY_BYTES,
            );
        }
        self.redo_stack.clear();
        self.last_edit = Some(kind);
    }

    fn finish_edit(&mut self) {
        self.anchor = self.cursor;
        self.preferred_column = None;
        self.revision += 1;
    }

    fn swap_history(&mut self, is_undo: bool) -> bool {
        let (from, to) = if is_undo {
            (&mut self.undo_stack, &mut self.redo_stack)
        } else {
            (&mut self.redo_stack, &mut self.undo_stack)
        };
        let Some(snapshot) = from.pop() else {
            return false;
        };
        let current = Snapshot {
            byte_size: lines_byte_size(&self.lines),
            lines: std::mem::replace(&mut self.lines, snapshot.lines),
            cursor: self.cursor,
            anchor: self.anchor,
        };
        to.push(current);
        (self.cursor, self.anchor) = (snapshot.cursor, snapshot.anchor);
        self.last_edit = None;
        self.revision += 1;
        true
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            lines: self.lines.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
            byte_size: lines_byte_size(&self.lines),
        }
    }

    /// 선택을 지우고 선택 앞 위치를 돌려준다
    fn delete_selection(&mut self) -> TextPoint {
        let Some((start, end)) = self.selection() else {
            return self.cursor;
        };
        let tail = self.lines[end.line][end.column..].to_string();
        self.lines[start.line].truncate(start.column);
        self.lines[start.line].push_str(&tail);
        self.lines.drain(start.line + 1..=end.line);
        self.cursor = start;
        self.anchor = start;
        start
    }

    fn step_left(&self, point: TextPoint) -> TextPoint {
        if point.column > 0 {
            let line = &self.lines[point.line];
            let previous = line[..point.column]
                .char_indices()
                .next_back()
                .map_or(0, |(index, _)| index);
            return TextPoint {
                line: point.line,
                column: previous,
            };
        }
        if point.line == 0 {
            return point;
        }
        TextPoint {
            line: point.line - 1,
            column: self.lines[point.line - 1].len(),
        }
    }

    fn step_right(&self, point: TextPoint) -> TextPoint {
        let line = &self.lines[point.line];
        if let Some(character) = line[point.column..].chars().next() {
            return TextPoint {
                line: point.line,
                column: point.column + character.len_utf8(),
            };
        }
        if point.line + 1 >= self.lines.len() {
            return point;
        }
        TextPoint {
            line: point.line + 1,
            column: 0,
        }
    }

    fn clamp(&self, point: TextPoint) -> TextPoint {
        let line = point.line.min(self.lines.len() - 1);
        let text = &self.lines[line];
        let mut column = point.column.min(text.len());
        while !text.is_char_boundary(column) {
            column -= 1;
        }
        TextPoint { line, column }
    }
}

fn ordered(first: TextPoint, second: TextPoint) -> (TextPoint, TextPoint) {
    if first <= second {
        (first, second)
    } else {
        (second, first)
    }
}

fn byte_index_for_char(line: &str, char_count: usize) -> usize {
    line.char_indices()
        .nth(char_count)
        .map_or(line.len(), |(index, _)| index)
}

fn byte_index_for_utf16(line: &str, utf16_count: usize) -> usize {
    let mut counted = 0;
    for (index, character) in line.char_indices() {
        if counted >= utf16_count {
            return index;
        }
        counted += character.len_utf16();
    }
    line.len()
}

/// 들여쓴 줄 가운데 가장 얕은 공백 수를 들여쓰기 단위로 본다(Monaco `detectIndentation`의 근사)
/// 앞 공백을 다음 들여쓰기 자리까지 늘릴 바이트 수
fn indent_change(text: &str, indent_width: usize) -> isize {
    let leading_spaces = text.len() - text.trim_start_matches(' ').len();
    (indent_width - leading_spaces % indent_width) as isize
}

/// 앞 공백을 이전 들여쓰기 자리까지 줄일 바이트 수(음수). 탭으로 시작하면 탭 하나를 뺀다
fn outdent_change(text: &str, indent_width: usize) -> isize {
    if text.starts_with('\t') {
        return -1;
    }
    let leading_spaces = text.len() - text.trim_start_matches(' ').len();
    match leading_spaces {
        0 => 0,
        spaces => -((((spaces - 1) % indent_width) + 1) as isize),
    }
}

/// 줄 앞 공백이 바뀐 만큼 위치를 옮긴다. 들일 때 줄 맨 앞 위치는 그대로 두어 줄 전체 선택이 새 들여쓰기까지 덮는다
fn shifted_point(point: TextPoint, leading_changes: &[(usize, isize)]) -> TextPoint {
    let Some((_, change)) = leading_changes.iter().find(|(line, _)| *line == point.line) else {
        return point;
    };
    let column = match *change > 0 {
        true if point.column == 0 => 0,
        true => point.column + change.unsigned_abs(),
        false => point.column.saturating_sub(change.unsigned_abs()),
    };
    TextPoint {
        line: point.line,
        column,
    }
}

fn lines_byte_size(lines: &[String]) -> usize {
    lines.iter().map(String::len).sum()
}

/// 기록이 개수나 합친 크기 상한을 넘으면 가장 오래된 것부터 버린다. 방금 넣은 최신 기록은 남긴다
fn trim_oldest_snapshots(stack: &mut Vec<Snapshot>, max_count: usize, max_bytes: usize) {
    let mut total_bytes: usize = stack.iter().map(|snapshot| snapshot.byte_size).sum();
    let mut dropped_count = 0;
    while stack.len() - dropped_count > 1
        && (stack.len() - dropped_count > max_count || total_bytes > max_bytes)
    {
        total_bytes -= stack[dropped_count].byte_size;
        dropped_count += 1;
    }
    stack.drain(..dropped_count);
}

fn detect_indent_width(lines: &[String]) -> usize {
    lines
        .iter()
        .map(|line| line.len() - line.trim_start_matches(' ').len())
        .filter(|width| *width > 0)
        .min()
        .unwrap_or(DEFAULT_INDENT_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(line: usize, column: usize) -> TextPoint {
        TextPoint { line, column }
    }

    #[test]
    fn typing_at_the_line_end_appends_text_and_bumps_the_revision() {
        let mut buffer = TextBuffer::new("export function a() {\n}\n");
        buffer.set_cursor(point(0, 0), false);
        buffer.move_to_line_end(false);

        for character in " // TODO".chars() {
            buffer.insert_text(&character.to_string());
        }

        assert_eq!(buffer.lines()[0], "export function a() { // TODO");
        assert_eq!(buffer.revision(), 8);
        assert_eq!(buffer.text(), "export function a() { // TODO\n}\n");
    }

    #[test]
    fn undo_reverts_a_typing_run_at_once_and_redo_restores_it() {
        let mut buffer = TextBuffer::new("ab");
        buffer.set_cursor(point(0, 2), false);
        buffer.insert_text("c");
        buffer.insert_text("d");
        buffer.insert_newline(Language::TypeScript);

        assert!(buffer.undo());
        assert_eq!(buffer.text(), "abcd");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "ab");
        assert!(buffer.redo());
        assert_eq!(buffer.text(), "abcd");
    }

    #[test]
    fn selection_replacement_and_deletion_span_lines() {
        let mut buffer = TextBuffer::new("one\ntwo\nthree");
        buffer.set_cursor(point(0, 1), false);
        buffer.set_cursor(point(2, 2), true);

        buffer.insert_text("X");

        assert_eq!(buffer.text(), "oXree");
        buffer.delete_backward();
        buffer.delete_backward();
        assert_eq!(buffer.text(), "ree");
    }

    #[test]
    fn enter_keeps_the_current_indentation_and_tab_uses_the_detected_width() {
        let mut buffer = TextBuffer::new("{\n  a;\n}");
        buffer.set_cursor(point(1, 4), false);

        buffer.insert_newline(Language::TypeScript);
        buffer.insert_indent();

        assert_eq!(buffer.lines()[2], "    ");
    }

    #[test]
    fn tab_on_a_multi_line_selection_indents_each_line_and_keeps_the_selection() {
        let mut buffer = TextBuffer::new("a\nb\n\nc\nd");
        buffer.set_cursor(point(0, 0), false);
        buffer.set_cursor(point(3, 1), true);

        buffer.indent();

        assert_eq!(buffer.text(), "    a\n    b\n\n    c\nd");
        assert_eq!(
            buffer.selected_text().as_deref(),
            Some("    a\n    b\n\n    c")
        );
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "a\nb\n\nc\nd");
    }

    #[test]
    fn shift_tab_outdents_the_selected_lines_or_the_cursor_line() {
        let mut buffer = TextBuffer::new("    a\n      b\nc");
        buffer.set_cursor(point(0, 0), false);
        buffer.set_cursor(point(2, 1), true);

        buffer.outdent();
        assert_eq!(buffer.text(), "a\n    b\nc");

        buffer.set_cursor(point(1, 5), false);
        buffer.outdent();
        assert_eq!(buffer.text(), "a\nb\nc");
        assert_eq!(buffer.cursor(), point(1, 1));
    }

    #[test]
    fn enter_between_braces_opens_an_indented_line() {
        let mut buffer = TextBuffer::new("  f() {}\n");
        buffer.set_cursor(point(0, 7), false);

        buffer.insert_newline(Language::TypeScript);

        assert_eq!(buffer.text(), "  f() {\n    \n  }\n");
        assert_eq!(buffer.cursor(), point(1, 4));
    }

    #[test]
    fn word_moves_and_deletes_cross_line_edges() {
        let mut buffer = TextBuffer::new("let a = b;\nnext");
        buffer.set_cursor(point(0, 10), false);

        buffer.delete_word(false);
        assert_eq!(buffer.lines()[0], "let a = b");
        buffer.move_word(true, false);
        assert_eq!(buffer.cursor(), point(1, 0));
        buffer.move_word(false, true);
        assert_eq!(buffer.selection(), Some((point(0, 9), point(1, 0))));
    }

    #[test]
    fn pairs_surround_and_replacements_are_single_undo_steps() {
        let mut buffer = TextBuffer::new("ab ab");
        buffer.set_cursor(point(0, 0), false);
        buffer.set_cursor(point(0, 2), true);
        buffer.surround_selection('(', ')');
        assert_eq!(buffer.text(), "(ab) ab");
        assert_eq!(buffer.selected_text().as_deref(), Some("ab"));

        buffer.replace_ranges(&[
            (point(0, 1), point(0, 3), "xyz".to_string()),
            (point(0, 5), point(0, 7), "1\n2".to_string()),
        ]);
        assert_eq!(buffer.text(), "(xyz) 1\n2");
        buffer.undo();
        assert_eq!(buffer.text(), "(ab) ab");
    }

    #[test]
    fn multi_click_selects_a_word_or_a_whole_line() {
        let mut buffer = TextBuffer::new("count(cards)\nnext");

        buffer.select_word_at(point(0, 2));
        assert_eq!(buffer.selected_text().as_deref(), Some("count"));
        buffer.select_line(0);
        assert_eq!(buffer.selected_text().as_deref(), Some("count(cards)\n"));
    }

    #[test]
    fn crlf_files_keep_their_line_endings_when_saved() {
        let mut buffer = TextBuffer::new("a\r\nb");
        buffer.set_cursor(point(0, 0), false);
        buffer.set_cursor(point(1, 1), true);

        assert_eq!(buffer.lines()[0], "a");
        assert_eq!(buffer.selected_text().as_deref(), Some("a\r\nb"));
        assert_eq!(buffer.text(), "a\r\nb");
    }

    #[test]
    fn held_backspace_is_one_undo_step_that_restores_the_document() {
        let mut buffer = TextBuffer::new("hello\nworld");
        buffer.set_cursor(point(1, 5), false);

        for _ in 0..8 {
            buffer.delete_backward();
        }

        assert_eq!(buffer.text(), "hel");
        assert_eq!(buffer.undo_stack.len(), 1);
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "hello\nworld");
    }

    #[test]
    fn delete_after_moving_the_cursor_starts_a_new_undo_step() {
        let mut buffer = TextBuffer::new("abcd");
        buffer.set_cursor(point(0, 4), false);
        buffer.delete_backward();
        buffer.set_cursor(point(0, 0), false);
        buffer.delete_forward();

        assert_eq!(buffer.text(), "bc");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "abc");
    }

    #[test]
    fn undo_history_keeps_at_most_the_snapshot_limit_dropping_the_oldest() {
        let mut buffer = TextBuffer::new("");

        for _ in 0..MAX_UNDO_SNAPSHOTS + 5 {
            buffer.insert_text("\n");
        }

        assert_eq!(buffer.undo_stack.len(), MAX_UNDO_SNAPSHOTS);
        assert_eq!(buffer.undo_stack[0].lines.len(), 6);
    }

    #[test]
    fn undo_history_over_the_byte_budget_drops_the_oldest_but_keeps_the_newest() {
        let snapshot_of = |text: &str| Snapshot {
            lines: vec![text.to_string()],
            cursor: point(0, 0),
            anchor: point(0, 0),
            byte_size: text.len(),
        };
        let mut stack = vec![snapshot_of("aaaa"), snapshot_of("bbbb"), snapshot_of("cc")];

        trim_oldest_snapshots(&mut stack, 10, 6);
        assert_eq!(stack, vec![snapshot_of("bbbb"), snapshot_of("cc")]);

        trim_oldest_snapshots(&mut stack, 10, 1);
        assert_eq!(stack, vec![snapshot_of("cc")]);
    }

    #[test]
    fn utf16_range_text_counts_a_crlf_line_break_as_one_unit() {
        let buffer = TextBuffer::new("가a\r\n나b\r\nc");
        let second_line = buffer.utf16_offset(point(1, 0))..buffer.utf16_offset(point(1, 4));

        let (text, adjusted_range) = buffer.text_for_utf16_range(second_line.clone());

        assert_eq!(text, "나b");
        assert_eq!(adjusted_range, second_line);
        let (tail, tail_range) = buffer.text_for_utf16_range(5..100);
        assert_eq!(tail, "\nc");
        assert_eq!(tail_range, 5..7);
    }

    #[test]
    fn utf16_offsets_round_trip_through_korean_text() {
        let buffer = TextBuffer::new("가나\nab");

        assert_eq!(buffer.utf16_offset(point(1, 1)), 4);
        assert_eq!(buffer.point_for_utf16_offset(1), point(0, 3));
    }

    #[test]
    fn enter_inside_a_doc_comment_continues_it_like_monaco_typescript() {
        let mut buffer = TextBuffer::new("  /** */");
        buffer.set_cursor(point(0, 5), false);

        buffer.insert_newline(Language::TypeScript);
        assert_eq!(buffer.text(), "  /**\n   * \n   */");
        assert_eq!(buffer.cursor(), point(1, 5));

        buffer.insert_text("Sum");
        buffer.insert_newline(Language::TypeScript);
        assert_eq!(buffer.lines()[2], "   * ");

        let mut closing = TextBuffer::new("   */");
        closing.set_cursor(point(0, 5), false);
        closing.insert_newline(Language::TypeScript);
        assert_eq!(closing.lines()[1], "  ");

        let mut plain = TextBuffer::new("/**");
        plain.set_cursor(point(0, 3), false);
        plain.insert_newline(Language::Rust);
        assert_eq!(plain.lines()[1], "");
    }
}
