//! 탭 닫기와 탭 단축키(TS `closeTerminalTabOrWindow`·`runTerminalTabCommand`).
//! 닫기 전에 그 탭에서 도는 프로세스를 확인받고, 마지막 탭이라 창까지 닫아야 하면 창을 닫는다.

use std::sync::Arc;

use gpui_kit::{App, AppContext, Context, Task, Window};
use kanvibe_domain::shortcuts::bindings::ShortcutCommandId;
use kanvibe_domain::terminal_tabs::TerminalTabShortcutCommand;
use kanvibe_runtime::DesktopRuntime;
use kanvibe_terminal::terminal_tab_service::TerminalTabClosePreview;
use serde_json::Value;

use super::TaskDetailView;
use super::data::desktop_runtime;
use super::dock_commands::show_alert;
use super::layout_state::MainView;
use super::tab_bar::tab_close_confirm_message;
use super::tabs::TabCommand;
use crate::qa::faults::FaultOutcome;
use crate::qa::prompts::confirm_action;
use crate::services::{AppServices, apply_fault};
use crate::shortcuts::ShortcutState;

/// TS IPC `terminalTabs.getTerminalTabCloseInfo`
const CLOSE_PREVIEW_SERVICE: &str = "terminalTabs.getTerminalTabCloseInfo";
/// TS IPC `terminalTabs.closeTerminalTab`
const CLOSE_TAB_SERVICE: &str = "terminalTabs.closeTerminalTab";

impl TaskDetailView {
    /// 터미널이 보이지 않으면 탭 닫기는 창 닫기다(일반 앱처럼). 그 밖의 탭 명령은 터미널이 보일 때만 한다
    pub(super) fn run_terminal_tab_shortcut(
        &mut self,
        command: TerminalTabShortcutCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_terminal_visible = self
            .snapshot()
            .is_some_and(|snapshot| snapshot.has_terminal())
            && self.layout.main_view == MainView::Terminal;
        if command == TerminalTabShortcutCommand::CloseTab && !is_terminal_visible {
            window.remove_window();
            return;
        }
        if !is_terminal_visible {
            return;
        }
        let target = match command {
            TerminalTabShortcutCommand::NewTab => Some(TabCommand::Create),
            TerminalTabShortcutCommand::CloseTab => {
                if let Some(tab_id) = self.terminal_tabs.active_tab().map(|tab| tab.id.clone()) {
                    self.close_terminal_tab(&tab_id, window, cx);
                }
                None
            }
            TerminalTabShortcutCommand::PreviousTab => self
                .terminal_tabs
                .relative_tab_id(-1)
                .map(TabCommand::Select),
            TerminalTabShortcutCommand::NextTab => self
                .terminal_tabs
                .relative_tab_id(1)
                .map(TabCommand::Select),
            TerminalTabShortcutCommand::GoToTab { position } => self
                .terminal_tabs
                .tab_id_at(usize::from(position).saturating_sub(1))
                .map(TabCommand::Select),
            TerminalTabShortcutCommand::CloseWindow => None,
        };
        if let Some(command) = target {
            self.run_tab_command(command, cx);
        }
    }

    /// 닫을지 묻고 닫는다. 확인 중이거나 닫는 중이면 겹쳐 시작하지 않는다.
    /// 화면이 사라지면 진행 중인 확인도 함께 버려 묻지도 닫지도 않는다
    pub(super) fn close_terminal_tab(
        &mut self,
        tab_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_closing_tab {
            return;
        }
        let Some(runtime) = desktop_runtime(cx) else {
            return;
        };
        self.is_closing_tab = true;
        let preview = preview_tab_close(&runtime, &self.task_id, tab_id, cx);
        let (close_window_shortcut, platform) = close_window_shortcut(cx);
        let translator = self.translator;
        let task_id = self.task_id.clone();
        let tab_id = tab_id.to_string();
        self._tab_close = Some(cx.spawn_in(window, async move |this, cx| {
            let outcome = async {
                let preview = Some(preview.await).filter(|preview| preview.is_ok)?;
                if preview.should_close_window {
                    return Some(true);
                }
                let message = tab_close_confirm_message(
                    &translator,
                    preview.process_names.as_deref(),
                    &close_window_shortcut,
                    platform,
                );
                let answer = cx
                    .update(|window, cx| confirm_action(&message, None, window, cx))
                    .ok()?;
                if !answer.await {
                    return Some(false);
                }
                let close = cx
                    .update(|_, cx| close_tab(&runtime, &task_id, &tab_id, cx))
                    .ok()?;
                close.await
            }
            .await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.finish_tab_close(outcome, window, cx)
            });
        }));
    }

    fn finish_tab_close(
        &mut self,
        outcome: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_closing_tab = false;
        match outcome {
            Some(true) => window.remove_window(),
            Some(false) => self.refresh_tabs(cx),
            None => show_alert(
                &self.translator.translate("terminalTabCloseCheckFailed"),
                window,
                cx,
            ),
        }
    }
}

