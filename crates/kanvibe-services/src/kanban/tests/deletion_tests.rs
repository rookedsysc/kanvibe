//! task 삭제·등록 제거와 다른 client로의 삭제 전파.

use std::sync::Arc;

use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskStatus};
use kanvibe_process::exec::ShellCommandRunner;
use kanvibe_process::task_target_propagation::PeerTargetResult;
use kanvibe_process::worktree::ResourceCleanupOptions;

use super::fakes::{KanbanFixture, SessionRemoval, WorktreeRemoval};
use super::{add_project, add_task, stored_task, tmux_session, with_branch};
use crate::board_notifier::{BoardEvent, TaskPeerDeletionFailedPayload, TaskSessionChangedPayload};
use crate::kanban::{TaskDeletion, TaskDeletionError, TaskDeletionOptions};
use crate::test_support::LoopbackShellRunner;

fn removal(
    project_path: &str,
    branch: &str,
    host: Option<&str>,
    worktree: Option<&str>,
) -> WorktreeRemoval {
    WorktreeRemoval {
        project_path: project_path.into(),
        branch_name: branch.into(),
        ssh_host: host.map(str::to_string),
        options: ResourceCleanupOptions {
            should_throw_on_error: true,
            worktree_path: worktree.map(str::to_string),
        },
    }
}

fn tmux_removal(session_name: &str, host: Option<&str>) -> SessionRemoval {
    SessionRemoval {
        session_type: SessionType::Tmux,
        session_name: session_name.into(),
        ssh_host: host.map(str::to_string),
        should_throw_on_error: true,
    }
}

fn worktree_removals(fixture: &KanbanFixture) -> Vec<WorktreeRemoval> {
    fixture.worktrees.worktree_removals.lock().unwrap().clone()
}

fn session_removals(fixture: &KanbanFixture) -> Vec<SessionRemoval> {
    fixture.worktrees.session_removals.lock().unwrap().clone()
}

fn detached(fixture: &KanbanFixture) -> Vec<(String, String)> {
    fixture.terminal.detached.lock().unwrap().clone()
}

#[test]
fn deletion_cleans_up_by_project_and_branch_even_when_the_stored_worktree_path_differs() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/remote/repo", Some("remote-host"));
    let task = add_task(&fixture, "dev", TaskStatus::Todo, |task| {
        with_branch(&project, "dev", Some("/Users/local/repo__worktrees/dev"))(task);
        task.ssh_host = Some("remote-host".into());
    });

    assert_eq!(fixture.service.delete_task(&task.id), Ok(true));

    assert_eq!(
        worktree_removals(&fixture),
        [removal(
            "/remote/repo",
            "dev",
            Some("remote-host"),
            Some("/Users/local/repo__worktrees/dev")
        )]
    );
    assert!(stored_task(&fixture, &task.id).is_none());
}

#[test]
fn deletion_without_a_stored_worktree_path_delegates_worktree_lookup_to_cleanup() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", Some("remote-host"));
    let task = add_task(
        &fixture,
        "실제 worktree",
        TaskStatus::Todo,
        with_branch(&project, "feature/actual-worktree", None),
    );

    assert_eq!(fixture.service.delete_task(&task.id), Ok(true));

    assert_eq!(
        worktree_removals(&fixture),
        [removal(
            "/workspace/repo",
            "feature/actual-worktree",
            Some("remote-host"),
            None
        )]
    );
    assert!(stored_task(&fixture, &task.id).is_none());
    assert_eq!(fixture.board_update_count(), 1);
}

#[test]
fn deleting_a_remote_tmux_task_without_session_name_derives_the_session_name() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/remote/repo", Some("remote-host"));
    let task = add_task(
        &fixture,
        "세션 정리",
        TaskStatus::Todo,
        with_branch(&project, "feature/cleanup-session", None),
    );

    assert_eq!(fixture.service.delete_task(&task.id), Ok(true));

    assert_eq!(
        detached(&fixture),
        [(task.id.clone(), "cleanup-task-resources".to_string())]
    );
    assert_eq!(
        session_removals(&fixture),
        [tmux_removal(
            "repo-feature-cleanup-session",
            Some("remote-host")
        )]
    );
    assert_eq!(
        worktree_removals(&fixture),
        [removal(
            "/remote/repo",
            "feature/cleanup-session",
            Some("remote-host"),
            None
        )]
    );
    assert!(stored_task(&fixture, &task.id).is_none());
    assert_eq!(fixture.board_update_count(), 1);
}

