//! 터미널 뷰가 브리지에 붙는 창 자리(TS `WebContents`). 레지스트리·브리지·탭 서비스는 런타임이 하나만 갖고,
//! 여기에는 뷰마다 다른 창 id와 출력 채널만 둔다.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use kanvibe_terminal::terminal_bridge::TerminalWindow;

/// 터미널 뷰 창 id. 브리지는 (창, task, 탭)마다 클라이언트를 하나 두므로 뷰마다 달라야 한다
static NEXT_WINDOW_ID: AtomicU64 = AtomicU64::new(1);

/// 터미널 뷰 하나가 쓰는 창 자리를 만든다
pub fn open_terminal_window() -> (
    Arc<ViewTerminalWindow>,
    UnboundedReceiver<TerminalWindowEvent>,
) {
    let (sender, receiver) = unbounded();
    let window = Arc::new(ViewTerminalWindow {
        window_id: NEXT_WINDOW_ID.fetch_add(1, Ordering::Relaxed),
        sender,
        is_destroyed: AtomicBool::new(false),
    });
    (window, receiver)
}

/// 브리지가 터미널 뷰에 알리는 일
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalWindowEvent {
    /// 레지스트리 화면에 이미 반영된 출력. OSC 52 판정에만 원본 바이트를 쓴다
    Output(String),
    Closed(Option<String>),
}

/// TS `WebContents` 자리. 출력은 채널로 뷰에 넘긴다
pub struct ViewTerminalWindow {
    window_id: u64,
    sender: UnboundedSender<TerminalWindowEvent>,
    is_destroyed: AtomicBool,
}

impl ViewTerminalWindow {
    pub fn id(&self) -> u64 {
        self.window_id
    }

    /// 뷰가 사라질 때 부른다. 이후 출력은 버린다
    pub fn mark_destroyed(&self) {
        self.is_destroyed.store(true, Ordering::Relaxed);
    }
}

impl TerminalWindow for ViewTerminalWindow {
    fn window_id(&self) -> u64 {
        self.window_id
    }

    fn is_destroyed(&self) -> bool {
        self.is_destroyed.load(Ordering::Relaxed)
    }

    fn send_terminal_output(&self, _task_id: &str, _tab_id: Option<&str>, output: &str) {
        let _ = self
            .sender
            .unbounded_send(TerminalWindowEvent::Output(output.to_string()));
    }

    fn send_terminal_closed(&self, _task_id: &str, _tab_id: Option<&str>, reason: Option<&str>) {
        let _ = self
            .sender
            .unbounded_send(TerminalWindowEvent::Closed(reason.map(str::to_string)));
    }
}
