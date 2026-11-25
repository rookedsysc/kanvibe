//! ssh stdio로 붙은 원격 agent 하나(TS `Helper`). stdin에는 줄 단위 JSON 레코드를 쓰고 stdout의 레코드로 응답·출력·닫힘을 받는다.
//! 쓰기는 전용 스레드가 큐에서 꺼내 쓰므로, 큐에 쌓인 바이트가 상한을 넘으면 Node `writableLength` 검사처럼 보내기를 거절한다.
//! 사용자 콜백(relay 출력·닫힘)은 잠금을 놓은 뒤에 부른다.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::Duration;

use kanvibe_ai::javascript::{js_string, js_trim, stringify};
use kanvibe_process::exec::ssh_process::Utf8ChunkDecoder;
use serde_json::{Map, Value, json};

use super::ServiceShared;
use super::agent_process::{RemoteTerminalAgentProcess, RemoteTerminalAgentTermination};
use crate::remote_terminal_protocol::{
    REMOTE_TERMINAL_ENVELOPE_BYTES, REMOTE_TERMINAL_FRAME_BYTES,
    REMOTE_TERMINAL_OWNER_HEARTBEAT_MS, decode_remote_terminal_bytes, js_integer,
};

/// 모바일 메타데이터 요청이 시작 실패를 명시적으로 처리할 시간을 남긴다
const START_TIMEOUT: Duration = Duration::from_millis(6_000);
/// warm join은 기록 읽기 한 번과 native 전체 프레임 확인 한 번이 필요할 수 있다
const REQUEST_TIMEOUT: Duration = Duration::from_millis(12_000);
const MAX_PENDING_REQUESTS: usize = 128;
const STDOUT_CHUNK_BYTES: usize = 64 * 1024;

pub(super) type RelayOutputCallback = dyn Fn(String) + Send + Sync;
pub(super) type RelayCloseCallback = dyn Fn(u16, String) + Send + Sync;

/// relay 하나의 닫힘 상태. 원격 닫힘과 로컬 닫힘이 같은 깃발을 본다
pub(super) struct RelayState {
    is_closed: AtomicBool,
    on_close: Arc<RelayCloseCallback>,
}

impl RelayState {
    pub(super) fn new(on_close: Arc<RelayCloseCallback>) -> Arc<Self> {
        Arc::new(Self {
            is_closed: AtomicBool::new(false),
            on_close,
        })
    }

    pub(super) fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::SeqCst)
    }

    /// 처음 닫을 때만 `true`다
    pub(super) fn mark_closed(&self) -> bool {
        !self.is_closed.swap(true, Ordering::SeqCst)
    }

    /// agent나 agent 중단이 relay를 닫았다. 이미 닫힌 relay에는 알리지 않는다
    pub(super) fn close_from_remote(&self, code: u16, reason: String) {
        if self.mark_closed() {
            (self.on_close)(code, reason);
        }
    }

    pub(super) fn notify_close(&self, code: u16, reason: &str) {
        (self.on_close)(code, reason.to_string());
    }
}

#[derive(Clone)]
pub(super) struct RelayRoute {
    pub(super) on_output: Arc<RelayOutputCallback>,
    pub(super) state: Arc<RelayState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentReadiness {
    Starting,
    Ready,
    Unsupported,
    Disconnected,
}

#[derive(Default)]
struct AgentTransport {
    is_stopped: bool,
    is_bootstrapped: bool,
    pending_requests: HashMap<String, mpsc::Sender<Result<(), String>>>,
    relays: HashMap<String, RelayRoute>,
}

#[derive(Default)]
struct QueuedStdin {
    chunks: VecDeque<Vec<u8>>,
    queued_bytes: usize,
    is_ended: bool,
}

/// agent stdin에 쓸 레코드 큐. 끝낸 뒤에도 남은 레코드는 다 쓰고 stdin을 닫는다(Node `stdin.end()`)
#[derive(Default)]
struct StdinQueue {
    state: Mutex<QueuedStdin>,
    changed: Condvar,
}

impl StdinQueue {
    fn push(&self, line: String) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if state.is_ended || state.queued_bytes > REMOTE_TERMINAL_ENVELOPE_BYTES {
            return Err("Remote terminal transport unavailable".to_string());
        }
        state.queued_bytes += line.len();
        state.chunks.push_back(line.into_bytes());
        self.changed.notify_all();
        Ok(())
    }

