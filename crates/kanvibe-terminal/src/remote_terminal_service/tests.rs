//! TS `remoteTerminalService.test.ts`는 실제 Node agent와 WSS 서버로 돌린다. 여기서는 agent를 stdio 규약만 따르는
//! 가짜로 바꾸고, 데스크탑이 정하는 부분(ssh 인자, stdin 레코드, agent 공유, relay 경로, 철회·중단)을 검사한다.

use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::*;

const FIXTURE_CERTIFICATE_DER: &[u8] = b"fixture certificate der";
const FIXTURE_PRIVATE_KEY: &str =
    "-----BEGIN PRIVATE KEY-----\nfixture\n-----END PRIVATE KEY-----\n";
const FIXTURE_LAUNCH_COMMAND: &str = "fixture-agent-launch";
const FIXTURE_AGENT_PORT: u16 = 43123;

fn fixture_fingerprint() -> String {
    hex::encode(Sha256::digest(FIXTURE_CERTIFICATE_DER))
}

struct PipeWriter(Sender<Vec<u8>>);

impl Write for PipeWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0
            .send(buffer.to_vec())
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct PipeReader {
    receiver: Receiver<Vec<u8>>,
    pending: Vec<u8>,
}

impl Read for PipeReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.pending.is_empty() {
            match self.receiver.recv() {
                Ok(chunk) => self.pending = chunk,
                Err(_) => return Ok(0),
            }
        }
        let length = self.pending.len().min(buffer.len());
        buffer[..length].copy_from_slice(&self.pending[..length]);
        self.pending.drain(..length);
        Ok(length)
    }
}

fn pipe() -> (Sender<Vec<u8>>, PipeReader) {
    let (sender, receiver) = mpsc::channel();
    let reader = PipeReader {
        receiver,
        pending: Vec::new(),
    };
    (sender, reader)
}

/// 가짜 agent가 어떻게 답할지
#[derive(Clone, Default)]
struct FakeAgentScript {
    is_unsupported: bool,
    /// bootstrap 요청도 내지 않고 stdin이 닫힐 때까지 기다린다
    is_silent: bool,
    /// 이 기기의 attach는 그 기기가 철회될 때까지 답하지 않는다(TS 지연된 geometry 조회)
    held_attach_device: Option<String>,
}

type AgentOutput = Arc<Mutex<Option<Sender<Vec<u8>>>>>;

#[derive(Default)]
struct FakeAgentSpawner {
    script: FakeAgentScript,
    spawned_arguments: Mutex<Vec<Vec<String>>>,
    received_records: Arc<Mutex<Vec<Value>>>,
    agent_outputs: Mutex<Vec<AgentOutput>>,
    termination_count: Arc<AtomicUsize>,
}

struct FakeTermination {
    agent_output: AgentOutput,
    termination_count: Arc<AtomicUsize>,
}

impl RemoteTerminalAgentTermination for FakeTermination {
    fn terminate(&self) {
        self.termination_count.fetch_add(1, Ordering::SeqCst);
        self.agent_output.lock().unwrap().take();
    }
}

impl RemoteTerminalAgentSpawner for FakeAgentSpawner {
    fn spawn(
        &self,
        _ssh_host: &str,
        ssh_arguments: &[String],
    ) -> Result<RemoteTerminalAgentProcess, String> {
        self.spawned_arguments
            .lock()
            .unwrap()
            .push(ssh_arguments.to_vec());
        let (desktop_stdin, agent_stdin) = pipe();
        let (agent_stdout, desktop_stdout) = pipe();
        let agent_output: AgentOutput = Arc::new(Mutex::new(Some(agent_stdout.clone())));
        self.agent_outputs
            .lock()
            .unwrap()
            .push(agent_output.clone());
        let script = self.script.clone();
        let received_records = self.received_records.clone();
        std::thread::spawn(move || {
            run_fake_agent(&script, agent_stdin, &agent_stdout, &received_records)
        });
        Ok(RemoteTerminalAgentProcess {
            stdin: Box::new(PipeWriter(desktop_stdin)),
            stdout: Box::new(desktop_stdout),
            termination: Box::new(FakeTermination {
                agent_output,
                termination_count: self.termination_count.clone(),
            }),
        })
    }
}

impl FakeAgentSpawner {
    fn spawned_arguments(&self) -> Vec<Vec<String>> {
        self.spawned_arguments.lock().unwrap().clone()
    }

