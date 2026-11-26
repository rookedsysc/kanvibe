//! Herdr API 소켓의 `tab.move` 요청. CLI에 탭 이동 명령이 없어 서버 소켓에 JSON 한 줄을 직접 보낸다.
//! 원격 서버는 이 요청 하나만을 위한 단독 SSH 터널로 소켓을 로컬에 잇고, 끝나면 터널과 임시 디렉터리를 정리한다.

use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use kanvibe_ai::javascript::utf16_last;
use kanvibe_process::ssh_config::{
    SshArgsOptions, build_ssh_args, kanvibe_ssh_connection_health_options,
};
use serde_json::{Value, json};

use super::response::read_herdr_result;
use super::{HERDR_COMMAND_TIMEOUT, HERDR_START_TIMEOUT, HerdrError, HerdrService, parse_target};

const HERDR_SOCKET_RESPONSE_MAX_BYTES: usize = 1024 * 1024;
const TUNNEL_READY_POLL_INTERVAL: Duration = Duration::from_millis(25);
const TUNNEL_FORCE_KILL_DELAY: Duration = Duration::from_millis(1_000);
const TUNNEL_STDERR_MAX_UNITS: usize = 4096;

fn message(text: &str) -> HerdrError {
    HerdrError::Message(text.to_string())
}

fn read_socket_error(error: std::io::Error) -> HerdrError {
    match error.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => message("Herdr 소켓 응답 시간 초과"),
        _ => HerdrError::Message(error.to_string()),
    }
}

/// 첫 줄이 올 때까지 읽는다. 응답이 여러 조각으로 나뉘어 와도 이어 붙인다
fn read_first_line(socket: &mut UnixStream) -> Result<String, HerdrError> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = socket.read(&mut chunk).map_err(read_socket_error)?;
        if read == 0 {
            return Err(message("Herdr 소켓 응답이 끊겼습니다."));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > HERDR_SOCKET_RESPONSE_MAX_BYTES {
            return Err(message("Herdr 응답 크기 초과"));
        }
        if let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            return Ok(String::from_utf8_lossy(&buffer[..end]).into_owned());
        }
    }
}

/// 소켓에 요청 한 줄을 한 번만 보내고 응답 첫 줄을 해석한다. 5초 동안 오가는 데이터가 없으면 실패한다
pub(super) fn request_herdr_socket(
    socket_path: &Path,
    method: &str,
    params: Value,
) -> Result<Value, HerdrError> {
    let mut socket =
        UnixStream::connect(socket_path).map_err(|error| HerdrError::Message(error.to_string()))?;
    socket
        .set_read_timeout(Some(HERDR_COMMAND_TIMEOUT))
        .and_then(|()| socket.set_write_timeout(Some(HERDR_COMMAND_TIMEOUT)))
        .map_err(|error| HerdrError::Message(error.to_string()))?;
    let request = json!({ "id": "kanvibe", "method": method, "params": params });
    socket
        .write_all(format!("{request}\n").as_bytes())
        .map_err(read_socket_error)?;
    let line = read_first_line(&mut socket)?;
    let response: Value = super::response::parse_json(&line)?;
    if response.get("id").and_then(Value::as_str) != Some("kanvibe") {
        return Err(message("Herdr 소켓 응답 ID가 일치하지 않습니다."));
    }
    read_herdr_result(&line)
}

/// 로컬 소켓 경로를 담을 임시 디렉터리. Unix 소켓 경로 길이 제한 때문에 이름을 짧게 둔다
fn create_tunnel_directory(temporary_root: &str) -> Result<PathBuf, HerdrError> {
    loop {
        let suffix: String = uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(6)
            .collect();
        let directory = Path::new(temporary_root).join(format!("kvh-{suffix}"));
        match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(HerdrError::Message(error.to_string())),
        }
    }
}

fn is_socket(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.file_type().is_socket())
}

/// 터널의 stderr 끝부분을 모은다. 터널이 먼저 끝나면 이 내용이 실패 사유다
fn collect_stderr(stderr: ChildStderr) -> (Arc<Mutex<String>>, JoinHandle<()>) {
    let collected = Arc::new(Mutex::new(String::new()));
    let sink = Arc::clone(&collected);
    let reader = std::thread::spawn(move || {
        let mut stderr = stderr;
        let mut chunk = [0_u8; 4096];
        while let Ok(read) = stderr.read(&mut chunk) {
            if read == 0 {
                break;
            }
            let mut text = sink.lock().unwrap();
            let appended = format!("{text}{}", String::from_utf8_lossy(&chunk[..read]));
            *text = utf16_last(&appended, TUNNEL_STDERR_MAX_UNITS).to_string();
        }
    });
    (collected, reader)
}

