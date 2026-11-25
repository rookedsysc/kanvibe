use std::path::{Path, PathBuf};

use kanvibe_domain::task_diff_stats::TaskDiffStats;
use rusqlite::Connection;

use super::steps;
use crate::database::Database;
use crate::location::DatabaseLocation;
use crate::repositories::task_diff_stats::{find_task_diff_stats, save_task_diff_stats};
use crate::schema::ensure_sqlite_database_ready;
use crate::table_introspection::{column_names, index_columns};

struct DatabaseFixture {
    directory: tempfile::TempDir,
}

impl DatabaseFixture {
    fn new() -> Self {
        Self {
            directory: tempfile::tempdir().unwrap(),
        }
    }

    fn database_path(&self) -> PathBuf {
        self.directory.path().join("kanvibe.db")
    }

    fn location(&self) -> DatabaseLocation {
        DatabaseLocation {
            database_path: self.database_path(),
            seed_database_path: None,
        }
    }

    fn open_app_database(&self) -> Database {
        Database::open(&self.location()).unwrap()
    }

    fn raw_connection(&self) -> Connection {
        Connection::open(self.database_path()).unwrap()
    }
}

fn query_strings(connection: &Connection, sql: &str) -> Vec<String> {
    let mut statement = connection.prepare(sql).unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn query_pairs(connection: &Connection, sql: &str) -> Vec<(String, Option<String>)> {
    let mut statement = connection.prepare(sql).unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn migration_names(connection: &Connection) -> Vec<String> {
    query_strings(connection, "SELECT name FROM migrations ORDER BY timestamp")
}

fn insert_legacy_bootstrap_data(database_path: &Path) {
    ensure_sqlite_database_ready(database_path).unwrap();
    Connection::open(database_path)
        .unwrap()
        .execute_batch(
            "INSERT INTO projects (id, name, repo_path, default_branch) VALUES
               ('project-1', 'Project 1', '/workspace/project-1', 'main'),
               ('project-2', 'Project 2', '/workspace/project-2', 'main');
             INSERT INTO kanban_tasks (id, title, status, branch_name, project_id, created_at, updated_at) VALUES
               ('task-1', 'Top card', 'todo', 'main', 'project-1', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
               ('task-2', 'Older card', 'todo', 'main', 'project-2', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z');",
        )
        .unwrap();
}

const LEGACY_UNIQUE_PROJECTS_TABLE: &str = "CREATE TABLE projects (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    repo_path TEXT NOT NULL,
    default_branch TEXT NOT NULL DEFAULT 'main',
    ssh_host TEXT,
    is_worktree INTEGER NOT NULL DEFAULT 0,
    color TEXT DEFAULT NULL,
    icon_data_url TEXT DEFAULT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
  )";

/// TypeORM이 마이그레이션 앞뒤로 하는 것처럼 foreign_keys를 끄고 단계를 실행한다
fn run_step_with_foreign_keys_off(
    connection: &Connection,
    step: fn(&Connection) -> crate::DatabaseResult<()>,
) {
    connection
        .pragma_update(None, "foreign_keys", "OFF")
        .unwrap();
    step(connection).unwrap();
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .unwrap();
}

// parity: TS-lib-databaseMigrations-814450
#[test]
fn baselines_databases_created_by_the_legacy_bootstrap() {
    let fixture = DatabaseFixture::new();
    insert_legacy_bootstrap_data(&fixture.database_path());

    let database = fixture.open_app_database();
    let connection = database.connection();

    assert_eq!(
        query_pairs(
            &connection,
            "SELECT id, branch_name FROM kanban_tasks ORDER BY id"
        ),
        [
            ("task-1".to_string(), Some("main".to_string())),
            ("task-2".to_string(), Some("main".to_string()))
        ]
    );
    let names = migration_names(&connection);
    assert_eq!(names.len(), 17);
    assert_eq!(names[0], "InitialSchema1770854400000");
}

// parity: TS-lib-databaseMigrations-370c89
#[test]
fn repairs_missing_diff_stats_for_a_legacy_database() {
    for is_already_baselined in [false, true] {
        let fixture = DatabaseFixture::new();
        insert_legacy_bootstrap_data(&fixture.database_path());
        if is_already_baselined {
            drop(fixture.open_app_database());
        }
        fixture
            .raw_connection()
            .execute_batch("DROP TABLE IF EXISTS task_diff_stats")
            .unwrap();

        let database = fixture.open_app_database();
        let connection = database.connection();
        let stats = TaskDiffStats {
            file_count: 3,
            additions: 7,
            deletions: 2,
        };
        save_task_diff_stats(&connection, "task-1", stats).unwrap();
        assert_eq!(
            find_task_diff_stats(&connection, "task-1")
                .unwrap()
                .unwrap()
                .stats,
            stats
        );

        connection
            .execute("DELETE FROM kanban_tasks WHERE id = ?1", ["task-1"])
            .unwrap();
        assert_eq!(find_task_diff_stats(&connection, "task-1").unwrap(), None);
    }
}

// parity: TS-lib-databaseMigrations-577bf7
#[test]
fn backfills_older_legacy_columns_before_baselining() {
    let fixture = DatabaseFixture::new();
    fixture
        .raw_connection()
        .execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL UNIQUE, repo_path TEXT NOT NULL,
               default_branch TEXT NOT NULL DEFAULT 'main', ssh_host TEXT,
               created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP);
             CREATE TABLE kanban_tasks (id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL,
               status TEXT NOT NULL DEFAULT 'todo', branch_name TEXT,
               created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
               updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP);
             CREATE TABLE pane_layout_configs (id TEXT PRIMARY KEY NOT NULL, layout_type TEXT NOT NULL,
               project_id TEXT UNIQUE, is_global INTEGER NOT NULL DEFAULT 0,
               created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
               updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP);
             INSERT INTO projects (id, name, repo_path, default_branch)
               VALUES ('project-1', 'Project 1', '/workspace/project-1', 'main');
             INSERT INTO kanban_tasks (id, title, status, branch_name) VALUES ('task-1', 'Task 1', 'todo', 'main');",
        )
        .unwrap();

    let database = fixture.open_app_database();
    let connection = database.connection();
    let task_columns = column_names(&connection, "kanban_tasks").unwrap();

    for expected_column in ["project_id", "base_branch", "pr_url", "priority"] {
        assert!(
            task_columns.iter().any(|name| name == expected_column),
            "{expected_column}"
        );
    }
    assert!(!task_columns.iter().any(|name| name == "display_order"));
    assert!(
        column_names(&connection, "pane_layout_configs")
            .unwrap()
            .iter()
            .any(|name| name == "panes")
    );
    let empty_columns: Vec<Option<String>> = connection
        .query_row(
            "SELECT project_id, base_branch, pr_url, priority FROM kanban_tasks",
            [],
            |row| (0..4).map(|index| row.get(index)).collect(),
        )
        .unwrap();
    assert_eq!(empty_columns, [None, None, None, None]);
    assert_eq!(migration_names(&connection).len(), 17);
}

// parity: TS-lib-databaseMigrations-4e4994
#[test]
fn runs_all_migrations_for_a_fresh_database_file() {
    let fixture = DatabaseFixture::new();

    let database = fixture.open_app_database();
    let connection = database.connection();

    let tables = query_strings(
        &connection,
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    );
    for expected_table in [
        "app_settings",
        "kanban_tasks",
        "migrations",
        "pane_layout_configs",
        "projects",
        "task_diff_stats",
    ] {
        assert!(
            tables.iter().any(|name| name == expected_table),
            "{expected_table}"
        );
    }
    let task_indexes = query_strings(
        &connection,
        r#"SELECT name FROM pragma_index_list("kanban_tasks")"#,
    );
    assert!(
        !task_indexes
            .iter()
            .any(|name| name == "UQ_kanban_tasks_branch_name")
    );
    assert_eq!(migration_names(&connection).len(), 17);

    connection
        .execute_batch(
            "INSERT INTO projects (id, name, repo_path, ssh_host) VALUES
               ('project-local', 'kanvibe', '/workspace/kanvibe', NULL),
               ('project-remote', 'kanvibe', '/workspace/kanvibe', 'remote-host')",
        )
        .unwrap();
    assert_eq!(
        query_strings(&connection, "SELECT name FROM projects ORDER BY id"),
        ["kanvibe", "kanvibe"]
    );
}

// parity: TS-lib-databaseMigrations-91be15
#[test]
fn rescope_drops_the_global_unique_name_without_unlinking_tasks() {
    let fixture = DatabaseFixture::new();
    let connection = fixture.raw_connection();
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .unwrap();
    connection
        .execute_batch(LEGACY_UNIQUE_PROJECTS_TABLE)
        .unwrap();
    connection
        .execute_batch(
            "CREATE TABLE kanban_tasks (id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL,
               status TEXT NOT NULL DEFAULT 'todo', project_id TEXT,
               created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
               updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
               FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL);
             INSERT INTO projects (id, name, repo_path, ssh_host) VALUES ('project-local', 'kanvibe', '/workspace/kanvibe', NULL);
             INSERT INTO kanban_tasks (id, title, project_id) VALUES ('task-1', 'main', 'project-local');",
        )
        .unwrap();

    run_step_with_foreign_keys_off(&connection, steps::rescope_project_names_per_host);
    connection
        .execute(
            "INSERT INTO projects (id, name, repo_path, ssh_host) VALUES ('project-remote', 'kanvibe', '/workspace/kanvibe', 'remote-host')",
            [],
        )
        .unwrap();

    assert_eq!(
        query_pairs(&connection, "SELECT id, ssh_host FROM projects ORDER BY id"),
        [
            ("project-local".to_string(), None),
            (
                "project-remote".to_string(),
                Some("remote-host".to_string())
            )
        ]
    );
    assert_eq!(
        query_strings(&connection, "SELECT name FROM projects ORDER BY id"),
        ["kanvibe", "kanvibe"]
    );
    assert_eq!(
        query_pairs(&connection, "SELECT id, project_id FROM kanban_tasks"),
        [("task-1".to_string(), Some("project-local".to_string()))]
    );
}

