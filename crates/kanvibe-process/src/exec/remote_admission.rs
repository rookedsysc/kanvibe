//! 원격 명령의 입장 순서. 호스트별 명령 채널 상한(백그라운드 몫 포함)과 호스트별 인증 순서를 맡는다.
//! TS의 AsyncLocalStorage 두 개(백그라운드 우선순위, stdin 응답기 문맥)는 스레드 지역 범위로 옮겼다.
//! 응답기 callback은 별도 스레드에서 돌고, 그 스레드에 호출한 쪽의 우선순위와 응답기 문맥을 심는다.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::remote_command::{CommandFailure, timeout_error};

/// OpenSSH 기본 MaxSessions=10보다 두 채널 적게 둬 응답기 안의 중첩 명령이 쓸 자리를 남긴다
pub(crate) const REMOTE_SSH_HOST_MAX_CHANNELS: usize = 8;
/// 백그라운드 복구 작업은 상세 화면 조회가 쓸 채널을 남기도록 따로 상한을 둔다
const REMOTE_SSH_HOST_MAX_BACKGROUND_CHANNELS: usize = 2;

thread_local! {
    static BACKGROUND_PRIORITY_DEPTH: Cell<usize> = const { Cell::new(0) };
    static RESPONDER_CONTEXT: RefCell<Option<ResponderContext>> = const { RefCell::new(None) };
}

/// 이 스레드에서 실행하는 동안 원격 명령을 백그라운드 우선순위로 줄 세운다(TS `runWithBackgroundSSHCommands`)
pub fn run_with_background_ssh_commands<T>(operation: impl FnOnce() -> T) -> T {
    BACKGROUND_PRIORITY_DEPTH.with(|depth| depth.set(depth.get() + 1));
    let _restore = PriorityRestore;
    operation()
}

struct PriorityRestore;

impl Drop for PriorityRestore {
    fn drop(&mut self) {
        BACKGROUND_PRIORITY_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

pub(crate) fn is_background_priority() -> bool {
    BACKGROUND_PRIORITY_DEPTH.with(|depth| depth.get() > 0)
}

/// 진행 중인 stdin 응답기 callback의 호스트. callback이 같은 호스트에 보내는 명령은 채널 줄을 서지 않는다
#[derive(Clone)]
pub(crate) struct ResponderContext {
    pub ssh_host: String,
    pub is_active: Arc<AtomicBool>,
}

/// 응답기 callback 스레드에 호출한 쪽의 문맥을 심는다. 범위가 끝나면 이전 값으로 돌린다
pub(crate) struct ResponderScope {
    previous_context: Option<ResponderContext>,
    previous_depth: usize,
}

impl ResponderScope {
    pub fn enter(context: ResponderContext, is_background: bool) -> Self {
        let previous_context = RESPONDER_CONTEXT.with(|current| current.replace(Some(context)));
        let previous_depth =
            BACKGROUND_PRIORITY_DEPTH.with(|depth| depth.replace(usize::from(is_background)));
        Self {
            previous_context,
            previous_depth,
        }
    }
}

impl Drop for ResponderScope {
    fn drop(&mut self) {
        RESPONDER_CONTEXT.with(|current| current.replace(self.previous_context.take()));
        BACKGROUND_PRIORITY_DEPTH.with(|depth| depth.set(self.previous_depth));
    }
}

/// 같은 호스트의 응답기 callback 안에서 부른 명령인지
pub(crate) fn is_nested_responder_command(ssh_host: &str) -> bool {
    RESPONDER_CONTEXT.with(|current| {
        current.borrow().as_ref().is_some_and(|context| {
            context.is_active.load(Ordering::SeqCst) && context.ssh_host == ssh_host
        })
    })
}

pub(crate) fn remaining_timeout(deadline: Instant) -> Result<Duration, CommandFailure> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(timeout_error(Duration::ZERO, "", ""));
    }
    Ok(remaining)
}

struct QueuedSlot {
    id: u64,
    is_background: bool,
}

#[derive(Default)]
struct HostSlotPool {
    active_connections: usize,
    active_background_connections: usize,
    queue: VecDeque<QueuedSlot>,
}

