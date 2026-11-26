//! 진행 중인 작업 하나를 여러 호출자가 함께 기다리는 자리(TS 진행 중 Promise를 맵에 올려 두던 방식).
//! 먼저 온 호출자가 작업을 실행하고 결과를 채우면, 기다리던 호출자는 같은 결과를 받는다.

use std::sync::{Arc, Condvar, Mutex, PoisonError};

pub(super) struct Flight<T: Clone> {
    state: Mutex<FlightState<T>>,
    finished: Condvar,
}

struct FlightState<T> {
    outcome: Option<T>,
    waiting_count: usize,
}

impl<T: Clone> Flight<T> {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(FlightState {
                outcome: None,
                waiting_count: 0,
            }),
            finished: Condvar::new(),
        })
    }

    pub(super) fn finish(&self, outcome: T) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.outcome = Some(outcome);
        self.finished.notify_all();
    }

    /// 작업이 끝날 때까지 기다려 결과를 받는다
    pub(super) fn wait(&self) -> T {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.waiting_count += 1;
        loop {
            if let Some(outcome) = state.outcome.clone() {
                state.waiting_count -= 1;
                return outcome;
            }
            state = self
                .finished
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// 지금 결과를 기다리는 호출자 수. 테스트가 경쟁 순서를 맞출 때 본다
    #[cfg(test)]
    pub(super) fn waiting_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .waiting_count
    }
}
