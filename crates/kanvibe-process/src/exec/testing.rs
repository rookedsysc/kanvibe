//! 테스트용 실행기. TS 테스트가 `vi.mock("@/lib/gitOperations")`로 바꾸던 `execGit`의 자리다.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use super::{ExecGitError, ExecGitOptions, ShellCommandRunner};
use crate::shell_environment::ShellEnvironment;

/// 실행기에 들어온 호출 하나
#[derive(Debug, Clone)]
pub(crate) struct RecordedCommand {
    pub command: String,
    pub ssh_host: Option<String>,
    pub options: ExecGitOptions,
}

type CommandHandler = dyn Fn(&RecordedCommand) -> Result<String, ExecGitError> + Send + Sync;

/// 호출을 기록하고 handler가 정한 결과를 돌려준다
pub(crate) struct ScriptedCommandRunner {
    calls: Mutex<Vec<RecordedCommand>>,
    handler: Box<CommandHandler>,
}

impl ScriptedCommandRunner {
    pub fn new(
        handler: impl Fn(&RecordedCommand) -> Result<String, ExecGitError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            handler: Box::new(handler),
        }
    }

    /// 모든 호출에 같은 stdout을 돌려준다
    pub fn returning(output: &str) -> Self {
        let output = output.to_string();
        Self::new(move |_| Ok(output.clone()))
    }

    pub fn calls(&self) -> Vec<RecordedCommand> {
        self.calls.lock().unwrap().clone()
    }

    pub fn commands(&self) -> Vec<String> {
        self.calls().into_iter().map(|call| call.command).collect()
    }
}

impl ShellCommandRunner for ScriptedCommandRunner {
    fn exec_git(
        &self,
        command: &str,
        ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<String, ExecGitError> {
        let call = RecordedCommand {
            command: command.to_string(),
            ssh_host: ssh_host.map(str::to_string),
            options: options.clone(),
        };
        self.calls.lock().unwrap().push(call.clone());
        (self.handler)(&call)
    }
}

/// ssh_host와 상관없이 이 머신의 `sh -c`로 실행해 원격 셸을 흉내 낸다.
/// `remote_stdin`과 stdin 응답기를 실제 원격 전송처럼 처리하므로 원격 명령 문자열을 그대로 검증할 수 있다.
pub(crate) struct LoopbackShellRunner {
    environment: ShellEnvironment,
}

impl LoopbackShellRunner {
    /// 사용자 HOME이 새지 않도록 PATH만 물려받고 HOME은 테스트가 정한 경로를 쓴다
    pub fn new(home_directory: &str) -> Self {
        let mut environment = ShellEnvironment::new();
        environment.insert("PATH".into(), std::env::var("PATH").unwrap_or_default());
        environment.insert("HOME".into(), home_directory.to_string());
        Self { environment }
    }
}

/// stdout을 줄 단위로 읽다가 응답기 marker 줄이 오면 한 번 응답하고 stdin을 닫는다
fn read_stdout_with_responder(
    stdout: impl Read,
    mut stdin: Option<std::process::ChildStdin>,
    options: &ExecGitOptions,
) -> (String, Option<ExecGitError>) {
    let mut output = String::new();
    let mut callback_error = None;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        output.push_str(&line);
        let responder = options.remote_stdin_responder.as_ref();
        let payload = responder.and_then(|responder| {
            line.strip_prefix(&format!("{}\t", responder.marker))
                .filter(|_| line.ends_with('\n'))
                .map(|payload| (responder, payload.trim_end_matches('\n').to_string()))
        });
        if let (Some((responder, payload)), Some(mut input)) = (payload, stdin.take()) {
            match (responder.respond)(&payload) {
                Ok(reply) => drop(input.write_all(reply.as_bytes())),
                Err(error) => callback_error = Some(error),
            }
        }
        line.clear();
    }
    (output, callback_error)
}

impl ShellCommandRunner for LoopbackShellRunner {
    fn exec_git(
        &self,
        command: &str,
        _ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<String, ExecGitError> {
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(command)
            .env_clear()
            .envs(&self.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| ExecGitError::new(error.to_string()))?;
        let mut stdin = child.stdin.take();
        if options.remote_stdin_responder.is_none()
            && let Some(mut input) = stdin.take()
        {
            let remote_stdin = options.remote_stdin.clone().unwrap_or_default();
            drop(input.write_all(remote_stdin.as_bytes()));
        }
        let mut stderr_stream = child.stderr.take().expect("piped stderr");
        let stderr_reader = std::thread::spawn(move || {
            let mut stderr = String::new();
            let _ = stderr_stream.read_to_string(&mut stderr);
            stderr
        });
        let stdout_stream = child.stdout.take().expect("piped stdout");
        let (stdout, callback_error) = read_stdout_with_responder(stdout_stream, stdin, options);
        let status = child
            .wait()
            .map_err(|error| ExecGitError::new(error.to_string()))?;
        let stderr = stderr_reader.join().unwrap_or_default();
        if status.success() {
            return Ok(stdout.trim().to_string());
        }
        Err(callback_error.unwrap_or_else(|| ExecGitError {
            message: format!("Command failed: {command}\n{stderr}"),
            exit_code: status.code(),
            stdout,
            stderr,
            has_remote_command_completed: true,
            ..ExecGitError::default()
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::exec::RemoteStdinResponder;

    #[test]
    fn scripted_runner_records_calls() {
        let runner = ScriptedCommandRunner::returning("ok");

        let output = runner.exec_git("git status", Some("remote"), &ExecGitOptions::default());

        assert_eq!(output.unwrap(), "ok");
        assert_eq!(runner.commands(), ["git status"]);
        assert_eq!(runner.calls()[0].ssh_host.as_deref(), Some("remote"));
        assert!(!runner.calls()[0].options.is_retry_safe);
    }

    #[test]
    fn loopback_runner_pipes_remote_stdin() {
        let runner = LoopbackShellRunner::new("/nonexistent-home");
        let options = ExecGitOptions {
            remote_stdin: Some("secret".into()),
            ..ExecGitOptions::default()
        };

        let output = runner.exec_git("cat; printf ' %s' \"$HOME\"", Some("remote"), &options);

        assert_eq!(output.unwrap(), "secret /nonexistent-home");
    }

    #[test]
    fn loopback_runner_answers_one_responder_request() {
        let runner = LoopbackShellRunner::new("/nonexistent-home");
        let options = ExecGitOptions {
            remote_stdin_responder: Some(RemoteStdinResponder {
                marker: "__READY__".into(),
                respond: Arc::new(|payload| Ok(format!("{payload}-reply\n"))),
            }),
            ..ExecGitOptions::default()
        };

        let output = runner.exec_git(
            "printf '__READY__\\trequest\\n'; IFS= read -r reply; printf '%s' \"$reply\"",
            None,
            &options,
        );

        assert_eq!(output.unwrap(), "__READY__\trequest\nrequest-reply");
    }
}
