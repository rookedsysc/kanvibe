//! 테스트 공용 가짜. 명령 문자열을 기록하고 등록한 응답기 순서대로 결과를 정하는 `exec_git` 실행기다.

use std::sync::{Arc, Mutex};

use kanvibe_process::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};

/// 실행기가 받은 호출 하나
#[derive(Debug, Clone)]
pub(crate) struct RecordedCommand {
    pub command: String,
    pub ssh_host: Option<String>,
    pub options: ExecGitOptions,
}

/// 명령과 호스트를 보고 결과를 정한다. `None`이면 다음 응답기에 넘긴다
pub(crate) type CommandResponder =
    dyn Fn(&str, Option<&str>) -> Option<Result<String, ExecGitError>> + Send + Sync;

/// 응답기가 하나도 답하지 않으면 빈 stdout으로 성공한다(vitest `mockResolvedValue("")`와 같다)
#[derive(Default)]
pub(crate) struct ScriptedShellRunner {
    calls: Mutex<Vec<RecordedCommand>>,
    responders: Mutex<Vec<Arc<CommandResponder>>>,
}

impl ScriptedShellRunner {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 뒤에 등록한 응답기를 먼저 본다(나중 설정이 앞 설정을 덮는 vitest mock 관례)
    pub fn respond(
        &self,
        responder: impl Fn(&str, Option<&str>) -> Option<Result<String, ExecGitError>>
        + Send
        + Sync
        + 'static,
    ) {
        self.responders.lock().unwrap().push(Arc::new(responder));
    }

    /// 명령에 `needle`이 들어 있으면 `stdout`으로 성공한다
    pub fn respond_when_contains(&self, needle: &str, stdout: &str) {
        let needle = needle.to_string();
        let stdout = stdout.to_string();
        self.respond(move |command, _| command.contains(&needle).then(|| Ok(stdout.clone())));
    }

    /// 명령에 `needle`이 들어 있으면 `message`로 실패한다
    pub fn fail_when_contains(&self, needle: &str, message: &str) {
        let needle = needle.to_string();
        let message = message.to_string();
        self.respond(move |command, _| {
            command
                .contains(&needle)
                .then(|| Err(ExecGitError::new(message.clone())))
        });
    }

    pub fn calls(&self) -> Vec<RecordedCommand> {
        self.calls.lock().unwrap().clone()
    }

    pub fn commands(&self) -> Vec<String> {
        self.calls().into_iter().map(|call| call.command).collect()
    }

    pub fn clear_calls(&self) {
        self.calls.lock().unwrap().clear();
    }
}

impl ShellCommandRunner for ScriptedShellRunner {
    fn exec_git(
        &self,
        command: &str,
        ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<String, ExecGitError> {
        self.calls.lock().unwrap().push(RecordedCommand {
            command: command.to_string(),
            ssh_host: ssh_host.map(str::to_string),
            options: options.clone(),
        });
        let responders = self.responders.lock().unwrap().clone();
        responders
            .iter()
            .rev()
            .find_map(|responder| responder(command, ssh_host))
            .unwrap_or_else(|| Ok(String::new()))
    }
}
