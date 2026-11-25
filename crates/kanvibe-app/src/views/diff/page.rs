//! 변경 파일 사이드바와 선택 파일 보기(TS `DiffPageClient`). 선택한 파일의 원본·현재 내용을 함께 읽어 비교·편집 모드로 보여 준다.
//! 내용 조회는 (task, 파일) 단위로 하나만 살아 있다. 새 조회를 시작하면 이전 조회를 버려 늦게 끝난 옛 응답이 화면을 덮지 않는다.
//! 같은 파일을 다시 읽는 동안에는 지난 내용과 열린 편집기를 그대로 두고, 그 재조회가 실패해도 지난 내용을 지킨다.

use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement, Render, Rgba,
    SharedString, StatefulInteractiveElement, Styled, Task, Window, div, prelude::FluentBuilder,
    px,
};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::task_diff_stats::{DiffFile, DiffFileStatus};

use super::file_editor::{DiffFileEditor, SaveHandler};
use super::file_tree::{
    FileRowState, FileTreeRow, FileTreeRowKind, all_folder_paths, change_colors, change_dot_split,
    file_row_children, file_row_text_color, folder_row_children, row_frame, visible_tree_rows,
};
use super::icons::{DiffIcon, diff_icon};
use super::source::SharedDiffSource;
use super::viewer::{DiffViewer, SideBySideModel};
use crate::components::{box_shadows, fade, focus_ring};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::text_baseline::{TextFamily, aligned_text, chromium_text};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::TailwindText;

pub const MIN_SIDEBAR_WIDTH: f32 = 180.0;
pub const MAX_SIDEBAR_WIDTH: f32 = 500.0;
pub const DEFAULT_SIDEBAR_WIDTH: f32 = 288.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Diff,
    Edit,
}

/// 지금 보여 주는 파일 내용. `task_id`·`file_path`가 지금 선택과 같아야 화면에 쓴다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileContentState {
    pub task_id: String,
    pub file_path: Option<String>,
    pub error: Option<String>,
    pub original: String,
    pub modified: String,
    pub is_loading: bool,
}

impl FileContentState {
    fn empty(task_id: &str, file_path: Option<String>, is_loading: bool) -> Self {
        Self {
            task_id: task_id.to_string(),
            file_path,
            error: None,
            original: String::new(),
            modified: String::new(),
            is_loading,
        }
    }
}

struct ResizeDrag {
    start_x: f32,
    start_width: f32,
}

pub struct DiffPage {
    task_id: String,
    locale: Locale,
    files: Vec<DiffFile>,
    source: SharedDiffSource,
    selected_file: Option<String>,
    view_mode: ViewMode,
    viewed_files: HashSet<String>,
    expanded_folders: HashSet<String>,
    content: FileContentState,
    viewer: Option<Entity<DiffViewer>>,
    editor: Option<Entity<DiffFileEditor>>,
    sidebar_width: f32,
    resize_drag: Option<ResizeDrag>,
    tree_focus: FocusHandle,
    /// 키보드로 고른 트리 줄
    keyboard_row: Option<usize>,
    _content_load: Option<Task<()>>,
}

impl DiffPage {
    pub fn new(
        task_id: &str,
        locale: Locale,
        files: Vec<DiffFile>,
        source: SharedDiffSource,
        cx: &mut Context<Self>,
    ) -> Self {
        let expanded_folders = all_folder_paths(&files);
        let mut page = Self {
            task_id: task_id.to_string(),
            locale,
            files,
            source,
            selected_file: None,
            view_mode: ViewMode::Diff,
            viewed_files: HashSet::new(),
            expanded_folders,
            content: FileContentState::empty(task_id, None, false),
            viewer: None,
            editor: None,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            resize_drag: None,
            tree_focus: cx.focus_handle(),
            keyboard_row: None,
            _content_load: None,
        };
        page.load_selected_content(cx);
        page
    }

    /// 선택한 파일이 목록에 없으면 첫 파일을 보여 준다(TS `effectiveSelectedFile`)
    pub fn effective_selected_file(&self) -> Option<String> {
        self.selected_file
            .clone()
            .filter(|selected| self.files.iter().any(|file| &file.path == selected))
            .or_else(|| self.files.first().map(|file| file.path.clone()))
    }

