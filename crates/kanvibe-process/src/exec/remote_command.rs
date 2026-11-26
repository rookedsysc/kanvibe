//! ssh 명령 프로세스 하나의 수명(TS `spawnSSHCommand`).
//! 원격 셸이 끝에 찍는 종료 코드 marker로 원격 명령 완료와 전송 실패를 구분하고, stdout·stderr 합계를 출력 상한으로 막는다.
//! stdin 응답기가 있으면 `marker\t<payload>` 요청 한 줄을 출력에서 떼어 내 한 번만 응답하고, 응답은 stdin으로만 보낸다.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::ssh_process::{
    SshEventSink, SshProcess, SshProcessEvent, SshProcessLauncher, SshSpawnMode,
};
use super::{ExecGitError, RemoteStdinResponder};
use crate::shell_environment::ShellEnvironment;

pub(crate) const REMOTE_COMMAND_EXIT_MARKER: &str = "__KANVIBE_REMOTE_COMMAND_EXIT_7b3f6e5d__";

/// 원격 실행 내부에서 오가는 실패. Node 오류 객체의 `stderr` 속성 유무와 준비 대기 시간 초과 표시를 함께 담는다
#[derive(Debug, Clone)]
pub(crate) struct CommandFailure {
    pub error: Box<ExecGitError>,
    /// 명령 출력(`stdout`·`stderr`)이 붙은 오류다. 붙지 않은 오류는 정규화하지 않고 그대로 돌려준다
    pub has_output: bool,
    /// 공용 연결 준비나 인증 차례를 기다리다 시간이 다 됐다
    pub is_preparation_wait_timeout: bool,
}

impl CommandFailure {
    pub fn plain(error: ExecGitError) -> Self {
        Self {
            error: Box::new(error),
            has_output: false,
            is_preparation_wait_timeout: false,
        }
    }

    pub fn message(message: impl Into<String>) -> Self {
        Self::plain(ExecGitError::new(message))
    }

    fn with_output(mut error: ExecGitError, stdout: &str, stderr: &str) -> Self {
        error.stdout = stdout.to_string();
        error.stderr = stderr.to_string();
        Self {
            error: Box::new(error),
            has_output: true,
            is_preparation_wait_timeout: false,
        }
    }

    pub fn is_timeout(&self) -> bool {
        is_ssh_command_timeout_error(&self.error)
    }
}

pub(crate) fn is_ssh_command_timeout_error(error: &ExecGitError) -> bool {
    error.was_killed || error.signal == Some("SIGTERM") || error.error_code == Some("ETIMEDOUT")
}

pub(crate) fn timeout_error(timeout: Duration, stdout: &str, stderr: &str) -> CommandFailure {
    let error = ExecGitError {
        message: format!("Command timed out after {}ms", timeout.as_millis()),
        was_killed: true,
        signal: Some("SIGTERM"),
        ..ExecGitError::default()
    };
    CommandFailure::with_output(error, stdout, stderr)
}

pub(crate) fn exit_error(
    code: Option<i32>,
    signal: Option<&'static str>,
    stdout: &str,
    stderr: &str,
) -> CommandFailure {
    let status = match (signal, code) {
        (Some(signal), _) => signal.to_string(),
        (None, Some(code)) => code.to_string(),
        (None, None) => "unknown".to_string(),
    };
    let error = ExecGitError {
        message: format!("Command failed: ssh exited with {status}"),
        exit_code: code,
        signal,
        ..ExecGitError::default()
    };
    CommandFailure::with_output(error, stdout, stderr)
}

pub(crate) fn attach_output(error: ExecGitError, stdout: &str, stderr: &str) -> CommandFailure {
    CommandFailure::with_output(error, stdout, stderr)
}

pub(crate) use crate::worktree::quote_for_posix_shell;

/// 원격 로그인 셸에서 명령을 돌리고 종료 코드를 marker 줄로 찍는 래퍼
pub(crate) fn build_remote_shell_command(command: &str) -> String {
    let wrapped_command = [
        format!("sh -lc {}", quote_for_posix_shell(command)),
        "__kanvibe_status=$?".to_string(),
        format!("printf '\\n{REMOTE_COMMAND_EXIT_MARKER}:%s\\n' \"$__kanvibe_status\""),
        "exit \"$__kanvibe_status\"".to_string(),
    ]
    .join("; ");
    format!("sh -lc {}", quote_for_posix_shell(&wrapped_command))
}

/// 문자열 끝 `max_length` 바이트. 문자 중간에서 자르지 않도록 시작을 다음 문자 경계로 민다
fn trailing_slice(value: &str, max_length: usize) -> &str {
    let mut start = value.len().saturating_sub(max_length);
    while !value.is_char_boundary(start) {
        start += 1;
    }
    &value[start..]
}

