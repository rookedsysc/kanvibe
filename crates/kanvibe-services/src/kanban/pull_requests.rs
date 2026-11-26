//! task 브랜치의 PR URL 조회·저장과 활성 task PR 상태 동기화.
//! 조회하는 동안 사용자가 task를 바꿨을 수 있으므로 저장은 id와 브랜치가 그대로일 때만 PR URL 하나를 바꾼다.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use kanvibe_domain::notifications::{
    BackgroundSyncFailurePayload, BackgroundSyncOperation, TaskPrMergedDetectedPayload,
};
use kanvibe_domain::project::Project;
use kanvibe_domain::task::KanbanTask;

use super::github_cli::{GitHubCliAuthFailures, GitHubCliQuery, resolve_task_git_context};
use super::records::{
    find_project_by_id, find_task_by_id, select_tasks, task_exists_with_branch,
    update_pr_url_if_branch_unchanged,
};
use super::{ActiveTaskPullRequestSyncResult, KanbanService};
use crate::error::{ServiceError, ServiceResult};

/// 보드 갱신마다 따라 도는 자동 PR 조회를 task당 이 간격에 한 번으로 줄인다
const AUTOMATIC_PR_LOOKUP_MIN_INTERVAL: Duration = Duration::from_secs(30);

type LookupResult = ServiceResult<Option<String>>;

/// 여러 창이 함께 기다리는 조회 하나
#[derive(Default)]
struct SharedLookup {
    result: Mutex<Option<LookupResult>>,
    finished: Condvar,
}

impl SharedLookup {
    fn wait(&self) -> LookupResult {
        let mut result = self.result.lock().unwrap();
        while result.is_none() {
            result = self.finished.wait(result).unwrap();
        }
        result.clone().expect("끝난 조회는 결과가 있다")
    }

    fn finish(&self, lookup_result: LookupResult) {
        *self.result.lock().unwrap() = Some(lookup_result);
        self.finished.notify_all();
    }
}

struct ActiveLookup {
    is_automatic: bool,
    shared: Arc<SharedLookup>,
}

/// PR 조회의 중복 합류·자동 조회 간격·인증 실패 기억(TS 모듈 전역 Map들)
#[derive(Default)]
pub(super) struct PullRequestLookups {
    pub(super) auth_failures: GitHubCliAuthFailures,
    automatic_attempts: Mutex<HashMap<String, Instant>>,
    active: Mutex<HashMap<String, ActiveLookup>>,
}

/// 새로 시작한 조회인지, 이미 도는 조회에 합류했는지
enum LookupStart {
    Started(Arc<SharedLookup>),
    Joined(Arc<SharedLookup>),
    Throttled,
}

pub(super) fn has_branch(task: &KanbanTask) -> bool {
    task.branch_name
        .as_deref()
        .is_some_and(|branch| !branch.is_empty())
}

pub(super) fn is_default_branch_task(task: &KanbanTask, project: Option<&Project>) -> bool {
    matches!((project, task.branch_name.as_deref()), (Some(project), Some(branch)) if branch == project.default_branch)
}

fn build_pull_request_sync_failure(
    task: &KanbanTask,
    error: &ServiceError,
) -> BackgroundSyncFailurePayload {
    let branch_name = task.branch_name.clone().filter(|branch| !branch.is_empty());
    let branch_suffix = branch_name
        .as_deref()
        .map_or(String::new(), |branch| format!(" ({branch})"));
    BackgroundSyncFailurePayload {
        operation: BackgroundSyncOperation::PullRequestSync,
        target: format!("{}{branch_suffix}", task.title),
        reason: error.message.clone(),
        task_id: Some(task.id.clone()),
        branch_name,
        ssh_host: task
            .ssh_host
            .clone()
            .filter(|host| !host.is_empty())
            .map(Some),
    }
}

