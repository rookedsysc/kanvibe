//! 원격 터미널 agent 테스트 도구: 소유자 봉투, 직접 WSS 연결, python3 가짜 controller.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use kanvibe_remote_agent::terminal_agent::{TerminalAgentHandle, start_terminal_agent};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, ring::default_provider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};
use serde_json::{Value, json};
use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, WebSocket};

const WAIT_LIMIT: Duration = Duration::from_secs(10);
const PANE_ID: &str = "w1:p1";

pub fn wait_until(description: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT_LIMIT;
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "기다리다 시간이 다 됐다: {description}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn has_python() -> bool {
    let has = Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !has {
        eprintln!("python3가 없어 가짜 Herdr controller를 띄울 수 없으므로 건너뛴다");
    }
    has
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

const CONTROLLER_SCRIPT: &str = r#"import sys, json, base64, os
counter, geometry = sys.argv[1], sys.argv[2]
cols, rows = int(sys.argv[3]), int(sys.argv[4])
with open(counter + '.env', 'w') as f:
    f.write(''.join('%s=%s\n' % pair for pair in os.environ.items()))
def log(path, text):
    with open(path, 'a') as f:
        f.write(text + '\n')
def store():
    with open(geometry, 'w') as f:
        json.dump({'cols': cols, 'rows': rows}, f)
def frame(text, full=False):
    record = {'type': 'terminal.frame', 'bytes': base64.b64encode(text.encode()).decode(), 'full': full, 'width': cols, 'height': rows}
    sys.stdout.write(json.dumps(record) + '\n')
    sys.stdout.flush()
log(counter, 'start'); store(); frame('controller-ready:%dx%d' % (cols, rows), True)
while True:
    line = sys.stdin.readline()
    if not line:
        break
    log(counter + '.records', line.strip())
    message = json.loads(line)
    kind = message['type']
    if kind == 'terminal.input':
        log(counter, 'input:' + message['bytes']); frame(base64.b64decode(message['bytes']).decode())
    elif kind == 'terminal.resize':
        cols, rows = message['cols'], message['rows']; store(); frame('resized:%dx%d' % (cols, rows), True)
    elif kind == 'terminal.scroll':
        log(counter, 'scroll:' + json.dumps(message, separators=(',', ':')))
    elif kind == 'terminal.release':
        break
"#;

const GEOMETRY_SCRIPT: &str = r#"import sys, json
g = json.load(open(sys.argv[1])); pane = sys.argv[2]
first = {'result': {'pane': {'pane_id': pane, 'scroll': {'viewport_rows': g['rows']}}}}
second = {'result': {'layout': {'area': {'width': g['cols']}, 'focused_pane_id': pane, 'zoomed': False, 'panes': [{'pane_id': pane, 'rect': {'width': g['cols']}}]}}}
sys.stdout.write(json.dumps(first) + '\x1e' + json.dumps(second))
"#;

/// 가짜 controller 스크립트와 그 기록 파일
pub struct FixtureFiles {
    directory: tempfile::TempDir,
}

impl FixtureFiles {
    fn create() -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("controller.py"), CONTROLLER_SCRIPT).unwrap();
        std::fs::write(directory.path().join("geom.py"), GEOMETRY_SCRIPT).unwrap();
        let files = Self { directory };
        files.write_geometry(80, 24);
        files
    }

    pub fn directory(&self) -> String {
        self.directory.path().to_string_lossy().into_owned()
    }

    fn path(&self, name: &str) -> String {
        self.directory
            .path()
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    pub fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.path(name)).unwrap_or_default()
    }

    pub fn write_geometry(&self, cols: u32, rows: u32) {
        std::fs::write(
            self.path("geometry"),
            json!({ "cols": cols, "rows": rows }).to_string(),
        )
        .unwrap();
    }

    /// TS 테스트의 `makeGrant`. 같은 pane(target key)을 가리키고 controller는 grant 크기(80x24)로 뜬다
    pub fn grant(&self, grant_id: &str, device_id: &str) -> Value {
        let python = |script: &str, arguments: &[String]| {
            let quoted: Vec<String> = arguments.iter().map(|argument| quote(argument)).collect();
            format!("python3 {} {}", quote(&self.path(script)), quoted.join(" "))
        };
        json!({
            "grantId": grant_id, "token": format!("{grant_id}-capability-{}", "a".repeat(64)),
            "deviceId": device_id, "taskId": "task-b", "paneId": PANE_ID, "targetKey": "owned-session:w1:p1",
            "expiresAt": 0, "cols": 80, "rows": 24,
            "geometryCommand": python("geom.py", &[self.path("geometry"), PANE_ID.to_string()]),
            "controlCommand": python("controller.py", &[self.path("counter"), self.path("geometry"), "80".into(), "24".into()]),
            "snapshotCommand": "printf 'initial-history\\r\\n'",
        })
    }
}

