use std::path::{Path, PathBuf};

use kanvibe_db::{Database, DatabaseLocation};
use rusqlite::Connection;

use super::*;

/// TS 시험과 같은 배치: `<appData>/KanVibe`(설치 앱)와 `<appData>/kanvibe-dev`(개발 실행)
struct Directories {
    _root: tempfile::TempDir,
    app_data: PathBuf,
    source: PathBuf,
    target: PathBuf,
}

impl Directories {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let app_data = root.path().to_path_buf();
        let source = app_data.join("KanVibe");
        let target = app_data.join("kanvibe-dev");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        Self {
            _root: root,
            app_data,
            source,
            target,
        }
    }

    fn source_path(&self) -> PathBuf {
        self.source.join("kanvibe.db")
    }

    fn target_path(&self) -> PathBuf {
        self.target.join("kanvibe.db")
    }

    fn request(&self, data_source: Option<&str>) -> DevelopmentDatabaseRequest {
        DevelopmentDatabaseRequest {
            is_development: true,
            is_packaged: false,
            is_qa: false,
            data_source: data_source.map(str::to_string),
            application_data_directory: None,
            platform_app_data: self.app_data.clone(),
            target_directory: self.target.clone(),
        }
    }

    /// 마이그레이션까지 끝낸 설치 앱 DB. WAL을 체크포인트하지 않아 커밋된 내용이 WAL에만 있다
    fn source_database(&self) -> Connection {
        drop(Database::open(&DatabaseLocation::in_app_data_directory(&self.source, None)).unwrap());
        let database = Connection::open(self.source_path()).unwrap();
        database.pragma_update(None, "journal_mode", "WAL").unwrap();
        database
            .pragma_update(None, "wal_autocheckpoint", 0)
            .unwrap();
        database
            .execute_batch(
                "INSERT INTO projects (id, name, repo_path) VALUES ('project', 'Application project', '/repo');
                 INSERT INTO kanban_tasks (id, title, project_id) VALUES ('task', 'Application task', 'project');
                 INSERT INTO app_settings (id, key, value) VALUES ('setting', 'theme', 'dark');
                 CREATE TABLE additional_data (value TEXT);
                 INSERT INTO additional_data VALUES ('included without a table allowlist');",
            )
            .unwrap();
        database
    }
}

fn text(database: &Connection, sql: &str) -> Option<String> {
    database
        .query_row(sql, [], |row| row.get::<_, String>(0))
        .optional()
        .unwrap()
}

