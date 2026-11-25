//! `KanbanService`에 넘기는 협력 객체 중 아직 앱 조립 지점이 없는 것. 앱 조립 지점이 생기면 그쪽 구현으로 바꿀 후보다.
//!
//! - 터미널: 이 앱에는 아직 task PTY 레지스트리가 없어 끊을 PTY가 없다. Herdr 공유 서버 workspace 정리·생존 확인은
//!   `kanvibe-terminal`의 `HerdrService`에 맡긴다.
//! - hook: hook 서버(kanvibe-server)가 이 앱에서 아직 뜨지 않아, 설치를 시도하지 않고 사유를 돌려준다.
//!   서비스는 이 실패를 로그·보드 이벤트로만 남기고 task 생성은 계속한다(TS도 hook 실패로 생성을 되돌리지 않는다).
//! - 다른 client 전파: hook 서버 주소가 없으므로 전파할 대상이 없다.

use std::sync::Arc;

use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::ssh_config::SshConfigReader;
use kanvibe_process::task_target_propagation::PeerTargetResult;
use kanvibe_process::worktree::HerdrTaskWorkspaces;
use kanvibe_services::kanban::ports::{
    HookVerificationFailure, HookVerificationSuccess, TaskHookInstallation, TaskPeerPropagation,
};
use kanvibe_services::terminal_ports::TaskTerminalSessions;
use kanvibe_terminal::environment::TerminalHostEnvironment;
use kanvibe_terminal::herdr::HerdrService;

const HOOK_SERVER_UNAVAILABLE: &str = "hook server is not running in this build";

pub struct BoardTerminalSessions {
    herdr: HerdrService,
}

impl BoardTerminalSessions {
    pub fn new(files: Arc<HostFileAccess>, home_directory: &str) -> Self {
        Self {
            herdr: HerdrService::new(
                files,
                Arc::new(TerminalHostEnvironment::for_current_process()),
                Arc::new(SshConfigReader::new(home_directory)),
            ),
        }
    }
}

impl HerdrTaskWorkspaces for BoardTerminalSessions {
    fn close_task_workspace(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), String> {
        self.herdr.close_task_workspace(session_name, ssh_host)
    }

    fn is_task_alive(&self, session_name: &str, ssh_host: Option<&str>) -> bool {
        self.herdr.is_task_alive(session_name, ssh_host)
    }
}

impl TaskTerminalSessions for BoardTerminalSessions {
    /// 보드 화면은 PTY를 열지 않으므로 끊을 것이 없다
    fn detach_session(&self, _task_id: &str, _trigger_label: &str) {}

    fn replace_herdr_remote_server(
        &self,
        _ssh_host: &str,
        _herdr_session: &str,
    ) -> Result<(), String> {
        Err(HOOK_SERVER_UNAVAILABLE.to_string())
    }
}

pub struct BoardHookInstallation;

impl TaskHookInstallation for BoardHookInstallation {
    fn install_hook_files(
        &self,
        _target_path: &str,
        _task_id: &str,
        _ssh_host: Option<&str>,
    ) -> Result<(), String> {
        Err(HOOK_SERVER_UNAVAILABLE.to_string())
    }

    fn schedule_hooks_verification(
        &self,
        _target_path: &str,
        _task_id: &str,
        _ssh_host: Option<&str>,
        _on_success: HookVerificationSuccess,
        on_failure: HookVerificationFailure,
    ) {
        on_failure(HOOK_SERVER_UNAVAILABLE.to_string());
    }
}

pub struct BoardPeerPropagation;

impl TaskPeerPropagation for BoardPeerPropagation {
    fn propagate_task_deletion_to_peers(
        &self,
        _worktree_path: &str,
        _ssh_host: Option<&str>,
    ) -> Vec<PeerTargetResult> {
        Vec::new()
    }
}
