//! task worktree의 미커밋 변경 목록, 원본·현재 파일 내용, 저장, 보드 배지용 변경 집계(TS `diffService.ts`).
//! 보드는 집계 캐시를 먼저 받고, 갱신은 기다리는 사용자가 없으므로 백그라운드 원격 우선순위로 따로 돈다.
//! 같은 task의 조회와 집계 저장은 상세와 보드가 하나를 함께 기다려 늦은 응답이 새 집계를 덮어쓰지 않게 한다.

mod output;
mod paths;
#[cfg(test)]
mod tests;
mod workspace_file;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use kanvibe_db::Database;
use kanvibe_db::repositories::task_diff_stats::save_task_diff_stats;
use kanvibe_db::repositories::tasks::{find_task, select_tasks_where};
use kanvibe_domain::task_diff_stats::{DiffFile, TaskDiffStats, summarize_diff_files};
use kanvibe_process::exec::{ExecGitOptions, run_with_background_ssh_commands};
use kanvibe_process::host_file_access::{HostFileAccess, quote_shell_argument};
use serde::Serialize;

use self::output::{build_git_command, build_git_diff_files_command, parse_git_diff_files};
pub use self::workspace_file::{TaskWorkspaceFile, TaskWorkspaceFileFailure};
use crate::error::{ServiceError, ServiceResult};

/// 파일 저장 결과. 화면은 실패 문구를 그대로 보여 준다
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileSaveResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

struct TaskWorktree {
    worktree_path: String,
    ssh_host: Option<String>,
}

/// 진행 중인 조회 하나. `None`은 조회 실패라 캐시를 덮어쓰지 않는다
type DiffRefreshResult = Option<Vec<DiffFile>>;

#[derive(Default)]
struct PendingDiffRefresh {
    result: Mutex<Option<DiffRefreshResult>>,
    is_done: Condvar,
}

impl PendingDiffRefresh {
    fn finish(&self, result: DiffRefreshResult) {
        *self.result.lock().unwrap() = Some(result);
        self.is_done.notify_all();
    }

    fn wait(&self) -> DiffRefreshResult {
        let guard = self.result.lock().unwrap();
        let guard = self
            .is_done
            .wait_while(guard, |result| result.is_none())
            .unwrap();
        guard.clone().flatten()
    }
}

/// 원격 명령을 백그라운드 우선순위로 실행하는 범위. 테스트는 우선순위를 관찰하는 범위로 바꿔 끼운다
type BackgroundScope = fn(&mut dyn FnMut());

fn run_in_background_ssh_scope(operation: &mut dyn FnMut()) {
    run_with_background_ssh_commands(operation);
}

/// 보드 배지 집계 갱신을 동시에 돌리는 worker 수. 원격 task가 많아도 OS 스레드를 task 수만큼 만들지 않는다
const DIFF_REFRESH_CONCURRENCY: usize = 8;

/// 예약한 집계 갱신. worker가 앞에서부터 꺼내 돌린다
#[derive(Default)]
struct DiffRefreshQueue {
    waiting: VecDeque<String>,
    /// 기다리거나 갱신 중인 task. 끝나기 전에는 다시 예약하지 않는다
    scheduled: HashSet<String>,
    worker_count: usize,
}

struct DiffServiceShared {
    database: Arc<Database>,
    files: Arc<HostFileAccess>,
    pending_refreshes: Mutex<HashMap<String, Arc<PendingDiffRefresh>>>,
    refresh_queue: Mutex<DiffRefreshQueue>,
    background_scope: BackgroundScope,
}

#[derive(Clone)]
pub struct DiffService {
    shared: Arc<DiffServiceShared>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 파일 경로가 worktree 안에 있는지 확인하고 절대 경로로 바꾼다. 상위 경로 탐색과 절대 경로를 막는다
fn validate_file_path(worktree_path: &str, file_path: &str) -> ServiceResult<String> {
    if paths::is_absolute(file_path) {
        return Err(ServiceError::new(
            "잘못된 파일 경로: 절대 경로는 허용되지 않습니다",
        ));
    }
    let resolved_path = paths::resolve(worktree_path, file_path);
    let relative_path = paths::relative(worktree_path, &resolved_path);
    if relative_path == ".."
        || relative_path.starts_with("../")
        || paths::is_absolute(&relative_path)
    {
        return Err(ServiceError::new(
            "잘못된 파일 경로: worktree 외부 접근이 감지되었습니다",
        ));
    }
    Ok(resolved_path)
}

impl DiffService {
    pub fn new(database: Arc<Database>, files: Arc<HostFileAccess>) -> Self {
        Self::with_background_scope(database, files, run_in_background_ssh_scope)
    }

    fn with_background_scope(
        database: Arc<Database>,
        files: Arc<HostFileAccess>,
        background_scope: BackgroundScope,
    ) -> Self {
        Self {
            shared: Arc::new(DiffServiceShared {
                database,
                files,
                pending_refreshes: Mutex::new(HashMap::new()),
                refresh_queue: Mutex::default(),
                background_scope,
            }),
        }
    }

