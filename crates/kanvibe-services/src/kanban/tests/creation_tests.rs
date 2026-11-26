//! task 생성, 터미널 연결, 브랜치 분기에서 hook 설치와 공유 상태 기록 순서.

use std::sync::Arc;
use std::time::Duration;

use kanvibe_domain::task::{SessionType, TaskPriority, TaskStatus};
use kanvibe_process::worktree::WorktreeSession;

use super::fakes::{Gate, KanbanFixture, wait_until};
use super::{add_project, add_task, with_branch};
use crate::board_notifier::{BoardEvent, TaskHookInstallFailedPayload};
use crate::kanban::{CreateTaskInput, KanbanTimings};

fn feature_input(project_id: &str, title: &str, branch_name: &str) -> CreateTaskInput {
    CreateTaskInput {
        title: title.into(),
        branch_name: Some(branch_name.into()),
        project_id: Some(project_id.into()),
        session_type: Some(SessionType::Tmux),
        ..CreateTaskInput::default()
    }
}

fn set_created_worktree(fixture: &KanbanFixture, worktree_path: &str) {
    *fixture.worktrees.created_session.lock().unwrap() = WorktreeSession {
        worktree_path: worktree_path.into(),
        session_name: "task-1".into(),
    };
}

#[test]
fn creating_a_local_worktree_task_installs_hook_files_and_records_todo() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", None);

    let created = fixture
        .service
        .create_task(&feature_input(
            &project.id,
            "알림 회귀 수정",
            "fix/notifications",
        ))
        .unwrap();

    let installs = fixture.hooks.file_installs.lock().unwrap().clone();
    assert_eq!(installs.len(), 1);
    assert_eq!(installs[0].target_path, "/workspace/repo-worktrees/task-1");
    assert_eq!(installs[0].task_id, created.id);
    assert_eq!(installs[0].ssh_host, None);
    assert_eq!(fixture.hooks.verifications.lock().unwrap().len(), 1);
    let status_writes = fixture.state.writes_of("status");
    assert_eq!(
        status_writes[0].repo_path,
        "/workspace/repo-worktrees/task-1"
    );
    assert_eq!(status_writes[0].content, "todo");
    assert_eq!(status_writes[0].ssh_host, None);
}

/// 생성을 다른 스레드에서 돌리고, hook 설치가 붙잡힌 동안 생성이 끝나지 않는지 본다
fn assert_creation_waits_for_hook_install(ssh_host: Option<&str>, worktree_path: &str) {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/remote/repo", ssh_host);
    set_created_worktree(&fixture, worktree_path);
    let gate = fixture.hooks.hold_installs();
    let service = Arc::clone(&fixture.service);
    let input = feature_input(&project.id, "원격 hooks 보장", "fix/remote-hooks");
    let creation = std::thread::spawn(move || service.create_task(&input));

    wait_until(|| fixture.log.count("install-files") == 1);
    std::thread::sleep(Duration::from_millis(30));
    assert!(!creation.is_finished());
    assert_eq!(fixture.board_update_count(), 0);
    assert_eq!(fixture.log.count("schedule-verification"), 0);

    gate.open();
    let created = creation.join().unwrap().unwrap();
    let install = fixture.hooks.file_installs.lock().unwrap()[0].clone();
    assert_eq!(install.task_id, created.id);
    assert_eq!(install.ssh_host.as_deref(), ssh_host);
    let install_at = fixture.log.position("install-files").unwrap();
    let verification_at = fixture.log.position("schedule-verification").unwrap();
    assert!(install_at < verification_at);
    assert!(verification_at < fixture.log.position("board:board-updated").unwrap());
    assert_eq!(fixture.board_update_count(), 1);
}

#[test]
fn worktree_task_creation_returns_after_hook_install_finishes() {
    assert_creation_waits_for_hook_install(None, "/workspace/repo-worktrees/task-1");
}

#[test]
fn remote_worktree_task_creation_returns_after_hook_install_finishes() {
    assert_creation_waits_for_hook_install(Some("remote-host"), "/remote/repo-worktrees/task-1");
}