/// 데스크탑 자리에서 NDJSON 봉투를 주고받는다
pub struct Owner {
    writer: Mutex<Box<dyn Write + Send>>,
    output: Arc<Mutex<Vec<u8>>>,
    next_id: AtomicU64,
}

impl Owner {
    pub fn send(&self, record: Value) {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(format!("{record}\n").as_bytes()).unwrap();
        writer.flush().unwrap();
    }

    pub fn records(&self) -> Vec<Value> {
        let output = self.output.lock().unwrap();
        String::from_utf8_lossy(&output)
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    pub fn request(&self, method: &str, body: Value) -> Value {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst).to_string();
        let mut record = json!({ "type": "request", "id": id, "method": method });
        record
            .as_object_mut()
            .unwrap()
            .extend(body.as_object().unwrap().clone());
        self.send(record);
        let find = || {
            self.records()
                .into_iter()
                .find(|record| record["type"] == "response" && record["id"] == id.as_str())
        };
        wait_until(&format!("{method} 응답"), || find().is_some());
        find().unwrap()
    }

    pub fn relay_output(&self, relay_id: &str) -> String {
        self.records()
            .iter()
            .filter(|record| record["type"] == "output" && record["relayId"] == relay_id)
            .filter_map(|record| BASE64.decode(record["data"].as_str()?).ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .collect()
    }

    fn wait_ready_port(&self) -> u16 {
        let find = || {
            self.records()
                .into_iter()
                .find(|record| record["type"] == "ready")
        };
        wait_until("agent ready", || find().is_some());
        find().unwrap()["port"].as_u64().unwrap() as u16
    }
}

struct ChannelReader {
    receiver: Receiver<Vec<u8>>,
    pending: Vec<u8>,
}

impl Read for ChannelReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.pending.is_empty() {
            match self.receiver.recv() {
                Ok(bytes) => self.pending = bytes,
                Err(_) => return Ok(0),
            }
        }
        let count = buffer.len().min(self.pending.len());
        buffer[..count].copy_from_slice(&self.pending[..count]);
        self.pending.drain(..count);
        Ok(count)
    }
}

struct ChannelWriter(Sender<Vec<u8>>);

impl Write for ChannelWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0
            .send(buffer.to_vec())
            .map_err(|_| std::io::ErrorKind::BrokenPipe)?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn test_certificate() -> (String, String) {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    (certified.cert.pem(), certified.signing_key.serialize_pem())
}

/// 같은 프로세스에서 띄운 agent
pub struct Fixture {
    pub owner: Owner,
    files: FixtureFiles,
    handle: TerminalAgentHandle,
    port: u16,
}

