//! 편집기 키보드. Monaco 기본 키 배치(플랫폼별)를 공용 단축키 판정기로 맞추고, 글자 입력은 입력기에서 자동 닫기를 거친다.

use gpui_kit::{ClipboardItem, Context, KeyDownEvent, Window};
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::shortcuts::key_event::match_shortcut_event;

use super::DiffFileEditor;
use crate::shortcuts::shortcut_event_from_keystroke;
use crate::views::diff::auto_closing::{
    TokenContext, TypingAction, TypingContext, decide_typing, is_deleting_auto_closed_pair,
};
use crate::views::diff::line_diff::TextPoint;
use crate::views::diff::monaco::LINE_HEIGHT;
use crate::views::diff::syntax::TokenKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorCommand {
    Save,
    Undo,
    Redo,
    SelectAll,
    Copy,
    Cut,
    Paste,
    Find,
    Replace,
    FindNext,
    FindPrevious,
    Escape,
    Word {
        is_forward: bool,
        is_extending: bool,
    },
    DeleteWord {
        is_forward: bool,
    },
    LineEdge {
        is_end: bool,
        is_extending: bool,
    },
    DocumentEdge {
        is_end: bool,
        is_extending: bool,
    },
    Fold,
    Unfold,
}

use EditorCommand::*;

/// 두 플랫폼이 같은 Monaco 기본 키
const COMMON_COMMANDS: [(&str, EditorCommand); 14] = [
    ("Mod+S", Save),
    ("Mod+Z", Undo),
    ("Mod+Shift+Z", Redo),
    ("Mod+A", SelectAll),
    ("Mod+C", Copy),
    ("Mod+X", Cut),
    ("Mod+V", Paste),
    ("Mod+F", Find),
    ("F3", FindNext),
    ("Shift+F3", FindPrevious),
    ("Escape", Escape),
    (
        "Home",
        LineEdge {
            is_end: false,
            is_extending: false,
        },
    ),
    (
        "End",
        LineEdge {
            is_end: true,
            is_extending: false,
        },
    ),
    (
        "Shift+Home",
        LineEdge {
            is_end: false,
            is_extending: true,
        },
    ),
];

/// macOS: 단어 이동은 Option, 줄 끝·문서 끝은 Command, 접기는 Command+Option+[
const MAC_COMMANDS: [(&str, EditorCommand); 20] = [
    (
        "Shift+End",
        LineEdge {
            is_end: true,
            is_extending: true,
        },
    ),
    ("Meta+Alt+F", Replace),
    ("Meta+G", FindNext),
    ("Meta+Shift+G", FindPrevious),
    (
        "Alt+ArrowLeft",
        Word {
            is_forward: false,
            is_extending: false,
        },
    ),
    (
        "Alt+ArrowRight",
        Word {
            is_forward: true,
            is_extending: false,
        },
    ),
    (
        "Alt+Shift+ArrowLeft",
        Word {
            is_forward: false,
            is_extending: true,
        },
    ),
    (
        "Alt+Shift+ArrowRight",
        Word {
            is_forward: true,
            is_extending: true,
        },
    ),
    ("Alt+Backspace", DeleteWord { is_forward: false }),
    ("Alt+Delete", DeleteWord { is_forward: true }),
    (
        "Meta+ArrowLeft",
        LineEdge {
            is_end: false,
            is_extending: false,
        },
    ),
    (
        "Meta+ArrowRight",
        LineEdge {
            is_end: true,
            is_extending: false,
        },
    ),
    (
        "Meta+Shift+ArrowLeft",
        LineEdge {
            is_end: false,
            is_extending: true,
        },
    ),
    (
        "Meta+Shift+ArrowRight",
        LineEdge {
            is_end: true,
            is_extending: true,
        },
    ),
    (
        "Meta+ArrowUp",
        DocumentEdge {
            is_end: false,
            is_extending: false,
        },
    ),
    (
        "Meta+ArrowDown",
        DocumentEdge {
            is_end: true,
            is_extending: false,
        },
    ),
    (
        "Meta+Shift+ArrowUp",
        DocumentEdge {
            is_end: false,
            is_extending: true,
        },
    ),
    (
        "Meta+Shift+ArrowDown",
        DocumentEdge {
            is_end: true,
            is_extending: true,
        },
    ),
    ("Meta+Alt+[", Fold),
    ("Meta+Alt+]", Unfold),
];

