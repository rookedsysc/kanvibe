//! 대화 기록의 파일 미리보기(TS `AiChatFilePreview`).
//!
//! 파일마다 상태를 새로 만들어, 새 파일을 읽기 전에 이전 파일 내용이 새 파일 모양으로 그려지지 않게 한다.
//! Markdown·TeX는 그린 모양과 원문을 오가고, 나머지는 원문만 보인다. 복사는 보이는 모양과 상관없이 파일 원문 전체다.

use gpui_kit::{
    AnyElement, ClipboardItem, Context, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Task, div, px,
};
use kanvibe_ai::types::AiMessageRole;
use kanvibe_services::diff::TaskWorkspaceFileFailure;

use super::ai_reader::read_workspace_file;
use super::chat_view::InlineAiChatView;
use super::message_content::render_message;
use crate::components::{TailwindText, box_shadows};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    Markdown,
    Latex,
    /// HTML·SVG. GPUI에는 격리된 문서 틀이 없어 원문만 보인다
    Document,
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewMode {
    Rendered,
    Source,
}

pub struct FilePreviewState {
    pub path: String,
    pub content: Option<String>,
    pub failure: Option<TaskWorkspaceFileFailure>,
    pub mode: PreviewMode,
    pub is_copied: bool,
    _read: Option<Task<()>>,
}

fn file_extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((_, extension)) => extension.to_lowercase(),
        None => String::new(),
    }
}

pub fn preview_kind(path: &str) -> PreviewKind {
    match file_extension(path).as_str() {
        "tex" | "latex" => PreviewKind::Latex,
        "md" | "markdown" | "mdx" => PreviewKind::Markdown,
        "html" | "htm" | "svg" => PreviewKind::Document,
        _ => PreviewKind::Source,
    }
}

/// 보기 전환은 그려 볼 수 있는 종류에만 둔다
pub fn has_view_toggle(kind: PreviewKind) -> bool {
    matches!(kind, PreviewKind::Markdown | PreviewKind::Latex)
}

/// TeX 문서를 수식 블록만 남긴 Markdown으로 바꾼다(주석·문서 머리 제거, 수식 환경은 `$$`)
pub fn latex_preview_markdown(content: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut environment: Option<(String, Vec<String>)> = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('%')
            || trimmed.starts_with("\\documentclass")
            || trimmed.starts_with("\\usepackage")
        {
            continue;
        }
        if trimmed == "\\begin{document}" || trimmed == "\\end{document}" {
            continue;
        }
        if let Some((name, body)) = environment.as_mut() {
            if trimmed == format!("\\end{{{name}}}") {
                let body = body.join("\n");
                lines.push(match name.starts_with("align") {
                    true => format!("$$\\begin{{aligned}}{body}\\end{{aligned}}$$"),
                    false => format!("$${body}$$"),
                });
                environment = None;
            } else {
                body.push(line.to_string());
            }
            continue;
        }
        let math_environment = [
            "equation",
            "equation*",
            "align",
            "align*",
            "gather",
            "gather*",
        ]
        .into_iter()
        .find(|name| trimmed == format!("\\begin{{{name}}}"));
        match math_environment {
            Some(name) => environment = Some((name.to_string(), Vec::new())),
            None => lines.push(line.to_string()),
        }
    }
    lines.join("\n")
}

pub fn failure_key(failure: TaskWorkspaceFileFailure) -> &'static str {
    match failure {
        TaskWorkspaceFileFailure::OutsideWorktree => {
            "aiSessions.filePreview.failure.outside-worktree"
        }
        TaskWorkspaceFileFailure::NotFound => "aiSessions.filePreview.failure.not-found",
        TaskWorkspaceFileFailure::TooLarge => "aiSessions.filePreview.failure.too-large",
        TaskWorkspaceFileFailure::Unreadable => "aiSessions.filePreview.failure.unreadable",
    }
}

