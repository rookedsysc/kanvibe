//! pane의 지금 화면을 모바일 터미널이 처음부터 다시 그리는 출력으로 떠 온다(TS `readPaneSnapshot`).
//!
//! tmux는 `capture-pane`, zellij는 `dump-screen`, Herdr는 `pane read --source recent`가 화면의 출처다.
//! 화면을 멀티플렉서에서 그대로 떠 오므로 xterm 직렬화와 달리 공백이 그대로 남는다.
//! terminal 세션만 KanVibe가 가진 VT 화면(`kanvibe_terminal::screen`)의 직렬화를 쓰며, 그 출력은 xterm 직렬화와 바이트 단위로 같지 않다.

use kanvibe_domain::task::SessionType;
use kanvibe_terminal::pane_mirror::{
    build_herdr_recent_screen_command, build_screen_redraw, build_tmux_capture_pane_command,
    build_tmux_pane_state_command, build_zellij_dump_viewport_command, parse_tmux_pane_state,
};
use kanvibe_terminal::remote_terminal_protocol::build_wheel_delegated_screen_redraw;

use crate::error::MirrorResult;
use crate::mirror::{MirrorSessionTarget, MirrorShared};

impl MirrorShared {
    pub fn read_pane_snapshot(
        &self,
        target: &MirrorSessionTarget,
        pane_id: &str,
    ) -> MirrorResult<String> {
        let ssh_host = target.ssh_host();
        match target.session_type {
            SessionType::Zellij => {
                let command = build_zellij_dump_viewport_command(&target.session_name, pane_id);
                let dump = self.run_command(&command, ssh_host)?;
                Ok(build_wheel_delegated_screen_redraw(&dump))
            }
            SessionType::Herdr => {
                let command = build_herdr_recent_screen_command(&target.session_name, pane_id)?;
                let recent_screen = self.run_command(&command, ssh_host)?;
                Ok(build_wheel_delegated_screen_redraw(&recent_screen))
            }
            _ => self.read_tmux_snapshot(pane_id, ssh_host),
        }
    }

    /// 화면과 커서를 한 번에 읽는다. 원격이면 왕복이 하나 줄고, 둘 사이에 화면이 바뀌는 틈도 좁아진다
    fn read_tmux_snapshot(&self, pane_id: &str, ssh_host: Option<&str>) -> MirrorResult<String> {
        let command = format!(
            "{} && {}",
            build_tmux_capture_pane_command(pane_id),
            build_tmux_pane_state_command(pane_id)
        );
        let output = self.run_command(&command, ssh_host)?;
        let cursor_line_start = output.rfind('\n');
        let state = cursor_line_start.and_then(|start| parse_tmux_pane_state(&output[start + 1..]));
        let screen = match (&state, cursor_line_start) {
            (Some(_), Some(start)) => &output[..start],
            _ => output.as_str(),
        };
        let restore_sequences = state
            .as_ref()
            .map_or("", |state| state.restore_sequences.as_str());
        let cursor = state.as_ref().map(|state| &state.cursor);
        Ok(format!(
            "{restore_sequences}{}",
            build_screen_redraw(screen, cursor)
        ))
    }
}
