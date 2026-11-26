//! 원격 Herdr pane을 모바일에 직접 잇는 grant 서비스(TS `desktop/main/services/remoteTerminalService.ts`).
//!
//! SSH 호스트마다 원격 terminal agent 하나를 ssh stdio로 띄우고, 같은 설정의 동시 요청은 시작 중인 agent를 함께 기다린다.
//! 소스·인증서 키·grant 권한은 argv가 아니라 stdin 레코드로만 넘긴다. 모바일은 grant의 직접 주소로 agent WSS에 붙고,
//! 직접 연결이 안 되면 데스크탑 relay([`RemoteTerminalRelay`])로 같은 grant를 쓴다.
//! agent 실행 명령·소스, TLS 인증서, ssh 프로세스는 [`agent_process`]의 trait 뒤에 있어 M4 agent로 바꿔 끼운다.

mod agent_connection;
mod agent_process;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use kanvibe_ai::javascript::stringify;
use kanvibe_process::ssh_config::{
    SshArgsOptions, SshConfigReader, SshHostConfig, build_ssh_args,
    kanvibe_ssh_connection_health_options,
};
use kanvibe_process::worktree::quote_for_posix_shell;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use self::agent_connection::{AgentConnection, LaunchedAgentConnection, RelayRoute, RelayState};
pub use self::agent_process::{
    NodeRemoteTerminalAgentBundle, RemoteTerminalAgentBundle, RemoteTerminalAgentProcess,
    RemoteTerminalAgentSpawner, RemoteTerminalAgentTermination, RemoteTerminalCertificate,
    RemoteTerminalCertificateIssuer, SystemRemoteTerminalAgentSpawner,
};
use crate::herdr::build_herdr_command;
use crate::pane_mirror::{build_herdr_control_pane_command, build_herdr_recent_screen_command};
use crate::remote_terminal_protocol::{
    REMOTE_TERMINAL_ADMISSION_MS, REMOTE_TERMINAL_GRANT_VERSION, REMOTE_TERMINAL_INPUT_BYTES,
    REMOTE_TERMINAL_MAX_DIMENSION, RemoteTerminalDirectRoute, RemoteTerminalGrant,
    RemoteTerminalGrantInput,
};

/// 미지원 런타임 결과는 descriptor 조회와 예전 방식 fallback이 잠시 함께 쓴다
const UNSUPPORTED_RUNTIME_CACHE: Duration = Duration::from_millis(60_000);
pub const CERTIFICATE_VALIDITY_MS: i64 = 24 * 60 * 60_000;
const DEFAULT_NATIVE_COLUMNS: u32 = 80;
const DEFAULT_NATIVE_ROWS: u32 = 24;
const GRANT_TOKEN_BYTES: usize = 32;

/// 서비스가 쓰는 협력자. 앱이 시작할 때 한 번 만든다
pub struct RemoteTerminalServiceDependencies {
    pub ssh_hosts: Arc<SshConfigReader>,
    pub agent_bundle: Arc<dyn RemoteTerminalAgentBundle>,
    pub certificate_issuer: Arc<dyn RemoteTerminalCertificateIssuer>,
    pub agent_spawner: Arc<dyn RemoteTerminalAgentSpawner>,
}

pub struct RemoteTerminalService {
    shared: Arc<ServiceShared>,
}

struct ServiceShared {
    dependencies: RemoteTerminalServiceDependencies,
    state: Mutex<ServiceState>,
    next_agent_id: AtomicU64,
}

#[derive(Default)]
struct ServiceState {
    /// ssh 설정 JSON에서 그 설정의 agent 시작 결과로
    agent_slots: HashMap<String, Arc<AgentSlot>>,
    live_agents: Vec<Arc<AgentConnection>>,
    owned_agents: Vec<Arc<AgentConnection>>,
    grants: HashMap<String, GrantRecord>,
    device_epochs: HashMap<String, u64>,
    task_epochs: HashMap<String, u64>,
    owner_epoch: u64,
}

struct GrantRecord {
    descriptor: RemoteTerminalGrant,
    input: RemoteTerminalGrantInput,
    agent: Arc<AgentConnection>,
}

