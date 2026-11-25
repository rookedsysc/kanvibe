//! PR URL 조회·저장, 조회 중복·반복 억제, 활성 task PR 동기화, 저장소 판별 실패 복구.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kanvibe_domain::notifications::{BackgroundSyncFailurePayload, BackgroundSyncOperation};
use kanvibe_domain::task::{KanbanTask, TaskStatus};
use kanvibe_process::exec::ExecGitError;

use super::fakes::{Gate, KanbanFixture, wait_until};
use super::{add_project, add_task, stored_task, with_branch};
use crate::board_notifier::BoardEvent;
use crate::kanban::ActiveTaskPullRequestSyncResult;

const AUTH_FAILURE: &str = "dev-vm 원격 명령 실패: Try authenticating with:  gh auth login";

fn pr_url_arguments(branch: &str) -> String {
    format!("pr list --head '{branch}' --json url -q '.[0].url'")
}

fn pr_status_arguments(branch: &str) -> String {
    format!("pr list --head '{branch}' --state all --json url,state,mergedAt,updatedAt")
}

/// 저장소로 내려가 direnv 환경에서 gh를 부르는 명령인지 본다
fn assert_github_cli_command(command: &str, repo_path: &str, gh_arguments: &str) {
    assert!(
        command.contains(&format!("repo='{repo_path}'")),
        "{command}"
    );
    assert!(command.contains(r#"if [ ! -e "$repo/.git" ]"#));
    assert!(command.contains(r#"if [ -n "$envrcDirectory" ] && command -v direnv"#));
    assert!(command.contains(&format!(
        r#"direnv exec "$repo" gh {gh_arguments} || gh {gh_arguments}"#
    )));
    assert!(command.contains(&format!("else gh {gh_arguments}")));
}

fn last_command(fixture: &KanbanFixture) -> String {
    fixture
        .runner
        .commands()
        .last()
        .cloned()
        .unwrap_or_default()
}

/// 원격 프로젝트 루트에서 조회하는 브랜치 task. 같은 저장소 task는 같은 프로젝트를 쓴다
fn remote_project_task(fixture: &KanbanFixture, branch: &str) -> KanbanTask {
    let existing =
        kanvibe_db::repositories::projects::list_projects(&fixture.database().connection())
            .unwrap();
    let project = match existing
        .into_iter()
        .find(|project| project.repo_path == "/remote/repo")
    {
        Some(project) => project,
        None => add_project(fixture, "/remote/repo", Some("dev-vm")),
    };
    add_task(
        fixture,
        branch,
        TaskStatus::Progress,
        with_branch(&project, branch, None),
    )
}

/// 호출 순서대로 정한 결과를 돌려준다. 남은 결과가 없으면 빈 출력이다
fn respond_in_sequence(fixture: &KanbanFixture, results: Vec<Result<&str, &str>>) {
    let results: Mutex<Vec<Result<String, String>>> = Mutex::new(
        results
            .into_iter()
            .rev()
            .map(|result| result.map(str::to_string).map_err(str::to_string))
            .collect(),
    );
    fixture
        .responses
        .set(move |_| match results.lock().unwrap().pop() {
            Some(Ok(output)) => Ok(output),
            Some(Err(message)) => Err(ExecGitError::new(message)),
            None => Ok(String::new()),
        });
}

fn pull_request_json(url: Option<&str>, state: &str, merged_at: Option<&str>) -> String {
    serde_json::json!([{ "url": url, "state": state, "mergedAt": merged_at, "updatedAt": "2026-04-30T01:00:00Z" }])
        .to_string()
}

fn sync(fixture: &KanbanFixture) -> ActiveTaskPullRequestSyncResult {
    fixture
        .service
        .sync_active_task_pull_requests(&mut HashSet::new())
        .unwrap()
}

fn merge_batch_count(fixture: &KanbanFixture) -> usize {
    fixture.log.count("board:task-pr-merged-detected-batch")
}

// parity: TS-main-kanbanService-6b0e86
#[test]
fn pr_url_lookup_runs_gh_inside_the_repository_with_direnv() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", None);
    let task = add_task(
        &fixture,
        "main",
        TaskStatus::Todo,
        with_branch(&project, "main", None),
    );
    fixture
        .responses
        .returning("https://github.com/kanvibe/kanvibe/pull/1\n");

    let result = fixture
        .service
        .fetch_and_save_pr_url(&task.id, false)
        .unwrap();

    assert_github_cli_command(
        &last_command(&fixture),
        "/workspace/repo",
        &pr_url_arguments("main"),
    );
    let call = fixture.runner.calls()[0].clone();
    assert_eq!(call.ssh_host, None);
    assert_eq!(call.options.timeout, Some(Duration::from_millis(10_000)));
    assert_eq!(
        result.as_deref(),
        Some("https://github.com/kanvibe/kanvibe/pull/1")
    );
    assert_eq!(stored_task(&fixture, &task.id).unwrap().pr_url, result);
    assert_eq!(fixture.board_update_count(), 1);
}

// parity: TS-main-kanbanService-b1d690
#[test]
fn missing_gh_cli_quietly_skips_the_lookup() {
    let fixture = KanbanFixture::new();
    let task = remote_project_task(&fixture, "dev");
    fixture.responses.set(|_| {
        Err(ExecGitError {
            message: "spawn gh ENOENT".into(),
            error_code: Some("ENOENT"),
            ..ExecGitError::default()
        })
    });

    assert_eq!(
        fixture.service.fetch_and_save_pr_url(&task.id, false),
        Ok(None)
    );
    assert_eq!(stored_task(&fixture, &task.id).unwrap().pr_url, None);
}

// parity: TS-main-kanbanService-48e4cd
#[test]
fn other_gh_failures_are_returned_instead_of_no_pull_request() {
    let fixture = KanbanFixture::new();
    let task = remote_project_task(&fixture, "feat/auth");
    fixture.responses.failing("HTTP 401: Bad credentials");

    let error = fixture
        .service
        .fetch_and_save_pr_url(&task.id, false)
        .unwrap_err();

    assert_eq!(error.message, "HTTP 401: Bad credentials");
    assert_eq!(stored_task(&fixture, &task.id).unwrap().pr_url, None);
}

// parity: TS-main-kanbanService-8764f8
#[test]
fn simultaneous_lookups_from_several_windows_share_one_gh_query() {
    let fixture = KanbanFixture::new();
    let task = remote_project_task(&fixture, "feat/task-auto");
    let gate = Gate::default();
    let held = gate.clone();
    fixture.responses.set(move |_| {
        held.wait();
        Ok("https://github.com/kanvibe/kanvibe/pull/7\n".into())
    });
    let spawn_window = || {
        let (service, task_id) = (Arc::clone(&fixture.service), task.id.clone());
        std::thread::spawn(move || service.fetch_and_save_pr_url(&task_id, false))
    };

    let first_window = spawn_window();
    wait_until(|| fixture.runner.calls().len() == 1);
    let second_window = spawn_window();
    std::thread::sleep(Duration::from_millis(50));
    gate.open();

    let expected = Ok(Some(
        "https://github.com/kanvibe/kanvibe/pull/7".to_string(),
    ));
    assert_eq!(first_window.join().unwrap(), expected);
    assert_eq!(second_window.join().unwrap(), expected);
    assert_eq!(fixture.runner.calls().len(), 1);
    assert_eq!(fixture.board_update_count(), 1);
}

// parity: TS-main-kanbanService-2d9287
#[test]
fn automatic_lookups_run_gh_once_per_thirty_seconds_but_manual_lookups_always_run() {
    let fixture = KanbanFixture::new();
    let task = remote_project_task(&fixture, "feat/task-auto");
    let other = remote_project_task(&fixture, "feat/task-other");

    for _ in 0..5 {
        assert_eq!(
            fixture.service.fetch_and_save_pr_url(&task.id, true),
            Ok(None)
        );
    }
    assert_eq!(fixture.runner.calls().len(), 1);
    assert_eq!(
        fixture.service.fetch_and_save_pr_url(&task.id, false),
        Ok(None)
    );
    assert_eq!(fixture.runner.calls().len(), 2);
    assert_eq!(
        fixture.service.fetch_and_save_pr_url(&other.id, true),
        Ok(None)
    );
    assert_eq!(fixture.runner.calls().len(), 3);
    fixture.clock.advance(Duration::from_secs(30));
    assert_eq!(
        fixture.service.fetch_and_save_pr_url(&task.id, true),
        Ok(None)
    );
    assert_eq!(fixture.runner.calls().len(), 4);
}

// parity: TS-main-kanbanService-dc2e5f
#[test]
fn auth_failures_stop_automatic_lookups_until_a_manual_lookup_succeeds() {
    let fixture = KanbanFixture::new();
    let task_a = remote_project_task(&fixture, "feat/task-a");
    let task_b = remote_project_task(&fixture, "feat/task-b");
    fixture.responses.failing(AUTH_FAILURE);

    let first = fixture
        .service
        .fetch_and_save_pr_url(&task_a.id, true)
        .unwrap_err();
    assert!(first.message.contains("gh auth login"));
    assert_eq!(
        fixture.service.fetch_and_save_pr_url(&task_b.id, true),
        Ok(None)
    );
    fixture.clock.advance(Duration::from_secs(30));
    assert_eq!(
        fixture.service.fetch_and_save_pr_url(&task_a.id, true),
        Ok(None)
    );
    assert_eq!(fixture.runner.calls().len(), 1);

    assert!(
        fixture
            .service
            .fetch_and_save_pr_url(&task_a.id, false)
            .is_err()
    );
    assert_eq!(fixture.runner.calls().len(), 2);

    fixture.responses.returning("");
    assert_eq!(
        fixture.service.fetch_and_save_pr_url(&task_a.id, false),
        Ok(None)
    );
    fixture.clock.advance(Duration::from_secs(30));
    assert_eq!(
        fixture.service.fetch_and_save_pr_url(&task_b.id, true),
        Ok(None)
    );
    assert_eq!(fixture.runner.calls().len(), 4);
}

// parity: TS-main-kanbanService-66f1df
#[test]
fn remembered_auth_failure_is_rechecked_once_after_five_minutes() {
    let fixture = KanbanFixture::new();
    let task = remote_project_task(&fixture, "feat/task-a");
    fixture.responses.failing(AUTH_FAILURE);
    assert!(
        fixture
            .service
            .fetch_and_save_pr_url(&task.id, true)
            .is_err()
    );

    fixture.clock.advance(Duration::from_secs(5 * 60));

    let recheck = fixture
        .service
        .fetch_and_save_pr_url(&task.id, true)
        .unwrap_err();
    assert!(recheck.message.contains("gh auth login"));
    assert_eq!(fixture.runner.calls().len(), 2);
}

// parity: TS-main-kanbanService-55aea9
#[test]
fn background_sync_remembers_auth_failure_per_task_path_for_the_next_cycle() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/remote/repo", Some("dev-vm"));
    let task_ids: Vec<String> = ["task-a", "task-b", "task-c"]
        .iter()
        .map(|name| {
            let worktree = format!("/remote/repo__worktrees/{name}");
            add_task(&fixture, name, TaskStatus::Progress, |task| {
                with_branch(&project, &format!("feat/{name}"), Some(&worktree))(task);
                task.ssh_host = Some("dev-vm".into());
            })
            .id
        })
        .collect();
    fixture.responses.failing(AUTH_FAILURE);

    let first_cycle = sync(&fixture);
    let second_cycle = sync(&fixture);

    let failed_ids: Vec<Option<String>> = first_cycle
        .failures
        .unwrap()
        .into_iter()
        .map(|failure| failure.task_id)
        .collect();
    assert_eq!(
        failed_ids,
        task_ids.into_iter().map(Some).collect::<Vec<_>>()
    );
    let reasons: Vec<String> = second_cycle
        .failures
        .unwrap()
        .into_iter()
        .map(|failure| failure.reason)
        .collect();
    assert_eq!(reasons, [AUTH_FAILURE; 3]);
    assert_eq!(fixture.runner.calls().len(), 3);
}

fn remote_worktree_task(
    fixture: &KanbanFixture,
    project_id: &str,
    branch: &str,
    worktree: &str,
) -> KanbanTask {
    add_task(fixture, branch, TaskStatus::Progress, |task| {
        task.project_id = Some(project_id.into());
        task.branch_name = Some(branch.into());
        task.worktree_path = Some(worktree.into());
        task.ssh_host = Some("remote-host".into());
    })
}

// parity: TS-main-kanbanService-1335ba
#[test]
fn remote_projects_look_up_the_pr_in_the_task_worktree_over_ssh() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/remote/repo", Some("remote-host"));
    let worktree = "/Users/local/repo__worktrees/feature-remote-pr";
    let task = remote_worktree_task(&fixture, &project.id, "feature/remote-pr", worktree);
    fixture
        .responses
        .returning("https://github.com/kanvibe/kanvibe/pull/99");

    let result = fixture
        .service
        .fetch_and_save_pr_url(&task.id, false)
        .unwrap();

    assert_github_cli_command(
        &last_command(&fixture),
        worktree,
        &pr_url_arguments("feature/remote-pr"),
    );
    let call = fixture.runner.calls()[0].clone();
    assert_eq!(call.ssh_host.as_deref(), Some("remote-host"));
    assert_eq!(call.options.timeout, None);
    assert_eq!(
        result.as_deref(),
        Some("https://github.com/kanvibe/kanvibe/pull/99")
    );
    assert_eq!(stored_task(&fixture, &task.id).unwrap().pr_url, result);
    assert_eq!(fixture.board_update_count(), 1);
}

// parity: TS-main-kanbanService-1b9f15
#[test]
fn without_a_project_the_lookup_uses_the_task_worktree() {
    let fixture = KanbanFixture::new();
    let worktree = "/remote/repo__worktrees/feature-fallback-path";
    let task = remote_worktree_task(
        &fixture,
        "missing-project",
        "feature/fallback-path",
        worktree,
    );
    fixture
        .responses
        .returning("https://github.com/kanvibe/kanvibe/pull/101");

    let result = fixture
        .service
        .fetch_and_save_pr_url(&task.id, false)
        .unwrap();

    assert_github_cli_command(
        &last_command(&fixture),
        worktree,
        &pr_url_arguments("feature/fallback-path"),
    );
    assert_eq!(
        fixture.runner.calls()[0].ssh_host.as_deref(),
        Some("remote-host")
    );
    assert_eq!(
        result.as_deref(),
        Some("https://github.com/kanvibe/kanvibe/pull/101")
    );
    assert_eq!(fixture.board_update_count(), 1);
}

// parity: TS-main-kanbanService-945c44
#[test]
fn missing_remote_gh_cli_quietly_skips_the_lookup() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/remote/repo", Some("remote-host"));
    let worktree = "/Users/local/repo__worktrees/feature-no-gh";
    let task = remote_worktree_task(&fixture, &project.id, "feature/no-gh", worktree);
    fixture.responses.failing("bash: gh: command not found");

    assert_eq!(
        fixture.service.fetch_and_save_pr_url(&task.id, false),
        Ok(None)
    );
    assert_github_cli_command(
        &last_command(&fixture),
        worktree,
        &pr_url_arguments("feature/no-gh"),
    );
    assert_eq!(fixture.board_update_count(), 0);
}

