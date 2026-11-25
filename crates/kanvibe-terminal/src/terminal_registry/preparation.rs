//! 화면 attach 없이 task의 멀티플렉서 세션만 준비한다(TS `prepareTerminalSession`). 에이전트가 화면이 보여 줄 세션 안에서
//! 작업을 시작할 수 있게 하고, 같은 세션의 준비가 겹치면 진행 중인 하나를 함께 기다린다.

use std::sync::Arc;

use kanvibe_domain::task::SessionType;
use kanvibe_process::exec::ExecGitOptions;

use super::local_sessions::{
    TMUX_BOOTSTRAP_TIMEOUT, bootstrap_local_tmux_session,
    create_local_zellij_session_in_background, is_tmux_session_alive, is_zellij_session_alive,
};
use super::remote_commands::{build_remote_session_prepare_command, build_remote_shell_command};
use super::{
    Flight, PreparedTerminalSession, TerminalRegistry, TerminalSessionPreparationTarget,
    build_terminal_key,
};

const TERMINAL_SESSION_SKIPPED_REASON: &str =
    "terminal 세션은 화면에서 열 때 셸을 새로 띄우므로 미리 만들 세션이 없습니다.";

/// TS `JSON.stringify([sshHost ?? null, sessionName])`
pub(super) fn build_session_preparation_key(ssh_host: Option<&str>, session_name: &str) -> String {
    serde_json::json!([ssh_host, session_name]).to_string()
}

fn prepared() -> PreparedTerminalSession {
    PreparedTerminalSession {
        is_prepared: true,
        skipped_reason: None,
        herdr_workspace_id: None,
        herdr_panes: None,
    }
}

impl TerminalRegistry {
    pub(super) fn prepare_terminal_session_once(
        &self,
        target: &TerminalSessionPreparationTarget,
    ) -> Result<PreparedTerminalSession, String> {
        let preparation_key =
            build_session_preparation_key(target.ssh_host.as_deref(), &target.session_name);
        let preparation = {
            let mut state = self.lock_state();
            if let Some(pending_preparation) = state.pending_preparations.get(&preparation_key) {
                let pending_preparation = Arc::clone(pending_preparation);
                drop(state);
                return pending_preparation.wait();
            }
            let preparation = Flight::new();
            state
                .pending_preparations
                .insert(preparation_key.clone(), Arc::clone(&preparation));
            preparation
        };
        let result = self.run_terminal_session_preparation(target);
        self.lock_state()
            .pending_preparations
            .remove(&preparation_key);
        preparation.finish(result.clone());
        result
    }

    fn run_terminal_session_preparation(
        &self,
        target: &TerminalSessionPreparationTarget,
    ) -> Result<PreparedTerminalSession, String> {
        if target.session_type == SessionType::Terminal {
            return Ok(PreparedTerminalSession {
                is_prepared: false,
                skipped_reason: Some(TERMINAL_SESSION_SKIPPED_REASON.to_string()),
                herdr_workspace_id: None,
                herdr_panes: None,
            });
        }
        // 화면이 같은 task의 터미널을 만드는 중이면 그 attach가 세션을 만들므로 끝나기를 기다린 뒤 확인만 한다
        self.wait_for_pending_terminal_creation(&build_terminal_key(&target.task_id, None));
        let worktree_path = target
            .worktree_path
            .as_deref()
            .filter(|path| !path.is_empty());
        let working_directory = match (worktree_path, target.ssh_host.is_some()) {
            (Some(worktree_path), _) => worktree_path.to_string(),
            (None, true) => "~".to_string(),
            (None, false) => self.dependencies.environment.home_directory.clone(),
        };
        if target.session_type == SessionType::Herdr {
            return self.prepare_herdr_space(target, &working_directory);
        }
        if let Some(ssh_host) = target.ssh_host.as_deref() {
            return self.prepare_remote_multiplexer_session(target, ssh_host);
        }
        self.prepare_local_multiplexer_session(target, worktree_path, &working_directory)
    }

    /// 떠 있는 PTY가 없고 만들어지는 중일 때만 기다린다
    fn wait_for_pending_terminal_creation(&self, terminal_key: &str) {
        let pending_creation = {
            let state = self.lock_state();
            match state.active_terminals.get(terminal_key) {
                Some(_) => None,
                None => state.pending_creations.get(terminal_key).cloned(),
            }
        };
        if let Some(pending_creation) = pending_creation {
            let _ = pending_creation.wait();
        }
    }

    fn prepare_herdr_space(
        &self,
        target: &TerminalSessionPreparationTarget,
        working_directory: &str,
    ) -> Result<PreparedTerminalSession, String> {
        let herdr = &self.dependencies.herdr;
        let ssh_host = target.ssh_host.as_deref();
        let workspace_id = herdr
            .ensure_session(
                &target.session_name,
                working_directory,
                target.pane_layout.as_ref(),
                ssh_host,
            )
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Herdr Space를 준비하지 못했습니다.".to_string())?;
        let herdr_panes = herdr
            .list_task_pane_tab_ids(&target.session_name, ssh_host)
            .map_err(|error| error.to_string())?;
        Ok(PreparedTerminalSession {
            is_prepared: true,
            skipped_reason: None,
            herdr_workspace_id: Some(workspace_id),
            herdr_panes: Some(herdr_panes),
        })
    }

    fn prepare_remote_multiplexer_session(
        &self,
        target: &TerminalSessionPreparationTarget,
        ssh_host: &str,
    ) -> Result<PreparedTerminalSession, String> {
        let prepare_command = build_remote_session_prepare_command(
            target.session_type,
            &target.session_name,
            target.worktree_path.as_deref(),
            target.pane_layout.as_ref(),
        );
        let options = ExecGitOptions {
            timeout: Some(TMUX_BOOTSTRAP_TIMEOUT),
            ..ExecGitOptions::default()
        };
        self.dependencies
            .runner
            .exec_git(
                &build_remote_shell_command(&prepare_command),
                Some(ssh_host),
                &options,
            )
            .map_err(|error| error.to_string())?;
        Ok(prepared())
    }

    fn prepare_local_multiplexer_session(
        &self,
        target: &TerminalSessionPreparationTarget,
        worktree_path: Option<&str>,
        working_directory: &str,
    ) -> Result<PreparedTerminalSession, String> {
        let runner = self.dependencies.runner.as_ref();
        let session_name = target.session_name.as_str();
        let creation = match target.session_type {
            SessionType::Tmux if !is_tmux_session_alive(runner, session_name) => {
                bootstrap_local_tmux_session(
                    runner,
                    session_name,
                    working_directory,
                    target.pane_layout.as_ref(),
                )
            }
            SessionType::Zellij if !is_zellij_session_alive(runner, session_name) => {
                create_local_zellij_session_in_background(
                    runner,
                    session_name,
                    worktree_path,
                    working_directory,
                )
            }
            _ => Ok(()),
        };
        creation.map_err(|error| error.to_string())?;
        Ok(prepared())
    }
}