/// grant 준비를 시작할 때의 취소 세대. 준비 중 소유자·기기·task가 철회되면 값이 달라진다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GrantEpochs {
    owner: u64,
    device: u64,
    task: u64,
}

impl ServiceState {
    fn grant_epochs(&self, input: &RemoteTerminalGrantInput) -> GrantEpochs {
        GrantEpochs {
            owner: self.owner_epoch,
            device: self
                .device_epochs
                .get(&input.device_id)
                .copied()
                .unwrap_or(0),
            task: self.task_epochs.get(&input.task_id).copied().unwrap_or(0),
        }
    }
}

enum SlotOutcome {
    Starting { should_stop_when_ready: bool },
    Ready(Arc<AgentConnection>),
    Unsupported { expires_at: Instant },
    Failed(String),
}

/// 같은 ssh 설정의 agent 시작 결과. 시작 중이면 뒤에 온 요청이 결과를 함께 기다린다(TS `agents` 약속 공유)
struct AgentSlot {
    outcome: Mutex<SlotOutcome>,
    settled: Condvar,
}

impl AgentSlot {
    fn starting() -> Arc<Self> {
        Arc::new(Self {
            outcome: Mutex::new(SlotOutcome::Starting {
                should_stop_when_ready: false,
            }),
            settled: Condvar::new(),
        })
    }

    fn is_reusable(&self) -> bool {
        match &*self.outcome.lock().unwrap() {
            SlotOutcome::Unsupported { expires_at } => Instant::now() < *expires_at,
            SlotOutcome::Failed(_) => false,
            SlotOutcome::Starting { .. } | SlotOutcome::Ready(_) => true,
        }
    }

    /// 결과를 기록한다. 시작하는 동안 소유자가 멈췄으면 멈출 agent 연결을 돌려준다
    fn settle(
        &self,
        result: Result<Option<Arc<AgentConnection>>, String>,
    ) -> Option<Arc<AgentConnection>> {
        let mut outcome = self.outcome.lock().unwrap();
        let should_stop = matches!(
            *outcome,
            SlotOutcome::Starting {
                should_stop_when_ready: true
            }
        );
        *outcome = match result {
            Ok(Some(agent)) => SlotOutcome::Ready(agent),
            Ok(None) => SlotOutcome::Unsupported {
                expires_at: Instant::now() + UNSUPPORTED_RUNTIME_CACHE,
            },
            Err(message) => SlotOutcome::Failed(message),
        };
        self.settled.notify_all();
        match &*outcome {
            SlotOutcome::Ready(agent) if should_stop => Some(agent.clone()),
            _ => None,
        }
    }

    fn wait(&self) -> Result<Option<Arc<AgentConnection>>, String> {
        let outcome = self.outcome.lock().unwrap();
        let outcome = self
            .settled
            .wait_while(outcome, |outcome| {
                matches!(outcome, SlotOutcome::Starting { .. })
            })
            .unwrap();
        match &*outcome {
            SlotOutcome::Ready(agent) => Ok(Some(agent.clone())),
            SlotOutcome::Unsupported { .. } => Ok(None),
            SlotOutcome::Failed(message) => Err(message.clone()),
            SlotOutcome::Starting { .. } => unreachable!("시작이 끝날 때까지 기다렸다"),
        }
    }

    /// 소유자가 멈췄다. 준비된 agent 연결은 돌려주어 멈추게 하고, 시작 중이면 준비되는 대로 멈추게 표시한다
    fn request_stop(&self) -> Option<Arc<AgentConnection>> {
        let mut outcome = self.outcome.lock().unwrap();
        match &mut *outcome {
            SlotOutcome::Ready(agent) => Some(agent.clone()),
            SlotOutcome::Starting {
                should_stop_when_ready,
            } => {
                *should_stop_when_ready = true;
                None
            }
            SlotOutcome::Unsupported { .. } | SlotOutcome::Failed(_) => None,
        }
    }

    fn ready_agent_id(&self) -> Option<u64> {
        match &*self.outcome.lock().unwrap() {
            SlotOutcome::Ready(agent) => Some(agent.agent_id),
            _ => None,
        }
    }
}

