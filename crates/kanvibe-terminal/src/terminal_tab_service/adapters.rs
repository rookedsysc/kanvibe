//! 탭 서비스의 협력자 trait을 실제 터미널 레지스트리와 Herdr 서비스로 잇는다.

use kanvibe_domain::terminal_tabs::TerminalTab;
use serde_json::Value;

use super::{TerminalTabHerdr, TerminalTabRegistry};
use crate::herdr::{HerdrError, HerdrMirrorPaneOptions, HerdrService};
use crate::pane_mirror::MirrorPane;
use crate::terminal_registry::TerminalRegistry;

impl TerminalTabRegistry for TerminalRegistry {
    fn list_local_terminal_tabs(&self, task_id: &str) -> Vec<TerminalTab> {
        TerminalRegistry::list_local_terminal_tabs(self, task_id)
    }

    fn list_remote_terminal_tabs(
        &self,
        task_id: &str,
        ssh_host: &str,
        session_name: &str,
    ) -> Result<Vec<TerminalTab>, String> {
        TerminalRegistry::list_remote_terminal_tabs(self, task_id, ssh_host, session_name)
    }

    fn close_remote_terminal_tab(
        &self,
        task_id: &str,
        tab_id: &str,
        ssh_host: &str,
        session_name: &str,
    ) -> Result<usize, String> {
        TerminalRegistry::close_remote_terminal_tab(self, task_id, tab_id, ssh_host, session_name)
    }

    fn close_local_terminal_tab(&self, task_id: &str, tab_id: &str) -> usize {
        TerminalRegistry::close_local_terminal_tab(self, task_id, tab_id)
    }

    fn local_terminal_tab_process(&self, task_id: &str, tab_id: &str) -> Option<String> {
        TerminalRegistry::local_terminal_tab_process(self, task_id, tab_id)
    }

    fn create_local_terminal_tab(&self, task_id: &str) {
        TerminalRegistry::create_local_terminal_tab(self, task_id);
    }

    fn select_local_terminal_tab(&self, task_id: &str, tab_id: &str) {
        TerminalRegistry::select_local_terminal_tab(self, task_id, tab_id);
    }

    fn rename_local_terminal_tab(&self, task_id: &str, tab_id: &str, name: &str) {
        TerminalRegistry::rename_local_terminal_tab(self, task_id, tab_id, name);
    }

    fn move_local_terminal_tab(&self, task_id: &str, tab_id: &str, target_index: usize) {
        TerminalRegistry::move_local_terminal_tab(self, task_id, tab_id, target_index);
    }

    fn herdr_task_active_tab_id(&self, task_id: &str) -> Option<String> {
        TerminalRegistry::herdr_task_active_tab_id(self, task_id)
    }

    fn focus_herdr_task_tab(&self, task_id: &str, tab_id: &str) -> Result<(), String> {
        TerminalRegistry::focus_herdr_task_tab(self, task_id, tab_id)
    }
}

impl TerminalTabHerdr for HerdrService {
    fn read_tabs(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<TerminalTab>, HerdrError> {
        self.read_herdr_tabs(session_name, ssh_host)
    }

    fn run_command(
        &self,
        session_name: &str,
        arguments: &[&str],
        ssh_host: Option<&str>,
    ) -> Result<Value, HerdrError> {
        self.run_herdr_command(session_name, arguments, ssh_host)
    }

    fn move_tab(
        &self,
        session_name: &str,
        tab_id: &str,
        insert_index: i64,
        ssh_host: Option<&str>,
    ) -> Result<(), HerdrError> {
        self.move_herdr_tab(session_name, tab_id, insert_index, ssh_host)
    }

    fn list_mirror_panes(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<MirrorPane>, HerdrError> {
        self.list_herdr_mirror_panes(session_name, ssh_host, &HerdrMirrorPaneOptions::default())
    }
}