    fn received_records(&self) -> Vec<Value> {
        self.received_records.lock().unwrap().clone()
    }

    fn received_requests(&self, method: &str) -> Vec<Value> {
        self.received_records()
            .into_iter()
            .filter(|record| record["type"] == "request" && record["method"] == method)
            .collect()
    }

    /// 가장 최근 agent가 stdout으로 레코드를 낸 것처럼 보낸다
    fn emit_from_agent(&self, record: Value) {
        let outputs = self.agent_outputs.lock().unwrap();
        let output = outputs.last().unwrap().lock().unwrap();
        let sender = output.as_ref().unwrap();
        sender.send(format!("{record}\n").into_bytes()).unwrap();
    }
}

#[derive(Default)]
struct FakeAgentState {
    relay_grants: HashMap<String, String>,
    relay_devices: HashMap<String, String>,
    revoked_devices: HashSet<String>,
    held_attach: Option<(String, String)>,
}

fn run_fake_agent(
    script: &FakeAgentScript,
    stdin: PipeReader,
    output: &Sender<Vec<u8>>,
    received_records: &Mutex<Vec<Value>>,
) {
    let emit = |record: Value| {
        let _ = output.send(format!("{record}\n").into_bytes());
    };
    if script.is_unsupported {
        emit(json!({ "type": "unsupported" }));
    } else if !script.is_silent {
        emit(json!({ "type": "bootstrap-ready" }));
    }
    let mut state = FakeAgentState::default();
    for line in BufReader::new(stdin).lines() {
        let Ok(line) = line else { return };
        let record: Value = serde_json::from_str(&line).unwrap();
        received_records.lock().unwrap().push(record.clone());
        handle_fake_agent_record(script, &mut state, &record, &emit);
    }
}

fn handle_fake_agent_record(
    script: &FakeAgentScript,
    state: &mut FakeAgentState,
    record: &Value,
    emit: &dyn Fn(Value),
) {
    if record.get("source").is_some() {
        emit(json!({
            "type": "ready",
            "port": FIXTURE_AGENT_PORT,
            "certificateFingerprint": fixture_fingerprint(),
        }));
        return;
    }
    match record["type"].as_str() {
        Some("request") => handle_fake_agent_request(script, state, record, emit),
        Some("data") => {
            emit(json!({ "type": "output", "relayId": record["relayId"], "data": record["data"] }))
        }
        _ => {}
    }
}

fn handle_fake_agent_request(
    script: &FakeAgentScript,
    state: &mut FakeAgentState,
    record: &Value,
    emit: &dyn Fn(Value),
) {
    let request_id = record["id"].as_str().unwrap().to_string();
    let respond =
        |id: &str, is_ok: bool| emit(json!({ "type": "response", "id": id, "ok": is_ok }));
    match record["method"].as_str().unwrap() {
        "attach" => {
            let device_id = record["deviceId"].as_str().unwrap().to_string();
            if script.held_attach_device.as_deref() == Some(device_id.as_str()) {
                state.held_attach = Some((request_id, device_id));
                return;
            }
            let relay_id = record["relayId"].as_str().unwrap().to_string();
            let grant_id = record["grantId"].as_str().unwrap().to_string();
            state.relay_grants.insert(relay_id.clone(), grant_id);
            state.relay_devices.insert(relay_id, device_id.clone());
            respond(&request_id, !state.revoked_devices.contains(&device_id));
        }
        "revokeGrant" => {
            let grant_id = record["grantId"].as_str().unwrap();
            close_fake_relays(
                state,
                |relay_id, state| state.relay_grants[relay_id] == grant_id,
                "grant-revoked",
                emit,
            );
            respond(&request_id, true);
        }
        "revokeDevice" => {
            let device_id = record["deviceId"].as_str().unwrap().to_string();
            if let Some((held_id, _)) = state.held_attach.take_if(|(_, held)| *held == device_id) {
                respond(&held_id, false);
            }
            close_fake_relays(
                state,
                |relay_id, state| state.relay_devices[relay_id] == device_id,
                "device-revoked",
                emit,
            );
            state.revoked_devices.insert(device_id);
            respond(&request_id, true);
        }
        _ => respond(&request_id, true),
    }
}

