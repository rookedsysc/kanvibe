//! 터미널 뷰가 PTY 세션과 주고받는 길(TS `window.kanvibeDesktop`의 `openTerminal`·`writeTerminal`·`resizeTerminal`·
//! `closeTerminal`·`pasteImageToRemoteTerminal`). 앱은 런타임 브리지를 쓰고, 테스트는 같은 자리에 기록용 구현을 넣는다.

use std::sync::Arc;

use kanvibe_runtime::DesktopRuntime;
use kanvibe_terminal::terminal_bridge::TerminalOpenResult;

use super::pane::TerminalTarget;
use super::window::ViewTerminalWindow;

/// 원격 이미지 전송 결과. TS는 응답의 `ok: false`(터미널에 빨간 줄)와 요청 자체의 실패(다시 시도 알림)를 갈랐다
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImagePasteOutcome {
    /// 원격에 놓인 파일 경로
    Pasted(String),
    /// 전송은 했지만 원격이 거절했다. 사유를 터미널에 남긴다
    Rejected(String),
    /// 요청을 끝내지 못했다. 같은 이미지를 다시 보낼 수 있게 알린다
    RequestFailed(String),
}

pub trait TerminalTransport: Send + Sync {
    fn open(
        &self,
        window: Arc<ViewTerminalWindow>,
        target: &TerminalTarget,
        columns: u16,
        rows: u16,
    ) -> TerminalOpenResult;
    fn write(&self, window_id: u64, target: &TerminalTarget, data: &str);
    fn resize(&self, window_id: u64, target: &TerminalTarget, columns: u16, rows: u16);
    fn close(&self, window_id: u64, target: &TerminalTarget);
    fn paste_image(&self, task_id: &str, image_data_url: &str) -> ImagePasteOutcome;
}

/// 런타임 터미널 브리지로 보내는 기본 길
pub struct BridgeTransport(pub Arc<DesktopRuntime>);

impl TerminalTransport for BridgeTransport {
    fn open(
        &self,
        window: Arc<ViewTerminalWindow>,
        target: &TerminalTarget,
        columns: u16,
        rows: u16,
    ) -> TerminalOpenResult {
        self.0.terminal_bridge().open_terminal(
            window,
            &target.task_id,
            target.tab_id.as_deref(),
            columns,
            rows,
        )
    }

    fn write(&self, window_id: u64, target: &TerminalTarget, data: &str) {
        self.0.terminal_bridge().write_terminal(
            window_id,
            &target.task_id,
            target.tab_id.as_deref(),
            data,
        );
    }

    fn resize(&self, window_id: u64, target: &TerminalTarget, columns: u16, rows: u16) {
        self.0.terminal_bridge().resize_terminal(
            window_id,
            &target.task_id,
            target.tab_id.as_deref(),
            columns,
            rows,
        );
    }

    fn close(&self, window_id: u64, target: &TerminalTarget) {
        self.0.terminal_bridge().close_terminal(
            window_id,
            &target.task_id,
            target.tab_id.as_deref(),
        );
    }

    /// 같은 프로세스 호출이라 요청 자체가 실패하는 경우가 없다. 실패는 모두 원격의 거절로 돌아온다
    fn paste_image(&self, task_id: &str, image_data_url: &str) -> ImagePasteOutcome {
        match self
            .0
            .terminal_bridge()
            .paste_image_to_remote_terminal(task_id, image_data_url)
        {
            Ok(remote_path) => ImagePasteOutcome::Pasted(remote_path),
            Err(reason) => ImagePasteOutcome::Rejected(reason),
        }
    }
}