impl RemoteTerminalService {
    pub fn new(dependencies: RemoteTerminalServiceDependencies) -> Self {
        Self {
            shared: Arc::new(ServiceShared {
                dependencies,
                state: Mutex::new(ServiceState::default()),
                next_agent_id: AtomicU64::new(1),
            }),
        }
    }

    /// pane 하나의 일회성 직접 연결 grant를 만든다. 원격에 agent 런타임이 없으면 `None`이다
    pub fn prepare_grant(
        &self,
        input: &RemoteTerminalGrantInput,
    ) -> Result<Option<RemoteTerminalGrant>, String> {
        let shared = &self.shared;
        let epochs = shared.lock_state().grant_epochs(input);
        let config = shared.find_ssh_config(&input.ssh_host)?;
        let Some(agent) = shared.acquire_agent(&config)? else {
            return Ok(None);
        };
        shared.ensure_grant_current(input, epochs)?;
        let grant_id = uuid::Uuid::new_v4().to_string();
        let token = create_grant_token()?;
        let expires_at = now_ms() + REMOTE_TERMINAL_ADMISSION_MS as i64;
        let wire_grant = build_wire_grant(input, &grant_id, &token, expires_at)?;
        agent.request("grant", request_payload(json!({ "grant": wire_grant })))?;
        if let Err(error) = shared.ensure_grant_current(input, epochs) {
            cancel_prepared_grant(agent, &grant_id);
            return Err(error);
        }
        let descriptor = RemoteTerminalGrant {
            version: REMOTE_TERMINAL_GRANT_VERSION,
            grant_id: grant_id.clone(),
            expires_at,
            direct: RemoteTerminalDirectRoute {
                host: config.hostname.clone(),
                port: agent.port(),
                path: format!("/stream/{grant_id}"),
                token,
                certificate_fingerprint: agent.fingerprint.clone(),
            },
        };
        let record = GrantRecord {
            descriptor: descriptor.clone(),
            input: input.clone(),
            agent,
        };
        shared.lock_state().grants.insert(grant_id, record);
        Ok(Some(descriptor))
    }

    /// 직접 연결이 안 될 때 데스크탑이 grant의 pane을 중계한다. 범위가 grant와 다르거나 만료됐으면 거절한다
    pub fn attach_relay(
        &self,
        grant_id: &str,
        device_id: &str,
        task_id: &str,
        pane_id: &str,
        on_output: impl Fn(String) + Send + Sync + 'static,
        on_close: impl Fn(u16, String) + Send + Sync + 'static,
    ) -> Result<RemoteTerminalRelay, String> {
        let agent = self
            .shared
            .find_scoped_grant_agent(grant_id, device_id, task_id, pane_id)
            .ok_or_else(|| "Invalid remote terminal scope".to_string())?;
        let relay_id = uuid::Uuid::new_v4().to_string();
        let state = RelayState::new(Arc::new(on_close));
        let route = RelayRoute {
            on_output: Arc::new(on_output),
            state: state.clone(),
        };
        agent.add_relay(&relay_id, route);
        let relay = RemoteTerminalRelay {
            agent,
            relay_id,
            state,
        };
        let payload = request_payload(json!({
            "relayId": relay.relay_id,
            "grantId": grant_id,
            "deviceId": device_id,
            "taskId": task_id,
            "paneId": pane_id,
        }));
        if let Err(error) = relay.agent.request("attach", payload) {
            relay.close();
            return Err(error);
        }
        Ok(relay)
    }

    /// 준비했거나 쓰는 중인 경로 하나만 취소한다. 같은 task의 다른 pane은 그대로 둔다
    pub fn revoke_grant(&self, grant_id: &str) {
        let targets = {
            let mut state = self.shared.lock_state();
            match state.grants.remove(grant_id) {
                Some(record) => vec![record.agent],
                // 입장 기록은 만료돼 지워졌어도 직접 스트림은 살아 있을 수 있다
                None => state.live_agents.clone(),
            }
        };
        revoke_in_background(targets, "revokeGrant", json!({ "grantId": grant_id }));
    }

    pub fn revoke_device(&self, device_id: &str) {
        let targets = {
            let mut state = self.shared.lock_state();
            *state
                .device_epochs
                .entry(device_id.to_string())
                .or_default() += 1;
            state
                .grants
                .retain(|_, record| record.input.device_id != device_id);
            state.live_agents.clone()
        };
        revoke_in_background(targets, "revokeDevice", json!({ "deviceId": device_id }));
    }

