//! 원격 loader를 돌려 daemon bridge에 붙는다(TS `connectHostProcess`).
//!
//! loader가 `upload-required`로 답하면 그 플랫폼 바이너리를 stdin으로 끝까지 보내 게시하게 하고, 같은 loader를 한 번
//! 더 돌린다. `bridge-ready`가 오면 그 줄의 build hash가 데스크탑이 가진 바이너리 것인지 확인한 뒤 `hello`를 보낸다.
//! `unsupported`면 daemon 없이 순수 ssh 경로로 가도록 `None`을 돌려준다.
//! 실패는 단계로 나눈다. loader의 첫 줄 전에 끊기면 SSH, 업로드와 bootstrap 순서는 준비, `hello` 뒤는 핸드셰이크다.

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kanvibe_process::ssh_config::SshHostConfig;
use kanvibe_remote_agent::bootstrap::AgentLaunchMode;
use serde_json::Value;

use super::{
    HostConnection, HostIdentity, HostTransport, RemoteHostConnectionError,
    RemoteHostConnectionErrorKind, START_TIMEOUT,
};
use crate::agent_binaries::AgentBinaryCatalog;

const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_PREAMBLE_BYTES: u64 = 4096;

/// 원격 셸 명령 하나를 실행하는 통로(시스템 ssh). 테스트는 소켓이나 로컬 `sh`로 바꾼다
pub trait HostTransportSpawner: Send + Sync {
    fn spawn(&self, config: &SshHostConfig, remote_command: &str) -> Result<HostTransport, String>;
}

/// 시간 안에 부트스트랩이 끝나지 않으면 통로를 끊는다
struct Watchdog {
    cancel: std::sync::mpsc::Sender<()>,
}

impl Watchdog {
    fn start(timeout: Duration, terminate: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (cancel, cancelled) = channel::<()>();
        std::thread::spawn(move || {
            if matches!(
                cancelled.recv_timeout(timeout),
                Err(RecvTimeoutError::Timeout)
            ) {
                terminate();
            }
        });
        Self { cancel }
    }

    fn stop(self) {
        let _ = self.cancel.send(());
    }
}

/// loader를 거쳐 daemon에 붙는다. 원격이 지원하지 않으면 `None`이다
pub fn connect_through_loader(
    spawner: &dyn HostTransportSpawner,
    config: &SshHostConfig,
    binaries: &AgentBinaryCatalog,
    namespace: &str,
    client_id: &str,
) -> Result<Option<Arc<HostConnection>>, RemoteHostConnectionError> {
    let loader = binaries
        .loader_script(namespace, AgentLaunchMode::HostBridge)
        .map_err(RemoteHostConnectionError::setup)?;
    let remote_command = format!(
        "sh -lc {}",
        kanvibe_process::worktree::quote_for_posix_shell(&loader)
    );
    for _ in 0..2 {
        let transport = spawner.spawn(config, &remote_command).map_err(|error| {
            RemoteHostConnectionError::new(RemoteHostConnectionErrorKind::Ssh, error)
        })?;
        match run_loader(transport, binaries, namespace, client_id)? {
            LoaderOutcome::Unsupported => return Ok(None),
            LoaderOutcome::Uploaded => continue,
            LoaderOutcome::Connected(connection) => return Ok(Some(connection)),
        }
    }
    Err(RemoteHostConnectionError::setup(
        "Host bootstrap did not start after upload",
    ))
}

enum LoaderOutcome {
    Unsupported,
    Uploaded,
    Connected(Arc<HostConnection>),
}

