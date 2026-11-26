//! 화면이 해석 중에 만든 터미널 질의 응답(DA1·DA2·DSR·CPR·`CSI t`·OSC 색 질의).
//!
//! TS 앱에서는 렌더러의 xterm.js가 이 질의에 답했고 main 프로세스의 headless xterm은 답하지 않았다.
//! 그래서 응답은 여기 모아 두기만 하고, 보낼지는 레지스트리가 데스크탑 뷰가 붙어 있는지 보고 정한다.

use std::sync::{Arc, Mutex, PoisonError};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::vte::ansi::Rgb;

/// `CSI 14t`·`CSI 16t` 응답에 쓰는 셀 픽셀 크기. 뷰가 실제 글꼴 크기를 알려 주기 전 값이다
const DEFAULT_CELL_PIXEL_SIZE: (u16, u16) = (8, 17);
/// 아무도 꺼내 가지 않는 화면(짧게 쓰고 버리는 pane chat 화면 등)에서 응답이 끝없이 쌓이지 않게 하는 상한
const MAX_PENDING_REPLIES: usize = 256;

#[derive(Debug)]
struct PendingReplies {
    replies: Vec<String>,
    columns: u16,
    rows: u16,
    cell_pixel_size: (u16, u16),
}

/// alacritty가 질의 응답을 보내는 자리. 화면과 같은 수명이고 응답을 순서대로 모은다
#[derive(Debug, Clone)]
pub(super) struct ScreenEvents {
    pending: Arc<Mutex<PendingReplies>>,
}

impl ScreenEvents {
    pub(super) fn new(columns: u16, rows: u16) -> Self {
        Self {
            pending: Arc::new(Mutex::new(PendingReplies {
                replies: Vec::new(),
                columns,
                rows,
                cell_pixel_size: DEFAULT_CELL_PIXEL_SIZE,
            })),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PendingReplies> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn take_replies(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().replies)
    }

    pub(super) fn set_size(&self, columns: u16, rows: u16) {
        let mut pending = self.lock();
        pending.columns = columns;
        pending.rows = rows;
    }

    pub(super) fn set_cell_pixel_size(&self, width: u16, height: u16) {
        self.lock().cell_pixel_size = (width.max(1), height.max(1));
    }

    fn window_size(pending: &PendingReplies) -> WindowSize {
        WindowSize {
            num_lines: pending.rows,
            num_cols: pending.columns,
            cell_width: pending.cell_pixel_size.0,
            cell_height: pending.cell_pixel_size.1,
        }
    }
}

impl EventListener for ScreenEvents {
    fn send_event(&self, event: Event) {
        let mut pending = self.lock();
        let reply = match event {
            Event::PtyWrite(text) => text,
            Event::ColorRequest(index, format) => format(default_color(index)),
            Event::TextAreaSizeRequest(format) => format(Self::window_size(&pending)),
            // OSC 52는 `Osc52ClipboardForwarder`가 원본 바이트에서 따로 처리한다. 나머지는 화면 상태가 아니다
            _ => return,
        };
        if pending.replies.len() >= MAX_PENDING_REPLIES {
            pending.replies.remove(0);
        }
        pending.replies.push(reply);
    }
}

/// xterm.js 기본 16색. TS 터미널 테마는 배경·글자·커서·선택 색만 바꿨다
const ANSI_COLORS: [u32; 16] = [
    0x2e3436, 0xcc0000, 0x4e9a06, 0xc4a000, 0x3465a4, 0x75507b, 0x06989a, 0xd3d7cf, 0x555753,
    0xef2929, 0x8ae234, 0xfce94f, 0x729fcf, 0xad7fa8, 0x34e2e2, 0xeeeeec,
];
/// TS `TERMINAL_THEME`의 글자·배경·커서 색
pub const TERMINAL_FOREGROUND: u32 = 0xe4e4e7;
pub const TERMINAL_BACKGROUND: u32 = 0x0a0a0a;
pub const TERMINAL_CURSOR: u32 = 0xe4e4e7;

/// 팔레트 번호(0~255)와 alacritty 특수 번호(256 글자, 257 배경, 258 커서)의 기본 색
pub fn default_color(index: usize) -> Rgb {
    let value = match index {
        0..=15 => ANSI_COLORS[index],
        16..=231 => color_cube(index - 16),
        232..=255 => {
            let level = 8 + (index as u32 - 232) * 10;
            (level << 16) | (level << 8) | level
        }
        256 => TERMINAL_FOREGROUND,
        258 => TERMINAL_CURSOR,
        _ => TERMINAL_BACKGROUND,
    };
    Rgb {
        r: (value >> 16) as u8,
        g: (value >> 8) as u8,
        b: value as u8,
    }
}

/// xterm 6x6x6 색 입방체. 각 축은 0, 95, 135, 175, 215, 255다
fn color_cube(offset: usize) -> u32 {
    let level = |step: usize| -> u32 {
        match step {
            0 => 0,
            step => 55 + step as u32 * 40,
        }
    };
    let (red, green, blue) = (offset / 36, (offset / 6) % 6, offset % 6);
    (level(red) << 16) | (level(green) << 8) | level(blue)
}