    /// 파일 목록이 새로 왔다. 같은 경로여도 내용을 다시 읽는다(TS effect가 `files`에 걸려 있다)
    pub fn set_files(&mut self, files: Vec<DiffFile>, cx: &mut Context<Self>) {
        self.files = files;
        self.load_selected_content(cx);
        cx.notify();
    }

    /// 트리에서 파일을 고르면 비교 모드로 돌아간다(TS `handleSelectFile`)
    pub fn select_file(&mut self, file_path: &str, cx: &mut Context<Self>) {
        self.selected_file = Some(file_path.to_string());
        self.set_view_mode(ViewMode::Diff, cx);
        self.load_selected_content(cx);
        cx.notify();
    }

    pub fn set_view_mode(&mut self, view_mode: ViewMode, cx: &mut Context<Self>) {
        self.view_mode = view_mode;
        if view_mode == ViewMode::Diff {
            self.editor = None;
        }
        cx.notify();
    }

    pub fn toggle_viewed(&mut self, file_path: &str, cx: &mut Context<Self>) {
        if !self.viewed_files.remove(file_path) {
            self.viewed_files.insert(file_path.to_string());
        }
        cx.notify();
    }

    pub fn toggle_folder(&mut self, folder_path: &str, cx: &mut Context<Self>) {
        if !self.expanded_folders.remove(folder_path) {
            self.expanded_folders.insert(folder_path.to_string());
        }
        cx.notify();
    }

    fn is_current_content(&self) -> bool {
        self.content.task_id == self.task_id
            && self.content.file_path == self.effective_selected_file()
    }

    fn selected_diff_file(&self) -> Option<&DiffFile> {
        let selected = self.effective_selected_file()?;
        self.files.iter().find(|file| file.path == selected)
    }
}

/// 내용 조회와 저장
impl DiffPage {
    fn load_selected_content(&mut self, cx: &mut Context<Self>) {
        let selection = self.effective_selected_file();
        if !self.is_current_content() {
            self.content =
                FileContentState::empty(&self.task_id, selection.clone(), selection.is_some());
            self.viewer = None;
            self.editor = None;
        }
        let Some(file_path) = selection else {
            self._content_load = None;
            return;
        };
        let original_path = self
            .files
            .iter()
            .find(|file| file.path == file_path)
            .and_then(|file| file.original_path.clone())
            .unwrap_or_else(|| file_path.clone());
        let original = self.source.read_original(&self.task_id, &original_path, cx);
        let modified = self.source.read_modified(&self.task_id, &file_path, cx);
        self._content_load = Some(cx.spawn(async move |this, cx| {
            let (original, modified) = futures::join!(original, modified);
            let result =
                original.and_then(|original| modified.map(|modified| (original, modified)));
            let _ = this.update(cx, |this, cx| {
                this.finish_content_load(file_path, result, cx)
            });
        }));
    }

    fn finish_content_load(
        &mut self,
        file_path: String,
        result: Result<(String, String), String>,
        cx: &mut Context<Self>,
    ) {
        let is_previous_intact =
            self.is_current_content() && !self.content.is_loading && self.content.error.is_none();
        match result {
            Ok((original, modified)) => {
                self.content = FileContentState {
                    task_id: self.task_id.clone(),
                    file_path: Some(file_path),
                    error: None,
                    original,
                    modified,
                    is_loading: false,
                };
                self.viewer = None;
            }
            Err(_) if is_previous_intact => {}
            Err(reason) => {
                self.content = FileContentState {
                    error: Some(reason),
                    ..FileContentState::empty(&self.task_id, Some(file_path), false)
                };
                self.viewer = None;
                self.editor = None;
            }
        }
        cx.notify();
    }

    /// 편집기 저장(TS `handleSave`). 보이는 내용이 아직 없거나 실패한 상태면 저장하지 않는다
    fn save_selected(
        &mut self,
        content: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let save_error = Translator::new(self.locale, "diffView").translate("saveError");
        let file_path = self.effective_selected_file();
        let Some(file_path) = file_path.filter(|_| {
            self.is_current_content() && !self.content.is_loading && self.content.error.is_none()
        }) else {
            return Task::ready(Err(save_error));
        };
        let save = self
            .source
            .save(&self.task_id, &file_path, content.clone(), cx);
        cx.spawn(async move |this, cx| {
            save.await?;
            let _ = this.update(cx, |this, cx| {
                if this.content.file_path.as_deref() == Some(file_path.as_str()) {
                    this.content.modified = content;
                    this.viewer = None;
                    cx.notify();
                }
            });
            Ok(())
        })
    }