#[test]
fn hooks_status_and_metadata_run_together_and_all_finish_before_returning() {
    for ssh_host in [None, Some("remote-host")] {
        let fixture = KanbanFixture::new();
        let project = add_project(&fixture, "/workspace/repo", ssh_host);
        let hooks_gate = fixture.hooks.hold_installs();
        let (status_gate, metadata_gate) = (Gate::default(), Gate::default());
        *fixture.state.status_gate.lock().unwrap() = Some(status_gate.clone());
        *fixture.state.metadata_gate.lock().unwrap() = Some(metadata_gate.clone());
        let service = Arc::clone(&fixture.service);
        let input = CreateTaskInput {
            description: Some("상태와 함께 저장할 설명".into()),
            priority: Some(TaskPriority::High),
            ..feature_input(&project.id, "생성 대기 개선", "perf/create")
        };
        let creation = std::thread::spawn(move || service.create_task(&input));

        wait_until(|| {
            fixture.log.count("install-files") == 1
                && fixture.log.count("state:status") == 1
                && fixture.log.count("state:metadata") == 1
        });
        let metadata = fixture.state.writes_of("metadata")[0].clone();
        assert_eq!(metadata.repo_path, "/workspace/repo-worktrees/task-1");
        assert!(metadata.content.contains("상태와 함께 저장할 설명"));
        assert_eq!(metadata.ssh_host.as_deref(), ssh_host);
        assert!(!creation.is_finished());

        hooks_gate.open();
        wait_until(|| fixture.log.count("schedule-verification") == 1);
        status_gate.open();
        std::thread::sleep(Duration::from_millis(20));
        assert!(!creation.is_finished());
        assert_eq!(fixture.board_update_count(), 0);

        metadata_gate.open();
        let created = creation.join().unwrap().unwrap();
        assert_eq!(
            created.description.as_deref(),
            Some("상태와 함께 저장할 설명")
        );
        assert_eq!(created.priority, Some(TaskPriority::High));
        assert_eq!(created.ssh_host.as_deref(), ssh_host);
        assert_eq!(fixture.board_update_count(), 1);
    }
}

#[test]
fn hook_install_failure_is_broadcast_and_the_task_is_still_returned() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", None);
    *fixture.hooks.install_error.lock().unwrap() = Some("codex config failed".into());

    let created = fixture
        .service
        .create_task(&feature_input(
            &project.id,
            "알림 회귀 수정",
            "fix/notifications",
        ))
        .unwrap();

    assert!(
        fixture
            .events()
            .contains(&BoardEvent::TaskHookInstallFailed(
                TaskHookInstallFailedPayload {
                    task_id: created.id.clone(),
                    task_title: "알림 회귀 수정".into(),
                    error: "codex config failed".into(),
                }
            ))
    );
    assert_eq!(fixture.log.count("schedule-verification"), 0);
    assert_eq!(fixture.board_update_count(), 1);
}

#[test]
fn successful_hook_install_broadcasts_one_board_update() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/remote/repo", Some("remote-host"));
    set_created_worktree(&fixture, "/remote/repo-worktrees/task-1");

    fixture
        .service
        .create_task(&feature_input(
            &project.id,
            "원격 hooks 성공",
            "fix/remote-hooks-success",
        ))
        .unwrap();

    let install = fixture.hooks.file_installs.lock().unwrap()[0].clone();
    assert_eq!(install.target_path, "/remote/repo-worktrees/task-1");
    assert_eq!(install.ssh_host.as_deref(), Some("remote-host"));
    assert_eq!(fixture.board_update_count(), 1);
}

fn add_remote_branch_task(fixture: &KanbanFixture, branch: &str, status: TaskStatus) -> String {
    let project = add_project(fixture, "/remote/repo", Some("remote-host"));
    let worktree_path = format!("/remote/repo__worktrees/{}", branch.replace('/', "-"));
    let task = add_task(fixture, "원격 브랜치 연결", status, |task| {
        with_branch(&project, branch, Some(&worktree_path))(task);
        task.base_branch = Some("main".into());
        task.ssh_host = Some("remote-host".into());
    });
    task.id
}

#[test]
fn connecting_a_remote_branch_task_creates_the_session_after_hooks_install() {
    let fixture = KanbanFixture::new();
    let task_id = add_remote_branch_task(&fixture, "feature/remote", TaskStatus::Todo);
    let gate = fixture.hooks.hold_installs();
    let service = Arc::clone(&fixture.service);
    let connect_id = task_id.clone();
    let connection = std::thread::spawn(move || {
        service.connect_terminal_session(&connect_id, SessionType::Tmux)
    });

    wait_until(|| fixture.log.count("install-files") == 1);
    assert_eq!(fixture.log.count("create-session"), 0);
    assert_eq!(fixture.board_update_count(), 0);
    gate.open();
    let connected = connection.join().unwrap().unwrap().unwrap();

    assert_eq!(connected.session_type, Some(SessionType::Tmux));
    assert_eq!(
        connected.session_name.as_deref(),
        Some("repo-feature-remote")
    );
    assert_eq!(connected.status, TaskStatus::Todo);
    let creation = fixture.worktrees.session_creations.lock().unwrap()[0].clone();
    assert_eq!(creation.project_path, "/remote/repo");
    assert_eq!(creation.branch_name, "feature/remote");
    assert_eq!(creation.ssh_host.as_deref(), Some("remote-host"));
    assert_eq!(
        creation.working_directory,
        "/remote/repo__worktrees/feature-remote"
    );
    let verification_at = fixture.log.position("schedule-verification").unwrap();
    assert!(fixture.log.position("install-files").unwrap() < verification_at);
    assert!(verification_at < fixture.log.position("create-session").unwrap());
    let status = fixture.state.writes_of("status")[0].clone();
    assert_eq!(status.repo_path, "/remote/repo__worktrees/feature-remote");
    assert_eq!(
        (status.content.as_str(), status.ssh_host.as_deref()),
        ("todo", Some("remote-host"))
    );
    assert_eq!(fixture.board_update_count(), 1);
}