/// Linux(Windows도 같다): 단어 이동·삭제는 Ctrl, 바꾸기는 Ctrl+H, 접기는 Ctrl+Shift+[
const OTHER_COMMANDS: [(&str, EditorCommand); 15] = [
    (
        "Shift+End",
        LineEdge {
            is_end: true,
            is_extending: true,
        },
    ),
    ("Ctrl+Y", Redo),
    ("Ctrl+H", Replace),
    (
        "Ctrl+ArrowLeft",
        Word {
            is_forward: false,
            is_extending: false,
        },
    ),
    (
        "Ctrl+ArrowRight",
        Word {
            is_forward: true,
            is_extending: false,
        },
    ),
    (
        "Ctrl+Shift+ArrowLeft",
        Word {
            is_forward: false,
            is_extending: true,
        },
    ),
    (
        "Ctrl+Shift+ArrowRight",
        Word {
            is_forward: true,
            is_extending: true,
        },
    ),
    ("Ctrl+Backspace", DeleteWord { is_forward: false }),
    ("Ctrl+Delete", DeleteWord { is_forward: true }),
    (
        "Ctrl+Home",
        DocumentEdge {
            is_end: false,
            is_extending: false,
        },
    ),
    (
        "Ctrl+End",
        DocumentEdge {
            is_end: true,
            is_extending: false,
        },
    ),
    (
        "Ctrl+Shift+Home",
        DocumentEdge {
            is_end: false,
            is_extending: true,
        },
    ),
    (
        "Ctrl+Shift+End",
        DocumentEdge {
            is_end: true,
            is_extending: true,
        },
    ),
    ("Ctrl+Shift+[", Fold),
    ("Ctrl+Shift+]", Unfold),
];

fn editor_commands(
    platform: ShortcutPlatform,
) -> impl Iterator<Item = (&'static str, EditorCommand)> {
    let platform_commands: &[(&str, EditorCommand)] = if platform == ShortcutPlatform::Mac {
        &MAC_COMMANDS
    } else {
        &OTHER_COMMANDS
    };
    COMMON_COMMANDS.iter().chain(platform_commands).copied()
}

