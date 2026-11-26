//! 원격 Herdr 서버를 로컬 Herdr의 machine으로 등록하고, 등록을 막는 이전 방식 서버를 live handoff로 교체한다.

use kanvibe_process::exec::ExecGitOptions;
use serde_json::Value;

use super::response::parse_json;
use super::{
    HERDR_COMMAND_TIMEOUT, HERDR_START_TIMEOUT, HerdrError, HerdrService, herdr_command,
    parse_target,
};

fn command_options(timeout: std::time::Duration) -> ExecGitOptions {
    ExecGitOptions {
        timeout: Some(timeout),
        ..ExecGitOptions::default()
    }
}

fn is_registered_machine(machine: &Value, ssh_host: &str, server_name: &str) -> bool {
    machine.get("target").and_then(Value::as_str) == Some(ssh_host)
        && machine.get("session").and_then(Value::as_str) == Some(server_name)
}

impl HerdrService {
    /// TS `ensureHerdrRemoteMachine`. 이미 같은 호스트·세션의 프로필이 있으면 사용자가 바꾼 이름과 활성화 상태를 건드리지 않는다.
    /// 앞선 등록의 성공 여부와 상관없이 등록은 하나씩 실행한다
    pub fn ensure_herdr_remote_machine(
        &self,
        session_name: &str,
        ssh_host: &str,
    ) -> Result<(), HerdrError> {
        let server_name = parse_target(session_name)?.server_name;
        let _registration = self
            .remote_machine_registration
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let list_command = herdr_command(&server_name, &["machine", "list", "--json"])?;
        let output =
            self.runner
                .exec_git(&list_command, None, &command_options(HERDR_COMMAND_TIMEOUT))?;
        let Value::Array(machines) = parse_json(&output)? else {
            return Err(HerdrError::Message(
                "Herdr machine 목록이 올바르지 않습니다.".to_string(),
            ));
        };
        if machines
            .iter()
            .any(|machine| is_registered_machine(machine, ssh_host, &server_name))
        {
            return Ok(());
        }
        if self.is_remote_server_handoff_required(&server_name, ssh_host)? {
            return Err(HerdrError::ServerHandoffRequired {
                ssh_host: ssh_host.to_string(),
                herdr_session: server_name,
            });
        }
        let add_arguments = ["machine", "add", ssh_host, "--remote-session", &server_name];
        let add_command = format!(
            "{} </dev/null",
            herdr_command(&server_name, &add_arguments)?
        );
        self.runner
            .exec_git(&add_command, None, &command_options(HERDR_START_TIMEOUT))?;
        Ok(())
    }

    /// 이전 KanVibe가 nohup으로 띄운 서버는 세션 리더가 아니어서 Herdr가 machine 등록을 거부한다.
    /// 그런 서버라도 live handoff를 지원하면 pane 프로세스를 살린 채 Herdr가 직접 세션 리더로 다시 띄울 수 있으므로 교체 대상으로 본다.
    /// live handoff를 지원하지 않는 서버는 교체할 수 없으니 등록 실패로 남긴다.
    fn is_remote_server_handoff_required(
        &self,
        server_name: &str,
        ssh_host: &str,
    ) -> Result<bool, HerdrError> {
        let command = herdr_command(server_name, &["status", "server", "--json"])?;
        let output = self.runner.exec_git(
            &command,
            Some(ssh_host),
            &command_options(HERDR_COMMAND_TIMEOUT),
        )?;
        let status = parse_json(&output)?;
        let capabilities = status.get("capabilities");
        let capability = |name: &str| {
            capabilities
                .and_then(|capabilities| capabilities.get(name))
                .and_then(Value::as_bool)
        };
        Ok(status.get("running").and_then(Value::as_bool) == Some(true)
            && capability("detached_server_daemon") == Some(false)
            && capability("live_handoff") == Some(true))
    }

    /// TS `handoffHerdrRemoteServer`. 원격 서버의 pane 프로세스를 새 서버로 넘긴다. 연결된 Herdr client는 이때 끊기므로 호출자가 다시 연결해야 한다
    pub fn handoff_herdr_remote_server(
        &self,
        herdr_session: &str,
        ssh_host: &str,
    ) -> Result<(), HerdrError> {
        let command = herdr_command(herdr_session, &["server", "live-handoff"])?;
        self.runner.exec_git(
            &command,
            Some(ssh_host),
            &command_options(HERDR_START_TIMEOUT),
        )?;
        Ok(())
    }
}
