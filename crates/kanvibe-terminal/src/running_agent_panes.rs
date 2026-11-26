//! tmux 서버와 Herdr 세션에서 실행 중인 에이전트 pane을 모은다(TS `aiSessions/runningAgentPanes.ts`).
//! 판정 규칙은 `kanvibe_ai::running_agent_panes`에 있고, 여기서는 pane 조회 명령을 실행해 그 규칙에 넘긴다.
//! zellij, KanVibe terminal 세션, 별도 터미널 앱의 에이전트는 보이지 않으므로 호출자는 세션 기록을 폴백으로 함께 본다.

use std::sync::Arc;
use std::thread;

use kanvibe_ai::running_agent_panes::{RunningAgentPaneSource, running_agent_pane_from_tmux};
use kanvibe_ai::types::RunningAgentPane;
use kanvibe_process::exec::{ExecGitOptions, ShellCommandRunner};

use crate::herdr::HerdrService;
use crate::terminal_tabs::{TmuxPaneSnapshot, build_tmux_list_panes_command, parse_tmux_pane_list};

/// Herdr 쪽 조회. 테스트에서는 가짜로 바꾼다
pub trait HerdrRunningAgentPanes: Send + Sync {
    fn list_herdr_running_agent_panes(&self, ssh_host: Option<&str>) -> Vec<RunningAgentPane>;
}

impl HerdrRunningAgentPanes for HerdrService {
    fn list_herdr_running_agent_panes(&self, ssh_host: Option<&str>) -> Vec<RunningAgentPane> {
        HerdrService::list_herdr_running_agent_panes(self, ssh_host)
    }
}

pub struct MultiplexerRunningAgentPanes {
    runner: Arc<dyn ShellCommandRunner>,
    herdr: Arc<dyn HerdrRunningAgentPanes>,
}

impl MultiplexerRunningAgentPanes {
    pub fn new(
        runner: Arc<dyn ShellCommandRunner>,
        herdr: Arc<dyn HerdrRunningAgentPanes>,
    ) -> Self {
        Self { runner, herdr }
    }

    /// tmux 세션이 하나도 없으면 tmux는 오류로 끝난다. 실행 중인 에이전트가 없는 정상 상태라 빈 목록으로 다룬다
    fn list_tmux_panes(&self, ssh_host: Option<&str>) -> Vec<TmuxPaneSnapshot> {
        self.runner
            .exec_git(
                &build_tmux_list_panes_command(),
                ssh_host,
                &ExecGitOptions::default(),
            )
            .map(|output| parse_tmux_pane_list(&output))
            .unwrap_or_default()
    }
}

fn to_running_agent_pane(pane: &TmuxPaneSnapshot) -> Option<RunningAgentPane> {
    running_agent_pane_from_tmux(
        &pane.command,
        &pane.current_path,
        &pane.session_name,
        &pane.window_id,
        &pane.window_name,
    )
}

impl RunningAgentPaneSource for MultiplexerRunningAgentPanes {
    /// 두 조회는 서로 기다리지 않도록 동시에 돌리고, tmux pane을 앞에 둔다
    fn list_running_agent_panes(&self, ssh_host: Option<&str>) -> Vec<RunningAgentPane> {
        let (tmux_panes, herdr_panes) = thread::scope(|scope| {
            let herdr_lookup = scope.spawn(|| self.herdr.list_herdr_running_agent_panes(ssh_host));
            let tmux_panes = self.list_tmux_panes(ssh_host);
            (tmux_panes, herdr_lookup.join().unwrap_or_default())
        });
        tmux_panes
            .iter()
            .filter_map(to_running_agent_pane)
            .chain(herdr_panes)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use kanvibe_ai::AiSessionProvider;
    use kanvibe_process::exec::ExecGitError;

    use super::*;
    use crate::test_support::ScriptedShellRunner;

    struct FixedHerdrPanes(Vec<RunningAgentPane>);

    impl HerdrRunningAgentPanes for FixedHerdrPanes {
        fn list_herdr_running_agent_panes(&self, _ssh_host: Option<&str>) -> Vec<RunningAgentPane> {
            self.0.clone()
        }
    }

    fn herdr_pane() -> RunningAgentPane {
        RunningAgentPane {
            provider: AiSessionProvider::Codex,
            worktree_path: "/repo/herdr".into(),
            session_name: "kanvibe:repo".into(),
            window_id: "w1:t1".into(),
            window_name: "codex".into(),
        }
    }

    #[test]
    fn agent_tmux_panes_come_before_herdr_panes_and_shells_are_skipped() {
        let runner = ScriptedShellRunner::new();
        let output = "main\t@1\t0\tclaude\t/repo/a\teditor\nmain\t@2\t1\tzsh\t/repo/a\tshell";
        runner.respond_when_contains("list-panes -a", output);
        let source = MultiplexerRunningAgentPanes::new(
            runner.clone(),
            Arc::new(FixedHerdrPanes(vec![herdr_pane()])),
        );

        let panes = source.list_running_agent_panes(Some("remote"));

        let providers: Vec<_> = panes.iter().map(|pane| pane.provider).collect();
        assert_eq!(
            providers,
            vec![AiSessionProvider::Claude, AiSessionProvider::Codex]
        );
        assert_eq!(panes[0].window_id, "@1");
        assert_eq!(runner.calls()[0].ssh_host.as_deref(), Some("remote"));
    }

    #[test]
    fn missing_tmux_server_is_an_empty_list_not_an_error() {
        let runner = ScriptedShellRunner::new();
        runner.respond(|_, _| Some(Err(ExecGitError::new("no server running"))));
        let source = MultiplexerRunningAgentPanes::new(runner, Arc::new(FixedHerdrPanes(vec![])));

        assert!(source.list_running_agent_panes(None).is_empty());
    }
}
