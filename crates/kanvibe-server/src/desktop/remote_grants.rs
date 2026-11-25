//! 원격 호스트 연결이 없는 데스크탑의 직접 연결 권한. 권한을 내지 않아 모바일은 로컬 미러로 비춘다.

use kanvibe_pane_mirror::{RelayClose, RelayOutput, RemotePaneGrants, RemotePaneRelay};
use kanvibe_terminal::remote_terminal_protocol::{RemoteTerminalGrant, RemoteTerminalGrantInput};

pub struct NoRemotePaneGrants;

impl RemotePaneGrants for NoRemotePaneGrants {
    fn prepare_grant(
        &self,
        _input: &RemoteTerminalGrantInput,
    ) -> Result<Option<RemoteTerminalGrant>, String> {
        Ok(None)
    }

    fn attach_relay(
        &self,
        _grant_id: &str,
        _device_id: &str,
        _task_id: &str,
        _pane_id: &str,
        _on_output: RelayOutput,
        _on_close: RelayClose,
    ) -> Result<Box<dyn RemotePaneRelay>, String> {
        Err("remote terminal agent is not available".to_string())
    }

    fn revoke_grant(&self, _grant_id: &str) {}
    fn revoke_device(&self, _device_id: &str) {}
    fn revoke_task(&self, _task_id: &str) {}
    fn stop_agents(&self) {}
}
