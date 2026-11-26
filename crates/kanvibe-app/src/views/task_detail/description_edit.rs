//! 타이틀 카드의 설명 인라인 편집(TS `TaskDetailTitleCard`). 설명을 누르면 편집으로 바뀌고,
//! Enter는 저장, Shift+Enter는 줄바꿈, Esc는 취소다. 저장 중에는 한 번만 보내고 실패하면 편집을 연 채 사유를 남긴다.

use gpui_kit::base::input::{Enter, InputEditorStyle, Position, Textarea, TextareaState};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_domain::task::KanbanTask;
use kanvibe_services::kanban::TaskUpdate;

use super::TaskDetailView;
use super::status_move::{KANBAN_SERVICE_UNAVAILABLE, kanban_service};
use super::usage_cards::visible_text;
use crate::components::{TailwindText, fade};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::settings::service_call::{call_service, failure};

pub struct DescriptionEdit {
    input: Entity<TextareaState>,
    is_saving: bool,
    error: Option<String>,
}

/// 저장할 값. `None`이면 바뀐 것이 없어 보내지 않고 편집만 닫는다. 공백뿐이면 설명을 지운다(`Some(None)`)
pub fn description_to_save(draft: &str, current: Option<&str>) -> Option<Option<String>> {
    let trimmed = draft.trim();
    let next = (!trimmed.is_empty()).then(|| trimmed.to_string());
    (next.as_deref() != current).then_some(next)
}

