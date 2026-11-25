//! agent 상태와 연결 하나하나의 붙기·떼기·닫기(TS `detach`, `client.close`, `revoke`).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::Instant;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;
use serde_json::{Value, json};

use super::Agent;
use super::direct::DirectChannel;
use crate::outbox::RecordOutbox;

/// 데스크탑이 보낸 grant(TS `RemoteTerminalWireGrant`)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WireGrant {
    pub grant_id: String,
    pub token: String,
    pub device_id: String,
    pub task_id: String,
    pub pane_id: String,
    pub target_key: String,
    pub cols: u32,
    pub rows: u32,
    pub geometry_command: String,
    pub control_command: String,
    pub snapshot_command: String,
}

pub(super) struct GrantSlot {
    pub wire: Arc<WireGrant>,
    pub admission_until: Instant,
    pub active: Option<u64>,
}

pub(super) enum ClientChannel {
    Direct(Arc<DirectChannel>),
    Relay { relay_id: String },
}

pub(super) struct Client {
    pub grant: Arc<WireGrant>,
    pub channel: ClientChannel,
    pub size: Option<(u32, u32)>,
    pub order: u64,
    pub has_written: bool,
    pub is_ready: bool,
    pub is_closed: bool,
    pub is_attached: bool,
    /// 붙기 전에 온 입력(바이트, 바이너리 여부)
    pub pending_input: Vec<(Vec<u8>, bool)>,
    pub pending_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Readiness {
    Pending,
    Ready,
    Failed,
}

pub(super) struct Controller {
    pub stdin: Arc<RecordOutbox>,
    pub pid: u32,
}

pub(super) struct Joiner {
    pub history: String,
    pub reply: Sender<Result<(), String>>,
}

/// 같은 pane을 함께 쓰는 연결과 controller 하나(TS `Group`)
pub(super) struct Group {
    pub target_key: String,
    pub grant: Arc<WireGrant>,
    pub clients: Vec<u64>,
    pub controller: Option<Controller>,
    pub history: String,
    pub is_initialized: bool,
    pub owns_terminal: bool,
    pub original: Option<(u32, u32)>,
    pub size: (u32, u32),
    pub joiners: HashMap<u64, Joiner>,
    pub is_dead: bool,
    pub order: u64,
    pub readiness: Readiness,
    pub is_bootstrapping: bool,
    pub buffered_frames: String,
    pub has_initial_frame: bool,
    pub has_controller_exited: bool,
}

pub(crate) struct AgentState {
    pub(super) grants: HashMap<String, GrantSlot>,
    /// target key에서 지금 그 pane을 맡은 group id로
    pub(super) groups: HashMap<String, u64>,
    pub(super) group_store: HashMap<u64, Group>,
    /// controller가 아직 끝나지 않은 닫힌 group. 같은 pane의 새 group은 이 controller가 끝나기를 기다린다
    pub(super) closing_groups: HashMap<String, u64>,
    pub(super) clients: HashMap<u64, Client>,
    pub(super) relays: HashMap<String, u64>,
    /// 떠 있는 조회 프로세스 그룹
    pub(super) queries: Vec<i32>,
    pub(super) last_owner_heartbeat: Instant,
    pub(super) next_id: u64,
    pub(super) should_stop: bool,
    pub(super) is_stopped: bool,
}

impl AgentState {
    pub(super) fn new() -> Self {
        Self {
            grants: HashMap::new(),
            groups: HashMap::new(),
            group_store: HashMap::new(),
            closing_groups: HashMap::new(),
            clients: HashMap::new(),
            relays: HashMap::new(),
            queries: Vec::new(),
            last_owner_heartbeat: Instant::now(),
            next_id: 1,
            should_stop: false,
            is_stopped: false,
        }
    }

    pub(super) fn allocate_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub(super) fn add_client(&mut self, grant: Arc<WireGrant>, channel: ClientChannel) -> u64 {
        let id = self.allocate_id();
        let client = Client {
            grant,
            channel,
            size: None,
            order: 0,
            has_written: false,
            is_ready: false,
            is_closed: false,
            is_attached: false,
            pending_input: Vec::new(),
            pending_bytes: 0,
        };
        self.clients.insert(id, client);
        id
    }

    /// grant가 아직 살아 있고 이 연결이 그 grant를 쥐고 있는지
    pub(super) fn is_active_client(&self, client_id: u64) -> bool {
        let Some(client) = self.clients.get(&client_id) else {
            return false;
        };
        self.grants
            .get(&client.grant.grant_id)
            .is_some_and(|slot| slot.active == Some(client_id))
    }
}

/// 닫힌 relay 연결은 소켓 스레드가 없어 닫는 자리에서 지운다
fn relay_id_is_some(state: &AgentState, client_id: u64) -> bool {
    state
        .clients
        .get(&client_id)
        .is_some_and(|client| matches!(client.channel, ClientChannel::Relay { .. }))
}

impl Agent {
    /// 연결에 화면 출력을 보낸다
    pub(super) fn send_to_client(&self, state: &mut AgentState, client_id: u64, text: &str) {
        let Some(client) = state
            .clients
            .get(&client_id)
            .filter(|client| !client.is_closed)
        else {
            return;
        };
        match &client.channel {
            ClientChannel::Direct(channel) => channel.push_text(text),
            ClientChannel::Relay { relay_id } => {
                let record =
                    json!({ "type": "output", "relayId": relay_id, "data": BASE64.encode(text) });
                self.emit(state, record);
            }
        }
    }

