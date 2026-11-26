//! 소유자(데스크탑)가 stdin으로 보내는 NDJSON 봉투(TS `handle`, `onInput`).
//!
//! `heartbeat`, `shutdown`, relay `data`/`detach`는 바로 처리하고, `request`는 `response`로 답한다. 붙기(`attach`)는
//! 기본 크기 읽기와 controller 준비를 기다리므로 따로 스레드에서 처리해 다른 봉투를 막지 않는다.

use std::io::{BufRead, BufReader, Read};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::Agent;
use super::attach::AttachTicket;
use super::state::{AgentState, ClientChannel, GrantSlot, WireGrant};
use super::wire::{
    REMOTE_TERMINAL_ADMISSION_MS, REMOTE_TERMINAL_ENVELOPE_BYTES, REMOTE_TERMINAL_INPUT_BYTES,
    decode_remote_terminal_bytes, is_remote_terminal_size,
};

pub(super) fn spawn_reader(agent: &Arc<Agent>, input: Box<dyn Read + Send>) {
    let agent = Arc::clone(agent);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(input);
        loop {
            let mut line = Vec::new();
            let limit = REMOTE_TERMINAL_ENVELOPE_BYTES as u64 + 1;
            let count = reader.by_ref().take(limit).read_until(b'\n', &mut line);
            let is_finished = !matches!(count, Ok(count) if count > 0 && (count as u64) < limit);
            let record = serde_json::from_slice::<Value>(&line);
            if is_finished || record.is_err() || !agent.handle_envelope(record.unwrap_or_default())
            {
                agent.stop();
                return;
            }
        }
    });
}

impl Agent {
    /// 봉투 하나를 처리한다. agent를 멈춰야 하면 false다
    fn handle_envelope(self: &Arc<Self>, record: Value) -> bool {
        let mut state = self.lock();
        if state.is_stopped {
            return false;
        }
        let kind = record
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let relay = |state: &AgentState| {
            let relay_id = record.get("relayId").map(value_text).unwrap_or_default();
            state.relays.get(&relay_id).copied()
        };
        match kind {
            "heartbeat" => state.last_owner_heartbeat = Instant::now(),
            "shutdown" => return false,
            "detach" => {
                if let Some(client_id) = relay(&state) {
                    self.close_client(&mut state, client_id, 1000, "detached");
                }
            }
            "data" => {
                if let Some(client_id) = relay(&state) {
                    self.receive_relay_data(&mut state, client_id, &record);
                }
            }
            "request" if record.get("id").is_some_and(Value::is_string) => {
                drop(state);
                self.handle_request(record);
            }
            _ => return false,
        }
        true
    }

    fn receive_relay_data(&self, state: &mut AgentState, client_id: u64, record: &Value) {
        let is_binary = record.get("binary") == Some(&Value::Bool(true));
        let result = decode_remote_terminal_bytes(record.get("data"), REMOTE_TERMINAL_INPUT_BYTES)
            .and_then(|bytes| self.receive(state, client_id, &bytes, is_binary));
        if result.is_err() {
            self.close_client(state, client_id, 4000, "stream-failed");
        }
    }

    fn handle_request(self: &Arc<Self>, record: Value) {
        let id = record["id"].as_str().unwrap_or_default().to_string();
        let method = record
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if method == "attach" {
            let ticket = self.begin_relay_attach(&record);
            let agent = Arc::clone(self);
            std::thread::spawn(move || {
                let result = ticket
                    .and_then(|(ticket, client_id)| agent.complete_relay_attach(ticket, client_id));
                agent.respond(&id, result);
            });
            return;
        }
        let mut state = self.lock();
        let result = match method.as_str() {
            "grant" => store_grant(&mut state, &record),
            _ => self.revoke_by_request(&mut state, &method, &record),
        };
        drop(state);
        self.respond(&id, result);
    }