    /// 태스크의 worktree 경로와 실행 호스트. worktree나 브랜치가 없는 태스크는 변경을 볼 수 없다
    fn task_worktree(&self, task_id: &str) -> ServiceResult<TaskWorktree> {
        let task = find_task(&self.shared.database.connection(), task_id)?
            .ok_or_else(|| ServiceError::new("태스크를 찾을 수 없습니다"))?;
        let worktree_path = task
            .worktree_path
            .filter(|path| !path.is_empty())
            .ok_or_else(|| ServiceError::new("worktree 경로가 설정되지 않은 태스크입니다"))?;
        if task.branch_name.as_deref().is_none_or(str::is_empty) {
            return Err(ServiceError::new("브랜치가 설정되지 않은 태스크입니다"));
        }
        Ok(TaskWorktree {
            worktree_path,
            ssh_host: task.ssh_host.filter(|host| !host.is_empty()),
        })
    }

    fn exec(
        &self,
        command: &str,
        ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> ServiceResult<String> {
        Ok(self
            .shared
            .files
            .runner()
            .exec_git(command, ssh_host, options)?)
    }

    /// 조회 실패와 정말 아무것도 바뀌지 않은 것을 구분해, 실패는 `None`으로 돌려준다
    fn read_git_diff_files(&self, task_id: &str) -> DiffRefreshResult {
        let read = || -> ServiceResult<Vec<DiffFile>> {
            let worktree = self.task_worktree(task_id)?;
            let command = build_git_diff_files_command(&worktree.worktree_path);
            let output = self.exec(
                &command,
                worktree.ssh_host.as_deref(),
                &ExecGitOptions::default(),
            )?;
            Ok(parse_git_diff_files(&output))
        };
        match read() {
            Ok(files) => Some(files),
            Err(error) => {
                eprintln!("git diff 파일 목록 조회 실패: {error}");
                None
            }
        }
    }

    /// HEAD 이후의 미커밋 변경 파일(untracked·unstaged·staged)을 조회하고, 그 김에 보드 배지 집계도 갱신한다
    pub fn get_git_diff_files(&self, task_id: &str) -> Vec<DiffFile> {
        self.refresh_task_diff_files(task_id).unwrap_or_default()
    }

    /// 같은 task의 조회가 진행 중이면 그 결과를 기다리고, 아니면 조회한 뒤 집계까지 저장하고 끝낸다
    fn refresh_task_diff_files(&self, task_id: &str) -> DiffRefreshResult {
        let (pending, is_leader) = {
            let mut refreshes = lock(&self.shared.pending_refreshes);
            match refreshes.get(task_id) {
                Some(pending) => (Arc::clone(pending), false),
                None => {
                    let pending = Arc::new(PendingDiffRefresh::default());
                    refreshes.insert(task_id.to_string(), Arc::clone(&pending));
                    (pending, true)
                }
            }
        };
        if !is_leader {
            return pending.wait();
        }
        let files = self.read_git_diff_files(task_id);
        if let Some(files) = &files {
            self.save_task_diff_stats(task_id, summarize_diff_files(files));
        }
        lock(&self.shared.pending_refreshes).remove(task_id);
        pending.finish(files.clone());
        files
    }

    /// 집계 저장 실패는 기록만 하고 파일 목록 조회는 유지한다
    fn save_task_diff_stats(&self, task_id: &str, stats: TaskDiffStats) {
        if let Err(error) = save_task_diff_stats(&self.shared.database.connection(), task_id, stats)
        {
            eprintln!("변경 집계 저장 실패: {error}");
        }
    }

    fn load_cached_task_diff_stats(&self) -> BTreeMap<String, TaskDiffStats> {
        let connection = self.shared.database.connection();
        let rows = connection
            .prepare("SELECT task_id, file_count, additions, deletions FROM task_diff_stats")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| {
                        let stats = TaskDiffStats {
                            file_count: row.get(1)?,
                            additions: row.get(2)?,
                            deletions: row.get(3)?,
                        };
                        Ok((row.get::<_, String>(0)?, stats))
                    })?
                    .collect::<Result<BTreeMap<_, _>, _>>()
            });
        rows.unwrap_or_else(|error| {
            eprintln!("변경 집계 캐시 조회 실패: {error}");
            BTreeMap::new()
        })
    }

    /// 진행 중이 아니고 DB에 남아 있는 task만 갱신을 예약한다. 지워진 task는 폴링마다 실패할 조회를 내지 않는다.
    /// 살아 있는지는 한 번의 조회로 확인하고(TS `repo.find({ where: { id: In(...) } })`), 갱신은 상한이 있는 worker가 돌린다
    fn schedule_task_diff_refreshes(&self, task_ids: &[String]) {
        let unscheduled_task_ids = self.unscheduled_task_ids(task_ids);
        if unscheduled_task_ids.is_empty() {
            return;
        }
        let alive_task_ids = match self.find_alive_task_ids(&unscheduled_task_ids) {
            Ok(alive_task_ids) => alive_task_ids,
            Err(error) => {
                eprintln!("변경 집계 갱신 대상 조회 실패: {error}");
                return;
            }
        };
        let workers_to_start = {
            let mut queue = lock(&self.shared.refresh_queue);
            for task_id in unscheduled_task_ids {
                if alive_task_ids.contains(&task_id) && queue.scheduled.insert(task_id.clone()) {
                    queue.waiting.push_back(task_id);
                }
            }
            let workers_to_start = DIFF_REFRESH_CONCURRENCY
                .saturating_sub(queue.worker_count)
                .min(queue.waiting.len());
            queue.worker_count += workers_to_start;
            workers_to_start
        };
        for _ in 0..workers_to_start {
            let service = self.clone();
            std::thread::spawn(move || service.run_diff_refresh_worker());
        }
    }

    /// 순서를 지켜 중복을 빼고, 진행 중이거나 이미 예약한 task를 뺀다
    fn unscheduled_task_ids(&self, task_ids: &[String]) -> Vec<String> {
        let pending_refreshes = lock(&self.shared.pending_refreshes);
        let queue = lock(&self.shared.refresh_queue);
        let mut seen = HashSet::new();
        task_ids
            .iter()
            .filter(|task_id| seen.insert(task_id.as_str()))
            .filter(|task_id| {
                !pending_refreshes.contains_key(*task_id) && !queue.scheduled.contains(*task_id)
            })
            .cloned()
            .collect()
    }

    fn find_alive_task_ids(&self, task_ids: &[String]) -> ServiceResult<HashSet<String>> {
        let placeholders = vec!["?"; task_ids.len()].join(", ");
        let parameters: Vec<&dyn rusqlite::ToSql> = task_ids
            .iter()
            .map(|task_id| task_id as &dyn rusqlite::ToSql)
            .collect();
        let alive_tasks = select_tasks_where(
            &self.shared.database.connection(),
            &format!("WHERE id IN ({placeholders})"),
            &parameters,
        )?;
        Ok(alive_tasks.into_iter().map(|task| task.id).collect())
    }

    /// 예약이 빌 때까지 하나씩 꺼내 갱신한다. 빈 것을 본 worker는 같은 잠금 안에서 수를 줄이고 끝난다
    fn run_diff_refresh_worker(&self) {
        loop {
            let task_id = {
                let mut queue = lock(&self.shared.refresh_queue);
                match queue.waiting.pop_front() {
                    Some(task_id) => task_id,
                    None => {
                        queue.worker_count -= 1;
                        return;
                    }
                }
            };
            (self.shared.background_scope)(&mut || {
                self.refresh_task_diff_files(&task_id);
            });
            lock(&self.shared.refresh_queue).scheduled.remove(&task_id);
        }
    }

    /// 저장된 집계를 먼저 돌려준다. 시작한 갱신은 끝나는 대로 다음 조회부터 항목별로 보인다
    pub fn get_task_diff_stats(
        &self,
        task_ids_to_refresh: &[String],
    ) -> BTreeMap<String, TaskDiffStats> {
        let cached_stats = self.load_cached_task_diff_stats();
        self.schedule_task_diff_refreshes(task_ids_to_refresh);
        cached_stats
    }

    /// HEAD 기준 원본 내용. HEAD에 없는 새 파일이거나 조회에 실패하면 빈 문자열이다
    pub fn get_original_file_content(&self, task_id: &str, file_path: &str) -> String {
        let read = || -> ServiceResult<String> {
            let worktree = self.task_worktree(task_id)?;
            validate_file_path(&worktree.worktree_path, file_path)?;
            let revision = quote_shell_argument(&format!("HEAD:{file_path}"));
            let command = build_git_command(&worktree.worktree_path, &["show", &revision]);
            self.exec(
                &command,
                worktree.ssh_host.as_deref(),
                &ExecGitOptions::default(),
            )
        };
        read().unwrap_or_default()
    }

    /// worktree의 현재 파일 내용. 읽지 못하면 빈 문자열이다
    pub fn get_file_content(&self, task_id: &str, file_path: &str) -> String {
        let read = || -> ServiceResult<String> {
            let worktree = self.task_worktree(task_id)?;
            let resolved_path = validate_file_path(&worktree.worktree_path, file_path)?;
            Ok(self.shared.files.read_text_file(
                &resolved_path,
                worktree.ssh_host.as_deref(),
                &ExecGitOptions::default(),
            )?)
        };
        read().unwrap_or_else(|error| {
            eprintln!("파일 내용 읽기 실패: {error}");
            String::new()
        })
    }

    pub fn save_file_content(
        &self,
        task_id: &str,
        file_path: &str,
        content: &str,
    ) -> FileSaveResult {
        let save = || -> ServiceResult<()> {
            let worktree = self.task_worktree(task_id)?;
            let resolved_path = validate_file_path(&worktree.worktree_path, file_path)?;
            Ok(self.shared.files.write_text_file(
                &resolved_path,
                content,
                worktree.ssh_host.as_deref(),
                &ExecGitOptions::default(),
            )?)
        };
        match save() {
            Ok(()) => FileSaveResult {
                success: true,
                error: None,
            },
            Err(error) => {
                eprintln!("파일 저장 실패: {error}");
                FileSaveResult {
                    success: false,
                    error: Some(error.message),
                }
            }
        }
    }
}
