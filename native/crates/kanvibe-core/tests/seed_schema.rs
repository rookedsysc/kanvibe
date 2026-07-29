use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use kanvibe_core::{
    CreateTaskInput, DONE_PAGE_SIZE, DoneCleanupOutcome, DoneCleanupResult, KanvibeDb,
    REQUIRED_TASK_STATUSES, TABLE_CONTRACTS, TaskPriority, TaskStatus, TaskUpdatePatch,
    seed_db_path_from_crate_manifest,
};

fn sqlite_scalar(db_path: &Path, sql: &str) -> String {
    let output = Command::new("sqlite3")
        .arg("-batch")
        .arg("-noheader")
        .arg(db_path)
        .arg(sql)
        .output()
        .expect("sqlite3 must be installed for seed schema compatibility tests");

    assert!(
        output.status.success(),
        "sqlite3 failed for SQL `{sql}`\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn writable_seed_copy(test_name: &str) -> PathBuf {
    let seed = seed_db_path_from_crate_manifest(env!("CARGO_MANIFEST_DIR"));
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "kanvibe-native-{test_name}-{}-{unique}.sqlite",
        std::process::id()
    ));

    fs::copy(seed, &path).expect("seed DB should copy to temp path");
    path
}

#[test]
fn electron_seed_database_matches_initial_rust_schema_contract() {
    let source = seed_db_path_from_crate_manifest(env!("CARGO_MANIFEST_DIR"));
    assert!(source.exists(), "missing seed DB at {}", source.display());
    let seed = writable_seed_copy("schema-contract");

    assert_eq!(sqlite_scalar(&seed, "PRAGMA integrity_check;"), "ok");
    assert_eq!(sqlite_scalar(&seed, "PRAGMA foreign_key_check;"), "");
    assert_eq!(
        sqlite_scalar(
            &seed,
            "SELECT group_concat(name, '|') FROM (SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name);"
        ),
        "app_settings|kanban_tasks|pane_layout_configs|projects"
    );

    for table in TABLE_CONTRACTS {
        let row_count = sqlite_scalar(&seed, &format!("SELECT COUNT(*) FROM {};", table.name))
            .parse::<u32>()
            .expect("row count must be an integer");
        assert!(
            row_count >= table.min_rows,
            "{} expected at least {} rows, got {}",
            table.name,
            table.min_rows,
            row_count
        );

        for column in table.columns {
            assert_eq!(
                sqlite_scalar(
                    &seed,
                    &format!(
                        "SELECT COUNT(*) FROM pragma_table_info('{}') WHERE name = '{}';",
                        table.name, column
                    )
                ),
                "1",
                "{}.{} is missing from seed schema",
                table.name,
                column
            );
        }
    }

    let expected_statuses = REQUIRED_TASK_STATUSES.join("|");
    assert_eq!(
        sqlite_scalar(
            &seed,
            "SELECT group_concat(status, '|') FROM (SELECT DISTINCT status FROM kanban_tasks ORDER BY CASE status WHEN 'todo' THEN 1 WHEN 'progress' THEN 2 WHEN 'pending' THEN 3 WHEN 'review' THEN 4 WHEN 'done' THEN 5 ELSE 99 END);"
        ),
        expected_statuses
    );
}

#[test]
fn rust_read_models_load_the_seed_board_snapshot() {
    let seed = writable_seed_copy("read-models");
    let database = KanvibeDb::open_read_only(&seed).expect("seed DB should open read-only");
    let snapshot = database
        .board_snapshot(DONE_PAGE_SIZE)
        .expect("seed DB should load as board snapshot");

    assert_eq!(snapshot.projects.len(), 3);
    assert_eq!(snapshot.done_total, 3);
    assert_eq!(snapshot.done_limit, DONE_PAGE_SIZE);
    assert_eq!(snapshot.task_count(TaskStatus::Todo), 3);
    assert_eq!(snapshot.task_count(TaskStatus::Progress), 3);
    assert_eq!(snapshot.task_count(TaskStatus::Pending), 3);
    assert_eq!(snapshot.task_count(TaskStatus::Review), 3);
    assert_eq!(snapshot.task_count(TaskStatus::Done), 3);

    let todo_column = snapshot
        .column(TaskStatus::Todo)
        .expect("todo column should exist");
    assert_eq!(todo_column.tasks[0].title, "Draft native board shell");
    assert_eq!(todo_column.tasks[0].priority, Some(TaskPriority::High));
    assert_eq!(
        todo_column.tasks[0].branch_name.as_deref(),
        Some("feat/native-board-shell")
    );

    let worktree_project = snapshot
        .projects
        .iter()
        .find(|project| project.id == "qa-project-docs-worktree")
        .expect("worktree project should be present");
    assert!(worktree_project.is_worktree);
    assert_eq!(worktree_project.color.as_deref(), Some("#8B5CF6"));
}

