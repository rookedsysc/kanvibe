//! 계정 로그인 CLI를 띄운 내장 터미널(TS `AiAccountLoginTerminal`). 출력은 VT 화면(`TerminalScreen`)에 쌓아 셀 색·굵기와 함께
//! 고정폭 글자로 그리고, 포커스된 동안 누른 글자는 그대로 CLI에 보낸다. 상자 크기가 바뀌면 xterm fit처럼 열·행을 다시 재어
//! 화면과 PTY를 함께 맞춘다. 화면이 사라지면 로그인 세션을 닫는다.
//!
//! task-detail 터미널 뷰가 공용 컴포넌트로 옮겨지면 이 화면도 그 뷰로 바꾼다(선택·스크롤백·커서 깜빡임은 그때 따라온다).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui_kit::{
    Context, EventEmitter, FocusHandle, FontStyle, FontWeight, HighlightStyle, Hsla,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Pixels, Render, Size,
    StrikethroughStyle, Styled, StyledText, Task, UnderlineStyle, Window, canvas, div, px,
};
use kanvibe_ai::accounts::desktop_login_bridge::LoginWindow;
use kanvibe_ai::usage::types::AiUsageProvider;
use kanvibe_terminal::osc52_clipboard::{
    Osc52Body, Osc52SequenceScanner, read_osc52_clipboard_text,
};
use kanvibe_terminal::screen::{
    CellColor, ScreenCell, TERMINAL_CURSOR, TerminalScreen, default_color,
};

use crate::components::TailwindText;
use crate::theme::{ThemeTokens, radius};
use crate::views::ai_accounts::backend::AiAccountsBackend;

