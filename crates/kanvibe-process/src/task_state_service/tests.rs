use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use kanvibe_db::{Database, DatabaseLocation};
use kanvibe_domain::task::{KanbanTask, TASK_STATUS_ORDER, TaskStatus};

use super::status_sync::{TaskStatusSync, TaskStatusSyncResult};
use super::*;
use crate::exec::testing::ScriptedCommandRunner;
use crate::host_file_access::{HostFileAccess, TextFileReadResult};
use crate::kanvibe_project_state::KanvibeTaskState;
use crate::kanvibe_state_store::HostKanvibeStateStore;
use crate::kanvibe_state_store::testing::{RecordingProjectRecords, RecordingStateStore};

fn task(id: &str, status: TaskStatus) -> KanbanTask {
    KanbanTask {
        id: id.into(),
        title: id.into(),
        description: None,
        status,
        branch_name: None,
        worktree_path: None,
        session_type: None,
        session_name: None,
        ssh_host: None,
        agent_type: None,
        project_id: None,
        base_branch: None,
        pr_url: None,
        priority: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn project_record(ssh_host: Option<&str>) -> RecordingProjectRecords {
    let records = RecordingProjectRecords::default();
    let mut root_project = crate::kanvibe_state_store::testing::project(None, ssh_host);
    root_project.repo_path = "/workspace/repo".into();
    *records.project.lock().unwrap() = Some(root_project);
    records
}

fn failing(function_names: &'static [&'static str], message: &'static str) -> RecordingStateStore {
    RecordingStateStore {
        failure: Box::new(move |function_name| {
            function_names
                .contains(&function_name)
                .then(|| message.to_string())
        }),
        ..RecordingStateStore::default()
    }
}

#[test]
fn state_write_without_a_path_is_ignored() {
    let store = RecordingStateStore::default();
    let records = RecordingProjectRecords::default();

    TaskStateService::new(&store, &records).persist_task_state_at_path(
        None,
        "task-1",
        TaskStatus::Progress,
        Some("ssh-host"),
    );

    assert!(store.calls.lock().unwrap().is_empty());
}

#[test]
fn git_exclude_is_updated_before_the_status_file_is_written() {
    let store = RecordingStateStore::default();
    let records = RecordingProjectRecords::default();

    TaskStateService::new(&store, &records).persist_task_state_at_path(
        Some("/remote/repo__worktrees/feature-task"),
        "task-1",
        TaskStatus::Review,
        Some("remote-host"),
    );

    assert_eq!(
        *store.calls.lock().unwrap(),
        [
            "add_ai_tool_patterns_to_git_exclude /remote/repo__worktrees/feature-task remote-host",
            "write_task_status /remote/repo__worktrees/feature-task review remote-host",
        ]
    );
}

#[test]
fn failed_git_exclude_does_not_block_the_status_write() {
    let store = failing(&["add_ai_tool_patterns_to_git_exclude"], "not a git repo");
    let records = RecordingProjectRecords::default();

    TaskStateService::new(&store, &records).persist_task_state_at_path(
        Some("/workspace/repo"),
        "task-1",
        TaskStatus::Progress,
        None,
    );

    assert_eq!(
        store.calls_of("write_task_status"),
        ["write_task_status /workspace/repo progress null"]
    );
}

#[test]
fn repeated_hook_events_check_git_exclude_once_but_write_every_status() {
    let store = RecordingStateStore::default();
    let records = RecordingProjectRecords::default();
    let service = TaskStateService::new(&store, &records);
    let repo_path = Some("/remote/repo__worktrees/feature-task");

    for status in [
        TaskStatus::Progress,
        TaskStatus::Pending,
        TaskStatus::Review,
    ] {
        service.persist_task_state_at_path(repo_path, "task-1", status, Some("remote-host"));
    }

    assert_eq!(
        store.calls_of("add_ai_tool_patterns_to_git_exclude").len(),
        1
    );
    let written_statuses: Vec<String> = store
        .calls_of("write_task_status")
        .iter()
        .map(|call| call.split(' ').nth(2).unwrap().to_string())
        .collect();
    assert_eq!(written_statuses, ["progress", "pending", "review"]);
}

/// 첫 exclude 확인만 `release`를 받을 때까지 붙잡아 둔다
struct BlockingExcludeStore {
    release: Mutex<Option<mpsc::Receiver<()>>>,
    started: mpsc::SyncSender<()>,
}

impl BlockingExcludeStore {
    fn block_first_exclude(&self) {
        if let Some(release) = self.release.lock().unwrap().take() {
            self.started.send(()).unwrap();
            release.recv().unwrap();
        }
    }
}

#[test]
fn exclude_checks_are_kept_per_repository_and_host_and_overlapping_writes_wait_for_the_running_check()
 {
    let (release_sender, release_receiver) = mpsc::channel();
    let (started_sender, started_receiver) = mpsc::sync_channel(1);
    let blocking = BlockingExcludeStore {
        release: Mutex::new(Some(release_receiver)),
        started: started_sender,
    };
    let store = RecordingStateStore {
        failure: Box::new(move |function_name| {
            if function_name == "add_ai_tool_patterns_to_git_exclude" {
                blocking.block_first_exclude();
            }
            None
        }),
        ..RecordingStateStore::default()
    };
    let records = RecordingProjectRecords::default();
    let service = TaskStateService::new(&store, &records);

    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            service.persist_task_state_at_path(
                Some("/repo/a"),
                "task-1",
                TaskStatus::Progress,
                Some("host-a"),
            )
        });
        started_receiver.recv().unwrap();
        let overlapping = scope.spawn(|| {
            service.persist_task_state_at_path(
                Some("/repo/a"),
                "task-1",
                TaskStatus::Review,
                Some("host-a"),
            )
        });
        std::thread::sleep(Duration::from_millis(50));

        // 확인이 끝나기 전에는 어느 쪽도 상태 파일을 쓰지 않아 exclude 뒤에 기록하는 순서가 유지된다
        assert_eq!(
            store.calls_of("add_ai_tool_patterns_to_git_exclude").len(),
            1
        );
        assert!(store.calls_of("write_task_status").is_empty());
        release_sender.send(()).unwrap();
        first.join().unwrap();
        overlapping.join().unwrap();
    });
    assert_eq!(store.calls_of("write_task_status").len(), 2);

    service.persist_task_state_at_path(
        Some("/repo/a"),
        "task-2",
        TaskStatus::Progress,
        Some("host-b"),
    );
    service.persist_task_state_at_path(
        Some("/repo/b"),
        "task-3",
        TaskStatus::Progress,
        Some("host-a"),
    );
    assert_eq!(
        store.calls_of("add_ai_tool_patterns_to_git_exclude").len(),
        3
    );
}