/// SIGTERM을 보내고 1초 안에 끝나지 않으면 SIGKILL로 끝낸 뒤 거둔다
fn stop_tunnel(tunnel: &mut Child) {
    if let Ok(process_id) = i32::try_from(tunnel.id()) {
        // SAFETY: 아직 거두지 않은 자식 프로세스에만 신호를 보낸다
        unsafe { libc::kill(process_id, libc::SIGTERM) };
    }
    let deadline = std::time::Instant::now() + TUNNEL_FORCE_KILL_DELAY;
    while std::time::Instant::now() < deadline {
        if matches!(tunnel.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(TUNNEL_READY_POLL_INTERVAL);
    }
    let _ = tunnel.kill();
    let _ = tunnel.wait();
}

impl HerdrService {
    /// TS `moveHerdrTab`. 공유 서버 세션은 자기 Space의 탭만 옮길 수 있다
    pub fn move_herdr_tab(
        &self,
        session_name: &str,
        tab_id: &str,
        target_index: i64,
        ssh_host: Option<&str>,
    ) -> Result<(), HerdrError> {
        let server_name = parse_target(session_name)?.server_name;
        let session = self.find_running_session(&server_name, ssh_host)?;
        let workspace_id = self.get_herdr_workspace_id(session_name, ssh_host)?;
        if workspace_id.is_some_and(|workspace_id| !tab_id.starts_with(&format!("{workspace_id}:")))
        {
            return Err(message("다른 Space의 탭을 이동할 수 없습니다."));
        }
        let params = json!({ "tab_id": tab_id, "insert_index": target_index });
        let socket_path = session.socket_path.unwrap_or_default();
        let Some(ssh_host) = ssh_host else {
            return request_herdr_socket(Path::new(&socket_path), "tab.move", params).map(|_| ());
        };
        let directory = create_tunnel_directory(&self.temporary_root())?;
        let result = self.request_through_tunnel(ssh_host, &directory, &socket_path, params);
        let _ = std::fs::remove_dir_all(&directory);
        result
    }

    /// Node `os.tmpdir()`. 좁힌 셸 환경의 TMPDIR, 없으면 `/tmp`다
    fn temporary_root(&self) -> String {
        let root = self.environment.variable("TMPDIR").unwrap_or("/tmp");
        match root.trim_end_matches('/') {
            "" => "/".to_string(),
            trimmed => trimmed.to_string(),
        }
    }

    fn tunnel_command(
        &self,
        ssh_host: &str,
        local_socket: &Path,
        remote_socket: &str,
    ) -> Result<Command, HerdrError> {
        let hosts = self.ssh_config.parse_ssh_config();
        let Some(host) = hosts.iter().find(|entry| entry.host == ssh_host) else {
            return Err(HerdrError::Message(format!(
                "SSH 호스트를 찾을 수 없습니다: {ssh_host}"
            )));
        };
        let options = SshArgsOptions {
            is_tty_disabled: true,
            connection_health: Some(kanvibe_ssh_connection_health_options()),
            ..SshArgsOptions::default()
        };
        let mut ssh_arguments = build_ssh_args(host, &options);
        let forward = format!("{}:{remote_socket}", local_socket.to_string_lossy());
        let tunnel_options = [
            "-N",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ControlMaster=no",
            "-o",
            "ControlPath=none",
            "-L",
        ];
        let destination_index = ssh_arguments.len() - 1;
        let inserted = tunnel_options
            .iter()
            .map(ToString::to_string)
            .chain([forward]);
        ssh_arguments.splice(destination_index..destination_index, inserted);
        let mut command = Command::new("ssh");
        command
            .args(ssh_arguments)
            .env_clear()
            .envs(&self.environment.shell_environment)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        Ok(command)
    }

    fn request_through_tunnel(
        &self,
        ssh_host: &str,
        directory: &Path,
        remote_socket: &str,
        params: Value,
    ) -> Result<(), HerdrError> {
        let local_socket = directory.join("api");
        let mut command = self.tunnel_command(ssh_host, &local_socket, remote_socket)?;
        let mut tunnel = command
            .spawn()
            .map_err(|error| HerdrError::Message(error.to_string()))?;
        let stderr = tunnel.stderr.take().expect("piped stderr");
        let (stderr_text, stderr_reader) = collect_stderr(stderr);
        let ready =
            self.wait_for_tunnel_socket(&mut tunnel, &local_socket, stderr_reader, &stderr_text);
        let result = ready
            .and_then(|()| request_herdr_socket(&local_socket, "tab.move", params).map(|_| ()));
        stop_tunnel(&mut tunnel);
        result
    }

    /// 로컬 소켓이 생길 때까지 기다린다. 그 전에 터널이 끝나면 stderr를 사유로 실패한다
    fn wait_for_tunnel_socket(
        &self,
        tunnel: &mut Child,
        local_socket: &Path,
        stderr_reader: JoinHandle<()>,
        stderr_text: &Mutex<String>,
    ) -> Result<(), HerdrError> {
        let deadline = self.now() + HERDR_START_TIMEOUT;
        while !is_socket(local_socket) {
            if matches!(tunnel.try_wait(), Ok(Some(_))) {
                let _ = stderr_reader.join();
                let stderr = stderr_text.lock().unwrap().clone();
                return Err(match stderr.is_empty() {
                    true => message("Herdr SSH 소켓 연결이 종료됐습니다."),
                    false => HerdrError::Message(stderr),
                });
            }
            if self.now() >= deadline {
                return Err(message("Herdr SSH 소켓 연결 시간 초과"));
            }
            std::thread::sleep(TUNNEL_READY_POLL_INTERVAL);
        }
        Ok(())
    }
}
