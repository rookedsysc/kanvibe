//! 터미널 탭 상태(TS `useTerminalTabs`). tmux·zellij·Herdr 탭은 KanVibe 밖에서도 바뀌어 주기적으로 다시 읽고,
//! terminal 세션 탭은 KanVibe만 바꾸므로 조작 직후에만 읽는다. 탭 명령은 한 번에 하나만 보내고, 끝나면 성공·실패와
//! 상관없이 다시 읽어 실제 상태로 돌아온다.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{AppContext, Context};
use kanvibe_domain::terminal_tabs::TerminalTab;
use kanvibe_terminal::terminal_tab_service::TerminalTabMutationResult;

use super::TaskDetailView;
use super::terminal::runtime::{TerminalRuntime, terminal_runtime};

const LOCAL_POLL_INTERVAL: Duration = Duration::from_millis(1_000);
const REMOTE_POLL_INTERVAL: Duration = Duration::from_millis(3_000);
/// TS가 목록 조회 예외에 쓰던 문구
const TAB_LIST_READ_FAILED: &str = "터미널 탭 목록을 읽지 못했습니다.";

/// 탭 명령 하나
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabCommand {
    Create,
    Select(String),
    Rename(String, String),
    Move(String, usize),
}

impl TabCommand {
    /// 처리 중 표시를 받을 탭. 추가는 아직 탭이 없다
    pub fn tab_id(&self) -> Option<&str> {
        match self {
            Self::Create => None,
            Self::Select(tab_id) | Self::Rename(tab_id, _) | Self::Move(tab_id, _) => Some(tab_id),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerminalTabsState {
    pub tabs: Vec<TerminalTab>,
    pub is_list_loaded: bool,
    pub error: Option<String>,
    pub pending: Option<TabCommand>,
    pub action_error: Option<String>,
    /// 실패한 명령(다시 시도)
    pub failed_command: Option<TabCommand>,
}

impl TerminalTabsState {
    pub fn active_tab(&self) -> Option<&TerminalTab> {
        self.tabs.iter().find(|tab| tab.is_active)
    }

    /// 선택 응답을 기다리지 않고 먼저 화면을 바꾼다(실제 상태로 다시 덮인다)
    pub fn select_optimistically(&mut self, tab_id: &str) {
        for tab in &mut self.tabs {
            tab.is_active = tab.id == tab_id;
        }
    }

    /// `Mod+Alt+[`·`]`: 앞뒤 탭(끝에서 돌아간다)
    pub fn relative_tab_id(&self, offset: isize) -> Option<String> {
        let count = self.tabs.len() as isize;
        if count == 0 {
            return None;
        }
        let current = self.tabs.iter().position(|tab| tab.is_active).unwrap_or(0) as isize;
        let target = (current + offset).rem_euclid(count) as usize;
        Some(self.tabs[target].id.clone())
    }

    /// `Mod+Alt+{n}`의 n-1 번째 탭
    pub fn tab_id_at(&self, position: usize) -> Option<String> {
        self.tabs.get(position).map(|tab| tab.id.clone())
    }
}

/// 이름 변경 확정: 앞뒤 공백을 떼고 비었거나 그대로면 보내지 않는다
pub fn renamed_tab_name(tabs: &[TerminalTab], tab_id: &str, draft: &str) -> Option<String> {
    let trimmed = draft.trim();
    let previous = tabs
        .iter()
        .find(|tab| tab.id == tab_id)
        .map(|tab| tab.name.as_str());
    (!trimmed.is_empty() && Some(trimmed) != previous).then(|| trimmed.to_string())
}

impl TaskDetailView {
    /// 탭 목록 조회를 시작한다. 멀티플렉서 세션이면 주기 조회도 건다
    pub(super) fn start_tab_polling(
        &mut self,
        is_multiplexer: bool,
        is_remote: bool,
        cx: &mut Context<Self>,
    ) {
        self.refresh_tabs(cx);
        if !is_multiplexer {
            self._tab_poll = None;
            return;
        }
        let interval = if is_remote {
            REMOTE_POLL_INTERVAL
        } else {
            LOCAL_POLL_INTERVAL
        };
        self._tab_poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                let is_alive = this.update(cx, |view, cx| view.refresh_tabs(cx)).is_ok();
                if !is_alive {
                    return;
                }
            }
        }));
    }

    /// 읽는 중이면 겹쳐 보내지 않고, 끝난 뒤 한 번 더 읽도록 예약한다(TS `queued`)
    pub(super) fn refresh_tabs(&mut self, cx: &mut Context<Self>) {
        if self._tab_refresh.is_some() {
            self.is_tab_refresh_queued = true;
            return;
        }
        let Some(runtime) = terminal_runtime(cx) else {
            return;
        };
        let task_id = self.task_id.clone();
        let read = cx.background_spawn(async move { runtime.tabs.list_terminal_tabs(&task_id) });
        self._tab_refresh = Some(cx.spawn(async move |this, cx| {
            let result = read.await;
            let _ = this.update(cx, |view, cx| {
                view._tab_refresh = None;
                let tabs = &mut view.terminal_tabs;
                match result {
                    Ok(list) => {
                        tabs.tabs = if list.is_ok { list.tabs } else { Vec::new() };
                        tabs.error = if list.is_ok { None } else { list.error };
                    }
                    Err(error) => {
                        tabs.tabs = Vec::new();
                        tabs.error = Some(match error.to_string().is_empty() {
                            true => TAB_LIST_READ_FAILED.to_string(),
                            false => error.to_string(),
                        });
                    }
                }
                tabs.is_list_loaded = true;
                view.sync_terminal_tab_pane(cx);
                if std::mem::take(&mut view.is_tab_refresh_queued) {
                    view.refresh_tabs(cx);
                }
                cx.notify();
            });
        }));
    }

    /// 탭 명령을 보낸다. 처리 중이면 새 명령은 받지 않는다(TS `useTrackedAction`)
    pub(super) fn run_tab_command(&mut self, command: TabCommand, cx: &mut Context<Self>) {
        if self.terminal_tabs.pending.is_some() {
            return;
        }
        let Some(runtime) = terminal_runtime(cx) else {
            return;
        };
        if let TabCommand::Select(tab_id) = &command {
            self.terminal_tabs.select_optimistically(tab_id);
        }
        self.terminal_tabs.pending = Some(command.clone());
        self.terminal_tabs.action_error = None;
        let task_id = self.task_id.clone();
        let sent = command.clone();
        let mutation =
            cx.background_spawn(async move { send_tab_command(&runtime, &task_id, &sent) });
        self._tab_command = Some(cx.spawn(async move |this, cx| {
            let result = mutation.await;
            let _ = this.update(cx, |view, cx| view.finish_tab_command(command, result, cx));
        }));
        cx.notify();
    }

    fn finish_tab_command(
        &mut self,
        command: TabCommand,
        result: Result<TerminalTabMutationResult, String>,
        cx: &mut Context<Self>,
    ) {
        self.terminal_tabs.pending = None;
        let failure = match result {
            Ok(mutation) if mutation.is_ok => None,
            Ok(mutation) => Some(mutation.error.unwrap_or_default()),
            Err(error) => Some(error),
        };
        match failure {
            Some(reason) => {
                self.terminal_tabs.action_error = Some(reason);
                self.terminal_tabs.failed_command = Some(command);
            }
            None => self.terminal_tabs.failed_command = None,
        }
        self._tab_refresh = None;
        self.refresh_tabs(cx);
        cx.notify();
    }

    pub(super) fn retry_tab_command(&mut self, cx: &mut Context<Self>) {
        if let Some(command) = self.terminal_tabs.failed_command.take() {
            self.run_tab_command(command, cx);
        }
    }
}