    fn end(&self) {
        self.state.lock().unwrap().is_ended = true;
        self.changed.notify_all();
    }

    /// 다음에 쓸 레코드. 끝났고 남은 것이 없으면 `None`이다
    fn next_chunk(&self) -> Option<Vec<u8>> {
        let state = self.state.lock().unwrap();
        let mut state = self
            .changed
            .wait_while(state, |state| state.chunks.is_empty() && !state.is_ended)
            .unwrap();
        state.chunks.pop_front()
    }

    fn finish_chunk(&self, length: usize) {
        self.state.lock().unwrap().queued_bytes -= length;
    }
}

pub(super) struct AgentConnection {
    pub(super) agent_id: u64,
    pub(super) fingerprint: String,
    port: AtomicU16,
    transport: Mutex<AgentTransport>,
    stopped_changed: Condvar,
    readiness: Mutex<AgentReadiness>,
    readiness_changed: Condvar,
    stdin_queue: StdinQueue,
    termination: Box<dyn RemoteTerminalAgentTermination>,
    service: Weak<ServiceShared>,
}

/// 실행한 agent 연결과 아직 읽기·쓰기 스레드를 붙이지 않은 입출력
pub(super) struct LaunchedAgentConnection {
    pub(super) agent: Arc<AgentConnection>,
    stdin: Box<dyn Write + Send>,
    stdout: Box<dyn Read + Send>,
}

impl LaunchedAgentConnection {
    pub(super) fn new(
        agent_id: u64,
        fingerprint: String,
        process: RemoteTerminalAgentProcess,
        service: Weak<ServiceShared>,
    ) -> Self {
        let agent = Arc::new(AgentConnection {
            agent_id,
            fingerprint,
            port: AtomicU16::new(0),
            transport: Mutex::new(AgentTransport::default()),
            stopped_changed: Condvar::new(),
            readiness: Mutex::new(AgentReadiness::Starting),
            readiness_changed: Condvar::new(),
            stdin_queue: StdinQueue::default(),
            termination: process.termination,
            service,
        });
        Self {
            agent,
            stdin: process.stdin,
            stdout: process.stdout,
        }
    }
}

impl AgentConnection {
    pub(super) fn port(&self) -> u16 {
        self.port.load(Ordering::SeqCst)
    }

    /// 준비 결과를 기다린다. agent가 런타임 미지원을 알렸으면 `Ok(false)`다.
    /// 시작 제한 시간 안에 준비되지 않으면 agent 연결을 멈춘다(TS `startTimer`)
    pub(super) fn wait_until_ready(&self) -> Result<bool, String> {
        let readiness = self.readiness.lock().unwrap();
        let (readiness, _) = self
            .readiness_changed
            .wait_timeout_while(readiness, START_TIMEOUT, |readiness| {
                *readiness == AgentReadiness::Starting
            })
            .unwrap();
        let outcome = *readiness;
        drop(readiness);
        match outcome {
            AgentReadiness::Ready => Ok(true),
            AgentReadiness::Unsupported => Ok(false),
            AgentReadiness::Starting => {
                self.stop();
                Err("Remote terminal helper disconnected".to_string())
            }
            AgentReadiness::Disconnected => Err("Remote terminal helper disconnected".to_string()),
        }
    }