    pub fn revoke_task(&self, task_id: &str) {
        let targets = {
            let mut state = self.shared.lock_state();
            *state.task_epochs.entry(task_id.to_string()).or_default() += 1;
            state
                .grants
                .retain(|_, record| record.input.task_id != task_id);
            state.live_agents.clone()
        };
        revoke_in_background(targets, "revokeTask", json!({ "taskId": task_id }));
    }

    /// 데스크탑이 소유한 agent를 모두 멈추고 시작 중인 agent도 준비되는 대로 멈춘다
    pub fn stop_agents(&self) {
        let (owned_agents, agent_slots) = {
            let mut state = self.shared.lock_state();
            state.owner_epoch += 1;
            state.grants.clear();
            let agent_slots: Vec<Arc<AgentSlot>> =
                state.agent_slots.drain().map(|(_, slot)| slot).collect();
            (state.owned_agents.clone(), agent_slots)
        };
        for agent in owned_agents {
            agent.stop();
        }
        for slot in agent_slots {
            if let Some(agent) = slot.request_stop() {
                agent.stop();
            }
        }
    }
}

impl ServiceShared {
    fn lock_state(&self) -> MutexGuard<'_, ServiceState> {
        self.state.lock().unwrap()
    }

    fn find_ssh_config(&self, ssh_host: &str) -> Result<SshHostConfig, String> {
        self.dependencies
            .ssh_hosts
            .parse_ssh_config()
            .into_iter()
            .find(|config| config.host == ssh_host)
            .ok_or_else(|| "Remote terminal SSH host not found".to_string())
    }

    fn ensure_grant_current(
        &self,
        input: &RemoteTerminalGrantInput,
        epochs: GrantEpochs,
    ) -> Result<(), String> {
        if self.lock_state().grant_epochs(input) == epochs {
            return Ok(());
        }
        Err("Remote terminal grant revoked during preparation".to_string())
    }

    /// 같은 ssh 설정의 agent를 함께 쓴다. 없으면 이 호출이 시작하고, 시작 중이면 결과를 기다린다
    fn acquire_agent(
        self: &Arc<Self>,
        config: &SshHostConfig,
    ) -> Result<Option<Arc<AgentConnection>>, String> {
        let slot_key = config.identity_json();
        let (slot, is_starter) = {
            let mut state = self.lock_state();
            match state.agent_slots.get(&slot_key) {
                Some(slot) if slot.is_reusable() => (slot.clone(), false),
                _ => {
                    let slot = AgentSlot::starting();
                    state.agent_slots.insert(slot_key.clone(), slot.clone());
                    (slot, true)
                }
            }
        };
        if is_starter {
            let result = self.start_agent(config, &slot_key);
            if result.is_err() {
                self.forget_slot(&slot_key, &slot);
            }
            if let Some(cancelled_agent) = slot.settle(result) {
                cancelled_agent.stop();
            }
        }
        slot.wait()
    }

    fn start_agent(
        self: &Arc<Self>,
        config: &SshHostConfig,
        slot_key: &str,
    ) -> Result<Option<Arc<AgentConnection>>, String> {
        let epoch = self.lock_state().owner_epoch;
        let source = self.dependencies.agent_bundle.read_source()?;
        let certificate = self
            .dependencies
            .certificate_issuer
            .issue(now_ms() + CERTIFICATE_VALIDITY_MS)?;
        if epoch != self.lock_state().owner_epoch {
            return Err("Remote terminal preparation cancelled".to_string());
        }
        let fingerprint = certificate_fingerprint(&certificate.certificate_pem)?;
        let ssh_arguments =
            build_agent_ssh_arguments(config, &self.dependencies.agent_bundle.launch_command());
        let process = self
            .dependencies
            .agent_spawner
            .spawn(&config.host, &ssh_arguments)
            .map_err(|_| "Remote terminal helper disconnected".to_string())?;
        let agent_id = self.next_agent_id.fetch_add(1, Ordering::SeqCst);
        let launched =
            LaunchedAgentConnection::new(agent_id, fingerprint, process, Arc::downgrade(self));
        let is_cancelled = {
            let mut state = self.lock_state();
            state.owned_agents.push(launched.agent.clone());
            state.owner_epoch != epoch
        };
        let bootstrap_record = json!({
            "source": source,
            "certificate": { "cert": certificate.certificate_pem, "key": certificate.private_key_pem },
        });
        let agent = launched.start_io(bootstrap_record, slot_key.to_string());
        if is_cancelled {
            agent.stop();
        }
        Ok(agent.wait_until_ready()?.then_some(agent))
    }

    fn forget_slot(&self, slot_key: &str, slot: &Arc<AgentSlot>) {
        let mut state = self.lock_state();
        if state
            .agent_slots
            .get(slot_key)
            .is_some_and(|current| Arc::ptr_eq(current, slot))
        {
            state.agent_slots.remove(slot_key);
        }
    }

    /// 준비됐던 agent 연결의 프로세스가 끝났다. 그 agent 연결을 담은 결과일 때만 지운다
    fn forget_closed_agent_slot(&self, slot_key: &str, agent_id: u64) {
        let mut state = self.lock_state();
        let is_same_agent = state
            .agent_slots
            .get(slot_key)
            .is_some_and(|slot| slot.ready_agent_id() == Some(agent_id));
        if is_same_agent {
            state.agent_slots.remove(slot_key);
        }
    }

    fn forget_agent(&self, agent_id: u64) {
        let mut state = self.lock_state();
        state.live_agents.retain(|agent| agent.agent_id != agent_id);
        state
            .owned_agents
            .retain(|agent| agent.agent_id != agent_id);
        state
            .grants
            .retain(|_, record| record.agent.agent_id != agent_id);
    }

    fn remember_live_agent(&self, agent: Arc<AgentConnection>) {
        self.lock_state().live_agents.push(agent);
    }

    fn forget_expired_grants(&self) {
        let now = now_ms();
        self.lock_state()
            .grants
            .retain(|_, record| record.descriptor.expires_at > now);
    }

    fn find_scoped_grant_agent(
        &self,
        grant_id: &str,
        device_id: &str,
        task_id: &str,
        pane_id: &str,
    ) -> Option<Arc<AgentConnection>> {
        let state = self.lock_state();
        let record = state.grants.get(grant_id)?;
        let is_in_scope = record.input.device_id == device_id
            && record.input.task_id == task_id
            && record.input.pane_id == pane_id
            && record.descriptor.expires_at > now_ms();
        is_in_scope.then(|| record.agent.clone())
    }
}

