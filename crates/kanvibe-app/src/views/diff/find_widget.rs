//! Monaco 찾기(·바꾸기) 위젯(`findWidget.css`, 라이트 테마). 편집기 오른쪽 위(오른쪽 여백 28px)에 붙는 419px 막대로,
//! 찾을 글자, 대소문자·단어 단위·정규식 토글, 결과 수, 이전·다음, 선택 영역에서 찾기, 닫기가 있다. 편집할 수 있는 편집기에서는
//! 왼쪽 펼침 단추와 바꿀 글자·바꾸기·모두 바꾸기 줄이 더 있다. 검색과 결과 이동은 편집기가 맡고, 위젯은 입력과 사건만 낸다.
//! Monaco 바꾸기 입력의 "대/소문자 보존" 토글은 옮기지 않았다.

use std::ops::Range;

use gpui_kit::{
    App, Bounds, ClipboardItem, Context, ElementInputHandler, EntityInputHandler, EventEmitter,
    FocusHandle, Hsla, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement,
    Pixels, Render, Rgba, SharedString, StatefulInteractiveElement, Styled, TextAlign, TextRun,
    UTF16Selection, Window, canvas, div, fill, point, prelude::FluentBuilder, px, rgb, size,
};
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::shortcuts::key_event::match_shortcut_event;

use super::line_diff::TextPoint;
use super::search::FindOptions;
use super::text_buffer::TextBuffer;
use crate::fonts::sans_font;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::shortcuts::shortcut_event_from_keystroke;

/// 위젯 전체 폭(`FIND_WIDGET_INITIAL_WIDTH`)
const WIDGET_WIDTH: f32 = 419.0;
/// 편집기 오른쪽 끝에서 띄우는 거리(세로 스크롤바 두 배)
pub const WIDGET_RIGHT_OFFSET: f32 = 28.0;
const FIND_ONLY_HEIGHT: f32 = 33.0;
const WITH_REPLACE_HEIGHT: f32 = 62.0;
/// 입력란 폭(`PART_WIDTH - 54`)
const INPUT_WIDTH: f32 = 221.0;
const INPUT_HEIGHT: f32 = 25.0;
const INPUT_FONT_SIZE: f32 = 13.0;
const INPUT_PADDING_LEFT: f32 = 4.0;
/// 입력란 오른쪽 토글 세 개 자리
const FIND_CONTROLS_WIDTH: f32 = 66.0;
const MATCHES_COUNT_MIN_WIDTH: f32 = 69.0;

/// Monaco 라이트 테마 위젯 색
mod widget_colors {
    use gpui_kit::{Rgba, rgb};

    pub fn background() -> Rgba {
        rgb(0xF3F3F3)
    }
    pub fn foreground() -> Rgba {
        rgb(0x616161)
    }
    pub fn input_background() -> Rgba {
        rgb(0xFFFFFF)
    }
    pub fn focus_border() -> Rgba {
        rgb(0x0090F1)
    }
    pub fn error_foreground() -> Rgba {
        rgb(0xA1260D)
    }
    pub fn error_border() -> Rgba {
        rgb(0xBE1100)
    }
    pub fn icon() -> Rgba {
        rgb(0x424242)
    }
    pub fn toggle_active_border() -> Rgba {
        rgb(0x007ACC)
    }
    pub fn selection() -> Rgba {
        rgb(0xADD6FF)
    }
    /// `inputOption.activeBackground`(focusBorder 20%)
    pub const TOGGLE_ACTIVE_BACKGROUND: Rgba = Rgba {
        r: 0.0,
        g: 144.0 / 255.0,
        b: 241.0 / 255.0,
        a: 0.2,
    };
    /// `toolbar.hoverBackground`
    pub const BUTTON_HOVER: Rgba = Rgba {
        r: 184.0 / 255.0,
        g: 184.0 / 255.0,
        b: 184.0 / 255.0,
        a: 80.0 / 255.0,
    };
    /// `widget.shadow`
    pub const SHADOW: Rgba = Rgba {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.16,
    };
    /// `input.placeholderForeground`(foreground 50%)
    pub const PLACEHOLDER: Rgba = Rgba {
        r: 97.0 / 255.0,
        g: 97.0 / 255.0,
        b: 97.0 / 255.0,
        a: 0.5,
    };
}

