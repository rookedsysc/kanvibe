//! 터미널 레지스트리가 쓰는 Herdr 연동(`terminal_registry::HerdrSessions`)을 [`HerdrService`]로 잇는다.

use kanvibe_process::worktree::PaneLayoutSelection;

use super::{HerdrError, HerdrService};
use crate::terminal_registry::{
    HerdrPaneTab, HerdrSessionError, HerdrSessions, HerdrTabBarPosition,
};

impl From<HerdrError> for HerdrSessionError {
    fn from(error: HerdrError) -> Self {
        match error {
            HerdrError::ServerHandoffRequired { .. } => Self::ServerHandoffRequired {
                message: error.to_string(),
            },
            HerdrError::Command(error) => Self::Command {
                message: error.message,
                stderr: error.stderr,
            },
            HerdrError::Message(message) => Self::Command {
                message,
                stderr: String::new(),
            },
        }
    }
}

impl HerdrSessions for HerdrService {
    fn ensure_session(
        &self,
        session_name: &str,
        working_directory: &str,
        pane_layout: Option<&PaneLayoutSelection>,
        ssh_host: Option<&str>,
    ) -> Result<Option<String>, HerdrSessionError> {
        Ok(self.ensure_herdr_session(session_name, working_directory, pane_layout, ssh_host)?)
    }

    fn ensure_remote_machine(
        &self,
        session_name: &str,
        ssh_host: &str,
    ) -> Result<(), HerdrSessionError> {
        Ok(self.ensure_herdr_remote_machine(session_name, ssh_host)?)
    }

    fn handoff_remote_server(
        &self,
        herdr_session: &str,
        ssh_host: &str,
    ) -> Result<(), HerdrSessionError> {
        Ok(self.handoff_herdr_remote_server(herdr_session, ssh_host)?)
    }

    fn client_socket_path(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<String, HerdrSessionError> {
        Ok(self.get_herdr_client_socket_path(session_name, ssh_host)?)
    }

    fn client_tab_bar_position(&self, ssh_host: Option<&str>) -> Option<HerdrTabBarPosition> {
        self.read_herdr_client_tab_bar_position(ssh_host)
    }

    fn list_task_pane_tab_ids(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<HerdrPaneTab>, HerdrSessionError> {
        let pane_tabs = self.list_herdr_task_pane_tab_ids(session_name, ssh_host)?;
        Ok(pane_tabs
            .into_iter()
            .map(|pane_tab| HerdrPaneTab {
                pane_id: pane_tab.pane_id,
                tab_id: pane_tab.tab_id,
            })
            .collect())
    }
}
