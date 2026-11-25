//! daemon으로 셸 명령 하나를 실행한다(TS `HostConnection.execute`/`createProcess`).
//!
//! stdin은 32 KiB 조각마다 `exec.stdin-ready`를 받은 뒤 다음 조각을 보내 큰 업로드도 메모리를 묶는다.
//! `remote_stdin_responder`가 있으면 원격이 `marker\t<payload>` 줄을 낼 때 한 번 응답해 그 값을 stdin으로 보낸다.
//! 종료 코드가 0이 아니거나 연결이 끊기면 stdout·stderr를 담은 오류를 돌려주고, 결과를 모르는 명령은 다시 보내지 않는다.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use kanvibe_process::exec::{ExecGitError, RemoteStdinResponder};
use kanvibe_remote_agent::protocol::{
    HOST_CHUNK_BYTES, HostRecord, decode_host_bytes, encode_host_bytes, record_integer, record_text,
};
use serde_json::json;

use super::HostConnection;

const DEFAULT_TIMEOUT: Duration = Duration::from_millis(45_000);
const DEFAULT_OUTPUT_BYTES: usize = 10 * 1024 * 1024;

/// TS `ExecuteOptions`. 값이 없으면 TS 기본값(45초, 10 MiB)을 쓴다
#[derive(Clone, Default)]
pub struct HostExecuteOptions {
    pub timeout: Option<Duration>,
    pub max_buffer_bytes: Option<usize>,
    pub remote_stdin: Option<String>,
    pub remote_stdin_responder: Option<RemoteStdinResponder>,
}

enum CommandEvent {
    Output {
        is_stdout: bool,
        bytes: Vec<u8>,
    },
    StdinReady,
    Exit {
        code: Option<i64>,
        signal: Option<String>,
        error: Option<String>,
    },
    Disconnected,
    Responded(Result<String, ExecGitError>),
}

/// 명령 하나의 진행 상태
struct CommandRun {
    id: String,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    pending_line: Vec<u8>,
    input: VecDeque<Vec<u8>>,
    is_input_in_flight: bool,
    should_end_input: bool,
    has_responded: bool,
    is_response_complete: bool,
    failure: Option<String>,
    output_limit: usize,
}

impl HostConnection {
    pub fn execute(
        self: &Arc<Self>,
        command: &str,
        options: &HostExecuteOptions,
    ) -> Result<String, ExecGitError> {
        let id = uuid::Uuid::new_v4().to_string();
        let (sender, events) = channel();
        let receiver_sender = sender.clone();
        self.subscribe(
            &id,
            Arc::new(move |record: &HostRecord| {
                if let Some(event) = command_event(record) {
                    let _ = receiver_sender.send(event);
                }
            }),
        )
        .map_err(ExecGitError::new)?;
        let timeout = options.timeout.unwrap_or(DEFAULT_TIMEOUT);
        let output_limit = options.max_buffer_bytes.unwrap_or(DEFAULT_OUTPUT_BYTES);
        let open = json!({ "type": "exec.open", "id": id, "command": command, "timeoutMs": timeout.as_millis() as u64, "maxBufferBytes": output_limit, "interactive": true });
        if let Err(error) = self.send(open) {
            self.unsubscribe(&id);
            return Err(ExecGitError::new(error));
        }
        let mut run = CommandRun::new(id.clone(), output_limit);
        if let Some(stdin) = &options.remote_stdin {
            run.queue_input(stdin.as_bytes());
        }
        run.should_end_input = options.remote_stdin_responder.is_none();
        run.pump_input(self);
        let outcome = self.drive(
            &mut run,
            &events,
            &sender,
            options,
            Instant::now() + timeout,
        );
        self.unsubscribe(&id);
        run.finish(outcome, options.remote_stdin_responder.as_ref())
    }

    fn drive(
        self: &Arc<Self>,
        run: &mut CommandRun,
        events: &Receiver<CommandEvent>,
        sender: &std::sync::mpsc::Sender<CommandEvent>,
        options: &HostExecuteOptions,
        deadline: Instant,
    ) -> (Option<i64>, Option<String>) {
        loop {
            let event =
                match events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(event) => event,
                    Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                        let _ = self.send(json!({ "type": "exec.cancel", "id": run.id }));
                        return (None, Some("SIGTERM".into()));
                    }
                };
            match event {
                CommandEvent::Exit {
                    code,
                    signal,
                    error,
                } => {
                    if let Some(error) = error {
                        run.failure.get_or_insert(error);
                    }
                    return (code, signal);
                }
                CommandEvent::Disconnected => {
                    run.failure.get_or_insert(
                        "Host transport disconnected; command outcome may be unknown".into(),
                    );
                    return (None, None);
                }
                event => {
                    if !run.apply(self, event, sender, options) {
                        let _ = self.send(json!({ "type": "exec.cancel", "id": run.id }));
                        return (None, Some("SIGTERM".into()));
                    }
                }
            }
        }
    }
}

fn command_event(record: &HostRecord) -> Option<CommandEvent> {
    match record_text(record, "type")? {
        "disconnected" => Some(CommandEvent::Disconnected),
        "exec.stdin-ready" => Some(CommandEvent::StdinReady),
        "exec.data" => {
            let bytes = decode_host_bytes(record.get("data"), HOST_CHUNK_BYTES).ok()?;
            Some(CommandEvent::Output {
                is_stdout: record_text(record, "stream") == Some("stdout"),
                bytes,
            })
        }
        "exec.exit" => Some(CommandEvent::Exit {
            code: record_integer(record, "code"),
            signal: record_text(record, "signal").map(str::to_string),
            error: record_text(record, "error").map(str::to_string),
        }),
        _ => None,
    }
}

