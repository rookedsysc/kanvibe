//! 미러 자식 프로세스(tmux 파이프 리더, Herdr observe·control). TS `spawnMirrorProcess`와 `stopMirrorProcess`.
//!
//! TS는 자식에게 서버 프로세스의 환경을 통째로 물려줬다(인벤토리 §6, AD-11). 여기서는 `create_local_shell_environment`로
//! 좁힌 환경만 넘겨 `PORT`, `HOST`, `NODE_ENV`, `KANVIBE_*`가 자식에 닿지 않게 한다.
//! 자식은 자기 프로세스 그룹으로 띄운다. 종료할 때 그룹 전체에 신호를 보내야 `sh` 아래의 `tail`까지 함께 정리된다.
//! stderr는 처음부터 버린다. 읽지 않는 파이프는 64KB에서 차고, 그때부터 자식이 쓰기에서 멈춰 스트림째 선다.

use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use kanvibe_process::ssh_config::{
    SshArgsOptions, SshConfigReader, build_ssh_args, kanvibe_ssh_connection_health_options,
};
use kanvibe_process::worktree::quote_for_posix_shell;
use kanvibe_terminal::environment::TerminalHostEnvironment;

use crate::ports::MirrorProcessSpawner;

/// 띄운 미러 프로세스. stdout은 원문 바이트이고, stdin은 Herdr control처럼 입력을 받는 명령만 쓴다
pub struct MirrorProcess {
    pub stdout: Box<dyn Read + Send>,
    pub stdin: Option<Box<dyn Write + Send>>,
    pub control: Arc<dyn MirrorProcessControl>,
}

pub trait MirrorProcessControl: Send + Sync {
    /// 프로세스 그룹 전체에 SIGTERM을 보낸다. 이미 끝났으면 아무것도 하지 않는다
    fn terminate(&self);
    /// 프로세스가 끝날 때까지 기다린다
    fn wait_for_exit(&self);
}

/// 시스템 `sh`·`ssh`로 띄운다. 자식에는 좁힌 로컬 셸 환경만 넘긴다
pub struct SystemMirrorProcessSpawner {
    environment: Arc<TerminalHostEnvironment>,
    ssh_hosts: Arc<SshConfigReader>,
}

impl SystemMirrorProcessSpawner {
    pub fn new(environment: Arc<TerminalHostEnvironment>, ssh_hosts: Arc<SshConfigReader>) -> Self {
        Self {
            environment,
            ssh_hosts,
        }
    }

    fn build_arguments(
        &self,
        command: &str,
        ssh_host: Option<&str>,
    ) -> Result<(&'static str, Vec<String>), String> {
        let Some(ssh_host) = ssh_host else {
            return Ok(("sh", vec!["-lc".to_string(), command.to_string()]));
        };
        let host_config = self
            .ssh_hosts
            .parse_ssh_config()
            .into_iter()
            .find(|config| config.host == ssh_host)
            .ok_or_else(|| format!("SSH 호스트를 찾을 수 없습니다: {ssh_host}"))?;
        let options = SshArgsOptions {
            connection_health: Some(kanvibe_ssh_connection_health_options()),
            ..SshArgsOptions::default()
        };
        let mut arguments = build_ssh_args(&host_config, &options);
        arguments.push(format!("sh -lc {}", quote_for_posix_shell(command)));
        Ok(("ssh", arguments))
    }
}

impl MirrorProcessSpawner for SystemMirrorProcessSpawner {
    fn spawn(&self, command: &str, ssh_host: Option<&str>) -> Result<MirrorProcess, String> {
        let (program, arguments) = self.build_arguments(command, ssh_host)?;
        let mut child = Command::new(program)
            .args(arguments)
            .env_clear()
            .envs(&self.environment.shell_environment)
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        let stdin = child.stdin.take().expect("stdin을 파이프로 열었다");
        let stdout = child.stdout.take().expect("stdout을 파이프로 열었다");
        let control = Arc::new(SystemProcessControl {
            process_id: child.id(),
            exit: Arc::new((Mutex::new(false), Condvar::new())),
        });
        let exit = Arc::clone(&control.exit);
        // 끝난 자식을 회수해 좀비로 남기지 않는다. 회수한 뒤에는 같은 pid에 신호를 보내지 않는다
        std::thread::spawn(move || {
            let _ = child.wait();
            *exit.0.lock().unwrap() = true;
            exit.1.notify_all();
        });
        Ok(MirrorProcess {
            stdout: Box::new(stdout),
            stdin: Some(Box::new(stdin)),
            control,
        })
    }
}

type ExitSignal = Arc<(Mutex<bool>, Condvar)>;

