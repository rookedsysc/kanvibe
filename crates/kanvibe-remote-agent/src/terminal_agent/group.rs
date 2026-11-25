//! pane 하나의 controller group: 만들기, 기본 크기 읽기, controller 띄우기, 프레임 나누기, 닫기(TS `getGroup`, `closeGroup`).
//!
//! 데스크탑이 발급한 grant는 다른 모바일 화면이 pane을 빌린 동안 만들어졌을 수 있으므로, controller를 잡기 바로 전에
//! 원래(native) 크기를 다시 읽어 두고, group이 닫힐 때 그 크기로 되돌린 뒤 controller 권한을 놓는다.

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::Agent;
use super::state::{AgentState, Controller, Group, Readiness, WireGrant};
use super::wire::{
    HerdrTerminalRecord, REMOTE_TERMINAL_COMMAND_TIMEOUT_MS, REMOTE_TERMINAL_ENVELOPE_BYTES,
    REMOTE_TERMINAL_FRAME_BYTES, Utf8StreamDecoder, build_wheel_delegated_screen_redraw,
    parse_herdr_terminal_record,
};
use crate::environment::current_child_environment;
use crate::outbox::RecordOutbox;

const COMMAND_TIMEOUT: Duration = Duration::from_millis(REMOTE_TERMINAL_COMMAND_TIMEOUT_MS);

impl Agent {
    /// 연결을 pane group에 넣는다. 없으면 만들고 기본 크기 읽기와 controller 준비를 따로 시작한다
    pub(super) fn join_group(
        self: &Arc<Self>,
        state: &mut AgentState,
        grant: &Arc<WireGrant>,
        client_id: u64,
    ) -> Result<u64, String> {
        if !state.is_active_client(client_id) {
            return Err("Terminal grant revoked".into());
        }
        if let Some(&group_id) = state.groups.get(&grant.target_key) {
            let group = state
                .group_store
                .get_mut(&group_id)
                .ok_or("Terminal unavailable")?;
            if !group.clients.contains(&client_id) {
                group.clients.push(client_id);
            }
            return Ok(group_id);
        }
        let group_id = state.allocate_id();
        state
            .group_store
            .insert(group_id, new_group(grant, client_id));
        state.groups.insert(grant.target_key.clone(), group_id);
        let agent = Arc::clone(self);
        std::thread::spawn(move || {
            if agent.bootstrap_group(group_id).is_err() {
                let mut state = agent.lock();
                agent.close_group(&mut state, group_id, 4000, "stream-failed");
                if let Some(group) = state.group_store.get_mut(&group_id) {
                    group.readiness = Readiness::Failed;
                }
                agent.changed.notify_all();
            }
        });
        Ok(group_id)
    }

    fn bootstrap_group(self: &Arc<Self>, group_id: u64) -> Result<(), String> {
        let geometry_command = self.group_grant(group_id)?.geometry_command.clone();
        let geometry = self.run_query(&geometry_command)?;
        let mut state = self.lock();
        let owner_grant = self.claim_owner(&mut state, group_id, &geometry)?;
        let started_at = Instant::now();
        self.spawn_controller(&mut state, group_id, &owner_grant)?;
        drop(state);
        let history = self.run_query(&owner_grant.snapshot_command)?;
        let state = self.lock();
        let (mut state, _) = self
            .changed
            .wait_timeout_while(
                state,
                COMMAND_TIMEOUT.saturating_sub(started_at.elapsed()),
                |state| {
                    state
                        .group_store
                        .get(&group_id)
                        .is_some_and(|group| !group.is_dead && !group.has_initial_frame)
                },
            )
            .unwrap();
        let group = state
            .group_store
            .get_mut(&group_id)
            .filter(|group| !group.is_dead);
        let group = group.ok_or("Terminal closed during bootstrap")?;
        if !group.has_initial_frame {
            return Err("Terminal initial frame timed out".into());
        }
        group.history = build_wheel_delegated_screen_redraw(&history);
        group.is_initialized = true;
        group.is_bootstrapping = false;
        group.readiness = Readiness::Ready;
        let opening = format!("{}{}", group.history, group.buffered_frames);
        group.buffered_frames.clear();
        for client_id in group.clients.clone() {
            self.send_to_client(&mut state, client_id, &opening);
            if let Some(client) = state.clients.get_mut(&client_id) {
                client.is_ready = true;
            }
        }
        self.changed.notify_all();
        Ok(())
    }