fn local_feature_task(
    fixture: &KanbanFixture,
    title: &str,
    branch: &str,
    status: TaskStatus,
) -> KanbanTask {
    let project = add_project(fixture, "/workspace/repo", None);
    let worktree = format!("/workspace/repo__worktrees/{}", branch.replace('/', "-"));
    add_task(
        fixture,
        title,
        status,
        with_branch(&project, branch, Some(&worktree)),
    )
}

// parity: TS-main-kanbanService-c3f17a
#[test]
fn pr_sync_saves_a_new_open_pr_url() {
    let fixture = KanbanFixture::new();
    let task = local_feature_task(
        &fixture,
        "PR sync target",
        "feature/pr-sync",
        TaskStatus::Todo,
    );
    let pr_url = "https://github.com/kanvibe/kanvibe/pull/210";
    fixture
        .responses
        .returning(&pull_request_json(Some(pr_url), "OPEN", None));

    let result = sync(&fixture);

    assert_eq!(result.updated_task_ids, std::slice::from_ref(&task.id));
    assert_eq!(
        stored_task(&fixture, &task.id).unwrap().pr_url.as_deref(),
        Some(pr_url)
    );
    assert_eq!(merge_batch_count(&fixture), 0);
}

// parity: TS-main-kanbanService-8587a7
#[test]
fn pr_sync_skips_the_default_branch_root_task() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", None);
    add_task(
        &fixture,
        "Main branch task",
        TaskStatus::Review,
        with_branch(&project, "main", Some("/workspace/repo")),
    );

    assert_eq!(sync(&fixture), ActiveTaskPullRequestSyncResult::default());
    assert!(fixture.runner.calls().is_empty());
    assert_eq!(merge_batch_count(&fixture), 0);
}

