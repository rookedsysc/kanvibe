//! SSH 세션 안에서 도는 bridge(TS `startRemoteHostBridge`). stdin/stdout을 daemon의 rpc 소켓에 잇는다.
//!
//! daemon이 없으면 같은 바이너리를 `host-serve`로 띄워 SSH 연결이 끊겨도 남게 한다. namespace마다 rpc 소켓이
//! 하나뿐이라, 다른 빌드의 daemon이 떠 있으면 새 빌드 데스크탑이 붙어도 `hello`가 거절된다. 그래서 붙기 전에
//! `start.lock/pid`의 프로세스가 같은 사용자로 이 디렉터리의 다른 빌드를 `--serve`(Node) 또는 `host-serve`(Rust)
//! 하는 중인지 `ps`로 확인하고, 그렇다고 확인될 때만 SIGTERM으로 내린다. 확인할 수 없으면 건드리지 않는다.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::paths::{connect_with_timeout, is_process_alive, prepare_paths};
use crate::environment::current_child_environment;
use crate::protocol::current_uid;

const PREVIOUS_DAEMON_STOP_TIMEOUT: Duration = Duration::from_secs(8);
const DAEMON_START_TIMEOUT: Duration = Duration::from_secs(8);
const RETRY_INTERVAL: Duration = Duration::from_millis(25);

/// 부트스트랩 loader가 확인해 둔 실행 파일 이름
pub fn agent_file_name(build_hash: &str) -> String {
    format!("agent-{build_hash}")
}

/// bridge를 돌린다. rpc 소켓이 닫히거나 stdin이 끝날 때까지 돌아오지 않는다
pub fn run_bridge(
    namespace: &str,
    build_hash: &str,
    input: impl Read + Send + 'static,
    mut output: impl Write,
) -> Result<(), String> {
    let paths = prepare_paths(namespace, None)?;
    let directory = Path::new(&paths.directory);
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    assert_trusted_executable(&executable, directory, build_hash)?;
    stop_previous_build_daemon(directory, namespace, build_hash)?;
    let socket_path = Path::new(&paths.rpc_socket_path);
    let socket = match connect_with_timeout(socket_path) {
        Ok(socket) => socket,
        Err(error) if is_absent_daemon(&error) => {
            start_daemon(&executable, namespace, build_hash, socket_path)?
        }
        Err(error) => return Err(error.to_string()),
    };
    let _ = socket.set_write_timeout(None);
    // 캐시된 바이너리를 바로 실행하면 데스크탑은 원격 플랫폼을 모르므로 hello에 쓸 build hash를 알려 준다
    let ready_line = format!("{{\"type\":\"bridge-ready\",\"buildHash\":\"{build_hash}\"}}\n");
    output
        .write_all(ready_line.as_bytes())
        .map_err(|error| error.to_string())?;
    output.flush().map_err(|error| error.to_string())?;
    pipe_until_closed(socket, input, output)
}

fn is_absent_daemon(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::NotFound | ErrorKind::ConnectionRefused
    )
}

fn assert_trusted_executable(
    executable: &Path,
    directory: &Path,
    build_hash: &str,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(executable).map_err(|error| error.to_string())?;
    // macOS의 /tmp는 /private/tmp를 가리키므로 실제 경로끼리 비교한다
    let executable_directory = executable
        .parent()
        .and_then(|parent| fs::canonicalize(parent).ok());
    let is_trusted = metadata.is_file()
        && metadata.uid() == current_uid()
        && executable_directory.is_some()
        && executable_directory == fs::canonicalize(directory).ok()
        && executable.file_name().and_then(|name| name.to_str())
            == Some(agent_file_name(build_hash).as_str());
    if !is_trusted {
        return Err("Untrusted host bundle".into());
    }
    Ok(())
}

fn start_daemon(
    executable: &Path,
    namespace: &str,
    build_hash: &str,
    socket_path: &Path,
) -> Result<UnixStream, String> {
    let mut command = Command::new(executable);
    command
        .args(["host-serve", namespace, build_hash])
        .env_clear()
        .envs(current_child_environment())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid는 async-signal-safe이고 fork 뒤 exec 전에 메모리를 건드리지 않는다
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command.spawn().map_err(|error| error.to_string())?;
    let deadline = Instant::now() + DAEMON_START_TIMEOUT;
    loop {
        match connect_with_timeout(socket_path) {
            Ok(socket) => return Ok(socket),
            Err(error) if is_absent_daemon(&error) && Instant::now() < deadline => {
                std::thread::sleep(RETRY_INTERVAL)
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn pipe_until_closed(
    socket: UnixStream,
    mut input: impl Read + Send + 'static,
    mut output: impl Write,
) -> Result<(), String> {
    let mut upstream = socket.try_clone().map_err(|error| error.to_string())?;
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut input, &mut upstream);
        let _ = upstream.shutdown(Shutdown::Write);
    });
    let mut downstream = socket;
    let mut buffer = vec![0; 64 * 1024];
    loop {
        match downstream.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                if output
                    .write_all(&buffer[..count])
                    .and_then(|()| output.flush())
                    .is_err()
                {
                    let _ = downstream.shutdown(Shutdown::Both);
                    return Ok(());
                }
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Ok(()),
        }
    }
}

