//! 작업 상세의 터미널 탭 단축키(TS `runTerminalTabCommand`). 탭 상태·명령·닫기는 공용 `TerminalTabsController`가 맡는다.

use gpui_kit::{Context, Window};
use kanvibe_domain::terminal_tabs::TerminalTabShortcutCommand;

use super::TaskDetailView;
use super::layout_state::MainView;
use crate::components::terminal_tabs::TabCommand;

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
        let tabs = self.terminal_tabs.read(cx).state();
        let target = match command {
            TerminalTabShortcutCommand::NewTab => Some(TabCommand::Create),
            TerminalTabShortcutCommand::CloseTab => {
                if let Some(tab_id) = tabs.active_tab().map(|tab| tab.id.clone()) {
                    self.terminal_tabs
                        .update(cx, |tabs, cx| tabs.close_tab(&tab_id, window, cx));
                }
                None
            }
            TerminalTabShortcutCommand::PreviousTab => {
                tabs.relative_tab_id(-1).map(TabCommand::Select)
            }
            TerminalTabShortcutCommand::NextTab => tabs.relative_tab_id(1).map(TabCommand::Select),
            TerminalTabShortcutCommand::GoToTab { position } => tabs
                .tab_id_at(usize::from(position).saturating_sub(1))
                .map(TabCommand::Select),
            TerminalTabShortcutCommand::CloseWindow => None,
        };
        if let Some(command) = target {
            self.terminal_tabs
                .update(cx, |tabs, cx| tabs.run_command(command, cx));
        }
    }
}