// parity: TS-main-kanbanService-d877ed
#[test]
fn pr_sync_failures_name_the_task_and_the_reason() {
    let fixture = KanbanFixture::new();
    let task = local_feature_task(
        &fixture,
        "PR sync target",
        "feature/pr-fail",
        TaskStatus::Review,
    );
    fixture.responses.failing("gh auth failed");

    let result = sync(&fixture);

    assert_eq!(
        result,
        ActiveTaskPullRequestSyncResult {
            failures: Some(vec![BackgroundSyncFailurePayload {
                operation: BackgroundSyncOperation::PullRequestSync,
                target: "PR sync target (feature/pr-fail)".into(),
                reason: "gh auth failed".into(),
                task_id: Some(task.id.clone()),
                branch_name: Some("feature/pr-fail".into()),
                ssh_host: None,
            }]),
            ..ActiveTaskPullRequestSyncResult::default()
        }
    );
    assert_eq!(merge_batch_count(&fixture), 0);
}

// parity: TS-main-kanbanService-b3057c
#[test]
fn pr_sync_broadcasts_each_merge_once() {
    let fixture = KanbanFixture::new();
    let task = local_feature_task(
        &fixture,
        "Merged PR task",
        "feature/merged-pr",
        TaskStatus::Review,
    );
    let pr_url = "https://github.com/kanvibe/kanvibe/pull/211";
    fixture.responses.returning(&pull_request_json(
        Some(pr_url),
        "MERGED",
        Some("2026-04-30T02:00:00Z"),
    ));
    let mut merge_event_keys = HashSet::new();

    fixture
        .service
        .sync_active_task_pull_requests(&mut merge_event_keys)
        .unwrap();
    fixture
        .service
        .sync_active_task_pull_requests(&mut merge_event_keys)
        .unwrap();

    let batches: Vec<BoardEvent> = fixture
        .events()
        .into_iter()
        .filter(|event| matches!(event, BoardEvent::TaskPrMergedDetectedBatch(_)))
        .collect();
    let BoardEvent::TaskPrMergedDetectedBatch(batch) = &batches[0] else {
        unreachable!()
    };
    assert_eq!(batches.len(), 1);
    let merged = &batch.merged_pull_requests[0];
    assert_eq!(
        (merged.task_id.as_str(), merged.task_title.as_str()),
        (task.id.as_str(), "Merged PR task")
    );
    assert_eq!(
        (merged.pr_url.as_str(), merged.merged_at.as_str()),
        (pr_url, "2026-04-30T02:00:00Z")
    );
}