/// 마지막 marker를 기준으로 종료 코드를 읽는다. 조각 경계에 걸친 marker와 `\r\n`도 이어서 해석한다
#[derive(Default)]
pub(crate) struct CompletionParser {
    tail: String,
    length: usize,
    marker_index: Option<usize>,
    digits: String,
    is_awaiting_newline: bool,
    is_invalid: bool,
}

fn marker_prefix() -> String {
    format!("\n{REMOTE_COMMAND_EXIT_MARKER}:")
}

impl CompletionParser {
    /// 완료가 확정되면 marker 위치(바이트)와 종료 코드를 돌려준다
    pub fn feed(&mut self, chunk: &str) -> Option<(usize, i32)> {
        let prefix = marker_prefix();
        let search = format!("{}{chunk}", self.tail);
        let mut suffix = chunk.to_string();
        if let Some(found) = search.rfind(&prefix) {
            self.marker_index = Some(self.length - self.tail.len() + found);
            self.digits.clear();
            self.is_awaiting_newline = false;
            self.is_invalid = false;
            suffix = search[found + prefix.len()..].to_string();
        }
        self.length += chunk.len();
        self.tail = trailing_slice(&search, prefix.len() - 1).to_string();
        let marker_index = self.marker_index?;
        if self.is_invalid || suffix.is_empty() {
            return None;
        }
        self.read_exit_code(&suffix)
            .map(|exit_code| (marker_index, exit_code))
    }

    fn read_exit_code(&mut self, suffix: &str) -> Option<i32> {
        let mut rest = suffix;
        if !self.is_awaiting_newline {
            let digit_length = rest.bytes().take_while(u8::is_ascii_digit).count();
            self.digits.push_str(&rest[..digit_length]);
            rest = &rest[digit_length..];
            if rest.is_empty() {
                return None;
            }
            if let Some(after_return) = rest.strip_prefix('\r') {
                self.is_awaiting_newline = true;
                rest = after_return;
                if rest.is_empty() {
                    return None;
                }
            }
        }
        if !self.digits.is_empty() && rest.starts_with('\n') {
            let code = self.digits.parse::<u64>().unwrap_or(u64::MAX);
            return Some(i32::try_from(code).unwrap_or(i32::MAX));
        }
        self.is_invalid = true;
        None
    }
}

pub(crate) struct SshCommandRequest {
    pub args: Vec<String>,
    pub timeout: Duration,
    pub max_buffer_bytes: usize,
    pub remote_stdin: Option<String>,
    pub responder: Option<RemoteStdinResponder>,
}

enum CommandEvent {
    Process(SshProcessEvent),
    Reply(Result<String, ExecGitError>),
}

/// 명령 프로세스 하나의 진행 상태
struct SshCommandRun {
    process: Box<dyn SshProcess>,
    request_timeout: Duration,
    max_buffer_bytes: usize,
    responder: Option<RemoteStdinResponder>,
    replies: Arc<Mutex<mpsc::Sender<CommandEvent>>>,
    stdout: String,
    stderr: String,
    completion: CompletionParser,
    response_search_tail: String,
    response_start: Option<usize>,
    pending_payload: Option<String>,
    has_responded: bool,
    is_response_complete: bool,
    responder_error: Option<ExecGitError>,
    has_exited: bool,
    is_stdin_ended: bool,
    has_killed: bool,
}

type Settlement = Result<String, CommandFailure>;

impl SshCommandRun {
    fn output_bytes(&self) -> usize {
        self.stdout.len() + self.stderr.len()
    }

    fn handle_event(&mut self, event: CommandEvent) -> Option<Settlement> {
        match event {
            CommandEvent::Process(SshProcessEvent::Stdout(chunk)) => self.handle_stdout(&chunk),
            CommandEvent::Process(SshProcessEvent::Stderr(chunk)) => {
                self.stderr.push_str(&chunk);
                self.reject_if_output_is_too_large()
            }
            CommandEvent::Process(SshProcessEvent::Exit(_)) => {
                self.has_exited = true;
                None
            }
            CommandEvent::Process(SshProcessEvent::Close { code, signal }) => {
                self.has_exited = true;
                if code == Some(0) {
                    return Some(self.finish_with_success(self.stdout.trim().to_string()));
                }
                let failure = exit_error(code, signal, &self.stdout, &self.stderr);
                Some(self.finish_with_error(failure))
            }
            CommandEvent::Process(SshProcessEvent::Error(error)) => {
                let failure = attach_output(error, &self.stdout, &self.stderr);
                Some(self.finish_with_error(failure))
            }
            CommandEvent::Reply(reply) => {
                self.handle_reply(reply);
                None
            }
        }
    }

