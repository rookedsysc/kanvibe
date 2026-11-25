//! Herdr 멀티플렉서 연동(TS `src/lib/herdr.ts`). 공유 서버 `kanvibe`의 Space 매핑, 레이아웃, 원격 machine 등록,
//! 서버 live-handoff, 탭·pane 조회를 맡는다.
//!
//! TS 모듈 전역 상태(진행 중인 부트스트랩, Space 연결 파일 경로와 최근 읽기, machine 등록 순서)는
//! 앱이 한 번 만드는 [`HerdrService`]의 필드다. 모든 셸 명령은 `ShellCommandRunner::exec_git`을 거친다.

mod bootstrap;
mod cli;
mod client_config;
mod command;
mod panes;
mod registry_adapter;
mod remote_machine;
mod response;
mod running_agents;
mod shared_outcome;
mod tab_move;
#[cfg(test)]
mod tests;
mod workspace_mapping;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kanvibe_process::exec::{ExecGitError, ShellCommandRunner, is_ssh_connection_error};
use kanvibe_process::host_file_access::{HostFileAccess, HostFileError};
use kanvibe_process::ssh_config::SshConfigReader;
use kanvibe_process::worktree::HerdrTaskWorkspaces;

use crate::environment::TerminalHostEnvironment;

pub use client_config::HerdrTabBarPosition;
pub use command::{
    HerdrSessionTarget, build_herdr_client_command, build_herdr_command,
    build_herdr_session_cleanup_command, parse_herdr_session_target,
};
pub use panes::{
    HerdrAgentSession, HerdrMirrorPaneOptions, HerdrPaneExtent, HerdrPaneSize, HerdrTaskPaneTab,
};

const HERDR_COMMAND_TIMEOUT: Duration = Duration::from_millis(5_000);
const HERDR_START_TIMEOUT: Duration = Duration::from_millis(30_000);

/// Herdr 연동 실패. TS가 던지던 오류 문자열을 그대로 담고, 호출자가 분기해야 하는 실패만 따로 나눈다
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HerdrError {
    /// TS `new Error(...)`로 던지던 메시지
    #[error("{0}")]
    Message(String),
    /// 셸 명령 실패. SSH 접속 실패 판정에 원래 오류가 필요해 그대로 둔다
    #[error(transparent)]
    Command(#[from] ExecGitError),
    /// TS `HerdrServerHandoffRequiredError`. 원격 서버가 이전 KanVibe 방식으로 떠 있어 machine 등록 전에 서버를 교체해야 한다
    #[error("{ssh_host}의 Herdr 서버({herdr_session})를 교체해야 machine으로 등록할 수 있습니다.")]
    ServerHandoffRequired {
        ssh_host: String,
        herdr_session: String,
    },
}

impl HerdrError {
    /// TS `isSSHConnectionError`. 원격에 닿지 못한 실패만 참이다
    pub fn is_ssh_connection_error(&self) -> bool {
        match self {
            Self::Command(error) => is_ssh_connection_error(error),
            Self::Message(message) => is_ssh_connection_error(&ExecGitError::new(message.clone())),
            Self::ServerHandoffRequired { .. } => false,
        }
    }
}

impl From<HostFileError> for HerdrError {
    fn from(error: HostFileError) -> Self {
        match error {
            HostFileError::Exec(error) => Self::Command(error),
            other => Self::Message(other.to_string()),
        }
    }
}

/// 시작 대기 마감과 Space 연결 파일 재사용 시간을 재는 단조 시계. 테스트는 시간을 앞당길 수 있는 시계로 바꾼다
type MonotonicClock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// Herdr 연동 서비스. 앱이 한 번 만들어 터미널 레지스트리, 탭 서비스, pane chat, 원격 터미널 서비스가 함께 쓴다
pub struct HerdrService {
    runner: Arc<dyn ShellCommandRunner>,
    files: Arc<HostFileAccess>,
    environment: Arc<TerminalHostEnvironment>,
    ssh_config: Arc<SshConfigReader>,
    bootstraps: Mutex<bootstrap::BootstrapFlights>,
    /// 호스트(로컬은 빈 문자열)마다 공유 서버의 Space 연결 파일 경로
    workspace_mapping_paths: Mutex<HashMap<String, String>>,
    workspace_mapping_reads: Mutex<HashMap<String, workspace_mapping::WorkspaceMappingRead>>,
    /// machine 목록 확인과 등록은 호스트와 상관없이 하나씩 실행한다. Herdr가 catalog 파일 하나에 저장하기 때문이다
    remote_machine_registration: Mutex<()>,
    clock: MonotonicClock,
    /// 로컬 서버를 띄우는 POSIX 셸
    posix_shell_path: String,
}

impl HerdrService {
    /// 명령 실행기는 `files`가 쓰는 것과 같은 실행기를 쓴다
    pub fn new(
        files: Arc<HostFileAccess>,
        environment: Arc<TerminalHostEnvironment>,
        ssh_config: Arc<SshConfigReader>,
    ) -> Self {
        Self {
            runner: files.shared_runner(),
            files,
            environment,
            ssh_config,
            bootstraps: Mutex::new(bootstrap::BootstrapFlights::default()),
            workspace_mapping_paths: Mutex::new(HashMap::new()),
            workspace_mapping_reads: Mutex::new(HashMap::new()),
            remote_machine_registration: Mutex::new(()),
            clock: Arc::new(Instant::now),
            posix_shell_path: "/bin/sh".to_string(),
        }
    }

    fn now(&self) -> Instant {
        (self.clock)()
    }
}

impl HerdrTaskWorkspaces for HerdrService {
    fn close_task_workspace(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), String> {
        self.close_herdr_task_workspace(session_name, ssh_host)
            .map_err(|error| error.to_string())
    }

    fn is_task_alive(&self, session_name: &str, ssh_host: Option<&str>) -> bool {
        self.is_herdr_task_alive(session_name, ssh_host)
    }
}

fn parse_target(session_name: &str) -> Result<HerdrSessionTarget, HerdrError> {
    parse_herdr_session_target(session_name).map_err(HerdrError::Message)
}

fn herdr_command(session_name: &str, arguments: &[&str]) -> Result<String, HerdrError> {
    build_herdr_command(session_name, arguments).map_err(HerdrError::Message)
}