// parity: TS-lib-databaseMigrations-194ba7
#[test]
fn rescope_removes_parent_folders_and_numbers_added_by_the_global_rule() {
    let fixture = DatabaseFixture::new();
    let connection = fixture.raw_connection();
    connection
        .execute_batch(LEGACY_UNIQUE_PROJECTS_TABLE)
        .unwrap();
    connection
        .execute_batch(
            "INSERT INTO projects (id, name, repo_path, ssh_host, created_at) VALUES
               ('project-local', 'kanvibe', '/home/tester/Documents/kanvibe', NULL, '2026-01-01T00:00:00.000Z'),
               ('project-remote', 'Documents/kanvibe', '/home/tester/Documents/kanvibe', 'remote-host', '2026-01-02T00:00:00.000Z'),
               ('project-other-remote', 'kanvibe-2', '/home/tester/Documents/kanvibe', 'other-host', '2026-01-03T00:00:00.000Z'),
               ('project-local-second', 'work/kanvibe', '/home/tester/work/kanvibe', NULL, '2026-01-04T00:00:00.000Z')",
        )
        .unwrap();

    run_step_with_foreign_keys_off(&connection, steps::rescope_project_names_per_host);

    assert_eq!(
        query_pairs(
            &connection,
            "SELECT id, name FROM projects ORDER BY created_at"
        ),
        [
            ("project-local".to_string(), Some("kanvibe".to_string())),
            ("project-remote".to_string(), Some("kanvibe".to_string())),
            (
                "project-other-remote".to_string(),
                Some("kanvibe".to_string())
            ),
            (
                "project-local-second".to_string(),
                Some("work/kanvibe".to_string())
            ),
        ]
    );
}

