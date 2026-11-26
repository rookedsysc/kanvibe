//! 시스템 `ssh`로 원격 loader를 실행하는 통로와 `ssh -G` 실효 설정 조회(TS `startSSHHost`, `getRemoteHostEndpointKey`).
//!
//! daemon 연결은 공유 ControlMaster를 타지 않고 `StrictHostKeyChecking=yes`로 따로 연결한다. 자식에는 서버 런타임 값을
//! 걷어 낸 로컬 셸 환경만 넘긴다(CLAUDE.md 런타임 환경 규칙).

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kanvibe_process::shell_environment::ShellEnvironment;
use kanvibe_process::ssh_config::{
    SshArgsOptions, SshHostConfig, build_ssh_args, kanvibe_ssh_connection_health_options,
};

use crate::connection::{HostTransport, HostTransportSpawner};

const STDERR_TAIL_CHARACTERS: usize = 4096;

/// daemon 연결용 ssh 인자. TTY 없이, 호스트 키를 엄격히 확인하고, 연결 상태 확인을 켠다
pub fn build_host_ssh_args(config: &SshHostConfig) -> Vec<String> {
    build_ssh_args(
        config,
        &SshArgsOptions {
            is_tty_disabled: true,
            should_check_host_key_strictly: true,
            connection_health: Some(kanvibe_ssh_connection_health_options()),
            ..SshArgsOptions::default()
        },
    )
}

pub struct SystemHostTransportSpawner {
    environment: ShellEnvironment,
}

impl SystemHostTransportSpawner {
    /// `environment`는 `create_local_shell_environment`로 좁힌 값이어야 한다
    pub fn new(environment: ShellEnvironment) -> Self {
        Self { environment }
    }
}

impl HostTransportSpawner for SystemHostTransportSpawner {
    fn spawn(&self, config: &SshHostConfig, remote_command: &str) -> Result<HostTransport, String> {
        let mut child = Command::new("ssh")
            .args(build_host_ssh_args(config))
            .arg(remote_command)
            .env_clear()
            .envs(&self.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| error.to_string())?;
        let writer = child.stdin.take().expect("stdin을 파이프로 열었다");
        let reader = child.stdout.take().expect("stdout을 파이프로 열었다");
        let mut stderr = child.stderr.take().expect("stderr를 파이프로 열었다");
        let stderr_tail = Arc::new(Mutex::new(String::new()));
        let tail = Arc::clone(&stderr_tail);
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(count @ 1..) = stderr.read(&mut buffer) {
                let mut tail = tail.lock().unwrap();
                tail.push_str(&String::from_utf8_lossy(&buffer[..count]));
                let excess = tail.chars().count().saturating_sub(STDERR_TAIL_CHARACTERS);
                *tail = tail.chars().skip(excess).collect();
            }
        });
        Ok(HostTransport {
            writer: Box::new(writer),
            reader: Box::new(reader),
            terminate: terminate_on_request(child),
            stderr_tail,
        })
    }
}

/// 끝난 자식은 회수하고, 아직 살아 있으면 SIGTERM을 보내는 종료 함수
fn terminate_on_request(mut child: std::process::Child) -> Box<dyn Fn() + Send + Sync> {
    let process_id = child.id() as libc::pid_t;
    let has_exited = Arc::new(AtomicBool::new(false));
    let exited = Arc::clone(&has_exited);
    std::thread::spawn(move || {
        let _ = child.wait();
        exited.store(true, Ordering::SeqCst);
    });
    Box::new(move || {
        if !has_exited.load(Ordering::SeqCst) {
            // SAFETY: 아직 회수하지 않은 ssh 자식의 pid에만 보낸다
            unsafe { libc::kill(process_id, libc::SIGTERM) };
        }
    })
}

/// `ssh -G`의 실효 설정 출력
pub trait EffectiveSshConfig: Send + Sync {
    fn resolve(&self, ssh_arguments: &[String]) -> Result<String, String>;
}

pub struct SystemEffectiveSshConfig {
    environment: ShellEnvironment,
}