#[test]
fn board_write_models_cover_crud_status_move_reorder_and_done_paging() {
    let db_path = writable_seed_copy("board-write-models");
    let database = KanvibeDb::open_read_write(&db_path).expect("writable seed copy should open");

    let created = database
        .create_task(CreateTaskInput {
            id: Some("qa-created-task".to_owned()),
            title: Some("Native created task".to_owned()),
            description: Some("Created from Rust behavior test".to_owned()),
            project_id: Some("qa-project-kanvibe".to_owned()),
            priority: Some(TaskPriority::Medium),
            ..CreateTaskInput::default()
        })
        .expect("task should be created");
    assert_eq!(created.status, TaskStatus::Todo);
    assert_eq!(created.display_order, 3);

    let updated = database
        .update_task(
            &created.id,
            TaskUpdatePatch {
                title: Some("Native edited task".to_owned()),
                description: Some(None),
                priority: Some(Some(TaskPriority::High)),
            },
        )
        .expect("task update should succeed")
        .expect("created task should still exist");
    assert_eq!(updated.title, "Native edited task");
    assert_eq!(updated.description, None);
    assert_eq!(updated.priority, Some(TaskPriority::High));

    let moved = database
        .update_task_status(&created.id, TaskStatus::Progress)
        .expect("status update should succeed")
        .expect("created task should still exist");
    assert_eq!(moved.task.status, TaskStatus::Progress);
    assert!(moved.done_cleanup.is_none());

    let todo_order = vec![
        "qa-task-todo-remote".to_owned(),
        "qa-task-todo-local".to_owned(),
        "qa-task-todo-unassigned".to_owned(),
    ];
    database
        .reorder_tasks(&todo_order)
        .expect("todo reorder should succeed");
    assert_eq!(
        sqlite_scalar(
            &db_path,
            "SELECT group_concat(id, '|') FROM (SELECT id FROM kanban_tasks WHERE status = 'todo' ORDER BY display_order ASC);"
        ),
        "qa-task-todo-remote|qa-task-todo-local|qa-task-todo-unassigned"
    );

    let done_order = vec![
        "qa-created-task".to_owned(),
        "qa-task-done-migrated".to_owned(),
    ];
    database
        .move_task_to_column("qa-created-task", TaskStatus::Done, &done_order)
        .expect("move to done should succeed");
    let done_task = database
        .task_by_id("qa-created-task")
        .expect("task lookup should succeed")
        .expect("task should exist after move");
    assert_eq!(done_task.status, TaskStatus::Done);

    let (done_page, done_total) = database
        .more_done_tasks(0, 2)
        .expect("done pagination should load");
    assert_eq!(done_total, 4);
    assert_eq!(done_page.len(), 2);
    assert_eq!(done_page[0].id, "qa-created-task");

    assert!(
        database
            .delete_task("qa-created-task")
            .expect("delete should succeed")
    );
    assert!(
        database
            .task_by_id("qa-created-task")
            .expect("task lookup should succeed")
            .is_none()
    );
}

