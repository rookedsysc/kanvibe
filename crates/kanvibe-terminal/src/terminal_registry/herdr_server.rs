//! 원격 Herdr 서버를 로컬 Herdr의 machine으로 등록하고, 사용자가 승인한 서버 교체를 실행한다.

use std::sync::Arc;

use kanvibe_domain::terminal_tabs::HERDR_SERVER_REPLACED_CLOSE_REASON;

use super::entry::lock;
use super::{Flight, HerdrSessionError, HerdrSessions, TerminalEventSink, TerminalRegistry};
use crate::herdr::parse_herdr_session_target;

/// TS `JSON.stringify([sshHost, herdrSession])`
fn build_herdr_server_key(ssh_host: &str, herdr_session: &str) -> String {
    serde_json::json!([ssh_host, herdr_session]).to_string()
}

/// Herdr CLI는 거부 사유를 stderr 마지막 줄에 쓰므로 그 줄을 보여 주고, stderr가 없으면 오류 메시지로 대신한다
fn read_herdr_machine_failure_reason(error: &HerdrSessionError) -> String {
    let HerdrSessionError::Command { message, stderr } = error else {
        return error.to_string();
    };
    stderr
        .split('\n')
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or(message)
        .to_string()
}

/// 서버가 이전 방식으로 떠 있으면 교체 여부를 묻도록 화면에 알리고, 그 밖의 실패는 원인과 함께 알린다
fn register_herdr_remote_machine(
    herdr: &dyn HerdrSessions,
    events: &dyn TerminalEventSink,
    herdr_session: &str,
    ssh_host: &str,
) {
    match herdr.ensure_remote_machine(herdr_session, ssh_host) {
        Ok(()) => {}
        Err(HerdrSessionError::ServerHandoffRequired { .. }) => {
            events.herdr_server_handoff_required(ssh_host, herdr_session);
        }
        Err(error) => {
            eprintln!("[kanvibe] Herdr machine 등록 실패 ({ssh_host}/{herdr_session}): {error}");
            events.herdr_machine_registration_failed(
                ssh_host,
                herdr_session,
                &read_herdr_machine_failure_reason(&error),
            );
        }
    }
}

impl TerminalRegistry {
    /// 터미널 연결을 막지 않도록 machine 등록은 뒤에서 돈다(TS `void registerHerdrRemoteMachine`)
    pub(super) fn start_herdr_machine_registration(&self, herdr_session: &str, ssh_host: &str) {
        let herdr = Arc::clone(&self.dependencies.herdr);
        let events = Arc::clone(&self.dependencies.events);
        let herdr_session = herdr_session.to_string();
        let ssh_host = ssh_host.to_string();
        std::thread::spawn(move || {
            register_herdr_remote_machine(
                herdr.as_ref(),
                events.as_ref(),
                &herdr_session,
                &ssh_host,
            );
        });
    }

    /// 같은 서버의 교체가 진행 중이면 끝날 때까지 기다린다
    pub(super) fn wait_for_herdr_server_handoff(&self, ssh_host: &str, herdr_session: &str) {
        let server_key = build_herdr_server_key(ssh_host, herdr_session);
        let handoff = self
            .lock_state()
            .herdr_server_handoffs
            .get(&server_key)
            .cloned();
        if let Some(handoff) = handoff {
            handoff.wait();
        }
    }

    /// 교체 중에는 그 서버에 붙은 Herdr client가 끊기므로, 붙어 있던 화면은 다시 연결하라는 사유로 닫고
    /// 새 연결은 교체가 끝날 때까지 기다리게 한다. 교체가 끝나면 machine 등록을 이어서 한다
    pub(super) fn run_herdr_server_replacement(&self, ssh_host: &str, herdr_session: &str) {
        let server_key = build_herdr_server_key(ssh_host, herdr_session);
        let Some((handoff, connected_terminal_keys)) =
            self.begin_herdr_server_handoff(&server_key, ssh_host, herdr_session)
        else {
            return;
        };
        let result = self
            .dependencies
            .herdr
            .handoff_remote_server(herdr_session, ssh_host);
        for terminal_key in &connected_terminal_keys {
            self.destroy_terminal(terminal_key, Some("herdr-server-replaced"), false);
        }
        self.lock_state().herdr_server_handoffs.remove(&server_key);
        handoff.finish(());
        let (herdr, events) = (
            self.dependencies.herdr.as_ref(),
            self.dependencies.events.as_ref(),
        );
        match result {
            Ok(()) => register_herdr_remote_machine(herdr, events, herdr_session, ssh_host),
            Err(error) => {
                eprintln!("[kanvibe] Herdr 서버 교체 실패 ({ssh_host}/{herdr_session}): {error}");
                events.herdr_machine_registration_failed(
                    ssh_host,
                    herdr_session,
                    &read_herdr_machine_failure_reason(&error),
                );
            }
        }
    }

    /// 교체를 등록하고 그 서버에 붙은 화면에 닫힘 사유를 단다. 이미 교체 중이면 그것이 끝나기를 기다리고 `None`이다
    fn begin_herdr_server_handoff(
        &self,
        server_key: &str,
        ssh_host: &str,
        herdr_session: &str,
    ) -> Option<(Arc<Flight<()>>, Vec<String>)> {
        let mut state = self.lock_state();
        if let Some(running) = state.herdr_server_handoffs.get(server_key).cloned() {
            drop(state);
            running.wait();
            return None;
        }
        let connected_terminal_keys: Vec<String> = state
            .active_terminals
            .iter()
            .filter(|(_, entry)| {
                entry.ssh_host.as_deref() == Some(ssh_host)
                    && parse_herdr_session_target(&entry.session_name)
                        .is_ok_and(|target| target.server_name == herdr_session)
            })
            .map(|(terminal_key, entry)| {
                *lock(&entry.close_reason) = Some(HERDR_SERVER_REPLACED_CLOSE_REASON.to_string());
                terminal_key.clone()
            })
            .collect();
        let handoff = Flight::new();
        state
            .herdr_server_handoffs
            .insert(server_key.to_string(), Arc::clone(&handoff));
        Some((handoff, connected_terminal_keys))
    }
}
