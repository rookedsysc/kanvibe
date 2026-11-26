//! TS `kanvibeTargetsConcurrency`·`kanvibeProjectConcurrency`·`kanvibeStateSymlink` 테스트.
//! "원격" 접근은 생성한 원격 명령을 이 머신의 sh로 실제 실행해, 로컬 잠금과 원격 셸 잠금이 같은 디스크 규약으로 맞물리는지 본다.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use kanvibe_domain::task::TaskPriority;
use serde_json::Value;

use super::*;
use crate::exec::testing::LoopbackShellRunner;

const FIXTURE_HOST: Option<&str> = Some("fixture-host");
/// 다른 프로세스 쓰기 테스트가 자식 테스트 프로세스에만 넘기는 값. 이 프로세스의 환경은 바꾸지 않는다
const CHILD_REPOSITORY_VARIABLE: &str = "KANVIBE_PROCESS_TEST_TARGETS_REPOSITORY";
const CHILD_CLIENT_VARIABLE: &str = "KANVIBE_PROCESS_TEST_TARGETS_CLIENT";

fn loopback_files() -> HostFileAccess {
    HostFileAccess::new(
        Arc::new(LoopbackShellRunner::new("/nonexistent-home")),
        "/nonexistent-home",
    )
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn sorted_entries(directory: &Path) -> Vec<String> {
    let mut entries: Vec<String> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    entries
}

fn read_targets(repo: &Path) -> Vec<Value> {
    let content = std::fs::read_to_string(repo.join(".kanvibe/targets.json")).unwrap();
    let state: Value = serde_json::from_str(&content).unwrap();
    state["targets"].as_array().unwrap().clone()
}

fn target(url: String, task_id: String) -> KanvibeHookTarget {
    KanvibeHookTarget {
        url,
        task_id,
        ..KanvibeHookTarget::default()
    }
}

fn local_hostname_hex() -> String {
    let output = Command::new("uname").arg("-n").output().unwrap();
    hex::encode(String::from_utf8_lossy(&output.stdout).trim_end_matches('\n'))
}

/// 이미 끝난 프로세스의 pid
fn exited_process_id() -> u32 {
    let mut child = Command::new("true").spawn().unwrap();
    let process_id = child.id();
    child.wait().unwrap();
    process_id
}

fn update_with(
    files: &HostFileAccess,
    target_path: &str,
    ssh_host: Option<&str>,
    update: &mut crate::host_file_access::TextFileUpdate<'_>,
) -> Result<(), HostFileError> {
    files.update_text_file(
        target_path,
        ssh_host,
        None,
        &ExecGitOptions::default(),
        update,
    )
}

/// mode별로 저장소 접근 경로를 고른다. mixed는 짝수 번째만 원격으로 쓴다
fn access_host(mode: &str, index: usize) -> Option<&'static str> {
    let is_remote = mode == "remote" || (mode == "mixed" && index.is_multiple_of(2));
    is_remote.then_some("fixture-host")
}

fn upsert_distinct_clients_concurrently(repo: &Path, mode: &str, client_count: usize) {
    let files = loopback_files();
    let repo_path = text(repo);
    std::thread::scope(|scope| {
        for index in 0..client_count {
            let (files, repo_path) = (&files, &repo_path);
            scope.spawn(move || {
                let client = target(
                    format!("http://client-{index}:19736"),
                    format!("task-{index}"),
                );
                upsert_kanvibe_hook_target(files, repo_path, &client, access_host(mode, index))
                    .unwrap();
            });
        }
    });
}

fn assert_all_distinct_clients_preserved(repo: &Path, client_count: usize) {
    let mut targets = read_targets(repo);
    targets.sort_by_key(|target| target["taskId"].as_str().unwrap().to_string());
    let mut expected: Vec<Value> = (0..client_count)
        .map(|index| serde_json::json!({ "url": format!("http://client-{index}:19736"), "taskId": format!("task-{index}") }))
        .collect();
    expected.sort_by_key(|target| target["taskId"].as_str().unwrap().to_string());
    assert_eq!(targets, expected);
    assert_eq!(sorted_entries(&repo.join(".kanvibe")), ["targets.json"]);
}

#[test]
fn preserves_all_concurrent_distinct_clients_with_local_remote_and_mixed_access() {
    for mode in ["local", "remote", "mixed"] {
        let repo = tempfile::Builder::new()
            .prefix("kanvibe-targets-'")
            .tempdir()
            .unwrap();

        upsert_distinct_clients_concurrently(repo.path(), mode, 8);

        assert_all_distinct_clients_preserved(repo.path(), 8);
    }
}

