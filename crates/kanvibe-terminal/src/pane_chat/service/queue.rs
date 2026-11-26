//! 태스크별 입력 큐와 여러 호출자가 함께 기다리는 결과.
//!
//! TS는 태스크마다 Promise 사슬로 입력을 한 줄로 세웠다. 여기서는 번호표로 같은 순서(먼저 온 동작이 먼저)를 지키고,
//! 같은 요청 식별자나 같은 공유 조회를 기다리는 호출자는 먼저 시작한 쪽의 결과를 함께 받는다.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};

use super::{PaneChatError, PaneChatService, lock};

/// 한 번 정해지면 바뀌지 않는 결과. 정해지기 전에 부른 쪽은 정해질 때까지 기다린다(TS 공유 Promise)
pub(super) struct SharedOutcome<T> {
    value: Mutex<Option<T>>,
    settled: Condvar,
}

impl<T: Clone> SharedOutcome<T> {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            value: Mutex::new(None),
            settled: Condvar::new(),
        })
    }

    pub(super) fn settle(&self, value: T) {
        *lock(&self.value) = Some(value);
        self.settled.notify_all();
    }

    pub(super) fn wait(&self) -> T {
        let mut value = lock(&self.value);
        loop {
            if let Some(settled) = value.as_ref() {
                return settled.clone();
            }
            value = self
                .settled
                .wait(value)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

/// 요청 식별자 하나로 받은 전송. 같은 식별자·같은 내용의 재요청은 이 결과를 그대로 돌려받는다
#[derive(Clone)]
pub(super) struct Receipt {
    pub fingerprint: String,
    pub outcome: Arc<SharedOutcome<Result<(), PaneChatError>>>,
}

/// 태스크 하나의 번호표. `next`는 다음에 줄 번호, `serving`은 지금 실행 중이거나 다음에 실행할 번호다
struct TaskTickets {
    next: u64,
    serving: u64,
}

#[derive(Default)]
pub(super) struct MutationQueue {
    tickets: Mutex<HashMap<String, TaskTickets>>,
    turn_changed: Condvar,
}

/// 동작이 패닉으로 끝나도 다음 번호로 넘긴다
struct TurnRelease<'a> {
    queue: &'a MutationQueue,
    task_id: &'a str,
}

impl Drop for TurnRelease<'_> {
    fn drop(&mut self) {
        let mut tickets = lock(&self.queue.tickets);
        if let Some(entry) = tickets.get_mut(self.task_id) {
            entry.serving += 1;
            if entry.serving == entry.next {
                tickets.remove(self.task_id);
            }
        }
        self.queue.turn_changed.notify_all();
    }
}

impl MutationQueue {
    /// 이 태스크에 줄 선 동작이 있는지(TS `mutations.has(taskId)`)
    pub(super) fn is_active(&self, task_id: &str) -> bool {
        lock(&self.tickets).contains_key(task_id)
    }

    /// 앞선 동작이 성공했든 실패했든 끝나기를 기다렸다가 실행한다
    pub(super) fn run<T>(&self, task_id: &str, action: impl FnOnce() -> T) -> T {
        let mut tickets = lock(&self.tickets);
        let entry = tickets.entry(task_id.to_string()).or_insert(TaskTickets {
            next: 0,
            serving: 0,
        });
        let ticket = entry.next;
        entry.next += 1;
        while tickets
            .get(task_id)
            .is_some_and(|entry| entry.serving != ticket)
        {
            tickets = self
                .turn_changed
                .wait(tickets)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        drop(tickets);
        let _release = TurnRelease {
            queue: self,
            task_id,
        };
        action()
    }
}

impl PaneChatService {
    /// 태스크의 입력 큐에서 동작을 실행한다. 끝나면 입력 전에 읽은 공유 조회를 버린다.
    /// 그 결과를 재사용하면 방금 보낸 내용이 다음 주기까지 보이지 않는다
    pub(super) fn queue_mutation(
        &self,
        task_id: &str,
        action: impl FnOnce() -> Result<(), PaneChatError>,
    ) -> Result<(), PaneChatError> {
        let result = self.mutations.run(task_id, action);
        self.forget_shared_readings(task_id);
        result
    }

    /// 이 태스크의 pane에 프롬프트나 명령을 치고 있는 중인지. 그동안의 입력란은 사용자가 아니라 KanVibe가 채운 것이다
    pub fn is_pane_chat_action_in_progress(&self, task_id: &str) -> bool {
        self.mutations.is_active(task_id)
    }

    pub(super) fn forget_receipts(&self, owner: &str) {
        lock(&self.receipts).remove(owner);
    }
}