impl Fixture {
    pub fn start() -> Option<Self> {
        if !has_python() {
            return None;
        }
        let (certificate, key) = test_certificate();
        let (sender, receiver) = channel();
        let output = Arc::new(Mutex::new(Vec::new()));
        let input = ChannelReader {
            receiver,
            pending: Vec::new(),
        };
        let handle = start_terminal_agent(
            &certificate,
            &key,
            Box::new(input),
            Box::new(SharedWriter(Arc::clone(&output))),
        )
        .unwrap();
        let owner = Owner {
            writer: Mutex::new(Box::new(ChannelWriter(sender))),
            output,
            next_id: AtomicU64::new(1),
        };
        let port = owner.wait_ready_port();
        Some(Self {
            owner,
            files: FixtureFiles::create(),
            handle,
            port,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn grant(&self, grant_id: &str, device_id: &str) -> Value {
        self.files.grant(grant_id, device_id)
    }

    pub fn write_geometry(&self, cols: u32, rows: u32) {
        self.files.write_geometry(cols, rows);
    }

    pub fn geometry(&self) -> (u64, u64) {
        let geometry: Value =
            serde_json::from_str(&self.files.read("geometry")).unwrap_or_default();
        (
            geometry["cols"].as_u64().unwrap_or(0),
            geometry["rows"].as_u64().unwrap_or(0),
        )
    }

    pub fn counter_lines(&self) -> Vec<String> {
        self.files
            .read("counter")
            .lines()
            .map(str::to_string)
            .collect()
    }

    pub fn controller_records(&self) -> Vec<String> {
        self.files
            .read("counter.records")
            .lines()
            .map(str::to_string)
            .collect()
    }

    pub fn connect(&self, grant: &Value) -> Result<DirectSocket, u16> {
        self.connect_with_token(grant, grant["token"].as_str().unwrap())
    }

    pub fn connect_with_token(&self, grant: &Value, token: &str) -> Result<DirectSocket, u16> {
        DirectSocket::connect(self.port, grant["grantId"].as_str().unwrap(), token)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.handle.stop();
        self.handle.wait_until_stopped();
    }
}

/// 실제 실행 파일을 `terminal-agent` 모드로 띄운 agent. 자식 환경에만 서버 런타임 변수를 넣는다
pub struct BinaryFixture {
    pub owner: Owner,
    pub files: FixtureFiles,
    child: Child,
}

impl BinaryFixture {
    pub fn start(environment: &[(&str, &str)]) -> Option<Self> {
        if !has_python() {
            return None;
        }
        let build_hash = "a".repeat(64);
        let mut command = Command::new(env!("CARGO_BIN_EXE_kanvibe-remote-agent"));
        command.args(["terminal-agent", "qa-terminal-env", &build_hash]);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, value) in environment {
            command.env(key, value);
        }
        let mut child = command.spawn().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut first = String::new();
        stdout.read_line(&mut first).unwrap();
        assert!(first.contains("bootstrap-ready"));
        let (certificate, key) = test_certificate();
        let mut stdin = child.stdin.take().unwrap();
        // 데스크탑은 아는 플랫폼 바이너리 hash를 모두 쉼표로 이어 보낸다
        let source = format!("{},{build_hash}", "b".repeat(64));
        let bootstrap =
            json!({ "source": source, "certificate": { "cert": certificate, "key": key } });
        stdin
            .write_all(format!("{bootstrap}\n").as_bytes())
            .unwrap();
        let output = Arc::new(Mutex::new(Vec::new()));
        let collected = Arc::clone(&output);
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut stdout, &mut SharedWriter(collected));
        });
        let owner = Owner {
            writer: Mutex::new(Box::new(stdin)),
            output,
            next_id: AtomicU64::new(1),
        };
        owner.wait_ready_port();
        Some(Self {
            owner,
            files: FixtureFiles::create(),
            child,
        })
    }
}

impl Drop for BinaryFixture {
    fn drop(&mut self) {
        self.owner.send(json!({ "type": "shutdown" }));
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug)]
struct AcceptAnyCertificate(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAnyCertificate {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            certificate,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            certificate,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

type ClientTls = StreamOwned<ClientConnection, TcpStream>;

fn connect_tls(port: u16) -> ClientTls {
    let provider = Arc::new(default_provider());
    let config = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyCertificate(provider)))
        .with_no_client_auth();
    let connection =
        ClientConnection::new(Arc::new(config), ServerName::try_from("localhost").unwrap())
            .unwrap();
    let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    StreamOwned::new(connection, tcp)
}

