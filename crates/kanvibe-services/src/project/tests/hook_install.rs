//! TS `projectService local hook installation` 앞부분: provider 재설치, root task 기준 조회, 목록 조회와 explicit sync의 분리

use kanvibe_process::hooks::providers::KanvibeHookProvider;

use super::fakes::{Harness, HookCall, hook_status, worktree};

const REPO: &str = "/workspace/repo";

fn provider_calls(harness: &Harness) -> Vec<HookCall> {
    harness.hooks.provider_install_calls.lock().unwrap().clone()
}

// parity: TS-main-projectService-d05f43
#[test]
fn local_claude_install_reinstalls_only_the_claude_provider() {
    let harness = Harness::new();
    let project = harness.project("repo", REPO, None);
    let task = harness.task(|task| {
        task.project_id = Some(project.id.clone());
        task.worktree_path = Some("/workspace/task-1".into());
    });

    harness
        .service
        .install_task_hooks(&task.id, KanvibeHookProvider::Claude)
        .unwrap();

    assert_eq!(
        provider_calls(&harness),
        [(
            "/workspace/task-1".to_string(),
            task.id,
            Some(KanvibeHookProvider::Claude),
            None
        )]
    );
    assert!(harness.hooks.install_calls.lock().unwrap().is_empty());
}

// parity: TS-main-projectService-c560ca
#[test]
fn local_open_code_install_reinstalls_only_the_open_code_provider_on_the_root_task() {
    let harness = Harness::new();
    let project = harness.project("repo", REPO, None);
    let root = harness.root_task(&project, Some(REPO));
    let task = harness.task(|task| task.project_id = Some(project.id.clone()));

    harness
        .service
        .install_task_hooks(&task.id, KanvibeHookProvider::OpenCode)
        .unwrap();

    assert_eq!(
        provider_calls(&harness),
        [(
            REPO.to_string(),
            root.id,
            Some(KanvibeHookProvider::OpenCode),
            None
        )]
    );
    assert!(harness.hooks.install_calls.lock().unwrap().is_empty());
}

// parity: TS-main-projectService-13ab72
#[test]
fn project_root_task_status_is_read_for_the_current_root_task() {
    let harness = Harness::new();
    let project = harness.project("repo", REPO, None);
    let root = harness.root_task(&project, Some(REPO));
    let stale = harness.task(|task| {
        task.project_id = Some(project.id.clone());
        task.worktree_path = Some(REPO.into());
    });

    harness
        .service
        .get_task_hooks_status(&stale.id, KanvibeHookProvider::Claude)
        .unwrap();

    let calls = harness.hooks.status_calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        [(
            KanvibeHookProvider::Claude,
            REPO.to_string(),
            Some(root.id),
            None
        )]
    );
}

// parity: TS-main-projectService-b2be88
#[test]
fn project_root_task_reinstall_rebinds_claude_to_the_current_root_task() {
    let harness = Harness::new();
    let project = harness.project("repo", REPO, None);
    let root = harness.root_task(&project, Some(REPO));
    let stale = harness.task(|task| {
        task.project_id = Some(project.id.clone());
        task.worktree_path = Some(REPO.into());
    });

    harness
        .service
        .install_task_hooks(&stale.id, KanvibeHookProvider::Claude)
        .unwrap();

    assert_eq!(
        provider_calls(&harness),
        [(
            REPO.to_string(),
            root.id,
            Some(KanvibeHookProvider::Claude),
            None
        )]
    );
    assert!(harness.hooks.install_calls.lock().unwrap().is_empty());
}

// parity: TS-main-projectService-11a128
#[test]
fn local_default_branch_task_gets_hooks_even_if_it_stays_todo() {
    let harness = Harness::new();
    *harness.sessions.created_session.lock().unwrap() = Err("tmux missing".into());

    let result = harness
        .service
        .register_project("api", "/workspace/api", None)
        .unwrap();

    assert!(result.is_success);
    assert_eq!(harness.projects().len(), 1);
    let root = harness.branch_task("main").unwrap();
    let ensured = harness.hooks.ensure_calls.lock().unwrap().clone();
    assert_eq!(
        ensured,
        [("/workspace/api".to_string(), root.id, None, None)]
    );
}

