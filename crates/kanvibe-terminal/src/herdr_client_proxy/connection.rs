//! TUI 연결 하나와 그 연결이 여는 Herdr 서버 연결 한 쌍(TS `HerdrEndpointConnection`).
//!
//! 두 소켓을 각자 읽는 스레드가 프레임을 해석하고, 모든 판단과 상태 변경은 `state` 잠금 하나 안에서 한다.
//! 나가는 프레임은 잠금 안에서 [`OutboundQueue`]에 순서대로 넣고 소켓마다 하나인 쓰기 스레드가 쓴다.
//! 그래서 TS 이벤트 루프가 보장하던 "판단한 순서 = 전송 순서"가 스레드 사이에서도 유지된다.
//!
//! TS는 최초 workspace 선택 응답을 promise `then`으로 받아, 응답 프레임이 든 조각을 끝까지 처리한 뒤에
//! 초기화를 마쳤다. 여기서도 그 결과를 `initial_focus_outcomes`에 모아 두었다가 잠금을 놓기 직전([`settle`])에
//! 처리한다. 즉시 처리하면 boot가 바뀌며 거절된 이전 선택이 아직 바뀌기 전의 boot ID와 비교돼 연결을 닫게 된다.
//!
//! [`settle`]: HerdrEndpointConnection::settle

use std::collections::{HashMap, VecDeque};
use std::io::{ErrorKind, Read, Write};
use std::mem;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use kanvibe_ai::javascript::{is_truthy, js_string, stringify};
use serde_json::{Value, json};

use super::HerdrSurfaceSize;
use super::codec::{
    FrameReader, MAX_FRAME_BYTES, encode_string, frame, read_string, read_unsigned,
};
use super::outbound::OutboundQueue;

const ENDPOINT_CONTROL_TAG: u8 = 20;
const ENDPOINT_REQUEST_TAG: u8 = 15;
const ENDPOINT_RESPONSE_TAG: u8 = 18;
const PANE_INPUT_TAG: u8 = 13;
const POPUP_INPUT_TAG: u8 = 14;
const ENDPOINT_RESIZE_TAG: u8 = 12;
const MAX_ENDPOINT_REQUEST_BYTES: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_millis(5_000);
/// Node 소켓이 `data` 이벤트 하나로 넘기는 크기
const READ_CHUNK_BYTES: usize = 64 * 1024;

pub(super) const CONNECTION_CLOSED_MESSAGE: &str = "Herdr 클라이언트 연결이 종료됐습니다.";

/// 응답을 기다리는 endpoint 요청
struct PendingRequest {
    id: String,
    method: String,
    params: Value,
    /// 전송한 서버의 boot ID. 아직 보내지 않았으면 `None`이다
    boot_id: Option<String>,
    deadline: Instant,
    /// 최초 workspace 선택이면 그 선택을 시작한 boot ID. 호출자가 기다리지 않고 연결이 직접 결과를 처리한다
    initializing_boot_id: Option<String>,
}

struct ConnectionState {
    boot_id: Option<String>,
    is_initialized: bool,
    is_awaiting_initial_workspace: bool,
    is_closed: bool,
    queued_input: Vec<Vec<u8>>,
    queued_input_bytes: usize,
    active_tab_id: Option<String>,
    active_pane_id: Option<String>,
    surface_size: Option<HerdrSurfaceSize>,
    is_surface_active: bool,
    sequence: u64,
    /// 삽입 순서를 지켜야 해서(TS `Map` 순회 순서) 목록으로 둔다
    pending: Vec<PendingRequest>,
    /// 호출자가 가져갈 요청 결과
    settled_results: HashMap<String, Result<Value, String>>,
    /// 잠금을 놓기 전에 처리할 최초 workspace 선택 결과
    initial_focus_outcomes: VecDeque<(String, Result<Value, String>)>,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self {
            boot_id: None,
            is_initialized: false,
            is_awaiting_initial_workspace: true,
            is_closed: false,
            queued_input: Vec::new(),
            queued_input_bytes: 0,
            active_tab_id: None,
            active_pane_id: None,
            surface_size: None,
            is_surface_active: false,
            sequence: 0,
            pending: Vec::new(),
            settled_results: HashMap::new(),
            initial_focus_outcomes: VecDeque::new(),
        }
    }
}

/// 어느 소켓에서 읽은 프레임인지
#[derive(Clone, Copy)]
enum RelaySource {
    Client,
    Upstream,
}

