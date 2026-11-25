//! 같은 호스트·세션의 탭 닫기를 하나씩 실행하는 대기열(TS `pendingTabCloses`).
//! 두 닫기가 동시에 목록을 읽으면 둘 다 탭이 둘이라고 보고 마지막 탭까지 지울 수 있다.
//! 앞선 닫기가 실패해도 다음 닫기는 그대로 실행한다.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use kanvibe_domain::task::SessionType;

/// 호스트, 세션 종류, 세션 이름(TS `JSON.stringify([sshHost, sessionType, sessionName])`)
pub(super) type TabCloseSessionKey = (Option<String>, SessionType, String);

#[derive(Default)]
pub(super) struct TabCloseQueues {
    turns_by_session: Mutex<HashMap<TabCloseSessionKey, Arc<Mutex<()>>>>,
}

impl TabCloseQueues {
    /// 같은 세션의 앞선 닫기가 끝난 뒤 `close`를 실행한다. 다른 세션의 닫기는 기다리지 않는다
    pub(super) fn run_in_turn<T>(
        &self,
        session_key: TabCloseSessionKey,
        close: impl FnOnce() -> T,
    ) -> T {
        let session_turn = self.join_session(&session_key);
        let result = {
            let _turn = session_turn
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            close()
        };
        self.leave_session(&session_key, session_turn);
        result
    }

    fn join_session(&self, session_key: &TabCloseSessionKey) -> Arc<Mutex<()>> {
        let mut turns_by_session = self.lock_turns_by_session();
        let session_turn = turns_by_session.entry(session_key.clone()).or_default();
        Arc::clone(session_turn)
    }

    /// 기다리는 닫기가 더 없으면 세션 항목을 지운다. 복제는 이 잠금 안에서만 일어나 참조 수가 정확하다
    fn leave_session(&self, session_key: &TabCloseSessionKey, session_turn: Arc<Mutex<()>>) {
        let mut turns_by_session = self.lock_turns_by_session();
        let is_last_waiter = Arc::strong_count(&session_turn) == 2;
        if is_last_waiter {
            turns_by_session.remove(session_key);
        }
    }

    fn lock_turns_by_session(&self) -> MutexGuard<'_, HashMap<TabCloseSessionKey, Arc<Mutex<()>>>> {
        self.turns_by_session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