    fn save_handler(&self, cx: &mut Context<Self>) -> SaveHandler {
        let page = cx.entity().downgrade();
        Rc::new(move |content: String, cx: &mut App| {
            page.update(cx, |page, cx| page.save_selected(content, cx))
                .unwrap_or_else(|_| Task::ready(Err(String::new())))
        })
    }

    /// 편집 모드이고 내용이 준비됐을 때만 편집기를 둔다. 한 번 만든 편집기는 비교 모드로 돌아가거나 파일이 바뀔 때까지 유지한다
    fn ensure_editor(&mut self, cx: &mut Context<Self>) -> Option<Entity<DiffFileEditor>> {
        if self.editor.is_none() {
            let (content, file_path) = (
                self.content.modified.clone(),
                self.content.file_path.clone()?,
            );
            let save_handler = self.save_handler(cx);
            let locale = self.locale;
            self.editor = Some(
                cx.new(|cx| DiffFileEditor::new(&content, &file_path, locale, save_handler, cx)),
            );
        }
        self.editor.clone()
    }

    fn ensure_viewer(&mut self, cx: &mut Context<Self>) -> Option<Entity<DiffViewer>> {
        if self.viewer.is_none() {
            let file_path = self.content.file_path.clone()?;
            let model =
                SideBySideModel::new(&self.content.original, &self.content.modified, &file_path);
            self.viewer = Some(cx.new(|cx| DiffViewer::new(model, cx)));
        }
        self.viewer.clone()
    }
}

/// 사이드바 폭 끌기(180~500)
impl DiffPage {
    fn begin_resize(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.resize_drag = Some(ResizeDrag {
            start_x: f32::from(event.position.x),
            start_width: self.sidebar_width,
        });
        cx.stop_propagation();
    }

    fn continue_resize(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = &self.resize_drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.resize_drag = None;
            return;
        }
        let next_width = drag.start_width + (f32::from(event.position.x) - drag.start_x);
        self.sidebar_width = next_width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        cx.notify();
    }
}

