//! 터미널 뷰가 브리지에 붙는 창 자리(TS `WebContents`). 레지스트리·브리지·탭 서비스는 런타임이 하나만 갖고,
//! 여기에는 뷰마다 다른 창 id와 출력 채널만 둔다.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use kanvibe_terminal::osc52_clipboard::{
    Osc52Body, Osc52SequenceScanner, read_osc52_clipboard_text,
};
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
        osc52: Mutex::default(),
        has_pending_output: AtomicBool::new(false),
    });
    (window, receiver)
}

/// 브리지가 터미널 뷰에 알리는 일
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalWindowEvent {
    /// 레지스트리 화면에 새 출력이 반영됐다. 뷰가 다시 그릴 때까지 온 출력은 이 신호 하나로 합친다
    Output,
    /// 멀티플렉서 너머 프로세스가 OSC 52로 보낸 복사 글
    ClipboardText(String),
    Closed(Option<String>),
}

/// TS `WebContents` 자리. 출력은 채널로 뷰에 넘긴다
pub struct ViewTerminalWindow {
    window_id: u64,
    sender: UnboundedSender<TerminalWindowEvent>,
    is_destroyed: AtomicBool,
    /// 출력을 보내는 스레드에서 OSC 52를 찾는다. 출력 바이트를 UI 스레드로 복사해 넘기지 않기 위해서다
    osc52: Mutex<Osc52SequenceScanner>,
    /// 뷰가 아직 받지 않은 `Output` 신호가 채널에 있는지
    has_pending_output: AtomicBool,
}

impl ViewTerminalWindow {
    pub fn id(&self) -> u64 {
        self.window_id
    }

    /// 뷰가 사라질 때 부른다. 이후 출력은 버린다
    pub fn mark_destroyed(&self) {
        self.is_destroyed.store(true, Ordering::Relaxed);
    }

    /// 뷰가 `Output` 신호를 받았다. 그 뒤에 오는 출력은 새 신호를 보낸다
    pub fn acknowledge_output(&self) {
        self.has_pending_output.store(false, Ordering::SeqCst);
    }

    fn forward_clipboard_requests(&self, output: &str) {
        let bodies = self
            .osc52
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .scan(output.as_bytes());
        for body in bodies {
            let Osc52Body::Text(payload) = body else {
                continue;
            };
            if let Some(text) = read_osc52_clipboard_text(&payload) {
                let _ = self
                    .sender
                    .unbounded_send(TerminalWindowEvent::ClipboardText(text));
            }
        }
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
        self.forward_clipboard_requests(output);
        if !self.has_pending_output.swap(true, Ordering::SeqCst) {
            let _ = self.sender.unbounded_send(TerminalWindowEvent::Output);
        }
    }

    fn send_terminal_closed(&self, _task_id: &str, _tab_id: Option<&str>, reason: Option<&str>) {
        let _ = self
            .sender
            .unbounded_send(TerminalWindowEvent::Closed(reason.map(str::to_string)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(receiver: &mut UnboundedReceiver<TerminalWindowEvent>) -> Vec<TerminalWindowEvent> {
        std::iter::from_fn(|| receiver.try_recv().ok()).collect()
    }

    #[test]
    fn a_clipboard_request_split_across_outputs_arrives_once_as_text() {
        let (window, mut receiver) = open_terminal_window();
        window.send_terminal_output("task", None, "before \u{1b}]52;c;Q09E");
        window.send_terminal_output("task", None, "RS0xMjM0\u{7} after");

        let clipboard_texts: Vec<_> = drain(&mut receiver)
            .into_iter()
            .filter_map(|event| match event {
                TerminalWindowEvent::ClipboardText(text) => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(clipboard_texts, ["CODE-1234"]);
    }

    #[test]
    fn plain_outputs_until_the_view_takes_them_become_one_redraw_signal() {
        let (window, mut receiver) = open_terminal_window();
        window.send_terminal_output("task", None, "first");
        window.send_terminal_output("task", None, "second");
        assert_eq!(drain(&mut receiver), [TerminalWindowEvent::Output]);

        window.acknowledge_output();
        window.send_terminal_output("task", None, "third");
        assert_eq!(drain(&mut receiver), [TerminalWindowEvent::Output]);
    }
}