/// 확인 문구에 넣을 창 닫기 단축키(사용자가 바꾼 조합)와 표기 플랫폼
fn close_window_shortcut(cx: &App) -> (String, kanvibe_domain::platform::ShortcutPlatform) {
    let shortcut_state = cx.global::<ShortcutState>();
    let shortcut = shortcut_state
        .bindings
        .shortcut(ShortcutCommandId::TerminalWindowClose)
        .to_string();
    (shortcut, shortcut_state.platform)
}

/// TS IPC `terminalTabs.getTerminalTabCloseInfo`. 던진 실패는 확인 실패(`is_ok: false`)로 합친다
fn preview_tab_close(
    runtime: &Arc<DesktopRuntime>,
    task_id: &str,
    tab_id: &str,
    cx: &App,
) -> Task<TerminalTabClosePreview> {
    let fault = cx.global::<AppServices>().check_fault(
        CLOSE_PREVIEW_SERVICE,
        &serde_json::json!([task_id, tab_id]).to_string(),
    );
    let executor = cx.background_executor().clone();
    let runtime = Arc::clone(runtime);
    let (task_id, tab_id) = (task_id.to_string(), tab_id.to_string());
    cx.background_spawn(async move {
        if let FaultOutcome::Result(value) = &fault {
            return close_preview_from_fault_value(value);
        }
        if apply_fault(fault, &executor).await.is_err() {
            return close_preview_from_fault_value(&Value::Null);
        }
        runtime
            .terminal_tab_service()
            .preview_terminal_tab_close(&task_id, &tab_id)
    })
}

/// TS IPC `terminalTabs.closeTerminalTab`. 창까지 닫아야 하면 `Some(true)`, 실패하면 `None`
fn close_tab(
    runtime: &Arc<DesktopRuntime>,
    task_id: &str,
    tab_id: &str,
    cx: &App,
) -> Task<Option<bool>> {
    let fault = cx.global::<AppServices>().check_fault(
        CLOSE_TAB_SERVICE,
        &serde_json::json!([task_id, tab_id]).to_string(),
    );
    let executor = cx.background_executor().clone();
    let runtime = Arc::clone(runtime);
    let (task_id, tab_id) = (task_id.to_string(), tab_id.to_string());
    cx.background_spawn(async move {
        if let FaultOutcome::Result(value) = &fault {
            return Some(value.get("shouldCloseWindow").and_then(Value::as_bool) == Some(true));
        }
        apply_fault(fault, &executor).await.ok()?;
        let closed = runtime
            .terminal_tab_service()
            .close_terminal_tab(&task_id, &tab_id)
            .ok()?;
        Some(closed.should_close_window == Some(true))
    })
}

/// QA 고장 주입 값(TS `{ ok, shouldCloseWindow, processNames }`)
fn close_preview_from_fault_value(value: &Value) -> TerminalTabClosePreview {
    TerminalTabClosePreview {
        is_ok: value.get("ok").and_then(Value::as_bool) == Some(true),
        should_close_window: value.get("shouldCloseWindow").and_then(Value::as_bool) == Some(true),
        process_names: value
            .get("processNames")
            .and_then(Value::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            }),
        error: None,
    }
}