fn run_loader(
    transport: HostTransport,
    binaries: &AgentBinaryCatalog,
    namespace: &str,
    client_id: &str,
) -> Result<LoaderOutcome, RemoteHostConnectionError> {
    let HostTransport {
        mut writer,
        reader,
        terminate,
        stderr_tail,
    } = transport;
    let terminate: Arc<dyn Fn() + Send + Sync> = Arc::from(terminate);
    let watchdog = Watchdog::start(START_TIMEOUT, Arc::clone(&terminate));
    let mut reader = BufReader::new(reader);
    let first = read_preamble_line(
        &mut reader,
        &stderr_tail,
        RemoteHostConnectionErrorKind::Ssh,
    );
    watchdog.stop();
    let first = first.inspect_err(|_| terminate())?;
    match first["type"].as_str().unwrap_or_default() {
        "unsupported" => {
            terminate();
            Ok(LoaderOutcome::Unsupported)
        }
        "upload-required" => {
            let platform = first["platform"].as_str().unwrap_or_default().to_string();
            let watchdog = Watchdog::start(UPLOAD_TIMEOUT, Arc::clone(&terminate));
            let outcome = upload(&mut writer, &mut reader, binaries, &platform, &stderr_tail)
                .map_err(RemoteHostConnectionError::setup);
            watchdog.stop();
            terminate();
            outcome.map(|()| LoaderOutcome::Uploaded)
        }
        "bridge-ready" => {
            let build_hash = first["buildHash"].as_str().unwrap_or_default().to_string();
            let transport = HostTransport {
                writer,
                reader: Box::new(reader),
                terminate: Box::new(move || terminate()),
                stderr_tail,
            };
            attach_bridge(transport, binaries, namespace, client_id, &build_hash)
        }
        _ => {
            terminate();
            Err(RemoteHostConnectionError::setup(
                "Invalid host bootstrap sequence",
            ))
        }
    }
}

/// bridge가 알린 build hash가 데스크탑의 바이너리 것일 때만 `hello`를 보낸다
fn attach_bridge(
    transport: HostTransport,
    binaries: &AgentBinaryCatalog,
    namespace: &str,
    client_id: &str,
    build_hash: &str,
) -> Result<LoaderOutcome, RemoteHostConnectionError> {
    if !binaries.contains_hash(build_hash) {
        (transport.terminate)();
        return Err(RemoteHostConnectionError::new(
            RemoteHostConnectionErrorKind::Handshake,
            "Host handshake mismatch",
        ));
    }
    let identity = HostIdentity {
        namespace: namespace.into(),
        client_id: client_id.into(),
        build_hash: build_hash.into(),
    };
    HostConnection::attach(transport, &identity).map(LoaderOutcome::Connected)
}

fn upload(
    writer: &mut Box<dyn Write + Send>,
    reader: &mut BufReader<Box<dyn Read + Send>>,
    binaries: &AgentBinaryCatalog,
    platform: &str,
    stderr_tail: &Arc<Mutex<String>>,
) -> Result<(), String> {
    let binary = binaries.read_binary(platform)?;
    writer
        .write_all(&binary)
        .and_then(|()| writer.flush())
        .map_err(|error| error.to_string())?;
    // stdin을 닫아야 원격 loader가 끝을 안다
    drop(std::mem::replace(writer, Box::new(std::io::sink())));
    let uploaded = read_preamble_line(reader, stderr_tail, RemoteHostConnectionErrorKind::Setup)
        .map_err(|error| error.message)?;
    if uploaded["type"] != "uploaded" {
        return Err("Invalid host bootstrap sequence".into());
    }
    Ok(())
}

/// JSON 한 줄을 읽는다. 끊기면 SSH stderr 끝부분을 붙여 `disconnect_kind` 단계의 실패로 알린다
fn read_preamble_line(
    reader: &mut BufReader<Box<dyn Read + Send>>,
    stderr_tail: &Arc<Mutex<String>>,
    disconnect_kind: RemoteHostConnectionErrorKind,
) -> Result<Value, RemoteHostConnectionError> {
    let mut line = String::new();
    let count = reader
        .by_ref()
        .take(MAX_PREAMBLE_BYTES)
        .read_line(&mut line)
        .unwrap_or(0);
    if count == 0 || !line.ends_with('\n') {
        let tail = stderr_tail.lock().unwrap().trim().to_string();
        let suffix = if tail.is_empty() {
            String::new()
        } else {
            format!(": {tail}")
        };
        return Err(RemoteHostConnectionError::new(
            disconnect_kind,
            format!("SSH host disconnected during startup{suffix}"),
        ));
    }
    serde_json::from_str(line.trim())
        .map_err(|_| RemoteHostConnectionError::setup("Invalid host bootstrap preamble"))
}