/// 트리 키보드 이동: 위·아래로 줄을 고르고 Enter·Space로 열거나 접는다. 왼쪽·오른쪽은 폴더를 접고 편다
impl DiffPage {
    fn handle_tree_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let rows = visible_tree_rows(&self.files, &self.expanded_folders);
        if rows.is_empty() {
            return;
        }
        let current = self.keyboard_row.unwrap_or(0).min(rows.len() - 1);
        let row = &rows[current];
        match event.keystroke.key.as_str() {
            "down" => self.keyboard_row = Some((current + 1).min(rows.len() - 1)),
            "up" => self.keyboard_row = Some(current.saturating_sub(1)),
            "enter" | "space" => self.activate_row(row, cx),
            "left" if row.kind == (FileTreeRowKind::Folder { is_expanded: true }) => {
                self.toggle_folder(&row.full_path, cx)
            }
            "right" if row.kind == (FileTreeRowKind::Folder { is_expanded: false }) => {
                self.toggle_folder(&row.full_path, cx)
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn activate_row(&mut self, row: &FileTreeRow, cx: &mut Context<Self>) {
        match row.kind {
            FileTreeRowKind::Folder { .. } => self.toggle_folder(&row.full_path, cx),
            FileTreeRowKind::File { .. } => self.select_file(&row.full_path, cx),
        }
    }
}

fn qa_target(role: &'static str, text: impl Into<SharedString>) -> impl IntoElement {
    let text = text.into();
    qa_target_layer(QaTargetDescription {
        role,
        name: text.clone(),
        text,
    })
}

/// 사이드바
impl DiffPage {
    fn render_sidebar(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let body: AnyElement = if self.files.is_empty() {
            div()
                .px(spacing(2.0))
                .py(spacing(4.0))
                .flex()
                .justify_center()
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .relative()
                .child(chromium_text(labels.translate("noChanges")))
                .child(qa_target("text", labels.translate("noChanges")))
                .into_any_element()
        } else {
            self.render_tree(theme, cx).into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .flex_none()
            .h_full()
            .w(px(self.sidebar_width))
            .bg(theme.background.surface)
            .border_r_1()
            .border_color(theme.border.default)
            .child(self.render_sidebar_header(theme, labels))
            .child(
                div()
                    .id("diff-tree-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(spacing(1.5))
                    .child(body),
            )
    }

    /// "N개 파일 변경 +a -d"(`text-[10px]`, 숫자는 `font-medium`)
    fn render_change_summary(&self, theme: &ThemeTokens, labels: &Translator) -> impl IntoElement {
        let additions: i64 = self.files.iter().map(|file| file.additions).sum();
        let deletions: i64 = self.files.iter().map(|file| file.deletions).sum();
        let count = self.files.len().to_string();
        let file_count = labels.translate_with("fileCount", &[("count", &count)]);
        let totals = (additions > 0 || deletions > 0).then(|| {
            let colored =
                |color: Rgba, text: String| div().text_color(color).child(chromium_text(text));
            div()
                .flex()
                .css_weight(500)
                .child(colored(change_colors::green_600(), format!("+{additions}")))
                .child(colored(change_colors::red_500(), format!(" -{deletions}")))
        });
        div()
            .flex()
            .items_center()
            .gap(spacing(2.0))
            .mt(spacing(0.5))
            .text_scale(text_scale::PX_10)
            .child(
                div()
                    .text_color(theme.text.muted)
                    .child(chromium_text(file_count)),
            )
            .children(totals)
    }

    fn render_sidebar_header(&self, theme: &ThemeTokens, labels: &Translator) -> impl IntoElement {
        let summary = self.render_change_summary(theme, labels);
        let viewed_counter = (!self.files.is_empty()).then(|| {
            let counter = format!("{} / {}", self.viewed_files.len(), self.files.len());
            div()
                .relative()
                .text_scale(text_scale::PX_10)
                .text_color(theme.text.muted)
                .whitespace_nowrap()
                .child(chromium_text(counter.clone()))
                .child(qa_target("text", counter))
        });
        div()
            .flex()
            .items_center()
            .justify_between()
            .px(spacing(3.0))
            .py(spacing(2.5))
            .border_b_1()
            .border_color(theme.border.default)
            .child(
                div()
                    .child(
                        div()
                            .text_scale(text_scale::XS)
                            .css_weight(600)
                            .text_color(theme.text.primary)
                            .child(chromium_text(labels.translate("changedFiles"))),
                    )
                    .child(summary),
            )
            .children(viewed_counter)
    }

    fn render_tree(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = visible_tree_rows(&self.files, &self.expanded_folders);
        let selected = self.effective_selected_file();
        let is_tree_focused = self.keyboard_row.is_some();
        let row_elements: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let is_keyboard_focused = is_tree_focused && self.keyboard_row == Some(index);
                self.render_tree_row(
                    theme,
                    index,
                    row,
                    selected.as_deref(),
                    is_keyboard_focused,
                    cx,
                )
            })
            .collect();
        div()
            .id("diff-file-tree")
            .track_focus(&self.tree_focus)
            .on_key_down(cx.listener(Self::handle_tree_key))
            .flex()
            .flex_col()
            .children(row_elements)
    }

    fn render_tree_row(
        &self,
        theme: &ThemeTokens,
        index: usize,
        row: &FileTreeRow,
        selected: Option<&str>,
        is_keyboard_focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let path = row.full_path.clone();
        let frame = row_frame(row).id(("diff-tree-row", index));
        let hover_background = theme.background.page;
        let frame = match row.kind {
            FileTreeRowKind::Folder { is_expanded } => frame
                .text_color(theme.text.primary)
                .hover(move |style| style.bg(hover_background))
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.tree_focus, cx);
                    this.toggle_folder(&path, cx);
                }))
                .children(folder_row_children(theme, row, is_expanded)),
            FileTreeRowKind::File { .. } => {
                let state = FileRowState {
                    is_selected: selected == Some(row.full_path.as_str()),
                    is_viewed: self.viewed_files.contains(&row.full_path),
                };
                style_file_row(frame, theme, &state)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&this.tree_focus, cx);
                        this.select_file(&path, cx);
                    }))
                    .children(file_row_children(theme, row, &state))
            }
        };
        let frame = if is_keyboard_focused {
            frame.shadow(focus_ring(theme))
        } else {
            frame
        };
        frame
            .child(qa_target("button", row.name.clone()))
            .into_any_element()
    }
}