fn add_stale_remote_task(
    fixture: &KanbanFixture,
    project_id: Option<&str>,
    branch: Option<&str>,
    worktree: Option<&str>,
) -> KanbanTask {
    add_task(fixture, "stale", TaskStatus::Todo, |task| {
        task.project_id = project_id.map(str::to_string);
        task.branch_name = branch.map(str::to_string);
        task.worktree_path = worktree.map(str::to_string);
        task.ssh_host = Some("roky-home".into());
        tmux_session(task, "prompt-main");
    })
}

fn assert_stale_task_is_kept_after_session_cleanup(project_id: Option<&str>) {
    let fixture = KanbanFixture::new();
    let task = add_stale_remote_task(
        &fixture,
        project_id,
        Some("main"),
        Some("/home/rookedsysc/Downloads/prompt"),
    );

    let error = fixture.service.delete_task(&task.id).unwrap_err();

    assert!(error.to_string().contains("연결된 프로젝트를 찾을 수 없어"));
    assert_eq!(
        detached(&fixture),
        [(task.id.clone(), "cleanup-task-resources".to_string())]
    );
    assert_eq!(
        session_removals(&fixture),
        [tmux_removal("prompt-main", Some("roky-home"))]
    );
    assert!(worktree_removals(&fixture).is_empty());
    assert!(stored_task(&fixture, &task.id).is_some());
}

#[test]
fn stale_remote_task_with_a_missing_project_is_kept_after_session_cleanup() {
    assert_stale_task_is_kept_after_session_cleanup(Some("missing-project"));
}

#[test]
fn orphaned_remote_task_without_project_is_kept_after_session_cleanup() {
    assert_stale_task_is_kept_after_session_cleanup(None);
}

#[test]
fn session_only_remote_task_is_deleted_even_without_its_project() {
    let fixture = KanbanFixture::new();
    let task = add_stale_remote_task(&fixture, Some("missing-project"), None, None);

    assert_eq!(fixture.service.delete_task(&task.id), Ok(true));

    assert_eq!(
        session_removals(&fixture),
        [tmux_removal("prompt-main", Some("roky-home"))]
    );
    assert!(worktree_removals(&fixture).is_empty());
    assert!(stored_task(&fixture, &task.id).is_none());
    assert_eq!(fixture.board_update_count(), 1);
}

#[test]
fn failed_session_cleanup_keeps_the_task_record() {
    let fixture = KanbanFixture::new();
    let task = add_stale_remote_task(
        &fixture,
        Some("missing-project"),
        Some("main"),
        Some("/home/rookedsysc/Downloads/prompt"),
    );
    *fixture.worktrees.session_removal_error.lock().unwrap() = Some("ssh failed".into());

    let error = fixture.service.delete_task(&task.id).unwrap_err();

    assert_eq!(error.to_string(), "ssh failed");
    assert_eq!(
        session_removals(&fixture),
        [tmux_removal("prompt-main", Some("roky-home"))]
    );
    assert!(stored_task(&fixture, &task.id).is_some());
    assert_eq!(fixture.board_update_count(), 0);
}

#[test]
fn active_terminal_ptys_are_detached_before_the_tmux_session_is_removed() {
    let fixture = KanbanFixture::new();
    let task = add_task(&fixture, "열린 터미널", TaskStatus::Todo, |task| {
        tmux_session(task, "repo-feat-open")
    });

    fixture.service.delete_task(&task.id).unwrap();

    assert!(
        fixture.log.position("detach").unwrap() < fixture.log.position("remove-session").unwrap()
    );
    assert_eq!(
        session_removals(&fixture),
        [tmux_removal("repo-feat-open", None)]
    );
}

fn add_merge_task(fixture: &KanbanFixture, project: &Project, branch: &str) -> KanbanTask {
    let worktree = format!("/workspace/repo__worktrees/{}", branch.replace('/', "-"));
    add_task(
        fixture,
        branch,
        TaskStatus::Review,
        with_branch(project, branch, Some(&worktree)),
    )
}

#[test]
fn bulk_deletion_returns_the_ids_it_actually_deleted() {
    let fixture = KanbanFixture::new();
    let task = add_task(&fixture, "removed", TaskStatus::Todo, |_| {});

    assert_eq!(
        fixture.service.delete_tasks(std::slice::from_ref(&task.id)),
        Ok(vec![task.id])
    );
}