// parity: TS-main-kanbanService-035c5c
#[test]
fn pr_sync_queries_gh_one_task_at_a_time() {
    let fixture = KanbanFixture::new();
    local_feature_task(
        &fixture,
        "Serial PR A",
        "feature/serial-a",
        TaskStatus::Review,
    );
    local_feature_task(
        &fixture,
        "Serial PR B",
        "feature/serial-b",
        TaskStatus::Review,
    );
    let (in_flight, most_in_flight) =
        (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (counter, maximum) = (Arc::clone(&in_flight), Arc::clone(&most_in_flight));
    fixture.responses.set(move |_| {
        let current = counter.fetch_add(1, Ordering::SeqCst) + 1;
        maximum.fetch_max(current, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(20));
        counter.fetch_sub(1, Ordering::SeqCst);
        Ok(pull_request_json(None, "OPEN", None))
    });

    sync(&fixture);

    assert_eq!(fixture.runner.calls().len(), 2);
    assert_eq!(most_in_flight.load(Ordering::SeqCst), 1);
}

// parity: TS-main-kanbanService-0016ac
#[test]
fn pr_sync_moves_on_after_a_local_gh_timeout() {
    let fixture = KanbanFixture::new();
    let stuck = local_feature_task(&fixture, "Stuck PR", "feature/pr-stuck", TaskStatus::Review);
    local_feature_task(&fixture, "Next PR", "feature/pr-next", TaskStatus::Review);
    fixture.responses.set(|call| {
        if call.command.contains("feature/pr-stuck") {
            return Err(ExecGitError {
                message: "gh pr list timed out".into(),
                was_killed: true,
                signal: Some("SIGTERM"),
                ..ExecGitError::default()
            });
        }
        Ok(pull_request_json(None, "OPEN", None))
    });

    let result = sync(&fixture);

    let calls = fixture.runner.calls();
    assert_github_cli_command(
        &calls[0].command,
        "/workspace/repo__worktrees/feature-pr-stuck",
        &pr_status_arguments("feature/pr-stuck"),
    );
    let timeouts: Vec<Option<Duration>> = calls.iter().map(|call| call.options.timeout).collect();
    assert_eq!(timeouts, [Some(Duration::from_millis(10_000)); 2]);
    let failures = result.failures.unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].task_id.as_deref(), Some(stuck.id.as_str()));
    assert_eq!(failures[0].branch_name.as_deref(), Some("feature/pr-stuck"));
    assert_eq!(failures[0].reason, "gh pr list timed out");
    assert!(result.updated_task_ids.is_empty());
}

