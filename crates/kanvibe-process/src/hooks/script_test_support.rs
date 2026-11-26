//! 생성한 bash 스크립트를 실제로 실행해 보는 테스트의 공용 도구.
//! TS 테스트의 `http.createServer`와 `execFile("bash")` 자리로, 요청을 기록하는 HTTP 서버(TCP·Unix 소켓)와
//! 좁은 환경으로 셸을 띄우는 함수를 둔다.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// 서버가 받은 요청 하나
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReceivedRequest {
    pub path: String,
    pub body: String,
    pub authorization: Option<String>,
    pub client_id: Option<String>,
}

/// 요청을 받아 (상태 코드, 응답 본문)을 정한다
pub(crate) type RequestResponder = dyn Fn(&ReceivedRequest) -> (u16, String) + Send + Sync;

pub(crate) struct RecordingServer {
    received: Arc<Mutex<Vec<ReceivedRequest>>>,
    /// TCP로 열었으면 `http://127.0.0.1:<port>`
    pub url: String,
}

impl RecordingServer {
    pub fn received(&self) -> Vec<ReceivedRequest> {
        self.received.lock().unwrap().clone()
    }

    /// 127.0.0.1의 빈 포트에서 듣는다. 응답은 TS `response.end("{}")`처럼 200과 `{}`다
    pub fn tcp() -> Self {
        Self::tcp_with(|_| (200, "{}".to_string()))
    }

    pub fn tcp_with(
        responder: impl Fn(&ReceivedRequest) -> (u16, String) + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("TCP 포트를 열 수 있어야 한다");
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let received = Arc::new(Mutex::new(Vec::new()));
        let responder: Arc<RequestResponder> = Arc::new(responder);
        let shared_received = Arc::clone(&received);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                serve_connection(stream, &shared_received, responder.as_ref());
            }
        });
        Self { received, url }
    }

    pub fn unix(socket_path: &Path) -> Self {
        Self::unix_with(socket_path, |_| (200, "{}".to_string()))
    }

    pub fn unix_with(
        socket_path: &Path,
        responder: impl Fn(&ReceivedRequest) -> (u16, String) + Send + Sync + 'static,
    ) -> Self {
        let listener = UnixListener::bind(socket_path).expect("Unix 소켓을 열 수 있어야 한다");
        let received = Arc::new(Mutex::new(Vec::new()));
        let responder: Arc<RequestResponder> = Arc::new(responder);
        let shared_received = Arc::clone(&received);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                serve_connection(stream, &shared_received, responder.as_ref());
            }
        });
        Self {
            received,
            url: String::new(),
        }
    }
}

/// 열었다 바로 닫아 연결이 거부되는 주소
pub(crate) fn reserve_closed_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port())
}

fn read_request(stream: &mut impl Read) -> Option<ReceivedRequest> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    let path = request_line.split_whitespace().nth(1)?.to_string();
    let mut headers = BTreeMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let content_length: usize = headers
        .get("content-length")
        .and_then(|length| length.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0_u8; content_length];
    reader.read_exact(&mut body).ok()?;
    Some(ReceivedRequest {
        path,
        body: String::from_utf8_lossy(&body).into_owned(),
        authorization: headers.get("authorization").cloned(),
        client_id: headers.get("x-kanvibe-client-id").cloned(),
    })
}

fn serve_connection(
    mut stream: impl Read + Write,
    received: &Mutex<Vec<ReceivedRequest>>,
    responder: &RequestResponder,
) {
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    received.lock().unwrap().push(request.clone());
    let (status, body) = responder(&request);
    let response = format!(
        "HTTP/1.1 {status} STATUS\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

/// 셸 실행 결과. 종료 코드는 신호로 끝났으면 -1이다
#[derive(Debug)]
pub(crate) struct ShellRun {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// 테스트 프로세스의 PATH만 물려받고 HOME은 작업 디렉터리로 둔 좁은 환경
pub(crate) fn narrow_test_environment(home_directory: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        ),
        ("HOME".to_string(), home_directory.display().to_string()),
        ("LANG".to_string(), "C.UTF-8".to_string()),
    ])
}

pub(crate) fn run_program(
    program: &str,
    arguments: &[&str],
    cwd: &Path,
    environment: &BTreeMap<String, String>,
    stdin: &[u8],
) -> ShellRun {
    let mut child = Command::new(program)
        .args(arguments)
        .current_dir(cwd)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("{program}: {error}"));
    let mut input = child.stdin.take().unwrap();
    let stdin = stdin.to_vec();
    let writer = std::thread::spawn(move || drop(input.write_all(&stdin)));
    let output = child.wait_with_output().unwrap();
    let _ = writer.join();
    ShellRun {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// `bash -c <script>`를 좁은 환경으로 실행한다
pub(crate) fn run_bash(script: &str, cwd: &Path) -> ShellRun {
    run_program(
        "bash",
        &["-c", script],
        cwd,
        &narrow_test_environment(cwd),
        b"",
    )
}

pub(crate) fn git(arguments: &[&str], cwd: &Path) {
    let run = run_program("git", arguments, cwd, &narrow_test_environment(cwd), b"");
    assert_eq!(run.code, 0, "git {arguments:?}: {}", run.stderr);
}