pub(super) struct HerdrEndpointConnection {
    workspace_id: String,
    request_prefix: String,
    state: Mutex<ConnectionState>,
    state_changed: Condvar,
    client: UnixStream,
    upstream: Option<UnixStream>,
    client_outbound: OutboundQueue,
    upstream_outbound: OutboundQueue,
}

impl HerdrEndpointConnection {
    /// TUI 연결을 받아 Herdr 서버에 연결하고 중계를 시작한다. 서버에 연결하지 못하면 닫힌 연결을 돌려준다
    pub(super) fn start(
        client: UnixStream,
        upstream_socket_path: &Path,
        workspace_id: &str,
    ) -> Arc<Self> {
        let (upstream, connect_error) = match UnixStream::connect(upstream_socket_path) {
            Ok(upstream) => (Some(upstream), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let connection = Arc::new(Self {
            workspace_id: workspace_id.to_string(),
            request_prefix: format!("kanvibe-proxy:{}:", uuid::Uuid::new_v4()),
            state: Mutex::new(ConnectionState::default()),
            state_changed: Condvar::new(),
            client,
            upstream,
            client_outbound: OutboundQueue::default(),
            upstream_outbound: OutboundQueue::default(),
        });
        match connect_error {
            None => connection.spawn_workers(),
            Some(error) => connection.destroy(Some(error)),
        }
        connection
    }

    fn spawn_workers(self: &Arc<Self>) {
        let streams = self.clone_streams();
        let (client_reader, client_writer, upstream_reader, upstream_writer) = match streams {
            Ok(streams) => streams,
            Err(error) => return self.destroy(Some(error.to_string())),
        };
        let connection = Arc::clone(self);
        thread::spawn(move || connection.run_reader(client_reader, RelaySource::Client));
        let connection = Arc::clone(self);
        thread::spawn(move || connection.run_reader(upstream_reader, RelaySource::Upstream));
        let connection = Arc::clone(self);
        thread::spawn(move || connection.run_writer(client_writer, RelaySource::Client));
        let connection = Arc::clone(self);
        thread::spawn(move || connection.run_writer(upstream_writer, RelaySource::Upstream));
        let connection = Arc::clone(self);
        thread::spawn(move || connection.run_request_timer());
    }

    fn clone_streams(&self) -> std::io::Result<(UnixStream, UnixStream, UnixStream, UnixStream)> {
        let upstream = self
            .upstream
            .as_ref()
            .ok_or_else(|| std::io::Error::from(ErrorKind::NotConnected))?;
        Ok((
            self.client.try_clone()?,
            self.client.try_clone()?,
            upstream.try_clone()?,
            upstream.try_clone()?,
        ))
    }

    pub(super) fn active_tab_id(&self) -> Option<String> {
        self.lock_state().active_tab_id.clone()
    }

    pub(super) fn active_pane_id(&self) -> Option<String> {
        self.lock_state().active_pane_id.clone()
    }

    pub(super) fn surface_size(&self) -> Option<HerdrSurfaceSize> {
        let state = self.lock_state();
        let is_projectable =
            state.is_initialized && !state.is_awaiting_initial_workspace && state.is_surface_active;
        if is_projectable {
            state.surface_size
        } else {
            None
        }
    }

    /// endpoint 요청을 보내고 성공 응답의 `result`나 실패 사유를 기다린다.
    /// 초기화 전이면 최초 workspace 선택이 끝난 뒤에 보낸다
    pub(super) fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut state = self.lock_state();
        if state.is_closed {
            return Err(CONNECTION_CLOSED_MESSAGE.to_string());
        }
        let id = self.register_request(&mut state, method, params, None);
        if state.boot_id.is_some() && state.is_initialized {
            self.send_request(&mut state, &id);
        }
        self.settle(&mut state);
        loop {
            if let Some(result) = state.settled_results.remove(&id) {
                return result;
            }
            state = self
                .state_changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// 연결을 닫는다. 대기 요청은 `error`로 실패하고, 쓰지 못한 프레임은 버린다
    pub(super) fn destroy(&self, error: Option<String>) {
        let mut state = self.lock_state();
        self.destroy_locked(
            &mut state,
            error.unwrap_or_else(|| CONNECTION_CLOSED_MESSAGE.to_string()),
        );
        self.settle(&mut state);
    }

    fn destroy_locked(&self, state: &mut ConnectionState, error: String) {
        if state.is_closed {
            return;
        }
        state.is_closed = true;
        state.active_tab_id = None;
        state.active_pane_id = None;
        state.surface_size = None;
        let pending_ids: Vec<String> = state
            .pending
            .iter()
            .map(|request| request.id.clone())
            .collect();
        for id in pending_ids {
            self.finish_request(state, &id, Err(error.clone()));
        }
        state.queued_input.clear();
        self.client_outbound.close();
        self.upstream_outbound.close();
        let _ = self.client.shutdown(Shutdown::Both);
        if let Some(upstream) = &self.upstream {
            let _ = upstream.shutdown(Shutdown::Both);
        }
    }

    fn run_reader(self: Arc<Self>, mut source: UnixStream, relay_source: RelaySource) {
        let mut frame_reader = FrameReader::default();
        let mut chunk = vec![0_u8; READ_CHUNK_BYTES];
        loop {
            let count = match source.read(&mut chunk) {
                Ok(0) => return self.destroy(None),
                Ok(count) => count,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) => return self.destroy(Some(error.to_string())),
            };
            if !self.receive_chunk(&mut frame_reader, &chunk[..count], relay_source) {
                return;
            }
            // Node `writeThrough`처럼 상대 쪽 쓰기 버퍼가 차면 비워질 때까지 이 쪽 읽기를 멈춘다
            let destination = match relay_source {
                RelaySource::Client => &self.upstream_outbound,
                RelaySource::Upstream => &self.client_outbound,
            };
            if destination.is_above_high_water_mark() {
                destination.wait_until_drained();
            }
        }
    }

    /// 받은 조각의 프레임을 모두 처리한다. 연결이 아직 열려 있으면 true다
    fn receive_chunk(
        &self,
        frame_reader: &mut FrameReader,
        chunk: &[u8],
        relay_source: RelaySource,
    ) -> bool {
        let mut state = self.lock_state();
        if state.is_closed {
            return false;
        }
        let outcome = frame_reader.push(chunk, |payload| match relay_source {
            RelaySource::Client => self.forward_input(&mut state, payload),
            RelaySource::Upstream => self.receive(&mut state, payload),
        });
        if let Err(error) = outcome {
            self.destroy_locked(&mut state, error);
        }
        self.settle(&mut state);
        !state.is_closed
    }

    fn run_writer(self: Arc<Self>, mut destination: UnixStream, relay_target: RelaySource) {
        let queue = match relay_target {
            RelaySource::Client => &self.client_outbound,
            RelaySource::Upstream => &self.upstream_outbound,
        };
        while let Some(bytes) = queue.next_frame() {
            if let Err(error) = destination.write_all(&bytes) {
                return self.destroy(Some(error.to_string()));
            }
            queue.mark_written(bytes.len());
        }
    }

    /// TS `setTimeout`을 대신한다. 가장 이른 마감까지 기다렸다가 지난 요청을 실패시킨다
    fn run_request_timer(self: Arc<Self>) {
        let mut state = self.lock_state();
        while !state.is_closed {
            let now = Instant::now();
            let expired_ids: Vec<String> = state
                .pending
                .iter()
                .filter(|request| request.deadline <= now)
                .map(|request| request.id.clone())
                .collect();
            for id in expired_ids {
                self.finish_request(
                    &mut state,
                    &id,
                    Err("Herdr 클라이언트 요청 시간 초과".to_string()),
                );
            }
            self.settle(&mut state);
            let next_deadline = state.pending.iter().map(|request| request.deadline).min();
            state = match next_deadline {
                _ if state.is_closed => break,
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(now);
                    self.state_changed
                        .wait_timeout(state, remaining)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
                None => self
                    .state_changed
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner),
            };
        }
    }

    fn register_request(
        &self,
        state: &mut ConnectionState,
        method: &str,
        params: Value,
        initializing_boot_id: Option<String>,
    ) -> String {
        state.sequence += 1;
        let id = format!("{}{}", self.request_prefix, state.sequence);
        state.pending.push(PendingRequest {
            id: id.clone(),
            method: method.to_string(),
            params,
            boot_id: None,
            deadline: Instant::now() + REQUEST_TIMEOUT,
            initializing_boot_id,
        });
        id
    }

    fn send_request(&self, state: &mut ConnectionState, id: &str) {
        let Some(boot_id) = state.boot_id.clone() else {
            return;
        };
        let Some(request) = state.pending.iter_mut().find(|request| request.id == id) else {
            return;
        };
        request.boot_id = Some(boot_id.clone());
        let request_json =
            stringify(&json!({ "id": id, "method": request.method, "params": request.params }));
        let mut payload = vec![ENDPOINT_REQUEST_TAG];
        payload.extend(encode_string(&boot_id));
        payload.extend(encode_string(&request_json));
        if payload.len() > MAX_ENDPOINT_REQUEST_BYTES {
            self.finish_request(
                state,
                id,
                Err("Herdr 클라이언트 요청 크기 초과".to_string()),
            );
            return;
        }
        self.upstream_outbound.push(frame(&payload));
    }

    fn finish_request(&self, state: &mut ConnectionState, id: &str, result: Result<Value, String>) {
        let Some(index) = state.pending.iter().position(|request| request.id == id) else {
            return;
        };
        let request = state.pending.remove(index);
        match request.initializing_boot_id {
            Some(initializing_boot_id) => state
                .initial_focus_outcomes
                .push_back((initializing_boot_id, result)),
            None => {
                state.settled_results.insert(request.id, result);
            }
        }
    }

    /// 잠금을 놓기 전에 미뤄 둔 최초 workspace 선택 결과를 처리하고 기다리는 쪽을 깨운다
    fn settle(&self, state: &mut ConnectionState) {
        while let Some((initializing_boot_id, result)) = state.initial_focus_outcomes.pop_front() {
            self.complete_initial_workspace_focus(state, &initializing_boot_id, result);
        }
        self.state_changed.notify_all();
    }

    fn complete_initial_workspace_focus(
        &self,
        state: &mut ConnectionState,
        initializing_boot_id: &str,
        result: Result<Value, String>,
    ) {
        let is_same_boot = state.boot_id.as_deref() == Some(initializing_boot_id);
        if let Err(error) = result {
            if is_same_boot {
                self.destroy_locked(state, error);
            }
            return;
        }
        if state.is_closed || !is_same_boot {
            return;
        }
        state.is_initialized = true;
        for bytes in mem::take(&mut state.queued_input) {
            self.upstream_outbound.push(bytes);
        }
        state.queued_input_bytes = 0;
        let unsent_ids: Vec<String> = state
            .pending
            .iter()
            .filter(|request| request.boot_id.is_none())
            .map(|request| request.id.clone())
            .collect();
        for id in unsent_ids {
            self.send_request(state, &id);
        }
    }

    /// TUI가 보낸 프레임. surface 상태를 읽고, 초기화 전 입력은 버리거나 모아 둔다
    fn forward_input(&self, state: &mut ConnectionState, payload: Vec<u8>) -> Result<(), String> {
        receive_surface_state(state, &payload)?;
        let tag = payload[0];
        // 초기 화면의 pane ID가 다른 task를 가리킬 수 있어 그 입력은 재전송하지 않는다
        let is_pane_input = tag == PANE_INPUT_TAG || tag == POPUP_INPUT_TAG;
        if (state.is_awaiting_initial_workspace || !state.is_initialized) && is_pane_input {
            return Ok(());
        }
        if state.boot_id.is_some() && !state.is_initialized {
            state.queued_input_bytes += payload.len() + 4;
            if state.queued_input_bytes > MAX_FRAME_BYTES {
                return Err("Herdr 초기 연결 입력 크기 초과".to_string());
            }
            state.queued_input.push(frame(&payload));
            return Ok(());
        }
        self.upstream_outbound.push(frame(&payload));
        Ok(())
    }

    /// Herdr 서버가 보낸 프레임. 이 프록시가 보낸 요청의 응답만 숨기고 나머지는 원문 그대로 TUI에 넘긴다
    fn receive(&self, state: &mut ConnectionState, payload: Vec<u8>) -> Result<(), String> {
        if payload[0] == ENDPOINT_RESPONSE_TAG && self.receive_response(state, &payload)? {
            return Ok(());
        }
        if payload[0] == ENDPOINT_CONTROL_TAG {
            let (kind, position) = read_string(&payload, 1)?;
            if kind == "shell.snapshot.v1" {
                let (snapshot_json, _) = read_string(&payload, position)?;
                self.receive_snapshot(state, &parse_json(&snapshot_json)?)?;
            }
        }
        // 초기 surface를 버리면 Herdr의 후속 delta가 기준 프레임을 잃는다
        self.client_outbound.push(frame(&payload));
        Ok(())
    }

    /// 이 프록시가 보낸 요청의 응답이면 처리하고 true를 준다
    fn receive_response(
        &self,
        state: &mut ConnectionState,
        payload: &[u8],
    ) -> Result<bool, String> {
        let (boot_id, id_position) = read_string(payload, 1)?;
        let (id, final_position) = read_string(payload, id_position)?;
        if !id.starts_with(&self.request_prefix) {
            return Ok(false);
        }
        let Some(request) = state.pending.iter().find(|request| request.id == id) else {
            return Ok(true);
        };
        if request.boot_id.as_deref() != Some(boot_id.as_str()) {
            return Ok(true);
        }
        if payload.get(final_position) != Some(&1) {
            let error = "Herdr 클라이언트 응답이 단일 프레임이 아닙니다.".to_string();
            self.finish_request(state, &id, Err(error));
            return Ok(true);
        }
        let (response_json, _) = read_string(payload, final_position + 1)?;
        let response = require_property_owner(parse_json(&response_json)?, "error")?;
        self.finish_request(state, &id, response_outcome(&response));
        Ok(true)
    }

    fn receive_snapshot(
        &self,
        state: &mut ConnectionState,
        snapshot: &Value,
    ) -> Result<(), String> {
        let snapshot = require_property_owner(snapshot.clone(), "boot_id")?;
        let Some(boot_id) = snapshot
            .get("boot_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            return Err("Herdr snapshot에 boot ID가 없습니다.".to_string());
        };
        self.track_focused_targets(state, &snapshot);
        let is_new_boot = state.boot_id.as_deref() != Some(boot_id);
        if is_new_boot {
            state.is_awaiting_initial_workspace = true;
            state.queued_input.clear();
            state.queued_input_bytes = 0;
        }
        if self.is_own_workspace(&snapshot) {
            state.is_awaiting_initial_workspace = false;
        }
        if is_new_boot {
            self.start_initial_workspace_focus(state, boot_id);
        }
        Ok(())
    }

    fn track_focused_targets(&self, state: &mut ConnectionState, snapshot: &Value) {
        let is_own_workspace = self.is_own_workspace(snapshot);
        let focused_tab_id = snapshot.get("focused_tab_id").and_then(Value::as_str);
        let focused_pane_id = snapshot.get("focused_pane_id").and_then(Value::as_str);
        let pane_prefix = format!("{}:", self.workspace_id);
        state.active_tab_id = focused_tab_id
            .filter(|_| is_own_workspace)
            .map(str::to_string);
        state.active_pane_id = focused_pane_id
            .filter(|pane_id| is_own_workspace && pane_id.starts_with(&pane_prefix))
            .map(str::to_string);
    }

    fn is_own_workspace(&self, snapshot: &Value) -> bool {
        snapshot.get("focused_workspace_id").and_then(Value::as_str)
            == Some(self.workspace_id.as_str())
    }

    /// 새 서버 boot에서 이 연결의 workspace를 고른다. 이전 boot로 보낸 요청은 다시 보낼 수 없어 실패시킨다
    fn start_initial_workspace_focus(&self, state: &mut ConnectionState, boot_id: &str) {
        let stale_ids: Vec<String> = state
            .pending
            .iter()
            .filter(|request| request.boot_id.is_some())
            .map(|request| request.id.clone())
            .collect();
        for id in stale_ids {
            self.finish_request(
                state,
                &id,
                Err("Herdr 서버 연결이 갱신됐습니다.".to_string()),
            );
        }
        state.boot_id = Some(boot_id.to_string());
        state.is_initialized = false;
        let params = json!({ "workspace_id": self.workspace_id });
        let id = self.register_request(state, "workspace.focus", params, Some(boot_id.to_string()));
        self.send_request(state, &id);
    }

    fn lock_state(&self) -> MutexGuard<'_, ConnectionState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// TUI가 알린 surface 크기와 표시 여부를 따라간다
fn receive_surface_state(state: &mut ConnectionState, payload: &[u8]) -> Result<(), String> {
    match payload[0] {
        ENDPOINT_CONTROL_TAG => receive_endpoint_hello(state, payload),
        // 알 수 없는 프레임도 중계하되, 추측한 좌표로 다른 pane을 덮지 않는다
        ENDPOINT_RESIZE_TAG => {
            state.surface_size = read_resize_surface_size(payload)
                .ok()
                .and_then(|(cols, rows)| {
                    surface_size_from_numbers(f64::from(cols), f64::from(rows))
                });
            Ok(())
        }
        ENDPOINT_REQUEST_TAG => receive_client_request(state, payload),
        _ => Ok(()),
    }
}

fn receive_endpoint_hello(state: &mut ConnectionState, payload: &[u8]) -> Result<(), String> {
    let (kind, position) = read_string(payload, 1)?;
    if kind != "endpoint.hello.v1" {
        return Ok(());
    }
    let (hello_json, _) = read_string(payload, position)?;
    let hello = require_property_owner(parse_json(&hello_json)?, "generation")?;
    let is_generation_one = hello.get("generation").and_then(Value::as_f64) == Some(1.0);
    state.is_surface_active =
        is_generation_one && hello.get("surface_active") == Some(&Value::Bool(true));
    let surface_size = hello.get("surface_size");
    let cols = surface_size
        .and_then(|size| size.get("cols"))
        .and_then(Value::as_f64);
    let rows = surface_size
        .and_then(|size| size.get("rows"))
        .and_then(Value::as_f64);
    state.surface_size = cols
        .zip(rows)
        .and_then(|(cols, rows)| surface_size_from_numbers(cols, rows));
    Ok(())
}

/// resize 프레임은 셀 너비·셀 높이·열·행 varint 뒤에 0이나 1인 바이트 하나로 끝난다
fn read_resize_surface_size(payload: &[u8]) -> Result<(u32, u32), String> {
    let (_, cell_width_end) = read_unsigned(payload, 1)?;
    let (_, cell_height_end) = read_unsigned(payload, cell_width_end)?;
    let (cols, cols_end) = read_unsigned(payload, cell_height_end)?;
    let (rows, rows_end) = read_unsigned(payload, cols_end)?;
    if rows_end + 1 != payload.len() || payload[rows_end] > 1 {
        return Err("알 수 없는 Herdr resize 프레임입니다.".to_string());
    }
    Ok((cols, rows))
}

fn receive_client_request(state: &mut ConnectionState, payload: &[u8]) -> Result<(), String> {
    let (_, position) = read_string(payload, 1)?;
    let (request_json, _) = read_string(payload, position)?;
    let request = require_property_owner(parse_json(&request_json)?, "method")?;
    if request.get("method").and_then(Value::as_str) == Some("client_shell.surface.set") {
        let active = request
            .get("params")
            .and_then(|params| params.get("active"));
        state.is_surface_active = active == Some(&Value::Bool(true));
    }
    Ok(())
}

/// 양의 정수 두 개일 때만 크기로 받는다
fn surface_size_from_numbers(cols: f64, rows: f64) -> Option<HerdrSurfaceSize> {
    let is_positive_integer =
        |value: f64| value.fract() == 0.0 && value > 0.0 && value <= f64::from(u32::MAX);
    if !is_positive_integer(cols) || !is_positive_integer(rows) {
        return None;
    }
    Some(HerdrSurfaceSize {
        cols: cols as u32,
        rows: rows as u32,
    })
}

/// 응답의 `error`가 참이면 그 메시지로, `result`가 참이면 그 값으로 끝난다
fn response_outcome(response: &Value) -> Result<Value, String> {
    if let Some(error) = response.get("error").filter(|error| is_truthy(error)) {
        let message = error
            .get("message")
            .filter(|message| !message.is_null())
            .or_else(|| error.get("code"));
        // JS `new Error(undefined)`의 메시지는 빈 문자열이다
        return Err(message.map(js_string).unwrap_or_default());
    }
    match response.get("result").filter(|result| is_truthy(result)) {
        Some(result) => Ok(result.clone()),
        None => Err("Herdr 클라이언트 응답에 result가 없습니다.".to_string()),
    }
}

fn parse_json(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|error| error.to_string())
}

/// JS에서 `null`의 속성을 읽으면 TypeError로 연결이 닫혔다. 같은 메시지로 실패시킨다
fn require_property_owner(value: Value, property: &str) -> Result<Value, String> {
    if value.is_null() {
        return Err(format!(
            "Cannot read properties of null (reading '{property}')"
        ));
    }
    Ok(value)
}