// parity: TS-main-kanbanService-3a20f2
#[test]
fn missing_repository_marker_quietly_skips_the_lookup() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/moved/away", None);
    let task = add_task(
        &fixture,
        "dev",
        TaskStatus::Todo,
        with_branch(&project, "dev", None),
    );
    fixture.responses.returning("kanvibe-missing-repository");

    assert_eq!(
        fixture.service.fetch_and_save_pr_url(&task.id, false),
        Ok(None)
    );
    assert_eq!(stored_task(&fixture, &task.id).unwrap().pr_url, None);
}

/// 조회 도중 사용자가 task를 바꾼 상황. gh가 답하기 직전에 DB를 바꾼다
fn change_task_during_lookup(
    fixture: &KanbanFixture,
    task_id: &str,
    sql: &'static str,
    output: String,
) {
    let (database, task_id) = (Arc::clone(&fixture.db.database), task_id.to_string());
    fixture.responses.set(move |_| {
        database.connection().execute(sql, [&task_id]).unwrap();
        Ok(output.clone())
    });
}

fn add_race_task(fixture: &KanbanFixture) -> KanbanTask {
    add_task(fixture, "initial", TaskStatus::Progress, |task| {
        task.branch_name = Some("feature".into());
        task.worktree_path = Some("/fixture/repo".into());
    })
}