fn close_fake_relays(
    state: &mut FakeAgentState,
    is_target: impl Fn(&str, &FakeAgentState) -> bool,
    reason: &str,
    emit: &dyn Fn(Value),
) {
    let targets: Vec<String> = state
        .relay_grants
        .keys()
        .filter(|relay_id| is_target(relay_id, state))
        .cloned()
        .collect();
    for relay_id in targets {
        state.relay_grants.remove(&relay_id);
        state.relay_devices.remove(&relay_id);
        emit(json!({ "type": "closed", "relayId": relay_id, "code": 4000, "reason": reason }));
    }
}

struct FixtureAgentBundle;

impl RemoteTerminalAgentBundle for FixtureAgentBundle {
    fn launch_command(&self) -> String {
        FIXTURE_LAUNCH_COMMAND.to_string()
    }

    fn read_source(&self) -> Result<String, String> {
        Ok("module.exports = {}".to_string())
    }
}

struct FixtureCertificateIssuer;

impl RemoteTerminalCertificateIssuer for FixtureCertificateIssuer {
    fn issue(&self, _not_after_ms: i64) -> Result<RemoteTerminalCertificate, String> {
        Ok(RemoteTerminalCertificate {
            certificate_pem: format!(
                "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
                BASE64.encode(FIXTURE_CERTIFICATE_DER)
            ),
            private_key_pem: FIXTURE_PRIVATE_KEY.to_string(),
        })
    }
}

struct Fixture {
    service: Arc<RemoteTerminalService>,
    spawner: Arc<FakeAgentSpawner>,
    _home: TempDir,
}

fn fixture(script: FakeAgentScript) -> Fixture {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".ssh")).unwrap();
    std::fs::write(
        home.path().join(".ssh/config"),
        "Host fixture\n  HostName 127.0.0.1\n  Port 22\n  User fixture\n  IdentityFile /fixture-key\n",
    )
    .unwrap();
    let spawner = Arc::new(FakeAgentSpawner {
        script,
        ..FakeAgentSpawner::default()
    });
    let service = RemoteTerminalService::new(RemoteTerminalServiceDependencies {
        ssh_hosts: Arc::new(SshConfigReader::new(home.path().to_str().unwrap())),
        agent_bundle: Arc::new(FixtureAgentBundle),
        certificate_issuer: Arc::new(FixtureCertificateIssuer),
        agent_spawner: spawner.clone(),
    });
    Fixture {
        service: Arc::new(service),
        spawner,
        _home: home,
    }
}

fn grant_input(device_id: &str) -> RemoteTerminalGrantInput {
    RemoteTerminalGrantInput {
        ssh_host: "fixture".to_string(),
        device_id: device_id.to_string(),
        task_id: "task-a".to_string(),
        session_name: "fixture-session".to_string(),
        pane_id: "w1:p1".to_string(),
        cols: Some(80),
        rows: Some(24),
    }
}