impl FilePreviewState {
    pub fn new(path: String) -> Self {
        Self {
            path,
            content: None,
            failure: None,
            mode: PreviewMode::Rendered,
            is_copied: false,
            _read: None,
        }
    }

    pub fn finish(&mut self, result: Result<String, TaskWorkspaceFileFailure>) {
        match result {
            Ok(content) => self.content = Some(content),
            Err(failure) => self.failure = Some(failure),
        }
    }

    /// 그린 모양을 보여 줄지. 원문으로 바꿨거나 그릴 수 없는 종류면 원문이다
    pub fn shows_rendered(&self) -> bool {
        has_view_toggle(preview_kind(&self.path)) && self.mode == PreviewMode::Rendered
    }
}

impl InlineAiChatView {
    pub(super) fn open_file_preview(&mut self, path: String, cx: &mut Context<Self>) {
        self.state.preview_file = Some(path.clone());
        self.file_preview = Some(FilePreviewState::new(path));
        self.read_preview_file(cx);
        cx.notify();
    }

    fn read_preview_file(&mut self, cx: &mut Context<Self>) {
        let Some(preview) = self.file_preview.as_mut() else {
            return;
        };
        preview.content = None;
        preview.failure = None;
        let path = preview.path.clone();
        let read = read_workspace_file(&self.task_id, &path, cx);
        preview._read = Some(cx.spawn(async move |this, cx| {
            let result = read.await;
            let _ = this.update(cx, |view, cx| {
                if let Some(preview) = view
                    .file_preview
                    .as_mut()
                    .filter(|preview| preview.path == path)
                {
                    preview.finish(result);
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn close_file_preview(&mut self, cx: &mut Context<Self>) {
        self.state.preview_file = None;
        self.file_preview = None;
        cx.notify();
    }

    pub(super) fn render_file_preview(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let preview = self
            .file_preview
            .as_ref()
            .filter(|preview| self.state.preview_file.as_deref() == Some(preview.path.as_str()))?;
        let header = self.render_preview_header(theme, preview, cx);
        let panel = div()
            .id("ai-chat-file-preview")
            .occlude()
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .shadow(box_shadows(&theme.shadow.lg))
            .child(header)
            .child(self.render_preview_body(theme, preview, cx));
        Some(panel.into_any_element())
    }

    /// 파일 이름·경로, 보기 전환, 복사, 닫기
    fn render_preview_header(
        &self,
        theme: &ThemeTokens,
        preview: &FilePreviewState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let name = preview
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&preview.path)
            .to_string();
        let title = div()
            .min_w_0()
            .flex_1()
            .child(
                div()
                    .truncate()
                    .text_scale(text_scale::SM)
                    .css_weight(500)
                    .text_color(theme.text.primary)
                    .child(name),
            )
            .child(
                div()
                    .truncate()
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.secondary)
                    .child(preview.path.clone()),
            );
        let close_label = self.translator.translate("aiSessions.filePreview.close");
        div()
            .flex()
            .min_h(px(48.0))
            .items_center()
            .gap(spacing(2.0))
            .border_b_1()
            .border_color(theme.border.subtle)
            .px(spacing(3.0))
            .child(title)
            .children(
                has_view_toggle(preview_kind(&preview.path))
                    .then(|| self.render_view_toggle(theme, preview.mode, cx)),
            )
            .child(self.render_copy_button(theme, preview, cx))
            .child(
                preview_button(theme, "file-preview-close", "✕".into(), close_label)
                    .on_click(cx.listener(|view, _, _, cx| view.close_file_preview(cx))),
            )
    }

    fn render_view_toggle(
        &self,
        theme: &ThemeTokens,
        mode: PreviewMode,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let options = [
            (PreviewMode::Rendered, "aiSessions.filePreview.rendered"),
            (PreviewMode::Source, "aiSessions.filePreview.source"),
        ];
        div()
            .flex()
            .flex_none()
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .children(options.into_iter().map(|(option, key)| {
                let label = self.translator.translate(key);
                let button = preview_button(
                    theme,
                    SharedString::from(format!("file-preview-{key}")),
                    label.clone(),
                    label,
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                    if let Some(preview) = view.file_preview.as_mut() {
                        preview.mode = option;
                        cx.notify();
                    }
                }));
                match option == mode {
                    true => button
                        .bg(theme.background.page)
                        .text_color(theme.text.primary),
                    false => button,
                }
            }))
            .into_any_element()
    }

    fn render_copy_button(
        &self,
        theme: &ThemeTokens,
        preview: &FilePreviewState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = if preview.is_copied {
            "aiSessions.filePreview.copied"
        } else {
            "aiSessions.filePreview.copy"
        };
        let label = self.translator.translate(key);
        let button = preview_button(theme, "file-preview-copy", label.clone(), label);
        let Some(content) = preview.content.clone() else {
            return button.opacity(0.4).into_any_element();
        };
        button
            .on_click(cx.listener(move |view, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(content.clone()));
                if let Some(preview) = view.file_preview.as_mut() {
                    preview.is_copied = true;
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn render_preview_body(
        &self,
        theme: &ThemeTokens,
        preview: &FilePreviewState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let body = div()
            .id("ai-chat-file-preview-body")
            .min_h_0()
            .flex_1()
            .overflow_y_scroll();
        if let Some(failure) = preview.failure {
            return body
                .child(self.render_preview_failure(theme, failure, cx))
                .into_any_element();
        }
        let Some(content) = preview.content.as_deref() else {
            return body
                .child(preview_status(
                    self.translator.translate("aiSessions.filePreview.loading"),
                    theme.text.secondary,
                ))
                .into_any_element();
        };
        if preview.shows_rendered() {
            let markdown = match preview_kind(&preview.path) {
                PreviewKind::Latex => latex_preview_markdown(content),
                _ => content.to_string(),
            };
            let rendered = render_message(&markdown, AiMessageRole::Assistant, theme, cx);
            return body
                .child(
                    div()
                        .p(spacing(4.0))
                        .text_color(theme.text.primary)
                        .child(rendered),
                )
                .into_any_element();
        }
        body.bg(theme.background.page)
            .child(
                div()
                    .p(spacing(4.0))
                    .text_scale(text_scale::XS)
                    .line_height(px(20.0))
                    .font(crate::fonts::mono_font(400))
                    .text_color(theme.text.primary)
                    .whitespace_nowrap()
                    .child(content.to_string()),
            )
            .into_any_element()
    }

    /// 읽지 못한 사유와 다시 시도
    fn render_preview_failure(
        &self,
        theme: &ThemeTokens,
        failure: TaskWorkspaceFileFailure,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let retry_label = self.translator.translate("aiSessions.filePreview.retry");
        let retry = preview_button(
            theme,
            "file-preview-retry",
            retry_label.clone(),
            retry_label,
        )
        .on_click(cx.listener(|view, _, _, cx| view.read_preview_file(cx)));
        preview_status(
            self.translator.translate(failure_key(failure)),
            theme.status.error,
        )
        .child(div().mt(spacing(3.0)).flex().child(retry))
    }
}

/// 불러오는 중·실패 안내(`p-4 text-sm`)
fn preview_status(text: String, color: gpui_kit::Rgba) -> gpui_kit::Div {
    div()
        .p(spacing(4.0))
        .text_scale(text_scale::SM)
        .text_color(color)
        .child(text)
}

/// 미리보기 머리줄 버튼(`min-h-8 rounded-md px-2 text-xs text-text-secondary`)
fn preview_button(
    theme: &ThemeTokens,
    id: impl Into<gpui_kit::ElementId>,
    label: String,
    accessible_name: String,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let hover_background = theme.background.page;
    let hover_text = theme.text.primary;
    div()
        .id(id)
        .relative()
        .flex()
        .flex_none()
        .min_h(px(32.0))
        .items_center()
        .rounded(radius::MD)
        .px(spacing(2.0))
        .text_scale(text_scale::XS)
        .text_color(theme.text.secondary)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background).text_color(hover_text))
        .child(label.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: accessible_name.into(),
            text: label.into(),
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_domain::i18n::{Locale, Translator};

    // parity: TS-renderer-AiChatFilePreview-2da909
    #[test]
    fn markdown_files_render_and_switch_to_their_source() {
        let mut preview = FilePreviewState::new("/repo/docs/plan.md".into());
        preview.finish(Ok("# 계획".into()));
        assert!(preview.shows_rendered());
        preview.mode = PreviewMode::Source;
        assert!(!preview.shows_rendered());
        assert_eq!(preview.content.as_deref(), Some("# 계획"));
    }

    // parity: TS-renderer-AiChatFilePreview-2aa06f
    #[test]
    fn copy_takes_the_whole_original_file_whatever_the_view() {
        let mut preview = FilePreviewState::new("/repo/docs/plan.md".into());
        preview.finish(Ok("# 계획\n\n- 하나".into()));
        assert!(preview.shows_rendered());
        assert_eq!(preview.content.as_deref(), Some("# 계획\n\n- 하나"));
    }

    // parity: TS-renderer-AiChatFilePreview-0d05d8
    #[test]
    fn files_that_cannot_render_show_only_their_source() {
        assert_eq!(preview_kind("/repo/src/filters.ts"), PreviewKind::Source);
        assert!(!has_view_toggle(preview_kind("/repo/src/filters.ts")));
        assert!(!FilePreviewState::new("/repo/Makefile".into()).shows_rendered());
        assert!(has_view_toggle(preview_kind("/repo/README.MD")));
    }

    // parity: TS-renderer-AiChatFilePreview-3c07ad
    #[test]
    fn another_file_starts_over_from_loading() {
        let mut first = FilePreviewState::new("/repo/a.md".into());
        first.finish(Ok("# a".into()));
        let next = FilePreviewState::new("/repo/b.ts".into());
        assert_eq!(next.content, None);
        assert_eq!(next.failure, None);
        assert_eq!(next.mode, PreviewMode::Rendered);
    }

    // parity: TS-renderer-AiChatFilePreview-d4b925
    #[test]
    fn each_failure_reason_has_its_own_message() {
        let translator = Translator::new(Locale::Ko, "taskDetail");
        let messages: Vec<String> = [
            TaskWorkspaceFileFailure::OutsideWorktree,
            TaskWorkspaceFileFailure::NotFound,
            TaskWorkspaceFileFailure::TooLarge,
            TaskWorkspaceFileFailure::Unreadable,
        ]
        .into_iter()
        .map(|failure| translator.translate(failure_key(failure)))
        .collect();
        assert_eq!(
            messages,
            [
                "작업 폴더 밖의 파일이라 여기서 열 수 없습니다.",
                "파일이 더 이상 없습니다.",
                "1MB보다 큰 파일은 미리 볼 수 없습니다.",
                "파일을 읽지 못했습니다.",
            ]
        );
        let mut preview = FilePreviewState::new("/repo/a.ts".into());
        preview.finish(Err(TaskWorkspaceFileFailure::NotFound));
        assert_eq!(preview.failure, Some(TaskWorkspaceFileFailure::NotFound));
    }

    // parity: TS-renderer-AiChatFilePreview-a42a5d
    #[test]
    fn tex_equations_render_as_formulas_and_keep_their_source() {
        let source = "\\documentclass{article}\n% 주석\n\\begin{document}\n\\begin{equation}\nE = mc^2\n\\end{equation}\n\\end{document}";
        assert_eq!(latex_preview_markdown(source), "$$E = mc^2$$");
        let mut preview = FilePreviewState::new("/repo/paper.tex".into());
        preview.finish(Ok(source.into()));
        preview.mode = PreviewMode::Source;
        assert_eq!(preview.content.as_deref(), Some(source));
    }
}