fn all_texts(database: &Connection, sql: &str) -> Vec<String> {
    let mut statement = database.prepare(sql).unwrap();
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// 표 하나의 모든 행을 열 값 문자열로 읽는다
fn table_rows(database: &Connection, table: &str) -> Vec<Vec<String>> {
    let quoted = format!("\"{}\"", table.replace('"', "\"\""));
    let mut statement = database
        .prepare(&format!("SELECT * FROM {quoted}"))
        .unwrap();
    let column_count = statement.column_count();
    statement
        .query_map([], |row| {
            Ok((0..column_count)
                .map(|index| format!("{:?}", row.get_ref(index).unwrap()))
                .collect())
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn prepare(request: &DevelopmentDatabaseRequest) -> DevelopmentDatabaseOutcome {
    prepare_development_database(request).unwrap()
}

fn prepare_error(request: &DevelopmentDatabaseRequest) -> String {
    prepare_development_database(request)
        .expect_err("실패해야 한다")
        .to_string()
}

fn insert_hook_client_id(database: &Connection, id: &str, value: &str) {
    database
        .execute(
            "INSERT INTO app_settings (id, key, value) VALUES (?, 'hook_client_id', ?)",
            [id, value],
        )
        .unwrap();
}

#[test]
fn copies_every_table_including_committed_wal_data_and_keeps_edits_separate() {
    let directories = Directories::new();
    let source = directories.source_database();
    let source_path = directories.source_path();
    let before_database = std::fs::read(&source_path).unwrap();
    let wal_path = PathBuf::from(format!("{}-wal", source_path.display()));
    let before_wal = std::fs::read(&wal_path).unwrap();
    assert!(!before_wal.is_empty());

    prepare(&directories.request(Some("application")));

    let target = Connection::open(directories.target_path()).unwrap();
    let tables_sql = "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name";
    let tables = all_texts(&source, tables_sql);
    assert_eq!(all_texts(&target, tables_sql), tables);
    for table in &tables {
        assert_eq!(
            table_rows(&target, table),
            table_rows(&source, table),
            "{table}"
        );
    }
    assert_eq!(
        text(&target, "PRAGMA integrity_check").as_deref(),
        Some("ok")
    );
    target
        .execute_batch(
            "UPDATE kanban_tasks SET title = 'Development edit'; DELETE FROM app_settings",
        )
        .unwrap();
    assert_eq!(
        text(&source, "SELECT title FROM kanban_tasks WHERE id = 'task'").as_deref(),
        Some("Application task")
    );
    assert_eq!(
        text(
            &source,
            "SELECT value FROM app_settings WHERE id = 'setting'"
        )
        .as_deref(),
        Some("dark")
    );
    assert_eq!(std::fs::read(&source_path).unwrap(), before_database);
    assert_eq!(std::fs::read(&wal_path).unwrap(), before_wal);
}

#[test]
fn refreshes_an_existing_development_database_when_application_mode_is_requested() {
    let directories = Directories::new();
    let source = directories.source_database();
    drop(prepare(&directories.request(Some("application"))));
    let target = Connection::open(directories.target_path()).unwrap();
    target
        .execute_batch("UPDATE kanban_tasks SET title = 'Previous development edit'")
        .unwrap();
    drop(target);
    source
        .execute_batch("UPDATE kanban_tasks SET title = 'Latest application task'")
        .unwrap();

    prepare(&directories.request(Some("application")));

    let target = Connection::open(directories.target_path()).unwrap();
    assert_eq!(
        text(&target, "SELECT title FROM kanban_tasks WHERE id = 'task'").as_deref(),
        Some("Latest application task")
    );
}

#[test]
fn does_not_copy_the_application_hook_client_id() {
    let directories = Directories::new();
    let source = directories.source_database();
    insert_hook_client_id(&source, "client", "application-client");

    prepare(&directories.request(Some("application")));

    let target = Connection::open(directories.target_path()).unwrap();
    let hook_client_id_sql = "SELECT value FROM app_settings WHERE key = 'hook_client_id'";
    assert_eq!(text(&target, hook_client_id_sql), None);
    assert_eq!(
        text(
            &target,
            "SELECT value FROM app_settings WHERE key = 'theme'"
        )
        .as_deref(),
        Some("dark")
    );
    assert_eq!(
        text(&source, hook_client_id_sql).as_deref(),
        Some("application-client")
    );
}

#[test]
fn keeps_the_development_hook_client_id_across_refreshes() {
    let directories = Directories::new();
    let source = directories.source_database();
    insert_hook_client_id(&source, "client", "application-client");
    drop(prepare(&directories.request(Some("application"))));
    let previous_target = Connection::open(directories.target_path()).unwrap();
    insert_hook_client_id(&previous_target, "dev-client", "development-client");
    drop(previous_target);

    prepare(&directories.request(Some("application")));

    let target = Connection::open(directories.target_path()).unwrap();
    assert_eq!(
        all_texts(
            &target,
            "SELECT value FROM app_settings WHERE key = 'hook_client_id'"
        ),
        ["development-client"]
    );
}

#[test]
fn drops_a_development_hook_client_id_that_an_older_copy_took_from_the_application() {
    let directories = Directories::new();
    let source = directories.source_database();
    insert_hook_client_id(&source, "client", "application-client");
    // 이 수정 전의 복사본처럼 설치 앱의 ID가 그대로 들어 있는 개발 DB를 만든다
    source
        .backup(
            rusqlite::MAIN_DB,
            directories.target_path(),
            None::<fn(rusqlite::backup::Progress)>,
        )
        .unwrap();

    prepare(&directories.request(Some("application")));

    let target = Connection::open(directories.target_path()).unwrap();
    assert_eq!(
        text(
            &target,
            "SELECT value FROM app_settings WHERE key = 'hook_client_id'"
        ),
        None
    );
}

#[test]
fn preserves_development_edits_across_restarts_in_existing_mode() {
    for data_source in [None, Some(""), Some("existing")] {
        let directories = Directories::new();
        directories.source_database();
        let target = Connection::open(directories.target_path()).unwrap();
        target
            .execute_batch(
                "CREATE TABLE dev_only (value TEXT); INSERT INTO dev_only VALUES ('keep')",
            )
            .unwrap();
        drop(target);

        drop(prepare(&directories.request(data_source)));
        drop(prepare(&directories.request(data_source)));

        let target = Connection::open(directories.target_path()).unwrap();
        assert_eq!(all_texts(&target, "SELECT value FROM dev_only"), ["keep"]);
    }
}

#[test]
fn does_not_import_application_data_into_a_new_development_database_by_default() {
    for has_custom_source in [false, true] {
        let directories = Directories::new();
        directories.source_database();
        let mut request = directories.request(None);
        if has_custom_source {
            request.application_data_directory = Some(directories.source.clone());
        }

        prepare(&request);

        assert!(!directories.target_path().exists());
    }
}

#[test]
fn keeps_development_data_by_default_even_when_the_application_database_is_corrupt() {
    let directories = Directories::new();
    std::fs::write(directories.source_path(), "corrupt source").unwrap();
    let target = Connection::open(directories.target_path()).unwrap();
    target
        .execute_batch("CREATE TABLE retained (value TEXT); INSERT INTO retained VALUES ('keep')")
        .unwrap();
    drop(target);

    prepare(&directories.request(None));

    let target = Connection::open(directories.target_path()).unwrap();
    assert_eq!(all_texts(&target, "SELECT value FROM retained"), ["keep"]);
}

#[test]
fn keeps_an_explicit_app_data_fixture_by_default() {
    let directories = Directories::new();
    directories.source_database();
    // `--data-dir`로 고른 fixture 디렉터리도 대상 디렉터리일 뿐이라 기본 모드는 아무것도 복사하지 않는다
    let fixture = directories.app_data.join("explicit-fixture");
    let mut request = directories.request(None);
    request.target_directory = fixture.clone();

    prepare(&request);

    assert!(!fixture.join("kanvibe.db").exists());
}

#[test]
fn copies_from_an_explicitly_selected_application_directory_into_an_explicit_target() {
    let directories = Directories::new();
    drop(directories.source_database());
    let custom_source = directories.app_data.join("custom-installation");
    std::fs::rename(&directories.source, &custom_source).unwrap();
    let explicit_target = directories.app_data.join("explicit-target");
    let mut request = directories.request(Some("application"));
    request.application_data_directory = Some(custom_source);
    request.target_directory = explicit_target.clone();

    prepare(&request);

    let target = Connection::open(explicit_target.join("kanvibe.db")).unwrap();
    assert!(
        text(&target, "SELECT value FROM additional_data")
            .unwrap()
            .contains("included")
    );
}

#[test]
fn leaves_packaged_start_automated_and_manual_qa_data_alone() {
    let cases: [fn(&mut DevelopmentDatabaseRequest); 3] = [
        |request| request.is_packaged = true,
        |request| request.is_development = false,
        |request| request.is_qa = true,
    ];
    for adjust in cases {
        let directories = Directories::new();
        directories.source_database();
        let mut request = directories.request(Some("application"));
        adjust(&mut request);

        let outcome = prepare(&request);

        assert!(matches!(
            outcome,
            DevelopmentDatabaseOutcome::NotDevelopmentRun
        ));
        assert!(!directories.target_path().exists());
        assert!(!directories.target.join(INSTANCE_LOCK_FILE_NAME).exists());
    }
}

#[test]
fn warns_and_retains_development_data_when_no_application_database_is_installed() {
    let directories = Directories::new();
    std::fs::write(directories.target_path(), "existing fixture").unwrap();

    let outcome = prepare(&directories.request(Some("application")));

    assert!(
        outcome
            .log_message()
            .unwrap()
            .contains("Application database not found")
    );
    assert_eq!(
        std::fs::read_to_string(directories.target_path()).unwrap(),
        "existing fixture"
    );
}

#[test]
fn rejects_a_missing_explicitly_requested_source() {
    let directories = Directories::new();
    let mut request = directories.request(Some("application"));
    request.application_data_directory = Some(directories.app_data.join("missing"));

    assert!(prepare_error(&request).contains("Application database not found"));
    assert!(!directories.target_path().exists());
}

#[test]
fn preserves_the_old_development_database_if_the_application_database_is_corrupt() {
    let directories = Directories::new();
    std::fs::write(directories.source_path(), "corrupt source").unwrap();
    let target = Connection::open(directories.target_path()).unwrap();
    target
        .execute_batch("CREATE TABLE retained (value TEXT); INSERT INTO retained VALUES ('keep')")
        .unwrap();

    assert!(prepare_development_database(&directories.request(Some("application"))).is_err());

    assert_eq!(
        text(&target, "SELECT value FROM retained").as_deref(),
        Some("keep")
    );
}

#[test]
fn rejects_the_application_directory_even_in_existing_mode() {
    for data_source in ["application", "existing"] {
        let directories = Directories::new();
        let mut request = directories.request(Some(data_source));
        request.target_directory = directories.source.clone();

        assert!(prepare_error(&request).contains("separate database directories"));
        assert!(!directories.source_path().exists());
    }
}

#[test]
fn rejects_a_development_directory_symlink_to_application_data() {
    let directories = Directories::new();
    directories.source_database();
    let alias = directories.app_data.join("alias");
    std::os::unix::fs::symlink(&directories.source, &alias).unwrap();
    let mut request = directories.request(None);
    request.target_directory = alias;

    assert!(prepare_error(&request).contains("separate database directories"));
}

#[test]
fn rejects_a_development_database_linked_to_the_application_database() {
    let link_kinds: [fn(&Path, &Path) -> std::io::Result<()>; 2] = [
        |source, target| std::os::unix::fs::symlink(source, target),
        |source, target| std::fs::hard_link(source, target),
    ];
    for link in link_kinds {
        let directories = Directories::new();
        directories.source_database();
        link(&directories.source_path(), &directories.target_path()).unwrap();

        assert!(prepare_error(&directories.request(None)).contains("separate database files"));
    }
}

#[test]
fn rejects_a_dangling_development_database_symlink_before_it_can_create_a_shared_database() {
    let directories = Directories::new();
    std::os::unix::fs::symlink(directories.source_path(), directories.target_path()).unwrap();

    assert!(prepare_error(&directories.request(None)).contains("separate database files"));
    assert!(!directories.source_path().exists());
}

#[test]
fn rejects_a_second_development_instance_before_refreshing_its_database() {
    let directories = Directories::new();
    directories.source_database();
    let _first_instance = prepare(&directories.request(None));

    let error = prepare_error(&directories.request(Some("application")));

    assert!(error.contains("Another development app"), "{error}");
    assert!(!directories.target_path().exists());
}

#[test]
fn rejects_an_unknown_data_source_mode() {
    let directories = Directories::new();

    assert!(prepare_error(&directories.request(Some("typo"))).contains("--data-source"));
}
