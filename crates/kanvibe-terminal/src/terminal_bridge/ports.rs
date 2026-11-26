//! 브리지가 부르는 협력자. TS 테스트가 `@/lib/terminal`, DB, `remoteSessionDependency`, `gitOperations`, hook 설치기,
//! `remoteImagePaste`, `boardNotifier`를 mock하듯 부르는 메서드만 trait으로 두고, 실제 타입은 위임으로 구현한다.

use std::sync::Arc;

use kanvibe_db::Database;
use kanvibe_db::repositories::{projects, tasks};
use kanvibe_domain::task::{KanbanTask, SessionType};
use kanvibe_process::exec::ExecGitError;
use kanvibe_process::exec::system::SystemShellCommandRunner;
use kanvibe_process::hooks::installer::KanvibeHooksInstaller;
use kanvibe_process::remote_session_dependency::SessionDependencies;
use kanvibe_process::ssh_config::SshHostConfig;
use kanvibe_process::worktree::PaneLayoutSelection;

use super::TerminalBridgeError;
use crate::pane_layout_service::get_effective_pane_layout;
use crate::terminal_registry::{
    LocalAttachRequest, PreparedTerminalSession, RemoteAttachRequest, TerminalAttachment,
    TerminalClient, TerminalRegistry, TerminalSessionPreparationTarget,
};

/// 터미널 세션 레지스트리 가운데 브리지가 쓰는 부분(TS `@/lib/terminal`)
pub trait TerminalSessions: Send + Sync {
    fn attach_local_session(
        &self,
        request: LocalAttachRequest,
        client: Arc<dyn TerminalClient>,
    ) -> Result<Option<TerminalAttachment>, String>;
    fn attach_remote_session(
        &self,
        request: RemoteAttachRequest,
        client: Arc<dyn TerminalClient>,
    ) -> Result<Option<TerminalAttachment>, String>;
    fn prepare_terminal_session(
        &self,
        target: TerminalSessionPreparationTarget,
    ) -> Result<PreparedTerminalSession, String>;
    fn focus_session(&self, task_id: &str);
    fn client_message(&self, attachment: &TerminalAttachment, message: &str);
    fn client_closed(&self, attachment: &TerminalAttachment);
}

impl TerminalSessions for Arc<TerminalRegistry> {
    fn attach_local_session(
        &self,
        request: LocalAttachRequest,
        client: Arc<dyn TerminalClient>,
    ) -> Result<Option<TerminalAttachment>, String> {
        TerminalRegistry::attach_local_session(self, request, client)
    }

    fn attach_remote_session(
        &self,
        request: RemoteAttachRequest,
        client: Arc<dyn TerminalClient>,
    ) -> Result<Option<TerminalAttachment>, String> {
        TerminalRegistry::attach_remote_session(self, request, client)
    }

    fn prepare_terminal_session(
        &self,
        target: TerminalSessionPreparationTarget,
    ) -> Result<PreparedTerminalSession, String> {
        TerminalRegistry::prepare_terminal_session(self, target)
    }

    fn focus_session(&self, task_id: &str) {
        TerminalRegistry::focus_session(self, task_id);
    }

    fn client_message(&self, attachment: &TerminalAttachment, message: &str) {
        TerminalRegistry::client_message(self, attachment, message);
    }

    fn client_closed(&self, attachment: &TerminalAttachment) {
        TerminalRegistry::client_closed(self, attachment);
    }
}

/// 브리지가 읽는 task·프로젝트·pane 레이아웃(TS `getTaskRepository`, `getProjectRepository`, `getEffectivePaneLayout`)
pub trait TerminalTaskStore: Send + Sync {
    fn find_task(&self, task_id: &str) -> Result<Option<KanbanTask>, String>;
    fn find_project_repo_path(&self, project_id: &str) -> Result<Option<String>, String>;
    /// 프로젝트 설정이 없으면 전역 기본값이다
    fn effective_pane_layout(
        &self,
        project_id: Option<&str>,
    ) -> Result<Option<PaneLayoutSelection>, String>;
}

impl TerminalTaskStore for Database {
    fn find_task(&self, task_id: &str) -> Result<Option<KanbanTask>, String> {
        tasks::find_task(&self.connection(), task_id).map_err(|error| error.to_string())
    }

    fn find_project_repo_path(&self, project_id: &str) -> Result<Option<String>, String> {
        let project = projects::find_project(&self.connection(), project_id)
            .map_err(|error| error.to_string())?;
        Ok(project.map(|project| project.repo_path))
    }