    fn reject_if_output_is_too_large(&mut self) -> Option<Settlement> {
        if self.output_bytes() <= self.max_buffer_bytes {
            return None;
        }
        let error = ExecGitError::new("SSH command output exceeded maxBuffer");
        let failure = attach_output(error, &self.stdout, &self.stderr);
        Some(self.finish_with_error(failure))
    }

    fn handle_stdout(&mut self, chunk: &str) -> Option<Settlement> {
        let previous_length = self.stdout.len();
        self.stdout.push_str(chunk);
        if let Some(settlement) = self.reject_if_output_is_too_large() {
            return Some(settlement);
        }
        let parsed = match self.responder.clone() {
            Some(responder) if !self.has_responded => {
                self.extract_response_request(&responder.marker, chunk, previous_length)
            }
            _ => self.completion.feed(chunk),
        };
        let (marker_index, exit_code) = parsed?;
        let completed_stdout = self.stdout[..marker_index].to_string();
        Some(self.finish_with_remote_completion(exit_code, completed_stdout))
    }

    /// 요청 줄을 찾으면 출력에서 떼어 내고 응답을 예약한다. 찾지 못하면 평소처럼 완료 marker를 본다
    fn extract_response_request(
        &mut self,
        marker: &str,
        chunk: &str,
        previous_length: usize,
    ) -> Option<(usize, i32)> {
        let response_prefix = format!("{marker}\t");
        let search = format!("{}{chunk}", self.response_search_tail);
        let search_offset = previous_length - self.response_search_tail.len();
        if self.response_start.is_none() {
            if search_offset == 0 && search.starts_with(&response_prefix) {
                self.response_start = Some(0);
            } else if let Some(found) = search.find(&format!("\n{response_prefix}")) {
                self.response_start = Some(search_offset + found + 1);
            }
        }
        self.response_search_tail = trailing_slice(&search, response_prefix.len()).to_string();
        let Some(response_start) = self.response_start else {
            return self.completion.feed(chunk);
        };
        let response_end = if response_prefix.contains('\n') {
            self.stdout[response_start..]
                .find('\n')
                .map(|end| response_start + end)
        } else {
            let chunk_start = response_start.saturating_sub(previous_length);
            chunk[chunk_start..]
                .find('\n')
                .map(|end| previous_length + chunk_start + end)
        };
        let Some(response_end) = response_end else {
            return self.completion.feed(chunk);
        };
        self.has_responded = true;
        let payload = self.stdout[response_start + response_prefix.len()..response_end].to_string();
        let remaining = format!(
            "{}{}",
            &self.stdout[..response_start],
            &self.stdout[response_end + 1..]
        );
        self.stdout = remaining;
        self.completion = CompletionParser::default();
        self.pending_payload = Some(payload);
        let remaining = self.stdout.clone();
        self.completion.feed(&remaining)
    }

    fn handle_reply(&mut self, reply: Result<String, ExecGitError>) {
        match reply {
            Ok(content) => {
                self.is_response_complete = true;
                self.end_stdin(Some(&content));
            }
            Err(error) => {
                self.responder_error = Some(error);
                self.end_stdin(None);
            }
        }
    }

    /// 응답 callback은 이미 쌓인 사건을 모두 처리한 뒤에 시작한다. 요청 직후 SSH가 끝났으면 callback을 부르지 않는다
    fn start_pending_response(&mut self) {
        let (Some(payload), Some(responder)) =
            (self.pending_payload.take(), self.responder.clone())
        else {
            return;
        };
        let replies = Arc::clone(&self.replies);
        std::thread::spawn(move || {
            let reply = (responder.respond)(&payload);
            let _ = replies.lock().unwrap().send(CommandEvent::Reply(reply));
        });
    }

    fn end_stdin(&mut self, content: Option<&str>) {
        if self.is_stdin_ended {
            return;
        }
        self.is_stdin_ended = true;
        self.process.end_stdin(content);
    }

    fn kill_if_running(&mut self) {
        if !self.has_exited && !self.has_killed {
            self.has_killed = true;
            self.process.kill();
        }
    }

    fn finish_with_remote_completion(
        &mut self,
        exit_code: i32,
        completed_stdout: String,
    ) -> Settlement {
        if let Some(error) = self.responder_error.clone() {
            return self.finish_with_error(CommandFailure::plain(error));
        }
        if exit_code == 0 {
            return self.finish_with_success(completed_stdout.trim().to_string());
        }
        let mut failure = exit_error(Some(exit_code), None, &completed_stdout, &self.stderr);
        failure.error.has_remote_command_completed = true;
        self.finish_with_error(failure)
    }

    fn finish_with_success(&mut self, output: String) -> Settlement {
        if self.responder_error.is_some() || (self.has_responded && !self.is_response_complete) {
            let error = self.responder_error.clone().unwrap_or_else(|| {
                ExecGitError::new("SSH transaction ended before its input response")
            });
            return self.finish_with_error(CommandFailure::plain(error));
        }
        if self.responder.is_some() && !self.has_responded {
            self.end_stdin(None);
        }
        self.kill_if_running();
        Ok(output)
    }

