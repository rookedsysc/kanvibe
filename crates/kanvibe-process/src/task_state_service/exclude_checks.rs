//! 저장소·호스트별 git exclude 확인을 기억한다.
//! 성공한 확인은 정해진 시간 동안 다시 하지 않고, 진행 중인 확인에 겹친 호출은 그 확인이 끝나기를 함께 기다린다.
//! 실패한 확인은 기억하지 않아 다음 기록이 다시 시도한다. 누군가 exclude를 지워도 기억 시간이 지나면 복구된다.

use std::collections::HashMap;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use super::MonotonicClock;

pub(super) const STATE_DIRECTORY_EXCLUDE_RECHECK: Duration = Duration::from_secs(10 * 60);

/// 확인 하나. `generation`으로 기다리던 확인이 끝났는지(성공해 남았든 실패해 지워졌든) 가린다
struct ExcludeCheck {
    started_at: Instant,
    generation: u64,
    is_finished: bool,
}

pub(super) struct StateDirectoryExcludeChecks {
    checks: Mutex<ExcludeCheckTable>,
    finished: Condvar,
    clock: Box<MonotonicClock>,
}

#[derive(Default)]
struct ExcludeCheckTable {
    by_key: HashMap<(Option<String>, String), ExcludeCheck>,
    next_generation: u64,
}

enum CheckDecision {
    Skip,
    Run(u64),
}

impl StateDirectoryExcludeChecks {
    pub(super) fn new(clock: Box<MonotonicClock>) -> Self {
        Self {
            checks: Mutex::new(ExcludeCheckTable::default()),
            finished: Condvar::new(),
            clock,
        }
    }

    /// 최근에 성공했거나 지금 진행 중인 확인이 없을 때만 `check`를 부른다. `check`는 성공 여부를 돌려준다
    pub(super) fn run_once(
        &self,
        repo_path: &str,
        ssh_host: Option<&str>,
        check: impl FnOnce() -> bool,
    ) {
        let key = (ssh_host.map(str::to_string), repo_path.to_string());
        let CheckDecision::Run(generation) = self.decide(&key) else {
            return;
        };
        let is_successful = check();
        self.finish(&key, generation, is_successful);
    }

    fn decide(&self, key: &(Option<String>, String)) -> CheckDecision {
        let mut table = self.checks.lock().unwrap();
        let mut joined_generation = None;
        while let Some(check) = table.by_key.get(key) {
            if !check.is_finished {
                joined_generation = Some(check.generation);
                table = self.finished.wait(table).unwrap();
                continue;
            }
            let is_joined_check = joined_generation == Some(check.generation);
            let is_recent = (self.clock)().saturating_duration_since(check.started_at)
                < STATE_DIRECTORY_EXCLUDE_RECHECK;
            if is_joined_check || is_recent {
                return CheckDecision::Skip;
            }
            break;
        }
        // 기다리던 확인이 실패해 지워졌으면 그 확인을 함께 기다린 것으로 끝낸다
        if joined_generation.is_some() {
            return CheckDecision::Skip;
        }
        table.next_generation += 1;
        let generation = table.next_generation;
        let started_at = (self.clock)();
        table.by_key.insert(
            key.clone(),
            ExcludeCheck {
                started_at,
                generation,
                is_finished: false,
            },
        );
        CheckDecision::Run(generation)
    }

    fn finish(&self, key: &(Option<String>, String), generation: u64, is_successful: bool) {
        let mut table = self.checks.lock().unwrap();
        let is_same_check = table
            .by_key
            .get(key)
            .is_some_and(|check| check.generation == generation);
        if is_same_check && is_successful {
            if let Some(check) = table.by_key.get_mut(key) {
                check.is_finished = true;
            }
        } else if is_same_check {
            table.by_key.remove(key);
        }
        self.finished.notify_all();
    }
}