fn wait_until(description: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "기다리다 시간이 지났다: {description}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// relay 출력과 닫힘을 모은다
#[derive(Clone, Default)]
struct RelayRecorder {
    output: Arc<Mutex<String>>,
    closures: Arc<Mutex<Vec<(u16, String)>>>,
}

impl RelayRecorder {
    fn attach(
        &self,
        service: &RemoteTerminalService,
        grant_id: &str,
        device_id: &str,
    ) -> Result<RemoteTerminalRelay, String> {
        let output = self.output.clone();
        let closures = self.closures.clone();
        service.attach_relay(
            grant_id,
            device_id,
            "task-a",
            "w1:p1",
            move |relayed| output.lock().unwrap().push_str(&relayed),
            move |code, reason| closures.lock().unwrap().push((code, reason)),
        )
    }

    fn output(&self) -> String {
        self.output.lock().unwrap().clone()
    }

    fn closures(&self) -> Vec<(u16, String)> {
        self.closures.lock().unwrap().clone()
    }
}

fn attach_in_background(
    service: &Arc<RemoteTerminalService>,
    grant_id: &str,
    device_id: &str,
) -> Receiver<Result<RemoteTerminalRelay, String>> {
    let (sender, receiver) = mpsc::channel();
    let service = service.clone();
    let grant_id = grant_id.to_string();
    let device_id = device_id.to_string();
    std::thread::spawn(move || {
        let result = RelayRecorder::default().attach(&service, &grant_id, &device_id);
        let _ = sender.send(result);
    });
    receiver
}

// parity: TS-main-remoteTerminalService-e06379
#[test]
fn concurrent_grants_share_one_strict_ssh_agent_and_relay_exact_unicode_and_nul_input() {
    let Fixture {
        service,
        spawner,
        _home,
    } = fixture(FakeAgentScript::default());

    let (direct, relayed) = std::thread::scope(|scope| {
        let first = scope.spawn(|| service.prepare_grant(&grant_input("device-a")));
        let second = scope.spawn(|| service.prepare_grant(&grant_input("device-b")));
        (first.join().unwrap(), second.join().unwrap())
    });
    let direct = direct.unwrap().unwrap();
    let relayed = relayed.unwrap().unwrap();

    let spawned = spawner.spawned_arguments();
    assert_eq!(spawned.len(), 1);
    assert!(spawned[0].contains(&"StrictHostKeyChecking=yes".to_string()));
    assert_eq!(
        spawned[0].last().unwrap(),
        &format!("sh -lc {}", quote_for_posix_shell(FIXTURE_LAUNCH_COMMAND))
    );
    assert!(!spawned[0].join(" ").contains(&direct.direct.token));
    assert!(!spawned[0].join(" ").contains("PRIVATE KEY"));
    let bootstrap = &spawner.received_records()[0];
    assert_eq!(bootstrap["certificate"]["key"], FIXTURE_PRIVATE_KEY);
    assert_eq!(direct.direct.host, "127.0.0.1");
    assert_eq!(direct.direct.port, FIXTURE_AGENT_PORT);
    assert_eq!(direct.direct.path, format!("/stream/{}", direct.grant_id));
    assert_eq!(direct.direct.certificate_fingerprint, fixture_fingerprint());

    let recorder = RelayRecorder::default();
    let relay = recorder
        .attach(&service, &relayed.grant_id, "device-b")
        .unwrap();
    let text = "한글\0🌊";
    for _ in 0..100 {
        relay.write(text.as_bytes(), false);
    }
    wait_until("100번의 입력이 모두 돌아온다", || {
        recorder.output().matches(text).count() == 100
    });

    service.revoke_device("device-a");
    wait_until("기기 철회가 agent에 닿는다", || {
        !spawner.received_requests("revokeDevice").is_empty()
    });
    relay.write(b"still-live", false);
    wait_until("다른 기기의 relay는 살아 있다", || {
        recorder.output().contains("still-live")
    });
    assert!(recorder.closures().is_empty());
    relay.close();
    assert_eq!(spawner.spawned_arguments().len(), 1);
}

// parity: TS-main-remoteTerminalService-824542
#[test]
fn relay_with_a_different_device_scope_is_rejected_before_reaching_the_agent() {
    let Fixture {
        service,
        spawner,
        _home,
    } = fixture(FakeAgentScript::default());
    let grant = service
        .prepare_grant(&grant_input("device-a"))
        .unwrap()
        .unwrap();

    let result = RelayRecorder::default().attach(&service, &grant.grant_id, "other-device");

    assert_eq!(result.err().unwrap(), "Invalid remote terminal scope");
    assert!(spawner.received_requests("attach").is_empty());
}

// parity: TS-main-remoteTerminalService-b4daa1
#[test]
fn unsupported_runtime_result_is_shared_by_the_next_probe() {
    let Fixture {
        service,
        spawner,
        _home,
    } = fixture(FakeAgentScript {
        is_unsupported: true,
        ..FakeAgentScript::default()
    });

    assert_eq!(service.prepare_grant(&grant_input("device-a")), Ok(None));
    assert_eq!(service.prepare_grant(&grant_input("device-a")), Ok(None));

    assert_eq!(spawner.spawned_arguments().len(), 1);
}

// parity: TS-main-remoteTerminalService-7a6477
#[test]
fn stopping_the_owner_fails_a_pending_ssh_bootstrap_immediately() {
    let Fixture {
        service,
        spawner,
        _home,
    } = fixture(FakeAgentScript {
        is_silent: true,
        ..FakeAgentScript::default()
    });
    let (sender, receiver) = mpsc::channel();
    let preparing_service = service.clone();
    std::thread::spawn(move || {
        let _ = sender.send(preparing_service.prepare_grant(&grant_input("device-a")));
    });
    wait_until("ssh agent가 떴다", || {
        spawner.spawned_arguments().len() == 1
    });

    service.stop_agents();

    let outcome = receiver.recv_timeout(Duration::from_millis(300)).unwrap();
    assert!(outcome.is_err());
    assert!(spawner.termination_count.load(Ordering::SeqCst) >= 1);
}

// parity: TS-main-remoteTerminalService-ab8552
#[test]
fn revoking_one_grant_keeps_another_stream_of_the_same_task() {
    let Fixture { service, _home, .. } = fixture(FakeAgentScript::default());
    let first = service
        .prepare_grant(&grant_input("device-a"))
        .unwrap()
        .unwrap();
    let second = service
        .prepare_grant(&grant_input("device-b"))
        .unwrap()
        .unwrap();
    let first_recorder = RelayRecorder::default();
    let second_recorder = RelayRecorder::default();
    let first_relay = first_recorder
        .attach(&service, &first.grant_id, "device-a")
        .unwrap();
    let second_relay = second_recorder
        .attach(&service, &second.grant_id, "device-b")
        .unwrap();

    service.revoke_grant(&first.grant_id);

    wait_until("철회한 grant의 relay가 닫힌다", || {
        first_recorder.closures() == vec![(4000, "grant-revoked".to_string())]
    });
    second_relay.write(b"surviving-pane", false);
    wait_until("남은 relay는 계속 쓴다", || {
        second_recorder.output().contains("surviving-pane")
    });
    assert!(second_recorder.closures().is_empty());
    first_relay.close();
    second_relay.close();
}

// parity: TS-main-remoteTerminalService-101229
#[test]
fn fresh_grant_attaches_after_the_previous_controller_stream_failed() {
    let Fixture {
        service,
        spawner,
        _home,
    } = fixture(FakeAgentScript::default());
    let first = service
        .prepare_grant(&grant_input("device-a"))
        .unwrap()
        .unwrap();
    let first_recorder = RelayRecorder::default();
    let _first_relay = first_recorder
        .attach(&service, &first.grant_id, "device-a")
        .unwrap();
    let relay_id = spawner.received_requests("attach")[0]["relayId"].clone();

    spawner.emit_from_agent(
        json!({ "type": "closed", "relayId": relay_id, "code": 4000, "reason": "stream-failed" }),
    );
    wait_until("controller가 끝나 relay가 닫힌다", || {
        first_recorder.closures() == vec![(4000, "stream-failed".to_string())]
    });

    let fresh = service
        .prepare_grant(&grant_input("device-a"))
        .unwrap()
        .unwrap();
    let fresh_recorder = RelayRecorder::default();
    let fresh_relay = fresh_recorder
        .attach(&service, &fresh.grant_id, "device-a")
        .unwrap();
    fresh_relay.write(b"fresh-controller", false);
    wait_until("새 relay가 입력을 넘긴다", || {
        fresh_recorder.output().contains("fresh-controller")
    });
    fresh_relay.close();
}

// parity: TS-main-remoteTerminalService-435674
#[test]
fn wide_native_geometry_is_clamped_to_the_mobile_bound_in_the_wire_grant() {
    let Fixture {
        service,
        spawner,
        _home,
    } = fixture(FakeAgentScript::default());
    let input = RemoteTerminalGrantInput {
        cols: Some(2000),
        rows: Some(1000),
        ..grant_input("device-a")
    };

    let grant = service.prepare_grant(&input).unwrap().unwrap();

    let wire_grant = spawner.received_requests("grant")[0]["grant"].clone();
    let keys: Vec<&str> = wire_grant
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "grantId",
            "token",
            "expiresAt",
            "deviceId",
            "taskId",
            "paneId",
            "targetKey",
            "controlCommand",
            "snapshotCommand",
            "cols",
            "rows",
            "geometryCommand",
        ]
    );
    assert_eq!(wire_grant["grantId"], grant.grant_id);
    assert_eq!(wire_grant["token"], grant.direct.token);
    assert_eq!(
        (wire_grant["cols"].clone(), wire_grant["rows"].clone()),
        (json!(500), json!(500))
    );
    assert_eq!(wire_grant["targetKey"], r#"["fixture-session","w1:p1"]"#);
    assert_eq!(
        wire_grant["controlCommand"],
        build_herdr_control_pane_command("fixture-session", "w1:p1", 500, 500).unwrap()
    );
    let geometry_command = format!(
        "{} && printf '\\036' && {}",
        build_herdr_command("fixture-session", &["pane", "get", "w1:p1"]).unwrap(),
        build_herdr_command("fixture-session", &["pane", "layout", "--pane", "w1:p1"]).unwrap()
    );
    assert_eq!(wire_grant["geometryCommand"], geometry_command);
}