/// 위젯이 편집기에 알리는 일
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindEvent {
    /// 찾을 글자·토글·선택 영역 범위가 바뀌었다
    Changed,
    Next,
    Previous,
    ReplaceOne,
    ReplaceAll,
    Close,
}

/// 결과 수 표시(`matchesCount`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchSummary {
    NoQuery,
    NoResults,
    Position {
        current: Option<usize>,
        total: usize,
    },
    InvalidPattern,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Find,
    Replace,
}

pub struct FindWidget {
    query: TextBuffer,
    replacement: TextBuffer,
    options: FindOptions,
    is_in_selection: bool,
    is_replace_allowed: bool,
    is_replace_visible: bool,
    summary: MatchSummary,
    find_focus: FocusHandle,
    replace_focus: FocusHandle,
    platform: ShortcutPlatform,
}

impl EventEmitter<FindEvent> for FindWidget {}

impl FindWidget {
    pub fn new(
        initial_query: &str,
        is_replace_allowed: bool,
        platform: ShortcutPlatform,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut query = TextBuffer::new(initial_query);
        query.select_all();
        Self {
            query,
            replacement: TextBuffer::new(""),
            options: FindOptions::default(),
            is_in_selection: false,
            is_replace_allowed,
            is_replace_visible: false,
            summary: MatchSummary::NoQuery,
            find_focus: cx.focus_handle(),
            replace_focus: cx.focus_handle(),
            platform,
        }
    }

    pub fn query(&self) -> String {
        self.query.text()
    }

    pub fn replacement(&self) -> String {
        self.replacement.text()
    }

    pub fn options(&self) -> FindOptions {
        self.options
    }

    pub fn is_in_selection(&self) -> bool {
        self.is_in_selection
    }

    pub fn set_summary(&mut self, summary: MatchSummary, cx: &mut Context<Self>) {
        self.summary = summary;
        cx.notify();
    }

    /// 다시 열 때: 새 검색어가 있으면 바꾸고, 찾을 글자를 모두 골라 바로 덮어 쓸 수 있게 한다
    pub fn reopen(
        &mut self,
        seed: Option<&str>,
        should_show_replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(seed) = seed.filter(|seed| !seed.is_empty()) {
            self.query = TextBuffer::new(seed);
            cx.emit(FindEvent::Changed);
        }
        self.query.select_all();
        if should_show_replace && self.is_replace_allowed {
            self.is_replace_visible = true;
            // Monaco 바꾸기 열기: 찾을 글자가 이미 있으면 바꿀 글자로, 없으면 찾을 글자로 간다
            let focus = if self.query.text().is_empty() {
                &self.find_focus
            } else {
                &self.replace_focus
            };
            window.focus(focus, cx);
        } else {
            window.focus(&self.find_focus, cx);
        }
        cx.notify();
    }

    fn active_field(&self, window: &Window) -> Field {
        if self.replace_focus.is_focused(window) {
            Field::Replace
        } else {
            Field::Find
        }
    }

    fn buffer_mut(&mut self, field: Field) -> &mut TextBuffer {
        match field {
            Field::Find => &mut self.query,
            Field::Replace => &mut self.replacement,
        }
    }

    fn after_text_change(&mut self, field: Field, cx: &mut Context<Self>) {
        if field == Field::Find {
            cx.emit(FindEvent::Changed);
        }
        cx.notify();
    }

    fn toggle_option(&mut self, option: FindOption, cx: &mut Context<Self>) {
        match option {
            FindOption::CaseSensitive => {
                self.options.is_case_sensitive = !self.options.is_case_sensitive
            }
            FindOption::WholeWord => self.options.is_whole_word = !self.options.is_whole_word,
            FindOption::Regex => self.options.is_regex = !self.options.is_regex,
            FindOption::InSelection => self.is_in_selection = !self.is_in_selection,
        }
        cx.emit(FindEvent::Changed);
        cx.notify();
    }

    fn toggle_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.is_replace_visible = !self.is_replace_visible;
        let focus = if self.is_replace_visible {
            &self.replace_focus
        } else {
            &self.find_focus
        };
        window.focus(focus, cx);
        cx.notify();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FindOption {
    CaseSensitive,
    WholeWord,
    Regex,
    InSelection,
}

/// 키보드: Monaco 찾기 위젯 단축키와 입력란 편집 키
impl FindWidget {
    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_handled = self.handle_widget_shortcut(event, window, cx)
            || self.handle_field_key(event, window, cx);
        if is_handled {
            cx.stop_propagation();
        }
    }

