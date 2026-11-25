//! `exec.*` 레코드로 여는 원격 명령(TS `openCommand`).
//!
//! 명령은 좁힌 환경의 `/bin/sh -lc`로 자기 프로세스 그룹에서 돌고, 취소·시간 초과·출력 상한이면 그룹 전체를
//! SIGKILL한다. 출력은 32 KiB 조각으로 보내되 쓰기 큐가 차면 읽기를 멈춰 메모리를 묶는다. stdin 조각은
//! 자식이 받아 간 뒤 `exec.stdin-ready`로 알려 데스크탑이 큰 업로드를 한 조각씩 보내게 한다.

use std::io::{Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::peer::PeerRequests;
use crate::environment::current_child_environment;
use crate::outbox::{RecordOutbox, STREAM_RESUME_BYTES};
use crate::protocol::{
    HOST_BUFFER_BYTES, HOST_CHUNK_BYTES, HostRecord, MAX_COMMAND_TEXT_BYTES, decode_host_bytes,
    encode_host_bytes, record_from, record_integer, record_text,
};

const MAX_COMMAND_OUTPUT_BYTES: i64 = 256 * 1024 * 1024;
const MAX_COMMAND_TIMEOUT_MS: i64 = 24 * 60 * 60_000;
const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

enum StdinMessage {
    Data(Vec<u8>),
    End,
}

pub(super) struct RunningCommand {
    process_group: i32,
    stdin: Mutex<Option<Sender<StdinMessage>>>,
    queued_stdin_bytes: AtomicUsize,
    error: Mutex<Option<String>>,
}

impl RunningCommand {
    /// 프로세스 그룹 전체를 SIGKILL한다
    pub(super) fn kill(&self) {
        // SAFETY: 이 명령이 만든 프로세스 그룹에만 신호를 보낸다
        unsafe {
            libc::kill(-self.process_group, libc::SIGKILL);
        }
    }

    fn fail(&self, message: &str) {
        self.error
            .lock()
            .unwrap()
            .get_or_insert_with(|| message.to_string());
        self.kill();
    }
}

/// `interactive`(stdin을 열어 둘지)와 처음 넣을 `stdin` 바이트
fn parse_stdin_mode(record: &HostRecord) -> Result<(bool, Option<Vec<u8>>), String> {
    let is_interactive = match record.get("interactive") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("Invalid host interactive mode".into()),
    };
    let initial_stdin = match record.get("stdin") {
        Some(value) => Some(
            decode_host_bytes(Some(value), HOST_CHUNK_BYTES).map_err(|error| error.to_string())?,
        ),
        None => None,
    };
    Ok((is_interactive, initial_stdin))
}

struct CommandBounds {
    timeout: Duration,
    output_limit: usize,
}

fn parse_bounds(record: &HostRecord) -> Result<CommandBounds, String> {
    let timeout = record_integer(record, "timeoutMs")
        .filter(|value| (1..=MAX_COMMAND_TIMEOUT_MS).contains(value));
    let limit = record_integer(record, "maxBufferBytes")
        .filter(|value| (0..=MAX_COMMAND_OUTPUT_BYTES).contains(value));
    let (Some(timeout), Some(limit)) = (timeout, limit) else {
        return Err("Invalid host command bounds".into());
    };
    Ok(CommandBounds {
        timeout: Duration::from_millis(timeout as u64),
        output_limit: limit as usize,
    })
}

