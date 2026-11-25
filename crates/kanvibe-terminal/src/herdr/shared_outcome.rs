//! 진행 중인 작업 하나의 결과를 여러 호출자가 함께 기다린다. TS에서 같은 Promise를 돌려주던 자리다.

use std::sync::{Arc, Condvar, Mutex};

pub(super) struct SharedOutcome<T> {
    value: Mutex<Option<T>>,
    completed: Condvar,
}

impl<T: Clone> SharedOutcome<T> {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            value: Mutex::new(None),
            completed: Condvar::new(),
        })
    }

    /// 결과가 정해질 때까지 기다렸다가 사본을 돌려준다
    pub(super) fn wait(&self) -> T {
        let mut value = self.value.lock().unwrap();
        loop {
            if let Some(outcome) = value.as_ref() {
                return outcome.clone();
            }
            value = self.completed.wait(value).unwrap();
        }
    }

    pub(super) fn complete(&self, outcome: T) {
        *self.value.lock().unwrap() = Some(outcome);
        self.completed.notify_all();
    }
}