#[test]
fn bulk_deletion_cleans_up_in_request_order_and_updates_the_board_once() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", None);
    let task_b = add_merge_task(&fixture, &project, "feature/merge-b");
    let task_a = add_merge_task(&fixture, &project, "feature/merge-a");
    let requested = [&task_a.id, "missing-task", &task_b.id, &task_a.id].map(str::to_string);

    let deleted = fixture.service.delete_tasks(&requested).unwrap();

    assert_eq!(deleted, [task_a.id.clone(), task_b.id.clone()]);
    assert_eq!(
        worktree_removals(&fixture),
        [
            removal(
                "/workspace/repo",
                "feature/merge-a",
                None,
                Some("/workspace/repo__worktrees/feature-merge-a")
            ),
            removal(
                "/workspace/repo",
                "feature/merge-b",
                None,
                Some("/workspace/repo__worktrees/feature-merge-b")
            ),
        ]
    );
    let session_changes: Vec<BoardEvent> = fixture
        .events()
        .into_iter()
        .filter(|event| matches!(event, BoardEvent::TaskSessionChanged(_)))
        .collect();
    let changed = |task_id: &str| {
        BoardEvent::TaskSessionChanged(TaskSessionChangedPayload {
            task_id: task_id.into(),
        })
    };
    assert_eq!(session_changes, [changed(&task_a.id), changed(&task_b.id)]);
    assert_eq!(fixture.board_update_count(), 1);
}

const SHARED_WORKTREE: &str = "/remote/repo__worktrees/feat-shared";

fn peer_result(url: &str, is_success: bool) -> PeerTargetResult {
    PeerTargetResult {
        url: url.into(),
        is_success,
        http_status: if is_success { "200" } else { "000" }.into(),
        message: if is_success {
            "ok"
        } else {
            "연결할 수 없습니다"
        }
        .into(),
    }
}

/// 다른 client와 worktree를 공유하는 원격 task
fn shared_task_fixture(worktree_path: &str) -> (KanbanFixture, KanbanTask) {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/remote/repo", Some("devbox"));
    let task = add_task(&fixture, "공유 작업", TaskStatus::Progress, |task| {
        with_branch(&project, "feat/shared", Some(worktree_path))(task);
        task.ssh_host = Some("devbox".into());
    });
    *fixture.peers.results.lock().unwrap() = vec![peer_result("http://10.0.0.7:9736", true)];
    (fixture, task)
}

const REJECT_DIRTY_AND_PROPAGATE: TaskDeletionOptions = TaskDeletionOptions {
    should_reject_dirty_worktree: true,
    should_propagate_to_peers: true,
};
const REJECT_DIRTY_ONLY: TaskDeletionOptions = TaskDeletionOptions {
    should_reject_dirty_worktree: true,
    should_propagate_to_peers: false,
};

/// 실행기를 이 머신의 셸로 바꾼다. `prelude`는 명령 앞에 붙일 셸 함수 정의다
fn use_local_shell(fixture: &KanbanFixture, prelude: &str) -> Arc<LoopbackShellRunner> {
    let shell = Arc::new(LoopbackShellRunner::new(fixture.db.directory.path()));
    let (runner, prelude) = (Arc::clone(&shell), prelude.to_string());
    fixture.responses.set(move |call| {
        runner.exec_git(
            &format!("{prelude}{}", call.command),
            call.ssh_host.as_deref(),
            &call.options,
        )
    });
    shell
}

#[test]
fn gui_deletion_propagates_to_peers_before_removing_the_worktree() {
    let (fixture, task) = shared_task_fixture(SHARED_WORKTREE);

    assert_eq!(fixture.service.delete_task(&task.id), Ok(true));

    assert_eq!(
        fixture.peers.calls.lock().unwrap().clone(),
        [(SHARED_WORKTREE.to_string(), Some("devbox".to_string()))]
    );
    assert!(
        fixture.log.position("propagate").unwrap()
            < fixture.log.position("remove-worktree").unwrap()
    );
    assert!(stored_task(&fixture, &task.id).is_none());
    assert!(
        !fixture
            .events()
            .iter()
            .any(|event| matches!(event, BoardEvent::TaskPeerDeletionFailed(_)))
    );
}

fn assert_invalid_worktree_keeps_everything(
    fixture: &KanbanFixture,
    task: &KanbanTask,
    options: &TaskDeletionOptions,
) {
    let error = fixture
        .service
        .delete_task_across_targets(&task.id, options)
        .unwrap_err();

    assert_eq!(error, TaskDeletionError::InvalidWorktree);
    assert!(stored_task(fixture, &task.id).is_some());
    assert!(fixture.peers.calls.lock().unwrap().is_empty());
    assert!(worktree_removals(fixture).is_empty());
}