    fn group_grant(&self, group_id: u64) -> Result<Arc<WireGrant>, String> {
        let state = self.lock();
        let group = state
            .group_store
            .get(&group_id)
            .ok_or("Terminal unavailable")?;
        Ok(Arc::clone(&group.grant))
    }

    /// 기본 크기를 기록하고, 아직 grant를 쥔 연결 가운데 첫 연결의 grant로 controller를 잡는다
    fn claim_owner(
        &self,
        state: &mut AgentState,
        group_id: u64,
        geometry: &str,
    ) -> Result<Arc<WireGrant>, String> {
        let pane_id = state
            .group_store
            .get(&group_id)
            .map(|group| group.grant.pane_id.clone())
            .unwrap_or_default();
        let original =
            parse_native_geometry(geometry, &pane_id).ok_or("Terminal geometry unavailable")?;
        let group = state
            .group_store
            .get(&group_id)
            .filter(|group| !group.is_dead);
        let group = group.ok_or("Terminal client detached before claim")?;
        let owner = group
            .clients
            .iter()
            .copied()
            .find(|id| state.is_active_client(*id));
        let owner = owner.ok_or("Terminal grant revoked before claim")?;
        let owner_grant = Arc::clone(&state.clients[&owner].grant);
        let group = state
            .group_store
            .get_mut(&group_id)
            .ok_or("Terminal unavailable")?;
        group.grant = Arc::clone(&owner_grant);
        group.size = (owner_grant.cols, owner_grant.rows);
        group.original = Some(original);
        Ok(owner_grant)
    }

    fn spawn_controller(
        self: &Arc<Self>,
        state: &mut AgentState,
        group_id: u64,
        grant: &WireGrant,
    ) -> Result<(), String> {
        let mut child = Command::new("sh")
            .args(["-lc", &format!("exec {}", grant.control_command)])
            .env_clear()
            .envs(current_child_environment())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        let stdin = child.stdin.take().expect("stdin을 파이프로 열었다");
        let stdout = child.stdout.take().expect("stdout을 파이프로 열었다");
        let controller = Controller {
            stdin: RecordOutbox::start(Box::new(stdin), Box::new(|| ())),
            pid: child.id(),
        };
        if let Some(group) = state.group_store.get_mut(&group_id) {
            group.controller = Some(controller);
        }
        let reader_agent = Arc::clone(self);
        std::thread::spawn(move || reader_agent.read_controller(group_id, stdout));
        let exit_agent = Arc::clone(self);
        std::thread::spawn(move || exit_agent.await_controller_exit(group_id, child));
        Ok(())
    }