/// 평범한 HTTPS GET의 상태 코드. 500ms 안에 답이 없으면 `None`이다
pub fn https_get_status(port: u16, path: &str) -> Option<u16> {
    let mut tls = connect_tls(port);
    tls.sock
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    tls.write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
        .ok()?;
    let mut response = Vec::new();
    let mut chunk = [0u8; 1024];
    while !response.windows(2).any(|pair| pair == b"\r\n") {
        let count = tls.read(&mut chunk).ok().filter(|count| *count > 0)?;
        response.extend_from_slice(&chunk[..count]);
    }
    String::from_utf8_lossy(&response)
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

enum SocketCommand {
    Send(Message),
    Close,
}

#[derive(Default)]
struct SocketRecord {
    messages: Vec<String>,
    closure: Option<(u16, String)>,
}

/// 직접 붙은 모바일 자리. 스레드 하나가 소켓을 돌며 받은 메시지를 모은다
pub struct DirectSocket {
    commands: Sender<SocketCommand>,
    record: Arc<Mutex<SocketRecord>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl DirectSocket {
    fn connect(port: u16, grant_id: &str, token: &str) -> Result<Self, u16> {
        let mut request = format!("wss://127.0.0.1:{port}/stream/{grant_id}")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
        let socket = match tungstenite::client(request, connect_tls(port)) {
            Ok((socket, _)) => socket,
            Err(tungstenite::HandshakeError::Failure(tungstenite::Error::Http(response))) => {
                return Err(response.status().as_u16());
            }
            Err(_) => return Err(0),
        };
        socket
            .get_ref()
            .sock
            .set_read_timeout(Some(Duration::from_millis(10)))
            .unwrap();
        let (commands, receiver) = channel();
        let record = Arc::new(Mutex::new(SocketRecord::default()));
        let collected = Arc::clone(&record);
        let worker = std::thread::spawn(move || pump(socket, &receiver, &collected));
        Ok(Self {
            commands,
            record,
            worker: Some(worker),
        })
    }

    pub fn send_text(&self, text: &str) {
        self.commands
            .send(SocketCommand::Send(Message::text(text)))
            .unwrap();
    }

    pub fn send_binary(&self, bytes: &[u8]) {
        self.commands
            .send(SocketCommand::Send(Message::binary(bytes.to_vec())))
            .unwrap();
    }

    pub fn messages(&self) -> Vec<String> {
        self.record.lock().unwrap().messages.clone()
    }

    pub fn received(&self) -> String {
        self.messages().concat()
    }

    pub fn closure(&self) -> Option<(u16, String)> {
        self.record.lock().unwrap().closure.clone()
    }

    /// 닫기 핸드셰이크를 하고 소켓 스레드가 끝나기를 기다린다
    pub fn close(mut self) {
        let _ = self.commands.send(SocketCommand::Close);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn pump(
    mut socket: WebSocket<ClientTls>,
    commands: &Receiver<SocketCommand>,
    record: &Mutex<SocketRecord>,
) {
    loop {
        match commands.try_recv() {
            Ok(SocketCommand::Send(message)) => {
                let _ = socket.send(message);
            }
            Ok(SocketCommand::Close) => {
                let _ = socket.close(None);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return,
        }
        match socket.read() {
            Ok(Message::Text(text)) => record.lock().unwrap().messages.push(text.to_string()),
            Ok(Message::Close(Some(frame))) => {
                record.lock().unwrap().closure =
                    Some((u16::from(frame.code), frame.reason.to_string()));
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }
    }
}

impl Drop for DirectSocket {
    fn drop(&mut self) {
        let _ = self.commands.send(SocketCommand::Close);
    }
}