    fn settle(&self, outcome: AgentReadiness) {
        let mut readiness = self.readiness.lock().unwrap();
        if *readiness == AgentReadiness::Starting {
            *readiness = outcome;
            self.readiness_changed.notify_all();
        }
    }

    fn is_ready(&self) -> bool {
        *self.readiness.lock().unwrap() == AgentReadiness::Ready
    }

    pub(super) fn send(&self, record: &Value) -> Result<(), String> {
        self.stdin_queue.push(format!("{}\n", stringify(record)))
    }

    /// agent에 요청을 보내고 응답을 기다린다. 응답이 `ok: true`가 아니면 거절이다
    pub(super) fn request(&self, method: &str, payload: Map<String, Value>) -> Result<(), String> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = mpsc::channel();
        {
            let mut transport = self.transport.lock().unwrap();
            if transport.is_stopped || transport.pending_requests.len() >= MAX_PENDING_REQUESTS {
                return Err("Remote terminal request unavailable".to_string());
            }
            transport
                .pending_requests
                .insert(request_id.clone(), sender);
        }
        let mut record = Map::new();
        record.insert("type".into(), json!("request"));
        record.insert("id".into(), json!(request_id));
        record.insert("method".into(), json!(method));
        record.extend(payload);
        if let Err(error) = self.send(&Value::Object(record)) {
            self.forget_request(&request_id);
            return Err(error);
        }
        match receiver.recv_timeout(REQUEST_TIMEOUT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                self.forget_request(&request_id);
                Err("Remote terminal request timed out".to_string())
            }
            Err(RecvTimeoutError::Disconnected) => Err("Remote terminal disconnected".to_string()),
        }
    }

    fn forget_request(&self, request_id: &str) {
        self.transport
            .lock()
            .unwrap()
            .pending_requests
            .remove(request_id);
    }

    pub(super) fn add_relay(&self, relay_id: &str, route: RelayRoute) {
        self.transport
            .lock()
            .unwrap()
            .relays
            .insert(relay_id.to_string(), route);
    }

    pub(super) fn remove_relay(&self, relay_id: &str) {
        self.transport.lock().unwrap().relays.remove(relay_id);
    }

    /// agent 연결을 멈춘다. 대기 중인 요청은 끊김으로 실패하고, 열린 relay는 `stream-failed`로 닫힌다
    pub(super) fn stop(&self) {
        let (pending_requests, relays) = {
            let mut transport = self.transport.lock().unwrap();
            if transport.is_stopped {
                return;
            }
            transport.is_stopped = true;
            (
                std::mem::take(&mut transport.pending_requests),
                std::mem::take(&mut transport.relays),
            )
        };
        self.stdin_queue.end();
        self.stopped_changed.notify_all();
        if let Some(service) = self.service.upgrade() {
            service.forget_agent(self.agent_id);
        }
        for sender in pending_requests.into_values() {
            let _ = sender.send(Err("Remote terminal disconnected".to_string()));
        }
        for route in relays.into_values() {
            route
                .state
                .close_from_remote(4000, "stream-failed".to_string());
        }
        self.termination.terminate();
        self.settle(AgentReadiness::Disconnected);
    }
}

impl LaunchedAgentConnection {
    /// 쓰기·읽기 스레드를 붙인다. 읽기 스레드가 bootstrap 요청을 받으면 `bootstrap_record`를 보낸다
    pub(super) fn start_io(
        self,
        bootstrap_record: Value,
        slot_key: String,
    ) -> Arc<AgentConnection> {
        let LaunchedAgentConnection {
            agent,
            stdin,
            stdout,
        } = self;
        let writer_agent = agent.clone();
        std::thread::spawn(move || write_agent_input(writer_agent, stdin));
        let reader_agent = agent.clone();
        std::thread::spawn(move || {
            read_agent_output(&reader_agent, stdout, &bootstrap_record);
            reader_agent.stop();
            // 준비된 agent 연결의 프로세스가 끝나면 다음 요청이 새 agent 연결을 띄우게 한다. 미지원 결과는 캐시에 남긴다
            if reader_agent.is_ready()
                && let Some(service) = reader_agent.service.upgrade()
            {
                service.forget_closed_agent_slot(&slot_key, reader_agent.agent_id);
            }
        });
        agent
    }
}