/// 시작 잠금을 쥔 프로세스가 이 디렉터리의 다른 빌드 daemon일 때만 SIGTERM으로 내리고 끝나기를 기다린다
pub fn stop_previous_build_daemon(
    directory: &Path,
    namespace: &str,
    build_hash: &str,
) -> Result<(), String> {
    let Some(pid) = read_start_lock_owner(directory) else {
        return Ok(());
    };
    let Some(running_hash) = read_serving_build_hash(pid, directory, namespace) else {
        return Ok(());
    };
    if running_hash == build_hash {
        return Ok(());
    }
    // SAFETY: 같은 사용자로 이 디렉터리의 daemon을 돌리고 있음을 확인한 pid에만 보낸다
    if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
        return Ok(());
    }
    let deadline = Instant::now() + PREVIOUS_DAEMON_STOP_TIMEOUT;
    while is_process_alive(pid) {
        if Instant::now() >= deadline {
            return Err(format!(
                "Previous host daemon {pid} did not stop after SIGTERM"
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

fn read_start_lock_owner(directory: &Path) -> Option<i32> {
    let owner_path = directory.join("start.lock").join("pid");
    let owner = fs::symlink_metadata(&owner_path).ok()?;
    if !owner.is_file() || owner.uid() != current_uid() {
        return None;
    }
    fs::read_to_string(owner_path)
        .ok()?
        .trim()
        .parse()
        .ok()
        .filter(|pid| *pid > 1)
}

/// `ps`로 본 명령줄에서 daemon이 돌리는 빌드 hash를 읽는다. 같은 사용자의 KanVibe daemon이 아니면 `None`이다
fn read_serving_build_hash(pid: i32, directory: &Path, namespace: &str) -> Option<String> {
    let output = Command::new("ps")
        .args(["-ww", "-o", "uid=", "-o", "args=", "-p", &pid.to_string()])
        .env_clear()
        .envs(current_child_environment())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.trim();
    let (uid, arguments) = line.split_once(char::is_whitespace)?;
    if uid.trim().parse::<u32>().ok()? != current_uid() {
        return None;
    }
    serving_build_hash(arguments.trim(), &directory.to_string_lossy(), namespace)
}

/// Node daemon(`node <dir>/server-<h>.cjs --serve <ns> <h>`)과 Rust daemon(`<dir>/agent-<h> host-serve <ns> <h>`)을 알아본다
pub fn serving_build_hash(arguments: &str, directory: &str, namespace: &str) -> Option<String> {
    let tokens: Vec<&str> = arguments.split_whitespace().collect();
    tokens.windows(4).find_map(|window| {
        let hash = match window[1] {
            "--serve" => window[0]
                .strip_prefix(&format!("{directory}/server-"))?
                .strip_suffix(".cjs")?,
            "host-serve" => window[0].strip_prefix(&format!("{directory}/agent-"))?,
            _ => return None,
        };
        let is_hash = hash.len() == 64
            && hash
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
        (is_hash && window[2] == namespace && window[3] == hash).then(|| hash.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_node_and_rust_daemons_of_this_directory_only() {
        let hash = "a".repeat(64);
        let node = format!("/usr/bin/node /tmp/kv-1/dev/server-{hash}.cjs --serve dev {hash} /x y");
        let rust = format!("/tmp/kv-1/dev/agent-{hash} host-serve dev {hash}");
        assert_eq!(
            serving_build_hash(&node, "/tmp/kv-1/dev", "dev"),
            Some(hash.clone())
        );
        assert_eq!(
            serving_build_hash(&rust, "/tmp/kv-1/dev", "dev"),
            Some(hash.clone())
        );
        assert_eq!(serving_build_hash(&rust, "/tmp/kv-1/prod", "prod"), None);
        assert_eq!(
            serving_build_hash("/usr/bin/node -e listen", "/tmp/kv-1/dev", "dev"),
            None
        );
    }
}