impl TaskDetailView {
    pub(super) fn render_description(
        &self,
        theme: &ThemeTokens,
        task: &KanbanTask,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(edit) = self.description_edit.as_ref() {
            return self.render_description_editor(theme, edit, cx);
        }
        let (text, color) = match task.description.as_deref() {
            Some(description) => (description.to_string(), theme.text.secondary),
            None => (
                self.translator.translate("addDescription"),
                theme.text.muted,
            ),
        };
        let mut body = div()
            .id("task-description")
            .relative()
            .w_full()
            .cursor_pointer()
            .text_scale(text_scale::SM)
            .line_height(px(22.75))
            .text_color(color)
            .child(text.clone())
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: text.clone().into(),
                text: text.into(),
            }))
            .on_click(cx.listener(|view, _, window, cx| view.begin_description_edit(window, cx)));
        if task.description.is_none() {
            body = body.italic();
        }
        body.into_any_element()
    }

    fn begin_description_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self
            .snapshot()
            .and_then(|snapshot| snapshot.task.description.clone())
            .unwrap_or_default();
        // TS처럼 커서를 끝에 두고 편집 칸에 포커스한다
        let end = Position {
            line: current.matches('\n').count() as u32,
            character: u32::MAX,
        };
        let input = cx.new(|cx| {
            let mut state =
                TextareaState::new(window, cx).default_value(SharedString::from(current));
            state.set_auto_grow(4, 4, cx);
            state
        });
        input.update(cx, |state, cx| state.set_cursor_position(end, window, cx));
        self.description_edit = Some(DescriptionEdit {
            input,
            is_saving: false,
            error: None,
        });
        cx.notify();
    }

    /// 설명 편집 칸이 입력 포커스를 가졌는지. 화면은 그릴 때 포커스가 자기 안에 없으면 터미널 쪽으로 되돌리는데,
    /// 막 연 편집 칸은 아직 한 번도 그려지지 않아 화면 안으로 판정되지 않는다
    pub(super) fn is_description_editor_focused(
        &self,
        window: &Window,
        cx: &gpui_kit::App,
    ) -> bool {
        self.description_edit.as_ref().is_some_and(|edit| {
            gpui_kit::Focusable::focus_handle(edit.input.read(cx), cx).is_focused(window)
        })
    }

    /// 저장 중에는 취소하지 않는다
    pub(super) fn cancel_description_edit(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self
            .description_edit
            .as_ref()
            .is_none_or(|edit| edit.is_saving)
        {
            return false;
        }
        self.description_edit = None;
        self.focus_terminal(window, cx);
        cx.notify();
        true
    }

    fn save_description(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self
            .description_edit
            .as_mut()
            .filter(|edit| !edit.is_saving)
        else {
            return;
        };
        let draft = edit.input.read(cx).value().to_string();
        let current = self
            .snapshot()
            .and_then(|snapshot| snapshot.task.description.clone());
        let Some(description) = description_to_save(&draft, current.as_deref()) else {
            self.cancel_description_edit(window, cx);
            return;
        };
        let Some(edit) = self.description_edit.as_mut() else {
            return;
        };
        edit.is_saving = true;
        edit.error = None;
        let service = kanban_service(cx);
        let task_id = self.task_id.clone();
        let arguments = serde_json::json!([task_id, { "description": description }]).to_string();
        let save = call_service(cx, "kanban.updateTask", arguments, move || {
            let service = service.ok_or_else(|| failure(KANBAN_SERVICE_UNAVAILABLE))?;
            let update = TaskUpdate {
                title: None,
                description: Some(description),
                priority: None,
            };
            service.update_task(&task_id, &update).map_err(failure)
        });
        self._description_save = Some(cx.spawn(async move |this, cx| {
            let result = save.await.map_err(|failure| failure.display_message());
            let _ = this.update(cx, |view, cx| view.finish_description_save(result, cx));
        }));
        cx.notify();
    }

    fn finish_description_save(
        &mut self,
        result: Result<Option<KanbanTask>, String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(updated) => {
                self.description_edit = None;
                self.apply_saved_task(updated, cx);
            }
            Err(reason) => {
                if let Some(edit) = self.description_edit.as_mut() {
                    edit.is_saving = false;
                    edit.error = Some(reason);
                }
            }
        }
        cx.notify();
    }

    fn render_description_editor(
        &self,
        theme: &ThemeTokens,
        edit: &DescriptionEdit,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let error = edit.error.as_ref().map(|reason| {
            visible_text(
                self.common
                    .translate_with("saveFailed", &[("reason", reason)]),
                text_scale::XS,
            )
            .mt(spacing(2.0))
            .text_color(theme.status.error)
        });
        div()
            .flex()
            .flex_col()
            .child(self.render_description_textarea(theme, edit, cx))
            .child(self.render_description_actions(theme, edit, cx))
            .children(error)
            .into_any_element()
    }

    /// `rows=4 text-sm leading-relaxed bg-bg-page border rounded-md p-2`, 포커스 테두리는 강조색
    fn render_description_textarea(
        &self,
        theme: &ThemeTokens,
        edit: &DescriptionEdit,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let editor_style = InputEditorStyle {
            foreground: theme.text.primary.into(),
            muted_foreground: fade(theme.text.primary, 0.5).into(),
            background: theme.background.page.into(),
            border: theme.brand.primary.into(),
            selection: theme.brand.muted.into(),
            caret: theme.text.primary.into(),
            ..InputEditorStyle::default()
        };
        edit.input
            .update(cx, |input, _| input.set_editor_style(editor_style));
        div()
            .relative()
            .w_full()
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.brand.primary)
            .bg(theme.background.page)
            .p(spacing(2.0))
            .text_scale(text_scale::SM)
            // `leading-relaxed`
            .line_height(px(22.75))
            .text_color(theme.text.primary)
            // Enter·Mod+Enter는 줄을 바꾸지 않고 저장하고(TS `preventDefault`), Shift+Enter만 줄을 바꾼다.
            // 편집 칸보다 먼저 받아야 줄바꿈이 들어가지 않는다
            .capture_action(cx.listener(|view, action: &Enter, window, cx| {
                if !action.shift {
                    cx.stop_propagation();
                    view.save_description(window, cx);
                }
            }))
            .child(Textarea::new(&edit.input))
            .child(qa_target_layer(QaTargetDescription {
                role: "textbox",
                name: SharedString::default(),
                text: edit.input.read(cx).value().to_string().into(),
            }))
    }

    /// 취소·저장 버튼 줄(`flex justify-end gap-2 mt-2`). 저장 중에는 저장 버튼이 `saving` 문구를 보인다
    fn render_description_actions(
        &self,
        theme: &ThemeTokens,
        edit: &DescriptionEdit,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let save_label = match edit.is_saving {
            true => self.common.translate("saving"),
            false => self.common.translate("save"),
        };
        let cancel = editor_button(
            theme,
            "description-cancel",
            self.common.translate("cancel"),
            false,
            edit.is_saving,
        )
        .on_click(cx.listener(|view, _, window, cx| {
            view.cancel_description_edit(window, cx);
        }));
        let save = editor_button(theme, "description-save", save_label, true, edit.is_saving)
            .on_click(cx.listener(|view, _, window, cx| view.save_description(window, cx)));
        div()
            .mt(spacing(2.0))
            .flex()
            .justify_end()
            .gap(spacing(2.0))
            .child(cancel)
            .child(save)
    }
}

/// `px-3 py-1.5 text-xs rounded-md`. 저장은 `bg-brand-primary text-white`, 취소는 `bg-bg-page border`
fn editor_button(
    theme: &ThemeTokens,
    id: &'static str,
    label: String,
    is_primary: bool,
    is_disabled: bool,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let mut button = div()
        .id(id)
        .relative()
        .rounded(radius::MD)
        .px(spacing(3.0))
        .py(spacing(1.5))
        .text_scale(text_scale::XS)
        .child(label.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.clone().into(),
            text: label.into(),
        }));
    button = match is_primary {
        true => button
            .bg(theme.brand.primary)
            .text_color(gpui_kit::rgb(0xffffff)),
        false => button
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.page)
            .text_color(theme.text.secondary),
    };
    match is_disabled {
        true => button.opacity(0.6),
        false => button.cursor_pointer(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_description_is_not_saved() {
        assert_eq!(description_to_save("  같은 설명 ", Some("같은 설명")), None);
    }

    #[test]
    fn whitespace_only_draft_clears_the_description() {
        assert_eq!(description_to_save("   ", Some("이전")), Some(None));
    }

    #[test]
    fn new_description_is_saved_for_a_task_without_one() {
        assert_eq!(
            description_to_save(" 새 설명 ", None),
            Some(Some("새 설명".to_string()))
        );
    }
}
