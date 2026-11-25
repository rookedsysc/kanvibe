//! Herdr CLI 실행. 공유 서버 세션의 명령은 그 task의 Space 밖을 가리키지 못하게 막는다.

use std::sync::LazyLock;

use kanvibe_process::exec::ExecGitOptions;
use regex::Regex;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::response::{HerdrSession, decode, parse_json, read_herdr_result};
use super::{HERDR_COMMAND_TIMEOUT, HerdrError, HerdrService, herdr_command, parse_target};

static SPACE_SCOPED_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^w[0-9A-Za-z]+:(?:t|p)[0-9A-Za-z]+$").expect("유효한 정규식"));

/// 다시 실행해도 원격 부작용이 겹치지 않는 읽기 명령만 재시도한다
fn is_read_only_command(arguments: &[String]) -> bool {
    let subcommand = arguments.get(1).map(String::as_str);
    subcommand == Some("list")
        || (arguments.first().map(String::as_str) == Some("pane")
            && matches!(subcommand, Some("get" | "layout")))
}

/// `--pane` 뒤의 값, 없으면 세 번째 인자가 명령이 다룰 탭·pane이다
fn command_target_id(arguments: &[String]) -> Option<&str> {
    let target = match arguments.iter().position(|argument| argument == "--pane") {
        Some(pane_index) => arguments.get(pane_index + 1),
        None => arguments.get(2),
    };
    target.map(String::as_str)
}

fn is_outside_workspace(arguments: &[String], workspace_id: &str) -> bool {
    let is_foreign_target = command_target_id(arguments).is_some_and(|target_id| {
        SPACE_SCOPED_ID_PATTERN.is_match(target_id)
            && !target_id.starts_with(&format!("{workspace_id}:"))
    });
    let workspace_index = arguments
        .iter()
        .position(|argument| argument == "--workspace");
    let is_foreign_workspace = workspace_index
        .is_some_and(|index| arguments.get(index + 1).map(String::as_str) != Some(workspace_id));
    is_foreign_target || is_foreign_workspace
}

impl HerdrService {
    /// TS `runHerdrCommand`. 결과 JSON의 `result`를 돌려준다. 출력 없이 성공한 `pane send-text`는 `null`이다
    pub fn run_herdr_command(
        &self,
        session_name: &str,
        arguments: &[&str],
        ssh_host: Option<&str>,
    ) -> Result<Value, HerdrError> {
        self.run_command(session_name, arguments, ssh_host, true)
    }

    pub(super) fn query<T: DeserializeOwned>(
        &self,
        session_name: &str,
        arguments: &[&str],
        ssh_host: Option<&str>,
    ) -> Result<T, HerdrError> {
        decode(self.run_command(session_name, arguments, ssh_host, true)?)
    }

    pub(super) fn run_command(
        &self,
        session_name: &str,
        arguments: &[&str],
        ssh_host: Option<&str>,
        should_log_failure: bool,
    ) -> Result<Value, HerdrError> {
        let arguments = self.scope_arguments_to_workspace(session_name, arguments, ssh_host)?;
        let argument_refs: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let options = ExecGitOptions {
            timeout: Some(HERDR_COMMAND_TIMEOUT),
            is_retry_safe: is_read_only_command(&arguments),
            is_failure_expected: !should_log_failure,
            ..ExecGitOptions::default()
        };
        let command = herdr_command(session_name, &argument_refs)?;
        let output = self.runner.exec_git(&command, ssh_host, &options)?;
        // send-text는 전송 성공 시 본문을 출력하지 않는다.
        if output.is_empty() && argument_refs.starts_with(&["pane", "send-text"]) {
            return Ok(Value::Null);
        }
        read_herdr_result(&output)
    }

    /// 공유 서버 세션이면 다른 Space의 탭·pane·Space를 가리키는 명령을 거절하고, 탭 목록은 그 Space로 좁힌다
    fn scope_arguments_to_workspace(
        &self,
        session_name: &str,
        arguments: &[&str],
        ssh_host: Option<&str>,
    ) -> Result<Vec<String>, HerdrError> {
        let mut scoped: Vec<String> = arguments.iter().map(ToString::to_string).collect();
        if parse_target(session_name)?.workspace_key.is_none() {
            return Ok(scoped);
        }
        let workspace_id = self
            .get_herdr_workspace_id(session_name, ssh_host)?
            .unwrap_or_default();
        if is_outside_workspace(&scoped, &workspace_id) {
            return Err(HerdrError::Message(
                "다른 Space의 터미널을 변경할 수 없습니다.".to_string(),
            ));
        }
        let has_workspace_flag = scoped.iter().any(|argument| argument == "--workspace");
        if arguments.starts_with(&["tab", "list"]) && !has_workspace_flag {
            scoped.extend(["--workspace".to_string(), workspace_id]);
        }
        Ok(scoped)
    }

    pub(super) fn list_herdr_sessions(
        &self,
        ssh_host: Option<&str>,
    ) -> Result<Vec<HerdrSession>, HerdrError> {
        let command = herdr_command("default", &["session", "list", "--json"])?;
        let options = ExecGitOptions {
            timeout: Some(HERDR_COMMAND_TIMEOUT),
            is_retry_safe: true,
            ..ExecGitOptions::default()
        };
        let output = self.runner.exec_git(&command, ssh_host, &options)?;
        let response = parse_json(&output)?;
        match response.get("sessions") {
            Some(sessions) if sessions.is_array() => decode(sessions.clone()),
            _ => Err(HerdrError::Message(
                "Herdr 세션 목록이 올바르지 않습니다.".to_string(),
            )),
        }
    }

    /// 이름이 같고 실행 중인 Herdr 서버 세션
    pub(super) fn find_running_session(
        &self,
        server_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<HerdrSession, HerdrError> {
        self.list_herdr_sessions(ssh_host)?
            .into_iter()
            .find(|session| session.name == server_name && session.running)
            .ok_or_else(|| HerdrError::Message("Herdr 세션을 찾을 수 없습니다.".to_string()))
    }
}