impl SystemEffectiveSshConfig {
    pub fn new(environment: ShellEnvironment) -> Self {
        Self { environment }
    }
}

impl EffectiveSshConfig for SystemEffectiveSshConfig {
    fn resolve(&self, ssh_arguments: &[String]) -> Result<String, String> {
        let mut command = Command::new("ssh");
        command
            .arg("-G")
            .args(ssh_arguments)
            .env_clear()
            .envs(&self.environment);
        bounded_command_stdout(command, EFFECTIVE_CONFIG_TIMEOUT)
    }
}

/// `ssh -G` 시간·출력 상한(TS `execFileAsync(..., { timeout: 10_000, maxBuffer: 1024 * 1024 })`).
/// `~/.ssh/config`의 `Match exec`가 끝나지 않아도 원격 연결 준비가 그 자리에서 멈추지 않게 한다
const EFFECTIVE_CONFIG_TIMEOUT: Duration = Duration::from_secs(10);
const EFFECTIVE_CONFIG_MAX_OUTPUT_BYTES: usize = 1024 * 1024;

/// 명령을 실행해 stdout을 돌려준다. 시간 안에 끝나지 않으면 죽이고 실패하며, 출력이 상한을 넘어도 실패한다
fn bounded_command_stdout(mut command: Command, timeout: Duration) -> Result<String, String> {
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let process_id = child.id() as libc::pid_t;
    let (sender, outputs) = channel();
    std::thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    let output = match outputs.recv_timeout(timeout) {
        Ok(output) => output.map_err(|error| error.to_string())?,
        Err(_) => {
            // SAFETY: 기다리는 스레드가 아직 회수하지 않은 자식 pid에만 보낸다
            unsafe { libc::kill(process_id, libc::SIGKILL) };
            return Err(format!(
                "ssh -G timed out after {} seconds",
                timeout.as_secs()
            ));
        }
    };
    if output.stdout.len() > EFFECTIVE_CONFIG_MAX_OUTPUT_BYTES
        || output.stderr.len() > EFFECTIVE_CONFIG_MAX_OUTPUT_BYTES
    {
        return Err("ssh -G maxBuffer length exceeded".into());
    }
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// 같은 원격 계정에 붙는 별칭이 같은 값을 갖는 식별자. `%n`·`%k`가 남은 설정은 별칭마다 접속 대상이 다를 수 있어 나눈다
pub fn endpoint_key_from_effective_config(config: &SshHostConfig, effective: &str) -> String {
    let effective: String = effective
        .split('\n')
        .filter(|line| !line.starts_with("host "))
        .collect::<Vec<_>>()
        .join("\n");
    let identity = if effective.contains("%n") || effective.contains("%k") {
        serde_json::to_string(&[config.host.as_str(), effective.as_str()]).expect("문자열 배열")
    } else {
        effective
    };
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(identity.as_bytes()))
}

/// `ssh -G`로 쓰는 인자. 연결 상태 옵션 없이 TTY 끔·엄격 확인만 켠다(TS와 같다)
pub fn build_endpoint_probe_args(config: &SshHostConfig) -> Vec<String> {
    build_ssh_args(
        config,
        &SshArgsOptions {
            is_tty_disabled: true,
            should_check_host_key_strictly: true,
            ..SshArgsOptions::default()
        },
    )
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[test]
    fn a_command_that_does_not_finish_in_time_is_killed_and_fails() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let started = Instant::now();

        let error = bounded_command_stdout(command, Duration::from_millis(200)).unwrap_err();

        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn output_over_the_limit_fails_and_ordinary_output_is_returned() {
        let mut oversized = Command::new("head");
        oversized.args(["-c", "1048577", "/dev/zero"]);
        let error = bounded_command_stdout(oversized, Duration::from_secs(10)).unwrap_err();
        assert!(error.contains("maxBuffer"), "{error}");

        let mut ordinary = Command::new("printf");
        ordinary.arg("user fixture\\nhost route-a\\n");
        assert_eq!(
            bounded_command_stdout(ordinary, Duration::from_secs(10)).unwrap(),
            "user fixture\nhost route-a\n"
        );
    }
}