impl KanbanService {
    fn github_cli_query<'a>(
        &'a self,
        now: &'a dyn Fn() -> Instant,
        cwd: &'a str,
        ssh_host: Option<&'a str>,
    ) -> GitHubCliQuery<'a> {
        GitHubCliQuery {
            runner: self.files.runner(),
            auth_failures: &self.pr_lookups.auth_failures,
            now,
            cwd,
            ssh_host,
        }
    }

    /// 작업 브랜치의 PR URL을 조회해 저장한다. PR이 없거나 gh가 없으면 `None`이고, 그 밖의 조회 실패는 오류다.
    /// 여러 창이 같은 task를 동시에 물으면 조회 하나를 함께 쓴다. 다만 자동 조회는 기억한 인증 실패를 만나면
    /// gh를 부르지 않고 끝나므로, 사용자가 직접 요청한 조회는 자동 조회에 합류하지 않는다.
    pub fn fetch_and_save_pr_url(&self, task_id: &str, is_automatic: bool) -> LookupResult {
        let shared = match self.start_pr_url_lookup(task_id, is_automatic) {
            LookupStart::Throttled => return Ok(None),
            LookupStart::Joined(shared) => return shared.wait(),
            LookupStart::Started(shared) => shared,
        };
        let result = self.look_up_and_save_pr_url(task_id, is_automatic);
        shared.finish(result.clone());
        let mut active = self.pr_lookups.active.lock().unwrap();
        if active
            .get(task_id)
            .is_some_and(|lookup| Arc::ptr_eq(&lookup.shared, &shared))
        {
            active.remove(task_id);
        }
        result
    }

    fn start_pr_url_lookup(&self, task_id: &str, is_automatic: bool) -> LookupStart {
        let mut active = self.pr_lookups.active.lock().unwrap();
        if let Some(lookup) = active.get(task_id)
            && (is_automatic || !lookup.is_automatic)
        {
            return LookupStart::Joined(Arc::clone(&lookup.shared));
        }
        // 화면이 스스로 건 조회는 보드 갱신마다 다시 오므로 task당 간격을 둔다
        if is_automatic {
            let now = self.now();
            let mut attempts = self.pr_lookups.automatic_attempts.lock().unwrap();
            let is_recent = attempts.get(task_id).is_some_and(|&attempted_at| {
                now.duration_since(attempted_at) < AUTOMATIC_PR_LOOKUP_MIN_INTERVAL
            });
            if is_recent {
                return LookupStart::Throttled;
            }
            attempts.insert(task_id.to_string(), now);
        }
        let shared = Arc::new(SharedLookup::default());
        let lookup = ActiveLookup {
            is_automatic,
            shared: Arc::clone(&shared),
        };
        active.insert(task_id.to_string(), lookup);
        LookupStart::Started(shared)
    }

    fn look_up_and_save_pr_url(&self, task_id: &str, is_automatic: bool) -> LookupResult {
        let Some(task) = find_task_by_id(&self.database, task_id)? else {
            return Ok(None);
        };
        let Some(branch_name) = task.branch_name.clone().filter(|branch| !branch.is_empty()) else {
            return Ok(None);
        };
        let pr_url = self
            .query_task_pr_url(&task, &branch_name, is_automatic)
            .inspect_err(|error| eprintln!("PR URL 조회 실패: {error}"))?;
        let Some(pr_url) = pr_url else {
            return Ok(None);
        };
        if update_pr_url_if_branch_unchanged(&self.database, &task.id, &branch_name, &pr_url)? == 0
        {
            return Ok(None);
        }
        self.board.broadcast_board_update();
        Ok(Some(pr_url))
    }

    fn query_task_pr_url(
        &self,
        task: &KanbanTask,
        branch_name: &str,
        is_automatic: bool,
    ) -> LookupResult {
        let project = match task.project_id.as_deref() {
            Some(project_id) => find_project_by_id(&self.database, project_id)?,
            None => None,
        };
        let (cwd, ssh_host) = resolve_task_git_context(task, project.as_ref());
        let now = || self.now();
        // 이미 한 번 알린 인증 실패를 화면 갱신마다 다시 던지지 않는다
        let has_recent_auth_failure = self
            .pr_lookups
            .auth_failures
            .find_recent(&cwd, ssh_host.as_deref(), self.now())
            .is_some();
        if is_automatic && has_recent_auth_failure {
            return Ok(None);
        }
        let query = self.github_cli_query(&now, &cwd, ssh_host.as_deref());
        Ok(query.pr_url(branch_name)?)
    }
}