// parity: TS-main-remoteTerminalService-fbd9a5
#[test]
fn revoking_the_only_requester_fails_its_pending_attach() {
    let Fixture {
        service,
        spawner,
        _home,
    } = fixture(FakeAgentScript {
        held_attach_device: Some("device-a".to_string()),
        ..FakeAgentScript::default()
    });
    let grant = service
        .prepare_grant(&grant_input("device-a"))
        .unwrap()
        .unwrap();
    let attaching = attach_in_background(&service, &grant.grant_id, "device-a");
    wait_until("attach 요청이 agent에 닿았다", || {
        spawner.received_requests("attach").len() == 1
    });

    service.revoke_device("device-a");

    let outcome = attaching.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(outcome.err().unwrap(), "Remote terminal request rejected");
}

// parity: TS-main-remoteTerminalService-379f64
#[test]
fn another_devices_attach_survives_when_the_first_requester_is_revoked() {
    let Fixture {
        service,
        spawner,
        _home,
    } = fixture(FakeAgentScript {
        held_attach_device: Some("device-a".to_string()),
        ..FakeAgentScript::default()
    });
    let first = service
        .prepare_grant(&grant_input("device-a"))
        .unwrap()
        .unwrap();
    let second = service
        .prepare_grant(&grant_input("device-b"))
        .unwrap()
        .unwrap();
    let attaching_first = attach_in_background(&service, &first.grant_id, "device-a");
    wait_until("첫 attach 요청이 agent에 닿았다", || {
        spawner.received_requests("attach").len() == 1
    });

    let second_recorder = RelayRecorder::default();
    let second_relay = second_recorder
        .attach(&service, &second.grant_id, "device-b")
        .unwrap();
    service.revoke_device("device-a");

    assert!(
        attaching_first
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .is_err()
    );
    second_relay.write(b"second-device", false);
    wait_until("두 번째 기기의 relay는 쓴다", || {
        second_recorder.output().contains("second-device")
    });
    assert_eq!(spawner.spawned_arguments().len(), 1);
    second_relay.close();
}

