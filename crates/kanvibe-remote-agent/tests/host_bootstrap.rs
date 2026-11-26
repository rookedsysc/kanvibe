//! 실제 바이너리와 sh loader로 원격 부트스트랩·bridge·daemon 수명을 확인한다.
//! namespace는 테스트마다 새 `qa-*` 이름을 써서 `/tmp/kv-<uid>/<namespace>` 아래만 쓰고 끝나면 지운다.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use kanvibe_remote_agent::bootstrap::{
    AgentBinaryDigest, AgentLaunchMode, build_agent_loader_script,
};
use kanvibe_remote_agent::protocol::{
    HostRecord, HostRecordDecoder, current_uid, decode_host_bytes, encode_host_record, record_from,
    record_text,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const AGENT_BINARY: &str = env!("CARGO_BIN_EXE_kanvibe-remote-agent");

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn platform_key() -> String {
    let uname = |flag: &str| {
        String::from_utf8(Command::new("uname").arg(flag).output().unwrap().stdout).unwrap()
    };
    format!("{}-{}", uname("-s").trim(), uname("-m").trim())
}

/// 테스트가 쓰는 namespace 디렉터리. 끝나면 남은 daemon을 내리고 디렉터리를 지운다
struct NamespaceFixture {
    namespace: String,
    daemon_pids: Vec<i32>,
}

impl NamespaceFixture {
    fn new(prefix: &str) -> Self {
        let suffix = &uuid::Uuid::new_v4().simple().to_string()[..8];
        Self {
            namespace: format!("qa-{prefix}-{suffix}"),
            daemon_pids: Vec::new(),
        }
    }

    fn directory(&self) -> PathBuf {
        PathBuf::from(format!("/tmp/kv-{}/{}", current_uid(), self.namespace))
    }

    fn loader(&self, binary: &[u8]) -> String {
        let digest = AgentBinaryDigest {
            platform: platform_key(),
            sha256: sha256_hex(binary),
        };
        build_agent_loader_script(&self.namespace, AgentLaunchMode::HostBridge, &[digest]).unwrap()
    }
}

impl Drop for NamespaceFixture {
    fn drop(&mut self) {
        for pid in &self.daemon_pids {
            // SAFETY: 이 테스트가 띄운 daemon pid에만 보낸다
            unsafe { libc::kill(*pid, libc::SIGTERM) };
            wait_for_exit(*pid);
        }
        let _ = std::fs::remove_dir_all(self.directory());
    }
}

fn is_alive(pid: i32) -> bool {
    // SAFETY: 신호 0은 존재 확인만 한다
    unsafe { libc::kill(pid, 0) == 0 }
}

fn wait_for_exit(pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while is_alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// loader 하나. 첫 줄들은 JSON 줄이고, bridge가 붙은 뒤로는 길이 접두 레코드다
struct LoaderRun {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<Value>,
    records: Receiver<HostRecord>,
}

fn spawn_loader(script: &str) -> LoaderRun {
    let mut child = Command::new("sh")
        .args(["-c", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().unwrap();
    let (line_sender, lines) = channel();
    let (record_sender, records) = channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let value: Value = serde_json::from_str(line.trim()).unwrap();
            let is_bridged = value["type"] == "bridge-ready";
            let _ = line_sender.send(value);
            if is_bridged {
                break;
            }
        }
        let mut decoder = HostRecordDecoder::new();
        let mut buffer = vec![0; 64 * 1024];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                return;
            }
            for record in decoder.push(&buffer[..count]).unwrap() {
                let _ = record_sender.send(record);
            }
        }
    });
    LoaderRun {
        child,
        stdin,
        lines,
        records,
    }
}

impl LoaderRun {
    fn next_line(&self) -> Value {
        self.lines
            .recv_timeout(Duration::from_secs(15))
            .expect("loader 줄이 오지 않았다")
    }

    fn send(&mut self, record: Value) {
        let bytes = encode_host_record(&record_from(record)).unwrap();
        self.stdin.as_mut().unwrap().write_all(&bytes).unwrap();
    }

    fn next(&self, kind: &str) -> HostRecord {
        loop {
            let record = self
                .records
                .recv_timeout(Duration::from_secs(15))
                .unwrap_or_else(|_| panic!("{kind} 레코드가 오지 않았다"));
            if record_text(&record, "type") == Some(kind) {
                return record;
            }
        }
    }