// parity: TS-main-kanbanService-1a0fd9
#[test]
fn lookups_keep_title_and_status_edited_while_waiting() {
    for is_manual in [true, false] {
        let fixture = KanbanFixture::new();
        let task = add_race_task(&fixture);
        let output = if is_manual {
            "https://example.invalid/pr/1".to_string()
        } else {
            serde_json::json!([{ "url": "https://example.invalid/pr/1", "state": "OPEN" }])
                .to_string()
        };
        change_task_during_lookup(
            &fixture,
            &task.id,
            "UPDATE kanban_tasks SET title = 'user edit', status = 'done' WHERE id = ?1",
            output,
        );

        if is_manual {
            fixture
                .service
                .fetch_and_save_pr_url(&task.id, false)
                .unwrap();
        } else {
            sync(&fixture);
        }

        let stored = stored_task(&fixture, &task.id).unwrap();
        assert_eq!(
            (stored.title.as_str(), stored.status),
            ("user edit", TaskStatus::Done)
        );
        assert_eq!(
            stored.pr_url.as_deref(),
            Some("https://example.invalid/pr/1")
        );
    }
}

// parity: TS-main-kanbanService-4f84f5
#[test]
fn lookups_skip_saving_and_notifying_when_the_task_was_deleted_or_rebranched() {
    let changes = [
        "DELETE FROM kanban_tasks WHERE id = ?1",
        "UPDATE kanban_tasks SET branch_name = 'other' WHERE id = ?1",
    ];
    for (is_manual, change) in [true, false]
        .into_iter()
        .flat_map(|mode| changes.map(|change| (mode, change)))
    {
        let fixture = KanbanFixture::new();
        let task = add_race_task(&fixture);
        let merged = serde_json::json!([{ "url": "https://example.invalid/pr/1", "state": "MERGED", "mergedAt": "2026-11-01T00:00:00Z" }]);
        let output = if is_manual {
            "https://example.invalid/pr/1".to_string()
        } else {
            merged.to_string()
        };
        change_task_during_lookup(&fixture, &task.id, change, output);
        let mut event_keys = HashSet::new();

        if is_manual {
            assert_eq!(
                fixture.service.fetch_and_save_pr_url(&task.id, false),
                Ok(None)
            );
        } else {
            let result = fixture
                .service
                .sync_active_task_pull_requests(&mut event_keys)
                .unwrap();
            assert_eq!(result, ActiveTaskPullRequestSyncResult::default());
        }

        assert!(stored_task(&fixture, &task.id).is_none_or(|stored| stored.pr_url.is_none()));
        assert_eq!(fixture.board_update_count(), 0);
        assert_eq!(merge_batch_count(&fixture), 0);
        assert!(event_keys.is_empty());
    }
}

