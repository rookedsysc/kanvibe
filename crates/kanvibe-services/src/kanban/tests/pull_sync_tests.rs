//! 활성 task worktree pull 동기화.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use kanvibe_domain::notifications::{BackgroundSyncPulledTaskPayload, PulledTaskStatus};
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, TaskStatus};
use kanvibe_process::exec::ExecGitError;

use super::fakes::KanbanFixture;
use super::{add_project, add_task, with_branch};
use crate::kanban::ActiveTaskPullSyncResult;
use crate::test_support::RecordedCommand;

const LOCAL_TIMEOUT: Option<Duration> = Some(Duration::from_millis(10_000));

/// 원격 브랜치 확인과 pull을 경로별로 흉내 낸다. `pull_outputs`는 pull 호출마다 하나씩 꺼내 쓴다
struct PullScript {
    missing_remote_paths: Vec<&'static str>,
    stuck_check_paths: Vec<&'static str>,
    pull_outputs: Mutex<Vec<Result<&'static str, &'static str>>>,
}

fn is_remote_branch_check(call: &RecordedCommand) -> bool {
    call.command
        .contains("ls-remote --exit-code --heads origin")
}

fn respond_to_pull_commands(fixture: &KanbanFixture, script: PullScript) {
    let script = Arc::new(script);
    fixture.responses.set(move |call| {
        let mentions = |path: &str| call.command.contains(&format!("'{path}'"));
        if is_remote_branch_check(call) {
            if script.stuck_check_paths.iter().any(|path| mentions(path)) {
                return Err(ExecGitError::new("Remote branch check timed out after 10s"));
            }
            let is_missing = script
                .missing_remote_paths
                .iter()
                .any(|path| mentions(path));
            return Ok(if is_missing { "missing" } else { "exists" }.into());
        }
        let mut outputs = script.pull_outputs.lock().unwrap();
        let output = if outputs.is_empty() {
            Ok("Already up to date.")
        } else {
            outputs.remove(0)
        };
        output.map(str::to_string).map_err(ExecGitError::new)
    });
}

fn pull_calls(fixture: &KanbanFixture) -> Vec<RecordedCommand> {
    fixture
        .runner
        .calls()
        .into_iter()
        .filter(|call| call.command.contains("pull --ff-only"))
        .collect()
}

fn add_pull_task(
    fixture: &KanbanFixture,
    project: &Project,
    title: &str,
    branch: &str,
    worktree: &str,
    status: TaskStatus,
) -> KanbanTask {
    add_task(
        fixture,
        title,
        status,
        with_branch(project, branch, Some(worktree)),
    )
}

/// `/workspace/repo__worktrees/<브랜치 마지막 이름>` worktree를 가진 task
fn add_feature_task(
    fixture: &KanbanFixture,
    project: &Project,
    title: &str,
    branch: &str,
    status: TaskStatus,
) -> KanbanTask {
    let worktree_name = branch.rsplit('/').next().unwrap_or(branch);
    let worktree = format!("/workspace/repo__worktrees/{worktree_name}");
    add_pull_task(fixture, project, title, branch, &worktree, status)
}

/// pull 대상에서 빠지는 프로젝트 루트의 기본 브랜치 task
fn add_main_task(fixture: &KanbanFixture, project: &Project, title: &str) {
    add_pull_task(
        fixture,
        project,
        title,
        "main",
        "/workspace/repo",
        TaskStatus::Progress,
    );
}

/// pull 명령이 이 worktree를 가리키는지
fn pulls_in(call: &RecordedCommand, worktree: &str) -> bool {
    call.command.contains(&format!("'{worktree}'"))
}

fn pulled(
    task: &KanbanTask,
    status: PulledTaskStatus,
    summary: &str,
) -> BackgroundSyncPulledTaskPayload {
    BackgroundSyncPulledTaskPayload {
        task_id: task.id.clone(),
        task_title: task.title.clone(),
        branch_name: task.branch_name.clone().unwrap(),
        worktree_path: task.worktree_path.clone().unwrap(),
        ssh_host: task.ssh_host.clone(),
        status,
        summary: summary.into(),
    }
}

