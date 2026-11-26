//! 계정 로그인 CLI를 띄운 내장 터미널(TS `AiAccountLoginTerminal`). 출력은 VT 화면(`TerminalScreen`)에 쌓아 셀 색·굵기와 함께
//! 고정폭 글자로 그리고, 포커스된 동안 누른 글자는 그대로 CLI에 보낸다. 상자 크기가 바뀌면 xterm fit처럼 열·행을 다시 재어
//! 화면과 PTY를 함께 맞춘다. 화면이 사라지면 로그인 세션을 닫는다.
//!
//! 왼쪽 버튼 드래그로 글을 고르고 복사 키로 클립보드에 넣는다(작업 터미널과 같은 선택 규칙·복사 키).
//!
//! task-detail 터미널 뷰가 공용 컴포넌트로 옮겨지면 이 화면도 그 뷰로 바꾼다(스크롤백·커서 깜빡임은 그때 따라온다).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui_kit::{
    Bounds, Context, EventEmitter, FocusHandle, FontStyle, FontWeight, HighlightStyle, Hsla,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, Render, StrikethroughStyle, Styled, StyledText, Task,
    UnderlineStyle, Window, canvas, div, px, size,
};
use kanvibe_ai::accounts::desktop_login_bridge::LoginWindow;
use kanvibe_ai::usage::types::AiUsageProvider;
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_terminal::osc52_clipboard::{
    Osc52Body, Osc52SequenceScanner, read_osc52_clipboard_text,
};
use kanvibe_terminal::screen::{
    CellColor, ScreenCell, TERMINAL_CURSOR, TerminalScreen, default_color,
};

use crate::components::TailwindText;
use crate::components::terminal_view::input::{KeyEncodingModes, encode_keystroke, encode_paste};
use crate::components::terminal_view::mouse::{GridPoint, GridSelection};
use crate::components::terminal_view::paint::{GridLayout, SELECTION_BACKGROUND};
use crate::components::terminal_view::pane::selection_text;
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
    /// 글자 칸이 그려진 자리. 마우스 위치를 칸으로 바꿀 때 쓴다
    grid_layout: Option<GridLayout>,
    /// 드래그로 고른 영역과 지금 끌고 있는지
    selection: Option<GridSelection>,
    is_selecting: bool,
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
            grid_layout: None,
            selection: None,
            is_selecting: false,
            _events: events,
        }
    }

    /// 지금 화면·PTY 크기(열, 행)
    pub fn grid_size(&self) -> (u16, u16) {
        self.grid_size
    }

    /// 글자 칸이 그려진 자리(첫 그리기 전에는 없다)
    pub fn grid_layout(&self) -> Option<GridLayout> {
        self.grid_layout
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

    /// 끝난 뒤에는 입력을 보내지 않는다. 방향키·Ctrl 조합은 작업 터미널과 같은 xterm 바이트로, 붙여넣기는 클립보드 글로 보낸다
    fn handle_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.has_exited {
            return;
        }
        let keystroke = &event.keystroke;
        let platform = ShortcutPlatform::current();
        if is_copy_chord(keystroke, platform) {
            self.copy_selection(cx);
            cx.stop_propagation();
            return;
        }
        let input_modes = self.screen.input_modes();
        let data = if is_paste_chord(keystroke, platform) {
            let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
                return;
            };
            encode_paste(&text, input_modes.is_bracketed_paste)
        } else {
            let modes = KeyEncodingModes {
                is_application_cursor: input_modes.is_application_cursor,
            };
            let typed_character = keystroke
                .key_char
                .clone()
                .filter(|_| !keystroke.modifiers.platform);
            match encode_keystroke(keystroke, modes).or(typed_character) {
                Some(data) => data,
                None => return,
            }
        };
        self.backend
            .write_ai_account_login(self.window.window_id, &self.account_root, &data);
        cx.stop_propagation();
    }
}