#[test]
fn failed_exclude_checks_are_retried_and_remembered_checks_expire() {
    let exclude_attempts = AtomicUsize::new(0);
    let store = RecordingStateStore {
        failure: Box::new(move |function_name| {
            let is_first_exclude = function_name == "add_ai_tool_patterns_to_git_exclude"
                && exclude_attempts.fetch_add(1, Ordering::SeqCst) == 0;
            is_first_exclude.then(|| "Connection reset by peer".to_string())
        }),
        ..RecordingStateStore::default()
    };
    let records = RecordingProjectRecords::default();
    let base = Instant::now();
    let elapsed = Arc::new(Mutex::new(Duration::ZERO));
    let clock_elapsed = Arc::clone(&elapsed);
    let service = TaskStateService::with_clock(
        &store,
        &records,
        Box::new(move || base + *clock_elapsed.lock().unwrap()),
    );
    let persist = || {
        service.persist_task_state_at_path(
            Some("/repo/a"),
            "task-1",
            TaskStatus::Progress,
            Some("host-a"),
        )
    };

    persist();
    persist();
    assert_eq!(
        store.calls_of("add_ai_tool_patterns_to_git_exclude").len(),
        2
    );

    persist();
    assert_eq!(
        store.calls_of("add_ai_tool_patterns_to_git_exclude").len(),
        2
    );

    // 누군가 exclude를 지워도 이 시간 뒤의 기록이 복구한다
    *elapsed.lock().unwrap() = Duration::from_secs(10 * 60);
    persist();
    assert_eq!(
        store.calls_of("add_ai_tool_patterns_to_git_exclude").len(),
        3
    );
}

