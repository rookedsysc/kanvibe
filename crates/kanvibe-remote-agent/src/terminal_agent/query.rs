//! 읽기 전용 조회(기본 크기, 화면 이력)를 자기 프로세스 그룹에서 돌린다(TS `snapshot`).
//!
//! 복합 셸만 죽이면 그 안의 RPC가 살아남으므로, 시간 초과·크기 초과·agent 정지 때 그룹 전체를 SIGKILL한다.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::Agent;
use super::state::AgentState;
use super::wire::{REMOTE_TERMINAL_COMMAND_TIMEOUT_MS, REMOTE_TERMINAL_FRAME_BYTES};
use crate::environment::current_child_environment;

const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

fn kill_group(process_group: i32) {
    // SAFETY: 이 agent가 만든 조회 프로세스 그룹에만 보낸다
    unsafe { libc::kill(-process_group, libc::SIGKILL) };
}

/// agent가 멈춘다. 떠 있는 조회를 모두 끝낸다
pub(super) fn kill_all(state: &mut AgentState) {
    for process_group in state.queries.drain(..) {
        kill_group(process_group);
    }
}

impl Agent {
    /// 명령을 돌려 stdout을 돌려준다. 0이 아닌 종료, 5초 초과, 8 MiB 초과는 실패다
    pub(super) fn run_query(&self, command: &str) -> Result<String, String> {
        let mut child = Command::new("sh")
            .args(["-lc", command])
            .env_clear()
            .envs(current_child_environment())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|error| error.to_string())?;
        let process_group = child.id() as i32;
        {
            let mut state = self.lock();
            if state.is_stopped {
                kill_group(process_group);
            }
            state.queries.push(process_group);
        }
        let mut stdout = child.stdout.take().expect("stdout을 파이프로 열었다");
        let reader = std::thread::spawn(move || {
            let mut output = Vec::new();
            let limit = REMOTE_TERMINAL_FRAME_BYTES as u64 + 1;
            let _ = stdout.by_ref().take(limit).read_to_end(&mut output);
            output
        });
        let deadline = Instant::now() + Duration::from_millis(REMOTE_TERMINAL_COMMAND_TIMEOUT_MS);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() < deadline => std::thread::sleep(EXIT_POLL_INTERVAL),
                Ok(None) | Err(_) => {
                    kill_group(process_group);
                    let _ = child.wait();
                    break None;
                }
            }
        };
        kill_group(process_group);
        self.lock().queries.retain(|group| *group != process_group);
        let output = reader.join().unwrap_or_default();
        if output.len() > REMOTE_TERMINAL_FRAME_BYTES {
            return Err("Terminal snapshot too large".into());
        }
        match status {
            Some(status) if status.success() => Ok(String::from_utf8_lossy(&output).into_owned()),
            Some(_) => Err("Terminal snapshot unavailable".into()),
            None => Err("Terminal snapshot timed out".into()),
        }
    }
}
