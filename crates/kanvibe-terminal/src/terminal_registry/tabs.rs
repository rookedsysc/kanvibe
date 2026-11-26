//! terminal 세션의 탭 목록. 탭은 PTY보다 먼저 생기고(렌더러가 붙기 전) PTY보다 늦게까지 남을 수 있어(셸이 죽어도 탭이 보임)
//! 활성 PTY 목록과 따로 두되, 소유자는 레지스트리 하나로 유지한다.

use std::collections::HashMap;

use kanvibe_domain::task::SessionType;
use kanvibe_domain::terminal_tabs::TerminalTab;

use super::RemoteHostPtySession;

#[derive(Debug, Clone)]
pub(super) struct LocalTab {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub(super) struct LocalTerminalTabState {
    pub tabs: Vec<LocalTab>,
    pub active_tab_id: String,
    pub next_tab_number: i64,
}

/// 태스크마다 탭 목록을 둔다
#[derive(Default)]
pub(super) struct LocalTerminalTabs {
    states: HashMap<String, LocalTerminalTabState>,
}

impl LocalTerminalTabs {
    pub fn contains(&self, task_id: &str) -> bool {
        self.states.contains_key(task_id)
    }

    pub fn remove_task(&mut self, task_id: &str) {
        self.states.remove(task_id);
    }

    pub fn clear(&mut self) {
        self.states.clear();
    }

    pub fn insert(&mut self, task_id: &str, state: LocalTerminalTabState) {
        self.states.insert(task_id.to_string(), state);
    }

    /// 처음 여는 태스크는 탭 하나를 만들어 준다
    pub fn state_mut(
        &mut self,
        task_id: &str,
        default_tab_name: &str,
    ) -> &mut LocalTerminalTabState {
        self.states
            .entry(task_id.to_string())
            .or_insert_with(|| LocalTerminalTabState {
                tabs: vec![LocalTab {
                    id: format!("{task_id}-1"),
                    name: default_tab_name.to_string(),
                }],
                active_tab_id: format!("{task_id}-1"),
                next_tab_number: 2,
            })
    }

    pub fn list(&mut self, task_id: &str, default_tab_name: &str) -> Vec<TerminalTab> {
        to_terminal_tabs(self.state_mut(task_id, default_tab_name))
    }

    pub fn create(&mut self, task_id: &str, default_tab_name: &str) -> TerminalTab {
        let state = self.state_mut(task_id, default_tab_name);
        let created_tab = LocalTab {
            id: format!("{task_id}-{}", state.next_tab_number),
            name: default_tab_name.to_string(),
        };
        state.next_tab_number += 1;
        state.tabs.push(created_tab.clone());
        state.active_tab_id = created_tab.id.clone();
        TerminalTab {
            id: created_tab.id,
            native_index: state.tabs.len() as i64 - 1,
            name: created_tab.name,
            is_active: true,
        }
    }

    /// 탭을 목록에서 지우고 남은 탭 수를 돌려준다. 활성 탭을 지우면 그 자리를 이어받는 탭이 활성이 된다
    pub fn remove_tab(&mut self, task_id: &str, tab_id: &str) -> Option<usize> {
        let state = self.states.get_mut(task_id)?;
        let closed_index = state.tabs.iter().position(|tab| tab.id == tab_id)?;
        state.tabs.remove(closed_index);
        if state.tabs.is_empty() {
            self.states.remove(task_id);
            return Some(0);
        }
        if state.active_tab_id == tab_id {
            let successor_index = closed_index.min(state.tabs.len() - 1);
            state.active_tab_id = state.tabs[successor_index].id.clone();
        }
        Some(state.tabs.len())
    }

    pub fn rename(&mut self, task_id: &str, tab_id: &str, name: &str, default_tab_name: &str) {
        let state = self.state_mut(task_id, default_tab_name);
        if let Some(target_tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            target_tab.name = name.to_string();
        }
    }

    pub fn select(&mut self, task_id: &str, tab_id: &str, default_tab_name: &str) {
        let state = self.state_mut(task_id, default_tab_name);
        if state.tabs.iter().any(|tab| tab.id == tab_id) {
            state.active_tab_id = tab_id.to_string();
        }
    }

    pub fn move_tab(
        &mut self,
        task_id: &str,
        tab_id: &str,
        target_index: usize,
        default_tab_name: &str,
    ) {
        let state = self.state_mut(task_id, default_tab_name);
        let Some(current_index) = state.tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        if target_index >= state.tabs.len() {
            return;
        }
        let moved_tab = state.tabs.remove(current_index);
        state.tabs.insert(target_index, moved_tab);
    }
}

fn to_terminal_tabs(state: &LocalTerminalTabState) -> Vec<TerminalTab> {
    state
        .tabs
        .iter()
        .enumerate()
        .map(|(index, tab)| TerminalTab {
            id: tab.id.clone(),
            native_index: index as i64,
            name: tab.name.clone(),
            is_active: tab.id == state.active_tab_id,
        })
        .collect()
}

/// daemon이 아직 소유한 terminal 세션에서 이 태스크의 탭 목록을 되살린다. 되살릴 탭이 없으면 `None`이다
pub(super) fn restore_tabs_from_remote_sessions(
    task_id: &str,
    session_name: &str,
    sessions: &[RemoteHostPtySession],
    default_tab_name: &str,
) -> Option<LocalTerminalTabState> {
    let key_prefix = format!("{task_id}#");
    let mut tabs: Vec<LocalTab> = Vec::new();
    for session in sessions {
        let Some(terminal_key) = read_terminal_session_key(&session.key, session_name) else {
            continue;
        };
        let Some(id) = terminal_key.strip_prefix(&key_prefix) else {
            continue;
        };
        if !tabs.iter().any(|tab| tab.id == id) {
            tabs.push(LocalTab {
                id: id.to_string(),
                name: default_tab_name.to_string(),
            });
        }
    }
    let first_tab_id = tabs.first()?.id.clone();
    let highest_tab_number = tabs
        .iter()
        .filter_map(|tab| read_tab_number(&tab.id, task_id))
        .fold(0, i64::max);
    Some(LocalTerminalTabState {
        tabs,
        active_tab_id: first_tab_id,
        next_tab_number: highest_tab_number + 1,
    })
}

/// `JSON.stringify([terminalKey, "terminal", sessionName])` 꼴일 때만 terminalKey를 돌려준다
fn read_terminal_session_key(key: &str, session_name: &str) -> Option<String> {
    let parsed = kanvibe_ai::javascript::parse_json(key)?;
    let parts = parsed.as_array()?;
    let is_terminal_session = parts.get(1).and_then(|value| value.as_str())
        == Some(SessionType::Terminal.as_str())
        && parts.get(2).and_then(|value| value.as_str()) == Some(session_name);
    if !is_terminal_session {
        return None;
    }
    parts.first()?.as_str().map(str::to_string)
}

/// JS `Number(tab.id.slice(taskId.length + 1))`가 안전한 정수일 때의 값
fn read_tab_number(tab_id: &str, task_id: &str) -> Option<i64> {
    let suffix = kanvibe_ai::javascript::utf16_suffix_from(
        tab_id,
        kanvibe_ai::javascript::utf16_length(task_id) + 1,
    );
    let trimmed = kanvibe_ai::javascript::js_trim(suffix);
    let number = match trimmed.is_empty() {
        true => 0.0,
        false => trimmed.parse::<f64>().ok()?,
    };
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    let is_safe_integer =
        number.is_finite() && number.fract() == 0.0 && number.abs() <= MAX_SAFE_INTEGER;
    is_safe_integer.then_some(number as i64)
}