// parity: TS-main-projectService-49ab3f
#[test]
fn legacy_project_is_repaired_by_explicit_sync_not_by_listing() {
    let harness = Harness::new();
    let project = harness.project("api", "/workspace/api", None);

    let projects = harness.service.get_all_projects().unwrap();
    assert_eq!(projects, [project]);
    assert!(harness.repositories.listed_repo_paths().is_empty());
    assert!(harness.hooks.install_calls.lock().unwrap().is_empty());
    assert_eq!(harness.events.board_update_count(), 0);
    assert!(harness.tasks().is_empty());

    harness
        .repositories
        .returning_worktrees(vec![worktree("/workspace/api", "main")]);
    let result = harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert!(result.has_changed);
    assert!(result.errors.is_empty());
    assert!(harness.branch_task("main").is_some());
    assert!(harness.hooks.install_calls.lock().unwrap().is_empty());
}

// parity: TS-main-projectService-f25d76
#[test]
fn failed_root_task_sync_is_retried_on_the_next_sync() {
    let harness = Harness::new();
    harness.project("api", "/workspace/api", None);
    harness
        .repositories
        .returning_worktrees(vec![worktree("/workspace/api", "main")]);
    *harness.sessions.created_session.lock().unwrap() = Ok("api-main".into());
    *harness.state_files.next_read_error.lock().unwrap() = Some("database offline".into());

    let failed = harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();
    assert!(failed.errors.join(" ").contains("database offline"));
    assert!(harness.tasks().is_empty());
    assert!(harness.hooks.install_calls.lock().unwrap().is_empty());
    assert_eq!(harness.events.board_update_count(), 0);

    let retried = harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();
    assert!(retried.has_changed);
    assert!(retried.errors.is_empty());
    assert!(harness.branch_task("main").is_some());
    assert!(harness.hooks.install_calls.lock().unwrap().is_empty());
    assert_eq!(harness.events.board_update_count(), 0);
}

fn assert_no_hook_work(harness: &Harness) {
    assert!(harness.hooks.status_calls.lock().unwrap().is_empty());
    assert!(harness.hooks.statuses_calls.lock().unwrap().is_empty());
    assert!(harness.hooks.install_calls.lock().unwrap().is_empty());
    assert!(harness.hooks.ensure_calls.lock().unwrap().is_empty());
}

// parity: TS-main-projectService-26ef71
#[test]
fn listing_projects_does_not_read_remote_root_hook_status() {
    let harness = Harness::new();
    let project = harness.project("prompt", "/remote/prompt", Some("remote-host"));
    harness.root_task(&project, Some("/remote/prompt"));

    harness.service.get_all_projects().unwrap();
    harness.service.join_background_jobs();

    assert_no_hook_work(&harness);
}

// parity: TS-main-projectService-ea5945
#[test]
fn listing_projects_does_not_install_remote_root_hooks() {
    let harness = Harness::new();
    let project = harness.project(
        "techtaurant-be",
        "/remote/techtaurant-be",
        Some("remote-host"),
    );
    let mut project = project;
    project.default_branch = "dev".into();
    harness.root_task(&project, Some("/remote/techtaurant-be"));

    harness.service.get_all_projects().unwrap();
    harness.service.join_background_jobs();

    assert_no_hook_work(&harness);
}

// parity: TS-main-projectService-faeab0
#[test]
fn project_hook_reinstall_continues_when_the_legacy_root_task_is_missing() {
    let harness = Harness::new();
    let project = harness.project("api", "/workspace/api", None);
    *harness.sessions.created_session.lock().unwrap() = Err("tmux missing".into());
    *harness.hooks.is_installed.lock().unwrap() = true;

    let result = harness
        .service
        .install_project_hooks(&project.id, KanvibeHookProvider::Claude)
        .unwrap();

    assert!(result.is_success);
    assert_eq!(
        result.status,
        Some(hook_status(KanvibeHookProvider::Claude, true))
    );
    let root = harness.branch_task("main").unwrap();
    assert_eq!(
        provider_calls(&harness),
        [(
            "/workspace/api".to_string(),
            root.id,
            Some(KanvibeHookProvider::Claude),
            None
        )]
    );
    assert!(harness.hooks.install_calls.lock().unwrap().is_empty());
}