/// Done 전환은 세션/worktree 컬럼을 비우므로, 정리 대상을 스냅샷으로 넘겨받지 못하면
/// 디스크의 worktree와 세션이 영구 고아가 된다. Electron의 optimistic 전환 + 롤백 계약을 검증한다.
#[test]
fn done_transition_hands_back_cleanup_targets_and_rolls_back_on_failure() {
    let db_path = writable_seed_copy("done-cleanup-lifecycle");
    let database = KanvibeDb::open_read_write(&db_path).expect("writable seed copy should open");

    let before = database
        .task_by_id("qa-task-review-diff")
        .expect("task lookup should succeed")
        .expect("seed task should exist");
    assert_eq!(before.session_name.as_deref(), Some("kanvibe-native-diff"));

    let update = database
        .update_task_status("qa-task-review-diff", TaskStatus::Done)
        .expect("done transition should succeed")
        .expect("task should exist");
    let plan = update
        .done_cleanup
        .expect("done transition must return a cleanup plan");

    // DB에서는 비워지지만 정리에 필요한 값은 스냅샷에 남는다.
    assert_eq!(update.task.status, TaskStatus::Done);
    assert_eq!(update.task.session_name, None);
    assert_eq!(update.task.worktree_path, None);
    assert_eq!(update.task.session_type, None);
    assert_eq!(plan.cleanup_task.session_name, before.session_name);
    assert_eq!(plan.cleanup_task.worktree_path, before.worktree_path);
    assert_eq!(plan.cleanup_task.session_type, before.session_type);
    assert!(plan.has_resources_to_clean());

    // 정리 실패 -> 전환 직전 상태로 롤백된다.
    assert_eq!(
        database
            .finish_done_cleanup(&plan, DoneCleanupResult::Failed)
            .expect("rollback should succeed"),
        DoneCleanupOutcome::RolledBack
    );
    let restored = database
        .task_by_id("qa-task-review-diff")
        .expect("task lookup should succeed")
        .expect("task should exist after rollback");
    assert_eq!(restored.status, before.status);
    assert_eq!(restored.session_type, before.session_type);
    assert_eq!(restored.session_name, before.session_name);
    assert_eq!(restored.worktree_path, before.worktree_path);
    assert_eq!(restored.ssh_host, before.ssh_host);

    // 정리 성공 -> 비워진 Done 상태를 유지한다.
    let second = database
        .update_task_status("qa-task-review-diff", TaskStatus::Done)
        .expect("done transition should succeed")
        .expect("task should exist")
        .done_cleanup
        .expect("done transition must return a cleanup plan");
    assert_eq!(
        database
            .finish_done_cleanup(&second, DoneCleanupResult::Succeeded)
            .expect("cleanup completion should succeed"),
        DoneCleanupOutcome::Cleared
    );
    let cleared = database
        .task_by_id("qa-task-review-diff")
        .expect("task lookup should succeed")
        .expect("task should exist after cleanup");
    assert_eq!(cleared.status, TaskStatus::Done);
    assert_eq!(cleared.session_name, None);
    assert_eq!(cleared.worktree_path, None);
}

#[test]
fn hook_status_update_to_done_preserves_session_and_worktree_resources() {
    let db_path = writable_seed_copy("hook-status-preserves-resources");
    let database = KanvibeDb::open_read_write(&db_path).expect("writable seed copy should open");
    let before = database
        .task_by_id("qa-task-review-diff")
        .expect("task lookup should succeed")
        .expect("seed task should exist");

    let updated = database
        .set_task_status_preserving_resources("qa-task-review-diff", TaskStatus::Done)
        .expect("hook status update should succeed")
        .expect("task should exist");

    assert_eq!(updated.status, TaskStatus::Done);
    assert_eq!(updated.session_type, before.session_type);
    assert_eq!(updated.session_name, before.session_name);
    assert_eq!(updated.worktree_path, before.worktree_path);
    assert_eq!(updated.ssh_host, before.ssh_host);
}

#[test]
fn live_session_binding_only_fills_unassigned_task_metadata() {
    let db_path = writable_seed_copy("live-session-binding");
    let database = KanvibeDb::open_read_write(&db_path).expect("writable seed copy should open");
    let task = database
        .create_task(CreateTaskInput {
            id: Some("live-session-task".to_owned()),
            title: Some("Discover session".to_owned()),
            worktree_path: Some("/tmp/discovered-worktree".to_owned()),
            ..CreateTaskInput::default()
        })
        .expect("create unassigned task");

    let bound = database
        .bind_live_session_if_unassigned(
            &task.id,
            kanvibe_core::SessionType::Tmux,
            "discovered-session",
            "/tmp/discovered-worktree",
            None,
        )
        .expect("bind discovered session")
        .expect("task exists");
    assert_eq!(bound.session_type, Some(kanvibe_core::SessionType::Tmux));
    assert_eq!(bound.session_name.as_deref(), Some("discovered-session"));

    let preserved = database
        .bind_live_session_if_unassigned(
            &task.id,
            kanvibe_core::SessionType::Zellij,
            "must-not-replace",
            "/tmp/other",
            None,
        )
        .expect("repeat session binding")
        .expect("task exists");
    assert_eq!(
        preserved.session_type,
        Some(kanvibe_core::SessionType::Tmux)
    );
    assert_eq!(
        preserved.session_name.as_deref(),
        Some("discovered-session")
    );
    assert_eq!(
        preserved.worktree_path.as_deref(),
        Some("/tmp/discovered-worktree")
    );
}