    fn widget_shortcut(&self, event: &KeyDownEvent) -> Option<FindCommand> {
        let shortcut = shortcut_event_from_keystroke(&event.keystroke);
        let is_mac = self.platform == ShortcutPlatform::Mac;
        let table: [(&str, FindCommand); 11] = [
            ("Escape", FindCommand::Close),
            ("Enter", FindCommand::EnterKey),
            ("Shift+Enter", FindCommand::Emit(FindEvent::Previous)),
            ("F3", FindCommand::Emit(FindEvent::Next)),
            ("Shift+F3", FindCommand::Emit(FindEvent::Previous)),
            (
                if is_mac { "Meta+Alt+C" } else { "Alt+C" },
                FindCommand::Toggle(FindOption::CaseSensitive),
            ),
            (
                if is_mac { "Meta+Alt+W" } else { "Alt+W" },
                FindCommand::Toggle(FindOption::WholeWord),
            ),
            (
                if is_mac { "Meta+Alt+R" } else { "Alt+R" },
                FindCommand::Toggle(FindOption::Regex),
            ),
            ("Mod+Shift+1", FindCommand::Emit(FindEvent::ReplaceOne)),
            (
                if is_mac {
                    "Meta+Enter"
                } else {
                    "Ctrl+Alt+Enter"
                },
                FindCommand::Emit(FindEvent::ReplaceAll),
            ),
            ("Tab", FindCommand::SwitchField),
        ];
        table
            .into_iter()
            .find(|(notation, _)| match_shortcut_event(&shortcut, notation, self.platform))
            .map(|(_, command)| command)
    }

    fn handle_widget_shortcut(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(command) = self.widget_shortcut(event) else {
            return false;
        };
        let is_replace_field = self.active_field(window) == Field::Replace;
        match command {
            FindCommand::Close => cx.emit(FindEvent::Close),
            FindCommand::EnterKey if is_replace_field => cx.emit(FindEvent::ReplaceOne),
            FindCommand::EnterKey => cx.emit(FindEvent::Next),
            FindCommand::Emit(find_event) => cx.emit(find_event),
            FindCommand::Toggle(option) => self.toggle_option(option, cx),
            FindCommand::SwitchField if self.is_replace_visible => {
                let target = if is_replace_field {
                    &self.find_focus
                } else {
                    &self.replace_focus
                };
                window.focus(target, cx);
            }
            FindCommand::SwitchField => return false,
        }
        true
    }