#[test]
fn failed_status_write_does_not_block_the_caller() {
    let store = failing(&["write_task_status"], "disk full");
    let records = RecordingProjectRecords::default();

    TaskStateService::new(&store, &records).persist_task_state_at_path(
        Some("/workspace/repo"),
        "task-1",
        TaskStatus::Review,
        Some("ssh-host"),
    );

    assert_eq!(
        store.calls_of("write_task_status"),
        ["write_task_status /workspace/repo review ssh-host"]
    );
}

#[test]
fn task_with_a_worktree_path_writes_there_without_looking_up_the_project() {
    let store = RecordingStateStore::default();
    let records = project_record(None);
    let mut worktree_task = task("task-1", TaskStatus::Pending);
    worktree_task.worktree_path = Some("/workspace/repo__worktrees/task-1".into());
    worktree_task.ssh_host = Some("ssh-host".into());
    worktree_task.project_id = Some("project-1".into());
    worktree_task.branch_name = Some("feature/sync".into());

    TaskStateService::new(&store, &records).persist_task_state_for_task(&worktree_task);

    assert!(records.lookups.lock().unwrap().is_empty());
    assert_eq!(
        store.calls_of("write_task_status"),
        ["write_task_status /workspace/repo__worktrees/task-1 pending ssh-host"]
    );
}

#[test]
fn default_branch_task_without_a_worktree_writes_to_the_project_root() {
    let store = RecordingStateStore::default();
    let records = project_record(Some("project-ssh"));
    let mut root_task = task("task-main", TaskStatus::Todo);
    root_task.project_id = Some("project-1".into());
    root_task.branch_name = Some("main".into());

    TaskStateService::new(&store, &records).persist_task_state_for_task(&root_task);

    assert_eq!(*records.lookups.lock().unwrap(), ["find_project project-1"]);
    assert_eq!(
        store.calls_of("write_task_status"),
        ["write_task_status /workspace/repo todo project-ssh"]
    );
}

#[test]
fn non_default_branch_task_without_a_worktree_writes_nothing() {
    let store = RecordingStateStore::default();
    let records = project_record(None);
    let mut feature_task = task("task-feature", TaskStatus::Progress);
    feature_task.project_id = Some("project-1".into());
    feature_task.branch_name = Some("feature/sync".into());

    TaskStateService::new(&store, &records).persist_task_state_for_task(&feature_task);

    assert_eq!(*records.lookups.lock().unwrap(), ["find_project project-1"]);
    assert!(store.calls_of("write_task_status").is_empty());
}

#[test]
fn reads_only_the_saved_task_status() {
    let store = RecordingStateStore::default();
    *store.task_state.lock().unwrap() = Some(KanvibeTaskState {
        schema_version: 1,
        status: TaskStatus::Done,
        updated_at: Some("2026-06-03T00:00:00.000Z".into()),
    });
    let records = RecordingProjectRecords::default();

    let status = TaskStateService::new(&store, &records)
        .read_persisted_task_status_at_path(Some("/workspace/repo"), Some("ssh-host"))
        .unwrap();

    assert_eq!(status, Some(TaskStatus::Done));
    assert_eq!(
        store.calls_of("read_task_state"),
        ["read_task_state /workspace/repo ssh-host"]
    );
}

#[test]
fn root_task_metadata_keeps_priority_in_the_project_state() {
    let store = RecordingStateStore::default();
    let records = project_record(None);
    let mut root_task = task("task-main", TaskStatus::Todo);
    root_task.project_id = Some("project-1".into());
    root_task.branch_name = Some("main".into());
    root_task.description = Some("설명".into());
    root_task.priority = Some(kanvibe_domain::task::TaskPriority::High);

    TaskStateService::new(&store, &records).persist_task_metadata_for_task(&root_task);

    assert_eq!(
        store.calls_of("write_task_metadata"),
        [
            "write_task_metadata /workspace/repo KanvibeTaskMetadataUpdate { description: Some(Some(\"설명\")), priority: None } null"
        ]
    );
    assert_eq!(
        store.calls_of("write_project_priority"),
        ["write_project_priority /workspace/repo high null"]
    );
}

// ---- 상태 파일 → DB 반영 ----

#[derive(Default)]
struct CountingNotifier {
    broadcasts: AtomicUsize,
}