/// task 하나의 PR 동기화에서 나온 변화
#[derive(Default)]
struct TaskPullRequestSync {
    updated_task_id: Option<String>,
    merged: Option<(String, TaskPrMergedDetectedPayload)>,
}

impl KanbanService {
    /// Done이 아닌 task의 PR URL을 갱신하고, 새로 merge된 PR은 한 번만 모아 알린다.
    /// `emitted_merge_event_keys`는 백그라운드 sync가 주기 사이에 유지하는 이미 알린 merge 목록이다
    pub fn sync_active_task_pull_requests(
        &self,
        emitted_merge_event_keys: &mut HashSet<String>,
    ) -> ServiceResult<ActiveTaskPullRequestSyncResult> {
        let tasks = select_tasks(
            &self.database,
            "WHERE status != 'done' ORDER BY updated_at ASC, rowid ASC",
            &[],
        )?;
        let mut result = ActiveTaskPullRequestSyncResult::default();
        let mut failures = Vec::new();
        for task in tasks.iter().filter(|task| has_branch(task)) {
            match self.sync_task_pull_request(task, emitted_merge_event_keys) {
                Ok(sync) => record_task_pull_request_sync(&mut result, sync),
                Err(error) => {
                    eprintln!(
                        "PR 상태 동기화 실패: {{ taskId: {:?}, branchName: {:?}, error: {:?} }}",
                        task.id, task.branch_name, error.message
                    );
                    failures.push(build_pull_request_sync_failure(task, &error));
                }
            }
        }
        if !result.merged_pull_requests.is_empty() {
            self.board
                .broadcast_task_pr_merged_detected_batch(result.merged_pull_requests.clone());
        }
        if !failures.is_empty() {
            result.failures = Some(failures);
        }
        Ok(result)
    }

    fn sync_task_pull_request(
        &self,
        task: &KanbanTask,
        emitted_merge_event_keys: &mut HashSet<String>,
    ) -> ServiceResult<TaskPullRequestSync> {
        let branch_name = task.branch_name.as_deref().unwrap_or_default();
        let project = match task.project_id.as_deref() {
            Some(project_id) => find_project_by_id(&self.database, project_id)?,
            None => None,
        };
        if is_default_branch_task(task, project.as_ref()) {
            return Ok(TaskPullRequestSync::default());
        }
        let (cwd, ssh_host) = resolve_task_git_context(task, project.as_ref());
        let now = || self.now();
        let status = self
            .github_cli_query(&now, &cwd, ssh_host.as_deref())
            .pr_status(branch_name)?;
        let Some((status, url)) = status.and_then(|status| {
            let url = status.url.clone()?;
            Some((status, url))
        }) else {
            return Ok(TaskPullRequestSync::default());
        };
        let mut sync = TaskPullRequestSync::default();
        if task.pr_url.as_deref() != Some(url.as_str()) {
            if update_pr_url_if_branch_unchanged(&self.database, &task.id, branch_name, &url)? == 0
            {
                return Ok(sync);
            }
            sync.updated_task_id = Some(task.id.clone());
        } else if !task_exists_with_branch(&self.database, &task.id, branch_name)? {
            return Ok(sync);
        }
        if let (Some("MERGED"), Some(merged_at)) = (status.state.as_deref(), status.merged_at) {
            let merge_event_key = format!("{}:{url}:{merged_at}", task.id);
            if emitted_merge_event_keys.insert(merge_event_key.clone()) {
                let payload = TaskPrMergedDetectedPayload {
                    task_id: task.id.clone(),
                    task_title: task.title.clone(),
                    branch_name: branch_name.to_string(),
                    pr_url: url,
                    merged_at,
                };
                sync.merged = Some((merge_event_key, payload));
            }
        }
        Ok(sync)
    }
}

fn record_task_pull_request_sync(
    result: &mut ActiveTaskPullRequestSyncResult,
    sync: TaskPullRequestSync,
) {
    result.updated_task_ids.extend(sync.updated_task_id);
    if let Some((merge_event_key, payload)) = sync.merged {
        result.merge_event_keys.push(merge_event_key);
        result.merged_pull_requests.push(payload);
    }
}