    fn handle_field_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let field = self.active_field(window);
        let shortcut = shortcut_event_from_keystroke(&event.keystroke);
        let platform = self.platform;
        let matches = |notation: &str| match_shortcut_event(&shortcut, notation, platform);
        if matches("Mod+A") {
            self.buffer_mut(field).select_all();
        } else if matches("Mod+C") || matches("Mod+X") {
            self.copy_field(field, matches("Mod+X"), cx);
        } else if matches("Mod+V") {
            let pasted = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .unwrap_or_default();
            self.buffer_mut(field)
                .insert_text(&pasted.replace(['\r', '\n'], " "));
            self.after_text_change(field, cx);
        } else if !self.handle_editing_key(field, event, cx) {
            return false;
        }
        cx.notify();
        true
    }

    fn copy_field(&mut self, field: Field, is_cut: bool, cx: &mut Context<Self>) {
        let Some(text) = self.buffer_mut(field).selected_text() else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        if is_cut {
            self.buffer_mut(field).delete_backward();
            self.after_text_change(field, cx);
        }
    }

    fn handle_editing_key(
        &mut self,
        field: Field,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.platform || modifiers.alt {
            return false;
        }
        let is_extending = modifiers.shift;
        let buffer = self.buffer_mut(field);
        match event.keystroke.key.as_str() {
            "left" => buffer.move_horizontally(false, is_extending),
            "right" => buffer.move_horizontally(true, is_extending),
            "home" | "up" => buffer.set_cursor(TextPoint { line: 0, column: 0 }, is_extending),
            "end" | "down" => buffer.move_to_line_end(is_extending),
            "backspace" => buffer.delete_backward(),
            "delete" => buffer.delete_forward(),
            _ => return false,
        }
        if matches!(event.keystroke.key.as_str(), "backspace" | "delete") {
            self.after_text_change(field, cx);
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FindCommand {
    Close,
    EnterKey,
    Emit(FindEvent),
    Toggle(FindOption),
    SwitchField,
}

impl EntityInputHandler for FindWidget {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.buffer_mut(self.active_field(window)).text();
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let range = range.start.min(utf16.len())..range.end.min(utf16.len());
        *adjusted_range = Some(range.clone());
        Some(String::from_utf16_lossy(&utf16[range]))
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let buffer = self.buffer_mut(self.active_field(window));
        let cursor = buffer.cursor();
        let (start, end) = buffer.selection().unwrap_or((cursor, cursor));
        let range = buffer.utf16_offset(start)..buffer.utf16_offset(end);
        Some(UTF16Selection {
            reversed: cursor == start && start != end,
            range,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let field = self.active_field(window);
        let buffer = self.buffer_mut(field);
        if let Some(range) = range {
            buffer.select_utf16_range(range);
        }
        buffer.insert_text(&text.replace(['\r', '\n'], " "));
        self.after_text_change(field, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_text_in_range(range, new_text, window, cx);
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
        None
    }
}

/// 그리기
impl FindWidget {
    fn summary_label(&self) -> (String, Rgba) {
        match self.summary {
            MatchSummary::NoQuery => ("No results".into(), widget_colors::foreground()),
            MatchSummary::NoResults | MatchSummary::InvalidPattern => {
                ("No results".into(), widget_colors::error_foreground())
            }
            MatchSummary::Position {
                current: Some(current),
                total,
            } => (
                format!("{} of {total}", current + 1),
                widget_colors::foreground(),
            ),
            MatchSummary::Position {
                current: None,
                total,
            } => (format!("? of {total}"), widget_colors::foreground()),
        }
    }

    fn render_input(
        &self,
        field: Field,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (buffer, focus, placeholder, controls_width) = match field {
            Field::Find => (&self.query, &self.find_focus, "Find", FIND_CONTROLS_WIDTH),
            Field::Replace => (&self.replacement, &self.replace_focus, "Replace", 0.0),
        };
        let is_focused = focus.is_focused(window);
        let has_error = field == Field::Find && self.summary == MatchSummary::InvalidPattern;
        let border = if has_error {
            Some(widget_colors::error_border())
        } else {
            is_focused.then(widget_colors::focus_border)
        };
        let text_area = self.render_text_area(buffer, focus, is_focused, placeholder, cx);
        let focus_on_click = focus.clone();
        div()
            .id(SharedString::from(format!("find-input-{placeholder}")))
            .relative()
            .flex_none()
            .w(px(INPUT_WIDTH))
            .h(px(INPUT_HEIGHT))
            .bg(widget_colors::input_background())
            .track_focus(focus)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&focus_on_click, cx)
            })
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(INPUT_PADDING_LEFT))
                    .right(px(controls_width + 2.0))
                    .child(text_area),
            )
            .when_some(border, |input, color| input.border_1().border_color(color))
            .when(field == Field::Find, |input| {
                input.child(self.render_option_toggles(cx))
            })
            .child(qa_target_layer(QaTargetDescription {
                role: "textbox",
                name: placeholder.into(),
                text: buffer.text().into(),
            }))
    }

    /// 입력란 글자 영역. 포커스가 있으면 입력기를 이 영역에 건다
    fn render_text_area(
        &self,
        buffer: &TextBuffer,
        focus: &FocusHandle,
        is_focused: bool,
        placeholder: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let paint = InputPaint {
            text: buffer.text(),
            cursor: buffer.cursor().column,
            selection: buffer
                .selection()
                .map(|(start, end)| start.column..end.column),
            placeholder,
            is_focused,
        };
        let (focus_for_input, entity) = (focus.clone(), cx.entity());
        canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                window.handle_input(
                    &focus_for_input,
                    ElementInputHandler::new(bounds, entity),
                    cx,
                );
                paint.paint(bounds, window, cx);
            },
        )
        .size_full()
    }

    fn render_option_toggles(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let toggles = [
            (
                FindOption::CaseSensitive,
                "Aa",
                "Match Case",
                self.options.is_case_sensitive,
            ),
            (
                FindOption::WholeWord,
                "ab",
                "Match Whole Word",
                self.options.is_whole_word,
            ),
            (
                FindOption::Regex,
                ".*",
                "Use Regular Expression",
                self.options.is_regex,
            ),
        ];
        let buttons = toggles.into_iter().map(|(option, glyph, name, is_active)| {
            let toggle = icon_button(name, glyph, 20.0)
                .ml(px(2.0))
                .rounded(px(3.0))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_option(option, cx)));
            if is_active {
                toggle
                    .border_1()
                    .border_color(widget_colors::toggle_active_border())
                    .bg(widget_colors::TOGGLE_ACTIVE_BACKGROUND)
            } else {
                toggle
            }
        });
        div()
            .absolute()
            .top(px(2.0))
            .right(px(2.0))
            .flex()
            .children(buttons)
    }

    fn render_find_part(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (label, color) = self.summary_label();
        let in_selection = icon_button("Find in Selection", "≡", 22.0)
            .ml(px(3.0))
            .when(self.is_in_selection, |button| {
                button.bg(widget_colors::TOGGLE_ACTIVE_BACKGROUND)
            })
            .on_click(
                cx.listener(|this, _, _, cx| this.toggle_option(FindOption::InSelection, cx)),
            );
        div()
            .flex()
            .items_center()
            .mt(px(3.0))
            .ml(px(17.0))
            .mr(px(25.0))
            .child(self.render_input(Field::Find, window, cx))
            .child(
                div()
                    .relative()
                    .flex_none()
                    .ml(px(3.0))
                    .pl(px(2.0))
                    .min_w(px(MATCHES_COUNT_MIN_WIDTH))
                    .h(px(INPUT_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(color)
                    .child(label.clone())
                    .child(qa_target_layer(QaTargetDescription {
                        role: "status",
                        name: label.clone().into(),
                        text: label.into(),
                    })),
            )
            .child(
                icon_button("Previous Match", "↑", 22.0)
                    .ml(px(3.0))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::Previous))),
            )
            .child(
                icon_button("Next Match", "↓", 22.0)
                    .ml(px(3.0))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::Next))),
            )
            .child(in_selection)
    }

    fn render_replace_part(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .mt(px(4.0))
            .ml(px(17.0))
            .mr(px(25.0))
            .child(self.render_input(Field::Replace, window, cx))
            .child(
                icon_button("Replace", "↵", 22.0)
                    .ml(px(3.0))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::ReplaceOne))),
            )
            .child(
                icon_button("Replace All", "⇶", 22.0)
                    .ml(px(3.0))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::ReplaceAll))),
            )
    }
}

