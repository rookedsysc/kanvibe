//! 저장소의 `.kanvibe/status.json`에 남은 상태를 DB task에 반영한다.
//! 파일을 읽는 동안 사용자가 task를 바꿨을 수 있으므로, 읽기 전에 본 상태와 수정 시각이 그대로일 때만 바꾼다(compare-and-set).

use std::collections::{HashMap, HashSet};

use kanvibe_db::Database;
use kanvibe_domain::task::TaskStatus;

use super::BoardUpdateNotifier;
use crate::kanvibe_project_state::{get_kanvibe_task_state_path, parse_kanvibe_task_state};
use crate::kanvibe_state_store::KanvibeStateStore;

/// 상태 반영에 필요한 task 스냅샷. `updated_at`은 DB에 저장된 문자열 그대로다
#[derive(Debug, Clone)]
struct TaskStatusSnapshot {
    id: String,
    status: TaskStatus,
    updated_at: String,
    /// 상태 파일이 있는 저장소. worktree가 없으면 프로젝트 기본 브랜치 task만 프로젝트 루트를 쓴다
    repo_path: Option<String>,
    ssh_host: Option<String>,
}

/// 한 호스트에서 반영할 task와 그 상태 파일 경로
type HostTaskStatePaths = Vec<(TaskStatusSnapshot, String)>;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskStatusSyncResult {
    pub has_changed: bool,
    pub errors: Vec<String>,
}

pub struct TaskStatusSync<'a> {
    database: &'a Database,
    state_store: &'a dyn KanvibeStateStore,
    board_notifier: &'a dyn BoardUpdateNotifier,
}

const SNAPSHOT_SQL: &str = "SELECT t.id, t.status, t.updated_at, t.worktree_path, t.branch_name, t.ssh_host, \
     p.repo_path, p.default_branch, p.ssh_host FROM kanban_tasks t \
     LEFT JOIN projects p ON p.id = t.project_id ORDER BY t.rowid";

/// TS `repository.update({ id, status, updatedAt: Raw(julianday = julianday(snapshot)) }, { status })`.
/// TypeORM처럼 수정 시각도 지금으로 바꾼다
const COMPARE_AND_SET_STATUS_SQL: &str = "UPDATE kanban_tasks SET status = ?1, updated_at = datetime('now') \
     WHERE id = ?2 AND status = ?3 AND julianday(updated_at) = julianday(?4)";

fn snapshot_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(TaskStatusSnapshot, String)> {
    let status_name: String = row.get(1)?;
    let worktree_path: Option<String> = row.get(3)?;
    let branch_name: Option<String> = row.get(4)?;
    let project_repo_path: Option<String> = row.get(6)?;
    let project_default_branch: Option<String> = row.get(7)?;
    let task_ssh_host: Option<String> = row.get(5)?;
    let project_ssh_host: Option<String> = row.get(8)?;
    let is_project_root_task = branch_name.is_some() && branch_name == project_default_branch;
    let repo_path = worktree_path
        .filter(|path| !path.is_empty())
        .or_else(|| project_repo_path.filter(|_| is_project_root_task));
    let snapshot = TaskStatusSnapshot {
        id: row.get(0)?,
        status: TaskStatus::parse(&status_name).unwrap_or(TaskStatus::Todo),
        updated_at: row.get(2)?,
        repo_path,
        ssh_host: task_ssh_host.or(project_ssh_host),
    };
    Ok((snapshot, status_name))
}

impl<'a> TaskStatusSync<'a> {
    pub fn new(
        database: &'a Database,
        state_store: &'a dyn KanvibeStateStore,
        board_notifier: &'a dyn BoardUpdateNotifier,
    ) -> Self {
        Self {
            database,
            state_store,
            board_notifier,
        }
    }

