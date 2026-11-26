//! 한 소켓으로 나갈 프레임의 순서 있는 대기열. Node 소켓의 쓰기 버퍼에 해당한다.
//! 상태 잠금 안에서 막히지 않고 순서대로 넣고, 소켓마다 하나인 쓰기 스레드가 꺼내 쓴다.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// Node `net.Socket` 쓰기 버퍼의 기본 highWaterMark. 이만큼 쌓이면 읽는 쪽을 멈춘다
const HIGH_WATER_MARK_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub(super) struct OutboundQueue {
    backlog: Mutex<OutboundBacklog>,
    changed: Condvar,
}

#[derive(Default)]
struct OutboundBacklog {
    frames: VecDeque<Vec<u8>>,
    unwritten_bytes: usize,
    is_closed: bool,
}

impl OutboundQueue {
    /// 닫힌 뒤에 넣은 프레임은 버린다(Node에서 파괴한 소켓에 쓰는 것과 같다)
    pub(super) fn push(&self, bytes: Vec<u8>) {
        let mut backlog = self.lock_backlog();
        if backlog.is_closed {
            return;
        }
        backlog.unwritten_bytes += bytes.len();
        backlog.frames.push_back(bytes);
        self.changed.notify_all();
    }

    /// Node `write()`가 false를 돌려줄 만큼 쌓였는지
    pub(super) fn is_above_high_water_mark(&self) -> bool {
        self.lock_backlog().unwritten_bytes >= HIGH_WATER_MARK_BYTES
    }

    /// Node `drain`처럼 쌓인 프레임을 모두 쓰거나 닫힐 때까지 기다린다
    pub(super) fn wait_until_drained(&self) {
        let mut backlog = self.lock_backlog();
        while backlog.unwritten_bytes > 0 && !backlog.is_closed {
            backlog = self
                .changed
                .wait(backlog)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// 쓸 프레임을 기다려 꺼낸다. 닫히면 `None`이다
    pub(super) fn next_frame(&self) -> Option<Vec<u8>> {
        let mut backlog = self.lock_backlog();
        loop {
            if backlog.is_closed {
                return None;
            }
            if let Some(bytes) = backlog.frames.pop_front() {
                return Some(bytes);
            }
            backlog = self
                .changed
                .wait(backlog)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    pub(super) fn mark_written(&self, byte_count: usize) {
        let mut backlog = self.lock_backlog();
        backlog.unwritten_bytes = backlog.unwritten_bytes.saturating_sub(byte_count);
        self.changed.notify_all();
    }

    /// 남은 프레임을 버리고 기다리는 쪽을 모두 깨운다
    pub(super) fn close(&self) {
        let mut backlog = self.lock_backlog();
        backlog.is_closed = true;
        backlog.frames.clear();
        backlog.unwritten_bytes = 0;
        self.changed.notify_all();
    }

    fn lock_backlog(&self) -> MutexGuard<'_, OutboundBacklog> {
        self.backlog.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