/// 16px 아이콘 + 3px 여백 단추(`.find-widget .button`). Monaco codicon 대신 같은 뜻의 글자로 그린다
fn icon_button(
    name: &'static str,
    glyph: &'static str,
    side: f32,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(name)
        .relative()
        .flex_none()
        .size(px(side))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.0))
        .text_color(widget_colors::icon())
        .text_size(px(13.0))
        .cursor_pointer()
        .hover(|style| style.bg(widget_colors::BUTTON_HOVER))
        .child(glyph)
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: name.into(),
            text: name.into(),
        }))
}

/// 입력란 글자·선택·커서
struct InputPaint {
    text: String,
    cursor: usize,
    selection: Option<Range<usize>>,
    placeholder: &'static str,
    is_focused: bool,
}

impl InputPaint {
    /// 글자가 없으면 흐린 자리표시 글자(`Find`·`Replace`)를 그린다
    fn shape(&self, window: &mut Window) -> gpui_kit::ShapedLine {
        let (shown, color) = if self.text.is_empty() {
            (self.placeholder.to_string(), widget_colors::PLACEHOLDER)
        } else {
            (self.text.clone(), rgb(0x000000))
        };
        let run = TextRun {
            len: shown.len(),
            font: sans_font(400),
            color: Hsla::from(color),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window.text_system().shape_line(
            SharedString::from(shown),
            px(INPUT_FONT_SIZE),
            &[run],
            None,
        )
    }

    fn paint(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let shaped = self.shape(window);
        let caret_x = if self.text.is_empty() {
            0.0
        } else {
            f32::from(shaped.x_for_index(self.cursor))
        };
        // 커서가 오른쪽 끝을 넘으면 글자를 왼쪽으로 밀어 커서를 보인다
        let origin_x =
            f32::from(bounds.origin.x) - (caret_x - f32::from(bounds.size.width) + 2.0).max(0.0);
        window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
            if let Some(selection) = self.selection.clone().filter(|_| !self.text.is_empty()) {
                let start = f32::from(shaped.x_for_index(selection.start));
                let end = f32::from(shaped.x_for_index(selection.end));
                window.paint_quad(fill(
                    inner_bar(bounds, origin_x + start, end - start),
                    widget_colors::selection(),
                ));
            }
            let _ = shaped.paint(
                point(px(origin_x), bounds.origin.y),
                bounds.size.height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
            if self.is_focused {
                window.paint_quad(fill(
                    inner_bar(bounds, (origin_x + caret_x).round(), 1.0),
                    rgb(0x000000),
                ));
            }
        });
    }
}