#[derive(Default)]
struct SlotRegistry {
    pools: HashMap<String, HostSlotPool>,
    granted: HashSet<u64>,
    next_id: u64,
}

/// 호스트별 명령 채널. 대화형 명령이 앞서고, 백그라운드 명령은 따로 정한 몫까지만 채널을 잡는다
pub(crate) struct HostSlots {
    ceiling: usize,
    registry: Mutex<SlotRegistry>,
    changed: Condvar,
}

impl HostSlots {
    pub fn new(ceiling: usize) -> Self {
        Self {
            ceiling,
            registry: Mutex::new(SlotRegistry::default()),
            changed: Condvar::new(),
        }
    }

    fn dispatch(&self, registry: &mut SlotRegistry, ssh_host: &str) {
        let background_ceiling =
            REMOTE_SSH_HOST_MAX_BACKGROUND_CHANNELS.min(self.ceiling.saturating_sub(1).max(1));
        let Some(pool) = registry.pools.get_mut(ssh_host) else {
            return;
        };
        while pool.active_connections < self.ceiling {
            let mut index = pool.queue.iter().position(|entry| !entry.is_background);
            if index.is_none()
                && pool.active_background_connections < background_ceiling
                && !pool.queue.is_empty()
            {
                index = Some(0);
            }
            let Some(entry) = index.and_then(|index| pool.queue.remove(index)) else {
                break;
            };
            pool.active_connections += 1;
            if entry.is_background {
                pool.active_background_connections += 1;
            }
            registry.granted.insert(entry.id);
        }
        if pool.active_connections == 0 && pool.queue.is_empty() {
            registry.pools.remove(ssh_host);
        }
        self.changed.notify_all();
    }

    /// 채널을 얻을 때까지 기다린다. 기한이 지나면 줄에서 빠지고, 빠진 요청은 나중에 채널이 비어도 시작하지 않는다
    pub fn acquire(
        self: &Arc<Self>,
        ssh_host: &str,
        deadline: Instant,
    ) -> Result<SlotLease, CommandFailure> {
        let timeout = remaining_timeout(deadline)?;
        let is_background = is_background_priority();
        let mut registry = self.registry.lock().unwrap();
        let id = registry.next_id;
        registry.next_id += 1;
        let pool = registry.pools.entry(ssh_host.to_string()).or_default();
        pool.queue.push_back(QueuedSlot { id, is_background });
        self.dispatch(&mut registry, ssh_host);
        loop {
            if registry.granted.remove(&id) {
                return Ok(SlotLease {
                    slots: Arc::clone(self),
                    ssh_host: ssh_host.to_string(),
                    is_background,
                });
            }
            let now = Instant::now();
            if now >= deadline {
                if let Some(pool) = registry.pools.get_mut(ssh_host) {
                    pool.queue.retain(|entry| entry.id != id);
                }
                self.dispatch(&mut registry, ssh_host);
                return Err(timeout_error(timeout, "", ""));
            }
            registry = self
                .changed
                .wait_timeout(registry, deadline - now)
                .unwrap()
                .0;
        }
    }

    #[cfg(test)]
    pub fn queued_count(&self, ssh_host: &str) -> usize {
        let registry = self.registry.lock().unwrap();
        registry
            .pools
            .get(ssh_host)
            .map_or(0, |pool| pool.queue.len())
    }
}

/// 쥐고 있는 명령 채널. 놓으면 다음 줄을 깨운다
pub(crate) struct SlotLease {
    slots: Arc<HostSlots>,
    ssh_host: String,
    is_background: bool,
}

impl Drop for SlotLease {
    fn drop(&mut self) {
        let mut registry = self.slots.registry.lock().unwrap();
        let pool = registry.pools.entry(self.ssh_host.clone()).or_default();
        pool.active_connections = pool.active_connections.saturating_sub(1);
        if self.is_background {
            pool.active_background_connections =
                pool.active_background_connections.saturating_sub(1);
        }
        self.slots.dispatch(&mut registry, &self.ssh_host);
    }
}