impl BoardUpdateNotifier for CountingNotifier {
    fn broadcast_board_update(&self) {
        self.broadcasts.fetch_add(1, Ordering::SeqCst);
    }
}

struct DatabaseFixture {
    _directory: tempfile::TempDir,
    database: Arc<Database>,
}

const SNAPSHOT_TIME: &str = "2026-11-15 00:00:00.000";

fn database_fixture() -> DatabaseFixture {
    let directory = tempfile::tempdir().unwrap();
    let location = DatabaseLocation::in_app_data_directory(directory.path(), None);
    let database = Arc::new(Database::open(&location).unwrap());
    DatabaseFixture {
        _directory: directory,
        database,
    }
}

/// 상태는 progress, worktree는 `/fixture/<id>`, 수정 시각은 고정 스냅샷인 task. `overrides`는 `(컬럼, 값)`이다
fn insert_task(database: &Database, id: &str, overrides: &[(&str, Option<&str>)]) {
    let mut columns = vec![
        ("status", Some("progress".to_string())),
        ("worktree_path", Some(format!("/fixture/{id}"))),
        ("updated_at", Some(SNAPSHOT_TIME.to_string())),
    ];
    for (column, value) in overrides {
        columns.retain(|(existing, _)| existing != column);
        columns.push((column, value.map(str::to_string)));
    }
    let names: Vec<&str> = columns.iter().map(|(column, _)| *column).collect();
    let placeholders: Vec<String> = (0..columns.len())
        .map(|index| format!("?{}", index + 3))
        .collect();
    let sql = format!(
        "INSERT INTO kanban_tasks (id, title, {}) VALUES (?1, ?2, {})",
        names.join(", "),
        placeholders.join(", ")
    );
    let mut values: Vec<Option<String>> = vec![Some(id.into()), Some(id.into())];
    values.extend(columns.into_iter().map(|(_, value)| value));
    database
        .connection()
        .execute(&sql, rusqlite::params_from_iter(values))
        .unwrap();
}