#[test]
fn missing_or_broken_git_metadata_is_reported_as_registration_only_removal() {
    for metadata in ["missing", "broken"] {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("keep.txt"), "사용자 파일").unwrap();
        if metadata == "broken" {
            std::fs::write(
                directory.path().join(".git"),
                "gitdir: /nonexistent/kanvibe-gitdir\n",
            )
            .unwrap();
        }
        let (fixture, task) = shared_task_fixture(&directory.path().to_string_lossy());
        use_local_shell(&fixture, "");

        assert_invalid_worktree_keeps_everything(&fixture, &task, &REJECT_DIRTY_AND_PROPAGATE);
    }
}

#[test]
fn git_repository_damage_diagnostics_are_reported_as_registration_only_removal() {
    for message in [
        "fatal: not a git repository: /missing/gitdir",
        "fatal: gitfile does not point to a valid repository: /worktree/.git",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let (fixture, task) = shared_task_fixture(&directory.path().to_string_lossy());
        use_local_shell(
            &fixture,
            &format!("git() {{ printf '%s\\n' '{message}' >&2; return 128; }}\n"),
        );

        assert_invalid_worktree_keeps_everything(&fixture, &task, &REJECT_DIRTY_ONLY);
    }
}

#[test]
fn uncommitted_changes_in_a_real_repository_still_block_deletion() {
    for is_dirty in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let (fixture, task) = shared_task_fixture(&directory.path().to_string_lossy());
        let shell = use_local_shell(&fixture, "");
        shell.git(directory.path(), &["init", "-q"]);
        if is_dirty {
            std::fs::write(directory.path().join("keep.txt"), "사용자 변경").unwrap();
        }

        let deletion = fixture
            .service
            .delete_task_across_targets(&task.id, &REJECT_DIRTY_ONLY);

        if is_dirty {
            assert!(matches!(
                deletion,
                Err(TaskDeletionError::DirtyWorktree { .. })
            ));
            assert!(stored_task(&fixture, &task.id).is_some());
        } else {
            assert_eq!(
                deletion,
                Ok(TaskDeletion {
                    is_deleted: true,
                    peer_results: Vec::new()
                })
            );
            assert!(stored_task(&fixture, &task.id).is_none());
        }
    }
}

#[test]
fn git_permission_errors_are_not_mistaken_for_repository_damage() {
    let directory = tempfile::tempdir().unwrap();
    let (fixture, task) = shared_task_fixture(&directory.path().to_string_lossy());
    use_local_shell(
        &fixture,
        "git() { printf 'fatal: permission denied\\n' >&2; return 128; }\n",
    );

    let error = fixture
        .service
        .delete_task_across_targets(&task.id, &REJECT_DIRTY_ONLY)
        .unwrap_err();

    assert!(matches!(error, TaskDeletionError::Failed(_)));
    assert!(error.to_string().contains("permission denied"));
    assert!(stored_task(&fixture, &task.id).is_some());
}

#[test]
fn removing_only_the_registration_touches_no_other_resource() {
    let (fixture, task) = shared_task_fixture(SHARED_WORKTREE);

    assert_eq!(fixture.service.remove_task_registration(&task.id), Ok(true));

    assert!(stored_task(&fixture, &task.id).is_none());
    assert!(fixture.events().contains(&BoardEvent::TaskSessionChanged(
        TaskSessionChangedPayload {
            task_id: task.id.clone()
        }
    )));
    assert_eq!(fixture.board_update_count(), 1);
    assert!(fixture.runner.calls().is_empty());
    assert!(fixture.peers.calls.lock().unwrap().is_empty());
    assert!(detached(&fixture).is_empty());
    assert!(session_removals(&fixture).is_empty() && worktree_removals(&fixture).is_empty());
}

#[test]
fn removing_a_missing_registration_changes_nothing() {
    let fixture = KanbanFixture::new();

    assert_eq!(
        fixture.service.remove_task_registration("missing"),
        Ok(false)
    );

    assert!(fixture.events().is_empty());
}