fn write_agent_input(agent: Arc<AgentConnection>, mut stdin: Box<dyn Write + Send>) {
    while let Some(chunk) = agent.stdin_queue.next_chunk() {
        let write_result = stdin.write_all(&chunk).and_then(|()| stdin.flush());
        agent.stdin_queue.finish_chunk(chunk.len());
        if write_result.is_err() {
            agent.stop();
            return;
        }
    }
}

/// stdout을 끝까지 읽는다. 레코드가 깨졌거나 너무 크면 멈추고 돌아간다
fn read_agent_output(
    agent: &Arc<AgentConnection>,
    mut stdout: Box<dyn Read + Send>,
    bootstrap_record: &Value,
) {
    let mut decoder = Utf8ChunkDecoder::default();
    let mut partial = String::new();
    let mut buffer = vec![0u8; STDOUT_CHUNK_BYTES];
    loop {
        let read_length = match stdout.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read_length) => read_length,
        };
        partial.push_str(&decoder.decode(&buffer[..read_length]));
        if consume_records(agent, &mut partial, bootstrap_record).is_err() {
            return;
        }
    }
}

fn consume_records(
    agent: &Arc<AgentConnection>,
    partial: &mut String,
    bootstrap_record: &Value,
) -> Result<(), String> {
    if partial.len() > REMOTE_TERMINAL_ENVELOPE_BYTES {
        return Err("Remote terminal record too large".to_string());
    }
    while let Some(line_end) = partial.find('\n') {
        let record: Value =
            serde_json::from_str(&partial[..line_end]).map_err(|error| error.to_string())?;
        partial.drain(..=line_end);
        // JS에서 `null.type`은 예외라 깨진 레코드로 본다
        if record.is_null() {
            return Err("Invalid remote terminal record".to_string());
        }
        handle_record(agent, &record, bootstrap_record)?;
    }
    Ok(())
}

fn handle_record(
    agent: &Arc<AgentConnection>,
    record: &Value,
    bootstrap_record: &Value,
) -> Result<(), String> {
    match record.get("type").and_then(Value::as_str) {
        Some("bootstrap-ready") => send_bootstrap(agent, bootstrap_record),
        Some("unsupported") => {
            agent.settle(AgentReadiness::Unsupported);
            agent.stop();
            Ok(())
        }
        Some("ready") => accept_ready(agent, record),
        Some("response") => {
            resolve_response(agent, record);
            Ok(())
        }
        Some("output") => route_output(agent, record),
        Some("closed") => {
            route_closed(agent, record);
            Ok(())
        }
        _ => Ok(()),
    }
}

fn send_bootstrap(agent: &AgentConnection, bootstrap_record: &Value) -> Result<(), String> {
    {
        let mut transport = agent.transport.lock().unwrap();
        if transport.is_bootstrapped {
            return Err("Duplicate remote terminal bootstrap".to_string());
        }
        transport.is_bootstrapped = true;
    }
    agent.send(bootstrap_record)
}

fn accept_ready(agent: &Arc<AgentConnection>, record: &Value) -> Result<(), String> {
    let is_bootstrapped = agent.transport.lock().unwrap().is_bootstrapped;
    let port = js_integer(record.get("port")).filter(|port| (1.0..=65535.0).contains(port));
    let fingerprint = record.get("certificateFingerprint").and_then(Value::as_str);
    let is_expected_endpoint =
        is_bootstrapped && agent.port() == 0 && fingerprint == Some(agent.fingerprint.as_str());
    let Some(port) = port.filter(|_| is_expected_endpoint) else {
        return Err("Invalid remote terminal endpoint".to_string());
    };
    agent.port.store(port as u16, Ordering::SeqCst);
    if let Some(service) = agent.service.upgrade() {
        service.remember_live_agent(agent.clone());
    }
    let heartbeat_agent = agent.clone();
    std::thread::spawn(move || run_owner_heartbeat(&heartbeat_agent));
    agent.settle(AgentReadiness::Ready);
    Ok(())
}

