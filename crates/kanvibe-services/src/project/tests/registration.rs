//! TS `projectService remote registration flow`

use std::sync::Arc;

use kanvibe_domain::project::DUPLICATE_PROJECT_NAME_ERROR;

use super::fakes::{Gate, Harness, wait_until, worktree};

#[test]
fn remote_scan_searches_repositories_without_session_dependency_checks() {
    let harness = Harness::new();
    *harness.repositories.scanned_repos.lock().unwrap() = vec!["/workspace/api".into()];

    let result = harness
        .service
        .scan_and_register_projects("~/workspace", Some("remote-host"))
        .unwrap();

    let scans = harness.repositories.scan_calls.lock().unwrap().clone();
    assert_eq!(
        scans,
        [("~/workspace".to_string(), Some("remote-host".to_string()))]
    );
    assert_eq!(result.registered, ["api"]);
}

#[test]
fn remote_registration_survives_a_failed_session_creation() {
    let harness = Harness::new();
    *harness.sessions.created_session.lock().unwrap() = Err("tmux missing".into());

    let result = harness
        .service
        .register_project("api", "/workspace/api", Some("remote-host"))
        .unwrap();

    assert!(result.is_success);
    let project = result.project.unwrap();
    assert_eq!(project.name, "api");
    assert_eq!(project.ssh_host.as_deref(), Some("remote-host"));
    assert_eq!(harness.projects().len(), 1);
}

#[test]
fn remote_default_branch_task_does_not_create_a_tmux_session() {
    let harness = Harness::new();
    harness
        .repositories
        .returning_worktrees(vec![worktree("/remote/api", "main")]);

    let result = harness
        .service
        .register_project("api", "/remote/api", Some("remote-host"))
        .unwrap();

    assert!(result.is_success);
    assert!(harness.sessions.creations.lock().unwrap().is_empty());
}

#[test]
fn bare_common_repo_default_task_keeps_a_null_worktree_path() {
    let harness = Harness::new();
    harness
        .repositories
        .on_command(|_| Ok("/remote/repo/.git".into()));
    harness.repositories.returning_worktrees(vec![worktree(
        "/remote/repo__worktrees/feature-login",
        "feature-login",
    )]);

    let result = harness
        .service
        .register_project("repo", "/remote/repo", Some("remote-host"))
        .unwrap();

    assert!(result.is_success);
    let root = harness.branch_task("main").unwrap();
    assert_eq!(root.worktree_path, None);
    assert_eq!(root.ssh_host.as_deref(), Some("remote-host"));
}

#[test]
fn same_name_on_another_pc_is_registered_unchanged() {
    let harness = Harness::new();
    harness.project("kanvibe", "/workspace/kanvibe", None);

    let result = harness
        .service
        .register_project(
            "kanvibe",
            "/home/rookedsysc/Documents/kanvibe/kanvibe",
            Some("remote-host"),
        )
        .unwrap();

    assert!(result.is_success);
    let project = result.project.unwrap();
    assert_eq!(project.name, "kanvibe");
    assert_eq!(project.ssh_host.as_deref(), Some("remote-host"));
}

#[test]
fn same_name_on_the_same_pc_gets_the_parent_folder() {
    let harness = Harness::new();
    harness.project("kanvibe", "/workspace/kanvibe", Some("remote-host"));

    let result = harness
        .service
        .register_project(
            "kanvibe",
            "/home/tester/Documents/kanvibe",
            Some("remote-host"),
        )
        .unwrap();

    assert!(result.is_success);
    assert_eq!(result.project.unwrap().name, "Documents/kanvibe");
}

#[test]
fn same_name_and_parent_on_the_same_pc_is_rejected() {
    let harness = Harness::new();
    harness.project("kanvibe", "/home/tester/a/work/kanvibe", None);
    harness.project("work/kanvibe", "/home/tester/b/work/kanvibe", None);

    let result = harness
        .service
        .register_project("kanvibe", "/home/tester/c/work/kanvibe", None)
        .unwrap();

    assert!(!result.is_success);
    assert_eq!(result.error.as_deref(), Some(DUPLICATE_PROJECT_NAME_ERROR));
    assert_eq!(result.project, None);
    assert_eq!(harness.projects().len(), 2);
}

#[test]
fn scanned_repo_with_colliding_name_and_parent_is_reported_as_an_error() {
    let harness = Harness::new();
    harness.project("kanvibe", "/home/tester/a/work/kanvibe", None);
    harness.project("work/kanvibe", "/home/tester/b/work/kanvibe", None);
    *harness.repositories.scanned_repos.lock().unwrap() =
        vec!["/home/tester/c/work/kanvibe".into()];

    let result = harness
        .service
        .scan_and_register_projects("/home/tester", None)
        .unwrap();

    assert!(result.registered.is_empty());
    assert_eq!(
        result.errors,
        [format!(
            "/home/tester/c/work/kanvibe: {DUPLICATE_PROJECT_NAME_ERROR}"
        )]
    );
    assert_eq!(harness.projects().len(), 2);
}

#[test]
fn same_host_and_path_is_not_registered_twice_under_another_name() {
    let harness = Harness::new();
    harness.project("kanvibe", "/workspace/kanvibe", Some("remote-host"));

    let result = harness
        .service
        .register_project("kanvibe-remote", "/workspace/kanvibe", Some("remote-host"))
        .unwrap();

    assert!(!result.is_success);
    assert_eq!(result.error.as_deref(), Some("이미 등록된 프로젝트입니다."));
}

#[test]
fn registration_saves_first_and_fills_the_icon_in_the_background() {
    let harness = Harness::new();
    let icon_gate = Arc::new(Gate::default());
    let gate = Arc::clone(&icon_gate);
    harness.icons.on_lookup(move |_| {
        gate.wait();
        Some("data:image/png;base64,icon".into())
    });

    let result = harness
        .service
        .register_project("api", "/workspace/api", None)
        .unwrap();

    assert!(result.is_success);
    let saved = result.project.unwrap();
    assert_eq!(saved.icon_data_url, None);
    wait_until(|| !harness.icons.lookups.lock().unwrap().is_empty());
    assert_eq!(harness.projects()[0].icon_data_url, None);

    icon_gate.open();
    harness.service.join_background_jobs();

    assert_eq!(harness.projects().len(), 1);
    assert_eq!(
        harness.projects()[0].icon_data_url.as_deref(),
        Some("data:image/png;base64,icon")
    );
}

#[test]
fn icon_is_not_saved_again_when_the_project_was_deleted_meanwhile() {
    let harness = Harness::new();
    let database = Arc::clone(&harness.db.database);
    harness.icons.on_lookup(move |repo_path| {
        let connection = database.connection();
        connection
            .execute("DELETE FROM projects WHERE repo_path = ?1", [repo_path])
            .unwrap();
        Some("data:image/png;base64,icon".into())
    });

    harness
        .service
        .register_project("api", "/workspace/api", None)
        .unwrap();
    harness.service.join_background_jobs();

    assert!(harness.projects().is_empty());
    // 등록 직후 1회만 갱신되고, 반영되지 못한 아이콘으로 보드를 다시 갱신하지 않는다
    assert_eq!(harness.events.board_update_count(), 1);
}