/// 데스크탑이 중계하는 pane 연결 하나(TS `Relay`)
pub struct RemoteTerminalRelay {
    agent: Arc<AgentConnection>,
    relay_id: String,
    state: Arc<RelayState>,
}

impl RemoteTerminalRelay {
    /// 입력을 agent에 넘긴다. 한 번에 넘칠 만큼 크거나 보내지 못하면 relay를 닫고 닫힘 콜백을 부른다
    pub fn write(&self, input: &[u8], is_binary: bool) {
        if self.state.is_closed() {
            return;
        }
        if input.len() > REMOTE_TERMINAL_INPUT_BYTES {
            self.close();
            self.state.notify_close(4000, "input-too-large");
            return;
        }
        let record = json!({
            "type": "data",
            "relayId": self.relay_id,
            "binary": is_binary,
            "data": BASE64.encode(input),
        });
        if self.agent.send(&record).is_err() {
            self.close();
            self.state.notify_close(4000, "stream-failed");
        }
    }

    /// 데스크탑 쪽에서 닫는다. 닫힘 콜백은 부르지 않는다
    pub fn close(&self) {
        if !self.state.mark_closed() {
            return;
        }
        self.agent.remove_relay(&self.relay_id);
        // 소유자를 잃은 agent 연결은 이미 스스로 닫혔다
        let _ = self
            .agent
            .send(&json!({ "type": "detach", "relayId": self.relay_id }));
    }
}

fn build_agent_ssh_arguments(config: &SshHostConfig, launch_command: &str) -> Vec<String> {
    let options = SshArgsOptions {
        is_tty_disabled: true,
        should_check_host_key_strictly: true,
        connection_health: Some(kanvibe_ssh_connection_health_options()),
        ..SshArgsOptions::default()
    };
    let mut arguments = build_ssh_args(config, &options);
    arguments.push(format!("sh -lc {}", quote_for_posix_shell(launch_command)));
    arguments
}