/// 파일 줄 색: 선택은 `bg-brand-subtle`, 나머지는 호버하면 `bg-bg-page`와 한 단계 진한 글자
fn style_file_row(
    frame: gpui_kit::Stateful<gpui_kit::Div>,
    theme: &ThemeTokens,
    state: &FileRowState,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let frame = frame.text_color(file_row_text_color(theme, state));
    if state.is_selected {
        return frame.bg(theme.brand.subtle);
    }
    let hover_background = theme.background.page;
    let hover_text = if state.is_viewed {
        theme.text.secondary
    } else {
        theme.text.primary
    };
    frame.hover(move |style| style.bg(hover_background).text_color(hover_text))
}

/// 본문: 모드 전환·경로·변경량·확인 줄과 비교/편집 영역
impl DiffPage {
    fn render_main(
        &mut self,
        theme: &ThemeTokens,
        labels: &Translator,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(selected) = self.selected_diff_file().cloned() else {
            return render_empty_main(theme, labels, !self.files.is_empty()).into_any_element();
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .min_h_0()
            .min_w_0()
            .child(self.render_toolbar(theme, labels, &selected, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_content(theme, cx)),
            )
            .into_any_element()
    }

    fn render_toolbar(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        selected: &DiffFile,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing(3.0))
            .px(spacing(4.0))
            .py(spacing(2.0))
            .bg(theme.background.surface)
            .border_b_1()
            .border_color(theme.border.default)
            .child(self.render_mode_toggle(
                theme,
                labels,
                selected.status == DiffFileStatus::Deleted,
                cx,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_scale(text_scale::XS)
                    .mono(400)
                    .text_color(theme.text.secondary)
                    .child(aligned_text(
                        selected.path.clone(),
                        text_scale::XS,
                        TextFamily::Mono,
                    )),
            )
            .child(render_selected_stats(selected))
            .child(self.render_viewed_checkbox(theme, labels, &selected.path, cx))
    }

    fn render_mode_toggle(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        is_deleted: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut modes = vec![(ViewMode::Diff, labels.translate("diffMode"))];
        if !is_deleted {
            modes.push((ViewMode::Edit, labels.translate("editMode")));
        }
        let buttons = modes.into_iter().map(|(mode, label)| {
            let is_active = self.view_mode == mode;
            let button = div()
                .id(SharedString::from(format!("diff-mode-{label}")))
                .relative()
                .px(spacing(2.5))
                .py(spacing(1.0))
                .text_scale(text_scale::XS)
                .rounded(radius::BASE)
                .on_click(cx.listener(move |this, _, _, cx| this.set_view_mode(mode, cx)))
                .child(chromium_text(label.clone()))
                .child(qa_target("button", label));
            if is_active {
                button
                    .bg(theme.background.surface)
                    .text_color(theme.text.primary)
                    .css_weight(500)
                    .shadow(box_shadows(&theme.shadow.xs))
            } else {
                let hover_text = theme.text.secondary;
                button
                    .text_color(theme.text.muted)
                    .hover(move |style| style.text_color(hover_text))
            }
        });
        div()
            .flex()
            .flex_none()
            .bg(theme.background.page)
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .p(spacing(0.5))
            .children(buttons)
    }

    fn render_viewed_checkbox(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        file_path: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_viewed = self.viewed_files.contains(file_path);
        let label = labels.translate(if is_viewed { "viewed" } else { "markViewed" });
        let path = file_path.to_string();
        div()
            .id("diff-viewed-toggle")
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing(1.5))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_viewed(&path, cx)))
            .child(render_checkbox(theme, is_viewed))
            .child(
                div()
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.muted)
                    .child(chromium_text(label.clone())),
            )
            .child(qa_target("checkbox", label))
    }

    fn render_content(&mut self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        if !self.is_current_content() || self.content.is_loading {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.background.page)
                .child(
                    div()
                        .text_scale(text_scale::SM)
                        .text_color(theme.text.muted)
                        .child(chromium_text("Loading...")),
                )
                .into_any_element();
        }
        if let Some(error) = self.content.error.clone() {
            return div()
                .relative()
                .p(spacing(4.0))
                .text_scale(text_scale::SM)
                .text_color(theme.text.muted)
                .child(aligned_text(
                    error.clone(),
                    text_scale::SM,
                    TextFamily::Sans,
                ))
                .child(qa_target("alert", error))
                .into_any_element();
        }
        match self.view_mode {
            ViewMode::Diff => self
                .ensure_viewer(cx)
                .map(|viewer| viewer.into_any_element()),
            ViewMode::Edit => self
                .ensure_editor(cx)
                .map(|editor| editor.into_any_element()),
        }
        .unwrap_or_else(|| div().into_any_element())
    }
}

