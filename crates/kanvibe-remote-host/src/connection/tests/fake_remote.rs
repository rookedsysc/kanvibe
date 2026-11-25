//! 원격 bridge 흉내. 소켓 한쪽 끝을 데스크탑 통로로 주고, 다른 끝에서 `bridge-ready`와 `ready`로 답한다.

use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kanvibe_process::ssh_config::SshHostConfig;
use kanvibe_remote_agent::protocol::{HostRecordDecoder, encode_host_record, record_from};
use serde_json::{Value, json};

use crate::agent_binaries::AgentBinaryCatalog;
use crate::connection::{HostTransport, HostTransportSpawner};

pub fn fixture_config() -> SshHostConfig {
    SshHostConfig {
        host: "fixture".into(),
        hostname: "127.0.0.1".into(),
        port: 22,
        username: "fixture".into(),
        private_key_path: "/fixture-key".into(),
    }
}

pub struct FakeRemote {
    stream: UnixStream,
    records: Arc<Mutex<Vec<Value>>>,
    _binary_directory: tempfile::TempDir,
}

/// 준비한 통로를 한 번만 내주는 실행기
pub struct OneShotSpawner(Mutex<Option<HostTransport>>);

impl HostTransportSpawner for OneShotSpawner {
    fn spawn(
        &self,
        _config: &SshHostConfig,
        _remote_command: &str,
    ) -> Result<HostTransport, String> {
        self.0
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| "spawned twice".to_string())
    }
}

impl FakeRemote {
    pub fn start() -> (Self, OneShotSpawner, AgentBinaryCatalog) {
        let binary_directory = tempfile::tempdir().unwrap();
        let binary_path: PathBuf = binary_directory
            .path()
            .join("kanvibe-remote-agent-Linux-x86_64");
        std::fs::write(&binary_path, b"fixture binary").unwrap();
        let catalog =
            AgentBinaryCatalog::from_files(vec![("Linux-x86_64".into(), binary_path)]).unwrap();
        let build_hash = catalog.digests()[0].sha256.clone();
        let (desktop, remote) = UnixStream::pair().unwrap();
        let closer = desktop.try_clone().unwrap();
        let transport = HostTransport {
            writer: Box::new(desktop.try_clone().unwrap()),
            reader: Box::new(desktop),
            terminate: Box::new(move || drop(closer.shutdown(Shutdown::Both))),
            stderr_tail: Arc::new(Mutex::new(String::new())),
        };
        let records = Arc::new(Mutex::new(Vec::new()));
        let mut writer = remote.try_clone().unwrap();
        writer
            .write_all(
                format!("{{\"type\":\"bridge-ready\",\"buildHash\":\"{build_hash}\"}}\n")
                    .as_bytes(),
            )
            .unwrap();
        spawn_reader(
            remote.try_clone().unwrap(),
            Arc::clone(&records),
            build_hash,
        );
        let fake = Self {
            stream: remote,
            records,
            _binary_directory: binary_directory,
        };
        (fake, OneShotSpawner(Mutex::new(Some(transport))), catalog)
    }

    pub fn send(&self, record: Value) {
        let mut stream = self.stream.try_clone().unwrap();
        stream
            .write_all(&encode_host_record(&record_from(record)).unwrap())
            .unwrap();
    }

    pub fn disconnect(&self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }

    pub fn records_of(&self, kind: &str) -> Vec<Value> {
        self.records
            .lock()
            .unwrap()
            .iter()
            .filter(|record| record["type"] == kind)
            .cloned()
            .collect()
    }

    /// `kind` 레코드가 `count`개 모일 때까지 기다린다
    pub fn wait_for(&self, kind: &str, count: usize) -> Vec<Value> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let found = self.records_of(kind);
            if found.len() >= count {
                return found;
            }
            assert!(Instant::now() < deadline, "{kind} 레코드가 오지 않았다");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// hello에 ready로 답하고, 그 뒤 레코드는 모아 둔다
fn spawn_reader(mut stream: UnixStream, records: Arc<Mutex<Vec<Value>>>, build_hash: String) {
    std::thread::spawn(move || {
        let mut decoder = HostRecordDecoder::new();
        let mut buffer = vec![0; 64 * 1024];
        let mut writer = stream.try_clone().unwrap();
        while let Ok(count @ 1..) = stream.read(&mut buffer) {
            let Ok(decoded) = decoder.push(&buffer[..count]) else {
                return;
            };
            for record in decoded {
                if record.get("type") == Some(&Value::from("hello")) {
                    let ready = json!({ "type": "ready", "version": 2, "namespace": "qa-client", "buildHash": build_hash, "pid": 123, "instanceId": "server", "capabilities": ["exec", "socket", "hooks", "pty"] });
                    let _ = writer.write_all(&encode_host_record(&record_from(ready)).unwrap());
                }
                records.lock().unwrap().push(Value::Object(record));
            }
        }
    });
}