pub(super) fn open_command(
    outbox: &Arc<RecordOutbox>,
    requests: &Arc<Mutex<PeerRequests>>,
    record: &HostRecord,
    id: &str,
) -> Result<(), String> {
    if !requests.lock().unwrap().has_room_for(id) {
        return Err("Host request capacity exceeded or duplicate id".into());
    }
    let command = record_text(record, "command")
        .filter(|command| !command.is_empty() && command.len() <= MAX_COMMAND_TEXT_BYTES)
        .ok_or("Invalid host command")?;
    let bounds = parse_bounds(record)?;
    let (is_interactive, initial_stdin) = parse_stdin_mode(record)?;
    let mut child = match spawn_shell(command) {
        Ok(child) => child,
        Err(error) => {
            let _ = outbox.send(&record_from(json!({ "type": "exec.exit", "id": id, "code": null, "signal": null, "error": error })));
            return Ok(());
        }
    };
    let (stdin_sender, stdin_receiver) = channel();
    let running = Arc::new(RunningCommand {
        process_group: child.id() as i32,
        stdin: Mutex::new(Some(stdin_sender)),
        queued_stdin_bytes: AtomicUsize::new(0),
        error: Mutex::new(None),
    });
    requests
        .lock()
        .unwrap()
        .commands
        .insert(id.to_string(), Arc::clone(&running));
    let stdin = child.stdin.take().expect("stdin을 파이프로 열었다");
    spawn_stdin_writer(outbox, requests, &running, id, stdin, stdin_receiver);
    if let Some(bytes) = initial_stdin {
        queue_stdin(&running, StdinMessage::Data(bytes));
    }
    if !is_interactive {
        queue_stdin(&running, StdinMessage::End);
    }
    let context = CommandContext {
        outbox: Arc::clone(outbox),
        requests: Arc::clone(requests),
        running,
        id: id.to_string(),
    };
    std::thread::spawn(move || supervise(context, child, bounds));
    Ok(())
}

fn spawn_shell(command: &str) -> Result<Child, String> {
    Command::new("/bin/sh")
        .args(["-lc", command])
        .env_clear()
        .envs(current_child_environment())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|error| error.to_string())
}

fn queue_stdin(running: &RunningCommand, message: StdinMessage) {
    if let StdinMessage::Data(bytes) = &message {
        let queued = running
            .queued_stdin_bytes
            .fetch_add(bytes.len(), Ordering::SeqCst)
            + bytes.len();
        if queued > HOST_BUFFER_BYTES {
            running.kill();
            return;
        }
    }
    let is_end = matches!(message, StdinMessage::End);
    let mut stdin = running.stdin.lock().unwrap();
    if let Some(sender) = stdin.as_ref() {
        let _ = sender.send(message);
    }
    if is_end {
        stdin.take();
    }
}

fn spawn_stdin_writer(
    outbox: &Arc<RecordOutbox>,
    requests: &Arc<Mutex<PeerRequests>>,
    running: &Arc<RunningCommand>,
    id: &str,
    mut stdin: ChildStdin,
    messages: Receiver<StdinMessage>,
) {
    let (outbox, requests, running, id) = (
        Arc::clone(outbox),
        Arc::clone(requests),
        Arc::clone(running),
        id.to_string(),
    );
    std::thread::spawn(move || {
        while let Ok(StdinMessage::Data(bytes)) = messages.recv() {
            let is_written = stdin.write_all(&bytes).and_then(|()| stdin.flush()).is_ok();
            running
                .queued_stdin_bytes
                .fetch_sub(bytes.len(), Ordering::SeqCst);
            if !is_written {
                running.kill();
                return;
            }
            if requests.lock().unwrap().commands.contains_key(&id) {
                let _ = outbox.send(&record_from(
                    json!({ "type": "exec.stdin-ready", "id": id }),
                ));
            }
        }
    });
}

/// `exec.stdin` 한 조각을 자식 stdin에 넣는다. `end`면 다 쓴 뒤 stdin을 닫는다
pub(super) fn write_stdin(
    requests: &Arc<Mutex<PeerRequests>>,
    record: &HostRecord,
    id: &str,
) -> Result<(), String> {
    let Some(running) = requests.lock().unwrap().commands.get(id).cloned() else {
        return Ok(());
    };
    let bytes = decode_host_bytes(record.get("data"), HOST_CHUNK_BYTES)
        .map_err(|error| error.to_string())?;
    if !bytes.is_empty() {
        queue_stdin(&running, StdinMessage::Data(bytes));
    }
    if record.get("end") == Some(&Value::Bool(true)) {
        queue_stdin(&running, StdinMessage::End);
    }
    Ok(())
}

pub(super) fn cancel(requests: &Arc<Mutex<PeerRequests>>, id: &str) {
    if let Some(running) = requests.lock().unwrap().commands.get(id) {
        running.kill();
    }
}

struct CommandContext {
    outbox: Arc<RecordOutbox>,
    requests: Arc<Mutex<PeerRequests>>,
    running: Arc<RunningCommand>,
    id: String,
}

