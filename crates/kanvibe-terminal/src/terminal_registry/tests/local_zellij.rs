//! TS `terminal.test.ts` "attachLocalSession — zellij 세션 생성 및 레이아웃 적용"

use std::path::Path;

use kanvibe_domain::task::SessionType;

use super::fakes::*;

/// 레이아웃 파일 유무를 실제 파일로 정하는 worktree
fn worktree(harness: &Harness, has_layout_file: bool) -> String {
    let directory = harness.temp_directory.path().join("workspace");
    std::fs::create_dir_all(&directory).unwrap();
    if has_layout_file {
        std::fs::write(directory.join(".zellij-layout.kdl"), "layout {}\n").unwrap();
    }
    directory.to_string_lossy().into_owned()
}

fn harness_with_zellij_sessions(list_sessions_output: &str) -> Harness {
    let harness = Harness::new();
    harness
        .runner
        .respond_when_contains("list-sessions", list_sessions_output);
    harness
}

fn attach_zellij(harness: &Harness, task_id: &str, working_directory: Option<&str>) {
    let request = local_request(
        task_id,
        None,
        SessionType::Zellij,
        "feat-login",
        working_directory,
    );
    harness.attach_local(request, &FakeClient::new()).unwrap();
}

#[test]
fn missing_local_zellij_session_is_created_with_the_session_flag() {
    // Given
    let harness = harness_with_zellij_sessions("other-session [Created 1h ago]\n");
    let workspace = worktree(&harness, false);

    // When
    attach_zellij(&harness, "task-z1", Some(&workspace));

    // Then
    let request = harness.ptys.request(0);
    assert_eq!(request.program, "zellij");
    assert_eq!(request.args, vec!["--session", "feat-login"]);
    assert_eq!(request.working_directory, workspace);
}

#[test]
fn new_local_zellij_session_applies_the_kdl_layout_file() {
    // Given
    let harness = harness_with_zellij_sessions("");
    let workspace = worktree(&harness, true);
    let layout_file = Path::new(&workspace)
        .join(".zellij-layout.kdl")
        .to_string_lossy()
        .into_owned();

    // When
    attach_zellij(&harness, "task-z2", Some(&workspace));

    // Then
    let request = harness.ptys.request(0);
    assert_eq!(
        request.args,
        vec![
            "--session",
            "feat-login",
            "--new-session-with-layout",
            &layout_file
        ]
    );
    assert_eq!(request.working_directory, workspace);
}

#[test]
fn alive_local_zellij_session_is_attached_without_creation() {
    // Given
    let harness = harness_with_zellij_sessions("feat-login [Created 1h ago]\n");
    let workspace = worktree(&harness, true);

    // When
    attach_zellij(&harness, "task-z3", Some(&workspace));

    // Then
    let request = harness.ptys.request(0);
    assert_eq!(request.args, vec!["attach", "feat-login"]);
    assert_eq!(request.working_directory, "/home/local-user");
}

#[test]
fn missing_layout_file_leaves_out_the_layout_flag() {
    // Given
    let harness = harness_with_zellij_sessions("");
    let workspace = worktree(&harness, false);

    // When
    attach_zellij(&harness, "task-z4", Some(&workspace));

    // Then
    let request = harness.ptys.request(0);
    assert_eq!(request.args, vec!["--session", "feat-login"]);
    assert_eq!(request.working_directory, workspace);
}

#[test]
fn zellij_without_working_directory_skips_the_layout_check_and_starts_at_home() {
    // Given
    let harness = harness_with_zellij_sessions("");

    // When
    attach_zellij(&harness, "task-z5", None);

    // Then
    let request = harness.ptys.request(0);
    assert_eq!(request.args, vec!["--session", "feat-login"]);
    assert_eq!(request.working_directory, "/home/local-user");
}

#[test]
fn exited_local_zellij_session_counts_as_missing() {
    // Given
    let harness = harness_with_zellij_sessions("EXITED: feat-login\n");
    let workspace = worktree(&harness, false);

    // When
    attach_zellij(&harness, "task-z6", Some(&workspace));

    // Then
    let request = harness.ptys.request(0);
    assert_eq!(request.args, vec!["--session", "feat-login"]);
    assert_eq!(request.working_directory, workspace);
}