/// `+N -N ●●●●●`(`text-xs font-mono`, 숫자는 `font-semibold`)
fn render_selected_stats(selected: &DiffFile) -> impl IntoElement {
    let mut stats = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing(1.5))
        .text_scale(text_scale::XS)
        .mono(600)
        .child(
            div()
                .text_color(change_colors::green_600())
                .child(chromium_text(format!("+{}", selected.additions))),
        )
        .child(
            div()
                .text_color(change_colors::red_500())
                .child(chromium_text(format!("-{}", selected.deletions))),
        );
    if let Some((addition_dots, deletion_dots)) =
        change_dot_split(selected.additions, selected.deletions)
    {
        let dot = |color: Rgba| div().size(px(8.0)).rounded(radius::SM).bg(color);
        let dots = (0..addition_dots)
            .map(|_| dot(change_colors::green_500()))
            .chain((0..deletion_dots).map(|_| dot(change_colors::red_500())));
        stats = stats.child(
            div()
                .flex()
                .gap(spacing(0.5))
                .ml(spacing(1.0))
                .children(dots),
        );
    }
    stats
}

/// Chromium 다크 `color-scheme`의 기본 체크박스(14px, 회색 테두리). 체크하면 brand 색 바탕에 흰 체크
fn render_checkbox(theme: &ThemeTokens, is_checked: bool) -> impl IntoElement {
    let checkbox = div().size(px(14.0)).flex_none().rounded(px(2.0)).border_1();
    if is_checked {
        checkbox
            .bg(theme.brand.primary)
            .border_color(theme.brand.primary)
            .child(diff_icon_check())
    } else {
        checkbox
            .bg(gpui_kit::rgb(0x3B3B3B))
            .border_color(gpui_kit::rgb(0x858585))
    }
}

fn diff_icon_check() -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(diff_icon(DiffIcon::Viewed, gpui_kit::rgb(0xFFFFFF)).size(px(12.0)))
}

/// 선택할 파일이 없을 때: 가운데 문서 아이콘과 안내
fn render_empty_main(
    theme: &ThemeTokens,
    labels: &Translator,
    has_files: bool,
) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(spacing(3.0))
        .bg(theme.background.page)
        .child(diff_icon(
            DiffIcon::EmptyDocument,
            fade(theme.text.muted, 0.3),
        ))
        .child({
            let message = labels.translate(if has_files {
                "changedFiles"
            } else {
                "noChanges"
            });
            div()
                .relative()
                .text_scale(text_scale::SM)
                .text_color(theme.text.muted)
                .child(chromium_text(message.clone()))
                .child(qa_target("text", message))
        })
}

impl Render for DiffPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let labels = Translator::new(self.locale, "diffView");
        let resize_hover = fade(theme.brand.primary, 0.3);
        let resize_active = fade(theme.brand.primary, 0.5);
        let is_resizing = self.resize_drag.is_some();
        let handle = div()
            .id("diff-sidebar-resize")
            .w(px(4.0))
            .h_full()
            .flex_none()
            .cursor_col_resize()
            .when(is_resizing, |handle| handle.bg(resize_active))
            .hover(move |style| style.bg(resize_hover))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::begin_resize));
        div()
            .id("diff-page")
            .flex()
            .flex_1()
            .min_h_0()
            .on_mouse_move(cx.listener(Self::continue_resize))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.resize_drag.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(self.render_sidebar(&theme, &labels, cx))
            .child(handle)
            .child(self.render_main(&theme, &labels, cx))
    }
}
