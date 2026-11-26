//! 가짜 서비스의 호출 기록과 응답 재정의(TS `vi.fn` + `createBridge(overrides)`).
//!
//! 모든 가짜 메서드는 이름과 인자(JSON)를 남긴 뒤, 테스트가 그 이름에 응답을 걸어 두었으면 그것을, 아니면 TS 기본값을 돌려준다.
//! 응답 함수는 서버의 블로킹 스레드에서 불리므로 [`Gate`]로 막아 두었다가 테스트가 원하는 순간에 풀 수 있다.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use kanvibe_server::bridge::{BridgeError, BridgeResult, Subscription};
use serde_json::Value;

use super::fake_defaults::default_response;

pub type Responder = Arc<dyn Fn(&Value) -> BridgeResult<Value> + Send + Sync>;

/// 블로킹 응답 함수가 테스트를 기다리는 문. 열리지 않으면 10초 뒤 `null`로 풀려 런타임 종료가 매달리지 않는다
#[derive(Default)]
pub struct Gate {
    value: Mutex<Option<Value>>,
    opened: Condvar,
}

impl Gate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn open(&self, value: Value) {
        *lock(&self.value) = Some(value);
        self.opened.notify_all();
    }

    pub fn wait(&self) -> Value {
        let guard = lock(&self.value);
        let (guard, _) = self
            .opened
            .wait_timeout_while(guard, Duration::from_secs(10), |value| value.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        guard.clone().unwrap_or(Value::Null)
    }
}

#[derive(Default)]
pub struct Recorder {
    calls: Mutex<Vec<(String, Value)>>,
    responders: Mutex<HashMap<String, Responder>>,
}

impl Recorder {
    /// 호출을 남기고 응답을 고른다
    pub fn call(&self, name: &str, args: Value) -> BridgeResult<Value> {
        self.record(name, args.clone());
        let responder = lock(&self.responders).get(name).cloned();
        match responder {
            Some(responder) => responder(&args),
            None => Ok(default_response(name, &args)),
        }
    }

    pub fn record(&self, name: &str, args: Value) {
        lock(&self.calls).push((name.to_string(), args));
    }

    pub fn respond(&self, name: &str, responder: Responder) {
        lock(&self.responders).insert(name.to_string(), responder);
    }

    pub fn calls(&self, name: &str) -> Vec<Value> {
        lock(&self.calls)
            .iter()
            .filter(|(called, _)| called == name)
            .map(|(_, args)| args.clone())
            .collect()
    }
}

/// 등록된 콜백 목록. 돌려준 [`Subscription`]을 멈추면 목록에서 빠진다(TS의 `Set` + 해제 함수)
pub struct Listeners<T> {
    entries: Arc<Mutex<(u64, BTreeMap<u64, T>)>>,
}

impl<T> Default for Listeners<T> {
    fn default() -> Self {
        Self {
            entries: Arc::new(Mutex::new((0, BTreeMap::new()))),
        }
    }
}

impl<T: Clone + Send + 'static> Listeners<T> {
    pub fn add(&self, item: T) -> Subscription {
        let id = {
            let mut entries = lock(&self.entries);
            entries.0 += 1;
            let id = entries.0;
            entries.1.insert(id, item);
            id
        };
        let entries = self.entries.clone();
        Subscription::new(move || {
            lock(&entries).1.remove(&id);
        })
    }

    pub fn snapshot(&self) -> Vec<T> {
        lock(&self.entries).1.values().cloned().collect()
    }

    pub fn count(&self) -> usize {
        lock(&self.entries).1.len()
    }
}

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 이름 없는 예외(TS `new Error(message)`)
pub fn unexpected(message: &str) -> BridgeError {
    BridgeError::Internal(message.to_string())
}