    fn effective_pane_layout(
        &self,
        project_id: Option<&str>,
    ) -> Result<Option<PaneLayoutSelection>, String> {
        let config =
            get_effective_pane_layout(self, project_id).map_err(|error| error.to_string())?;
        Ok(config.map(|config| PaneLayoutSelection {
            layout_type: config.layout_type,
            panes: config.panes,
        }))
    }
}

/// 원격 세션 도구 확인·설치(TS `ensureRemoteSessionDependency`)
pub trait RemoteSessionDependencyCheck: Send + Sync {
    fn ensure_remote_session_dependency(
        &self,
        session_type: SessionType,
        ssh_host: &str,
    ) -> Result<(), ExecGitError>;
}

impl RemoteSessionDependencyCheck for SessionDependencies {
    fn ensure_remote_session_dependency(
        &self,
        session_type: SessionType,
        ssh_host: &str,
    ) -> Result<(), ExecGitError> {
        self.ensure_remote(session_type, Some(ssh_host))
    }
}

/// 백그라운드 실패로 걸린 SSH 전송 쿨다운 해제(TS `clearRemoteSSHTransportCooldown`)
pub trait SshTransportCooldowns: Send + Sync {
    fn clear_transport_cooldown(&self, ssh_host: &str);
}

impl SshTransportCooldowns for SystemShellCommandRunner {
    fn clear_transport_cooldown(&self, ssh_host: &str) {
        self.remote().clear_transport_cooldown(ssh_host);
    }
}

/// task hook이 현재 버전인지 확인하고 필요하면 다시 설치한다(TS `ensureKanvibeHooksCurrent`).
/// SSH 전송 쿨다운으로 실패하면 오류에 재시도 시각이 실린다
pub trait TerminalHookVerifier: Send + Sync {
    fn ensure_hooks_current(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<bool, TerminalBridgeError>;
}

impl TerminalHookVerifier for KanvibeHooksInstaller {
    fn ensure_hooks_current(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<bool, TerminalBridgeError> {
        Ok(KanvibeHooksInstaller::ensure_hooks_current(
            self,
            target_path,
            task_id,
            ssh_host,
        )?)
    }
}

/// 붙여넣은 이미지를 원격 호스트로 보낸다(TS `remoteImagePaste`). scp 전송은 `kanvibe-services`의
/// `decode_data_url_to_bytes`와 `RemoteImagePaste::transfer_image_to_remote_host`가 맡으므로 앱이 위임으로 구현한다
pub trait RemoteImageTransfer: Send + Sync {
    fn decode_data_url(&self, image_data_url: &str) -> Result<Vec<u8>, String>;
    /// 성공하면 원격 경로를 돌려준다. 실패 메시지는 사용자에게 그대로 보인다
    fn transfer_image(
        &self,
        ssh_config: &SshHostConfig,
        image_bytes: &[u8],
    ) -> Result<String, String>;
}

/// hook 설치 실패 보드 알림(TS `broadcastTaskHookInstallFailed` payload)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskHookInstallFailure {
    pub task_id: String,
    pub task_title: String,
    pub error: String,
}

/// hook 확인 실패 로그(TS `console.error("터미널 hooks 확인 실패:", ...)`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalHookCheckFailure {
    pub task_id: String,
    pub task_title: String,
    pub target_path: Option<String>,
    pub ssh_host: Option<String>,
    pub error: String,
}

/// 원격 연결 단계별 걸린 시간(ms). "연결 중"이 길 때 어디서 걸렸는지 가린다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteConnectionPhases {
    pub task_id: String,
    pub ssh_host: String,
    pub session_type: SessionType,
    pub total_ms: u64,
    pub dependency_check_ms: u64,
    /// 초기 pane 명령이 있어 hook을 attach 전에 확인했을 때만 있다
    pub hooks_before_attach_ms: Option<u64>,
    pub session_attach_ms: u64,
}

/// 브리지가 내보내는 알림과 진단 로그. 앱은 보드 알림만 보드 notifier로 잇는다
pub trait TerminalBridgeEvents: Send + Sync {
    fn task_hook_install_failed(&self, failure: &TaskHookInstallFailure);

    fn terminal_hook_check_failed(&self, failure: &TerminalHookCheckFailure) {
        eprintln!("터미널 hooks 확인 실패: {failure:?}");
    }

    fn remote_connection_phases(&self, phases: &RemoteConnectionPhases) {
        println!("[터미널] 원격 연결 단계 {phases:?}");
    }
}
