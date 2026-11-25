//! 실행 중인 Herdr 서버들의 에이전트 pane. 공유 서버의 pane은 Space 연결 파일로 task 세션 이름을 되찾는다.

use kanvibe_ai::running_agent_panes::provider_for_pane_command;
use kanvibe_ai::types::RunningAgentPane;
use kanvibe_domain::terminal_tabs::TerminalTab;
use serde::Deserialize;

use super::response::HerdrSession;
use super::workspace_mapping::WorkspaceMapping;
use super::{HerdrError, HerdrService};

#[derive(Deserialize)]
struct HerdrAgent {
    agent: Option<String>,
    pane_id: Option<String>,
    tab_id: Option<String>,
    cwd: Option<String>,
    foreground_cwd: Option<String>,
}

#[derive(Deserialize)]
struct HerdrAgentList {
    agents: Vec<HerdrAgent>,
}

/// pane이 속한 Space의 task 키
fn workspace_key_of(mapping: &WorkspaceMapping, pane_id: Option<&str>) -> Option<String> {
    let pane_id = pane_id?;
    mapping
        .iter()
        .find(|(_, workspace_id)| {
            workspace_id
                .as_str()
                .is_some_and(|workspace_id| pane_id.starts_with(&format!("{workspace_id}:")))
        })
        .map(|(workspace_key, _)| workspace_key.clone())
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn running_agent_pane(
    agent: HerdrAgent,
    session_name: &str,
    tabs: &[TerminalTab],
    mapping: &WorkspaceMapping,
) -> Option<RunningAgentPane> {
    let provider = provider_for_pane_command(agent.agent.as_deref()?)?;
    let worktree_path = non_empty(agent.foreground_cwd).or_else(|| non_empty(agent.cwd))?;
    let session_name = match workspace_key_of(mapping, agent.pane_id.as_deref()) {
        Some(workspace_key) => format!("kanvibe:{workspace_key}"),
        None => session_name.to_string(),
    };
    let window_id = agent.tab_id.unwrap_or_default();
    let window_name = tabs
        .iter()
        .find(|tab| tab.id == window_id)
        .map_or_else(|| window_id.clone(), |tab| tab.name.clone());
    Some(RunningAgentPane {
        provider,
        worktree_path,
        session_name,
        window_id,
        window_name,
    })
}

impl HerdrService {
    /// TS `listHerdrRunningAgentPanes`. 세션 목록이나 한 세션의 조회가 실패해도 나머지 세션의 에이전트는 모은다
    pub fn list_herdr_running_agent_panes(&self, ssh_host: Option<&str>) -> Vec<RunningAgentPane> {
        let sessions = self.list_herdr_sessions(ssh_host).unwrap_or_default();
        let running_sessions: Vec<&HerdrSession> =
            sessions.iter().filter(|session| session.running).collect();
        let panes_by_session: Vec<Vec<RunningAgentPane>> = std::thread::scope(|scope| {
            let readers: Vec<_> = running_sessions
                .iter()
                .map(|session| {
                    scope.spawn(move || self.read_session_agent_panes(&session.name, ssh_host))
                })
                .collect();
            readers
                .into_iter()
                .map(|reader| {
                    reader
                        .join()
                        .expect("에이전트 조회 스레드")
                        .unwrap_or_default()
                })
                .collect()
        });
        panes_by_session.into_iter().flatten().collect()
    }

    fn read_session_agent_panes(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<RunningAgentPane>, HerdrError> {
        let (agents, tabs) = std::thread::scope(|scope| {
            let tabs = scope.spawn(|| self.read_herdr_tabs(session_name, ssh_host));
            let agents = self.query::<HerdrAgentList>(session_name, &["agent", "list"], ssh_host);
            (agents, tabs.join().expect("탭 목록 조회 스레드"))
        });
        let HerdrAgentList { agents } = agents?;
        let tabs = tabs?;
        let mapping = match session_name {
            "kanvibe" => self.read_workspace_mapping_file(ssh_host)?,
            _ => WorkspaceMapping::new(),
        };
        let panes = agents
            .into_iter()
            .filter_map(|agent| running_agent_pane(agent, session_name, &tabs, &mapping))
            .collect();
        Ok(panes)
    }
}