/// agent에 넘기는 grant. 키 순서는 TS 객체 리터럴과 같다
fn build_wire_grant(
    input: &RemoteTerminalGrantInput,
    grant_id: &str,
    token: &str,
    expires_at: i64,
) -> Result<Value, String> {
    let native_columns = input.cols.unwrap_or(DEFAULT_NATIVE_COLUMNS);
    let native_rows = input.rows.unwrap_or(DEFAULT_NATIVE_ROWS);
    if native_columns == 0 || native_rows == 0 {
        return Err("Invalid native terminal geometry".to_string());
    }
    let columns = native_columns.min(REMOTE_TERMINAL_MAX_DIMENSION);
    let rows = native_rows.min(REMOTE_TERMINAL_MAX_DIMENSION);
    let session_name = input.session_name.as_str();
    let pane_id = input.pane_id.as_str();
    let control_command =
        build_herdr_control_pane_command(session_name, pane_id, columns.into(), rows.into())?;
    let snapshot_command = build_herdr_recent_screen_command(session_name, pane_id)?;
    let geometry_command = format!(
        "{} && printf '\\036' && {}",
        build_herdr_command(session_name, &["pane", "get", pane_id])?,
        build_herdr_command(session_name, &["pane", "layout", "--pane", pane_id])?,
    );
    Ok(json!({
        "grantId": grant_id,
        "token": token,
        "expiresAt": expires_at,
        "deviceId": input.device_id,
        "taskId": input.task_id,
        "paneId": pane_id,
        "targetKey": stringify(&json!([session_name, pane_id])),
        "controlCommand": control_command,
        "snapshotCommand": snapshot_command,
        "cols": columns,
        "rows": rows,
        "geometryCommand": geometry_command,
    }))
}

/// 준비하는 동안 철회된 grant를 agent에서도 지운다. 실패해도 agent 연결은 그대로 둔다
fn cancel_prepared_grant(agent: Arc<AgentConnection>, grant_id: &str) {
    let payload = request_payload(json!({ "grantId": grant_id }));
    std::thread::spawn(move || {
        let _ = agent.request("revokeGrant", payload);
    });
}

/// 철회 요청은 응답을 기다리지 않는다. 철회를 전하지 못한 agent 연결은 멈춘다
fn revoke_in_background(agents: Vec<Arc<AgentConnection>>, method: &'static str, payload: Value) {
    for agent in agents {
        let payload = request_payload(payload.clone());
        std::thread::spawn(move || {
            if agent.request(method, payload).is_err() {
                agent.stop();
            }
        });
    }
}

fn request_payload(value: Value) -> Map<String, Value> {
    let Value::Object(entries) = value else {
        unreachable!("요청 payload는 객체 리터럴로만 만든다");
    };
    entries
}

/// JS `Date.now()`
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

/// TS `randomBytes(32).toString("hex")`
fn create_grant_token() -> Result<String, String> {
    let mut bytes = [0u8; GRANT_TOKEN_BYTES];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut random_source| random_source.read_exact(&mut bytes))
        .map_err(|error| error.to_string())?;
    Ok(hex::encode(bytes))
}

/// 인증서 DER의 SHA-256을 콜론 없는 소문자 hex로 낸다(TS `X509Certificate.fingerprint256`)
fn certificate_fingerprint(certificate_pem: &str) -> Result<String, String> {
    const INVALID_CERTIFICATE: &str = "Invalid remote terminal certificate";
    let (_, after_begin) = certificate_pem
        .split_once("-----BEGIN CERTIFICATE-----")
        .ok_or(INVALID_CERTIFICATE)?;
    let (body, _) = after_begin
        .split_once("-----END CERTIFICATE-----")
        .ok_or(INVALID_CERTIFICATE)?;
    let encoded: String = body.split_whitespace().collect();
    let der = BASE64
        .decode(encoded)
        .map_err(|_| INVALID_CERTIFICATE.to_string())?;
    Ok(hex::encode(Sha256::digest(der)))
}
