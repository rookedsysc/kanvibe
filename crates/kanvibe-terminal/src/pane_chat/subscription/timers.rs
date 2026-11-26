//! 구독 감시의 시계와 예약 실행(TS `setTimeout`·`clearTimeout`·`Date.now`).
//!
//! 실제 앱은 예약 스레드 하나가 때가 된 일을 새 스레드에서 돌려, pane 하나를 오래 읽어도 다른 pane의 예약이 밀리지 않는다.
//! 테스트는 가상 시간을 앞당기는 구현을 넣어 vitest 가짜 타이머처럼 정해진 순서로 돌린다.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

/// 예약 하나를 가리키는 값. 취소할 때 쓴다
pub type PaneChatTimerId = u64;

/// 예약한 시각에 한 번 돌릴 일
pub type PaneChatTimerCallback = Box<dyn FnOnce() + Send>;

pub trait PaneChatTimers: Send + Sync {
    /// 경과 시간을 잴 밀리초 시각
    fn now_ms(&self) -> f64;
    /// `delay` 뒤에 `callback`을 한 번 돌린다. 호출한 쪽을 막지 않는다
    fn schedule(&self, delay: Duration, callback: PaneChatTimerCallback) -> PaneChatTimerId;
    /// 아직 돌지 않은 예약을 지운다. 이미 돌았거나 없는 예약이면 아무것도 하지 않는다
    fn cancel(&self, timer: PaneChatTimerId);
}

#[derive(Default)]
struct TimerQueueState {
    next_id: PaneChatTimerId,
    pending: BTreeMap<(Instant, PaneChatTimerId), PaneChatTimerCallback>,
    due_by_id: HashMap<PaneChatTimerId, Instant>,
    is_shut_down: bool,
}

#[derive(Default)]
struct TimerQueue {
    state: Mutex<TimerQueueState>,
    wakeup: Condvar,
}

impl TimerQueue {
    fn lock(&self) -> MutexGuard<'_, TimerQueueState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 예약 스레드의 본문. 때가 된 일은 새 스레드에서 돌려 읽기가 오래 걸려도 다음 예약을 막지 않는다
    fn run(&self) {
        let mut state = self.lock();
        loop {
            if state.is_shut_down {
                return;
            }
            let Some((&(due, id), _)) = state.pending.first_key_value() else {
                state = self
                    .wakeup
                    .wait(state)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                continue;
            };
            let now = Instant::now();
            if due > now {
                state = self
                    .wakeup
                    .wait_timeout(state, due - now)
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .0;
                continue;
            }
            state.due_by_id.remove(&id);
            if let Some(callback) = state.pending.remove(&(due, id)) {
                thread::spawn(callback);
            }
        }
    }
}

/// 실제 앱의 예약 실행기. 값이 사라지면 예약 스레드도 끝난다
pub struct ThreadPaneChatTimers {
    queue: Arc<TimerQueue>,
    origin: Instant,
}

impl ThreadPaneChatTimers {
    pub fn new() -> Self {
        let queue = Arc::new(TimerQueue::default());
        let worker_queue = Arc::clone(&queue);
        thread::spawn(move || worker_queue.run());
        Self {
            queue,
            origin: Instant::now(),
        }
    }
}

impl Default for ThreadPaneChatTimers {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ThreadPaneChatTimers {
    fn drop(&mut self) {
        self.queue.lock().is_shut_down = true;
        self.queue.wakeup.notify_all();
    }
}

impl PaneChatTimers for ThreadPaneChatTimers {
    fn now_ms(&self) -> f64 {
        self.origin.elapsed().as_secs_f64() * 1_000.0
    }

    fn schedule(&self, delay: Duration, callback: PaneChatTimerCallback) -> PaneChatTimerId {
        let due = Instant::now() + delay;
        let mut state = self.queue.lock();
        state.next_id += 1;
        let id = state.next_id;
        state.pending.insert((due, id), callback);
        state.due_by_id.insert(id, due);
        drop(state);
        self.queue.wakeup.notify_all();
        id
    }

    fn cancel(&self, timer: PaneChatTimerId) {
        let mut state = self.queue.lock();
        if let Some(due) = state.due_by_id.remove(&timer) {
            state.pending.remove(&(due, timer));
        }
    }
}