fn task_status(database: &Database, id: &str) -> Option<String> {
    database
        .connection()
        .query_row(
            "SELECT status FROM kanban_tasks WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .ok()
}

fn single_file_reader(content: &'static str) -> RecordingStateStore {
    RecordingStateStore {
        text_files: Box::new(move |paths: &[String], _| Ok(files_with(paths, content))),
        ..RecordingStateStore::default()
    }
}

fn files_with(paths: &[String], content: &str) -> BTreeMap<String, TextFileReadResult> {
    paths
        .iter()
        .map(|path| {
            (
                path.clone(),
                TextFileReadResult {
                    exists: true,
                    content: content.to_string(),
                },
            )
        })
        .collect()
}

fn sync_from_files(database: &Database, store: &RecordingStateStore) -> TaskStatusSyncResult {
    let notifier = CountingNotifier::default();
    TaskStatusSync::new(database, store, &notifier)
        .sync_task_statuses_from_files()
        .unwrap()
}

fn changed() -> TaskStatusSyncResult {
    TaskStatusSyncResult {
        has_changed: true,
        errors: Vec::new(),
    }
}

#[test]
fn file_status_corrects_existing_tasks_without_git_discovery() {
    for status in TASK_STATUS_ORDER {
        let fixture = database_fixture();
        let stored_status = if status == TaskStatus::Progress {
            "done"
        } else {
            "progress"
        };
        insert_task(
            &fixture.database,
            "standalone",
            &[("status", Some(stored_status))],
        );
        let content = format!("{{\"status\":\"{}\"}}", status.as_str());
        let store = RecordingStateStore {
            text_files: Box::new(move |paths: &[String], _| Ok(files_with(paths, &content))),
            ..RecordingStateStore::default()
        };

        assert_eq!(sync_from_files(&fixture.database, &store), changed());
        assert_eq!(
            task_status(&fixture.database, "standalone").as_deref(),
            Some(status.as_str())
        );
        assert_eq!(
            store.calls_of("read_text_files"),
            ["read_text_files /fixture/standalone/.kanvibe/status.json null"]
        );
    }
}

#[test]
fn one_batch_read_per_host_using_the_project_host_and_root_task_path() {
    let fixture = database_fixture();
    fixture
        .database
        .connection()
        .execute(
            "INSERT INTO projects (id, name, repo_path, default_branch, ssh_host) VALUES ('project', 'api', '/remote/api', 'main', 'host-a')",
            [],
        )
        .unwrap();
    insert_task(
        &fixture.database,
        "root",
        &[
            ("project_id", Some("project")),
            ("branch_name", Some("main")),
            ("worktree_path", None),
        ],
    );
    insert_task(&fixture.database, "remote", &[("ssh_host", Some("host-a"))]);
    insert_task(&fixture.database, "local", &[]);
    insert_task(
        &fixture.database,
        "same-path",
        &[("worktree_path", Some("/fixture/local"))],
    );
    let store = single_file_reader("{\"schemaVersion\":1,\"status\":\"review\"}");

    assert_eq!(sync_from_files(&fixture.database, &store), changed());

    let mut reads = store.calls_of("read_text_files");
    reads.sort();
    assert_eq!(
        reads,
        [
            "read_text_files /fixture/local/.kanvibe/status.json null",
            "read_text_files /remote/api/.kanvibe/status.json,/fixture/remote/.kanvibe/status.json host-a",
        ]
    );
    assert_eq!(
        task_status(&fixture.database, "root").as_deref(),
        Some("review")
    );
    assert_eq!(
        task_status(&fixture.database, "same-path").as_deref(),
        Some("review")
    );
}

#[test]
fn missing_broken_or_unchanged_files_and_pathless_tasks_leave_the_database_alone() {
    let fixture = database_fixture();
    for id in ["missing", "broken", "unchanged"] {
        insert_task(&fixture.database, id, &[]);
    }
    insert_task(&fixture.database, "no-path", &[("worktree_path", None)]);
    let store = RecordingStateStore {
        text_files: Box::new(|_, _| {
            let file = |exists, content: &str| TextFileReadResult {
                exists,
                content: content.into(),
            };
            Ok(BTreeMap::from([
                (
                    "/fixture/missing/.kanvibe/status.json".into(),
                    file(false, ""),
                ),
                (
                    "/fixture/broken/.kanvibe/status.json".into(),
                    file(true, "{\"status\":\"unknown\"}"),
                ),
                (
                    "/fixture/unchanged/.kanvibe/status.json".into(),
                    file(true, "{\"status\":\"progress\"}"),
                ),
            ]))
        }),
        ..RecordingStateStore::default()
    };

    assert_eq!(
        sync_from_files(&fixture.database, &store),
        TaskStatusSyncResult::default()
    );
    for id in ["missing", "broken", "unchanged", "no-path"] {
        assert_eq!(
            task_status(&fixture.database, id).as_deref(),
            Some("progress")
        );
    }
    assert!(!store.calls_of("read_text_files")[0].contains("/fixture/no-path/"));
}

#[test]
fn one_failed_host_read_does_not_block_other_hosts() {
    let fixture = database_fixture();
    insert_task(
        &fixture.database,
        "offline",
        &[("ssh_host", Some("offline-host"))],
    );
    insert_task(&fixture.database, "local", &[]);
    let store = RecordingStateStore {
        text_files: Box::new(|paths: &[String], ssh_host| match ssh_host {
            Some(_) => Err(crate::host_file_access::HostFileError::Message(
                "offline".into(),
            )),
            None => Ok(files_with(&paths[..1], "{\"status\":\"pending\"}")),
        }),
        ..RecordingStateStore::default()
    };

    let result = sync_from_files(&fixture.database, &store);

    assert!(result.has_changed);
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].contains("offline"));
    assert_eq!(
        task_status(&fixture.database, "offline").as_deref(),
        Some("progress")
    );
    assert_eq!(
        task_status(&fixture.database, "local").as_deref(),
        Some("pending")
    );
}