    /// 업로드를 요구하면 바이너리를 보내고 게시 결과를 기다린다
    fn upload(mut self, binary: &[u8]) {
        assert_eq!(self.next_line()["type"], "upload-required");
        let mut stdin = self.stdin.take().unwrap();
        stdin.write_all(binary).unwrap();
        drop(stdin);
        assert_eq!(self.next_line()["type"], "uploaded");
        assert!(self.child.wait().unwrap().success());
    }

    /// 이미 올라간 바이너리로 bridge에 붙어 hello까지 마친다
    fn handshake(mut self, namespace: &str, build_hash: &str) -> (Self, HostRecord) {
        assert_eq!(self.next_line()["type"], "bridge-ready");
        let client_id = uuid::Uuid::new_v4().to_string();
        self.send(json!({ "type": "hello", "version": 2, "namespace": namespace, "clientId": client_id, "role": "desktop", "buildHash": build_hash }));
        let ready = self.next("ready");
        (self, ready)
    }
}

impl Drop for LoaderRun {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn install(fixture: &NamespaceFixture, binary: &[u8]) {
    spawn_loader(&fixture.loader(binary)).upload(binary);
}

fn connect(fixture: &mut NamespaceFixture, binary: &[u8]) -> (LoaderRun, HostRecord) {
    let run = spawn_loader(&fixture.loader(binary));
    let (run, ready) = run.handshake(&fixture.namespace, &sha256_hex(binary));
    fixture
        .daemon_pids
        .push(ready["pid"].as_i64().unwrap() as i32);
    (run, ready)
}

fn agent_binary() -> Vec<u8> {
    std::fs::read(AGENT_BINARY).unwrap()
}

#[test]
fn eight_concurrent_loaders_publish_one_complete_binary_and_share_one_daemon() {
    let mut fixture = NamespaceFixture::new("r");
    let binary = agent_binary();
    let uploads: Vec<LoaderRun> = (0..8)
        .map(|_| spawn_loader(&fixture.loader(&binary)))
        .collect();

    let uploaders: Vec<_> = uploads
        .into_iter()
        .map(|run| {
            let binary = binary.clone();
            std::thread::spawn(move || run.upload(&binary))
        })
        .collect();
    uploaders
        .into_iter()
        .for_each(|uploader| uploader.join().unwrap());
    let bridges: Vec<LoaderRun> = (0..8)
        .map(|_| spawn_loader(&fixture.loader(&binary)))
        .collect();
    let readies: Vec<(LoaderRun, HostRecord)> = bridges
        .into_iter()
        .map(|run| run.handshake(&fixture.namespace, &sha256_hex(&binary)))
        .collect();

    let published = std::fs::read(
        fixture
            .directory()
            .join(format!("agent-{}", sha256_hex(&binary))),
    )
    .unwrap();
    assert_eq!(published, binary);
    let instance_ids: std::collections::HashSet<String> = readies
        .iter()
        .map(|(_, ready)| ready["instanceId"].to_string())
        .collect();
    let pids: std::collections::HashSet<String> = readies
        .iter()
        .map(|(_, ready)| ready["pid"].to_string())
        .collect();
    assert_eq!((instance_ids.len(), pids.len()), (1, 1));
    fixture
        .daemon_pids
        .push(readies[0].1["pid"].as_i64().unwrap() as i32);
}

#[test]
fn a_detached_daemon_outlives_one_bridge_and_serves_the_next() {
    let mut fixture = NamespaceFixture::new("b");
    let binary = agent_binary();
    install(&fixture, &binary);

    let (first, one) = connect(&mut fixture, &binary);
    drop(first);
    let (mut second, two) = connect(&mut fixture, &binary);

    assert_eq!(
        (two["instanceId"].clone(), two["pid"].clone()),
        (one["instanceId"].clone(), one["pid"].clone())
    );
    second.send(json!({ "type": "exec.open", "id": "binary", "command": "printf '\\377\\000a'", "timeoutMs": 3000, "maxBufferBytes": 4096 }));
    let output = second.next("exec.data");
    assert_eq!(
        decode_host_bytes(output.get("data"), 1024).unwrap(),
        vec![255, 0, 97]
    );
    assert_eq!(second.next("exec.exit")["code"], json!(0));
    second.send(json!({ "type": "stop", "id": "owned-stop" }));
    second.next("stopped");
    wait_for_exit(one["pid"].as_i64().unwrap() as i32);
    assert!(!is_alive(one["pid"].as_i64().unwrap() as i32));
}

#[test]
fn a_previous_build_daemon_is_replaced_instead_of_rejecting_the_new_build() {
    let mut fixture = NamespaceFixture::new("u");
    let current = agent_binary();
    // 실행 파일 뒤에 붙인 바이트는 실행에 영향이 없고 hash만 바꾼다
    let previous = [current.as_slice(), b"\n// previous build\n"].concat();
    install(&fixture, &previous);
    install(&fixture, &current);

    let (_previous_bridge, previous_ready) = connect(&mut fixture, &previous);
    let (_current_bridge, current_ready) = connect(&mut fixture, &current);

    assert_eq!(current_ready["buildHash"], json!(sha256_hex(&current)));
    assert_ne!(current_ready["pid"], previous_ready["pid"]);
    let previous_pid = previous_ready["pid"].as_i64().unwrap() as i32;
    wait_for_exit(previous_pid);
    assert!(!is_alive(previous_pid));
}

/// Node daemon(`node <dir>/server-<h>.cjs --serve <ns> <h>`)처럼 보이는 이전 빌드를 Rust bridge가 내리는지 본다
#[test]
fn an_older_node_daemon_is_stopped_before_the_rust_daemon_starts() {
    let Ok(python) = which_python() else {
        eprintln!("python3가 없어 Node daemon 흉내를 건너뛴다");
        return;
    };
    let mut fixture = NamespaceFixture::new("n");
    let binary = agent_binary();
    install(&fixture, &binary);
    let node_hash = "c".repeat(64);
    let node_bundle = fixture.directory().join(format!("server-{node_hash}.cjs"));
    std::fs::write(&node_bundle, fake_node_daemon(&fixture.directory())).unwrap();
    let mut node_daemon = Command::new(python)
        .args([
            node_bundle.to_str().unwrap(),
            "--serve",
            &fixture.namespace,
            &node_hash,
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut listening = String::new();
    BufReader::new(node_daemon.stdout.take().unwrap())
        .read_line(&mut listening)
        .unwrap();
    // 실제 Node daemon은 init이 회수하는 고아 프로세스다. 테스트 자식은 좀비로 남지 않게 따로 회수한다
    let (exit_sender, node_exit) = channel();
    std::thread::spawn(move || {
        let _ = exit_sender.send(node_daemon.wait().unwrap());
    });

    let (_bridge, ready) = connect(&mut fixture, &binary);

    assert_eq!(ready["runtime"]["implementation"], json!("rust"));
    let status = node_exit.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(!status.success(), "SIGTERM으로 끝나야 한다");
}

fn which_python() -> Result<String, ()> {
    let found = Command::new("sh")
        .args(["-c", "command -v python3"])
        .output()
        .map_err(|_| ())?;
    let path = String::from_utf8_lossy(&found.stdout).trim().to_string();
    if path.is_empty() { Err(()) } else { Ok(path) }
}

/// rpc 소켓과 start.lock을 쥐고 SIGTERM에 끝나는 Node daemon 흉내(파일 이름만 .cjs인 Python)
fn fake_node_daemon(directory: &Path) -> String {
    let directory = directory.display();
    format!(
        "import os, socket, sys\n\
         s = socket.socket(socket.AF_UNIX); s.bind('{directory}/rpc-v1.sock'); s.listen(8)\n\
         os.mkdir('{directory}/start.lock', 0o700)\n\
         open('{directory}/start.lock/pid', 'w').write(str(os.getpid()))\n\
         print('listening', flush=True)\n\
         while True: s.accept()\n"
    )
}

#[test]
fn a_start_lock_owner_that_is_not_a_kanvibe_daemon_is_left_alone() {
    let fixture = NamespaceFixture::new("l");
    let binary = agent_binary();
    install(&fixture, &binary);
    // KanVibe daemon이 아닌 프로세스(이 테스트 프로세스)가 rpc 소켓과 start.lock을 쥔다
    let directory = fixture.directory();
    let _listener = UnixListener::bind(directory.join("rpc-v1.sock")).unwrap();
    std::fs::create_dir(directory.join("start.lock")).unwrap();
    std::fs::write(
        directory.join("start.lock/pid"),
        std::process::id().to_string(),
    )
    .unwrap();

    let run = spawn_loader(&fixture.loader(&binary));

    assert_eq!(run.next_line()["type"], "bridge-ready");
    assert!(is_alive(std::process::id() as i32));
}

#[test]
fn server_runtime_environment_never_reaches_user_commands_or_terminals() {
    let fixture = NamespaceFixture::new("e");
    let build_hash = sha256_hex(b"environment fixture");
    let mut daemon = Command::new(AGENT_BINARY)
        .args(["host-serve", &fixture.namespace, &build_hash])
        .env("PORT", "4100")
        .env("HOST", "0.0.0.0")
        .env("NODE_ENV", "production")
        .env("KANVIBE_PROVIDER_SECRET", "private-runtime")
        .spawn()
        .unwrap();
    let socket_path = fixture.directory().join("rpc-v1.sock");
    let mut peer = connect_socket(&socket_path, &fixture.namespace, &build_hash);
    let probe = r#"printf '<%s|%s|%s|%s>' "${PORT-unset}" "${HOST-unset}" "${NODE_ENV-unset}" "${KANVIBE_PROVIDER_SECRET-unset}""#;

    peer.send(json!({ "type": "exec.open", "id": "env", "command": probe, "timeoutMs": 3000, "maxBufferBytes": 4096 }));
    let command_output = decode_host_bytes(peer.next("exec.data").get("data"), 4096).unwrap();
    peer.send(json!({ "type": "pty.open", "id": "terminal", "key": "env", "command": format!("{probe}; sleep 5"), "cols": 80, "rows": 24 }));
    let terminal_output = peer.collect_pty_output("terminal", "unset>");
    peer.send(json!({ "type": "stop", "id": "stop" }));
    peer.next("stopped");
    let _ = daemon.wait();

    assert_eq!(
        String::from_utf8(command_output).unwrap(),
        "<unset|unset|unset|unset>"
    );
    assert!(
        terminal_output.contains("<unset|unset|unset|unset>"),
        "{terminal_output}"
    );
}

/// daemon rpc 소켓에 직접 붙는 데스크탑
struct SocketPeer {
    stream: std::os::unix::net::UnixStream,
    records: Receiver<HostRecord>,
}

fn connect_socket(socket_path: &Path, namespace: &str, build_hash: &str) -> SocketPeer {
    let deadline = Instant::now() + Duration::from_secs(5);
    let stream = loop {
        match std::os::unix::net::UnixStream::connect(socket_path) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => panic!("daemon 소켓에 붙지 못했다: {error}"),
        }
    };
    let (sender, records) = channel();
    let mut reader = stream.try_clone().unwrap();
    std::thread::spawn(move || {
        let mut decoder = HostRecordDecoder::new();
        let mut buffer = vec![0; 64 * 1024];
        while let Ok(count @ 1..) = reader.read(&mut buffer) {
            for record in decoder.push(&buffer[..count]).unwrap() {
                let _ = sender.send(record);
            }
        }
    });
    let mut peer = SocketPeer { stream, records };
    peer.send(json!({ "type": "hello", "version": 2, "namespace": namespace, "clientId": "env-client", "role": "desktop", "buildHash": build_hash }));
    peer.next("ready");
    peer
}

impl SocketPeer {
    fn send(&mut self, record: Value) {
        self.stream
            .write_all(&encode_host_record(&record_from(record)).unwrap())
            .unwrap();
    }

    fn next(&self, kind: &str) -> HostRecord {
        loop {
            let record = self
                .records
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|_| panic!("{kind} 레코드가 오지 않았다"));
            if record_text(&record, "type") == Some(kind) {
                return record;
            }
        }
    }

    fn collect_pty_output(&self, id: &str, until: &str) -> String {
        let mut output = String::new();
        while !output.contains(until) {
            let record = self.next("pty.data");
            if record_text(&record, "id") == Some(id) {
                output.push_str(&String::from_utf8_lossy(
                    &decode_host_bytes(record.get("data"), 1 << 20).unwrap(),
                ));
            }
        }
        output
    }
}
