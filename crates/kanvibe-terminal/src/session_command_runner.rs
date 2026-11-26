//! AI 세션 리더(`kanvibe_ai`)가 부르는 명령 실행기를 `exec_git` 하나로 잇는다.
//! 로컬은 좁힌 셸 환경으로, 원격은 SSH 연결 풀로 같은 셸 문자열을 실행한다.

use std::sync::Arc;
use std::time::Duration;

use kanvibe_ai::host::{CommandOptions, CommandRunner};
use kanvibe_ai::{AiError, AiResult};
use kanvibe_process::exec::{ExecGitOptions, ShellCommandRunner, is_ssh_connection_error};

pub struct ShellSessionCommandRunner {
    runner: Arc<dyn ShellCommandRunner>,
}

impl ShellSessionCommandRunner {
    pub fn new(runner: Arc<dyn ShellCommandRunner>) -> Self {
        Self { runner }
    }
}

impl CommandRunner for ShellSessionCommandRunner {
    fn run(
        &self,
        command: &str,
        ssh_host: Option<&str>,
        options: CommandOptions,
    ) -> AiResult<String> {
        let exec_options = ExecGitOptions {
            timeout: options.timeout_ms.map(Duration::from_millis),
            max_buffer_bytes: options.max_buffer_bytes,
            ..ExecGitOptions::default()
        };
        self.runner
            .exec_git(command, ssh_host, &exec_options)
            .map_err(|error| AiError::Command {
                is_connection_failure: is_ssh_connection_error(&error),
                message: error.message,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ScriptedShellRunner;

    #[test]
    fn options_and_failures_are_passed_through() {
        let shell = ScriptedShellRunner::new();
        shell.fail_when_contains("broken", "명령 실패");
        let runner = ShellSessionCommandRunner::new(shell.clone());
        let options = CommandOptions {
            max_buffer_bytes: Some(64),
            timeout_ms: Some(1_500),
        };

        let error = runner.run("broken", Some("remote"), options).unwrap_err();

        assert_eq!(error.to_string(), "명령 실패");
        let call = &shell.calls()[0];
        assert_eq!(call.ssh_host.as_deref(), Some("remote"));
        assert_eq!(call.options.timeout, Some(Duration::from_millis(1_500)));
        assert_eq!(call.options.max_buffer_bytes, Some(64));
    }
}
