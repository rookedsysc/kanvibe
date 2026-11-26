//! 연결을 grant의 pane에 붙이고 입력을 controller로 넘긴다(TS `attach`, `receive`).

use std::sync::Arc;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Value, json};

use super::Agent;
use super::state::{AgentState, Joiner, Readiness, WireGrant};
use super::wire::{
    REMOTE_TERMINAL_COMMAND_TIMEOUT_MS, REMOTE_TERMINAL_INPUT_BYTES,
    REMOTE_TERMINAL_INPUT_QUEUE_BYTES, build_wheel_delegated_screen_redraw,
    is_remote_terminal_size, parse_scroll_control_record,
};

/// 붙기 첫 단계에서 쥔 grant와 바뀐 앞 연결
pub(super) struct AttachTicket {
    grant: Arc<WireGrant>,
    previous: Option<u64>,
    was_ready: bool,
}

const REFRESH_TIMEOUT: Duration = Duration::from_millis(REMOTE_TERMINAL_COMMAND_TIMEOUT_MS);

impl Agent {
    /// 연결이 grant를 쥐게 한다. 업그레이드·relay 요청을 받은 그 자리에서 바로 해야 이어 오는 철회가 이 연결을 닫는다
    pub(super) fn begin_attach(
        &self,
        state: &mut AgentState,
        grant_id: &str,
        client_id: u64,
    ) -> Result<AttachTicket, String> {
        let (grant, previous) = claim_grant(state, grant_id, client_id)?;
        let was_ready = state
            .groups
            .get(&grant.target_key)
            .and_then(|group_id| state.group_store.get(group_id))
            .is_some_and(|group| group.is_initialized);
        Ok(AttachTicket {
            grant,
            previous,
            was_ready,
        })
    }

    /// 쥔 grant의 pane group에 붙는다. 이미 화면이 돌던 group이면 이력을 다시 읽고 새 전체 화면을 받아 합류한다
    pub(super) fn complete_attach(
        self: &Arc<Self>,
        ticket: AttachTicket,
        client_id: u64,
    ) -> Result<(), String> {
        let AttachTicket {
            grant,
            previous,
            was_ready,
        } = ticket;
        let state = self.lock();
        let mut state = self
            .changed
            .wait_while(state, |state| {
                state.closing_groups.contains_key(&grant.target_key)
            })
            .unwrap();
        let group_id = self.join_group(&mut state, &grant, client_id)?;
        if let Some(previous) = previous.filter(|previous| *previous != client_id) {
            self.close_client(&mut state, previous, 1000, "transport-replaced");
        }
        let state = self
            .changed
            .wait_while(state, |state| {
                state
                    .group_store
                    .get(&group_id)
                    .is_some_and(|group| group.readiness == Readiness::Pending)
            })
            .unwrap();
        ensure_attached(&state, group_id, client_id)?;
        drop(state);
        if was_ready {
            self.join_running_group(&grant, group_id, client_id)?;
        }
        Ok(())
    }