fn script(pull_outputs: Vec<Result<&'static str, &'static str>>) -> PullScript {
    PullScript {
        missing_remote_paths: Vec::new(),
        stuck_check_paths: Vec::new(),
        pull_outputs: Mutex::new(pull_outputs),
    }
}

#[test]
fn pull_sync_skips_the_default_branch_and_pulls_one_task_at_a_time() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", None);
    add_main_task(&fixture, &project, "Main");
    let task_a = add_feature_task(
        &fixture,
        &project,
        "Pull A",
        "feature/pull-a",
        TaskStatus::Progress,
    );
    let task_b = add_feature_task(
        &fixture,
        &project,
        "Pull B",
        "feature/pull-b",
        TaskStatus::Review,
    );
    respond_to_pull_commands(
        &fixture,
        script(vec![
            Ok("Fast-forward\n src/file.ts | 1 +"),
            Err("Not possible to fast-forward"),
        ]),
    );

    let result = fixture.service.sync_active_task_pulls().unwrap();

    let pulls = pull_calls(&fixture);
    assert_eq!(pulls.len(), 2);
    assert!(pulls_in(&pulls[0], "/workspace/repo__worktrees/pull-a"));
    assert!(pulls_in(&pulls[1], "/workspace/repo__worktrees/pull-b"));
    assert!(
        pulls
            .iter()
            .all(|call| call.ssh_host.is_none() && call.options.timeout == LOCAL_TIMEOUT)
    );
    assert_eq!(
        result.pulled_tasks,
        [
            pulled(&task_a, PulledTaskStatus::Updated, "Fast-forward"),
            pulled(
                &task_b,
                PulledTaskStatus::Failed,
                "Not possible to fast-forward"
            ),
        ]
    );
}

#[test]
fn pull_sync_moves_on_when_one_remote_branch_check_times_out() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", None);
    add_main_task(&fixture, &project, "Main task");
    let stuck = add_feature_task(
        &fixture,
        &project,
        "Stuck pull",
        "feature/stuck",
        TaskStatus::Progress,
    );
    add_feature_task(
        &fixture,
        &project,
        "Next pull",
        "feature/next",
        TaskStatus::Review,
    );
    respond_to_pull_commands(
        &fixture,
        PullScript {
            stuck_check_paths: vec!["/workspace/repo__worktrees/stuck"],
            ..script(Vec::new())
        },
    );

    let result = fixture.service.sync_active_task_pulls().unwrap();

    let checks: Vec<RecordedCommand> = fixture
        .runner
        .calls()
        .into_iter()
        .filter(is_remote_branch_check)
        .collect();
    assert_eq!(checks.len(), 2);
    assert!(
        checks
            .iter()
            .all(|call| call.options.timeout == LOCAL_TIMEOUT)
    );
    let pulls = pull_calls(&fixture);
    assert_eq!(pulls.len(), 1);
    assert!(pulls_in(&pulls[0], "/workspace/repo__worktrees/next"));
    assert_eq!(result.pulled_tasks.len(), 1);
    assert_eq!(result.pulled_tasks[0].task_id, stuck.id);
    assert_eq!(result.pulled_tasks[0].status, PulledTaskStatus::Failed);
    assert!(result.pulled_tasks[0].summary.contains("timed out"));
}