    fn read_controller(&self, group_id: u64, stdout: impl Read) {
        let mut reader = BufReader::new(stdout);
        let mut decoder = Utf8StreamDecoder::default();
        loop {
            let mut line = Vec::new();
            let limit = REMOTE_TERMINAL_ENVELOPE_BYTES as u64 + 1;
            match reader.by_ref().take(limit).read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => return,
                Ok(count) if count as u64 >= limit => {
                    self.close_group(&mut self.lock(), group_id, 4000, "stream-failed");
                    return;
                }
                Ok(_) => {}
            }
            let text = String::from_utf8_lossy(&line);
            if !self.receive_controller_line(group_id, text.trim_end_matches('\n'), &mut decoder) {
                return;
            }
        }
    }

    /// controller 출력 한 줄. 계속 읽어야 하면 true다
    fn receive_controller_line(
        &self,
        group_id: u64,
        line: &str,
        decoder: &mut Utf8StreamDecoder,
    ) -> bool {
        let mut state = self.lock();
        if state
            .group_store
            .get(&group_id)
            .is_none_or(|group| group.is_dead)
        {
            return false;
        }
        if line.trim().is_empty() {
            return true;
        }
        let (is_full_screen, bytes) = match parse_herdr_terminal_record(line) {
            None => return true,
            Some(HerdrTerminalRecord::Frame {
                is_full_screen,
                bytes,
            }) => (is_full_screen, bytes),
            Some(HerdrTerminalRecord::Closed) => {
                if let Some(group) = state.group_store.get_mut(&group_id) {
                    group.owns_terminal = false;
                }
                self.close_group(&mut state, group_id, 4000, "stream-failed");
                return false;
            }
            Some(HerdrTerminalRecord::Invalid) => {
                self.close_group(&mut state, group_id, 4000, "stream-failed");
                return false;
            }
        };
        let text = decoder.write(&bytes);
        self.fan_out_frame(&mut state, group_id, is_full_screen, &text)
    }

    /// 프레임을 붙은 연결에 나눈다. 전체 화면 프레임이면 기다리던 합류 연결에 이력과 함께 먼저 보낸다
    fn fan_out_frame(
        &self,
        state: &mut AgentState,
        group_id: u64,
        is_full_screen: bool,
        text: &str,
    ) -> bool {
        let mut joined = Vec::new();
        let Some(group) = state.group_store.get_mut(&group_id) else {
            return false;
        };
        if is_full_screen {
            group.owns_terminal = true;
            group.has_initial_frame = true;
            joined = group.joiners.drain().collect();
            self.changed.notify_all();
        }
        if group.is_bootstrapping {
            group.buffered_frames.push_str(text);
            if group.buffered_frames.len() > REMOTE_TERMINAL_FRAME_BYTES {
                self.close_group(state, group_id, 4000, "stream-failed");
                return false;
            }
        }
        let is_bootstrapping = group.is_bootstrapping;
        let clients = group.clients.clone();
        for (client_id, joiner) in &joined {
            self.send_to_client(state, *client_id, &format!("{}{text}", joiner.history));
            if let Some(client) = state.clients.get_mut(client_id) {
                client.is_ready = true;
            }
            let _ = joiner.reply.send(Ok(()));
        }
        if !is_bootstrapping {
            for client_id in clients {
                let is_ready = state
                    .clients
                    .get(&client_id)
                    .is_some_and(|client| client.is_ready);
                if is_ready && !joined.iter().any(|(id, _)| *id == client_id) {
                    self.send_to_client(state, client_id, text);
                }
            }
        }
        true
    }

    fn await_controller_exit(&self, group_id: u64, mut child: Child) {
        let _ = child.wait();
        let mut state = self.lock();
        let target_key = match state.group_store.get_mut(&group_id) {
            Some(group) => {
                group.has_controller_exited = true;
                group.owns_terminal = false;
                group.target_key.clone()
            }
            None => return,
        };
        self.close_group(&mut state, group_id, 4000, "stream-failed");
        if state.closing_groups.get(&target_key) == Some(&group_id) {
            state.closing_groups.remove(&target_key);
        }
        state.group_store.remove(&group_id);
        self.changed.notify_all();
    }

    /// group을 닫는다. 정상 떼기(1000)가 아니면 같은 pane의 grant도 지우고, controller는 원래 크기로 되돌린 뒤 놓아 준다
    pub(super) fn close_group(
        &self,
        state: &mut AgentState,
        group_id: u64,
        code: u16,
        reason: &str,
    ) {
        let Some(group) = state
            .group_store
            .get_mut(&group_id)
            .filter(|group| !group.is_dead)
        else {
            return;
        };
        group.is_dead = true;
        if group.readiness == Readiness::Pending {
            group.readiness = Readiness::Failed;
        }
        let target_key = group.target_key.clone();
        let joiners: Vec<_> = group.joiners.drain().collect();
        let clients = std::mem::take(&mut group.clients);
        if state.groups.get(&target_key) == Some(&group_id) {
            state.groups.remove(&target_key);
        }
        if code != 1000 {
            state
                .grants
                .retain(|_, slot| slot.wire.target_key != target_key);
        }
        for (_, joiner) in joiners {
            let _ = joiner.reply.send(Err("Terminal closed".into()));
        }
        for client_id in clients {
            self.close_client(state, client_id, code, reason);
        }
        self.release_controller(state, group_id, &target_key);
        self.changed.notify_all();
    }

    fn release_controller(&self, state: &mut AgentState, group_id: u64, target_key: &str) {
        let Some(group) = state.group_store.get(&group_id) else {
            return;
        };
        let Some(controller) = group.controller.as_ref() else {
            state.group_store.remove(&group_id);
            return;
        };
        if group.has_controller_exited {
            return;
        }
        let mut records = Vec::new();
        if let (true, Some((cols, rows))) = (group.owns_terminal, group.original) {
            records.push(json!({ "type": "terminal.resize", "cols": cols, "rows": rows }));
        }
        records.push(json!({ "type": "terminal.release" }));
        let lines: String = records.iter().map(|record| format!("{record}\n")).collect();
        let _ = controller.stdin.send_bytes(lines.into_bytes());
        controller.stdin.close_after_flush();
        let pid = controller.pid;
        state
            .closing_groups
            .insert(target_key.to_string(), group_id);
        self.schedule_controller_termination(group_id, pid);
    }

    /// 놓아 준 controller가 시간 안에 끝나지 않으면 SIGTERM을 보낸다
    fn schedule_controller_termination(&self, group_id: u64, pid: u32) {
        let Some(agent) = self.self_reference() else {
            return;
        };
        std::thread::spawn(move || {
            std::thread::sleep(COMMAND_TIMEOUT);
            let state = agent.lock();
            let is_running = state
                .group_store
                .get(&group_id)
                .is_some_and(|group| !group.has_controller_exited);
            if is_running {
                // SAFETY: 아직 끝나지 않은(회수 전) controller pid에만 보낸다
                unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            }
        });
    }
}