    /// 돌던 group에 합류한다. 끝없는 델타를 쌓아 두는 대신 이력을 다시 읽고 controller에 새 전체 화면을 청한다
    fn join_running_group(
        &self,
        grant: &WireGrant,
        group_id: u64,
        client_id: u64,
    ) -> Result<(), String> {
        let history =
            build_wheel_delegated_screen_redraw(&self.run_query(&grant.snapshot_command)?);
        let (reply, replies) = channel();
        {
            let mut state = self.lock();
            ensure_attached(&state, group_id, client_id)?;
            let group = state
                .group_store
                .get_mut(&group_id)
                .ok_or("Terminal grant revoked")?;
            group.joiners.insert(client_id, Joiner { history, reply });
            let (cols, rows) = group.size;
            let record = json!({ "type": "terminal.resize", "cols": cols, "rows": rows });
            if let Err(error) = self.send_controller(&state, group_id, &record) {
                remove_joiner(&mut state, group_id, client_id);
                return Err(error);
            }
        }
        match replies.recv_timeout(REFRESH_TIMEOUT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                remove_joiner(&mut self.lock(), group_id, client_id);
                Err("Terminal refresh timed out".into())
            }
            Err(RecvTimeoutError::Disconnected) => Err("Terminal closed".into()),
        }
    }

    /// 붙은 연결의 입력 하나. 텍스트는 pane 입력이고, 바이너리는 크기·스크롤·이력 갱신 요청 JSON이다
    pub(super) fn receive(
        &self,
        state: &mut AgentState,
        client_id: u64,
        bytes: &[u8],
        is_binary: bool,
    ) -> Result<(), String> {
        if !state.is_active_client(client_id) {
            return Err("Terminal grant revoked".into());
        }
        if bytes.len() > REMOTE_TERMINAL_INPUT_BYTES {
            return Err("Terminal input too large".into());
        }
        let group_id = live_group(state, client_id)?;
        if !is_binary {
            let text = std::str::from_utf8(bytes).map_err(|_| "Invalid UTF-8 terminal input")?;
            mark_written(state, client_id);
            let record = json!({ "type": "terminal.input", "bytes": BASE64.encode(text) });
            return self.send_controller(state, group_id, &record);
        }
        let record: Value =
            serde_json::from_slice(bytes).map_err(|_| "Invalid terminal control")?;
        // 이력 갱신은 데스크탑 미러에만 있는 요청이다. 직접 붙은 기기의 이력은 붙을 때 받은 그대로다
        if record.get("type").and_then(Value::as_str) == Some("refresh-history") {
            return Ok(());
        }
        if let Some(scroll) = parse_scroll_control_record(&record) {
            // 휠은 마우스 보고를 켠 프로그램에 입력으로 닿으므로 grant를 쓴 것으로 남긴다
            mark_written(state, client_id);
            return self.send_controller(state, group_id, &scroll);
        }
        self.resize_from_client(state, group_id, client_id, &record)
    }

    fn resize_from_client(
        &self,
        state: &mut AgentState,
        group_id: u64,
        client_id: u64,
        record: &Value,
    ) -> Result<(), String> {
        let is_resize = record.get("type").and_then(Value::as_str) == Some("resize");
        if !is_resize || !is_remote_terminal_size(record.get("cols"), record.get("rows")) {
            return Err("Invalid terminal size".into());
        }
        let size = (
            record["cols"].as_f64().unwrap_or(0.0) as u32,
            record["rows"].as_f64().unwrap_or(0.0) as u32,
        );
        let group = state
            .group_store
            .get_mut(&group_id)
            .ok_or("Terminal unavailable")?;
        group.size = size;
        group.order += 1;
        let order = group.order;
        let client = state.clients.get_mut(&client_id).expect("확인한 연결");
        client.size = Some(size);
        client.order = order;
        let resize = json!({ "type": "terminal.resize", "cols": size.0, "rows": size.1 });
        self.send_controller(state, group_id, &resize)
    }

    /// 직접 연결이 보낸 메시지. 붙기 전이면 순서대로 쌓아 두고, 붙은 뒤 차례로 넘긴다
    pub(super) fn receive_direct(&self, client_id: u64, bytes: Vec<u8>, is_binary: bool) {
        let mut state = self.lock();
        let Some(client) = state
            .clients
            .get_mut(&client_id)
            .filter(|client| !client.is_closed)
        else {
            return;
        };
        if !client.is_attached {
            client.pending_bytes += bytes.len();
            client.pending_input.push((bytes, is_binary));
            if client.pending_bytes > REMOTE_TERMINAL_INPUT_QUEUE_BYTES {
                self.close_client(&mut state, client_id, 4000, "input-too-large");
            }
            return;
        }
        if self
            .receive(&mut state, client_id, &bytes, is_binary)
            .is_err()
        {
            self.close_client(&mut state, client_id, 4000, "stream-failed");
        }
    }

    /// 직접 연결의 붙기가 끝났다. 쌓아 둔 입력을 넘기거나, 실패면 연결을 닫는다
    pub(super) fn finish_direct_attach(&self, client_id: u64, result: Result<(), String>) {
        let mut state = self.lock();
        if result.is_err() {
            self.close_client(&mut state, client_id, 4000, "stream-failed");
            return;
        }
        let Some(client) = state.clients.get_mut(&client_id) else {
            return;
        };
        client.is_attached = true;
        client.pending_bytes = 0;
        let pending = std::mem::take(&mut client.pending_input);
        for (bytes, is_binary) in pending {
            if self
                .receive(&mut state, client_id, &bytes, is_binary)
                .is_err()
            {
                self.close_client(&mut state, client_id, 4000, "stream-failed");
                return;
            }
        }
    }
}

/// grant를 이 연결이 쥐게 한다. 입장 시간이 지났거나 다른 연결이 이미 입력을 보냈으면 거절한다
fn claim_grant(
    state: &mut AgentState,
    grant_id: &str,
    client_id: u64,
) -> Result<(Arc<WireGrant>, Option<u64>), String> {
    let slot = state.grants.get(grant_id).ok_or("Terminal grant revoked")?;
    if slot.admission_until <= Instant::now() {
        return Err("Expired terminal grant".into());
    }
    let is_in_use = slot
        .active
        .and_then(|active| state.clients.get(&active))
        .is_some_and(|client| client.has_written);
    if is_in_use {
        return Err("Terminal grant already in use".into());
    }
    let slot = state.grants.get_mut(grant_id).expect("확인한 grant");
    let previous = slot.active.replace(client_id);
    Ok((Arc::clone(&slot.wire), previous))
}

fn ensure_attached(state: &AgentState, group_id: u64, client_id: u64) -> Result<(), String> {
    let is_alive = state
        .group_store
        .get(&group_id)
        .is_some_and(|group| !group.is_dead && group.readiness == Readiness::Ready);
    if !is_alive || !state.is_active_client(client_id) {
        return Err("Terminal grant revoked".into());
    }
    Ok(())
}

fn remove_joiner(state: &mut AgentState, group_id: u64, client_id: u64) {
    if let Some(group) = state.group_store.get_mut(&group_id) {
        group.joiners.remove(&client_id);
    }
}

/// 연결이 가리키는 pane의 살아 있는 group
fn live_group(state: &AgentState, client_id: u64) -> Result<u64, String> {
    let target_key = &state.clients[&client_id].grant.target_key;
    let group_id = *state.groups.get(target_key).ok_or("Terminal unavailable")?;
    let is_alive = state
        .group_store
        .get(&group_id)
        .is_some_and(|group| !group.is_dead);
    is_alive
        .then_some(group_id)
        .ok_or_else(|| "Terminal unavailable".to_string())
}

/// 이 연결이 grant를 썼다. 끊기면 grant도 함께 지운다
fn mark_written(state: &mut AgentState, client_id: u64) {
    if let Some(client) = state.clients.get_mut(&client_id) {
        client.has_written = true;
    }
}
