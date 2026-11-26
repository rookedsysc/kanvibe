//! 우선순위 편집(TS `PriorityEditor` + `PrioritySelector`). 없음·!·!!·!!! 네 버튼 중 하나를 누르면 바로 저장한다.
//! 저장 중에는 네 버튼을 모두 막아(TS `fieldset disabled`) 겹쳐 저장하지 않고, 실패하면 사유를 보인다.

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Rgba, SharedString,
    StatefulInteractiveElement, Styled, div, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::task::TaskPriority;
use kanvibe_services::kanban::TaskUpdate;

use crate::components::{TailwindText, box_shadows, fade};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::services::ServiceResult;
use crate::text_baseline::{TextFamily, aligned_text};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::settings::service_call::{call_service, failure};
use crate::views::task_detail::TaskDetailView;
use crate::views::task_detail::status_move::{KANBAN_SERVICE_UNAVAILABLE, kanban_service};
use crate::views::task_detail::usage_cards::visible_text;

/// TS `PRIORITY_OPTIONS` 순서
pub const PRIORITY_OPTIONS: [(Option<TaskPriority>, &str); 4] = [
    (None, "priorityNone"),
    (Some(TaskPriority::Low), "priorityLow"),
    (Some(TaskPriority::Medium), "priorityMedium"),
    (Some(TaskPriority::High), "priorityHigh"),
];

/// 강조 고리를 그릴 칸. 우선순위가 없으면 `없음` 칸이다
pub fn selected_priority_index(current: Option<TaskPriority>) -> usize {
    PRIORITY_OPTIONS
        .iter()
        .position(|(priority, _)| *priority == current)
        .unwrap_or(0)
}

/// 버튼 하나의 배경·글자·테두리
fn option_colors(theme: &ThemeTokens, priority: Option<TaskPriority>) -> (Rgba, Rgba, Rgba) {
    match priority {
        None => (
            theme.background.page,
            theme.text.muted,
            theme.border.default,
        ),
        Some(TaskPriority::Low) => {
            let colors = theme.priority.low;
            (colors.background, colors.text, fade(colors.text, 0.3))
        }
        Some(TaskPriority::Medium) => {
            let colors = theme.priority.medium;
            (colors.background, colors.text, fade(colors.text, 0.3))
        }
        Some(TaskPriority::High) => {
            let colors = theme.priority.high;
            (colors.background, colors.text, fade(colors.text, 0.3))
        }
    }
}

/// `ring-2 ring-offset-1 ring-brand-primary`. 바깥 2px 강조 고리와 그 안쪽 1px 흰 띄움을 겉 감싸개로 그린다.
/// 감싸개는 고리 두께만큼 음수 여백을 줘 자리를 차지하지 않는다(CSS 고리는 배치에 영향이 없다)
fn selected_ring(theme: &ThemeTokens, button: AnyElement) -> AnyElement {
    const RING_WIDTH: f32 = 3.0;
    div()
        .m(px(-RING_WIDTH))
        .rounded(radius::MD + px(RING_WIDTH))
        .border_2()
        .border_color(theme.brand.primary)
        .child(
            div()
                .rounded(radius::MD + px(1.0))
                .border_1()
                .border_color(gpui_kit::rgb(0xffffff))
                .child(button),
        )
        .into_any_element()
}

impl TaskDetailView {
    pub(in crate::views::task_detail) fn render_priority_editor(
        &self,
        theme: &ThemeTokens,
        current: Option<TaskPriority>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let task_namespace = Translator::new(self.screen.route.locale, "task");
        let save = &self.editors.priority;
        let selected_index = selected_priority_index(current);
        let buttons = PRIORITY_OPTIONS
            .iter()
            .enumerate()
            .map(|(index, (priority, label_key))| {
                let label = task_namespace.translate(label_key);
                self.render_priority_option(theme, *priority, label, index == selected_index, cx)
            })
            .collect::<Vec<_>>();
        let status = save.is_pending.then(|| {
            visible_text(self.common.translate("saving"), text_scale::PX_11)
                .mt(spacing(1.0))
                .text_color(theme.text.muted)
        });
        let error = save.error.as_ref().map(|reason| {
            visible_text(
                self.common
                    .translate_with("saveFailed", &[("reason", reason)]),
                text_scale::PX_11,
            )
            .mt(spacing(1.0))
            .text_color(theme.status.error)
        });
        let selector = div()
            .flex()
            .items_center()
            .gap(spacing(1.5))
            .children(buttons);
        let selector = match save.is_pending {
            true => selector.opacity(0.5),
            false => selector,
        };
        div()
            .flex()
            .flex_col()
            .items_end()
            .child(selector)
            .children(status)
            .children(error)
            .into_any_element()
    }

    fn render_priority_option(
        &self,
        theme: &ThemeTokens,
        priority: Option<TaskPriority>,
        label: String,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (background, text, border) = option_colors(theme, priority);
        let id = priority.map_or("none", TaskPriority::as_str);
        let mut button = div()
            .id(SharedString::from(format!("priority-{id}")))
            .relative()
            .rounded(radius::MD)
            .border_1()
            .border_color(border)
            .bg(background)
            .px(spacing(2.5))
            .py(spacing(1.0))
            .text_scale(text_scale::XS)
            .css_weight(500)
            .text_color(text)
            .child(aligned_text(
                label.clone(),
                text_scale::XS,
                TextFamily::Sans,
            ))
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: label.clone().into(),
                text: label.into(),
            }));
        button = match is_selected {
            true => button.shadow(box_shadows(&theme.shadow.sm)),
            false => button.opacity(0.6).hover(|style| style.opacity(1.0)),
        };
        if !self.editors.priority.is_pending {
            button = button
                .cursor_pointer()
                .on_click(cx.listener(move |view, _, _, cx| view.save_priority(priority, cx)));
        }
        match is_selected {
            true => selected_ring(theme, button.into_any_element()),
            false => button.into_any_element(),
        }
    }

    fn save_priority(&mut self, priority: Option<TaskPriority>, cx: &mut Context<Self>) {
        if !self.editors.priority.begin() {
            return;
        }
        let service = kanban_service(cx);
        let task_id = self.task_id.clone();
        let arguments = serde_json::json!([task_id, { "priority": priority }]).to_string();
        let request = call_service(
            cx,
            "kanban.updateTask",
            arguments,
            move || -> ServiceResult<_> {
                let service = service.ok_or_else(|| failure(KANBAN_SERVICE_UNAVAILABLE))?;
                let update = TaskUpdate {
                    title: None,
                    description: None,
                    priority: Some(priority),
                };
                service.update_task(&task_id, &update).map_err(failure)
            },
        );
        self.editors._priority_save = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(saved) => {
                        view.editors.priority.finish(None);
                        view.apply_saved_task(saved, cx);
                    }
                    Err(failure) => view
                        .editors
                        .priority
                        .finish(Some(failure.display_message())),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_priority_gets_the_selection_ring() {
        assert_eq!(selected_priority_index(Some(TaskPriority::High)), 3);
        assert_eq!(selected_priority_index(Some(TaskPriority::Low)), 1);
        assert_eq!(selected_priority_index(None), 0);
    }
}