#[derive(Default)]
struct AuthenticationQueue {
    next_ticket: u64,
    serving: u64,
    released: BTreeSet<u64>,
}

/// 같은 주소·포트로 가는 인증을 한 번에 하나씩 통과시킨다.
/// 기한이 지나 포기한 대기자도 앞사람이 끝날 때까지 자리를 지켜 뒤에 온 요청이 앞지르지 못한다
#[derive(Default)]
pub(crate) struct AuthenticationQueues {
    queues: Mutex<HashMap<String, AuthenticationQueue>>,
    changed: Condvar,
}

impl AuthenticationQueues {
    fn release(&self, key: &str, ticket: u64) {
        let mut queues = self.queues.lock().unwrap();
        if let Some(queue) = queues.get_mut(key) {
            queue.released.insert(ticket);
            while queue.released.remove(&queue.serving) {
                queue.serving += 1;
            }
            if queue.serving == queue.next_ticket {
                queues.remove(key);
            }
        }
        self.changed.notify_all();
    }

    /// 차례가 올 때까지 기다린다. `deadline`이 없으면 끝까지 기다린다
    pub fn acquire(
        self: &Arc<Self>,
        key: &str,
        deadline: Option<Instant>,
    ) -> Result<Arc<AuthenticationTurn>, CommandFailure> {
        let wait_timeout =
            deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
        let mut queues = self.queues.lock().unwrap();
        let queue = queues.entry(key.to_string()).or_default();
        let ticket = queue.next_ticket;
        queue.next_ticket += 1;
        loop {
            let serving = queues.get(key).map_or(ticket, |queue| queue.serving);
            if serving == ticket {
                return Ok(Arc::new(AuthenticationTurn {
                    queues: Arc::clone(self),
                    key: key.to_string(),
                    ticket,
                    is_released: AtomicBool::new(false),
                }));
            }
            let remaining =
                deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
            match remaining {
                Some(remaining) if remaining.is_zero() => {
                    drop(queues);
                    self.release(key, ticket);
                    let mut failure = timeout_error(wait_timeout.unwrap_or_default(), "", "");
                    failure.is_preparation_wait_timeout = true;
                    return Err(failure);
                }
                Some(remaining) => queues = self.changed.wait_timeout(queues, remaining).unwrap().0,
                None => queues = self.changed.wait(queues).unwrap(),
            }
        }
    }
}

/// 인증 차례. 응답기 marker를 받으면 인증이 끝난 것이므로 명령이 끝나기 전에도 일찍 놓는다
pub(crate) struct AuthenticationTurn {
    queues: Arc<AuthenticationQueues>,
    key: String,
    ticket: u64,
    is_released: AtomicBool,
}

impl AuthenticationTurn {
    pub fn release(&self) {
        if !self.is_released.swap(true, Ordering::SeqCst) {
            self.queues.release(&self.key, self.ticket);
        }
    }
}

impl Drop for AuthenticationTurn {
    fn drop(&mut self) {
        self.release();
    }
}

/// 같은 호스트 키를 공유하는 마스터 여러 개가 경쟁하지 않도록 인증 순서 키는 주소와 포트로 정한다
pub(crate) fn authentication_key(hostname: &str, port: i64) -> String {
    serde_json::json!([hostname, port]).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_priority_scope_restores_on_exit() {
        assert!(!is_background_priority());

        let was_background = run_with_background_ssh_commands(is_background_priority);

        assert!(was_background);
        assert!(!is_background_priority());
    }

    #[test]
    fn a_timed_out_authentication_waiter_keeps_its_place() {
        let queues = Arc::new(AuthenticationQueues::default());
        let first = queues.acquire("host", None).unwrap();

        let expired = queues.acquire("host", Some(Instant::now() + Duration::from_millis(10)));
        let third = std::thread::spawn({
            let queues = Arc::clone(&queues);
            move || queues.acquire("host", None).map(|turn| turn.ticket)
        });
        std::thread::sleep(Duration::from_millis(30));
        first.release();

        assert!(expired.err().unwrap().is_preparation_wait_timeout);
        assert_eq!(third.join().unwrap().unwrap(), 2);
    }
}
