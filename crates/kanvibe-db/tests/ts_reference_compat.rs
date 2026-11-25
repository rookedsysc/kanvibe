//! TS 앱이 만든 DB를 Rust가 그대로 여는지 확인한다.
//!
//! `fixtures/ts-reference.db`는 기준 커밋 ba3c5093의 TS 코드를 Electron(ELECTRON_RUN_AS_NODE)으로 실행해 만들었다.
//! 패키지 앱과 같은 길(시드 DB 복사 → 스키마 복구 → baseline → 마이그레이션)로 DB를 연 뒤, TypeORM 저장소와
//! `appSettingsService`·`notificationStore`로 다섯 테이블에 행을 넣었다.
//! `fixtures/ts-migrated-empty.db`는 시드 없이 열어 TypeORM 마이그레이션 17개가 처음부터 돈 DB다.

use std::fs;
use std::path::{Path, PathBuf};

use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_db::repositories::{pane_layouts, projects, task_diff_stats, tasks};
use kanvibe_db::schema::build_seed_database;
use kanvibe_db::{Database, DatabaseLocation};
use kanvibe_domain::board_sort::serialize_board_sort_preference;
use kanvibe_domain::notifications::AppNotification;
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::shortcuts::bindings::ShortcutCommandId;
use kanvibe_domain::task::{SessionType, TaskStatus};
use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};

const SNAPSHOT_TABLES: [&str; 6] = [
    "migrations",
    "projects",
    "kanban_tasks",
    "pane_layout_configs",
    "app_settings",
    "task_diff_stats",
];

fn fixture_path(file_name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(file_name)
}

/// fixture는 WAL 모드라 그냥 읽기 전용으로 열어도 `-shm` 파일이 생긴다. 변하지 않는 파일로 열어 원본 디렉터리를 건드리지 않는다
fn open_read_only(database_path: &Path) -> Connection {
    let immutable_uri = format!("file:{}?immutable=1", database_path.display());
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    Connection::open_with_flags(immutable_uri, flags).unwrap()
}

