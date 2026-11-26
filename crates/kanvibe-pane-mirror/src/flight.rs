//! 한 번만 정해지는 결과를 여러 스레드가 함께 기다린다. TS에서 진행 중인 약속을 지도에 넣어 나눠 쓰던 자리다.

use std::sync::{Arc, Condvar, Mutex};

pub(crate) struct Flight<T: Clone> {
    outcome: Mutex<Option<T>>,
    settled: Condvar,
}

impl<T: Clone> Flight<T> {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            outcome: Mutex::new(None),
            settled: Condvar::new(),
        })
    }

    pub fn settle(&self, outcome: T) {
        *self.outcome.lock().unwrap() = Some(outcome);
        self.settled.notify_all();
    }

    pub fn wait(&self) -> T {
        let mut outcome = self.outcome.lock().unwrap();
        loop {
            if let Some(value) = outcome.as_ref() {
                return value.clone();
            }
            outcome = self.settled.wait(outcome).unwrap();
        }
    }

    /// 아직 정해지지 않았으면 `None`이다
    pub fn peek(&self) -> Option<T> {
        self.outcome.lock().unwrap().clone()
    }
}
