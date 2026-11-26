//! 다음 sync를 예약하는 타이머. 앱은 스레드 타이머를 쓰고, 테스트는 시간을 직접 넘기는 타이머를 쓴다.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub type TimerCallback = Box<dyn FnOnce() + Send>;

/// 예약 취소에 쓰는 번호
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimerId(pub u64);

pub trait SyncTimer: Send + Sync {
    /// `delay` 뒤에 한 번 부른다
    fn schedule(&self, delay: Duration, callback: TimerCallback) -> TimerId;
    /// 아직 불리지 않은 예약을 취소한다. 이미 불렸으면 아무 일도 하지 않는다
    fn cancel(&self, timer_id: TimerId);
}

#[derive(Default)]
struct ScheduledWait {
    is_cancelled: Mutex<bool>,
    wake: Condvar,
}

type ScheduledWaits = Arc<Mutex<Vec<(TimerId, Arc<ScheduledWait>)>>>;

/// 아직 불리지 않은 예약이면 목록에서 빼고 돌려준다. 발화와 취소가 겹쳐도 한쪽만 이긴다
fn take_wait(waits: &ScheduledWaits, timer_id: TimerId) -> Option<Arc<ScheduledWait>> {
    let mut waits = waits.lock().unwrap();
    let index = waits.iter().position(|(id, _)| *id == timer_id)?;
    Some(waits.remove(index).1)
}

/// 예약마다 대기 스레드 하나를 둔다. 예약은 사이클 사이에 하나뿐이라 스레드가 쌓이지 않는다
#[derive(Default)]
pub struct ThreadSyncTimer {
    next_id: AtomicU64,
    waits: ScheduledWaits,
}

impl SyncTimer for ThreadSyncTimer {
    fn schedule(&self, delay: Duration, callback: TimerCallback) -> TimerId {
        let timer_id = TimerId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let wait = Arc::new(ScheduledWait::default());
        self.waits
            .lock()
            .unwrap()
            .push((timer_id, Arc::clone(&wait)));
        let waits = Arc::clone(&self.waits);
        std::thread::spawn(move || {
            let deadline = Instant::now() + delay;
            let mut is_cancelled = wait.is_cancelled.lock().unwrap();
            while !*is_cancelled {
                let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                    break;
                };
                is_cancelled = wait.wake.wait_timeout(is_cancelled, remaining).unwrap().0;
            }
            drop(is_cancelled);
            if take_wait(&waits, timer_id).is_some() {
                callback();
            }
        });
        timer_id
    }

    fn cancel(&self, timer_id: TimerId) {
        if let Some(wait) = take_wait(&self.waits, timer_id) {
            *wait.is_cancelled.lock().unwrap() = true;
            wait.wake.notify_all();
        }
    }
}