// parity: TS-lib-databaseMigrations-899033
#[test]
fn baselined_database_also_loses_the_global_unique_name() {
    let fixture = DatabaseFixture::new();
    fixture
        .raw_connection()
        .execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL UNIQUE, repo_path TEXT NOT NULL,
               default_branch TEXT NOT NULL DEFAULT 'main', ssh_host TEXT,
               created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP);
             CREATE TABLE kanban_tasks (id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL,
               status TEXT NOT NULL DEFAULT 'todo', branch_name TEXT, project_id TEXT,
               created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
               updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
               FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL);
             INSERT INTO projects (id, name, repo_path, default_branch) VALUES ('project-local', 'kanvibe', '/workspace/kanvibe', 'main');
             INSERT INTO kanban_tasks (id, title, status, branch_name, project_id) VALUES ('task-1', 'main', 'todo', 'main', 'project-local');",
        )
        .unwrap();

    let database = fixture.open_app_database();
    let connection = database.connection();
    connection
        .execute(
            "INSERT INTO projects (id, name, repo_path, ssh_host) VALUES ('project-remote', 'kanvibe', '/workspace/kanvibe', 'remote-host')",
            [],
        )
        .unwrap();

    assert_eq!(
        query_strings(&connection, "SELECT name FROM projects ORDER BY id"),
        ["kanvibe", "kanvibe"]
    );
    assert_eq!(
        query_pairs(&connection, "SELECT id, project_id FROM kanban_tasks"),
        [("task-1".to_string(), Some("project-local".to_string()))]
    );
}

