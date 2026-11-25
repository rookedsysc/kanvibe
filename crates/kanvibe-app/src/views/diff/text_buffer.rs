//! 편집 모드 편집기의 글 내용과 커서·선택, 실행 취소. TS 앱은 Monaco `Editor`가 이 일을 했다.
//! 위치는 줄 번호와 줄 안 바이트 위치(`TextPoint`)이고, 언제나 글자 경계에 놓인다.

use std::ops::Range;

use super::line_diff::TextPoint;

const DEFAULT_INDENT_WIDTH: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    lines: Vec<String>,
    cursor: TextPoint,
    anchor: TextPoint,
}

/// 마지막 편집이 이어 붙일 수 있는 글자 입력이었는지. 연속 입력은 실행 취소 한 번에 함께 되돌린다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Typing,
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
        if start.line == end.line {
            return Some(self.lines[start.line][start.column..end.column].to_string());
        }
        let mut parts = vec![self.lines[start.line][start.column..].to_string()];
        parts.extend(self.lines[start.line + 1..end.line].iter().cloned());
        parts.push(self.lines[end.line][..end.column].to_string());
        Some(parts.join(self.line_ending))
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
        let start = self.delete_selection();
        let inserted: Vec<&str> = text.split('\n').collect();
        let line = &self.lines[start.line];
        let (head, tail) = (
            line[..start.column].to_string(),
            line[start.column..].to_string(),
        );
        let mut new_lines: Vec<String> = inserted.iter().map(|part| part.to_string()).collect();
        new_lines[0] = head + &new_lines[0];
        let last_index = new_lines.len() - 1;
        let cursor_column = new_lines[last_index].len();
        new_lines[last_index].push_str(&tail);
        self.lines.splice(start.line..=start.line, new_lines);
        self.cursor = TextPoint {
            line: start.line + last_index,
            column: cursor_column,
        };
        self.finish_edit();
    }

    /// Enter. 지금 줄의 들여쓰기를 이어 간다(Monaco `autoIndent`의 기본 동작)
    pub fn insert_newline(&mut self) {
        let line = &self.lines[self.cursor.line];
        let indentation: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        self.insert_text(&format!("\n{indentation}"));
    }

    pub fn insert_indent(&mut self) {
        let unit = self.indent_unit.clone();
        self.insert_text(&unit);
    }

    pub fn delete_backward(&mut self) {
        if self.selection().is_none() {
            let previous = self.step_left(self.cursor);
            if previous == self.cursor {
                return;
            }
            self.anchor = previous;
        }
        self.record_undo(EditKind::Other);
        self.delete_selection();
        self.finish_edit();
    }

    pub fn delete_forward(&mut self) {
        if self.selection().is_none() {
            let next = self.step_right(self.cursor);
            if next == self.cursor {
                return;
            }
            self.anchor = next;
        }
        self.record_undo(EditKind::Other);
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

    pub fn move_vertically(&mut self, line_delta: isize, is_extending: bool) {
        let column_chars = self.preferred_column.unwrap_or_else(|| {
            self.lines[self.cursor.line][..self.cursor.column]
                .chars()
                .count()
        });
        let last_line = self.lines.len() as isize - 1;
        let target_line = (self.cursor.line as isize + line_delta).clamp(0, last_line) as usize;
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
        let is_continuing_typing = kind == EditKind::Typing
            && self.last_edit == Some(EditKind::Typing)
            && self.selection().is_none();
        if !is_continuing_typing {
            self.undo_stack.push(self.snapshot());
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
        to.push(Snapshot {
            lines: self.lines.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        });
        (self.lines, self.cursor, self.anchor) = (snapshot.lines, snapshot.cursor, snapshot.anchor);
        self.last_edit = None;
        self.revision += 1;
        true
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            lines: self.lines.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
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
        buffer.insert_newline();

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

        buffer.insert_newline();
        buffer.insert_indent();

        assert_eq!(buffer.lines()[2], "    ");
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
    fn utf16_offsets_round_trip_through_korean_text() {
        let buffer = TextBuffer::new("가나\nab");

        assert_eq!(buffer.utf16_offset(point(1, 1)), 4);
        assert_eq!(buffer.point_for_utf16_offset(1), point(0, 3));
    }
}
