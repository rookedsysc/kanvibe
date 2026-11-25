//! 보드 화면의 한 줄 입력(검색어·찾기·작업 제목). 커서는 항상 끝에 있고, 입력기(IME) 조합 글자를 받는다.
//! 겉모양(테두리·여백)은 부르는 쪽이 감싸서 정하고, 여기서는 글자·placeholder·커서만 그린다.

use std::ops::Range;

use gpui_kit::{
    App, Bounds, Context, ElementInputHandler, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Pixels, Render, Rgba,
    SharedString, Styled, UTF16Selection, Window, canvas, div, px,
};

/// 입력이 바뀌었거나 Enter·Esc를 눌렀을 때 부모에게 알린다
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextFieldEvent {
    Changed(String),
    Submitted {
        is_shift_pressed: bool,
    },
    Escaped,
    /// ↑·↓·Tab처럼 입력이 쓰지 않는 키(목록 이동에 쓴다)
    NavigationKey(String),
}

pub struct BoardTextField {
    text: String,
    marked_range: Option<Range<usize>>,
    placeholder: SharedString,
    focus_handle: FocusHandle,
    pub text_color: Rgba,
    pub placeholder_color: Rgba,
    pub caret_color: Rgba,
}

impl EventEmitter<TextFieldEvent> for BoardTextField {}

impl Focusable for BoardTextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn utf16_to_byte(text: &str, utf16_offset: usize) -> usize {
    let mut utf16_count = 0;
    for (byte_index, character) in text.char_indices() {
        if utf16_count >= utf16_offset {
            return byte_index;
        }
        utf16_count += character.len_utf16();
    }
    text.len()
}

fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

impl BoardTextField {
    pub fn new(
        placeholder: impl Into<SharedString>,
        colors: [Rgba; 3],
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            text: String::new(),
            marked_range: None,
            placeholder: placeholder.into(),
            focus_handle: cx.focus_handle(),
            text_color: colors[0],
            placeholder_color: colors[1],
            caret_color: colors[2],
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.text = text.into();
        self.marked_range = None;
        cx.notify();
    }

    fn utf16_range_to_bytes(&self, range: &Range<usize>) -> Range<usize> {
        utf16_to_byte(&self.text, range.start)..utf16_to_byte(&self.text, range.end)
    }

    fn replace(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        cx: &mut Context<Self>,
    ) -> usize {
        let range = range
            .or_else(|| self.marked_range.clone())
            .map(|range| self.utf16_range_to_bytes(&range))
            .unwrap_or(self.text.len()..self.text.len());
        let start = range.start.min(self.text.len());
        let end = range.end.clamp(start, self.text.len());
        self.text.replace_range(start..end, new_text);
        cx.emit(TextFieldEvent::Changed(self.text.clone()));
        cx.notify();
        utf16_len(&self.text[..start])
    }

    fn handle_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let has_command_modifier = keystroke.modifiers.control || keystroke.modifiers.platform;
        match keystroke.key.as_str() {
            "backspace" if self.marked_range.is_none() => {
                if self.text.pop().is_some() {
                    cx.emit(TextFieldEvent::Changed(self.text.clone()));
                    cx.notify();
                }
                cx.stop_propagation();
            }
            "enter" => {
                cx.emit(TextFieldEvent::Submitted {
                    is_shift_pressed: keystroke.modifiers.shift,
                });
                cx.stop_propagation();
            }
            "escape" => {
                cx.emit(TextFieldEvent::Escaped);
                cx.stop_propagation();
            }
            "up" | "down" | "tab" => {
                cx.emit(TextFieldEvent::NavigationKey(keystroke.key.clone()));
                cx.stop_propagation();
            }
            "a" if has_command_modifier => cx.stop_propagation(),
            _ => {}
        }
    }
}

impl EntityInputHandler for BoardTextField {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let bytes = self.utf16_range_to_bytes(&range);
        *adjusted_range = Some(range);
        self.text.get(bytes).map(str::to_string)
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = utf16_len(&self.text);
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.clone()
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace(range, text, cx);
        self.marked_range = None;
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let start = self.replace(range, new_text, cx);
        self.marked_range = (!new_text.is_empty()).then(|| start..start + utf16_len(new_text));
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _: gpui_kit::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(utf16_len(&self.text))
    }
}

impl Render for BoardTextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_focused = self.focus_handle.is_focused(window);
        let entity = cx.entity();
        let focus_handle = self.focus_handle.clone();
        let (content, color) = if self.text.is_empty() {
            (self.placeholder.to_string(), self.placeholder_color)
        } else {
            (self.text.clone(), self.text_color)
        };
        let caret_color = self.caret_color;
        let caret =
            move || is_focused.then(|| div().w(px(1.0)).h(px(16.0)).flex_none().bg(caret_color));
        let caret_first = self.text.is_empty();
        div()
            .id("board-text-field")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_key_down))
            .relative()
            .flex()
            .items_center()
            .w_full()
            .overflow_hidden()
            .text_color(color)
            .children(caret().filter(|_| caret_first))
            .child(div().flex_none().whitespace_nowrap().child(content))
            .children(caret().filter(|_| !caret_first))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window: &mut Window, cx: &mut App| {
                        window.handle_input(
                            &focus_handle,
                            ElementInputHandler::new(bounds, entity.clone()),
                            cx,
                        );
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}
