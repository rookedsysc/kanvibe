//! 타이틀 카드의 설명 인라인 편집(TS `TaskDetailTitleCard`). 설명을 누르면 편집으로 바뀌고,
//! Enter는 저장, Shift+Enter는 줄바꿈, Esc는 취소다. 저장 중에는 한 번만 보내고 실패하면 편집을 연 채 사유를 남긴다.

use gpui_kit::base::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};
use kanvibe_domain::task::KanbanTask;
use kanvibe_services::kanban::TaskUpdate;

use super::TaskDetailView;
use super::status_move::{KANBAN_SERVICE_NOT_COMPOSED, SharedKanbanService};
use crate::components::TailwindText;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

pub struct DescriptionEdit {
    input: Entity<TextareaState>,
    is_saving: bool,
    error: Option<String>,
    _events: Subscription,
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
        let input = cx.new(|cx| {
            let mut state =
                TextareaState::new(window, cx).default_value(SharedString::from(current));
            state.set_auto_grow(4, 4, cx);
            state
        });
        input.update(cx, |state, cx| state.focus(window, cx));
        let events = cx.subscribe_in(&input, window, |view, _, event, window, cx| {
            if let InputEvent::PressEnter { shift: false, .. } = event {
                view.save_description(window, cx);
            }
        });
        self.description_edit = Some(DescriptionEdit {
            input,
            is_saving: false,
            error: None,
            _events: events,
        });
        cx.notify();
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
        let service = cx
            .try_global::<SharedKanbanService>()
            .and_then(|shared| shared.service.clone());
        let Some(edit) = self.description_edit.as_mut() else {
            return;
        };
        let Some(service) = service else {
            edit.error = Some(KANBAN_SERVICE_NOT_COMPOSED.to_string());
            cx.notify();
            return;
        };
        edit.is_saving = true;
        edit.error = None;
        let task_id = self.task_id.clone();
        let update = TaskUpdate {
            title: None,
            description: Some(description),
            priority: None,
        };
        let save = cx.background_spawn(async move {
            service
                .update_task(&task_id, &update)
                .map_err(|error| error.to_string())
        });
        self._description_save = Some(cx.spawn(async move |this, cx| {
            let result = save.await;
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
                if let (Some(updated), super::DetailLoad::Loaded(snapshot)) =
                    (updated, &mut self.load)
                {
                    snapshot.task = updated;
                }
                self.description_edit = None;
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
        let cancel_label = self.common.translate("cancel");
        let save_label = match edit.is_saving {
            true => self.common.translate("saving"),
            false => self.common.translate("save"),
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .w_full()
                    .rounded(radius::MD)
                    .border_1()
                    .border_color(theme.brand.primary)
                    .bg(theme.background.page)
                    .p(spacing(2.0))
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.primary)
                    .child(Textarea::new(&edit.input)),
            )
            .child(
                div()
                    .mt(spacing(2.0))
                    .flex()
                    .justify_end()
                    .gap(spacing(2.0))
                    .child(
                        editor_button(
                            theme,
                            "description-cancel",
                            cancel_label,
                            false,
                            edit.is_saving,
                        )
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.cancel_description_edit(window, cx);
                        })),
                    )
                    .child(
                        editor_button(theme, "description-save", save_label, true, edit.is_saving)
                            .on_click(
                                cx.listener(|view, _, window, cx| {
                                    view.save_description(window, cx)
                                }),
                            ),
                    ),
            )
            .children(edit.error.as_ref().map(|reason| {
                div()
                    .mt(spacing(2.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.status.error)
                    .child(
                        self.common
                            .translate_with("saveFailed", &[("reason", reason)]),
                    )
            }))
            .into_any_element()
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

    // parity: TS-components-TaskDetailTitleCard-0b0dcc
    #[test]
    fn unchanged_description_is_not_saved() {
        assert_eq!(description_to_save("  같은 설명 ", Some("같은 설명")), None);
    }

    // parity: TS-components-TaskDetailTitleCard-3a1875
    #[test]
    fn whitespace_only_draft_clears_the_description() {
        assert_eq!(description_to_save("   ", Some("이전")), Some(None));
    }

    // parity: TS-components-TaskDetailTitleCard-a4b0fd
    #[test]
    fn new_description_is_saved_for_a_task_without_one() {
        assert_eq!(
            description_to_save(" 새 설명 ", None),
            Some(Some("새 설명".to_string()))
        );
    }
}
