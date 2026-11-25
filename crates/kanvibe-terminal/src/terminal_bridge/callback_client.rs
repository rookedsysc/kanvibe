//! 화면 없이 세션에 붙는 클라이언트(TS `CallbackTerminalClient`). 받은 출력을 콜백으로 넘긴다.
//!
//! 이미 떠 있는 PTY에 붙을 때는 지금 화면을 먼저 그려야 하는데, 그 화면이 준비되는 사이에도 출력이 들어온다.
//! [`CallbackTerminalClient::hold_output`]으로 그 출력을 모아 두었다가 [`CallbackTerminalClient::release_held_output`]이 화면 뒤에 이어 보낸다.
//! 이 클라이언트를 닫을 때는 attach가 돌려준 첨부로 레지스트리에 `client_closed`를 알린다.

use std::sync::Mutex;

use crate::terminal_registry::{ClientClosure, TerminalClient};

struct CallbackClientState {
    is_open: bool,
    held_chunks: Option<Vec<String>>,
}

pub struct CallbackTerminalClient {
    on_chunk: Box<dyn Fn(&str) + Send + Sync>,
    /// 화면과 모아 둔 출력 사이에 새 출력이 끼지 않도록 콜백도 이 잠금 안에서 부른다
    state: Mutex<CallbackClientState>,
}

impl CallbackTerminalClient {
    pub fn new(on_chunk: impl Fn(&str) + Send + Sync + 'static) -> Self {
        Self {
            on_chunk: Box::new(on_chunk),
            state: Mutex::new(CallbackClientState {
                is_open: true,
                held_chunks: None,
            }),
        }
    }

    pub fn hold_output(&self) {
        self.state
            .lock()
            .unwrap()
            .held_chunks
            .get_or_insert_with(Vec::new);
    }

    /// 화면을 먼저 보내고 모아 둔 출력을 이어 보낸다. 이후 출력은 곧바로 흘린다
    pub fn release_held_output(&self, screen: &str) {
        let mut state = self.state.lock().unwrap();
        let held_chunks = state.held_chunks.take().unwrap_or_default();
        if !screen.is_empty() {
            (self.on_chunk)(screen);
        }
        for chunk in &held_chunks {
            (self.on_chunk)(chunk);
        }
    }
}

impl TerminalClient for CallbackTerminalClient {
    fn is_open(&self) -> bool {
        self.state.lock().unwrap().is_open
    }

    fn send(&self, output: &str) {
        let mut state = self.state.lock().unwrap();
        if let Some(held_chunks) = &mut state.held_chunks {
            held_chunks.push(output.to_string());
            return;
        }
        (self.on_chunk)(output);
    }

    fn buffered_amount(&self) -> usize {
        0
    }

    fn close(&self, _closure: Option<ClientClosure>) {
        self.state.lock().unwrap().is_open = false;
    }
}
