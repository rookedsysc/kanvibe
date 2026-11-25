//! 세션 없는 프로젝트 작업을 여는 순간 기본 세션 타입으로 터미널 세션을 붙인다(TS `AutoConnectTerminalSession`).
//! 연결 중에는 `connecting`, 실패하면 `noTerminal`·사유·연결 버튼을 보인다.

use gpui_kit::{
    AnyElement, AppContext, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div,
};

use super::TaskDetailView;
use super::status_move::SharedKanbanService;
use crate::components::TailwindText;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

const KANBAN_SERVICE_NOT_COMPOSED: &str = "보드 서비스가 아직 앱에 조립되지 않았습니다.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoConnectState {
    Connecting,
    /// 사유가 없으면 `connectFailed` 문구를 쓴다
    Failed(Option<String>),
}

impl TaskDetailView {
    /// 같은 작업 연결이 진행 중이면 다시 보내지 않는다
    pub(super) fn start_auto_connect(&mut self, cx: &mut Context<Self>) {
        if self._auto_connect.is_some() {
            return;
        }
        let Some(snapshot) = self.snapshot() else {
            return;
        };
        let session_type = snapshot.default_session_type;
        let task_id = snapshot.task.id.clone();
        self.auto_connect = AutoConnectState::Connecting;
        let service = cx
            .try_global::<SharedKanbanService>()
            .and_then(|shared| shared.service.clone());
        let connect = cx.background_spawn(async move {
            let service = service.ok_or_else(|| KANBAN_SERVICE_NOT_COMPOSED.to_string())?;
            service
                .connect_terminal_session(&task_id, session_type)
                .map_err(|error| error.to_string())
        });
        self._auto_connect = Some(cx.spawn(async move |this, cx| {
            let result = connect.await;
            let _ = this.update(cx, |view, cx| {
                view._auto_connect = None;
                match result {
                    Ok(Some(connected)) => {
                        if let Some(mut snapshot) = view.snapshot().cloned() {
                            snapshot.task = connected;
                            view.apply_snapshot(snapshot, cx);
                        }
                    }
                    Ok(None) => view.auto_connect = AutoConnectState::Failed(None),
                    Err(reason) => view.auto_connect = AutoConnectState::Failed(Some(reason)),
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn render_auto_connect(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let content: AnyElement = match &self.auto_connect {
            AutoConnectState::Connecting => div()
                .text_scale(text_scale::SM)
                .text_color(theme.text.muted)
                .child(self.translator.translate("connecting"))
                .into_any_element(),
            AutoConnectState::Failed(reason) => {
                let message = reason
                    .clone()
                    .unwrap_or_else(|| self.translator.translate("connectFailed"));
                let label = self.translator.translate("connectTerminal");
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(spacing(3.0))
                    .child(
                        div()
                            .text_scale(text_scale::SM)
                            .text_color(theme.text.muted)
                            .child(self.translator.translate("noTerminal")),
                    )
                    .child(
                        div()
                            .text_scale(text_scale::XS)
                            .text_color(theme.status.error)
                            .child(message),
                    )
                    .child(
                        div()
                            .id("auto-connect-terminal")
                            .relative()
                            .rounded(radius::MD)
                            .bg(theme.brand.primary)
                            .px(spacing(4.0))
                            .py(spacing(1.5))
                            .text_scale(text_scale::SM)
                            .css_weight(500)
                            .text_color(theme.text.inverse)
                            .cursor_pointer()
                            .child(label.clone())
                            .child(qa_target_layer(QaTargetDescription {
                                role: "button",
                                name: label.clone().into(),
                                text: label.into(),
                            }))
                            .on_click(cx.listener(|view, _, _, cx| view.start_auto_connect(cx))),
                    )
                    .into_any_element()
            }
        };
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .rounded(radius::LG)
            .border_1()
            .border_dashed()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .child(content)
            .into_any_element()
    }
}