#[test]
fn updates_made_while_reading_are_not_overwritten() {
    for status in ["done", "progress"] {
        let fixture = database_fixture();
        insert_task(&fixture.database, "concurrent", &[]);
        let database = Arc::clone(&fixture.database);
        let store = RecordingStateStore {
            text_files: Box::new(move |paths: &[String], _| {
                database
                    .connection()
                    .execute(
                        "UPDATE kanban_tasks SET status = ?1, title = 'newer', updated_at = '2026-11-15 00:00:01.123' WHERE id = 'concurrent'",
                        [status],
                    )
                    .unwrap();
                Ok(files_with(&paths[..1], "{\"status\":\"review\"}"))
            }),
            ..RecordingStateStore::default()
        };

        assert_eq!(
            sync_from_files(&fixture.database, &store),
            TaskStatusSyncResult::default()
        );
        assert_eq!(
            task_status(&fixture.database, "concurrent").as_deref(),
            Some(status)
        );
    }
}

#[test]
fn tasks_deleted_while_reading_are_not_recreated() {
    let fixture = database_fixture();
    insert_task(&fixture.database, "deleted", &[]);
    let database = Arc::clone(&fixture.database);
    let store = RecordingStateStore {
        text_files: Box::new(move |paths: &[String], _| {
            database
                .connection()
                .execute("DELETE FROM kanban_tasks WHERE id = 'deleted'", [])
                .unwrap();
            Ok(files_with(&paths[..1], "{\"status\":\"review\"}"))
        }),
        ..RecordingStateStore::default()
    };

    assert_eq!(
        sync_from_files(&fixture.database, &store),
        TaskStatusSyncResult::default()
    );
    let count: i64 = fixture
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM kanban_tasks", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn no_tasks_means_no_file_reads() {
    let fixture = database_fixture();
    let store = RecordingStateStore::default();

    assert_eq!(
        sync_from_files(&fixture.database, &store),
        TaskStatusSyncResult::default()
    );
    assert!(store.calls_of("read_text_files").is_empty());
}

/// TS 테스트처럼 원격 `execGit`만 가짜로 두고 실제 파일 읽기 경로로 status.json을 받는다
fn refresh_with_remote_output(
    fixture: &DatabaseFixture,
    respond: impl Fn() -> String + Send + Sync + 'static,
) -> usize {
    let runner = Arc::new(ScriptedCommandRunner::new(move |_| Ok(respond())));
    let files = HostFileAccess::new(runner, "/home/local");
    let store = HostKanvibeStateStore::new(&files);
    let notifier = CountingNotifier::default();
    let hosts = HashSet::from(["host".to_string()]);

    TaskStatusSync::new(&fixture.database, &store, &notifier)
        .refresh_host_task_statuses(&hosts, &|| true)
        .unwrap();
    notifier.broadcasts.load(Ordering::SeqCst)
}

#[test]
fn reconnect_refresh_updates_timestamps_stored_at_second_precision() {
    let fixture = database_fixture();
    insert_task(
        &fixture.database,
        "fixture",
        &[
            ("ssh_host", Some("host")),
            ("worktree_path", Some("/fixture")),
            ("updated_at", Some("2026-11-14 00:00:00")),
        ],
    );

    let broadcasts = refresh_with_remote_output(&fixture, || {
        "{\"schemaVersion\":1,\"status\":\"review\"}".into()
    });

    assert_eq!(
        task_status(&fixture.database, "fixture").as_deref(),
        Some("review")
    );
    assert_eq!(broadcasts, 1);
}

#[test]
fn reconnect_refresh_preserves_a_newer_live_update_made_during_the_read() {
    let fixture = database_fixture();
    insert_task(
        &fixture.database,
        "fixture",
        &[
            ("ssh_host", Some("host")),
            ("worktree_path", Some("/fixture")),
        ],
    );
    let database = Arc::clone(&fixture.database);

    refresh_with_remote_output(&fixture, move || {
        database
            .connection()
            .execute("UPDATE kanban_tasks SET status = 'done', updated_at = '2026-11-14 01:00:00.123' WHERE id = 'fixture'", [])
            .unwrap();
        "{\"schemaVersion\":1,\"status\":\"review\"}".into()
    });

    assert_eq!(
        task_status(&fixture.database, "fixture").as_deref(),
        Some("done")
    );
}

// ---- 다시 연결된 호스트의 현재 상태 스냅샷(TS hostStatusReconnect.test.ts) ----

