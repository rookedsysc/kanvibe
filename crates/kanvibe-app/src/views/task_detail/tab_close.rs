//! 탭 닫기와 탭 단축키(TS `closeTerminalTabOrWindow`·`runTerminalTabCommand`).
//! 닫기 전에 그 탭에서 도는 프로세스를 확인받고, 마지막 탭이라 창까지 닫아야 하면 창을 닫는다.

use gpui_kit::{AppContext, Context, PromptButton, PromptLevel, Window};
use kanvibe_domain::shortcuts::bindings::ShortcutCommandId;
use kanvibe_domain::terminal_tabs::TerminalTabShortcutCommand;

use super::TaskDetailView;
use super::layout_state::MainView;
use super::tab_bar::tab_close_confirm_message;
use super::tabs::TabCommand;
use super::terminal::runtime::terminal_runtime;
use crate::shortcuts::ShortcutState;

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

    /// 닫을지 묻고 닫는다. 확인 중이거나 닫는 중이면 겹쳐 시작하지 않는다
    pub(super) fn close_terminal_tab(
        &mut self,
        tab_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_closing_tab {
            return;
        }
        let Some(runtime) = terminal_runtime(cx) else {
            return;
        };
        self.is_closing_tab = true;
        let task_id = self.task_id.clone();
        let tab_id = tab_id.to_string();
        let preview_runtime = runtime.clone();
        let preview_task = task_id.clone();
        let preview_tab = tab_id.clone();
        let preview = cx.background_spawn(async move {
            preview_runtime
                .tabs
                .preview_terminal_tab_close(&preview_task, &preview_tab)
        });
        let shortcut_state = cx.global::<ShortcutState>();
        let close_window_shortcut = shortcut_state
            .bindings
            .shortcut(ShortcutCommandId::TerminalWindowClose)
            .to_string();
        let platform = shortcut_state.platform;
        let translator = self.translator;
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
                    .update(|window, cx| {
                        window.prompt(
                            PromptLevel::Warning,
                            &message,
                            None,
                            &[PromptButton::ok("OK"), PromptButton::cancel("Cancel")],
                            cx,
                        )
                    })
                    .ok()?;
                if answer.await.ok()? != 0 {
                    return Some(false);
                }
                let closed = cx
                    .background_spawn(
                        async move { runtime.tabs.close_terminal_tab(&task_id, &tab_id) },
                    )
                    .await
                    .ok()?;
                Some(closed.should_close_window == Some(true))
            }
            .await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.is_closing_tab = false;
                match outcome {
                    Some(true) => window.remove_window(),
                    Some(false) => view.refresh_tabs(cx),
                    None => {
                        drop(window.prompt(
                            PromptLevel::Critical,
                            &view.translator.translate("terminalTabCloseCheckFailed"),
                            None,
                            &[PromptButton::ok("OK")],
                            cx,
                        ));
                    }
                }
            });
        }));
    }
}