    /// 기기·task·grant 단위 철회. 각각 4001, 4002, 4000으로 닫는다
    fn revoke_by_request(
        &self,
        state: &mut AgentState,
        method: &str,
        record: &Value,
    ) -> Result<(), String> {
        let (field, code, reason) = match method {
            "revokeDevice" => ("deviceId", 4001, "device-unpaired"),
            "revokeTask" => ("taskId", 4002, "session-changed"),
            "revokeGrant" => ("grantId", 4000, "grant-revoked"),
            _ => return Err("Unknown terminal request".into()),
        };
        let target = record.get(field).map(value_text).unwrap_or_default();
        let matches_target = |grant: &WireGrant| match field {
            "deviceId" => grant.device_id == target,
            "taskId" => grant.task_id == target,
            _ => grant.grant_id == target,
        };
        self.revoke(state, matches_target, code, reason);
        Ok(())
    }

    fn respond(&self, id: &str, result: Result<(), String>) {
        let response = match result {
            Ok(()) => json!({ "type": "response", "id": id, "ok": true }),
            Err(_) => {
                json!({ "type": "response", "id": id, "ok": false, "error": "Terminal request rejected" })
            }
        };
        let mut state = self.lock();
        self.emit(&mut state, response);
    }

    /// 데스크탑 relay 연결을 만들고 grant를 쥐게 한다. scope가 grant와 다르면 거절한다
    fn begin_relay_attach(&self, record: &Value) -> Result<(AttachTicket, u64), String> {
        let grant_id = record.get("grantId").map(value_text).unwrap_or_default();
        let relay_id = record
            .get("relayId")
            .and_then(Value::as_str)
            .ok_or("Invalid terminal scope")?
            .to_string();
        let mut state = self.lock();
        let slot = state
            .grants
            .get(&grant_id)
            .ok_or("Invalid terminal scope")?;
        let wire = Arc::clone(&slot.wire);
        let field = |name: &str| record.get(name).and_then(Value::as_str);
        let is_in_scope = field("deviceId") == Some(wire.device_id.as_str())
            && field("taskId") == Some(wire.task_id.as_str())
            && field("paneId") == Some(wire.pane_id.as_str());
        if !is_in_scope {
            return Err("Invalid terminal scope".into());
        }
        let client_id = state.add_client(
            wire,
            ClientChannel::Relay {
                relay_id: relay_id.clone(),
            },
        );
        state.relays.insert(relay_id, client_id);
        match self.begin_attach(&mut state, &grant_id, client_id) {
            Ok(ticket) => Ok((ticket, client_id)),
            Err(error) => {
                self.close_client(&mut state, client_id, 4000, "stream-failed");
                Err(error)
            }
        }
    }

    fn complete_relay_attach(
        self: &Arc<Self>,
        ticket: AttachTicket,
        client_id: u64,
    ) -> Result<(), String> {
        let result = self.complete_attach(ticket, client_id);
        if result.is_err() {
            self.close_client(&mut self.lock(), client_id, 4000, "stream-failed");
        }
        result
    }
}

/// grant를 검사해 저장한다. 입장 시간은 데스크탑이 적은 만료 시각이 아니라 이 agent의 시계로 잰다
fn store_grant(state: &mut AgentState, record: &Value) -> Result<(), String> {
    let invalid = || "Invalid terminal grant".to_string();
    let raw = record.get("grant").ok_or_else(invalid)?;
    let is_valid = raw
        .get("token")
        .and_then(Value::as_str)
        .is_some_and(|token| token.len() >= 32)
        && is_remote_terminal_size(raw.get("cols"), raw.get("rows"))
        && ["geometryCommand", "controlCommand", "snapshotCommand"]
            .iter()
            .all(|field| raw.get(*field).is_some_and(Value::is_string));
    if !is_valid {
        return Err(invalid());
    }
    let wire: WireGrant = serde_json::from_value(raw.clone()).map_err(|_| invalid())?;
    let admission_until = Instant::now() + Duration::from_millis(REMOTE_TERMINAL_ADMISSION_MS);
    let slot = GrantSlot {
        wire: Arc::new(wire),
        admission_until,
        active: None,
    };
    state.grants.insert(slot.wire.grant_id.clone(), slot);
    Ok(())
}

/// JS `String(value)`
fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
