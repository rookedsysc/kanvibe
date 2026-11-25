//! 계정 로그인 CLI를 띄운 내장 터미널(TS `AiAccountLoginTerminal`). 출력은 VT 화면(`TerminalScreen`)에 쌓아 고정폭 글자로 그리고,
//! 포커스된 동안 누른 글자는 그대로 CLI에 보낸다. 화면이 사라지면 로그인 세션을 닫는다.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui_kit::{
    Context, EventEmitter, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, Styled, Task, Window, div, px,
};
use kanvibe_ai::accounts::desktop_login_bridge::LoginWindow;
use kanvibe_ai::usage::types::AiUsageProvider;
use kanvibe_terminal::osc52_clipboard::{
    Osc52Body, Osc52SequenceScanner, read_osc52_clipboard_text,
};
use kanvibe_terminal::screen::TerminalScreen;

use crate::components::TailwindText;
use crate::theme::{ThemeTokens, radius, spacing};
use crate::views::ai_accounts::backend::AiAccountsBackend;

/// 로그인 터미널 크기. TS는 `h-64` 상자에 맞췄다(xterm fit)
const LOGIN_TERMINAL_COLUMNS: u16 = 74;
const LOGIN_TERMINAL_ROWS: u16 = 14;
/// `h-64`
const LOGIN_TERMINAL_HEIGHT: f32 = 256.0;
const TERMINAL_LINE_HEIGHT: f32 = 16.0;

/// 로그인 명령이 끝났다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginExited(pub i32);

enum LoginEvent {
    Output(String),
    Exit(i32),
}

/// 로그인 브리지가 출력과 종료를 보내는 창. 화면 쪽은 채널로 받는다
struct ChannelLoginWindow {
    window_id: u64,
    sender: UnboundedSender<LoginEvent>,
    is_closed: AtomicBool,
}

impl LoginWindow for ChannelLoginWindow {
    fn id(&self) -> u64 {
        self.window_id
    }

    fn is_destroyed(&self) -> bool {
        self.is_closed.load(Ordering::SeqCst)
    }

    fn send_output(&self, _: &str, data: &str) {
        let _ = self
            .sender
            .unbounded_send(LoginEvent::Output(data.to_string()));
    }

    fn send_exit(&self, _: &str, exit_code: i32) {
        let _ = self.sender.unbounded_send(LoginEvent::Exit(exit_code));
    }
}

pub struct AiAccountLoginTerminal {
    backend: Arc<dyn AiAccountsBackend>,
    account_root: String,
    window: Arc<ChannelLoginWindow>,
    screen: TerminalScreen,
    /// 로그인 CLI가 OSC 52로 복사한 코드(기기 인증 코드 등)를 시스템 클립보드로 옮긴다
    clipboard_scanner: Osc52SequenceScanner,
    has_exited: bool,
    focus_handle: FocusHandle,
    _events: Task<()>,
}

impl EventEmitter<LoginExited> for AiAccountLoginTerminal {}

impl AiAccountLoginTerminal {
    pub fn new(
        backend: Arc<dyn AiAccountsBackend>,
        provider: AiUsageProvider,
        account_root: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (sender, mut receiver) = unbounded();
        let login_window = Arc::new(ChannelLoginWindow {
            window_id: window.window_handle().window_id().as_u64(),
            sender,
            is_closed: AtomicBool::new(false),
        });
        let events = cx.spawn(async move |this, cx| {
            while let Some(event) = receiver.next().await {
                if this
                    .update(cx, |this, cx| this.handle_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let opened = backend.open_ai_account_login(
            login_window.clone(),
            provider,
            &account_root,
            LOGIN_TERMINAL_COLUMNS,
            LOGIN_TERMINAL_ROWS,
        );
        let mut screen = TerminalScreen::new(LOGIN_TERMINAL_COLUMNS, LOGIN_TERMINAL_ROWS);
        if let Some(error) = opened.error.filter(|_| !opened.ok) {
            screen.write(error.as_bytes());
        }
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        cx.on_release(|this, _| this.close()).detach();
        Self {
            backend,
            account_root,
            window: login_window,
            screen,
            clipboard_scanner: Osc52SequenceScanner::default(),
            has_exited: false,
            focus_handle,
            _events: events,
        }
    }

    /// 지금 화면에 보이는 줄
    pub fn visible_lines(&self) -> Vec<String> {
        self.screen.viewport_lines()
    }

    fn handle_event(&mut self, event: LoginEvent, cx: &mut Context<Self>) {
        match event {
            LoginEvent::Output(data) => {
                self.forward_clipboard_requests(&data, cx);
                self.screen.write(data.as_bytes());
            }
            LoginEvent::Exit(exit_code) => {
                self.has_exited = true;
                cx.emit(LoginExited(exit_code));
            }
        }
        cx.notify();
    }

    /// 화면에 넣기 전에 같은 출력에서 끝난 OSC 52 복사 요청을 찾아 클립보드에 쓴다
    fn forward_clipboard_requests(&mut self, data: &str, cx: &mut Context<Self>) {
        for body in self.clipboard_scanner.scan(data.as_bytes()) {
            let Osc52Body::Text(payload) = body else {
                continue;
            };
            if let Some(text) = read_osc52_clipboard_text(&payload) {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));
            }
        }
    }

    fn close(&self) {
        self.window.is_closed.store(true, Ordering::SeqCst);
        self.backend
            .close_ai_account_login(self.window.window_id, &self.account_root);
    }

    /// 끝난 뒤에는 입력을 보내지 않는다
    fn handle_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.has_exited {
            return;
        }
        let data = match event.keystroke.key.as_str() {
            "enter" => "\r".to_string(),
            "backspace" => "\u{7f}".to_string(),
            "tab" => "\t".to_string(),
            "escape" => "\u{1b}".to_string(),
            _ => match &event.keystroke.key_char {
                Some(text) => text.clone(),
                None => return,
            },
        };
        self.backend
            .write_ai_account_login(self.window.window_id, &self.account_root, &data);
        cx.stop_propagation();
    }
}

impl Render for AiAccountLoginTerminal {
    /// `h-64 w-full overflow-hidden rounded-md bg-terminal-bg p-2`
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let lines = self.screen.viewport_lines();
        let cursor_row = self.screen.cursor_row();
        let cursor_column = self.screen.cursor_column();
        div()
            .id("ai-account-login-terminal")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_key_down))
            .h(px(LOGIN_TERMINAL_HEIGHT))
            .w_full()
            .overflow_hidden()
            .rounded(radius::MD)
            .bg(theme.terminal.background)
            .p(spacing(2.0))
            .mono(400)
            .text_size(px(13.0))
            .line_height(px(TERMINAL_LINE_HEIGHT))
            .text_color(theme.terminal.text)
            .children(lines.into_iter().enumerate().map(|(row, line)| {
                let text = if row == cursor_row && !self.has_exited {
                    with_cursor(&line, cursor_column)
                } else {
                    line
                };
                div().h(px(TERMINAL_LINE_HEIGHT)).child(text)
            }))
    }
}

/// 커서 자리에 블록 글자를 그린다(xterm 기본 블록 커서)
fn with_cursor(line: &str, cursor_column: usize) -> String {
    let mut characters: Vec<char> = line.chars().collect();
    while characters.len() <= cursor_column {
        characters.push(' ');
    }
    characters[cursor_column] = '█';
    characters
        .into_iter()
        .collect::<String>()
        .trim_end()
        .to_string()
}
