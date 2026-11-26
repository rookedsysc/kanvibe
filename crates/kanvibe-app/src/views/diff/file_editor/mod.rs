//! 편집 모드(TS `DiffFileEditor`): 파일 경로·저장하지 않은 변경 표시·저장 버튼 줄과 Monaco 단일 편집기.
//! `Mod+S`와 저장 버튼은 같은 저장 경로를 쓰고, 저장 중에는 어느 쪽으로도 다시 보내지 않는다.
//! 저장 실패 사유는 다시 저장할 때까지 남긴다. 저장 중 더 편집했으면 저장이 끝나도 미저장 표시를 지우지 않는다.

mod find;
mod keyboard;
mod mouse;

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{
    App, Bounds, Context, ElementInputHandler, Entity, EntityInputHandler, FocusHandle,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Render, Rgba, ScrollEvent,
    StatefulInteractiveElement, Styled, Task, UTF16Selection, Window, canvas, div, point,
    prelude::FluentBuilder, px, size,
};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::platform::ShortcutPlatform;

use super::find_session::FindSession;
use super::folding::{FoldRange, FoldState, compute_fold_ranges, innermost_range_at};
use super::line_diff::TextPoint;
use super::monaco::{
    CHARACTER_WIDTH, GutterLayout, LINE_HEIGHT, MonacoPane, PaneDocument, PaneRow,
    active_indent_guide, hit_test, paint_pane,
};
use super::syntax::{
    Language, LineEndLexState, PreviousTokenization, detect_language, matching_bracket_pair,
    retokenize_lines, tokenize_lines,
};
use super::text_buffer::TextBuffer;
use crate::components::fade;
use crate::qa::targets::{QaTargetDescription, QaTargetRegistry, qa_target_layer};
use crate::services::AppServices;
use crate::text_baseline::{TextFamily, aligned_text, chromium_text};
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
    /// `document` 줄마다 줄 끝 어휘 상태. 편집 뒤 바뀐 줄부터만 다시 토큰화하는 데 쓴다
    line_end_states: Vec<LineEndLexState>,
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
    platform: ShortcutPlatform,
    find: Option<FindSession>,
    fold_ranges: Rc<Vec<FoldRange>>,
    fold_state: FoldState,
    /// 접힌 줄을 뺀, 화면에 그리는 줄 번호들
    visible_lines: Rc<Vec<usize>>,
    /// 줄 번호 칸 위에 포인터가 있으면 접기 단추를 보인다(Monaco `showFoldingControls: mouseover`)
    is_gutter_hovered: bool,
    /// 자동 닫기로 넣은 닫는 글자 자리. 그 글자 위를 칠 때 지나가고 빈 쌍은 Backspace로 함께 지운다
    auto_closed: Vec<TextPoint>,
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
        let lines = buffer.lines().to_vec();
        let tokenized = tokenize_lines(&lines, language);
        let document = Rc::new(PaneDocument::new(lines, tokenized.tokens));
        let fold_ranges = compute_fold_ranges(buffer.lines(), document.indent_size, language);
        let visible_lines = (0..buffer.lines().len()).collect();
        let platform = cx.global::<AppServices>().shortcut_platform;
        Self {
            file_path: file_path.to_string(),
            locale,
            language,
            buffer,
            document,
            line_end_states: tokenized.line_end_states,
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
            platform,
            find: None,
            fold_ranges: Rc::new(fold_ranges),
            fold_state: FoldState::default(),
            visible_lines: Rc::new(visible_lines),
            is_gutter_hovered: false,
            auto_closed: Vec::new(),
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
            self.rebuild_document(cx);
        }
        self.reveal_cursor_line();
        if self._caret_blink.is_some() {
            self.restart_caret_blink(cx);
        }
        self.scroll_caret_into_view();
        cx.notify();
    }

    /// 내용이 바뀌었다: 강조·접기 범위·찾기 결과를 새 내용으로 다시 만든다
    fn rebuild_document(&mut self, cx: &mut Context<Self>) {
        let lines = self.buffer.lines().to_vec();
        let previous = PreviousTokenization {
            lines: &self.document.lines,
            tokens: &self.document.tokens,
            line_end_states: &self.line_end_states,
        };
        let tokenized = retokenize_lines(&previous, &lines, self.language);
        self.line_end_states = tokenized.line_end_states;
        self.document = Rc::new(PaneDocument::new(lines, tokenized.tokens));
        self.document_revision = self.buffer.revision();
        let ranges = compute_fold_ranges(
            self.buffer.lines(),
            self.document.indent_size,
            self.language,
        );
        self.fold_state.retain_valid(&ranges);
        self.fold_ranges = Rc::new(ranges);
        self.refresh_visible_lines();
        self.recompute_find(cx);
    }

    fn refresh_visible_lines(&mut self) {
        self.visible_lines = Rc::new(
            self.fold_state
                .visible_lines(&self.fold_ranges, self.buffer.lines().len()),
        );
    }

    /// 커서가 접힌 범위 안으로 들어가면 그 범위를 편다(Monaco도 숨은 줄에 커서를 두지 않는다)
    fn reveal_cursor_line(&mut self) {
        let line = self.buffer.cursor().line;
        let mut has_changed = false;
        while let Some(range) = self.fold_state.hidden_by(&self.fold_ranges, line) {
            self.fold_state.expand(range.start);
            has_changed = true;
        }
        if has_changed {
            self.refresh_visible_lines();
        }
    }

    /// Ctrl+Shift+[ / ]: 커서 줄을 감싼 가장 안쪽 범위를 접거나 편다
    fn fold_at_cursor(&mut self, should_fold: bool) {
        let line = self.buffer.cursor().line;
        let target = if should_fold {
            innermost_range_at(&self.fold_ranges, line)
                .filter(|range| !self.fold_state.is_collapsed(range.start))
        } else {
            self.fold_ranges
                .iter()
                .filter(|range| {
                    range.start <= line
                        && line <= range.end
                        && self.fold_state.is_collapsed(range.start)
                })
                .max_by_key(|range| range.start)
                .copied()
        };
        let Some(range) = target else {
            return;
        };
        if should_fold {
            self.fold_state.collapse(range.start);
            self.buffer.set_cursor(
                TextPoint {
                    line: range.start,
                    column: self.buffer.lines()[range.start].len(),
                },
                false,
            );
        } else {
            self.fold_state.expand(range.start);
        }
        self.refresh_visible_lines();
    }

    fn cursor_row(&self) -> usize {
        let line = self.buffer.cursor().line;
        self.visible_lines
            .iter()
            .position(|visible| *visible == line)
            .unwrap_or(0)
    }

    /// Monaco처럼 포커스가 있고 창이 앞에 있을 때만 깜박인다. 둘이 바뀌면 GPUI가 창을 다시 그리므로 그릴 때 맞춘다
    fn sync_caret_blink(&mut self, window: &Window, cx: &mut Context<Self>) {
        let should_blink = self.focus_handle.is_focused(window) && window.is_window_active();
        if !should_blink {
            self._caret_blink = None;
            self.is_caret_visible = true;
        } else if self._caret_blink.is_none() {
            self.restart_caret_blink(cx);
        }
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
        let caret_top = self.cursor_row() as f32 * LINE_HEIGHT;
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

    /// 커서를 둘러싼 괄호 쌍(Monaco `matchBrackets`)
    fn bracket_boxes(&self, caret: TextPoint) -> Vec<TextPoint> {
        let Some((open, close)) =
            matching_bracket_pair(&self.document.tokens, (caret.line, caret.column))
        else {
            return Vec::new();
        };
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
    }

    fn fold_markers(&self) -> Vec<(usize, bool)> {
        self.fold_ranges
            .iter()
            .map(|range| (range.start, self.fold_state.is_collapsed(range.start)))
            .collect()
    }

    fn pane(&self, window: &Window) -> MonacoPane {
        let caret = self.buffer.cursor();
        let is_focused = self.focus_handle.is_focused(window) && window.is_window_active();
        let bracket_boxes = self.bracket_boxes(caret);
        MonacoPane {
            document: Rc::clone(&self.document),
            rows: Rc::new(
                self.visible_lines
                    .iter()
                    .map(|line| PaneRow::Text {
                        line: *line,
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
            find_highlights: Rc::new(
                self.find
                    .as_ref()
                    .map(|session| session.matches.clone())
                    .unwrap_or_default(),
            ),
            current_find_match: self
                .find
                .as_ref()
                .and_then(FindSession::current_match)
                .cloned(),
            fold_markers: Rc::new(self.fold_markers()),
            are_fold_controls_visible: self.is_gutter_hovered,
        }
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
        let (text, document_range) = self.buffer.text_for_utf16_range(range);
        *adjusted_range = Some(document_range);
        Some(text)
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
        let mut characters = text.chars();
        let single_character = characters.next().filter(|_| characters.next().is_none());
        if let (None, None, Some(typed)) = (&range, &self.marked_range, single_character) {
            self.type_character(typed, cx);
            return;
        }
        if let Some(range) = range.or_else(|| self.marked_range.clone()) {
            self.buffer.select_utf16_range(range);
        }
        self.marked_range = None;
        self.buffer.insert_text(text);
        self.auto_closed.clear();
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
        let y =
            element_bounds.origin.y + px(self.cursor_row() as f32 * LINE_HEIGHT - self.scroll_y);
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
            .key_context(crate::shortcuts::DIFF_FILE_EDITOR_KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::handle_mouse_down))
            .on_mouse_move(cx.listener(Self::handle_mouse_move))
            .on_scroll(cx.listener(|this, event: &ScrollEvent, window, cx| {
                this.handle_scroll(event, window, cx)
            }))
            .relative()
            .child(surface)
            .children(
                cx.has_global::<QaTargetRegistry>()
                    .then(|| self.render_line_targets()),
            )
            .children(self.find.as_ref().map(|session| session.widget.clone()))
    }

    /// 재생 드라이버가 코드 줄을 글자로 찾아 누를 수 있게 보이는 줄의 위치를 남긴다. 기록할 QA 레지스트리가 있을 때만 부른다
    fn render_line_targets(&self) -> impl IntoElement {
        let viewport_height = self
            .surface_bounds
            .get()
            .map_or(0.0, |bounds| f32::from(bounds.size.height));
        let first_line = (self.scroll_y / LINE_HEIGHT).floor() as usize;
        let line_count = (viewport_height / LINE_HEIGHT).ceil() as usize + 1;
        let content_left = self.pane_gutter_width();
        let targets = self
            .visible_lines
            .iter()
            .enumerate()
            .skip(first_line)
            .take(line_count)
            .map(|(row, line)| {
                let text = &self.document.lines[*line];
                div()
                    .absolute()
                    .top(px(row as f32 * LINE_HEIGHT - self.scroll_y))
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
        self.sync_caret_blink(window, cx);
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

#[cfg(test)]
mod caret_tests {
    use gpui_kit::{TestAppContext, gpui};

    use super::*;
    use crate::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions};

    /// Monaco는 편집기가 포커스를 잃으면(창이 비활성이면 함께) 커서를 그리지 않는다
    #[gpui::test]
    fn the_caret_is_drawn_only_while_the_focused_editor_window_is_active(cx: &mut TestAppContext) {
        let launch = LaunchOptions {
            data_dir: None,
            initial_route: "/ko".into(),
            window_size: DEFAULT_WINDOW_SIZE,
            qa: None,
        };
        cx.update(|cx| crate::init_app(&launch, cx).expect("app init"));
        let save_handler: SaveHandler = Rc::new(|_, _| Task::ready(Ok(())));
        let (editor, cx) = cx.add_window_view(move |_, cx| {
            DiffFileEditor::new("text", "a.ts", Locale::Ko, save_handler, cx)
        });
        let has_caret = |cx: &mut gpui_kit::VisualTestContext| {
            editor.update_in(cx, |editor, window, _| editor.pane(window).caret.is_some())
        };

        assert!(!has_caret(cx));
        let focus_handle = cx.update(|_, cx| editor.read(cx).focus_handle.clone());
        cx.update(|window, cx| {
            window.activate_window();
            focus_handle.focus(window, cx);
        });
        cx.run_until_parked();
        assert!(has_caret(cx));
        cx.deactivate_window();
        assert!(!has_caret(cx));
    }

    /// 포커스를 잃은 편집기는 깜박임 타이머를 멈춰 0.5초마다 다시 그리지 않는다
    #[gpui::test]
    fn the_caret_stops_blinking_once_the_editor_window_is_inactive(cx: &mut TestAppContext) {
        let launch = LaunchOptions {
            data_dir: None,
            initial_route: "/ko".into(),
            window_size: DEFAULT_WINDOW_SIZE,
            qa: None,
        };
        cx.update(|cx| crate::init_app(&launch, cx).expect("app init"));
        let save_handler: SaveHandler = Rc::new(|_, _| Task::ready(Ok(())));
        let (editor, cx) = cx.add_window_view(move |_, cx| {
            DiffFileEditor::new("text", "a.ts", Locale::Ko, save_handler, cx)
        });
        let caret_states = |cx: &mut gpui_kit::VisualTestContext| {
            let mut states = Vec::new();
            for _ in 0..3 {
                cx.executor().advance_clock(CARET_BLINK_INTERVAL);
                cx.run_until_parked();
                states.push(editor.read_with(cx, |editor, _| editor.is_caret_visible));
            }
            states
        };

        let focus_handle = cx.update(|_, cx| editor.read(cx).focus_handle.clone());
        cx.update(|window, cx| {
            window.activate_window();
            focus_handle.focus(window, cx);
        });
        cx.run_until_parked();
        assert_eq!(caret_states(cx), [false, true, false]);
        cx.deactivate_window();
        assert_eq!(caret_states(cx), [true, true, true]);
        assert!(editor.read_with(cx, |editor, _| editor._caret_blink.is_none()));
    }
}
