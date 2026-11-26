//! 앱 서비스에 미러 경계를 붙이는 구현. 구성 지점은 이것들로 [`crate::PaneMirrorParts`]를 채운다.

use std::sync::Arc;

use kanvibe_db::Database;
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::task::{KanbanTask, SessionType};
use kanvibe_services::kanban::KanbanService;
use kanvibe_terminal::herdr::{
    HerdrMirrorPaneOptions, HerdrPaneExtent, HerdrPaneSize, HerdrService,
};
use kanvibe_terminal::pane_mirror::MirrorPane;
use kanvibe_terminal::remote_terminal_protocol::{RemoteTerminalGrant, RemoteTerminalGrantInput};
use kanvibe_terminal::remote_terminal_service::{RemoteTerminalRelay, RemoteTerminalService};
use kanvibe_terminal::terminal_bridge::TerminalBridge;
use kanvibe_terminal::terminal_registry::{TerminalAttachment, TerminalClient};
use kanvibe_terminal::terminal_tab_service::TerminalTabService;

use crate::ports::{
    HerdrMirrorQueries, MirrorTaskStore, MobileTerminalTabs, RelayClose, RelayOutput,
    RemotePaneGrants, RemotePaneRelay, TaskTerminalAttacher,
};

/// 보드 서비스와 앱 설정으로 태스크를 찾고 세션을 붙인다
pub struct KanbanMirrorTaskStore {
    kanban: Arc<KanbanService>,
    database: Arc<Database>,
}

impl KanbanMirrorTaskStore {
    pub fn new(kanban: Arc<KanbanService>, database: Arc<Database>) -> Self {
        Self { kanban, database }
    }
}

impl MirrorTaskStore for KanbanMirrorTaskStore {
    fn find_task(&self, task_id: &str) -> Result<Option<KanbanTask>, String> {
        self.kanban
            .get_task_by_id(task_id)
            .map(|found| found.map(|found| found.task))
            .map_err(|error| error.to_string())
    }

    fn default_session_type(&self) -> Result<SessionType, String> {
        AppSettingsService::new(&self.database, ShortcutPlatform::current())
            .get_default_session_type()
            .map_err(|error| error.to_string())
    }

    fn connect_terminal_session(
        &self,
        task_id: &str,
        session_type: SessionType,
    ) -> Result<Option<KanbanTask>, String> {
        self.kanban
            .connect_terminal_session(task_id, session_type)
            .map_err(|error| error.to_string())
    }
}

impl HerdrMirrorQueries for HerdrService {
    fn list_mirror_panes(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
        options: &HerdrMirrorPaneOptions,
    ) -> Result<Vec<MirrorPane>, String> {
        self.list_herdr_mirror_panes(session_name, ssh_host, options)
            .map_err(|error| error.to_string())
    }

    fn read_pane_extent(
        &self,
        session_name: &str,
        pane_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<HerdrPaneExtent, String> {
        self.read_herdr_pane_extent(session_name, pane_id, ssh_host)
            .map_err(|error| error.to_string())
    }

    fn read_task_pane_size(
        &self,
        session_name: &str,
        pane_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<Option<HerdrPaneSize>, String> {
        self.read_herdr_task_pane_size(session_name, pane_id, ssh_host)
            .map_err(|error| error.to_string())
    }

    fn is_task_alive(&self, session_name: &str, ssh_host: Option<&str>) -> bool {
        self.is_herdr_task_alive(session_name, ssh_host)
    }
}

impl TaskTerminalAttacher for TerminalBridge {
    fn attach_task_terminal(
        &self,
        task: &KanbanTask,
        tab_id: Option<&str>,
        client: Arc<dyn TerminalClient>,
        columns: Option<u16>,
        rows: Option<u16>,
    ) -> Result<Option<TerminalAttachment>, String> {
        TerminalBridge::attach_task_terminal(self, task, tab_id, client, columns, rows)
            .map_err(|error| error.message)
    }
}

impl MobileTerminalTabs for TerminalTabService {
    fn create_terminal_tab(&self, task_id: &str) -> Result<(), String> {
        let result = TerminalTabService::create_terminal_tab(self, task_id)
            .map_err(|error| error.to_string())?;
        match result.is_ok {
            true => Ok(()),
            false => Err(result
                .error
                .unwrap_or_else(|| "터미널 탭 생성 실패".to_string())),
        }
    }
}

impl RemotePaneGrants for RemoteTerminalService {
    fn prepare_grant(
        &self,
        input: &RemoteTerminalGrantInput,
    ) -> Result<Option<RemoteTerminalGrant>, String> {
        RemoteTerminalService::prepare_grant(self, input)
    }

    fn attach_relay(
        &self,
        grant_id: &str,
        device_id: &str,
        task_id: &str,
        pane_id: &str,
        on_output: RelayOutput,
        on_close: RelayClose,
    ) -> Result<Box<dyn RemotePaneRelay>, String> {
        let relay = RemoteTerminalService::attach_relay(
            self,
            grant_id,
            device_id,
            task_id,
            pane_id,
            move |output| on_output(output),
            move |code, reason| on_close(code, reason),
        )?;
        Ok(Box::new(relay))
    }

    fn revoke_grant(&self, grant_id: &str) {
        RemoteTerminalService::revoke_grant(self, grant_id);
    }

    fn revoke_device(&self, device_id: &str) {
        RemoteTerminalService::revoke_device(self, device_id);
    }

    fn revoke_task(&self, task_id: &str) {
        RemoteTerminalService::revoke_task(self, task_id);
    }

    fn stop_agents(&self) {
        RemoteTerminalService::stop_agents(self);
    }
}

impl RemotePaneRelay for RemoteTerminalRelay {
    fn write(&self, input: &[u8], is_binary: bool) {
        RemoteTerminalRelay::write(self, input, is_binary);
    }

    fn close(&self) {
        RemoteTerminalRelay::close(self);
    }
}