// parity: TS-main-kanbanService-612383
#[test]
fn unchanged_pr_url_is_not_rewritten_but_the_task_must_still_exist() {
    for does_exist in [true, false] {
        let fixture = KanbanFixture::new();
        let pr_url = "https://example.invalid/pr/2";
        let task = add_task(&fixture, "task", TaskStatus::Progress, |task| {
            task.branch_name = Some("feature".into());
            task.worktree_path = Some("/fixture/repo".into());
            task.pr_url = Some(pr_url.into());
        });
        let merged = serde_json::json!([{ "url": pr_url, "state": "MERGED", "mergedAt": "2026-11-01T00:00:00Z" }]).to_string();
        let change = if does_exist {
            "UPDATE kanban_tasks SET title = title WHERE id = ?1"
        } else {
            "DELETE FROM kanban_tasks WHERE id = ?1"
        };
        change_task_during_lookup(&fixture, &task.id, change, merged);

        let result = sync(&fixture);

        assert_eq!(result.merged_pull_requests.len(), usize::from(does_exist));
        assert!(result.updated_task_ids.is_empty());
        if does_exist {
            assert_eq!(
                stored_task(&fixture, &task.id).unwrap().updated_at,
                task.updated_at
            );
        }
    }
}

/// 원격 task worktree에서 조회하는 fallback 테스트 task
fn fallback_task(fixture: &KanbanFixture) -> KanbanTask {
    let project = add_project(fixture, "/projects/repo", Some("dev-vm"));
    add_task(fixture, "fallback", TaskStatus::Progress, |task| {
        with_branch(&project, "feat/fallback", Some("/tasks/fallback"))(task);
        task.ssh_host = Some("dev-vm".into());
    })
}

const UNRESOLVED: &str =
    "GraphQL: Could not resolve to a Repository with the name 'stale/repo'. (repository)";

// parity: TS-main-kanbanService-54ccf0
#[test]
fn task_worktree_is_queried_before_the_project() {
    let fixture = KanbanFixture::new();
    let task = fallback_task(&fixture);
    fixture
        .responses
        .returning("https://github.com/example/repo/pull/12");

    fixture
        .service
        .fetch_and_save_pr_url(&task.id, false)
        .unwrap();

    assert!(last_command(&fixture).contains("repo='/tasks/fallback'"));
}