struct SystemProcessControl {
    process_id: u32,
    exit: ExitSignal,
}

impl MirrorProcessControl for SystemProcessControl {
    fn terminate(&self) {
        let has_exited = self.exit.0.lock().unwrap();
        if *has_exited {
            return;
        }
        let process_id = self.process_id as libc::pid_t;
        // SAFETY: 아직 회수하지 않은 자식의 pid와 그 프로세스 그룹에만 신호를 보낸다. 회수 표시를 쥔 채 보내 회수와 겹치지 않는다
        unsafe {
            if libc::kill(-process_id, libc::SIGTERM) != 0 {
                libc::kill(process_id, libc::SIGTERM);
            }
        }
    }

    fn wait_for_exit(&self) {
        let mut has_exited = self.exit.0.lock().unwrap();
        while !*has_exited {
            has_exited = self.exit.1.wait(has_exited).unwrap();
        }
    }
}

/// 출력을 읽는 스레드가 붙은 미러 프로세스. 출력이 끝나고 프로세스가 끝나면 종료 콜백을 한 번 부른다
pub(crate) struct WatchedProcess {
    control: Arc<dyn MirrorProcessControl>,
    stdin: Mutex<Option<Box<dyn Write + Send>>>,
    exit: ExitSignal,
}

impl WatchedProcess {
    pub fn start(
        process: MirrorProcess,
        mut on_output: impl FnMut(&[u8]) + Send + 'static,
        on_exit: impl FnOnce() + Send + 'static,
    ) -> Arc<Self> {
        let watched = Arc::new(Self {
            control: Arc::clone(&process.control),
            stdin: Mutex::new(process.stdin),
            exit: Arc::new((Mutex::new(false), Condvar::new())),
        });
        let control = process.control;
        let exit = Arc::clone(&watched.exit);
        let mut stdout = process.stdout;
        std::thread::spawn(move || {
            let mut buffer = [0_u8; 16 * 1024];
            loop {
                match stdout.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => on_output(&buffer[..count]),
                }
            }
            control.wait_for_exit();
            *exit.0.lock().unwrap() = true;
            exit.1.notify_all();
            on_exit();
        });
        watched
    }

    pub fn stop(&self) {
        self.control.terminate();
    }

    pub fn write_line(&self, line: &str) -> Result<(), String> {
        let mut stdin = self.stdin.lock().unwrap();
        let writer = stdin
            .as_mut()
            .ok_or_else(|| "Herdr 터미널 제어 입력이 닫혔습니다.".to_string())?;
        writer
            .write_all(format!("{line}\n").as_bytes())
            .and_then(|()| writer.flush())
            .map_err(|error| error.to_string())
    }

    /// 마지막 줄을 쓰고 입력을 닫는다(TS `stdin.end(line)`)
    pub fn finish_input(&self, line: &str) -> Result<(), String> {
        let result = self.write_line(line);
        self.stdin.lock().unwrap().take();
        result
    }

    /// `timeout` 안에 끝나면 참이다
    pub fn wait_for_exit(&self, timeout: Duration) -> bool {
        let has_exited = self.exit.0.lock().unwrap();
        let (has_exited, _) = self
            .exit
            .1
            .wait_timeout_while(has_exited, timeout, |has_exited| !*has_exited)
            .unwrap();
        *has_exited
    }
}

/// NDJSON 출력을 줄로 자른다. 조각 경계에서 잘린 줄은 다음 조각과 잇는다
#[derive(Default)]
pub(crate) struct LineSplitter {
    partial_line: Vec<u8>,
}

impl LineSplitter {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.partial_line.extend_from_slice(bytes);
        let Some(last_newline) = self.partial_line.iter().rposition(|byte| *byte == b'\n') else {
            return Vec::new();
        };
        let complete: Vec<u8> = self.partial_line.drain(..=last_newline).collect();
        complete[..complete.len() - 1]
            .split(|byte| *byte == b'\n')
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect()
    }

    /// 지금까지 모은 잘린 줄의 바이트 수
    pub fn pending_bytes(&self) -> usize {
        self.partial_line.len()
    }
}

/// 원문 바이트 출력을 문자열로 푼다. 조각 끝에서 잘린 UTF-8 글자는 다음 조각과 이어 푼다(TS `setEncoding("utf8")`)
#[derive(Default)]
pub(crate) struct Utf8ChunkDecoder {
    pending: Vec<u8>,
}

impl Utf8ChunkDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let complete_length = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(_) => self.pending.len(),
        };
        let complete: Vec<u8> = self.pending.drain(..complete_length).collect();
        String::from_utf8_lossy(&complete).into_owned()
    }
}