impl CommandRun {
    fn new(id: String, output_limit: usize) -> Self {
        Self {
            id,
            stdout: Vec::new(),
            stderr: Vec::new(),
            pending_line: Vec::new(),
            input: VecDeque::new(),
            is_input_in_flight: false,
            should_end_input: false,
            has_responded: false,
            is_response_complete: false,
            failure: None,
            output_limit,
        }
    }

    fn queue_input(&mut self, bytes: &[u8]) {
        self.input
            .extend(bytes.chunks(HOST_CHUNK_BYTES).map(<[u8]>::to_vec));
    }

    /// 앞 조각의 ACK를 받은 뒤에만 다음 조각을 보낸다. 다 보냈고 끝낼 차례면 stdin을 닫는다
    fn pump_input(&mut self, connection: &HostConnection) {
        if self.is_input_in_flight {
            return;
        }
        if let Some(chunk) = self.input.pop_front() {
            self.is_input_in_flight = true;
            let _ = connection.send(
                json!({ "type": "exec.stdin", "id": self.id, "data": encode_host_bytes(&chunk) }),
            );
        } else if self.should_end_input {
            self.should_end_input = false;
            let _ = connection
                .send(json!({ "type": "exec.stdin", "id": self.id, "data": "", "end": true }));
        }
    }

    /// 이벤트 하나를 반영한다. 출력 상한을 넘거나 응답기가 실패하면 false를 돌려 명령을 취소하게 한다
    fn apply(
        &mut self,
        connection: &Arc<HostConnection>,
        event: CommandEvent,
        sender: &std::sync::mpsc::Sender<CommandEvent>,
        options: &HostExecuteOptions,
    ) -> bool {
        match event {
            CommandEvent::StdinReady => {
                self.is_input_in_flight = false;
                self.pump_input(connection);
            }
            CommandEvent::Output { is_stdout, bytes } => {
                if self.stdout.len() + self.stderr.len() + bytes.len() > self.output_limit {
                    self.failure = Some("Host command output exceeded maxBuffer".into());
                    return false;
                }
                if is_stdout {
                    self.watch_marker(&bytes, sender, options);
                    self.stdout.extend_from_slice(&bytes);
                } else {
                    self.stderr.extend_from_slice(&bytes);
                }
            }
            CommandEvent::Responded(Ok(response)) => {
                self.is_response_complete = true;
                self.queue_input(response.as_bytes());
                self.should_end_input = true;
                self.pump_input(connection);
            }
            CommandEvent::Responded(Err(error)) => {
                self.failure = Some(error.message);
                return false;
            }
            CommandEvent::Exit { .. } | CommandEvent::Disconnected => {}
        }
        true
    }

    /// stdout에서 `marker\t` 줄을 처음 찾으면 응답기를 다른 스레드에서 부른다(응답기가 같은 호스트 명령을 다시 쓸 수 있다)
    fn watch_marker(
        &mut self,
        bytes: &[u8],
        sender: &std::sync::mpsc::Sender<CommandEvent>,
        options: &HostExecuteOptions,
    ) {
        let Some(responder) = options
            .remote_stdin_responder
            .clone()
            .filter(|_| !self.has_responded)
        else {
            return;
        };
        self.pending_line.extend_from_slice(bytes);
        let prefix = format!("{}\t", responder.marker);
        while let Some(newline) = self.pending_line.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending_line.drain(..=newline).collect();
            let line = String::from_utf8_lossy(&line[..line.len() - 1]).into_owned();
            let Some(payload) = line.strip_prefix(&prefix) else {
                continue;
            };
            self.has_responded = true;
            let (payload, sender, respond) = (
                payload.to_string(),
                sender.clone(),
                Arc::clone(&responder.respond),
            );
            std::thread::spawn(move || {
                let _ = sender.send(CommandEvent::Responded(respond(&payload)));
            });
            return;
        }
    }

    fn finish(
        self,
        (code, signal): (Option<i64>, Option<String>),
        responder: Option<&RemoteStdinResponder>,
    ) -> Result<String, ExecGitError> {
        let stdout = String::from_utf8_lossy(&self.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&self.stderr).into_owned();
        let is_responder_incomplete =
            responder.is_some() && !(self.has_responded && self.is_response_complete);
        if self.failure.is_none() && code == Some(0) && !is_responder_incomplete {
            let visible = match responder {
                Some(responder) => {
                    let prefix = format!("{}\t", responder.marker);
                    stdout
                        .split('\n')
                        .filter(|line| !line.starts_with(&prefix))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
                None => stdout,
            };
            return Ok(visible.trim().to_string());
        }
        let exit_description = signal
            .clone()
            .or_else(|| code.map(|code| code.to_string()))
            .unwrap_or_else(|| "unknown".into());
        let message = self.failure.clone().unwrap_or_else(|| {
            if stderr.is_empty() {
                format!("Remote command exited with {exit_description}")
            } else {
                stderr.clone()
            }
        });
        Err(ExecGitError {
            message,
            stdout,
            stderr,
            exit_code: code.map(|code| code as i32),
            signal: signal.as_deref().and_then(static_signal_name),
            has_remote_command_completed: code.is_some(),
            ..ExecGitError::default()
        })
    }
}

fn static_signal_name(signal: &str) -> Option<&'static str> {
    [
        "SIGHUP", "SIGINT", "SIGQUIT", "SIGKILL", "SIGPIPE", "SIGTERM", "SIGABRT", "SIGSEGV",
    ]
    .into_iter()
    .find(|name| *name == signal)
}