// parity: TS-main-remoteTerminalService-fc0714
#[test]
fn stopping_the_owner_fails_a_pending_attach_and_terminates_the_ssh_agent() {
    let Fixture {
        service,
        spawner,
        _home,
    } = fixture(FakeAgentScript {
        held_attach_device: Some("device-a".to_string()),
        ..FakeAgentScript::default()
    });
    let grant = service
        .prepare_grant(&grant_input("device-a"))
        .unwrap()
        .unwrap();
    let attaching = attach_in_background(&service, &grant.grant_id, "device-a");
    wait_until("attach 요청이 agent에 닿았다", || {
        spawner.received_requests("attach").len() == 1
    });

    service.stop_agents();

    let outcome = attaching.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(outcome.err().unwrap(), "Remote terminal disconnected");
    assert_eq!(spawner.termination_count.load(Ordering::SeqCst), 1);
}

#[test]
fn node_agent_bundle_keeps_the_ts_launch_command_and_missing_bundle_error() {
    let directory = tempfile::tempdir().unwrap();
    let bundle = NodeRemoteTerminalAgentBundle::new(directory.path().join("main"));

    let launch_command = bundle.launch_command();

    assert!(launch_command.starts_with(
        r#"kanvibe_node=$(command -v node) || { printf '{"type":"unsupported"}\n'; exit 0; }; "$kanvibe_node" -e 'process.exit(Number(process.versions.node.split(".")[0])>=18?0:1)' || { printf '{"type":"unsupported"}\n'; exit 0; }; exec "$kanvibe_node" -e 'process.stdout.write('"'"'{"type":"bootstrap-ready"}\n'"'"');"#
    ));
    assert_eq!(
        bundle.read_source(),
        Err("Remote terminal agent bundle is missing; run the desktop build.".to_string())
    );
    std::fs::create_dir_all(directory.path().join("build/remote")).unwrap();
    std::fs::write(
        directory.path().join("build/remote/terminal-agent.cjs"),
        "agent-source",
    )
    .unwrap();
    assert_eq!(bundle.read_source(), Ok("agent-source".to_string()));
}
