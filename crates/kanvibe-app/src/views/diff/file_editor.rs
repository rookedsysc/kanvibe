//! 편집 모드(TS `DiffFileEditor`): 파일 경로·저장하지 않은 변경 표시·저장 버튼 줄과 Monaco 단일 편집기.
//! `Mod+S`와 저장 버튼은 같은 저장 경로를 쓰고, 저장 중에는 어느 쪽으로도 다시 보내지 않는다.
//! 저장 실패 사유는 다시 저장할 때까지 남긴다. 저장 중 더 편집했으면 저장이 끝나도 미저장 표시를 지우지 않는다.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{
    App, Bounds, ClipboardItem, Context, ElementInputHandler, Entity, EntityInputHandler,
    FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement, Pixels, Render, Rgba, ScrollEvent, StatefulInteractiveElement,
    Styled, Task, UTF16Selection, Window, canvas, div, point, prelude::FluentBuilder, px, size,
};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::shortcuts::key_event::match_shortcut_event;

use super::line_diff::TextPoint;
use super::monaco::{
    CHARACTER_WIDTH, GutterLayout, LINE_HEIGHT, MonacoPane, PaneDocument, PaneRow,
    active_indent_guide, hit_test, paint_pane,
};
use super::syntax::{Language, detect_language, matching_bracket_pair, tokenize_document};
use super::text_baseline::{TextFamily, aligned_text, chromium_text};
use super::text_buffer::TextBuffer;
use super::viewer::SCROLLBAR_FADE_AFTER;
use crate::components::fade;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::services::AppServices;
use crate::shortcuts::shortcut_event_from_keystroke;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::TailwindText;

/// 저장 완료 표시를 보여 주는 시간(TS `setTimeout(..., 2000)`)
const SAVED_DISPLAY_DURATION: Duration = Duration::from_millis(2000);
/// Monaco `cursorBlinking: blink` 주기
const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(500);

