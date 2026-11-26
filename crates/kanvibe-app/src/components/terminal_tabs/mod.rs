//! 터미널 탭 상태와 탭 바(TS `useTerminalTabs` + `TerminalTabBar`). 작업 상세와 보드 워크스페이스가 작업마다 하나씩 둔다.
//! tmux·zellij·Herdr 탭은 KanVibe 밖에서도 바뀌어 주기적으로 다시 읽고, terminal 세션 탭은 KanVibe만 바꾸므로 조작 직후에만 읽는다.
//! 탭 명령은 한 번에 하나만 보내고, 끝나면 성공·실패와 상관없이 다시 읽어 실제 상태로 돌아온다.
//! 터미널 뷰 바꾸기·창 닫기·입력 포커스 돌려주기처럼 화면마다 다른 일은 `TerminalTabsEvent`로 그 화면에 넘긴다.

mod loader;
mod tab_bar;
mod tab_close;

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::base::input::InputState;
use gpui_kit::{AppContext, Context, Entity, EventEmitter, Subscription, Task};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::terminal_tabs::TerminalTab;
use kanvibe_runtime::DesktopRuntime;
use kanvibe_terminal::terminal_tab_service::{TerminalTabListResult, TerminalTabMutationResult};
use serde_json::Value;

use crate::qa::faults::FaultOutcome;
use crate::services::{AppServices, apply_fault, format_service_error};

pub use loader::render_terminal_loader;

const LOCAL_POLL_INTERVAL: Duration = Duration::from_millis(1_000);
const REMOTE_POLL_INTERVAL: Duration = Duration::from_millis(3_000);
/// TS가 목록 조회 예외에 쓰던 문구
const TAB_LIST_READ_FAILED: &str = "터미널 탭 목록을 읽지 못했습니다.";
/// TS IPC `terminalTabs.listTerminalTabs`
const LIST_TABS_SERVICE: &str = "terminalTabs.listTerminalTabs";

/// 탭 명령 하나
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabCommand {
    Create,
    Select(String),
    Rename(String, String),
    Move(String, usize),
}

impl TabCommand {
    /// TS IPC 이름과 인자(QA 고장 주입 판정용)
    fn service_call(&self, task_id: &str) -> (&'static str, serde_json::Value) {
        use serde_json::json;
        match self {
            Self::Create => ("terminalTabs.createTerminalTab", json!([task_id])),
            Self::Select(tab_id) => ("terminalTabs.selectTerminalTab", json!([task_id, tab_id])),
            Self::Rename(tab_id, name) => (
                "terminalTabs.renameTerminalTab",
                json!([task_id, tab_id, name]),
            ),
            Self::Move(tab_id, target_index) => (
                "terminalTabs.moveTerminalTab",
                json!([task_id, tab_id, target_index]),
            ),
        }
    }

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

