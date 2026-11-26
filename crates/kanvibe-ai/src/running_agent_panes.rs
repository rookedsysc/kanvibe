//! tmux·Herdr pane에서 관측한 실행중 에이전트. pane 조회 한 번으로 실행 여부, 작업 경로, 전환할 window를 함께 얻는다.
//!
//! tmux `list-panes` 출력 해석과 Herdr 조회는 터미널 crate(M3)의 몫이라 [`RunningAgentPaneSource`]로 받는다.
//! zellij, KanVibe terminal 세션, 별도 터미널 앱의 에이전트는 이 조회에 보이지 않으므로 호출자는 세션 기록의 최근 활동을 함께 본다.

use std::collections::BTreeMap;

use kanvibe_domain::locale_order::compare_like_locale;

use crate::providers::AiSessionProvider;
use crate::types::{AgentCallNode, RunningAgentPane};

/// tmux 서버와 실행 중인 Herdr 세션에서 에이전트 pane을 모은다. tmux 세션이 하나도 없으면 정상 상태라 빈 목록이다
pub trait RunningAgentPaneSource: Send + Sync {
    fn list_running_agent_panes(&self, ssh_host: Option<&str>) -> Vec<RunningAgentPane>;
}

/// pane 조회 수단이 없는 호출자용. 세션 기록만으로 판정한다
pub struct NoRunningAgentPanes;

impl RunningAgentPaneSource for NoRunningAgentPanes {
    fn list_running_agent_panes(&self, _ssh_host: Option<&str>) -> Vec<RunningAgentPane> {
        Vec::new()
    }
}

/// `pane_current_command`는 실행 파일 이름이라 CLI 이름과 그대로 대응한다
pub fn provider_for_pane_command(command: &str) -> Option<AiSessionProvider> {
    match command {
        "claude" => Some(AiSessionProvider::Claude),
        "codex" => Some(AiSessionProvider::Codex),
        "opencode" => Some(AiSessionProvider::Opencode),
        "gemini" => Some(AiSessionProvider::Gemini),
        _ => None,
    }
}

/// tmux pane 한 줄을 에이전트 pane으로 바꾼다. 에이전트 명령이 아니면 `None`이다
pub fn running_agent_pane_from_tmux(
    command: &str,
    current_path: &str,
    session_name: &str,
    window_id: &str,
    window_name: &str,
) -> Option<RunningAgentPane> {
    Some(RunningAgentPane {
        provider: provider_for_pane_command(command)?,
        worktree_path: current_path.to_string(),
        session_name: session_name.to_string(),
        window_id: window_id.to_string(),
        window_name: window_name.to_string(),
    })
}

/// 같은 worktree에서 도는 pane만 남긴다. 하위 디렉터리에서 실행한 에이전트도 그 worktree의 것으로 본다
pub fn filter_panes_by_worktree(
    panes: &[RunningAgentPane],
    worktree_path: Option<&str>,
) -> Vec<RunningAgentPane> {
    let Some(worktree_path) = worktree_path.filter(|path| !path.is_empty()) else {
        return Vec::new();
    };
    let nested_prefix = format!("{worktree_path}/");
    panes
        .iter()
        .filter(|pane| {
            pane.worktree_path == worktree_path || pane.worktree_path.starts_with(&nested_prefix)
        })
        .cloned()
        .collect()
}

/// provider별 개수. 보드 배지와 세션 패널의 칩이 같은 자리를 지키도록 provider 이름순으로 둔다
pub fn count_by_provider(
    providers: impl IntoIterator<Item = AiSessionProvider>,
) -> Vec<(AiSessionProvider, usize)> {
    let mut counts: BTreeMap<AiSessionProvider, usize> = BTreeMap::new();
    for provider in providers {
        *counts.entry(provider).or_default() += 1;
    }
    let mut entries: Vec<(AiSessionProvider, usize)> = counts.into_iter().collect();
    entries.sort_by(|(left, _), (right, _)| compare_like_locale(left.id(), right.id()));
    entries
}

/// 호출 그래프 트리를 깊이 우선 순서의 평면 목록으로 편다
pub fn flatten_agent_call_nodes(nodes: &[AgentCallNode]) -> Vec<&AgentCallNode> {
    nodes
        .iter()
        .flat_map(|node| std::iter::once(node).chain(flatten_agent_call_nodes(&node.children)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(worktree_path: &str) -> RunningAgentPane {
        RunningAgentPane {
            provider: AiSessionProvider::Claude,
            worktree_path: worktree_path.to_string(),
            session_name: "kanvibe-task".to_string(),
            window_id: "@1".to_string(),
            window_name: "claude".to_string(),
        }
    }

    fn paths(panes: &[RunningAgentPane]) -> Vec<&str> {
        panes
            .iter()
            .map(|pane| pane.worktree_path.as_str())
            .collect()
    }

    #[test]
    fn keeps_worktree_and_nested_paths() {
        let panes = [
            pane("/repo/task"),
            pane("/repo/task/src"),
            pane("/repo/other"),
        ];
        assert_eq!(
            paths(&filter_panes_by_worktree(&panes, Some("/repo/task"))),
            ["/repo/task", "/repo/task/src"]
        );
    }

    #[test]
    fn excludes_worktrees_sharing_only_a_prefix() {
        assert!(filter_panes_by_worktree(&[pane("/repo/task-2")], Some("/repo/task")).is_empty());
    }

    #[test]
    fn unknown_worktree_keeps_nothing() {
        assert!(filter_panes_by_worktree(&[pane("/repo/task")], None).is_empty());
    }

    #[test]
    fn maps_agent_commands_and_counts_in_name_order() {
        assert!(running_agent_pane_from_tmux("zsh", "/a", "s", "@1", "w").is_none());
        let codex = running_agent_pane_from_tmux("codex", "/a", "s", "@1", "w").unwrap();
        assert_eq!(codex.provider, AiSessionProvider::Codex);
        let counts = count_by_provider([
            AiSessionProvider::Opencode,
            AiSessionProvider::Claude,
            AiSessionProvider::Opencode,
        ]);
        assert_eq!(
            counts,
            [
                (AiSessionProvider::Claude, 1),
                (AiSessionProvider::Opencode, 2)
            ]
        );
    }
}