    /// 연결을 닫고 group에서 뗀다(TS `client.close`)
    pub(super) fn close_client(
        &self,
        state: &mut AgentState,
        client_id: u64,
        code: u16,
        reason: &str,
    ) {
        let Some(client) = state
            .clients
            .get_mut(&client_id)
            .filter(|client| !client.is_closed)
        else {
            return;
        };
        client.is_closed = true;
        let relay_id = match &client.channel {
            ClientChannel::Direct(channel) => {
                channel.request_close(code, reason);
                None
            }
            ClientChannel::Relay { relay_id } => Some(relay_id.clone()),
        };
        if let Some(relay_id) =
            relay_id.filter(|relay_id| state.relays.get(relay_id) == Some(&client_id))
        {
            state.relays.remove(&relay_id);
            let record =
                json!({ "type": "closed", "relayId": relay_id, "code": code, "reason": reason });
            self.emit(state, record);
        }
        self.detach(state, client_id);
        if relay_id_is_some(state, client_id) {
            state.clients.remove(&client_id);
        }
    }

    /// 연결을 group에서 뗀다. 마지막 연결이면 group을 닫고, 아니면 남은 연결 가운데 마지막으로 크기를 정한 쪽 크기로 맞춘다
    pub(super) fn detach(&self, state: &mut AgentState, client_id: u64) {
        let Some(client) = state.clients.get(&client_id) else {
            return;
        };
        let (grant_id, target_key, has_written) = (
            client.grant.grant_id.clone(),
            client.grant.target_key.clone(),
            client.has_written,
        );
        if let Some(slot) = state
            .grants
            .get_mut(&grant_id)
            .filter(|slot| slot.active == Some(client_id))
        {
            slot.active = None;
        }
        if has_written {
            state.grants.remove(&grant_id);
        }
        let Some(&group_id) = state.groups.get(&target_key) else {
            return;
        };
        let Some(group) = state.group_store.get_mut(&group_id) else {
            return;
        };
        let Some(position) = group.clients.iter().position(|id| *id == client_id) else {
            return;
        };
        group.clients.remove(position);
        if group.is_dead {
            return;
        }
        if let Some(joiner) = group.joiners.remove(&client_id) {
            let _ = joiner.reply.send(Err("Terminal client detached".into()));
        }
        if group.clients.is_empty() {
            self.close_group(state, group_id, 1000, "detached");
            return;
        }
        self.resize_to_latest_owner(state, group_id);
    }

    fn resize_to_latest_owner(&self, state: &mut AgentState, group_id: u64) {
        let Some(group) = state
            .group_store
            .get(&group_id)
            .filter(|group| group.controller.is_some())
        else {
            return;
        };
        let owner_size = group
            .clients
            .iter()
            .filter_map(|id| state.clients.get(id))
            .filter_map(|client| client.size.map(|size| (client.order, size)))
            .max_by_key(|(order, _)| *order)
            .map(|(_, size)| size);
        let size = owner_size.unwrap_or((group.grant.cols, group.grant.rows));
        if let Some(group) = state.group_store.get_mut(&group_id) {
            group.size = size;
        }
        let record = json!({ "type": "terminal.resize", "cols": size.0, "rows": size.1 });
        if self.send_controller(state, group_id, &record).is_err() {
            self.close_group(state, group_id, 4000, "stream-failed");
        }
    }

    /// controller stdin에 NDJSON 한 줄을 넣는다
    pub(super) fn send_controller(
        &self,
        state: &AgentState,
        group_id: u64,
        record: &Value,
    ) -> Result<(), String> {
        let unavailable = || "Terminal input unavailable".to_string();
        let group = state
            .group_store
            .get(&group_id)
            .filter(|group| !group.is_dead)
            .ok_or_else(unavailable)?;
        let controller = group.controller.as_ref().ok_or_else(unavailable)?;
        let mut line = record.to_string().into_bytes();
        line.push(b'\n');
        controller.stdin.send_bytes(line).map_err(|_| unavailable())
    }

    /// 조건에 맞는 grant를 지우고 그 grant를 쥔 연결을 닫는다
    pub(super) fn revoke(
        &self,
        state: &mut AgentState,
        is_target: impl Fn(&WireGrant) -> bool,
        code: u16,
        reason: &str,
    ) {
        let revoked: Vec<String> = state
            .grants
            .values()
            .filter(|slot| is_target(&slot.wire))
            .map(|slot| slot.wire.grant_id.clone())
            .collect();
        for grant_id in revoked {
            let active = state.grants.remove(&grant_id).and_then(|slot| slot.active);
            if let Some(client_id) = active {
                self.close_client(state, client_id, code, reason);
            }
        }
    }
}