/// TS에서 부하가 걸리면 가끔 쓰기를 잃던 경로를 더 세게 누른다. 스레드 16개가 세 가지 접근 방식으로 여러 번 반복한다
#[test]
fn stress_concurrent_distinct_clients_never_lose_a_write() {
    for _round in 0..3 {
        for mode in ["local", "remote", "mixed"] {
            let repo = tempfile::tempdir().unwrap();

            upsert_distinct_clients_concurrently(repo.path(), mode, 16);

            assert_all_distinct_clients_preserved(repo.path(), 16);
        }
    }
}

/// 다른 프로세스 쓰기 테스트가 띄우는 자식 테스트. 부모가 넘긴 값이 없으면 아무것도 하지 않는다
#[test]
fn separate_process_target_writer() {
    let (Ok(repo_path), Ok(client)) = (
        std::env::var(CHILD_REPOSITORY_VARIABLE),
        std::env::var(CHILD_CLIENT_VARIABLE),
    ) else {
        return;
    };
    let files = loopback_files();
    std::thread::scope(|scope| {
        for index in 0..4 {
            let (files, repo_path, client) = (&files, &repo_path, &client);
            scope.spawn(move || {
                let writer_target = target(
                    format!("http://client-{client}-{index}"),
                    format!("task-{index}"),
                );
                upsert_kanvibe_hook_target(files, repo_path, &writer_target, None).unwrap();
            });
        }
    });
}

fn spawn_separate_writer(repo_path: &str, client: usize) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "kanvibe_project_state::concurrency_tests::separate_process_target_writer",
        ])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env(CHILD_REPOSITORY_VARIABLE, repo_path)
        .env(CHILD_CLIENT_VARIABLE, client.to_string())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap()
}

#[test]
fn keeps_registrations_made_by_two_separate_processes() {
    let repo = tempfile::tempdir().unwrap();
    let repo_path = text(repo.path());

    let writers: Vec<_> = (0..2)
        .map(|client| spawn_separate_writer(&repo_path, client))
        .collect();
    for mut writer in writers {
        assert!(writer.wait().unwrap().success());
    }

    assert_eq!(read_targets(repo.path()).len(), 8);
}

fn saved_target(repo: &Path) -> String {
    let target_path = repo.join("targets.json");
    std::fs::write(&target_path, "saved").unwrap();
    text(&target_path)
}

#[test]
fn does_not_publish_or_remove_a_successor_after_the_lock_is_replaced() {
    for host in [None, FIXTURE_HOST] {
        let repo = tempfile::tempdir().unwrap();
        let target_path = saved_target(repo.path());
        let marker = format!(
            "owner-{}-successor-{}",
            std::process::id(),
            local_hostname_hex()
        );
        let lock_path = format!("{target_path}.lock");

        let mut replace_lock = |_: &str| {
            std::fs::rename(&lock_path, format!("{target_path}.previous-lock")).unwrap();
            std::fs::create_dir_all(format!("{lock_path}/{marker}")).unwrap();
            Ok("replacement".to_string())
        };
        let result = update_with(&loopback_files(), &target_path, host, &mut replace_lock);

        assert!(result.is_err(), "{host:?}");
        assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "saved");
        assert_eq!(sorted_entries(Path::new(&lock_path)), [marker]);
    }
}

#[test]
fn recovers_a_proven_dead_owner_and_preserves_saved_contents() {
    for host in [None, FIXTURE_HOST] {
        let repo = tempfile::tempdir().unwrap();
        let target_path = saved_target(repo.path());
        let dead_owner = format!(
            "owner-{}-dead-{}",
            exited_process_id(),
            local_hostname_hex()
        );
        std::fs::create_dir_all(format!("{target_path}.lock/{dead_owner}")).unwrap();

        let mut append = |content: &str| Ok(format!("{content} next"));
        update_with(&loopback_files(), &target_path, host, &mut append).unwrap();

        assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "saved next");
        assert_eq!(sorted_entries(repo.path()), ["targets.json"]);
    }
}

#[test]
fn does_not_reclaim_an_owner_from_another_host_by_its_pid() {
    for host in [None, FIXTURE_HOST] {
        let repo = tempfile::tempdir().unwrap();
        let target_path = saved_target(repo.path());
        let marker = format!(
            "owner-{}-dead-{}",
            exited_process_id(),
            hex::encode("another-host")
        );
        std::fs::create_dir_all(format!("{target_path}.lock/{marker}")).unwrap();
        let mut is_called = false;

        let mut update = |_: &str| {
            is_called = true;
            Ok("replacement".to_string())
        };
        let error = update_with(&loopback_files(), &target_path, host, &mut update).unwrap_err();

        assert!(
            error.to_string().contains(&format!("{target_path}.lock")),
            "{error}"
        );
        assert!(!is_called);
        assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "saved");
        assert_eq!(
            sorted_entries(Path::new(&format!("{target_path}.lock"))),
            [marker]
        );
    }
}