impl DiffFileEditor {
    pub(super) fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shortcut = shortcut_event_from_keystroke(&event.keystroke);
        let command = editor_commands(self.platform)
            .find(|(notation, _)| match_shortcut_event(&shortcut, notation, self.platform))
            .map(|(_, command)| command);
        let is_handled = match command {
            Some(command) => self.run_command(command, window, cx),
            None => self.handle_plain_key(event, cx),
        };
        if is_handled {
            cx.stop_propagation();
        }
    }

    fn run_command(
        &mut self,
        command: EditorCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match command {
            Save => self.save(cx),
            Undo | Redo => self.apply_history(command == Undo, cx),
            SelectAll => self.buffer.select_all(),
            Copy | Cut => self.copy_selection(command == Cut, cx),
            Paste => self.paste(cx),
            Find | Replace => self.open_find(command == Replace, window, cx),
            FindNext | FindPrevious => return self.step_find(command == FindNext, cx),
            Escape => return self.escape(window, cx),
            _ => return self.run_motion_command(command, cx),
        }
        cx.notify();
        true
    }

    fn run_motion_command(&mut self, command: EditorCommand, cx: &mut Context<Self>) -> bool {
        match command {
            Word {
                is_forward,
                is_extending,
            } => self.buffer.move_word(is_forward, is_extending),
            DeleteWord { is_forward } => {
                self.buffer.delete_word(is_forward);
                self.after_edit(cx);
                return true;
            }
            LineEdge {
                is_end: true,
                is_extending,
            } => self.buffer.move_to_line_end(is_extending),
            LineEdge {
                is_end: false,
                is_extending,
            } => self.buffer.move_to_line_start(is_extending),
            DocumentEdge {
                is_end,
                is_extending,
            } => {
                let last = self.buffer.lines().len() - 1;
                let target = if is_end {
                    TextPoint {
                        line: last,
                        column: self.buffer.lines()[last].len(),
                    }
                } else {
                    TextPoint { line: 0, column: 0 }
                };
                self.buffer.set_cursor(target, is_extending);
            }
            Fold | Unfold => self.fold_at_cursor(command == Fold),
            _ => return false,
        }
        self.after_cursor_move(cx);
        true
    }

    /// Escape: 찾기 위젯을 닫고, 없으면 선택을 푼다
    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.find.is_some() {
            self.close_find(window, cx);
            return true;
        }
        if self.buffer.selection().is_none() {
            return false;
        }
        let cursor = self.buffer.cursor();
        self.buffer.set_cursor(cursor, false);
        cx.notify();
        true
    }

    fn apply_history(&mut self, is_undo: bool, cx: &mut Context<Self>) {
        let has_changed = if is_undo {
            self.buffer.undo()
        } else {
            self.buffer.redo()
        };
        if has_changed {
            self.auto_closed.clear();
            self.after_edit(cx);
        }
    }

    fn copy_selection(&mut self, is_cut: bool, cx: &mut Context<Self>) {
        let Some(text) = self.buffer.selected_text() else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        if is_cut {
            self.buffer.delete_backward();
            self.after_edit(cx);
        }
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.buffer.insert_text(&text.replace("\r\n", "\n"));
            self.auto_closed.clear();
            self.after_edit(cx);
        }
    }

    fn handle_plain_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.platform || modifiers.alt {
            return false;
        }
        let is_extending = modifiers.shift;
        let page_rows = self
            .surface_bounds
            .get()
            .map_or(20.0, |bounds| f32::from(bounds.size.height) / LINE_HEIGHT)
            as isize;
        match event.keystroke.key.as_str() {
            "left" => self.buffer.move_horizontally(false, is_extending),
            "right" => self.buffer.move_horizontally(true, is_extending),
            "up" => self.move_rows(-1, is_extending),
            "down" => self.move_rows(1, is_extending),
            "pageup" => self.move_rows(-page_rows, is_extending),
            "pagedown" => self.move_rows(page_rows, is_extending),
            "home" => self.buffer.move_to_line_start(is_extending),
            "end" => self.buffer.move_to_line_end(is_extending),
            "backspace" | "delete" | "enter" | "tab" => {
                return self.handle_edit_key(event.keystroke.key.as_str(), cx);
            }
            _ => return false,
        }
        self.after_cursor_move(cx);
        true
    }

    /// 위아래 이동은 접힌 줄을 건너 화면에 보이는 줄 단위로 움직인다
    fn move_rows(&mut self, delta: isize, is_extending: bool) {
        let cursor_line = self.buffer.cursor().line;
        let current_row = self
            .visible_lines
            .iter()
            .rposition(|line| *line <= cursor_line)
            .unwrap_or(0);
        let target_row =
            (current_row as isize + delta).clamp(0, self.visible_lines.len() as isize - 1) as usize;
        self.buffer
            .move_to_line(self.visible_lines[target_row], is_extending);
    }

    fn handle_edit_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        match key {
            "backspace" if self.is_cursor_inside_auto_closed_pair() => {
                self.buffer.delete_surrounding_pair()
            }
            "backspace" => self.buffer.delete_backward(),
            "delete" => self.buffer.delete_forward(),
            "enter" => self.buffer.insert_newline(),
            _ => self.buffer.insert_indent(),
        }
        self.auto_closed.clear();
        self.after_edit(cx);
        true
    }

    fn is_cursor_inside_auto_closed_pair(&self) -> bool {
        let cursor = self.buffer.cursor();
        self.buffer.selection().is_none()
            && is_deleting_auto_closed_pair(
                self.language,
                &self.buffer.lines()[cursor.line],
                cursor.column,
                self.auto_closed.contains(&cursor),
            )
    }

    /// 글자 하나를 친다. 괄호·따옴표는 자동 닫기·지나가기·감싸기를 따른다
    pub(super) fn type_character(&mut self, typed: char, cx: &mut Context<Self>) {
        let cursor = self.buffer.cursor();
        let line = self.buffer.lines()[cursor.line].clone();
        let context = TypingContext {
            line: &line,
            column: cursor.column,
            has_selection: self.buffer.selection().is_some(),
            tokens: self.token_context(cursor),
            is_next_auto_closed: self.auto_closed.contains(&cursor),
        };
        match decide_typing(self.language, typed, &context) {
            TypingAction::Overtype => {
                self.auto_closed.retain(|closer| *closer != cursor);
                self.buffer.move_horizontally(true, false);
                self.after_cursor_move(cx);
                return;
            }
            TypingAction::Surround { close } => self.buffer.surround_selection(typed, close),
            TypingAction::InsertPair { close } => {
                self.buffer.insert_pair(typed, close);
                self.shift_auto_closed(cursor, typed.len_utf8());
                self.auto_closed.push(self.buffer.cursor());
            }
            TypingAction::Insert => {
                self.buffer.insert_text(&typed.to_string());
                self.shift_auto_closed(cursor, typed.len_utf8());
            }
        }
        self.after_edit(cx);
    }

    /// 같은 줄에서 입력 자리 뒤에 있던 자동 닫는 글자들을 밀린 만큼 옮긴다
    fn shift_auto_closed(&mut self, inserted_at: TextPoint, length: usize) {
        for closer in &mut self.auto_closed {
            if closer.line == inserted_at.line && closer.column >= inserted_at.column {
                closer.column += length;
            }
        }
    }

    /// 커서 바로 앞 글자의 토큰이 문자열·주석인지(Monaco `notIn`)
    fn token_context(&self, cursor: TextPoint) -> TokenContext {
        let Some(tokens) = self.document.tokens.get(cursor.line) else {
            return TokenContext::default();
        };
        let kind = tokens
            .iter()
            .find(|token| token.range.start < cursor.column && cursor.column <= token.range.end)
            .map(|token| token.kind);
        let is_closed_string_end = kind == Some(TokenKind::String)
            && self.buffer.lines()[cursor.line][..cursor.column].ends_with(['"', '\'', '`']);
        TokenContext {
            is_in_string: kind == Some(TokenKind::String) && !is_closed_string_end,
            is_in_comment: kind == Some(TokenKind::Comment),
        }
    }
}