#[test]
fn connecting_a_task_that_already_has_a_session_returns_it_unchanged() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/remote/repo", None);
    let task = add_task(
        &fixture,
        "이미 연결된 task",
        TaskStatus::Progress,
        |task| {
            with_branch(
                &project,
                "feature/connected",
                Some("/remote/repo__worktrees/feature-connected"),
            )(task);
            super::tmux_session(task, "repo-feature-connected");
        },
    );

    let result = fixture
        .service
        .connect_terminal_session(&task.id, SessionType::Zellij)
        .unwrap()
        .unwrap();

    assert_eq!(result, task);
    assert_eq!(fixture.log.count("create-session"), 0);
    assert_eq!(fixture.board_update_count(), 0);
}

#[test]
fn connecting_a_branch_task_keeps_its_current_status() {
    let fixture = KanbanFixture::new();
    let task_id = add_remote_branch_task(&fixture, "feature/review", TaskStatus::Review);

    let connected = fixture
        .service
        .connect_terminal_session(&task_id, SessionType::Tmux)
        .unwrap()
        .unwrap();

    assert_eq!(connected.status, TaskStatus::Review);
    assert_eq!(
        super::stored_task(&fixture, &task_id).unwrap().status,
        TaskStatus::Review
    );
    let status = fixture.state.writes_of("status")[0].clone();
    assert_eq!(status.repo_path, "/remote/repo__worktrees/feature-review");
    assert_eq!(status.content, "review");
}

#[test]
fn connecting_continues_creating_the_session_when_hooks_install_is_slow() {
    let fixture = KanbanFixture::with_timings(KanbanTimings {
        terminal_hook_install_pre_attach_wait: Duration::from_millis(50),
    });
    let task_id = add_remote_branch_task(&fixture, "feature/slow-hooks", TaskStatus::Todo);
    let gate = fixture.hooks.hold_installs();

    let connected = fixture
        .service
        .connect_terminal_session(&task_id, SessionType::Tmux)
        .unwrap()
        .unwrap();

    assert_eq!(
        connected.session_name.as_deref(),
        Some("repo-feature-remote")
    );
    assert_eq!(connected.status, TaskStatus::Todo);
    assert_eq!(fixture.log.count("create-session"), 1);
    assert_eq!(fixture.log.count("schedule-verification"), 0);
    assert_eq!(fixture.board_update_count(), 1);
    gate.open();
    wait_until(|| fixture.log.count("schedule-verification") == 1);
    let verification = fixture.hooks.verifications.lock().unwrap()[0].clone();
    assert_eq!(
        verification.target_path,
        "/remote/repo__worktrees/feature-slow-hooks"
    );
}

#[test]
fn branching_from_a_task_installs_hooks_then_schedules_verification_then_updates_the_board() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/remote/repo", Some("remote-host"));
    let task = add_task(&fixture, "원격 브랜치 생성", TaskStatus::Todo, |_| {});
    *fixture.worktrees.created_session.lock().unwrap() = WorktreeSession {
        worktree_path: "/remote/repo__worktrees/feature-from-task".into(),
        session_name: "repo-feature-from-task".into(),
    };
    let gate = fixture.hooks.hold_installs();
    let service = Arc::clone(&fixture.service);
    let (task_id, project_id) = (task.id.clone(), project.id.clone());
    let branching = std::thread::spawn(move || {
        service.branch_from_task(
            &task_id,
            &project_id,
            "main",
            "feature/from-task",
            SessionType::Zellij,
        )
    });

    wait_until(|| fixture.log.count("install-files") == 1);
    std::thread::sleep(Duration::from_millis(20));
    assert!(!branching.is_finished());
    assert_eq!(fixture.board_update_count(), 0);
    gate.open();
    let branched = branching.join().unwrap().unwrap().unwrap();

    assert_eq!(branched.project_id.as_deref(), Some(project.id.as_str()));
    assert_eq!(branched.branch_name.as_deref(), Some("feature/from-task"));
    assert_eq!(branched.session_type, Some(SessionType::Zellij));
    assert_eq!(
        branched.session_name.as_deref(),
        Some("repo-feature-from-task")
    );
    assert_eq!(branched.status, TaskStatus::Progress);
    let verification_at = fixture.log.position("schedule-verification").unwrap();
    assert!(fixture.log.position("install-files").unwrap() < verification_at);
    assert!(verification_at < fixture.log.position("board:board-updated").unwrap());
    let status = fixture.state.writes_of("status")[0].clone();
    assert_eq!(
        status.repo_path,
        "/remote/repo__worktrees/feature-from-task"
    );
    assert_eq!(
        (status.content.as_str(), status.ssh_host.as_deref()),
        ("progress", Some("remote-host"))
    );
    assert_eq!(fixture.board_update_count(), 1);
}