/// sqlite_master의 모든 객체. 내부 autoindex 이름과 SQL 원문 공백까지 비교한다
fn schema_objects(connection: &Connection) -> Vec<(String, String, String, Option<String>)> {
    let mut statement = connection
        .prepare("SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name")
        .unwrap();
    statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// 공백 차이만 접은 스키마. 원문이 다를 때 무엇이 어긋났는지 읽기 쉽게 보여 준다
fn normalized_schema(connection: &Connection) -> Vec<String> {
    schema_objects(connection)
        .into_iter()
        .map(|(object_type, name, table_name, sql)| {
            let sql_words = sql
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            format!("{object_type} {name} {table_name} {sql_words}")
        })
        .collect()
}

fn table_rows(connection: &Connection, table_name: &str) -> Vec<Vec<Value>> {
    let mut statement = connection
        .prepare(&format!("SELECT * FROM {table_name} ORDER BY rowid"))
        .unwrap();
    let column_count = statement.column_count();
    statement
        .query_map([], |row| {
            (0..column_count)
                .map(|index| row.get::<_, Value>(index))
                .collect()
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn all_rows(connection: &Connection) -> Vec<Vec<Vec<Value>>> {
    SNAPSHOT_TABLES
        .iter()
        .map(|table_name| table_rows(connection, table_name))
        .collect()
}

struct CopiedFixture {
    _directory: tempfile::TempDir,
    location: DatabaseLocation,
}

/// 여는 과정이 파일을 바꿀 수 있으므로 원본 fixture 대신 복사본을 연다
fn copy_fixture(file_name: &str) -> CopiedFixture {
    let directory = tempfile::tempdir().unwrap();
    let database_path = directory.path().join("kanvibe.db");
    fs::copy(fixture_path(file_name), &database_path).unwrap();
    CopiedFixture {
        location: DatabaseLocation {
            database_path,
            seed_database_path: None,
        },
        _directory: directory,
    }
}

fn stored_setting(service: &AppSettingsService<'_>, key: &str) -> String {
    service
        .get_app_setting(key)
        .unwrap()
        .unwrap_or_else(|| panic!("{key} 설정이 fixture에 있어야 한다"))
}

#[test]
fn rust_opens_the_ts_database_without_running_migrations_or_changing_rows() {
    let original = open_read_only(&fixture_path("ts-reference.db"));
    let fixture = copy_fixture("ts-reference.db");

    let database = Database::open(&fixture.location).unwrap();
    let connection = database.connection();

    assert_eq!(table_rows(&connection, "migrations").len(), 17);
    assert_eq!(all_rows(&connection), all_rows(&original));
    assert_eq!(schema_objects(&connection), schema_objects(&original));
}

#[test]
fn rust_reads_every_ts_row_through_the_repositories() {
    let fixture = copy_fixture("ts-reference.db");
    let database = Database::open(&fixture.location).unwrap();
    let connection = database.connection();

    let stored_projects = projects::list_projects(&connection).unwrap();
    assert_eq!(stored_projects.len(), 2);
    assert!(
        stored_projects
            .iter()
            .all(|project| project.name == "kanvibe")
    );
    let stored_tasks = tasks::list_tasks(&connection).unwrap();
    assert_eq!(stored_tasks.len(), 3);
    let updated_task = stored_tasks
        .iter()
        .find(|task| task.status == TaskStatus::Pending)
        .unwrap();
    assert_eq!(updated_task.session_type, Some(SessionType::Tmux));
    assert!(
        task_diff_stats::find_task_diff_stats(&connection, &updated_task.id)
            .unwrap()
            .is_some()
    );

    let project_layout =
        pane_layouts::find_project_pane_layout(&connection, &stored_projects[0].id)
            .unwrap()
            .unwrap();
    let stored_panes: String = connection
        .query_row(
            "SELECT panes FROM pane_layout_configs WHERE id = ?1",
            [&project_layout.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::to_string(&project_layout.panes).unwrap(),
        stored_panes
    );
    assert!(
        pane_layouts::find_global_pane_layout(&connection)
            .unwrap()
            .is_some()
    );
}

#[test]
fn app_settings_json_written_by_ts_round_trips_byte_for_byte() {
    let fixture = copy_fixture("ts-reference.db");
    let database = Database::open(&fixture.location).unwrap();
    let service = AppSettingsService::new(&database, ShortcutPlatform::Linux);

    let stored_notifications = stored_setting(&service, "app_notifications");
    let notifications: Vec<AppNotification> = serde_json::from_str(&stored_notifications).unwrap();
    assert_eq!(
        serde_json::to_string(&notifications).unwrap(),
        stored_notifications
    );

    let stored_sort = stored_setting(&service, "board_sort_preference");
    assert_eq!(
        serialize_board_sort_preference(&service.get_board_sort_preference().unwrap()),
        stored_sort
    );

    let bindings = service.get_shortcut_bindings().unwrap();
    assert_eq!(
        bindings.shortcut(ShortcutCommandId::CreateTask),
        "Mod+Alt+N"
    );
    let stored_overrides = stored_setting(&service, "shortcut_bindings");
    let rewritten_overrides =
        serde_json::to_string(&bindings.collect_overrides(ShortcutPlatform::Linux)).unwrap();
    assert_eq!(rewritten_overrides, stored_overrides);
}

#[test]
fn scalar_and_list_settings_written_by_ts_read_the_same() {
    let fixture = copy_fixture("ts-reference.db");
    let database = Database::open(&fixture.location).unwrap();
    let service = AppSettingsService::new(&database, ShortcutPlatform::Linux);

    let dismissed_versions = service.get_release_update_dismissed_versions().unwrap();
    assert_eq!(
        serde_json::to_string(&dismissed_versions).unwrap(),
        stored_setting(&service, "release_update_dismissed_versions")
    );
    let notification_settings = service.get_notification_settings().unwrap();
    assert!(!notification_settings.is_enabled);
    assert_eq!(
        serde_json::to_string(&notification_settings.enabled_statuses).unwrap(),
        stored_setting(&service, "notification_statuses")
    );

    assert!(service.get_sidebar_hint_dismissed().unwrap());
    assert_eq!(
        service.get_default_session_type().unwrap(),
        SessionType::Zellij
    );
    assert_eq!(service.get_background_sync_interval_ms().unwrap(), 300_000);
    assert!(!service.get_vim_mode_enabled().unwrap());
    assert_eq!(
        service.get_or_create_hook_client_id().unwrap(),
        stored_setting(&service, "hook_client_id")
    );
}

#[test]
fn fresh_rust_database_from_the_seed_matches_the_ts_schema() {
    let directory = tempfile::tempdir().unwrap();
    let seed_path = directory.path().join("resources/database/app.seed.db");
    build_seed_database(&seed_path).unwrap();
    let location = DatabaseLocation::in_app_data_directory(
        &directory.path().join("app-data"),
        Some(seed_path),
    );

    let database = Database::open(&location).unwrap();
    let ts_database = open_read_only(&fixture_path("ts-reference.db"));

    assert_eq!(
        normalized_schema(&database.connection()),
        normalized_schema(&ts_database)
    );
    assert_eq!(
        schema_objects(&database.connection()),
        schema_objects(&ts_database)
    );
}

#[test]
fn fresh_rust_database_without_a_seed_matches_the_ts_migration_schema() {
    let directory = tempfile::tempdir().unwrap();
    let location = DatabaseLocation::in_app_data_directory(directory.path(), None);

    let database = Database::open(&location).unwrap();
    let ts_database = open_read_only(&fixture_path("ts-migrated-empty.db"));
    let connection = database.connection();

    assert_eq!(
        normalized_schema(&connection),
        normalized_schema(&ts_database)
    );
    assert_eq!(schema_objects(&connection), schema_objects(&ts_database));
    assert_eq!(
        table_rows(&connection, "migrations"),
        table_rows(&ts_database, "migrations")
    );
}

#[test]
fn rust_opens_the_ts_migrated_database_without_changes() {
    let original = open_read_only(&fixture_path("ts-migrated-empty.db"));
    let fixture = copy_fixture("ts-migrated-empty.db");

    let database = Database::open(&fixture.location).unwrap();
    let connection = database.connection();

    assert_eq!(all_rows(&connection), all_rows(&original));
    assert_eq!(schema_objects(&connection), schema_objects(&original));
}