/// 정리가 진행되는 동안 사용자가 상태를 다시 옮겼다면 롤백이 그 변경을 덮어써서는 안 된다.
#[test]
fn done_cleanup_failure_does_not_override_a_later_status_change() {
    let db_path = writable_seed_copy("done-cleanup-late-change");
    let database = KanvibeDb::open_read_write(&db_path).expect("writable seed copy should open");

    let plan = database
        .update_task_status("qa-task-progress-terminal", TaskStatus::Done)
        .expect("done transition should succeed")
        .expect("task should exist")
        .done_cleanup
        .expect("done transition must return a cleanup plan");

    database
        .update_task_status("qa-task-progress-terminal", TaskStatus::Todo)
        .expect("later status change should succeed")
        .expect("task should exist");

    assert_eq!(
        database
            .finish_done_cleanup(&plan, DoneCleanupResult::Failed)
            .expect("rollback decision should succeed"),
        DoneCleanupOutcome::SkippedRollback
    );
    assert_eq!(
        database
            .task_by_id("qa-task-progress-terminal")
            .expect("task lookup should succeed")
            .expect("task should exist")
            .status,
        TaskStatus::Todo
    );
}

/// Done 컬럼으로의 드래그 이동도 같은 정리 계약을 통과해야 한다.
#[test]
fn move_task_to_done_column_returns_cleanup_plan() {
    let db_path = writable_seed_copy("done-column-move-plan");
    let database = KanvibeDb::open_read_write(&db_path).expect("writable seed copy should open");

    let plan = database
        .move_task_to_column(
            "qa-task-pending-review",
            TaskStatus::Done,
            &[
                "qa-task-pending-review".to_owned(),
                "qa-task-done-migrated".to_owned(),
            ],
        )
        .expect("move to done should succeed")
        .expect("done column move must return a cleanup plan");

    assert!(plan.has_resources_to_clean());
    assert_eq!(
        plan.cleanup_task.session_name.as_deref(),
        Some("kanvibe-design-parity")
    );

    let non_done = database
        .move_task_to_column(
            "qa-task-todo-local",
            TaskStatus::Progress,
            &["qa-task-todo-local".to_owned()],
        )
        .expect("move to progress should succeed");
    assert!(non_done.is_none());
}

#[test]
fn project_color_update_uses_existing_repo_path_grouping() {
    let db_path = writable_seed_copy("project-color");
    let database = KanvibeDb::open_read_write(&db_path).expect("writable seed copy should open");

    database
        .update_project_color("qa-project-api", "#123456")
        .expect("project color update should succeed");

    assert_eq!(
        sqlite_scalar(
            &db_path,
            "SELECT color FROM projects WHERE id = 'qa-project-api';"
        ),
        "#123456"
    );
}

#[test]
fn app_settings_cover_sidebar_default_and_hint_dismissal() {
    let db_path = writable_seed_copy("app-settings");
    let database = KanvibeDb::open_read_write(&db_path).expect("writable seed copy should open");

    assert!(
        !database
            .sidebar_default_collapsed()
            .expect("sidebar default setting should load")
    );
    database
        .set_sidebar_default_collapsed(true)
        .expect("sidebar default setting should save");
    assert!(
        database
            .sidebar_default_collapsed()
            .expect("sidebar default setting should reload")
    );

    assert!(
        !database
            .sidebar_hint_dismissed()
            .expect("sidebar hint setting should load")
    );
    database
        .dismiss_sidebar_hint()
        .expect("sidebar hint dismissal should save");
    assert!(
        database
            .sidebar_hint_dismissed()
            .expect("sidebar hint setting should reload")
    );
}

#[test]
fn branch_from_task_persists_worktree_session_metadata() {
    let db_path = writable_seed_copy("branch-from-task");
    let database = KanvibeDb::open_read_write(&db_path).expect("writable seed copy should open");
    let branched = database
        .branch_from_task(
            "qa-task-todo-local",
            "qa-project-kanvibe",
            "main",
            "qa/branch-from-task",
            kanvibe_core::SessionType::Tmux,
            "kanvibe-qa-branch-from-task",
            "/tmp/kanvibe__worktrees/qa-branch-from-task",
        )
        .expect("branch metadata update should succeed")
        .expect("task should exist");

    assert_eq!(branched.status, TaskStatus::Progress);
    assert_eq!(branched.base_branch.as_deref(), Some("main"));
    assert_eq!(branched.branch_name.as_deref(), Some("qa/branch-from-task"));
    assert_eq!(branched.session_type, Some(kanvibe_core::SessionType::Tmux));
    assert_eq!(
        branched.worktree_path.as_deref(),
        Some("/tmp/kanvibe__worktrees/qa-branch-from-task")
    );
}