    fn finish_with_error(&mut self, failure: CommandFailure) -> Settlement {
        if self.responder.is_some() {
            self.end_stdin(None);
        }
        self.kill_if_running();
        match self.responder_error.clone() {
            Some(error) => Err(CommandFailure::plain(error)),
            None => Err(failure),
        }
    }

    fn timeout_failure(&mut self) -> Settlement {
        let failure = timeout_error(self.request_timeout, &self.stdout, &self.stderr);
        self.finish_with_error(failure)
    }
}

/// 이미 도착한 사건을 모두 처리한다. 확정되면 결과를 돌려준다
fn drain_ready_events(
    run: &mut SshCommandRun,
    events: &Receiver<CommandEvent>,
) -> Option<Settlement> {
    loop {
        match events.try_recv() {
            Ok(event) => {
                if let Some(settlement) = run.handle_event(event) {
                    return Some(settlement);
                }
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => return None,
        }
    }
}

fn wait_for_settlement(
    run: &mut SshCommandRun,
    events: &Receiver<CommandEvent>,
    deadline: Instant,
) -> Settlement {
    loop {
        if let Some(settlement) = drain_ready_events(run, events) {
            return settlement;
        }
        run.start_pending_response();
        let remaining = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(remaining) {
            Ok(event) => {
                if let Some(settlement) = run.handle_event(event) {
                    return settlement;
                }
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                return run.timeout_failure();
            }
        }
    }
}

/// ssh 명령을 띄우고 결과가 확정될 때까지 기다린다
pub(crate) fn run_ssh_command(
    launcher: &dyn SshProcessLauncher,
    environment: &ShellEnvironment,
    request: SshCommandRequest,
) -> Result<String, CommandFailure> {
    let deadline = Instant::now() + request.timeout;
    let (sender, receiver) = mpsc::channel();
    let sender = Arc::new(Mutex::new(sender));
    let process_sender = Arc::clone(&sender);
    let sink: SshEventSink = Arc::new(move |event| {
        let _ = process_sender
            .lock()
            .unwrap()
            .send(CommandEvent::Process(event));
    });
    let is_interactive = request.responder.is_some();
    let mode = if request.remote_stdin.is_none() && !is_interactive {
        SshSpawnMode::CommandWithoutStdin
    } else {
        SshSpawnMode::CommandWithStdin
    };
    let process = launcher.spawn(&request.args, mode, environment, sink);
    let mut run = SshCommandRun {
        process,
        request_timeout: request.timeout,
        max_buffer_bytes: request.max_buffer_bytes,
        responder: request.responder,
        replies: sender,
        stdout: String::new(),
        stderr: String::new(),
        completion: CompletionParser::default(),
        response_search_tail: String::new(),
        response_start: None,
        pending_payload: None,
        has_responded: false,
        is_response_complete: false,
        responder_error: None,
        has_exited: false,
        is_stdin_ended: false,
        has_killed: false,
    };
    if mode == SshSpawnMode::CommandWithStdin && !is_interactive {
        let remote_stdin = request.remote_stdin.unwrap_or_default();
        run.end_stdin(Some(&remote_stdin));
    }
    wait_for_settlement(&mut run, &receiver, deadline)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(chunks: &[&str]) -> Option<(usize, i32)> {
        let mut parser = CompletionParser::default();
        chunks.iter().find_map(|chunk| parser.feed(chunk))
    }

    #[test]
    fn completion_parser_reads_the_exit_code_after_the_marker() {
        let marker = REMOTE_COMMAND_EXIT_MARKER;

        assert_eq!(feed_all(&[&format!("out\n{marker}:3\n")]), Some((3, 3)));
        assert_eq!(
            feed_all(&[&format!("out\n{marker}:"), "0\r", "\n"]),
            Some((3, 0))
        );
        assert_eq!(feed_all(&[&format!("out\n{marker}:x\n")]), None);
    }

    #[test]
    fn remote_shell_command_wraps_and_reports_the_status() {
        let wrapped = build_remote_shell_command("printf 'a'");

        assert_eq!(
            wrapped,
            "sh -lc 'sh -lc '\"'\"'printf '\"'\"'\"'\"'\"'\"'\"'\"'a'\"'\"'\"'\"'\"'\"'\"'\"''\"'\"'; __kanvibe_status=$?; printf '\"'\"'\\n__KANVIBE_REMOTE_COMMAND_EXIT_7b3f6e5d__:%s\\n'\"'\"' \"$__kanvibe_status\"; exit \"$__kanvibe_status\"'"
        );
    }
}