fn new_group(grant: &Arc<WireGrant>, client_id: u64) -> Group {
    Group {
        target_key: grant.target_key.clone(),
        grant: Arc::clone(grant),
        clients: vec![client_id],
        controller: None,
        history: String::new(),
        is_initialized: false,
        owns_terminal: false,
        original: None,
        size: (grant.cols, grant.rows),
        joiners: Default::default(),
        is_dead: false,
        order: 0,
        readiness: Readiness::Pending,
        is_bootstrapping: true,
        buffered_frames: String::new(),
        has_initial_frame: false,
        has_controller_exited: false,
    }
}

/// `pane get`과 `pane layout` 결과(사이 구분자 0x1e)에서 pane의 원래 열·행을 읽는다
fn parse_native_geometry(geometry: &str, pane_id: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = geometry.split('\u{1e}').collect();
    let [pane_part, layout_part] = parts.as_slice() else {
        return None;
    };
    let pane: Value = serde_json::from_str(pane_part).ok()?;
    let layout: Value = serde_json::from_str(layout_part).ok()?;
    let pane = &pane["result"]["pane"];
    let layout = &layout["result"]["layout"];
    let rect = layout["panes"]
        .as_array()?
        .iter()
        .find(|entry| entry["pane_id"].as_str() == Some(pane_id))?
        .get("rect")?;
    let is_zoomed_on_pane = layout["zoomed"] == Value::Bool(true)
        && layout["focused_pane_id"].as_str() == Some(pane_id);
    let cols = if is_zoomed_on_pane {
        &layout["area"]["width"]
    } else {
        &rect["width"]
    };
    let rows = &pane["scroll"]["viewport_rows"];
    let is_valid_pane =
        pane["pane_id"].as_str() == Some(pane_id) && !is_js_truthy(&pane["restore_error"]);
    let positive = |value: &Value| {
        value
            .as_f64()
            .filter(|size| size.fract() == 0.0 && *size > 0.0)
            .map(|size| size as u32)
    };
    let (cols, rows) = (positive(cols)?, positive(rows)?);
    is_valid_pane.then_some((cols, rows))
}

/// JS 참값 판정
fn is_js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number
            .as_f64()
            .is_some_and(|number| number != 0.0 && !number.is_nan()),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}
