//! 프로젝트 worktree 동기화 작업 공유와 호스트 직렬화(TS `projectWorktreeSyncJobs`, `projectWorktreeSyncHosts`).
//! 같은 프로젝트를 동시에 요청하면 진행 중인 스캔 하나를 함께 기다리고, 같은 호스트의 스캔은 차례로 돈다.
//! 다른 호스트끼리는 서로 기다리지 않는다.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};

use super::RegisteredProjectWorktreeSyncResult;

/// 진행 중인 스캔 하나. 끝나면 결과를 남기고 기다리는 호출자를 깨운다
#[derive(Default)]
struct SharedScan {
    result: Mutex<Option<RegisteredProjectWorktreeSyncResult>>,
    finished: Condvar,
}

impl SharedScan {
    fn wait(&self) -> RegisteredProjectWorktreeSyncResult {
        let mut result = self.result.lock().unwrap();
        while result.is_none() {
            result = self.finished.wait(result).unwrap();
        }
        result.clone().expect("끝난 스캔에는 결과가 있다")
    }

    fn finish(&self, value: RegisteredProjectWorktreeSyncResult) {
        *self.result.lock().unwrap() = Some(value);
        self.finished.notify_all();
    }
}

#[derive(Default)]
pub(super) struct WorktreeSyncJobs {
    scans: Mutex<HashMap<String, Arc<SharedScan>>>,
    host_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

/// 패닉으로 결과 없이 끝나도 기다리는 호출자가 멈추지 않게 빈 결과로 끝낸다
struct ScanCompletion<'a> {
    jobs: &'a WorktreeSyncJobs,
    project_key: &'a str,
    scan: Arc<SharedScan>,
    result: Option<RegisteredProjectWorktreeSyncResult>,
}

impl Drop for ScanCompletion<'_> {
    fn drop(&mut self) {
        let result = self.result.take().unwrap_or_default();
        let mut scans = self.jobs.scans.lock().unwrap();
        if scans
            .get(self.project_key)
            .is_some_and(|current| Arc::ptr_eq(current, &self.scan))
        {
            scans.remove(self.project_key);
        }
        drop(scans);
        self.scan.finish(result);
    }
}

impl WorktreeSyncJobs {
    fn host_lock(&self, host_key: &str) -> Arc<Mutex<()>> {
        let mut locks = self.host_locks.lock().unwrap();
        Arc::clone(locks.entry(host_key.to_string()).or_default())
    }

    /// 같은 프로젝트 스캔이 진행 중이면 그 결과를 기다리고, 아니면 호스트 차례를 기다렸다가 `scan`을 실행한다
    pub(super) fn run(
        &self,
        project_key: &str,
        host_key: &str,
        scan: impl FnOnce() -> RegisteredProjectWorktreeSyncResult,
    ) -> RegisteredProjectWorktreeSyncResult {
        let (shared, is_owner) = {
            let mut scans = self.scans.lock().unwrap();
            match scans.get(project_key) {
                Some(existing) => (Arc::clone(existing), false),
                None => {
                    let created = Arc::new(SharedScan::default());
                    scans.insert(project_key.to_string(), Arc::clone(&created));
                    (created, true)
                }
            }
        };
        if !is_owner {
            return shared.wait();
        }

        let mut completion = ScanCompletion {
            jobs: self,
            project_key,
            scan: Arc::clone(&shared),
            result: None,
        };
        let host_lock = self.host_lock(host_key);
        let _host_turn = host_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = scan();
        completion.result = Some(result.clone());
        result
    }
}