    /// `Mod+Alt+[`·`]`: 앞뒤 탭(끝에서 돌아간다). 탭이 하나면 옮길 곳이 없어 선택 명령을 보내지 않는다
    pub fn relative_tab_id(&self, offset: isize) -> Option<String> {
        let count = self.tabs.len() as isize;
        if count < 2 {
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

/// 탭 상태가 바뀌어 탭을 가진 화면이 따라 해야 하는 일
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalTabsEvent {
    /// 탭 목록을 새로 읽었다. 화면은 활성 탭에 맞춰 터미널 뷰를 바꾼다
    TabsRead,
    /// 이름 바꾸기를 확정하거나 그만뒀다. 화면은 입력 포커스를 터미널로 돌린다
    RenameEnded,
    /// 닫으려던 탭이 마지막 탭이라 창까지 닫아야 한다
    WindowCloseRequested,
}

/// 작업 하나의 터미널 탭. 화면이 작업마다 하나씩 만들고, 버리면 진행 중인 조회·명령도 함께 멈춘다
pub struct TerminalTabsController {
    task_id: String,
    state: TerminalTabsState,
    /// 이름을 바꾸는 중인 탭과 입력
    rename: Option<(String, Entity<InputState>)>,
    translator: Translator,
    common: Translator,
    /// 터미널이 보이고 창이 활성이라 탭 목록을 주기적으로 읽는 중인지
    should_poll: bool,
    is_refresh_queued: bool,
    is_closing_tab: bool,
    _poll: Option<Task<()>>,
    _refresh: Option<Task<()>>,
    _command: Option<Task<()>>,
    _rename_events: Option<Subscription>,
    _close: Option<Task<()>>,
}

impl EventEmitter<TerminalTabsEvent> for TerminalTabsController {}

impl TerminalTabsController {
    pub fn new(task_id: String, locale: Locale) -> Self {
        Self {
            task_id,
            state: TerminalTabsState::default(),
            rename: None,
            translator: Translator::new(locale, "taskDetail"),
            common: Translator::new(locale, "common"),
            should_poll: false,
            is_refresh_queued: false,
            is_closing_tab: false,
            _poll: None,
            _refresh: None,
            _command: None,
            _rename_events: None,
            _close: None,
        }
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn state(&self) -> &TerminalTabsState {
        &self.state
    }

    /// 탭 이름 입력란이 열려 있는지. 막 연 입력란은 아직 그려지지 않아 화면이 포커스를 빼앗지 않게 묻는다
    pub fn is_renaming(&self) -> bool {
        self.rename.is_some()
    }

    /// 세션이 바뀌면 이전 목록을 비우고, 이전 세션의 늦은 탭 목록 응답이 새 세션 목록을 덮지 않게 버린다
    pub fn reset_for_new_session(&mut self) {
        self.state = TerminalTabsState::default();
        self._refresh = None;
        self.is_refresh_queued = false;
    }

    /// 탭 목록 조회를 시작한다. 멀티플렉서 세션이면 주기 조회도 건다
    pub fn start_polling(&mut self, is_multiplexer: bool, is_remote: bool, cx: &mut Context<Self>) {
        self.refresh(cx);
        if !is_multiplexer {
            self._poll = None;
            return;
        }
        let interval = if is_remote {
            REMOTE_POLL_INTERVAL
        } else {
            LOCAL_POLL_INTERVAL
        };
        self._poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                let is_alive = this
                    .update(cx, |tabs, cx| {
                        if tabs.should_poll {
                            tabs.poll(cx)
                        }
                    })
                    .is_ok();
                if !is_alive {
                    return;
                }
            }
        }));
    }

    /// 터미널이 보이고 창이 활성이면 주기 조회를 켠다. 꺼져 있다가 켜지는 순간에는 밀린 변화를 한 번 읽는다
    pub fn set_polling_active(&mut self, should_poll: bool, cx: &mut Context<Self>) {
        let is_resumed = should_poll && !self.should_poll;
        self.should_poll = should_poll;
        if is_resumed && self.state.is_list_loaded {
            self.poll(cx);
        }
    }