#[test]
fn remote_pull_sync_leaves_timing_to_the_ssh_layer() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/remote/repo", Some("remote-host"));
    let task = add_task(&fixture, "Remote pull", TaskStatus::Review, |task| {
        with_branch(
            &project,
            "feature/remote-pull",
            Some("/remote/repo__worktrees/remote-pull"),
        )(task);
        task.ssh_host = Some("remote-host".into());
    });
    respond_to_pull_commands(
        &fixture,
        script(vec![Ok("Fast-forward\n src/file.ts | 1 +")]),
    );

    let result = fixture.service.sync_active_task_pulls().unwrap();

    let calls = fixture.runner.calls();
    assert_eq!(calls.len(), 2);
    assert!(
        calls
            .iter()
            .all(|call| call.ssh_host.as_deref() == Some("remote-host")
                && call.options.timeout.is_none())
    );
    assert_eq!(
        result.pulled_tasks,
        [pulled(&task, PulledTaskStatus::Updated, "Fast-forward")]
    );
}

#[test]
fn pull_sync_never_pulls_done_tasks() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", None);
    add_feature_task(
        &fixture,
        &project,
        "Done task",
        "feature/done",
        TaskStatus::Done,
    );
    add_feature_task(
        &fixture,
        &project,
        "Progress task",
        "feature/progress",
        TaskStatus::Progress,
    );
    respond_to_pull_commands(&fixture, script(Vec::new()));

    let result = fixture.service.sync_active_task_pulls().unwrap();

    let pulls = pull_calls(&fixture);
    assert_eq!(pulls.len(), 1);
    assert!(pulls_in(&pulls[0], "/workspace/repo__worktrees/progress"));
    assert_eq!(result, ActiveTaskPullSyncResult::default());
}

#[test]
fn pull_sync_skips_tasks_without_a_remote_branch() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", None);
    let worktree = "/workspace/repo__worktrees/missing-remote";
    add_pull_task(
        &fixture,
        &project,
        "Missing remote branch",
        "feature/missing-remote",
        worktree,
        TaskStatus::Review,
    );
    respond_to_pull_commands(
        &fixture,
        PullScript {
            missing_remote_paths: vec![worktree],
            ..script(Vec::new())
        },
    );

    let result = fixture.service.sync_active_task_pulls().unwrap();

    let check = fixture.runner.calls()[0].clone();
    assert!(
        check
            .command
            .contains(&format!("git -C '{worktree}' ls-remote"))
    );
    assert!(
        check
            .command
            .contains("'refs/heads/feature/missing-remote'")
    );
    assert_eq!(check.options.timeout, LOCAL_TIMEOUT);
    assert!(pull_calls(&fixture).is_empty());
    assert_eq!(result, ActiveTaskPullSyncResult::default());
}

#[test]
fn missing_remote_ref_pull_errors_are_not_reported_as_failures() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", Some("remote-host"));
    add_feature_task(
        &fixture,
        &project,
        "Stale upstream",
        "feature/stale-upstream",
        TaskStatus::Review,
    );
    respond_to_pull_commands(
        &fixture,
        script(vec![Err(
            "remote-host 원격 명령 실패: Your configuration specifies to merge with the ref 'refs/heads/feature/stale-upstream' from the remote, but no such ref was fetched.",
        )]),
    );

    assert_eq!(
        fixture.service.sync_active_task_pulls(),
        Ok(ActiveTaskPullSyncResult::default())
    );
}

#[test]
fn the_same_pull_failure_is_reported_once_until_a_pull_succeeds() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", None);
    let task = add_feature_task(
        &fixture,
        &project,
        "Pull B",
        "feature/pull-b",
        TaskStatus::Review,
    );
    let failure = Err("Not possible to fast-forward");
    respond_to_pull_commands(
        &fixture,
        script(vec![failure, failure, Ok("Already up to date."), failure]),
    );

    let cycles: Vec<Vec<BackgroundSyncPulledTaskPayload>> = (0..4)
        .map(|_| {
            fixture
                .service
                .sync_active_task_pulls()
                .unwrap()
                .pulled_tasks
        })
        .collect();

    let reported = pulled(
        &task,
        PulledTaskStatus::Failed,
        "Not possible to fast-forward",
    );
    assert_eq!(
        cycles,
        [
            vec![reported.clone()],
            Vec::new(),
            Vec::new(),
            vec![reported]
        ]
    );
}