/// 입력란 안쪽 위아래 4px을 뺀 세로 막대(선택·커서)
fn inner_bar(bounds: Bounds<Pixels>, x: f32, width: f32) -> Bounds<Pixels> {
    Bounds::new(
        point(px(x), bounds.origin.y + px(4.0)),
        size(px(width), bounds.size.height - px(8.0)),
    )
}

impl FindWidget {
    /// 편집할 수 있는 편집기에서만 보이는 왼쪽 바꾸기 펼침 단추(`button.toggle`)
    fn render_replace_toggle(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let glyph = if self.is_replace_visible {
            "⌄"
        } else {
            "›"
        };
        self.is_replace_allowed.then(|| {
            icon_button("Toggle Replace", glyph, 18.0)
                .absolute()
                .top_0()
                .left(px(3.0))
                .h_full()
                .rounded(px(0.0))
                .on_click(cx.listener(|this, _, window, cx| this.toggle_replace(window, cx)))
        })
    }
}

impl Render for FindWidget {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let height = if self.is_replace_visible {
            WITH_REPLACE_HEIGHT
        } else {
            FIND_ONLY_HEIGHT
        };
        let shadow = gpui_kit::BoxShadow {
            color: widget_colors::SHADOW.into(),
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(8.0),
            spread_radius: px(2.0),
            inset: false,
        };
        let replace_toggle = self
            .render_replace_toggle(cx)
            .map(IntoElement::into_any_element);
        div()
            .id("find-widget")
            .key_context("FindWidget")
            .on_key_down(cx.listener(Self::handle_key_down))
            .absolute()
            .top_0()
            .right(px(WIDGET_RIGHT_OFFSET))
            .w(px(WIDGET_WIDTH))
            .h(px(height))
            .px(px(4.0))
            .bg(widget_colors::background())
            .shadow(vec![shadow])
            .rounded_b(px(4.0))
            .font(sans_font(400))
            .text_size(px(12.0))
            .line_height(px(19.0))
            .text_color(widget_colors::foreground())
            .children(replace_toggle)
            .child(self.render_find_part(window, cx))
            .when(self.is_replace_visible, |widget| {
                widget.child(self.render_replace_part(window, cx))
            })
            .child(
                icon_button("Close (Escape)", "×", 22.0)
                    .absolute()
                    .top(px(5.0))
                    .right(px(4.0))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(FindEvent::Close))),
            )
    }
}

/// 편집기가 위젯을 열 때 넘기는 첫 검색어: 한 줄 선택이 있으면 그 글자, 없으면 커서 아래 단어(Monaco `seedSearchStringFromSelection`)
pub fn seed_query(
    lines: &[String],
    selection: Option<(TextPoint, TextPoint)>,
    cursor: TextPoint,
) -> Option<String> {
    if let Some((start, end)) = selection {
        return (start.line == end.line)
            .then(|| lines[start.line][start.column..end.column].to_string());
    }
    let line = lines.get(cursor.line)?;
    let range = super::word_boundary::word_range_at(line, cursor.column);
    let word = &line[range];
    word.chars()
        .all(|character| character.is_alphanumeric() || character == '_')
        .then(|| word.to_string())
        .filter(|word| !word.is_empty())
}