// parity: TS-main-kanbanService-93e1f5
#[test]
fn unresolved_repository_is_retried_with_the_git_origin() {
    for remote_url in [
        "git@github.com:example/repo.git",
        "https://github.com/example/repo.git",
        "ssh://git@github.com/example/repo.git",
    ] {
        let fixture = KanbanFixture::new();
        let task = fallback_task(&fixture);
        respond_in_sequence(
            &fixture,
            vec![
                Err(UNRESOLVED),
                Ok(remote_url),
                Ok("https://github.com/example/repo/pull/12"),
            ],
        );

        let result = fixture
            .service
            .fetch_and_save_pr_url(&task.id, false)
            .unwrap();

        assert_eq!(
            result.as_deref(),
            Some("https://github.com/example/repo/pull/12")
        );
        let calls = fixture.runner.calls();
        assert_eq!(calls.len(), 3);
        assert!(calls[1].command.contains("git remote get-url origin"));
        assert!(calls[2].command.contains("--repo 'example/repo'"));
        assert!(
            calls
                .iter()
                .all(|call| call.command.contains("repo='/tasks/fallback'")
                    && call.ssh_host.as_deref() == Some("dev-vm"))
        );
    }
}

// parity: TS-main-kanbanService-908605
#[test]
fn background_status_lookup_uses_the_same_fallback() {
    let fixture = KanbanFixture::new();
    let task = fallback_task(&fixture);
    let open =
        serde_json::json!([{ "url": "https://github.com/example/repo/pull/12", "state": "OPEN" }])
            .to_string();
    respond_in_sequence(
        &fixture,
        vec![
            Err(UNRESOLVED),
            Ok("git@github.com:example/repo.git"),
            Ok(&open),
        ],
    );

    assert_eq!(sync(&fixture).updated_task_ids, [task.id]);
    let command = last_command(&fixture);
    assert!(command.contains("--repo 'example/repo'") && command.contains("--state all"));
}

// parity: TS-main-kanbanService-6711a1
#[test]
fn unreadable_git_origin_keeps_the_original_error() {
    let fixture = KanbanFixture::new();
    let task = fallback_task(&fixture);
    respond_in_sequence(&fixture, vec![Err(UNRESOLVED), Err("no origin")]);

    assert_eq!(
        fixture
            .service
            .fetch_and_save_pr_url(&task.id, false)
            .unwrap_err()
            .message,
        UNRESOLVED
    );
}

// parity: TS-main-kanbanService-ae75a6
#[test]
fn non_github_origin_keeps_the_original_error() {
    let fixture = KanbanFixture::new();
    let task = fallback_task(&fixture);
    let error = "GraphQL: Could not resolve to a Repository";
    respond_in_sequence(
        &fixture,
        vec![Err(error), Ok("git@gitlab.com:example/repo.git")],
    );

    assert_eq!(
        fixture
            .service
            .fetch_and_save_pr_url(&task.id, false)
            .unwrap_err()
            .message,
        error
    );
    assert_eq!(fixture.runner.calls().len(), 2);
}

// parity: TS-main-kanbanService-cfac78
#[test]
fn failed_retry_with_an_explicit_repository_returns_that_error() {
    let fixture = KanbanFixture::new();
    let task = fallback_task(&fixture);
    respond_in_sequence(
        &fixture,
        vec![
            Err("GraphQL: Could not resolve to a Repository"),
            Ok("git@github.com:example/repo.git"),
            Err("HTTP 403: forbidden"),
        ],
    );

    assert_eq!(
        fixture
            .service
            .fetch_and_save_pr_url(&task.id, false)
            .unwrap_err()
            .message,
        "HTTP 403: forbidden"
    );
}

// parity: TS-main-kanbanService-40b6c5
#[test]
fn empty_results_do_not_trigger_the_fallback() {
    let fixture = KanbanFixture::new();
    let task = fallback_task(&fixture);

    assert_eq!(
        fixture.service.fetch_and_save_pr_url(&task.id, false),
        Ok(None)
    );
    assert_eq!(fixture.runner.calls().len(), 1);
}