/// `host-a`만 다시 읽는다. 원격 명령은 `runner`가 답하고, 보드 알림 횟수를 돌려준다
fn refresh_host_a(
    fixture: &DatabaseFixture,
    runner: Arc<ScriptedCommandRunner>,
    is_current: &dyn Fn() -> bool,
) -> usize {
    let files = HostFileAccess::new(runner, "/home/local");
    let store = HostKanvibeStateStore::new(&files);
    let notifier = CountingNotifier::default();
    let hosts = HashSet::from(["host-a".to_string()]);
    TaskStatusSync::new(&fixture.database, &store, &notifier)
        .refresh_host_task_statuses(&hosts, is_current)
        .unwrap();
    notifier.broadcasts.load(Ordering::SeqCst)
}

fn host_a_task(fixture: &DatabaseFixture) {
    insert_task(
        &fixture.database,
        "one",
        &[
            ("ssh_host", Some("host-a")),
            ("worktree_path", Some("/fixture/one")),
        ],
    );
}

#[test]
fn reconnect_reads_only_the_current_snapshot_and_broadcasts_a_change_once() {
    let fixture = database_fixture();
    host_a_task(&fixture);
    let runner = Arc::new(ScriptedCommandRunner::returning(
        "{\"schemaVersion\":1,\"status\":\"review\"}",
    ));

    let broadcasts = refresh_host_a(&fixture, Arc::clone(&runner), &|| true);

    let calls = runner.calls();
    assert!(calls.iter().any(
        |call| call.command.contains("/fixture/one/.kanvibe/status.json")
            && call.ssh_host.as_deref() == Some("host-a")
    ));
    assert_eq!(
        task_status(&fixture.database, "one").as_deref(),
        Some("review")
    );
    assert_eq!(broadcasts, 1);
}

#[test]
fn reconnect_does_not_overwrite_a_live_update_that_won_during_the_read() {
    let fixture = database_fixture();
    host_a_task(&fixture);
    let database = Arc::clone(&fixture.database);
    let runner = Arc::new(ScriptedCommandRunner::new(move |_| {
        database
            .connection()
            .execute("UPDATE kanban_tasks SET status = 'done', updated_at = '2026-11-16 00:00:00.000' WHERE id = 'one'", [])
            .unwrap();
        Ok("{\"schemaVersion\":1,\"status\":\"review\"}".into())
    }));

    let broadcasts = refresh_host_a(&fixture, runner, &|| true);

    assert_eq!(
        task_status(&fixture.database, "one").as_deref(),
        Some("done")
    );
    assert_eq!(broadcasts, 0);
}

#[test]
fn reconnect_drops_a_snapshot_returned_after_the_connection_was_replaced() {
    let fixture = database_fixture();
    host_a_task(&fixture);
    let is_current = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let replaced = Arc::clone(&is_current);
    let runner = Arc::new(ScriptedCommandRunner::new(move |_| {
        replaced.store(false, Ordering::SeqCst);
        Ok("{\"schemaVersion\":1,\"status\":\"done\"}".into())
    }));

    let broadcasts = refresh_host_a(&fixture, runner, &|| is_current.load(Ordering::SeqCst));

    assert_eq!(
        task_status(&fixture.database, "one").as_deref(),
        Some("progress")
    );
    assert_eq!(broadcasts, 0);
}

#[test]
fn reconnect_ignores_other_hosts_and_tasks_without_an_execution_directory() {
    let fixture = database_fixture();
    insert_task(
        &fixture.database,
        "other",
        &[
            ("status", Some("todo")),
            ("ssh_host", Some("host-b")),
            ("worktree_path", Some("/other")),
        ],
    );
    insert_task(
        &fixture.database,
        "missing",
        &[
            ("status", Some("todo")),
            ("ssh_host", Some("host-a")),
            ("worktree_path", None),
        ],
    );
    let runner = Arc::new(ScriptedCommandRunner::returning(
        "{\"schemaVersion\":1,\"status\":\"done\"}",
    ));

    let broadcasts = refresh_host_a(&fixture, Arc::clone(&runner), &|| true);

    assert!(runner.calls().is_empty());
    assert_eq!(
        task_status(&fixture.database, "other").as_deref(),
        Some("todo")
    );
    assert_eq!(
        task_status(&fixture.database, "missing").as_deref(),
        Some("todo")
    );
    assert_eq!(broadcasts, 0);
}