fn send_tab_command(
    runtime: &Arc<TerminalRuntime>,
    task_id: &str,
    command: &TabCommand,
) -> Result<TerminalTabMutationResult, String> {
    let tabs = &runtime.tabs;
    let result = match command {
        TabCommand::Create => tabs.create_terminal_tab(task_id),
        TabCommand::Select(tab_id) => tabs.select_terminal_tab(task_id, tab_id),
        TabCommand::Rename(tab_id, name) => tabs.rename_terminal_tab(task_id, tab_id, name),
        TabCommand::Move(tab_id, target_index) => {
            tabs.move_terminal_tab(task_id, tab_id, *target_index as i64)
        }
    };
    result.map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: &str, is_active: bool) -> TerminalTab {
        TerminalTab {
            id: id.into(),
            native_index: 0,
            name: format!("tab {id}"),
            is_active,
        }
    }

    fn state(tabs: Vec<TerminalTab>) -> TerminalTabsState {
        TerminalTabsState {
            tabs,
            ..TerminalTabsState::default()
        }
    }

    #[test]
    fn relative_selection_wraps_around_the_tab_list() {
        let tabs = state(vec![tab("1", false), tab("2", false), tab("3", true)]);
        assert_eq!(tabs.relative_tab_id(1).as_deref(), Some("1"));
        assert_eq!(tabs.relative_tab_id(-1).as_deref(), Some("2"));
        assert_eq!(tabs.tab_id_at(1).as_deref(), Some("2"));
        assert_eq!(tabs.tab_id_at(5), None);
    }

    // parity: TS-renderer-useTerminalTabs-1430a2
    #[test]
    fn optimistic_selection_marks_only_the_chosen_tab() {
        let mut tabs = state(vec![tab("1", true), tab("2", false)]);
        tabs.select_optimistically("2");
        assert_eq!(tabs.active_tab().map(|tab| tab.id.as_str()), Some("2"));
    }

    // parity: TS-renderer-TerminalTabBar-5f0921
    #[test]
    fn rename_is_sent_only_for_a_changed_non_empty_name() {
        let tabs = vec![tab("1", true)];
        assert_eq!(renamed_tab_name(&tabs, "1", "  tab 1 "), None);
        assert_eq!(renamed_tab_name(&tabs, "1", "   "), None);
        assert_eq!(
            renamed_tab_name(&tabs, "1", " build ").as_deref(),
            Some("build")
        );
    }
}
