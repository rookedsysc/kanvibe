//! 상태 패널의 작업 삭제 버튼(TS `DeleteTaskButton`과 `TaskDetailRoute.handleDelete`).
//! 확인을 받은 뒤 삭제하고 보드로 간다. 다른 데스크톱 전파와 worktree 정리가 끝날 때까지 버튼을 막아 같은 요청을 두 번 보내지 않고,
//! 실패하면 사유를 남긴 채 버튼을 다시 쓸 수 있게 한다.

use gpui_kit::{
    AnyElement, Context, Div, InteractiveElement, IntoElement, ParentElement, Stateful,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use super::TaskDetailView;
use super::status_move::{KANBAN_SERVICE_UNAVAILABLE, kanban_service};
use crate::components::TailwindText;
use crate::components::spinner::{is_spinner_animated, spinner_ring};
use crate::qa::prompts::confirm_action;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::services::ServiceResult;
use crate::text_baseline::{TextFamily, aligned_text};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::settings::service_call::{call_service, failure};
use crate::views::task_detail::usage_cards::visible_text;

/// Tailwind `bg-red-50`·`hover:bg-red-100`(TS가 토큰 없이 쓴 고정 색)
const DELETE_BACKGROUND: u32 = 0xfef2f2;
const DELETE_HOVER_BACKGROUND: u32 = 0xfee2e2;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeleteTaskState {
    pub is_pending: bool,
    pub failure: Option<String>,
}

impl TaskDetailView {
    fn request_task_deletion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.delete_task.is_pending {
            return;
        }
        let message = self.translator.translate("deleteConfirm");
        let confirmation = confirm_action(&message, None, window, cx);
        self._delete_task = Some(cx.spawn(async move |this, cx| {
            if !confirmation.await {
                return;
            }
            let _ = this.update(cx, |view, cx| view.delete_current_task(cx));
        }));
    }

    fn delete_current_task(&mut self, cx: &mut Context<Self>) {
        self.delete_task = DeleteTaskState {
            is_pending: true,
            failure: None,
        };
        let service = kanban_service(cx);
        let task_id = self.task_id.clone();
        let arguments = serde_json::json!([task_id]).to_string();
        let request = call_service(
            cx,
            "kanban.deleteTask",
            arguments,
            move || -> ServiceResult<bool> {
                let service = service.ok_or_else(|| failure(KANBAN_SERVICE_UNAVAILABLE))?;
                service.delete_task(&task_id).map_err(failure)
            },
        );
        self._delete_task = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |view, cx| {
                view.delete_task.is_pending = false;
                match result {
                    Ok(_) => view.screen.navigator.navigate("/", cx),
                    Err(failure) => view.delete_task.failure = Some(failure.display_message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// `inline-flex items-center gap-1.5 px-3 py-1.5 text-xs bg-red-50 text-status-error rounded-md`.
    /// 삭제 중에는 `border-2 border-current border-t-transparent` 고리와 `deleting` 문구를 보인다
    pub(super) fn render_delete_task(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_pending = self.delete_task.is_pending;
        let label = match is_pending {
            true => self.translator.translate("deleting"),
            false => self.translator.translate("delete"),
        };
        let error_color = theme.status.error;
        let mut button = delete_button(theme, label, is_pending, is_spinner_animated(cx));
        button = match is_pending {
            true => button.opacity(0.6),
            false => button
                .cursor_pointer()
                .hover(|style| style.bg(gpui_kit::rgb(DELETE_HOVER_BACKGROUND)))
                .on_click(
                    cx.listener(|view, _, window, cx| view.request_task_deletion(window, cx)),
                ),
        };
        let failure = self.delete_task.failure.as_ref().map(|reason| {
            visible_text(
                self.translator
                    .translate_with("deleteFailed", &[("reason", reason)]),
                text_scale::XS,
            )
            .mt(spacing(2.0))
            .text_color(error_color)
        });
        div()
            .child(div().flex().child(button))
            .children(failure)
            .into_any_element()
    }
}

/// 삭제 버튼 겉모양. 삭제 중에는 `h-3 w-3 border-2 border-current border-t-transparent motion-safe:animate-spin`
/// 고리를 앞에 붙인다
fn delete_button(
    theme: &ThemeTokens,
    label: String,
    is_pending: bool,
    is_spinner_animated: bool,
) -> Stateful<Div> {
    let error_color = theme.status.error;
    div()
        .id("task-delete")
        .relative()
        .flex()
        .items_center()
        .gap(spacing(1.5))
        .rounded(radius::MD)
        .px(spacing(3.0))
        .py(spacing(1.5))
        .text_scale(text_scale::XS)
        .bg(gpui_kit::rgb(DELETE_BACKGROUND))
        .text_color(error_color)
        .children(is_pending.then(|| {
            div()
                .flex_none()
                .size(px(12.0))
                .child(spinner_ring(error_color, is_spinner_animated))
        }))
        .child(aligned_text(
            label.clone(),
            text_scale::XS,
            TextFamily::Sans,
        ))
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.clone().into(),
            text: label.into(),
        }))
}
