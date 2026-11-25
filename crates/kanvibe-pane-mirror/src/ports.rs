//! 미러가 기대는 바깥 기능. 앱은 [`crate::adapters`]의 구현을 넣고, 테스트는 가짜를 넣는다.
//!
//! 명령 실행(`ShellCommandRunner`), 터미널 레지스트리, 세션 의존성 확인, zellij 버전 캐시, 보드 알림은
//! 이미 구체 타입이 있어 그대로 받는다. 여기에는 미러가 쓰는 부분만 좁혀야 하는 협력자만 둔다.

use std::sync::Arc;

use kanvibe_domain::task::{KanbanTask, SessionType};
use kanvibe_terminal::herdr::{HerdrMirrorPaneOptions, HerdrPaneExtent, HerdrPaneSize};
use kanvibe_terminal::pane_mirror::MirrorPane;
use kanvibe_terminal::remote_terminal_protocol::{RemoteTerminalGrant, RemoteTerminalGrantInput};
use kanvibe_terminal::terminal_registry::{TerminalAttachment, TerminalClient};

use crate::process::MirrorProcess;

/// 태스크 조회와 세션 연결(TS `getTaskRepository().findOneBy`, `connectTerminalSession`, `getDefaultSessionType`)
pub trait MirrorTaskStore: Send + Sync {
    fn find_task(&self, task_id: &str) -> Result<Option<KanbanTask>, String>;
    fn default_session_type(&self) -> Result<SessionType, String>;
    /// 세션이 없는 태스크에 세션을 붙인다. 붙이지 못하면 `Ok(None)`이다
    fn connect_terminal_session(
        &self,
        task_id: &str,
        session_type: SessionType,
    ) -> Result<Option<KanbanTask>, String>;
}

/// Herdr 조회 가운데 미러가 쓰는 부분(TS `@/lib/herdr`)
pub trait HerdrMirrorQueries: Send + Sync {
    fn list_mirror_panes(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
        options: &HerdrMirrorPaneOptions,
    ) -> Result<Vec<MirrorPane>, String>;
    fn read_pane_extent(
        &self,
        session_name: &str,
        pane_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<HerdrPaneExtent, String>;
    fn read_task_pane_size(
        &self,
        session_name: &str,
        pane_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<Option<HerdrPaneSize>, String>;
    fn is_task_alive(&self, session_name: &str, ssh_host: Option<&str>) -> bool;
}

/// 데스크탑이 태스크를 열 때와 같은 길로 세션에 붙는다(TS `attachTaskTerminal`).
/// 준비 중 클라이언트가 닫히면 `Ok(None)`이다
pub trait TaskTerminalAttacher: Send + Sync {
    fn attach_task_terminal(
        &self,
        task: &KanbanTask,
        tab_id: Option<&str>,
        client: Arc<dyn TerminalClient>,
        columns: Option<u16>,
        rows: Option<u16>,
    ) -> Result<Option<TerminalAttachment>, String>;
}

/// 탭 바의 새 탭 만들기(TS `terminalTabService.createTerminalTab`). 실패하면 사용자에게 보일 메시지를 돌려준다
pub trait MobileTerminalTabs: Send + Sync {
    fn create_terminal_tab(&self, task_id: &str) -> Result<(), String>;
}

/// 미러 스트림을 띄우는 길. 스트림은 오래 살기 때문에 동시 실행 수를 제한하는 `exec_git`의 원격 경로를 쓰지 않는다
pub trait MirrorProcessSpawner: Send + Sync {
    /// `command`를 로컬 `sh -lc`나 `ssh <host> sh -lc`로 띄운다
    fn spawn(&self, command: &str, ssh_host: Option<&str>) -> Result<MirrorProcess, String>;
}

/// 받은 출력을 넘기는 콜백
pub type RelayOutput = Arc<dyn Fn(String) + Send + Sync>;
/// 원격에서 닫은 연결의 WebSocket 닫힘 코드와 사유를 넘기는 콜백
pub type RelayClose = Arc<dyn Fn(u16, String) + Send + Sync>;

/// 원격 Herdr pane의 직접 연결 권한(TS `remoteTerminalService`)
pub trait RemotePaneGrants: Send + Sync {
    fn prepare_grant(
        &self,
        input: &RemoteTerminalGrantInput,
    ) -> Result<Option<RemoteTerminalGrant>, String>;
    fn attach_relay(
        &self,
        grant_id: &str,
        device_id: &str,
        task_id: &str,
        pane_id: &str,
        on_output: RelayOutput,
        on_close: RelayClose,
    ) -> Result<Box<dyn RemotePaneRelay>, String>;
    fn revoke_grant(&self, grant_id: &str);
    fn revoke_device(&self, device_id: &str);
    fn revoke_task(&self, task_id: &str);
    fn stop_agents(&self);
}

/// 데스크탑이 중계하는 원격 pane 연결 하나
pub trait RemotePaneRelay: Send + Sync {
    fn write(&self, input: &[u8], is_binary: bool);
    /// 데스크탑 쪽에서 닫는다. 닫힘 콜백은 부르지 않는다
    fn close(&self);
}