/// Tailwind `bg-green-600`(저장 완료 버튼)
fn saved_button_color() -> Rgba {
    gpui_kit::rgb(0x00A63E)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveStatus {
    Idle,
    Saving,
    Saved,
}

/// 편집기가 부르는 저장. 실패하면 화면에 보일 사유를 돌려준다(TS `onSave`가 던지던 오류)
pub type SaveHandler = Rc<dyn Fn(String, &mut App) -> Task<Result<(), String>>>;

pub struct DiffFileEditor {
    file_path: String,
    locale: Locale,
    language: Language,
    buffer: TextBuffer,
    document: Rc<PaneDocument>,
    document_revision: u64,
    save_handler: SaveHandler,
    save_status: SaveStatus,
    save_failure_reason: Option<String>,
    has_unsaved_changes: bool,
    focus_handle: FocusHandle,
    scroll_x: f32,
    scroll_y: f32,
    surface_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    is_selecting: bool,
    is_caret_visible: bool,
    /// 입력기가 조합 중인 글자 범위(UTF-16)
    marked_range: Option<Range<usize>>,
    _save: Option<Task<()>>,
    _saved_reset: Option<Task<()>>,
    _caret_blink: Option<Task<()>>,
    is_scrollbar_visible: bool,
    _scrollbar_fade: Option<Task<()>>,
}

impl DiffFileEditor {
    pub fn new(
        content: &str,
        file_path: &str,
        locale: Locale,
        save_handler: SaveHandler,
        cx: &mut Context<Self>,
    ) -> Self {
        let language = detect_language(file_path);
        let buffer = TextBuffer::new(content);
        let document = Rc::new(build_document(&buffer, language));
        Self {
            file_path: file_path.to_string(),
            locale,
            language,
            buffer,
            document,
            document_revision: 0,
            save_handler,
            save_status: SaveStatus::Idle,
            save_failure_reason: None,
            has_unsaved_changes: false,
            focus_handle: cx.focus_handle(),
            scroll_x: 0.0,
            scroll_y: 0.0,
            surface_bounds: Rc::new(Cell::new(None)),
            is_selecting: false,
            is_caret_visible: true,
            marked_range: None,
            _save: None,
            _saved_reset: None,
            _caret_blink: None,
            is_scrollbar_visible: false,
            _scrollbar_fade: None,
        }
    }

    /// 버튼과 `Mod+S`가 함께 쓰는 저장. 저장 중이면 아무것도 하지 않는다(TS `isSavingRef`)
    pub fn save(&mut self, cx: &mut Context<Self>) {
        if self.save_status == SaveStatus::Saving {
            return;
        }
        let saving_revision = self.buffer.revision();
        self.save_status = SaveStatus::Saving;
        self.save_failure_reason = None;
        self._saved_reset = None;
        let save = (self.save_handler)(self.buffer.text(), cx);
        self._save = Some(cx.spawn(async move |this, cx| {
            let result = save.await;
            let _ = this.update(cx, |this, cx| this.finish_save(result, saving_revision, cx));
        }));
        cx.notify();
    }

    fn finish_save(
        &mut self,
        result: Result<(), String>,
        saving_revision: u64,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(()) => {
                self.save_status = SaveStatus::Saved;
                if self.buffer.revision() == saving_revision {
                    self.has_unsaved_changes = false;
                }
                self._saved_reset = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(SAVED_DISPLAY_DURATION).await;
                    let _ = this.update(cx, |this, cx| {
                        this.save_status = SaveStatus::Idle;
                        cx.notify();
                    });
                }));
            }
            Err(reason) => {
                self.save_status = SaveStatus::Idle;
                self.save_failure_reason = Some(reason);
            }
        }
        cx.notify();
    }

    /// 내용이 바뀐 뒤 부른다(TS `handleEditorChange`)
    fn after_edit(&mut self, cx: &mut Context<Self>) {
        self.has_unsaved_changes = true;
        self.after_cursor_move(cx);
    }

    fn after_cursor_move(&mut self, cx: &mut Context<Self>) {
        if self.document_revision != self.buffer.revision() {
            self.document = Rc::new(build_document(&self.buffer, self.language));
            self.document_revision = self.buffer.revision();
        }
        self.restart_caret_blink(cx);
        self.scroll_caret_into_view();
        cx.notify();
    }

    fn restart_caret_blink(&mut self, cx: &mut Context<Self>) {
        self.is_caret_visible = true;
        self._caret_blink = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CARET_BLINK_INTERVAL).await;
                let is_alive = this.update(cx, |this, cx| {
                    this.is_caret_visible = !this.is_caret_visible;
                    cx.notify();
                });
                if is_alive.is_err() {
                    break;
                }
            }
        }));
    }

    fn scroll_caret_into_view(&mut self) {
        let Some(bounds) = self.surface_bounds.get() else {
            return;
        };
        let caret = self.buffer.cursor();
        let caret_top = caret.line as f32 * LINE_HEIGHT;
        let viewport_height = f32::from(bounds.size.height);
        if caret_top < self.scroll_y {
            self.scroll_y = caret_top;
        } else if caret_top + LINE_HEIGHT > self.scroll_y + viewport_height {
            self.scroll_y = caret_top + LINE_HEIGHT - viewport_height;
        }
        let caret_x = self.buffer.lines()[caret.line][..caret.column]
            .chars()
            .count() as f32
            * CHARACTER_WIDTH;
        let content_width = f32::from(bounds.size.width)
            - self.pane_gutter_width()
            - super::monaco::SCROLLBAR_WIDTH;
        if caret_x < self.scroll_x {
            self.scroll_x = caret_x;
        } else if caret_x > self.scroll_x + content_width {
            self.scroll_x = caret_x - content_width;
        }
    }

    fn pane_gutter_width(&self) -> f32 {
        GutterLayout::EDITOR.content_left(self.document.lines.len())
    }

    fn pane(&self, window: &Window) -> MonacoPane {
        let caret = self.buffer.cursor();
        let is_focused = self.focus_handle.is_focused(window) && window.is_window_active();
        let bracket_boxes =
            matching_bracket_pair(&self.document.tokens, (caret.line, caret.column))
                .map(|(open, close)| {
                    vec![
                        TextPoint {
                            line: open.0,
                            column: open.1,
                        },
                        TextPoint {
                            line: close.0,
                            column: close.1,
                        },
                    ]
                })
                .unwrap_or_default();
        MonacoPane {
            document: Rc::clone(&self.document),
            rows: Rc::new(
                (0..self.document.lines.len())
                    .map(|line| PaneRow::Text {
                        line,
                        decoration: Default::default(),
                    })
                    .collect(),
            ),
            gutter: GutterLayout::EDITOR,
            current_line: Some(caret.line),
            caret: (is_focused && self.is_caret_visible).then_some(caret),
            selection: self.buffer.selection(),
            bracket_boxes,
            active_guide: active_indent_guide(&self.document, caret.line),
            has_right_shadow: false,
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            is_scrollbar_visible: self.is_scrollbar_visible,
        }
    }
}