#[test]
fn ssh_failure_during_the_dirty_check_is_neither_damage_nor_deletion() {
    let (fixture, task) = shared_task_fixture(SHARED_WORKTREE);
    fixture.responses.failing("SSH connection refused");

    let error = fixture
        .service
        .delete_task_across_targets(&task.id, &REJECT_DIRTY_AND_PROPAGATE)
        .unwrap_err();

    assert_eq!(error.to_string(), "SSH connection refused");
    assert!(stored_task(&fixture, &task.id).is_some());
}

#[test]
fn dirty_worktree_stops_propagation_cleanup_and_record_deletion() {
    let (fixture, task) = shared_task_fixture(SHARED_WORKTREE);
    fixture.responses.returning(" M src/app.ts\n");

    let error = fixture
        .service
        .delete_task_across_targets(&task.id, &REJECT_DIRTY_AND_PROPAGATE)
        .unwrap_err();

    assert_eq!(
        error,
        TaskDeletionError::DirtyWorktree {
            worktree_path: SHARED_WORKTREE.into()
        }
    );
    let check = fixture.runner.calls()[0].clone();
    assert!(
        check
            .command
            .contains("git -C '/remote/repo__worktrees/feat-shared' status --porcelain")
    );
    assert_eq!(check.ssh_host.as_deref(), Some("devbox"));
    assert!(fixture.peers.calls.lock().unwrap().is_empty());
    assert!(worktree_removals(&fixture).is_empty());
    assert!(stored_task(&fixture, &task.id).is_some());
}

#[test]
fn clean_deletion_returns_peer_results_and_relayed_deletion_does_not_propagate_again() {
    let (fixture, task) = shared_task_fixture(SHARED_WORKTREE);
    let relayed_options = TaskDeletionOptions {
        should_reject_dirty_worktree: false,
        should_propagate_to_peers: false,
    };

    let clean = fixture
        .service
        .delete_task_across_targets(&task.id, &REJECT_DIRTY_AND_PROPAGATE)
        .unwrap();
    let relayed_task = add_task(&fixture, "공유 작업", TaskStatus::Progress, |relayed| {
        relayed.worktree_path = Some(SHARED_WORKTREE.into());
        relayed.ssh_host = Some("devbox".into());
    });
    let relayed = fixture
        .service
        .delete_task_across_targets(&relayed_task.id, &relayed_options)
        .unwrap();

    assert_eq!(
        clean,
        TaskDeletion {
            is_deleted: true,
            peer_results: vec![peer_result("http://10.0.0.7:9736", true)]
        }
    );
    assert_eq!(
        relayed,
        TaskDeletion {
            is_deleted: true,
            peer_results: Vec::new()
        }
    );
    assert_eq!(fixture.peers.calls.lock().unwrap().len(), 1);
    assert!(
        stored_task(&fixture, &task.id).is_none()
            && stored_task(&fixture, &relayed_task.id).is_none()
    );
    assert_eq!(fixture.log.count("board:task-session-changed"), 2);
}

#[test]
fn deletion_finishes_locally_and_reports_peers_that_kept_the_task() {
    let (fixture, task) = shared_task_fixture(SHARED_WORKTREE);
    *fixture.peers.results.lock().unwrap() = vec![peer_result("http://10.0.0.9:9736", false)];

    assert_eq!(fixture.service.delete_task(&task.id), Ok(true));

    assert!(stored_task(&fixture, &task.id).is_none());
    assert!(
        fixture
            .events()
            .contains(&BoardEvent::TaskPeerDeletionFailed(
                TaskPeerDeletionFailedPayload {
                    task_id: task.id.clone(),
                    task_title: "공유 작업".into(),
                    failed_peer_urls: vec!["http://10.0.0.9:9736".into()],
                }
            ))
    );
}

#[test]
fn bulk_deletion_propagates_each_task_to_peers() {
    let (fixture, task) = shared_task_fixture(SHARED_WORKTREE);
    let other = add_task(&fixture, "다른 작업", TaskStatus::Progress, |other| {
        other.project_id = task.project_id.clone();
        other.branch_name = Some("feat/other".into());
        other.worktree_path = Some("/remote/repo__worktrees/feat-other".into());
        other.ssh_host = Some("devbox".into());
    });

    fixture
        .service
        .delete_tasks(&[task.id.clone(), other.id.clone()])
        .unwrap();

    assert_eq!(
        fixture.peers.calls.lock().unwrap().clone(),
        [
            (SHARED_WORKTREE.to_string(), Some("devbox".to_string())),
            (
                "/remote/repo__worktrees/feat-other".to_string(),
                Some("devbox".to_string())
            ),
        ]
    );
}