    /// 읽는 중이면 겹쳐 보내지 않고, 끝난 뒤 한 번 더 읽도록 예약한다(TS `queued`)
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.read_tabs(true, cx);
    }

    /// 주기 조회. 읽는 중이면 이번 차례를 건너뛰고 다시 읽기를 예약하지 않는다(TS `refresh(false)`)
    fn poll(&mut self, cx: &mut Context<Self>) {
        self.read_tabs(false, cx);
    }

    fn read_tabs(&mut self, should_queue_if_busy: bool, cx: &mut Context<Self>) {
        if self._refresh.is_some() {
            self.is_refresh_queued |= should_queue_if_busy;
            return;
        }
        let Some(runtime) = desktop_runtime(cx) else {
            return;
        };
        let task_id = self.task_id.clone();
        let fault = tab_service_fault(LIST_TABS_SERVICE, &serde_json::json!([task_id]), cx);
        let executor = cx.background_executor().clone();
        let read = cx.background_spawn(async move {
            if let FaultOutcome::Result(value) = &fault {
                return Ok(list_result_from_fault_value(value));
            }
            apply_fault(fault, &executor)
                .await
                .map_err(|failure| failure.reason)?;
            runtime
                .terminal_tab_service()
                .list_terminal_tabs(&task_id)
                .map_err(|error| error.to_string())
        });
        self._refresh = Some(cx.spawn(async move |this, cx| {
            let result = read.await;
            let _ = this.update(cx, |tabs, cx| tabs.finish_tab_read(result, cx));
        }));
    }

    fn finish_tab_read(
        &mut self,
        result: Result<TerminalTabListResult, String>,
        cx: &mut Context<Self>,
    ) {
        self._refresh = None;
        let tabs = &mut self.state;
        match result {
            Ok(list) => {
                tabs.tabs = if list.is_ok { list.tabs } else { Vec::new() };
                tabs.error = if list.is_ok { None } else { list.error };
            }
            Err(reason) => {
                tabs.tabs = Vec::new();
                tabs.error = Some(match reason.is_empty() {
                    true => TAB_LIST_READ_FAILED.to_string(),
                    false => format_service_error(&reason),
                });
            }
        }
        tabs.is_list_loaded = true;
        cx.emit(TerminalTabsEvent::TabsRead);
        if std::mem::take(&mut self.is_refresh_queued) {
            self.refresh(cx);
        }
        cx.notify();
    }

    /// 탭 명령을 보낸다. 처리 중이면 새 명령은 받지 않는다(TS `useTrackedAction`)
    pub fn run_command(&mut self, command: TabCommand, cx: &mut Context<Self>) {
        if self.state.pending.is_some() {
            return;
        }
        let Some(runtime) = desktop_runtime(cx) else {
            return;
        };
        if let TabCommand::Select(tab_id) = &command {
            self.state.select_optimistically(tab_id);
        }
        self.state.pending = Some(command.clone());
        self.state.action_error = None;
        let task_id = self.task_id.clone();
        let sent = command.clone();
        let (service, arguments) = command.service_call(&task_id);
        let fault = tab_service_fault(service, &arguments, cx);
        let executor = cx.background_executor().clone();
        let mutation = cx.background_spawn(async move {
            if let FaultOutcome::Result(value) = &fault {
                return Ok(mutation_result_from_fault_value(value));
            }
            apply_fault(fault, &executor)
                .await
                .map_err(|failure| failure.reason)?;
            send_tab_command(&runtime, &task_id, &sent)
        });
        self._command = Some(cx.spawn(async move |this, cx| {
            let result = mutation.await;
            let _ = this.update(cx, |tabs, cx| tabs.finish_tab_command(command, result, cx));
        }));
        cx.notify();
    }

    fn finish_tab_command(
        &mut self,
        command: TabCommand,
        result: Result<TerminalTabMutationResult, String>,
        cx: &mut Context<Self>,
    ) {
        self.state.pending = None;
        let failure = match result {
            Ok(mutation) if mutation.is_ok => None,
            Ok(mutation) => Some(mutation.error.unwrap_or_default()),
            Err(error) => Some(format_service_error(&error)),
        };
        match failure {
            Some(reason) => {
                self.state.action_error = Some(reason);
                self.state.failed_command = Some(command);
            }
            None => self.state.failed_command = None,
        }
        self._refresh = None;
        self.refresh(cx);
        cx.notify();
    }

    fn retry_command(&mut self, cx: &mut Context<Self>) {
        if let Some(command) = self.state.failed_command.take() {
            self.run_command(command, cx);
        }
    }
}

fn desktop_runtime(cx: &gpui_kit::App) -> Option<Arc<DesktopRuntime>> {
    cx.global::<AppServices>().runtime().map(Arc::clone)
}

/// QA 모드 고장 주입 판정
fn tab_service_fault(
    service: &str,
    arguments: &Value,
    cx: &Context<TerminalTabsController>,
) -> FaultOutcome {
    cx.global::<AppServices>()
        .check_fault(service, &arguments.to_string())
}

/// QA 고장 주입 값(TS `{ ok, tabs, error }`)을 목록 결과로 바꾼다
fn list_result_from_fault_value(value: &Value) -> TerminalTabListResult {
    let tabs = value
        .get("tabs")
        .cloned()
        .and_then(|tabs| serde_json::from_value::<Vec<TerminalTab>>(tabs).ok())
        .unwrap_or_default();
    TerminalTabListResult {
        is_ok: value.get("ok").and_then(Value::as_bool) == Some(true),
        tabs,
        error: value
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

/// QA 고장 주입 값(TS `{ ok, error, shouldCloseWindow }`)을 명령 결과로 바꾼다
fn mutation_result_from_fault_value(value: &Value) -> TerminalTabMutationResult {
    TerminalTabMutationResult {
        is_ok: value.get("ok").and_then(Value::as_bool) == Some(true),
        should_close_window: value.get("shouldCloseWindow").and_then(Value::as_bool),
        error: value
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

fn send_tab_command(
    runtime: &Arc<DesktopRuntime>,
    task_id: &str,
    command: &TabCommand,
) -> Result<TerminalTabMutationResult, String> {
    let tabs = runtime.terminal_tab_service();
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

    #[test]
    fn relative_selection_has_no_target_with_a_single_tab() {
        let tabs = state(vec![tab("1", true)]);
        assert_eq!(tabs.relative_tab_id(1), None);
        assert_eq!(tabs.relative_tab_id(-1), None);
    }

    #[test]
    fn optimistic_selection_marks_only_the_chosen_tab() {
        let mut tabs = state(vec![tab("1", true), tab("2", false)]);
        tabs.select_optimistically("2");
        assert_eq!(tabs.active_tab().map(|tab| tab.id.as_str()), Some("2"));
    }

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