fn build_document(buffer: &TextBuffer, language: Language) -> PaneDocument {
    let lines = buffer.lines().to_vec();
    let tokens = tokenize_document(&lines, language);
    PaneDocument::new(lines, tokens)
}

/// 키보드 입력. 글자 입력은 입력기(`EntityInputHandler`)로 흘려보내고 편집·이동 키만 여기서 처리한다
impl DiffFileEditor {
    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.handle_shortcut(event, window, cx) || self.handle_editing_key(event, cx) {
            cx.stop_propagation();
        }
    }

    fn handle_shortcut(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let platform = cx.global::<AppServices>().shortcut_platform;
        let shortcut = shortcut_event_from_keystroke(&event.keystroke);
        let matches = |notation: &str| match_shortcut_event(&shortcut, notation, platform);
        if matches("Mod+S") {
            self.save(cx);
        } else if matches("Mod+Z") {
            self.apply_history(true, cx);
        } else if matches("Mod+Shift+Z") || matches("Mod+Y") {
            self.apply_history(false, cx);
        } else if matches("Mod+A") {
            self.buffer.select_all();
            cx.notify();
        } else if matches("Mod+C") || matches("Mod+X") {
            self.copy_selection(matches("Mod+X"), cx);
        } else if matches("Mod+V") {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.buffer.insert_text(&text);
                self.after_edit(cx);
            }
        } else {
            return false;
        }
        true
    }

    fn apply_history(&mut self, is_undo: bool, cx: &mut Context<Self>) {
        let has_changed = if is_undo {
            self.buffer.undo()
        } else {
            self.buffer.redo()
        };
        if has_changed {
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

    fn handle_editing_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.platform || modifiers.alt {
            return false;
        }
        let is_extending = modifiers.shift;
        let page_lines = self
            .surface_bounds
            .get()
            .map_or(20.0, |bounds| f32::from(bounds.size.height) / LINE_HEIGHT)
            as isize;
        match event.keystroke.key.as_str() {
            "left" => self.buffer.move_horizontally(false, is_extending),
            "right" => self.buffer.move_horizontally(true, is_extending),
            "up" => self.buffer.move_vertically(-1, is_extending),
            "down" => self.buffer.move_vertically(1, is_extending),
            "pageup" => self.buffer.move_vertically(-page_lines, is_extending),
            "pagedown" => self.buffer.move_vertically(page_lines, is_extending),
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

    fn handle_edit_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        match key {
            "backspace" => self.buffer.delete_backward(),
            "delete" => self.buffer.delete_forward(),
            "enter" => self.buffer.insert_newline(),
            _ => self.buffer.insert_indent(),
        }
        self.after_edit(cx);
        true
    }
}

/// 마우스: 누르면 커서, 끌면 선택, 휠은 스크롤
impl DiffFileEditor {
    fn handle_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        let Some(bounds) = self.surface_bounds.get() else {
            return;
        };
        let pane = self.pane(window);
        let target = hit_test(&pane, bounds, event.position, window);
        self.buffer.set_cursor(target, event.modifiers.shift);
        self.is_selecting = true;
        self.after_cursor_move(cx);
    }

    fn handle_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_selecting || event.pressed_button != Some(MouseButton::Left) {
            self.is_selecting = false;
            return;
        }
        let Some(bounds) = self.surface_bounds.get() else {
            return;
        };
        let pane = self.pane(window);
        let target = hit_test(&pane, bounds, event.position, window);
        self.buffer.set_cursor(target, true);
        cx.notify();
    }

    fn handle_scroll(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.surface_bounds.get() else {
            return;
        };
        let delta = event.delta.pixel_delta(px(LINE_HEIGHT));
        let (max_x, max_y) = self.pane(window).scroll_limits(bounds);
        self.scroll_y = (self.scroll_y - f32::from(delta.y)).clamp(0.0, max_y);
        self.scroll_x = (self.scroll_x - f32::from(delta.x)).clamp(0.0, max_x);
        self.is_scrollbar_visible = true;
        self._scrollbar_fade = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SCROLLBAR_FADE_AFTER).await;
            let _ = this.update(cx, |this, cx| {
                this.is_scrollbar_visible = false;
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl EntityInputHandler for DiffFileEditor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let utf16: Vec<u16> = self.buffer.text().encode_utf16().collect();
        let range = range.start.min(utf16.len())..range.end.min(utf16.len());
        *adjusted_range = Some(range.clone());
        Some(String::from_utf16_lossy(&utf16[range]))
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let cursor = self.buffer.utf16_offset(self.buffer.cursor());
        let (start, end) = self
            .buffer
            .selection()
            .unwrap_or((self.buffer.cursor(), self.buffer.cursor()));
        let range = self.buffer.utf16_offset(start)..self.buffer.utf16_offset(end);
        Some(UTF16Selection {
            reversed: cursor == range.start && range.start != range.end,
            range,
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
        if let Some(range) = range.or_else(|| self.marked_range.clone()) {
            self.buffer.select_utf16_range(range);
        }
        self.marked_range = None;
        self.buffer.insert_text(text);
        self.after_edit(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(range) = range.or_else(|| self.marked_range.clone()) {
            self.buffer.select_utf16_range(range);
        }
        let (start, _) = self
            .buffer
            .selection()
            .unwrap_or((self.buffer.cursor(), self.buffer.cursor()));
        let start_offset = self.buffer.utf16_offset(start);
        self.buffer.insert_text(new_text);
        self.marked_range = Some(start_offset..start_offset + new_text.encode_utf16().count());
        self.after_edit(cx);
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let caret = self.buffer.cursor();
        let caret_x = self.buffer.lines()[caret.line][..caret.column]
            .chars()
            .count() as f32
            * CHARACTER_WIDTH;
        let x = element_bounds.origin.x + px(self.pane_gutter_width() + caret_x - self.scroll_x);
        let y = element_bounds.origin.y + px(caret.line as f32 * LINE_HEIGHT - self.scroll_y);
        Some(Bounds::new(point(x, y), size(px(2.0), px(LINE_HEIGHT))))
    }

    fn character_index_for_point(
        &mut self,
        position: gpui_kit::Point<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.surface_bounds.get()?;
        let pane = self.pane(window);
        let target = hit_test(&pane, bounds, position, window);
        Some(self.buffer.utf16_offset(target))
    }
}

impl DiffFileEditor {
    fn render_toolbar(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path = div().truncate().child(aligned_text(
            self.file_path.clone(),
            text_scale::XS,
            TextFamily::Mono,
        ));
        let unsaved = self.has_unsaved_changes.then(|| {
            let label = labels.translate("unsavedChanges");
            div()
                .relative()
                .flex_none()
                .css_weight(400)
                .text_color(theme.status.warning)
                .child(chromium_text(label.clone()))
                .child(qa_target_layer(QaTargetDescription {
                    role: "text",
                    name: label.clone().into(),
                    text: label.into(),
                }))
        });
        div()
            .flex()
            .items_center()
            .justify_between()
            .px(spacing(4.0))
            .py(spacing(2.0))
            .bg(theme.background.surface)
            .border_b_1()
            .border_color(theme.border.default)
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(spacing(2.0))
                    .text_scale(text_scale::XS)
                    .mono(400)
                    .text_color(theme.text.secondary)
                    .child(path)
                    .children(unsaved),
            )
            .child(self.render_save_button(theme, labels, cx))
    }

    fn render_save_button(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (label, background) = match self.save_status {
            SaveStatus::Saving => (labels.translate("saving"), fade(theme.brand.primary, 0.7)),
            SaveStatus::Saved => (labels.translate("saved"), saved_button_color()),
            SaveStatus::Idle => (labels.translate("save"), theme.brand.primary),
        };
        let hover_background = fade(theme.brand.primary, 0.9);
        let is_idle = self.save_status == SaveStatus::Idle;
        div()
            .id("diff-editor-save")
            .relative()
            .px(spacing(3.0))
            .py(spacing(1.0))
            .text_scale(text_scale::XS)
            .rounded(radius::MD)
            .bg(background)
            .text_color(gpui_kit::rgb(0xFFFFFF))
            .when(is_idle, |button| {
                button.hover(move |style| style.bg(hover_background))
            })
            .on_click(cx.listener(|this, _, _, cx| this.save(cx)))
            .child(chromium_text(label.clone()))
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: label.clone().into(),
                text: label.into(),
            }))
    }

    fn render_failure(&self, theme: &ThemeTokens, labels: &Translator) -> Option<impl IntoElement> {
        let reason = self.save_failure_reason.as_ref()?;
        let message = format!("{}: {reason}", labels.translate("saveError"));
        Some(
            div()
                .relative()
                .border_b_1()
                .border_color(fade(theme.status.error, 0.2))
                .bg(theme.background.page)
                .px(spacing(4.0))
                .py(spacing(1.5))
                .text_scale(text_scale::XS)
                .text_color(theme.status.error)
                .child(aligned_text(
                    message.clone(),
                    text_scale::XS,
                    TextFamily::Sans,
                ))
                .child(qa_target_layer(QaTargetDescription {
                    role: "alert",
                    name: message.clone().into(),
                    text: message.into(),
                })),
        )
    }

    fn render_surface(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pane = self.pane(window);
        let bounds_cell = Rc::clone(&self.surface_bounds);
        let focus_handle = self.focus_handle.clone();
        let entity: Entity<Self> = cx.entity();
        let entity_id = cx.entity_id();
        let surface = canvas(
            // 영역이 바뀌면 줄 위치 기록과 스크롤 한도를 새 영역으로 다시 그린다
            move |bounds, window, _| {
                if bounds_cell.replace(Some(bounds)) != Some(bounds) {
                    window.on_next_frame(move |_, cx| cx.notify(entity_id));
                    window.refresh();
                }
            },
            move |bounds, _, window, cx| {
                window.handle_input(&focus_handle, ElementInputHandler::new(bounds, entity), cx);
                paint_pane(bounds, &pane, window, cx);
            },
        )
        .size_full();
        div()
            .id("diff-file-editor")
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::handle_mouse_down))
            .on_mouse_move(cx.listener(Self::handle_mouse_move))
            .on_scroll(cx.listener(|this, event: &ScrollEvent, window, cx| {
                this.handle_scroll(event, window, cx)
            }))
            .child(surface)
            .child(self.render_line_targets())
    }

    /// 재생 드라이버가 코드 줄을 글자로 찾아 누를 수 있게 보이는 줄의 위치를 남긴다
    fn render_line_targets(&self) -> impl IntoElement {
        let viewport_height = self
            .surface_bounds
            .get()
            .map_or(0.0, |bounds| f32::from(bounds.size.height));
        let first_line = (self.scroll_y / LINE_HEIGHT).floor() as usize;
        let line_count = (viewport_height / LINE_HEIGHT).ceil() as usize + 1;
        let content_left = self.pane_gutter_width();
        let targets = self
            .document
            .lines
            .iter()
            .enumerate()
            .skip(first_line)
            .take(line_count)
            .map(|(line, text)| {
                div()
                    .absolute()
                    .top(px(line as f32 * LINE_HEIGHT - self.scroll_y))
                    .left(px(content_left - self.scroll_x))
                    .w(px(text.chars().count() as f32 * CHARACTER_WIDTH))
                    .h(px(LINE_HEIGHT))
                    .child(qa_target_layer(QaTargetDescription {
                        role: "code-line",
                        name: text.clone().into(),
                        text: text.clone().into(),
                    }))
            });
        div().absolute().inset_0().children(targets)
    }
}

impl Render for DiffFileEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let labels = Translator::new(self.locale, "diffView");
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(self.render_toolbar(&theme, &labels, cx))
            .children(self.render_failure(&theme, &labels))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.render_surface(window, cx)),
            )
    }
}
