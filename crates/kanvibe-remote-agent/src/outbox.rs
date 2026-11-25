//! 레코드를 쓰는 쪽 큐. 전용 스레드가 큐를 비우며 쓰고, 보내는 쪽은 쓰기에 막히지 않는다.
//!
//! TS `socket.write`처럼 쌓인 바이트가 8 MiB를 넘으면 연결을 끊고(`destroy`), 출력을 흘리는 쪽은
//! [`RecordOutbox::wait_until_drained`]로 큐가 줄 때까지 기다려 메모리를 묶는다. hook 응답처럼 작은
//! 레코드는 기다리지 않고 바로 들어가므로 대량 출력 중에도 앞서 나간다.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Condvar, Mutex};

use crate::protocol::{HOST_BUFFER_BYTES, HostRecord, HostWireError, encode_host_record};

/// 출력 스트림이 다음 조각을 읽기 전에 큐가 이 아래로 내려오기를 기다린다
pub const STREAM_RESUME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OutboxError {
    #[error("Host transport disconnected or backpressured")]
    Closed,
    #[error("Host output backpressure limit")]
    Backpressure,
    #[error(transparent)]
    Wire(#[from] HostWireError),
}

/// 쓰기 스레드가 끝날 때 한 번 부르는 정리. 소켓을 닫거나 자식 stdin을 닫는다
pub type OutboxClosed = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct OutboxState {
    queue: VecDeque<Vec<u8>>,
    queued_bytes: usize,
    is_closing: bool,
    is_closed: bool,
}

pub struct RecordOutbox {
    state: Mutex<OutboxState>,
    changed: Condvar,
}

impl RecordOutbox {
    /// `writer`로 쓰는 스레드를 띄운다. 쓰기가 실패하거나 닫히면 `on_closed`를 부른다
    pub fn start(writer: Box<dyn Write + Send>, on_closed: OutboxClosed) -> Arc<Self> {
        let outbox = Arc::new(Self {
            state: Mutex::new(OutboxState::default()),
            changed: Condvar::new(),
        });
        let worker = Arc::clone(&outbox);
        std::thread::spawn(move || worker.write_until_closed(writer, on_closed));
        outbox
    }

    pub fn send(&self, record: &HostRecord) -> Result<(), OutboxError> {
        let bytes = encode_host_record(record)?;
        self.send_bytes(bytes)
    }

    /// 이미 인코딩한 바이트를 그대로 큐에 넣는다(부트스트랩 줄처럼 레코드가 아닌 바이트)
    pub fn send_bytes(&self, bytes: Vec<u8>) -> Result<(), OutboxError> {
        let mut state = self.state.lock().unwrap();
        if state.is_closing || state.is_closed {
            return Err(OutboxError::Closed);
        }
        if state.queued_bytes + bytes.len() > HOST_BUFFER_BYTES {
            drop(state);
            self.abort();
            return Err(OutboxError::Backpressure);
        }
        state.queued_bytes += bytes.len();
        state.queue.push_back(bytes);
        self.changed.notify_all();
        Ok(())
    }

    pub fn queued_bytes(&self) -> usize {
        self.state.lock().unwrap().queued_bytes
    }

    pub fn is_closed(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.is_closing || state.is_closed
    }

    /// 큐가 `limit` 아래로 내려오거나 닫힐 때까지 기다린다
    pub fn wait_until_drained(&self, limit: usize) {
        let state = self.state.lock().unwrap();
        let _state = self
            .changed
            .wait_while(state, |state| {
                state.queued_bytes >= limit && !state.is_closed && !state.is_closing
            })
            .unwrap();
    }

    /// 남은 레코드를 다 쓴 뒤 닫는다(TS `socket.end()`)
    pub fn close_after_flush(&self) {
        let mut state = self.state.lock().unwrap();
        state.is_closing = true;
        self.changed.notify_all();
    }

    /// 남은 레코드를 버리고 바로 닫는다(TS `socket.destroy()`)
    pub fn abort(&self) {
        let mut state = self.state.lock().unwrap();
        state.is_closed = true;
        state.queue.clear();
        state.queued_bytes = 0;
        self.changed.notify_all();
    }

    fn write_until_closed(&self, mut writer: Box<dyn Write + Send>, on_closed: OutboxClosed) {
        while let Some(bytes) = self.next_bytes() {
            let is_written = writer
                .write_all(&bytes)
                .and_then(|()| writer.flush())
                .is_ok();
            let mut state = self.state.lock().unwrap();
            state.queued_bytes = state.queued_bytes.saturating_sub(bytes.len());
            self.changed.notify_all();
            if !is_written {
                state.is_closed = true;
                break;
            }
        }
        drop(writer);
        let mut state = self.state.lock().unwrap();
        state.is_closed = true;
        state.queue.clear();
        state.queued_bytes = 0;
        self.changed.notify_all();
        drop(state);
        on_closed();
    }

    fn next_bytes(&self) -> Option<Vec<u8>> {
        let state = self.state.lock().unwrap();
        let mut state = self
            .changed
            .wait_while(state, |state| {
                state.queue.is_empty() && !state.is_closing && !state.is_closed
            })
            .unwrap();
        if state.is_closed {
            return None;
        }
        // 큐에서 꺼낸 바이트도 쓰기가 끝날 때까지 queued_bytes에 남겨 상한 계산에 넣는다
        state.queue.pop_front()
    }
}