    /// 저장된 순서의 task 스냅샷. 알 수 없는 상태 값의 task는 반영 대상에서 뺀다
    fn read_task_snapshots(&self) -> Result<Vec<TaskStatusSnapshot>, String> {
        let connection = self.database.connection();
        let mut statement = connection
            .prepare(SNAPSHOT_SQL)
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], snapshot_from_row)
            .map_err(|error| error.to_string())?;
        let mut snapshots = Vec::new();
        for row in rows {
            let (snapshot, status_name) = row.map_err(|error| error.to_string())?;
            if TaskStatus::parse(&status_name).is_some() {
                snapshots.push(snapshot);
            }
        }
        Ok(snapshots)
    }

    /// 읽기 전에 본 상태와 수정 시각이 그대로일 때만 바꾸고, 바뀐 행이 있는지 돌려준다
    fn compare_and_set_status(
        &self,
        snapshot: &TaskStatusSnapshot,
        status: TaskStatus,
    ) -> Result<bool, String> {
        let connection = self.database.connection();
        let affected_rows = connection
            .execute(
                COMPARE_AND_SET_STATUS_SQL,
                (
                    status.as_str(),
                    &snapshot.id,
                    snapshot.status.as_str(),
                    &snapshot.updated_at,
                ),
            )
            .map_err(|error| error.to_string())?;
        Ok(affected_rows > 0)
    }

    /// 모든 task의 상태 파일을 호스트마다 한 번의 배치 읽기로 가져와 DB에 반영한다.
    /// SSH 인증 큐를 task 수만큼 점유하지 않도록 호스트별로 묶고, 호스트끼리는 함께 읽는다.
    pub fn sync_task_statuses_from_files(&self) -> Result<TaskStatusSyncResult, String> {
        let tasks_by_host = group_by_host(self.read_task_snapshots()?);
        let host_results: Vec<Result<bool, String>> = std::thread::scope(|scope| {
            let workers: Vec<_> = tasks_by_host
                .iter()
                .map(|(ssh_host, host_tasks)| {
                    scope.spawn(move || self.sync_host_statuses(ssh_host.as_deref(), host_tasks))
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().expect("호스트 상태 반영 스레드"))
                .collect()
        });

        let mut result = TaskStatusSyncResult::default();
        for (host_result, (ssh_host, _)) in host_results.into_iter().zip(&tasks_by_host) {
            match host_result {
                Ok(has_changed) => result.has_changed = has_changed || result.has_changed,
                Err(message) => result.errors.push(format!(
                    "{} task 상태 sync 실패: {message}",
                    ssh_host.as_deref().unwrap_or("local")
                )),
            }
        }
        Ok(result)
    }

    fn sync_host_statuses(
        &self,
        ssh_host: Option<&str>,
        host_tasks: &[(TaskStatusSnapshot, String)],
    ) -> Result<bool, String> {
        let mut paths: Vec<String> = Vec::new();
        for (_, state_path) in host_tasks {
            if !paths.contains(state_path) {
                paths.push(state_path.clone());
            }
        }
        let files = self
            .state_store
            .read_text_files(&paths, ssh_host)
            .map_err(|error| error.to_string())?;

        let mut has_changed = false;
        for (snapshot, state_path) in host_tasks {
            let content = files
                .get(state_path)
                .map(|file| file.content.as_str())
                .unwrap_or_default();
            let Some(status) = parse_kanvibe_task_state(content).map(|state| state.status) else {
                continue;
            };
            if status != snapshot.status {
                has_changed = self.compare_and_set_status(snapshot, status)? || has_changed;
            }
        }
        Ok(has_changed)
    }

    /// 다시 연결된 호스트의 task 상태 파일을 읽어 DB에 반영한다. `is_current`가 거짓이 되면 더 반영하지 않는다
    pub fn refresh_host_task_statuses(
        &self,
        ssh_hosts: &HashSet<String>,
        is_current: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        let mut has_changed = false;
        for snapshot in self.read_task_snapshots()? {
            let Some(ssh_host) = snapshot.ssh_host.as_deref().filter(|host| !host.is_empty())
            else {
                continue;
            };
            if !ssh_hosts.contains(ssh_host) || !is_current() {
                continue;
            }
            let Some(repo_path) = snapshot.repo_path.as_deref() else {
                continue;
            };
            match self.refresh_task_status(&snapshot, repo_path, ssh_host, is_current) {
                Ok(is_updated) => has_changed = has_changed || is_updated,
                Err(error) => eprintln!(
                    "[kanvibe] Host status snapshot failed: {{ taskId: {:?}, sshHost: {ssh_host:?}, error: {error:?} }}",
                    snapshot.id
                ),
            }
        }
        if has_changed && is_current() {
            self.board_notifier.broadcast_board_update();
        }
        Ok(())
    }

    fn refresh_task_status(
        &self,
        snapshot: &TaskStatusSnapshot,
        repo_path: &str,
        ssh_host: &str,
        is_current: &dyn Fn() -> bool,
    ) -> Result<bool, String> {
        let task_state = self
            .state_store
            .read_task_state(repo_path, Some(ssh_host))
            .map_err(|error| error.to_string())?;
        let Some(status) = task_state.map(|state| state.status) else {
            return Ok(false);
        };
        if status == snapshot.status || !is_current() {
            return Ok(false);
        }
        self.compare_and_set_status(snapshot, status)
    }
}

/// 호스트별로 task와 상태 파일 경로를 처음 나온 호스트 순서대로 묶는다. 저장소 경로가 없는 task는 뺀다
fn group_by_host(snapshots: Vec<TaskStatusSnapshot>) -> Vec<(Option<String>, HostTaskStatePaths)> {
    let mut host_order: Vec<Option<String>> = Vec::new();
    let mut tasks_by_host: HashMap<Option<String>, Vec<(TaskStatusSnapshot, String)>> =
        HashMap::new();
    for snapshot in snapshots {
        let Some(repo_path) = snapshot.repo_path.as_deref() else {
            continue;
        };
        let state_path = get_kanvibe_task_state_path(repo_path);
        if !tasks_by_host.contains_key(&snapshot.ssh_host) {
            host_order.push(snapshot.ssh_host.clone());
        }
        tasks_by_host
            .entry(snapshot.ssh_host.clone())
            .or_default()
            .push((snapshot, state_path));
    }
    host_order
        .into_iter()
        .map(|ssh_host| {
            let host_tasks = tasks_by_host.remove(&ssh_host).unwrap_or_default();
            (ssh_host, host_tasks)
        })
        .collect()
}
