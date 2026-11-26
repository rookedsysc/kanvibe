//! 같은 대상에 대한 설치·검사·상태 조회를 스레드 사이에서 하나로 합치는 장치.
//! TS는 진행 중인 Promise를 Map에 두고 함께 기다렸다. 여기서는 결과 칸 하나를 여러 스레드가 기다린다.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::client_runtime::HookInstallError;

/// 작업 하나의 결과 칸. 오류는 메시지로 나눠 갖는다
struct JobSlot<T> {
    outcome: Mutex<Option<Result<T, String>>>,
    finished: Condvar,
}

impl<T: Clone> JobSlot<T> {
    fn new() -> Self {
        Self {
            outcome: Mutex::new(None),
            finished: Condvar::new(),
        }
    }

    fn wait(&self) -> Result<T, String> {
        let mut outcome = self.outcome.lock().unwrap();
        while outcome.is_none() {
            outcome = self.finished.wait(outcome).unwrap();
        }
        outcome.clone().expect("방금 확인했다")
    }

    fn finish(&self, outcome: Result<T, String>) {
        *self.outcome.lock().unwrap() = Some(outcome);
        self.finished.notify_all();
    }
}

fn into_install_error<T>(outcome: Result<T, String>) -> Result<T, HookInstallError> {
    outcome.map_err(HookInstallError::Message)
}

/// 같은 키의 작업이 진행 중이면 그 결과를 함께 쓴다
pub(super) struct SharedJobs<T> {
    active: Mutex<HashMap<String, Arc<JobSlot<T>>>>,
}

impl<T: Clone> SharedJobs<T> {
    pub(super) fn new() -> Self {
        Self {
            active: Mutex::new(HashMap::new()),
        }
    }

    pub(super) fn run(
        &self,
        key: &str,
        start: impl FnOnce() -> Result<T, HookInstallError>,
    ) -> Result<T, HookInstallError> {
        let slot = {
            let mut active = self.active.lock().unwrap();
            if let Some(slot) = active.get(key) {
                let slot = Arc::clone(slot);
                drop(active);
                return into_install_error(slot.wait());
            }
            let slot = Arc::new(JobSlot::new());
            active.insert(key.to_string(), Arc::clone(&slot));
            slot
        };
        let outcome = start().map_err(|error| error.to_string());
        remove_if_current(&self.active, key, &slot);
        slot.finish(outcome.clone());
        into_install_error(outcome)
    }
}

fn remove_if_current<T>(
    active: &Mutex<HashMap<String, Arc<JobSlot<T>>>>,
    key: &str,
    slot: &Arc<JobSlot<T>>,
) {
    let mut active = active.lock().unwrap();
    if active
        .get(key)
        .is_some_and(|current| Arc::ptr_eq(current, slot))
    {
        active.remove(key);
    }
}

/// 상태 조회 공유와 원격 상태 재사용. 설치가 끝나면 `forget`으로 설치 전 조회를 버린다
pub(super) struct StatusReads<T> {
    active: Mutex<HashMap<String, Arc<JobSlot<T>>>>,
    recent: Mutex<HashMap<String, (Instant, T)>>,
    reuse_window: Duration,
    clock: Arc<dyn Fn() -> Instant + Send + Sync>,
}

impl<T: Clone> StatusReads<T> {
    pub(super) fn new(
        reuse_window: Duration,
        clock: Arc<dyn Fn() -> Instant + Send + Sync>,
    ) -> Self {
        Self {
            active: Mutex::new(HashMap::new()),
            recent: Mutex::new(HashMap::new()),
            reuse_window,
            clock,
        }
    }

    fn recent_value(&self, key: &str) -> Option<T> {
        let now = (self.clock)();
        let recent = self.recent.lock().unwrap();
        let (read_at, value) = recent.get(key)?;
        (now.saturating_duration_since(*read_at) < self.reuse_window).then(|| value.clone())
    }

    /// `should_remember`가 참이면(원격 조회) 끝난 결과를 재사용 창 동안 기억한다
    pub(super) fn read(
        &self,
        key: &str,
        should_remember: bool,
        read: impl FnOnce() -> Result<T, HookInstallError>,
    ) -> Result<T, HookInstallError> {
        if let Some(value) = self.recent_value(key) {
            return Ok(value);
        }
        let slot = {
            let mut active = self.active.lock().unwrap();
            if let Some(slot) = active.get(key) {
                let slot = Arc::clone(slot);
                drop(active);
                return into_install_error(slot.wait());
            }
            let slot = Arc::new(JobSlot::new());
            active.insert(key.to_string(), Arc::clone(&slot));
            slot
        };
        let outcome = read().map_err(|error| error.to_string());
        self.settle(key, &slot, &outcome, should_remember);
        slot.finish(outcome.clone());
        into_install_error(outcome)
    }

    /// 읽는 사이 설치가 끝나 이 조회가 버려졌으면 설치 전 상태일 수 있으므로 기억하지 않는다
    fn settle(
        &self,
        key: &str,
        slot: &Arc<JobSlot<T>>,
        outcome: &Result<T, String>,
        should_remember: bool,
    ) {
        let mut active = self.active.lock().unwrap();
        let is_current = active
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, slot));
        if !is_current {
            return;
        }
        active.remove(key);
        if let (true, Ok(value)) = (should_remember, outcome) {
            let read_at = (self.clock)();
            self.recent
                .lock()
                .unwrap()
                .insert(key.to_string(), (read_at, value.clone()));
        }
    }

    pub(super) fn forget(&self, key: &str) {
        self.active.lock().unwrap().remove(key);
        self.recent.lock().unwrap().remove(key);
    }
}