/// 작업 터미널과 같은 복사 키: macOS `Cmd+C`, 그 밖은 `Ctrl+Shift+C`. `Ctrl+C`는 CLI 중단 키로 그대로 보낸다
fn is_copy_chord(keystroke: &gpui_kit::Keystroke, platform: ShortcutPlatform) -> bool {
    let modifiers = &keystroke.modifiers;
    let is_clipboard_chord = match platform {
        ShortcutPlatform::Mac => modifiers.platform && !modifiers.control && !modifiers.alt,
        ShortcutPlatform::Linux => modifiers.control && modifiers.shift && !modifiers.alt,
    };
    keystroke.key == "c" && is_clipboard_chord
}

/// 작업 터미널과 같은 붙여넣기 키: macOS `Cmd+V`, 그 밖은 `Ctrl+Shift+V`와 `Shift+Insert`
fn is_paste_chord(keystroke: &gpui_kit::Keystroke, platform: ShortcutPlatform) -> bool {
    let modifiers = &keystroke.modifiers;
    match platform {
        ShortcutPlatform::Mac => {
            keystroke.key == "v" && modifiers.platform && !modifiers.control && !modifiers.alt
        }
        ShortcutPlatform::Linux => {
            let is_control_shift_v =
                keystroke.key == "v" && modifiers.control && modifiers.shift && !modifiers.alt;
            let is_shift_insert = keystroke.key == "insert"
                && modifiers.shift
                && !modifiers.control
                && !modifiers.platform
                && !modifiers.alt;
            is_control_shift_v || is_shift_insert
        }
    }
}

impl AiAccountLoginTerminal {
    /// 상자 안쪽 크기로 열·행을 다시 잰다(xterm fit). 바뀌었으면 화면과 PTY를 함께 맞춘다
    fn fit_to(&mut self, inner_bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        let inner_size = inner_bounds.size;
        let columns = (f32::from(inner_size.width) / CELL_WIDTH).floor().max(1.0) as u16;
        let rows = (f32::from(inner_size.height) / CELL_HEIGHT)
            .floor()
            .max(1.0) as u16;
        self.grid_layout = Some(GridLayout {
            origin: inner_bounds.origin,
            cell_size: size(px(CELL_WIDTH), px(CELL_HEIGHT)),
            columns,
            rows,
        });
        if (columns, rows) == self.grid_size {
            return;
        }
        self.selection = None;
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

    /// 왼쪽 버튼을 누른 칸에서 새 선택을 시작한다. 이전 선택은 버린다
    fn start_selection(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(layout) = self.grid_layout else {
            return;
        };
        let point = layout.grid_point(event.position);
        self.selection = Some(GridSelection {
            anchor: point,
            head: point,
        });
        self.is_selecting = true;
        cx.notify();
    }

    /// 상자 밖에서 버튼을 떼 뗌을 받지 못했으면 버튼 없는 이동에서 끌기를 끝낸다
    fn extend_selection(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.is_selecting = false;
            return;
        }
        let (true, Some(layout), Some(selection)) =
            (self.is_selecting, self.grid_layout, self.selection.as_mut())
        else {
            return;
        };
        let point = layout.grid_point(event.position);
        if selection.head != point {
            selection.head = point;
            cx.notify();
        }
    }

    fn finish_selection(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.filter(|selection| !selection.is_empty()) else {
            return;
        };
        let text = selection_text(&self.screen, selection, usize::from(self.grid_size.0));
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));
    }

    /// 한 줄을 셀 색·꾸밈이 있는 글자 조각으로 만든다. 커서 칸은 커서 색 블록으로, 고른 칸은 선택 색으로 칠한다
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
            let mut style = cell_highlight(cell, Some(column) == cursor_column, theme);
            let is_selected = self.selection.is_some_and(|selection| {
                selection.contains(GridPoint {
                    row: screen_row,
                    column,
                })
            });
            if is_selected {
                style.background_color = Some(gpui_kit::rgba(SELECTION_BACKGROUND).into());
            }
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
                    let _ = entity.update(cx, |this, cx| this.fit_to(bounds, cx));
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
            .on_mouse_down(MouseButton::Left, cx.listener(Self::start_selection))
            .on_mouse_move(cx.listener(Self::extend_selection))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::finish_selection))
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
