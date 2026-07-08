use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use kanvibe_core::{
    CreateTaskInput, DONE_PAGE_SIZE, KanvibeDb, REQUIRED_TASK_STATUSES, TABLE_CONTRACTS,
    TaskPriority, TaskStatus, TaskUpdatePatch, seed_db_path_from_crate_manifest,
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
    let seed = seed_db_path_from_crate_manifest(env!("CARGO_MANIFEST_DIR"));
    assert!(seed.exists(), "missing seed DB at {}", seed.display());

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
    let seed = seed_db_path_from_crate_manifest(env!("CARGO_MANIFEST_DIR"));
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
    assert_eq!(moved.status, TaskStatus::Progress);

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