/// 직접 소켓은 죽은 SSH 연결보다 오래 살 수 있어, 데스크탑의 heartbeat만 소유권을 갱신한다.
/// 주기마다 만료된 grant를 지우고 heartbeat를 보낸다. 보내지 못하면 agent 연결을 멈춘다
fn run_owner_heartbeat(agent: &AgentConnection) {
    let interval = Duration::from_millis(REMOTE_TERMINAL_OWNER_HEARTBEAT_MS);
    loop {
        let transport = agent.transport.lock().unwrap();
        let (transport, _) = agent
            .stopped_changed
            .wait_timeout_while(transport, interval, |transport| !transport.is_stopped)
            .unwrap();
        if transport.is_stopped {
            return;
        }
        drop(transport);
        if let Some(service) = agent.service.upgrade() {
            service.forget_expired_grants();
        }
        if agent.send(&json!({ "type": "heartbeat" })).is_err() {
            agent.stop();
            return;
        }
    }
}

fn resolve_response(agent: &AgentConnection, record: &Value) {
    let Some(request_id) = record.get("id").and_then(Value::as_str) else {
        return;
    };
    let Some(sender) = agent
        .transport
        .lock()
        .unwrap()
        .pending_requests
        .remove(request_id)
    else {
        return;
    };
    let result = if record.get("ok") == Some(&Value::Bool(true)) {
        Ok(())
    } else {
        Err("Remote terminal request rejected".to_string())
    };
    let _ = sender.send(result);
}

fn find_relay(agent: &AgentConnection, record: &Value) -> Option<RelayRoute> {
    let relay_id = record.get("relayId").and_then(Value::as_str)?;
    agent
        .transport
        .lock()
        .unwrap()
        .relays
        .get(relay_id)
        .cloned()
}

/// relay가 없으면 payload를 풀지 않는다(TS 선택적 호출은 인자를 평가하지 않는다)
fn route_output(agent: &AgentConnection, record: &Value) -> Result<(), String> {
    let Some(route) = find_relay(agent, record) else {
        return Ok(());
    };
    let bytes = decode_remote_terminal_bytes(record.get("data"), REMOTE_TERMINAL_FRAME_BYTES)?;
    (route.on_output)(String::from_utf8_lossy(&bytes).into_owned());
    Ok(())
}

fn route_closed(agent: &AgentConnection, record: &Value) {
    let route = record
        .get("relayId")
        .and_then(Value::as_str)
        .and_then(|relay_id| agent.transport.lock().unwrap().relays.remove(relay_id));
    let Some(route) = route else {
        return;
    };
    let reason = record
        .get("reason")
        .map_or_else(|| "undefined".to_string(), js_string);
    route
        .state
        .close_from_remote(close_code(record.get("code")), reason);
}

/// TS `Number(record.code) || 4000`. 0이나 숫자가 아닌 값은 4000이다
fn close_code(value: Option<&Value>) -> u16 {
    let code = js_number(value);
    if code.is_nan() || code == 0.0 {
        return 4000;
    }
    code as u16
}

/// JS `Number(value)`. JSON에 나올 수 있는 값만 다룬다
fn js_number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(flag)) => f64::from(u8::from(*flag)),
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => {
            let trimmed = js_trim(text);
            if trimmed.is_empty() {
                return 0.0;
            }
            trimmed.parse().unwrap_or(f64::NAN)
        }
        Some(Value::Array(_) | Value::Object(_)) => f64::NAN,
    }
}