/// stdout·stderr를 따로 읽어 보내는 스레드. 두 스트림이 출력 상한을 함께 센다
fn spawn_output_readers(
    context: &CommandContext,
    child: &mut Child,
    output_limit: usize,
) -> Vec<std::thread::JoinHandle<()>> {
    let total_output = Arc::new(AtomicUsize::new(0));
    let is_over_limit = Arc::new(AtomicBool::new(false));
    let streams = [
        (
            "stdout",
            child
                .stdout
                .take()
                .map(|stream| Box::new(stream) as Box<dyn Read + Send>),
        ),
        (
            "stderr",
            child
                .stderr
                .take()
                .map(|stream| Box::new(stream) as Box<dyn Read + Send>),
        ),
    ];
    streams
        .into_iter()
        .filter_map(|(name, stream)| stream.map(|stream| (name, stream)))
        .map(|(name, stream)| {
            let reader = OutputReader {
                context: context.clone_parts(),
                total_output: Arc::clone(&total_output),
                is_over_limit: Arc::clone(&is_over_limit),
                output_limit,
            };
            std::thread::spawn(move || reader.pump(name, stream))
        })
        .collect()
}

/// 출력을 흘리고, 끝나거나 시간이 다 되면 `exec.exit`을 보낸다
fn supervise(context: CommandContext, mut child: Child, bounds: CommandBounds) {
    let readers = spawn_output_readers(&context, &mut child, bounds.output_limit);
    let status = wait_with_deadline(
        &mut child,
        &context.running,
        Instant::now() + bounds.timeout,
    );
    for reader in readers {
        let _ = reader.join();
    }
    context
        .requests
        .lock()
        .unwrap()
        .commands
        .remove(&context.id);
    context.running.stdin.lock().unwrap().take();
    let mut exit = json!({
        "type": "exec.exit", "id": context.id,
        "code": status.and_then(|status| status.code()),
        "signal": status.and_then(|status| status.signal()).map(signal_name),
    });
    if let Some(error) = context.running.error.lock().unwrap().clone() {
        exit["error"] = Value::from(error);
    }
    let _ = context.outbox.send(&record_from(exit));
}

impl CommandContext {
    fn clone_parts(&self) -> CommandContext {
        CommandContext {
            outbox: Arc::clone(&self.outbox),
            requests: Arc::clone(&self.requests),
            running: Arc::clone(&self.running),
            id: self.id.clone(),
        }
    }
}

fn wait_with_deadline(
    child: &mut Child,
    running: &RunningCommand,
    deadline: Instant,
) -> Option<std::process::ExitStatus> {
    let mut has_timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(_) => return None,
        }
        if !has_timed_out && Instant::now() >= deadline {
            has_timed_out = true;
            running.fail("Remote command timed out");
        }
        std::thread::sleep(EXIT_POLL_INTERVAL);
    }
}

struct OutputReader {
    context: CommandContext,
    total_output: Arc<AtomicUsize>,
    is_over_limit: Arc<AtomicBool>,
    output_limit: usize,
}

impl OutputReader {
    fn pump(self, stream_name: &str, mut stream: Box<dyn Read + Send>) {
        let mut buffer = vec![0; HOST_CHUNK_BYTES];
        loop {
            let count = match stream.read(&mut buffer) {
                Ok(0) => return,
                Ok(count) => count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return,
            };
            let total = self.total_output.fetch_add(count, Ordering::SeqCst) + count;
            if self.output_limit > 0 && total > self.output_limit {
                self.is_over_limit.store(true, Ordering::SeqCst);
                self.context
                    .running
                    .fail("Remote command maxBuffer exceeded");
            }
            if self.is_over_limit.load(Ordering::SeqCst) {
                continue;
            }
            let record = json!({ "type": "exec.data", "id": self.context.id, "stream": stream_name, "data": encode_host_bytes(&buffer[..count]) });
            let _ = self.context.outbox.send(&record_from(record));
            self.context.outbox.wait_until_drained(STREAM_RESUME_BYTES);
        }
    }
}

/// Node `child.on("close", (code, signal))`의 신호 이름
fn signal_name(signal: i32) -> String {
    let name = match signal {
        libc::SIGHUP => "SIGHUP",
        libc::SIGINT => "SIGINT",
        libc::SIGQUIT => "SIGQUIT",
        libc::SIGKILL => "SIGKILL",
        libc::SIGPIPE => "SIGPIPE",
        libc::SIGTERM => "SIGTERM",
        libc::SIGABRT => "SIGABRT",
        libc::SIGSEGV => "SIGSEGV",
        _ => return format!("SIG{signal}"),
    };
    name.to_string()
}
