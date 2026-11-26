//! 모바일 원격 터미널 agent(TS `remoteTerminalAgent.ts`).
//!
//! 데스크탑이 띄운 SSH 자식 안에서만 돌고, 모든 권한(인증서 키, grant)은 그 자식의 비공개 stdin으로만 받는다.
//! 모바일은 grant의 토큰으로 이 agent의 WSS(`/stream/<grantId>`)에 직접 붙거나, 데스크탑 relay(stdin의
//! `attach`/`data` 레코드)로 같은 grant를 쓴다. 같은 pane(target key)에 붙은 연결은 Herdr controller 하나를 함께 쓴다.
//!
//! 스레드 모델: agent 상태는 `Mutex` 하나에 있고, 상태가 바뀌면 `Condvar`로 기다리는 쪽을 깨운다. 잠금 안에서는
//! 막히는 일(자식 쓰기, 소켓 쓰기)을 하지 않고 큐에 넣기만 한다. grant 입장 시간은 데스크탑 시계가 아니라
//! 이 agent의 단조 시계로 잰다.

mod attach;
mod direct;
mod group;
mod owner;
mod query;
mod state;
mod tls;
pub mod wire;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::outbox::RecordOutbox;
use state::AgentState;

/// 떠 있는 agent. 테스트와 실행 파일이 포트를 읽고 멈춘다
pub struct TerminalAgentHandle {
    agent: Arc<Agent>,
}

impl TerminalAgentHandle {
    pub fn port(&self) -> u16 {
        self.agent.port
    }

    pub fn stop(&self) {
        self.agent.stop();
    }

    /// agent가 멈출 때까지 기다린다
    pub fn wait_until_stopped(&self) {
        let state = self.agent.lock();
        let _state = self
            .agent
            .changed
            .wait_while(state, |state| !state.is_stopped)
            .unwrap();
    }
}

pub(crate) struct Agent {
    state: Mutex<AgentState>,
    changed: Condvar,
    output: Arc<RecordOutbox>,
    port: u16,
    is_listening: AtomicBool,
    myself: Weak<Agent>,
}

impl Agent {
    /// 잠금 밖에서 늦게 도는 작업(controller 종료 확인)이 agent를 붙잡게 한다
    fn self_reference(&self) -> Option<Arc<Agent>> {
        self.myself.upgrade()
    }

    fn lock(&self) -> MutexGuard<'_, AgentState> {
        self.state.lock().unwrap()
    }

    /// 소유자 stdout으로 NDJSON 한 줄을 보낸다. 큐가 넘치면 agent를 멈춘다
    fn emit(&self, state: &mut AgentState, record: Value) {
        if state.is_stopped {
            return;
        }
        let mut line = record.to_string().into_bytes();
        line.push(b'\n');
        if self.output.send_bytes(line).is_err() {
            state.should_stop = true;
            self.changed.notify_all();
        }
    }

    /// 모든 grant와 연결을 닫고 controller를 놓아 준다(TS `stop`)
    pub(crate) fn stop(&self) {
        let mut state = self.lock();
        if state.is_stopped {
            return;
        }
        self.stop_locked(&mut state);
        drop(state);
        self.is_listening.store(false, Ordering::SeqCst);
        self.output.close_after_flush();
    }

    fn stop_locked(&self, state: &mut AgentState) {
        query::kill_all(state);
        let group_ids: Vec<u64> = state.groups.values().copied().collect();
        for group_id in group_ids {
            self.close_group(state, group_id, 1001, "owner-disconnected");
        }
        for client in state.clients.values() {
            if let state::ClientChannel::Direct(channel) = &client.channel {
                channel.terminate();
            }
        }
        state.grants.clear();
        state.relays.clear();
        state.is_stopped = true;
        self.changed.notify_all();
    }
}

/// agent를 띄운다. `input`은 소유자(데스크탑)가 보내는 NDJSON, `output`은 agent가 쓰는 NDJSON이다
pub fn start_terminal_agent(
    certificate_pem: &str,
    private_key_pem: &str,
    input: Box<dyn Read + Send>,
    output: Box<dyn Write + Send>,
) -> Result<TerminalAgentHandle, String> {
    let (server_config, fingerprint) = tls::server_config(certificate_pem, private_key_pem)?;
    let listener = TcpListener::bind("0.0.0.0:0").map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let output = RecordOutbox::start(output, Box::new(|| ()));
    let agent = Arc::new_cyclic(|myself| Agent {
        state: Mutex::new(AgentState::new()),
        changed: Condvar::new(),
        output,
        port,
        is_listening: AtomicBool::new(true),
        myself: myself.clone(),
    });
    direct::spawn_listener(&agent, listener, server_config);
    owner::spawn_reader(&agent, input);
    spawn_lease_watch(&agent);
    let ready = json!({ "type": "ready", "port": port, "certificateFingerprint": fingerprint });
    agent.emit(&mut agent.lock(), ready);
    Ok(TerminalAgentHandle { agent })
}

/// 소유자 heartbeat가 30초 끊기면 멈추고, 아무도 쓰지 않은 채 입장 시간이 지난 grant를 지운다
fn spawn_lease_watch(agent: &Arc<Agent>) {
    let agent = Arc::clone(agent);
    let interval = Duration::from_millis(wire::REMOTE_TERMINAL_OWNER_LEASE_MS / 3);
    let lease = Duration::from_millis(wire::REMOTE_TERMINAL_OWNER_LEASE_MS);
    std::thread::spawn(move || {
        loop {
            let state = agent.lock();
            let (mut state, _) = agent
                .changed
                .wait_timeout_while(state, interval, |state| {
                    !state.should_stop && !state.is_stopped
                })
                .unwrap();
            if state.is_stopped {
                return;
            }
            let now = Instant::now();
            if state.should_stop || now.duration_since(state.last_owner_heartbeat) >= lease {
                drop(state);
                agent.stop();
                return;
            }
            state
                .grants
                .retain(|_, grant| grant.active.is_some() || grant.admission_until > now);
        }
    });
}

/// `terminal-agent` 모드 진입점. 첫 stdin 줄로 소스 확인값과 인증서를 받고 멈출 때까지 돈다
pub fn run_with_stdio(build_hash: &str) -> Result<(), String> {
    let mut stdout = std::io::stdout();
    stdout
        .write_all(b"{\"type\":\"bootstrap-ready\"}\n")
        .and_then(|()| stdout.flush())
        .map_err(|error| error.to_string())?;
    let mut input = BufReader::new(std::io::stdin());
    let mut line = String::new();
    input
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    let bootstrap: Value =
        serde_json::from_str(&line).map_err(|_| "Invalid terminal bootstrap".to_string())?;
    // 데스크탑은 원격이 어느 플랫폼 바이너리를 돌리는지 몰라 아는 hash를 모두 쉼표로 이어 보낸다
    let source = bootstrap
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !source.split(',').any(|entry| entry == build_hash) {
        return Err("Terminal agent build mismatch".into());
    }
    let certificate = |field: &str| bootstrap["certificate"][field].as_str().map(str::to_string);
    let (Some(certificate_pem), Some(private_key_pem)) = (certificate("cert"), certificate("key"))
    else {
        return Err("Invalid terminal bootstrap".into());
    };
    let handle = start_terminal_agent(
        &certificate_pem,
        &private_key_pem,
        Box::new(input),
        Box::new(stdout),
    )?;
    handle.wait_until_stopped();
    Ok(())
}
