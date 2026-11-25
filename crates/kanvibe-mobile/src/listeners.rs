//! 등록한 콜백 목록과 등록을 푸는 핸들. TS의 `Set<listener>`와 `() => set.delete(listener)`에 해당한다.

use std::sync::{Arc, Mutex, MutexGuard};

pub type Listener<Event> = Arc<dyn Fn(&Event) + Send + Sync>;

struct ListenerList<Event: ?Sized> {
    next_id: u64,
    entries: Vec<(u64, Listener<Event>)>,
}

/// 콜백 목록. 알릴 때는 목록을 복사해 잠금 밖에서 부르므로 콜백 안에서 다시 등록하거나 풀어도 막히지 않는다
pub(crate) struct ListenerSet<Event: ?Sized + 'static> {
    list: Arc<Mutex<ListenerList<Event>>>,
}

/// 등록 해제 핸들. `unsubscribe`를 부르거나 버리면 그 콜백은 더 부르지 않는다
pub struct ListenerHandle {
    release: Option<Box<dyn FnOnce() + Send>>,
}

impl ListenerHandle {
    pub fn unsubscribe(mut self) {
        self.release_now();
    }

    fn release_now(&mut self) {
        if let Some(release) = self.release.take() {
            release();
        }
    }
}

impl Drop for ListenerHandle {
    fn drop(&mut self) {
        self.release_now();
    }
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl<Event: ?Sized + 'static> Default for ListenerSet<Event> {
    fn default() -> Self {
        Self {
            list: Arc::new(Mutex::new(ListenerList {
                next_id: 0,
                entries: Vec::new(),
            })),
        }
    }
}

impl<Event: ?Sized + 'static> ListenerSet<Event> {
    pub fn add(&self, listener: Listener<Event>) -> ListenerHandle {
        let mut list = lock(&self.list);
        let listener_id = list.next_id;
        list.next_id += 1;
        list.entries.push((listener_id, listener));
        let shared = Arc::clone(&self.list);
        ListenerHandle {
            release: Some(Box::new(move || {
                lock(&shared).entries.retain(|(id, _)| *id != listener_id);
            })),
        }
    }

    /// 한 콜백이 실패해도 나머지 콜백은 계속 받는다(TS `try { listener(event) } catch`)
    pub fn emit(&self, event: &Event, failure_label: &str) {
        let listeners: Vec<Listener<Event>> = lock(&self.list)
            .entries
            .iter()
            .map(|(_, listener)| Arc::clone(listener))
            .collect();
        for listener in listeners {
            let delivery =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| listener(event)));
            if delivery.is_err() {
                eprintln!("[kanvibe] {failure_label}");
            }
        }
    }
}