/// 처음 여는 크기. 기본 폭(`max-w-2xl` 카드)의 `h-64` 상자에 xterm fit을 한 값이고, 그린 뒤 실제 상자로 다시 잰다
const INITIAL_COLUMNS: u16 = 73;
const INITIAL_ROWS: u16 = 15;
/// `h-64`
const LOGIN_TERMINAL_HEIGHT: f32 = 256.0;
/// `p-2`
const LOGIN_TERMINAL_PADDING: f32 = 8.0;
/// TS `createTerminalOptions`의 `fontSize: 14`와 그때 xterm이 잰 셀 크기
const TERMINAL_FONT_SIZE: f32 = 14.0;
const CELL_WIDTH: f32 = 8.4;
const CELL_HEIGHT: f32 = 16.0;

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
    /// 지금 화면·PTY 크기(열, 행)
    grid_size: (u16, u16),
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
            INITIAL_COLUMNS,
            INITIAL_ROWS,
        );
        let mut screen = TerminalScreen::new(INITIAL_COLUMNS, INITIAL_ROWS);
        if !opened.ok {
            let error = opened
                .error
                .unwrap_or_else(|| "로그인 명령을 실행하지 못했습니다.".to_string());
            screen.write(format!("\r\n\x1b[31m{error}\x1b[0m\r\n").as_bytes());
        }
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        cx.on_release(|this, _| this.close()).detach();
        Self {
            backend,
            account_root,
            window: login_window,
            screen,
            grid_size: (INITIAL_COLUMNS, INITIAL_ROWS),
            clipboard_scanner: Osc52SequenceScanner::default(),
            has_exited: false,
            focus_handle,
            _events: events,
        }
    }

    /// 지금 화면·PTY 크기(열, 행)
    pub fn grid_size(&self) -> (u16, u16) {
        self.grid_size
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

impl AiAccountLoginTerminal {
    /// 상자 안쪽 크기로 열·행을 다시 잰다(xterm fit). 바뀌었으면 화면과 PTY를 함께 맞춘다
    fn fit_to(&mut self, inner_size: Size<Pixels>, cx: &mut Context<Self>) {
        let columns = (f32::from(inner_size.width) / CELL_WIDTH).floor().max(1.0) as u16;
        let rows = (f32::from(inner_size.height) / CELL_HEIGHT)
            .floor()
            .max(1.0) as u16;
        if (columns, rows) == self.grid_size {
            return;
        }
        self.grid_size = (columns, rows);
        self.screen.resize(columns, rows);
        if !self.has_exited {
            self.backend.resize_ai_account_login(
                self.window.window_id,
                &self.account_root,
                columns,
                rows,
            );
        }
        cx.notify();
    }

    /// 한 줄을 셀 색·꾸밈이 있는 글자 조각으로 만든다. 커서 칸은 커서 색 블록으로 칠한다
    fn render_row(&self, screen_row: usize, theme: &ThemeTokens) -> StyledText {
        let cells = self.screen.visible_cells(screen_row).unwrap_or_default();
        let cursor = self.screen.cursor();
        let cursor_column = (cursor.is_visible && !self.has_exited && cursor.row == screen_row)
            .then_some(cursor.column);
        let mut text = String::new();
        let mut highlights = Vec::new();
        for (column, cell) in cells.iter().enumerate() {
            if cell.width == 0 {
                continue;
            }
            let start = text.len();
            text.push_str(if cell.text.is_empty() || cell.is_hidden {
                " "
            } else {
                &cell.text
            });
            let style = cell_highlight(cell, Some(column) == cursor_column, theme);
            if style != HighlightStyle::default() {
                highlights.push((start..text.len(), style));
            }
        }
        StyledText::new(text).with_highlights(highlights)
    }
}

/// 셀 하나의 글자색·배경·꾸밈. 기본 색이면 상자 글자색을 그대로 쓴다
fn cell_highlight(cell: &ScreenCell, is_cursor: bool, theme: &ThemeTokens) -> HighlightStyle {
    // xterm `drawBoldTextInBrightColors`: 굵은 기본 8색은 밝은 색으로 그린다
    let foreground = match cell.fg {
        CellColor::Palette(index) if cell.is_bold && index < 8 => CellColor::Palette(index + 8),
        other => other,
    };
    let (mut foreground, mut background) = (cell_color(foreground), cell_color(cell.bg));
    if cell.is_inverse {
        let default_text = Some(theme.terminal.text.into());
        let default_background = Some(theme.terminal.background.into());
        (foreground, background) = (
            background.or(default_background),
            foreground.or(default_text),
        );
    }
    if is_cursor {
        (foreground, background) = (
            Some(theme.terminal.background.into()),
            Some(gpui_kit::rgb(TERMINAL_CURSOR).into()),
        );
    }
    HighlightStyle {
        color: foreground,
        background_color: background,
        font_weight: cell.is_bold.then_some(FontWeight::BOLD),
        font_style: cell.is_italic.then_some(FontStyle::Italic),
        underline: cell.is_underline.then(|| UnderlineStyle {
            thickness: px(1.0),
            ..UnderlineStyle::default()
        }),
        strikethrough: cell.is_strikethrough.then(|| StrikethroughStyle {
            thickness: px(1.0),
            ..StrikethroughStyle::default()
        }),
        fade_out: cell.is_dim.then_some(0.5),
    }
}

fn cell_color(color: CellColor) -> Option<Hsla> {
    match color {
        CellColor::Default => None,
        CellColor::Palette(index) => {
            let palette = default_color(usize::from(index));
            Some(rgb_hsla(palette.r, palette.g, palette.b))
        }
        CellColor::Rgb(red, green, blue) => Some(rgb_hsla(red, green, blue)),
    }
}

fn rgb_hsla(red: u8, green: u8, blue: u8) -> Hsla {
    gpui_kit::rgb((u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)).into()
}

impl Render for AiAccountLoginTerminal {
    /// `h-64 w-full overflow-hidden rounded-md bg-terminal-bg p-2`. 안쪽 크기는 그릴 때마다 재어 열·행을 맞춘다
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let rows: Vec<StyledText> = (0..self.screen.rows())
            .map(|screen_row| self.render_row(screen_row, &theme))
            .collect();
        let entity = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, _, cx| {
                cx.defer(move |cx| {
                    let _ = entity.update(cx, |this, cx| this.fit_to(bounds.size, cx));
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        div()
            .id("ai-account-login-terminal")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_key_down))
            .h(px(LOGIN_TERMINAL_HEIGHT))
            .w_full()
            .overflow_hidden()
            .rounded(radius::MD)
            .bg(theme.terminal.background)
            .p(px(LOGIN_TERMINAL_PADDING))
            .child(
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    .mono(400)
                    .text_size(px(TERMINAL_FONT_SIZE))
                    .line_height(px(CELL_HEIGHT))
                    .text_color(theme.terminal.text)
                    .child(measure)
                    .children(
                        // 터미널 한 줄은 화면 줄바꿈을 따르므로 글자 요소가 다시 접지 않는다
                        rows.into_iter().map(|row| {
                            div()
                                .flex_none()
                                .h(px(CELL_HEIGHT))
                                .w_full()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(row)
                        }),
                    ),
            )
    }
}