// parity: TS-lib-databaseMigrations-6a1310
#[test]
fn rescope_fails_instead_of_dropping_an_unknown_projects_column() {
    let fixture = DatabaseFixture::new();
    let connection = fixture.raw_connection();
    let projects_with_extra_column = LEGACY_UNIQUE_PROJECTS_TABLE.replace(
        "created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP",
        "created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, pinned_at DATETIME DEFAULT NULL",
    );
    connection
        .execute_batch(&projects_with_extra_column)
        .unwrap();
    connection
        .execute(
            "INSERT INTO projects (id, name, repo_path, pinned_at) VALUES ('project-local', 'kanvibe', '/workspace/kanvibe', '2026-01-01T00:00:00.000Z')",
            [],
        )
        .unwrap();

    let error = steps::rescope_project_names_per_host(&connection).unwrap_err();

    assert!(error.to_string().contains("pinned_at"), "{error}");
    assert_eq!(
        query_pairs(&connection, "SELECT id, pinned_at FROM projects"),
        [(
            "project-local".to_string(),
            Some("2026-01-01T00:00:00.000Z".to_string())
        )]
    );
}

// parity: TS-lib-databaseMigrations-4f234f
#[test]
fn tied_timestamps_still_get_distinct_display_orders() {
    let fixture = DatabaseFixture::new();
    let connection = fixture.raw_connection();
    connection
        .execute_batch(
            "CREATE TABLE kanban_tasks (id TEXT PRIMARY KEY NOT NULL, status TEXT NOT NULL,
               display_order INTEGER NOT NULL DEFAULT 0, created_at DATETIME NOT NULL);
             INSERT INTO kanban_tasks (id, status, display_order, created_at) VALUES
               ('task-a', 'todo', 0, '2026-01-01T00:00:00.000Z'),
               ('task-b', 'todo', 0, '2026-01-01T00:00:00.000Z'),
               ('task-c', 'todo', 0, '2026-01-01T00:00:00.000Z');",
        )
        .unwrap();

    steps::assign_display_order(&connection).unwrap();

    assert_eq!(
        query_strings(
            &connection,
            "SELECT id || ':' || display_order FROM kanban_tasks ORDER BY id"
        ),
        ["task-a:0", "task-b:1", "task-c:2"]
    );
}

// parity: TS-lib-databaseMigrations-758366
#[test]
fn dropping_display_order_rebuilds_the_index_on_the_new_default_order() {
    let fixture = DatabaseFixture::new();
    let connection = fixture.raw_connection();
    connection
        .execute_batch(
            r#"CREATE TABLE kanban_tasks (id TEXT PRIMARY KEY NOT NULL, status TEXT NOT NULL,
                 display_order INTEGER NOT NULL DEFAULT 0, created_at DATETIME NOT NULL, updated_at DATETIME NOT NULL);
               CREATE INDEX "idx_kanban_tasks_status_order" ON kanban_tasks(status, display_order, created_at);"#,
        )
        .unwrap();

    steps::drop_display_order_from_kanban_tasks(&connection).unwrap();

    assert!(
        !column_names(&connection, "kanban_tasks")
            .unwrap()
            .iter()
            .any(|name| name == "display_order")
    );
    assert_eq!(
        index_columns(&connection, "idx_kanban_tasks_status_order").unwrap(),
        [Some("status".to_string()), Some("updated_at".to_string())]
    );
}

// parity: TS-lib-databaseMigrations-1583f7
#[test]
fn schema_repair_rebuilds_a_baselined_index_left_on_old_columns() {
    let fixture = DatabaseFixture::new();
    fixture
        .raw_connection()
        .execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL, repo_path TEXT NOT NULL,
               default_branch TEXT NOT NULL DEFAULT 'main', ssh_host TEXT,
               created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP);
             CREATE TABLE kanban_tasks (id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL,
               status TEXT NOT NULL DEFAULT 'todo', branch_name TEXT, display_order INTEGER NOT NULL DEFAULT 0,
               created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
               updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP);
             INSERT INTO kanban_tasks (id, title, status, display_order, created_at, updated_at) VALUES
               ('task-last', 'Last', 'todo', 2, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'),
               ('task-first', 'First', 'todo', 0, '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
             CREATE INDEX idx_kanban_tasks_status_order ON kanban_tasks(status, display_order, created_at);",
        )
        .unwrap();

    ensure_sqlite_database_ready(&fixture.database_path()).unwrap();

    assert_eq!(
        index_columns(&fixture.raw_connection(), "idx_kanban_tasks_status_order").unwrap(),
        [Some("status".to_string()), Some("updated_at".to_string())]
    );
}

#[test]
fn reopening_a_migrated_database_runs_nothing_again() {
    let fixture = DatabaseFixture::new();
    drop(fixture.open_app_database());

    let connection = fixture.raw_connection();

    assert_eq!(
        super::run_pending_migrations(&connection).unwrap(),
        Vec::<&str>::new()
    );
    assert_eq!(migration_names(&connection).len(), 17);
}