#[test]
fn preserves_an_ownerless_lock_and_reports_its_path() {
    for host in [None, FIXTURE_HOST] {
        let repo = tempfile::tempdir().unwrap();
        let target_path = saved_target(repo.path());
        let lock_path = format!("{target_path}.lock");
        std::fs::create_dir(&lock_path).unwrap();
        let mut is_called = false;

        let mut update = |_: &str| {
            is_called = true;
            Ok("replacement".to_string())
        };
        let error = update_with(&loopback_files(), &target_path, host, &mut update).unwrap_err();

        assert!(error.to_string().contains(&lock_path), "{error}");
        assert!(!is_called);
        assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "saved");
        assert!(sorted_entries(Path::new(&lock_path)).is_empty());
    }
}

#[test]
fn keeps_an_active_owner_until_its_callback_finishes() {
    for host in [None, FIXTURE_HOST] {
        let repo = tempfile::tempdir().unwrap();
        let target_path = text(&repo.path().join("targets.json"));
        let files = loopback_files();
        let (entered_sender, entered_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel::<()>();
        let following_calls = AtomicUsize::new(0);

        std::thread::scope(|scope| {
            let (files, target_path) = (&files, &target_path);
            scope.spawn(move || {
                let mut hold = |content: &str| {
                    entered_sender.send(()).unwrap();
                    release_receiver.recv().unwrap();
                    Ok(format!("{content}a"))
                };
                update_with(files, target_path, host, &mut hold).unwrap();
            });
            entered_receiver.recv().unwrap();
            let following_calls = &following_calls;
            scope.spawn(move || {
                let mut follow = |content: &str| {
                    following_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(format!("{content}b"))
                };
                update_with(files, target_path, None, &mut follow).unwrap();
            });
            std::thread::sleep(Duration::from_millis(300));
            assert_eq!(following_calls.load(Ordering::SeqCst), 0);
            release_sender.send(()).unwrap();
        });

        assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "ab");
        assert_eq!(following_calls.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn releases_the_lock_after_update_failure_without_replacing_saved_contents() {
    for host in [None, FIXTURE_HOST] {
        let repo = tempfile::tempdir().unwrap();
        let target_path = saved_target(repo.path());
        let files = loopback_files();

        let mut fail = |_: &str| Err(HostFileError::Message("update failed".into()));
        let error = update_with(&files, &target_path, host, &mut fail).unwrap_err();
        assert_eq!(error.to_string(), "update failed");
        assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "saved");

        let mut append = |content: &str| Ok(format!("{content} next"));
        update_with(&files, &target_path, host, &mut append).unwrap();
        assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "saved next");
        assert_eq!(sorted_entries(repo.path()), ["targets.json"]);
    }
}

#[test]
fn does_not_interpret_a_read_failure_as_an_absent_file() {
    for host in [None, FIXTURE_HOST] {
        let repo = tempfile::tempdir().unwrap();
        let target_path = repo.path().join("targets.json");
        std::fs::create_dir(&target_path).unwrap();
        let mut is_called = false;

        let mut update = |_: &str| {
            is_called = true;
            Ok("replacement".to_string())
        };
        let result = update_with(&loopback_files(), &text(&target_path), host, &mut update);

        assert!(result.is_err());
        assert!(!is_called);
        assert_eq!(sorted_entries(repo.path()), ["targets.json"]);
    }
}

fn observe_complete_json(target_path: &Path, is_completed: &AtomicBool) -> usize {
    let mut reads = 0;
    while !is_completed.load(Ordering::SeqCst) {
        let content = std::fs::read_to_string(target_path).unwrap();
        let state: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(state["schemaVersion"], 1);
        reads += 1;
    }
    reads
}

#[test]
fn publishes_only_complete_json_while_readers_run_during_writes() {
    for host in [None, FIXTURE_HOST] {
        let repo = tempfile::tempdir().unwrap();
        let repo_path = text(repo.path());
        let target_path = repo.path().join(".kanvibe/targets.json");
        let files = loopback_files();
        let initial = target("http://initial".into(), "initial".into());
        upsert_kanvibe_hook_target(&files, &repo_path, &initial, host).unwrap();
        let is_completed = AtomicBool::new(false);

        let reads = std::thread::scope(|scope| {
            let observer = scope.spawn(|| observe_complete_json(&target_path, &is_completed));
            let writers: Vec<_> = (0..4)
                .map(|index| {
                    let (files, repo_path) = (&files, &repo_path);
                    scope.spawn(move || {
                        let writer_target =
                            target(format!("http://client-{index}"), "x".repeat(100_000));
                        upsert_kanvibe_hook_target(files, repo_path, &writer_target, host).unwrap();
                    })
                })
                .collect();
            writers
                .into_iter()
                .for_each(|writer| writer.join().unwrap());
            is_completed.store(true, Ordering::SeqCst);
            observer.join().unwrap()
        });

        assert!(reads > 0);
        assert_eq!(read_targets(repo.path()).len(), 5);
    }
}

#[test]
fn preserves_concurrent_color_and_priority_with_local_remote_and_mixed_access() {
    for mode in ["local", "remote", "mixed"] {
        for priority in [Some(TaskPriority::High), None] {
            let repo = tempfile::Builder::new()
                .prefix("kanvibe-project-'")
                .tempdir()
                .unwrap();
            let repo_path = text(repo.path());
            let files = loopback_files();
            let color_host = (mode != "local").then_some("fixture-host");
            let priority_host = (mode == "remote").then_some("fixture-host");
            write_kanvibe_project_color(&files, &repo_path, "#111111", color_host).unwrap();
            write_kanvibe_project_priority(
                &files,
                &repo_path,
                Some(TaskPriority::Low),
                priority_host,
            )
            .unwrap();

            std::thread::scope(|scope| {
                scope.spawn(|| {
                    write_kanvibe_project_color(&files, &repo_path, "#222222", color_host).unwrap()
                });
                scope.spawn(|| {
                    write_kanvibe_project_priority(&files, &repo_path, priority, priority_host)
                        .unwrap()
                });
            });

            let content =
                std::fs::read_to_string(repo.path().join(".kanvibe/project.json")).unwrap();
            let state: Value = serde_json::from_str(&content).unwrap();
            assert_eq!(state["projectColor"], "#222222");
            assert_eq!(state["priority"], serde_json::to_value(priority).unwrap());
            assert_eq!(
                sorted_entries(&repo.path().join(".kanvibe")),
                ["project.json"]
            );
        }
    }
}

struct SymlinkFixture {
    workspace: tempfile::TempDir,
}

impl SymlinkFixture {
    fn new() -> Self {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::create_dir(workspace.path().join("repo")).unwrap();
        std::fs::create_dir(workspace.path().join("outside")).unwrap();
        Self { workspace }
    }

    fn repo(&self) -> std::path::PathBuf {
        self.workspace.path().join("repo")
    }

    fn outside(&self) -> std::path::PathBuf {
        self.workspace.path().join("outside")
    }
}

#[test]
fn state_paths_without_links_pass() {
    let fixture = SymlinkFixture::new();
    std::fs::create_dir(fixture.repo().join(".kanvibe")).unwrap();

    for host in [None, Some("remote-host")] {
        assert_kanvibe_state_paths_are_not_symlinks(
            &loopback_files(),
            &text(&fixture.repo()),
            &["task.sh"],
            host,
        )
        .unwrap();
    }
}

#[test]
fn a_linked_task_script_is_refused() {
    let fixture = SymlinkFixture::new();
    std::fs::create_dir(fixture.repo().join(".kanvibe")).unwrap();
    std::fs::write(fixture.outside().join("victim"), "keep").unwrap();
    std::os::unix::fs::symlink(
        fixture.outside().join("victim"),
        fixture.repo().join(".kanvibe/task.sh"),
    )
    .unwrap();

    for host in [None, Some("remote-host")] {
        let error = assert_kanvibe_state_paths_are_not_symlinks(
            &loopback_files(),
            &text(&fixture.repo()),
            &["task.sh"],
            host,
        )
        .unwrap_err();
        assert!(error.to_string().contains("심볼릭 링크"), "{error}");
    }
}

#[test]
fn a_linked_state_directory_never_receives_the_token() {
    for host in [None, Some("remote-host")] {
        let fixture = SymlinkFixture::new();
        std::os::unix::fs::symlink(fixture.outside(), fixture.repo().join(".kanvibe")).unwrap();
        let token_target = KanvibeHookTarget {
            token: Some("secret".into()),
            ..target("http://10.0.0.2:9736".into(), "task".into())
        };

        let error = upsert_kanvibe_hook_target(
            &loopback_files(),
            &text(&fixture.repo()),
            &token_target,
            host,
        )
        .unwrap_err();

        assert!(error.to_string().contains("심볼릭 링크"), "{error}");
        assert!(!fixture.outside().join("targets.json").exists());
    }
}
