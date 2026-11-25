//! TS `kanvibeHooksInstaller.test.ts`의 "conditional migration" describe.
//! provider 상태는 진짜 판정을 쓰고, 원격 파일은 메모리 Map, 원격 명령(execGit)은 기록하는 가짜 실행기로 바꾼다.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::installer_fakes::*;
use super::providers::{HookFileSnapshot, KanvibeHookProvider};
use crate::exec::ExecGitError;
use crate::host_file_access::TextFileReadResult;

const REPO_PATH: &str = "/remote/repo";
const TARGETS_PATH: &str = "/remote/repo/.kanvibe/targets.json";
const CLI_PATH: &str = "/remote/repo/.kanvibe/task.sh";
const HOOK_WRITE_BATCH_COUNT: usize = 5;
const TRANSPORT_MARKER: &str = "KANVIBE_HOOK_TRANSPORT_VERSION=4";

type SharedFiles = Arc<Mutex<HookFileSnapshot>>;

struct MigrationFixture {
    fixture: InstallerFixture,
    files: SharedFiles,
}

fn read_from(files: &SharedFiles, paths: &[String]) -> HookFileSnapshot {
    let files = files.lock().unwrap();
    paths
        .iter()
        .map(|path| {
            let file = files.get(path).cloned().unwrap_or(TextFileReadResult {
                exists: false,
                content: String::new(),
            });
            (path.clone(), file)
        })
        .collect()
}

fn write_into(files: &SharedFiles, written: &[super::HookProviderFile]) {
    let mut files = files.lock().unwrap();
    for file in written {
        files.insert(
            file.file_path.clone(),
            TextFileReadResult {
                exists: true,
                content: file.content.clone(),
            },
        );
    }
}

/// TS mock처럼 다른 client 항목을 남기고 현재 client(ssh 전송) 항목을 다시 쓴다
fn register_own_target(files: &SharedFiles) {
    let mut files = files.lock().unwrap();
    let existing = files
        .get(TARGETS_PATH)
        .map_or(r#"{"targets":[]}"#.to_string(), |file| file.content.clone());
    let existing: Value = serde_json::from_str(&existing).unwrap();
    let mut targets: Vec<Value> = existing["targets"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|target| {
            target["clientId"] != "own-client" && target["url"] != "http://127.0.0.1:9736"
        })
        .cloned()
        .collect();
    targets.push(json!({ "url": "http://127.0.0.1:9736", "taskId": "task-1", "token": "token", "clientId": "own-client", "transport": "ssh" }));
    let content = json!({ "targets": targets }).to_string();
    files.insert(
        TARGETS_PATH.into(),
        TextFileReadResult {
            exists: true,
            content,
        },
    );
}

impl MigrationFixture {
    /// TS `beforeEach`: 진짜 provider 상태로 원격 설치를 한 번 끝낸 뒤 호출 기록을 지운다
    fn new() -> Self {
        let fixture = InstallerFixture::new("http://127.0.0.1:9736");
        for provider in fixture.providers() {
            provider.use_real_status();
        }
        let files: SharedFiles = Arc::new(Mutex::new(HookFileSnapshot::new()));
        let read_files = Arc::clone(&files);
        fixture
            .operations
            .read_always(move |paths| Ok(read_from(&read_files, paths)));
        let register_files = Arc::clone(&files);
        fixture.operations.register_always(move || {
            register_own_target(&register_files);
            Ok(())
        });
        let written_files = Arc::clone(&files);
        fixture.operations.write_always(move |written| {
            write_into(&written_files, written);
            Ok(())
        });
        fixture
            .installer
            .install_hooks(REPO_PATH, "task-1", Some("remote-host"))
            .unwrap();
        fixture.operations.clear_calls();
        for provider in fixture.providers() {
            provider.clear_calls();
        }
        Self { fixture, files }
    }

    fn ensure(
        &self,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<bool, super::client_runtime::HookInstallError> {
        self.fixture
            .installer
            .ensure_hooks_current(REPO_PATH, task_id, ssh_host)
    }

    fn file(&self, path: &str) -> Option<TextFileReadResult> {
        self.files.lock().unwrap().get(path).cloned()
    }

    fn set_file(&self, path: &str, content: &str) {
        let file = TextFileReadResult {
            exists: true,
            content: content.to_string(),
        };
        self.files.lock().unwrap().insert(path.to_string(), file);
    }

    fn remove_file(&self, path: &str) {
        self.files.lock().unwrap().remove(path);
    }

    fn targets(&self) -> Vec<Value> {
        let content = self.file(TARGETS_PATH).unwrap().content;
        serde_json::from_str::<Value>(&content).unwrap()["targets"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn set_targets(&self, targets: Vec<Value>) {
        self.set_file(TARGETS_PATH, &json!({ "targets": targets }).to_string());
    }

    fn writes(&self) -> usize {
        self.fixture.operations.write_count()
    }

    fn command_hosts(&self) -> Vec<Option<String>> {
        self.fixture
            .runner
            .calls()
            .into_iter()
            .map(|call| call.ssh_host)
            .collect()
    }
}

fn own_peer(own: &Value) -> Value {
    let mut peer = own.clone();
    peer["clientId"] = json!("peer-client");
    peer["url"] = json!("http://127.0.0.1:9737");
    peer
}

// parity: TS-lib-kanvibeHooksInstaller-a045f8
#[test]
fn current_provider_and_cli_are_not_rewritten_but_file_changes_are_rechecked() {
    let migration = MigrationFixture::new();
    migration.ensure("task-1", Some("remote-host")).unwrap();
    migration.ensure("task-1", Some("remote-host")).unwrap();
    assert_eq!(migration.writes(), 0);

    migration.set_file(CLI_PATH, "#!/bin/sh\necho legacy");
    migration.ensure("task-1", Some("remote-host")).unwrap();

    assert_eq!(migration.writes(), HOOK_WRITE_BATCH_COUNT);
    assert!(
        migration
            .file(CLI_PATH)
            .unwrap()
            .content
            .contains(TRANSPORT_MARKER)
    );
}

// parity: TS-lib-kanvibeHooksInstaller-2d2d9c
#[test]
fn unchanged_remote_hooks_read_one_bundle_and_skip_provider_rechecks() {
    let migration = MigrationFixture::new();
    migration.ensure("task-1", Some("remote-host")).unwrap();
    migration.fixture.operations.clear_calls();

    assert!(!migration.ensure("task-1", Some("remote-host")).unwrap());

    let read_calls = migration
        .fixture
        .operations
        .read_calls
        .lock()
        .unwrap()
        .clone();
    assert_eq!(read_calls.len(), 1);
    let known_paths: Vec<String> = migration.files.lock().unwrap().keys().cloned().collect();
    for path in known_paths {
        assert!(read_calls[0].paths.contains(&path), "{path}");
    }
    assert_eq!(migration.writes(), 0);
}

// parity: TS-lib-kanvibeHooksInstaller-8de4c3
#[test]
fn a_deleted_provider_file_is_found_and_repaired_even_after_a_successful_check() {
    let migration = MigrationFixture::new();
    migration.ensure("task-1", Some("remote-host")).unwrap();
    let script_path = "/remote/repo/.claude/hooks/kanvibe-prompt-hook.sh";
    migration.remove_file(script_path);

    assert!(migration.ensure("task-1", Some("remote-host")).unwrap());

    assert!(
        migration
            .file(script_path)
            .unwrap()
            .content
            .contains(TRANSPORT_MARKER)
    );
    assert_eq!(migration.writes(), HOOK_WRITE_BATCH_COUNT);
}

// parity: TS-lib-kanvibeHooksInstaller-cf5489
#[test]
fn a_missing_open_code_pane_chat_binding_is_reinstalled_instead_of_failing() {
    let migration = MigrationFixture::new();
    let observer_path = "/remote/repo/.opencode/kanvibe-tui.mjs";
    migration.remove_file(observer_path);

    assert!(migration.ensure("task-1", Some("remote-host")).unwrap());

    assert!(
        migration
            .file(observer_path)
            .unwrap()
            .content
            .contains("kanvibe-pane-chat")
    );
}

// parity: TS-lib-kanvibeHooksInstaller-f88d76
#[test]
fn a_verification_does_not_skip_checks_for_another_remote_host() {
    let migration = MigrationFixture::new();
    migration.ensure("task-1", Some("remote-host")).unwrap();
    let commands_before = migration.command_hosts().len();

    assert!(!migration.ensure("task-1", Some("other-host")).unwrap());

    let new_hosts = migration.command_hosts().split_off(commands_before);
    assert!(new_hosts.contains(&Some("other-host".to_string())));
}

// parity: TS-lib-kanvibeHooksInstaller-04a967
#[test]
fn a_current_read_failure_is_not_reported_as_success_after_a_previous_check() {
    let migration = MigrationFixture::new();
    migration.ensure("task-1", Some("remote-host")).unwrap();
    migration
        .fixture
        .operations
        .read_once(|_| Err(message_error("SSH read failed")));

    let error = migration.ensure("task-1", Some("remote-host")).unwrap_err();

    assert!(error.to_string().contains("SSH read failed"));
    assert_eq!(migration.writes(), 0);
}

// parity: TS-lib-kanvibeHooksInstaller-97f036
#[test]
fn files_changed_during_a_check_are_not_remembered_as_verified() {
    let migration = MigrationFixture::new();
    let script_path = "/remote/repo/.claude/hooks/kanvibe-prompt-hook.sh";
    let read_files = Arc::clone(&migration.files);
    let is_first_snapshot = Arc::new(Mutex::new(true));
    migration.fixture.operations.read_always(move |paths| {
        let mut files = read_from(&read_files, paths);
        let mut is_first = is_first_snapshot.lock().unwrap();
        if *is_first && paths.contains(&CLI_PATH.to_string()) {
            *is_first = false;
            files.insert(
                script_path.into(),
                TextFileReadResult {
                    exists: false,
                    content: String::new(),
                },
            );
        }
        Ok(files)
    });
    migration.ensure("task-1", Some("remote-host")).unwrap();
    migration.remove_file(script_path);

    assert!(migration.ensure("task-1", Some("remote-host")).unwrap());

    assert!(
        migration
            .file(script_path)
            .unwrap()
            .content
            .contains(TRANSPORT_MARKER)
    );
}

// parity: TS-lib-kanvibeHooksInstaller-bc47f0
#[test]
fn a_lost_current_client_registration_is_restored_while_keeping_peers() {
    let migration = MigrationFixture::new();
    migration.ensure("task-1", Some("remote-host")).unwrap();
    let peer = own_peer(&migration.targets()[0]);
    migration.set_targets(vec![peer.clone()]);

    assert!(migration.ensure("task-1", Some("remote-host")).unwrap());

    assert!(migration.targets().contains(&peer));
    assert_eq!(migration.fixture.operations.register_count(), 1);
}

// parity: TS-lib-kanvibeHooksInstaller-b13e38
#[test]
fn a_changed_client_id_does_not_reuse_the_previous_clients_verification() {
    let migration = MigrationFixture::new();
    migration.ensure("task-1", Some("remote-host")).unwrap();
    *migration.fixture.runtime.client_id.lock().unwrap() = "new-client".into();

    let error = migration.ensure("task-1", Some("remote-host")).unwrap_err();

    assert!(error.to_string().contains("현재 client 등록이 없습니다"));
    assert_eq!(migration.fixture.operations.register_count(), 1);
}

// parity: TS-lib-kanvibeHooksInstaller-d30cd0
#[test]
fn another_tasks_missing_registration_is_not_hidden_by_a_previous_tasks_check() {
    let migration = MigrationFixture::new();
    migration.ensure("task-1", Some("remote-host")).unwrap();

    let error = migration.ensure("task-2", Some("remote-host")).unwrap_err();

    assert!(error.to_string().contains("hasRegisteredHookTarget"));
    assert!(migration.fixture.operations.register_count() > 0);
}

// parity: TS-lib-kanvibeHooksInstaller-e1041c
#[test]
fn a_changed_hook_port_rechecks_provider_status_for_identical_files() {
    let migration = MigrationFixture::new();
    migration.ensure("task-1", Some("remote-host")).unwrap();
    let commands_before = migration.command_hosts().len();
    migration
        .fixture
        .runtime
        .hook_server_port
        .store(19736, Ordering::SeqCst);

    assert!(!migration.ensure("task-1", Some("remote-host")).unwrap());

    assert!(migration.command_hosts().len() > commands_before);
}

// parity: TS-lib-kanvibeHooksInstaller-ed34c1
#[test]
fn local_provider_status_is_rechecked_every_time() {
    let migration = MigrationFixture::new();
    let mut target = migration.targets()[0].clone();
    target["transport"] = json!("local");
    migration.set_targets(vec![target]);
    for provider in migration.fixture.providers() {
        provider.status_always(Ok(installed_status(provider_kind(provider))));
    }

    migration.ensure("task-1", None).unwrap();
    migration.ensure("task-1", None).unwrap();

    assert_eq!(migration.fixture.open_code.status_count(), 2);
    assert_eq!(migration.writes(), 0);
}

fn provider_kind(provider: &FakeProvider) -> KanvibeHookProvider {
    use super::providers::KanvibeHookProviderModule;
    provider.provider()
}

// parity: TS-lib-kanvibeHooksInstaller-b7ccfc
#[test]
fn old_provider_transports_are_updated_to_the_latest() {
    for relative_path in [
        ".claude/hooks/kanvibe-prompt-hook.sh",
        ".gemini/hooks/kanvibe-stop-hook.sh",
        ".codex/hooks/kanvibe-permission-hook.sh",
        ".opencode/plugins/kanvibe-plugin.ts",
    ] {
        let migration = MigrationFixture::new();
        let file_path = format!("{REPO_PATH}/{relative_path}");
        let content = migration
            .file(&file_path)
            .unwrap()
            .content
            .replace(TRANSPORT_MARKER, "KANVIBE_HOOK_TRANSPORT_VERSION=1");
        migration.set_file(&file_path, &content);

        migration.ensure("task-1", Some("remote-host")).unwrap();

        assert_eq!(
            migration.writes(),
            HOOK_WRITE_BATCH_COUNT,
            "{relative_path}"
        );
        assert!(
            migration
                .file(&file_path)
                .unwrap()
                .content
                .contains(TRANSPORT_MARKER),
            "{relative_path}"
        );
    }
}

// parity: TS-lib-kanvibeHooksInstaller-30b63f
#[test]
fn current_client_registration_is_repaired_while_keeping_peers() {
    for scenario in ["missing", "wrong-transport", "other-client"] {
        let migration = MigrationFixture::new();
        let own = migration.targets()[0].clone();
        let peer = own_peer(&own);
        let mut changed = own.clone();
        match scenario {
            "wrong-transport" => changed["transport"] = json!("local"),
            _ => changed["clientId"] = json!("other-client"),
        }
        let targets = if scenario == "missing" {
            vec![peer.clone()]
        } else {
            vec![peer.clone(), changed]
        };
        migration.set_targets(targets);

        migration.ensure("task-1", Some("remote-host")).unwrap();

        assert_eq!(
            migration.fixture.operations.register_count(),
            1,
            "{scenario}"
        );
        assert!(migration.targets().contains(&peer), "{scenario}");
    }
}

// parity: TS-lib-kanvibeHooksInstaller-202bec
#[test]
fn matching_remote_client_and_transport_are_not_reinstalled_for_an_old_ip() {
    let migration = MigrationFixture::new();
    let mut targets = migration.targets();
    targets[0]["url"] = json!("http://192.0.2.99:9736");
    migration.set_targets(targets);

    assert!(!migration.ensure("task-1", Some("remote-host")).unwrap());

    assert_eq!(migration.writes(), 0);
}

// parity: TS-lib-kanvibeHooksInstaller-00e90a
#[test]
fn a_write_that_skips_the_cli_returns_a_verification_failure() {
    let migration = MigrationFixture::new();
    migration.remove_file(CLI_PATH);
    let written_files = Arc::clone(&migration.files);
    migration.fixture.operations.write_always(move |written| {
        let without_cli: Vec<_> = written
            .iter()
            .filter(|file| file.file_path != CLI_PATH)
            .cloned()
            .collect();
        write_into(&written_files, &without_cli);
        Ok(())
    });

    let error = migration.ensure("task-1", Some("remote-host")).unwrap_err();

    assert!(error.to_string().contains("task.sh"));
}

// parity: TS-lib-kanvibeHooksInstaller-6f55de
#[test]
fn an_unreachable_tunnel_alone_does_not_rewrite_current_files() {
    let migration = MigrationFixture::new();
    *migration.fixture.command_result.lock().unwrap() = Err(ExecGitError::new("SSH disconnected"));

    migration.ensure("task-1", Some("remote-host")).unwrap();

    assert_eq!(migration.writes(), 0);
}

// parity: TS-lib-kanvibeHooksInstaller-e19803
#[test]
fn concurrent_connections_for_the_same_task_share_the_check_and_install() {
    let migration = MigrationFixture::new();
    migration.remove_file(CLI_PATH);

    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..5)
            .map(|_| scope.spawn(|| migration.ensure("task-1", Some("remote-host"))))
            .collect();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }
    });

    assert_eq!(migration.fixture.operations.register_count(), 1);
    assert_eq!(migration.writes(), HOOK_WRITE_BATCH_COUNT);
}

// parity: TS-lib-kanvibeHooksInstaller-660674
#[test]
fn a_failed_check_does_not_write_and_is_retried_on_the_next_connection() {
    let migration = MigrationFixture::new();
    migration
        .fixture
        .operations
        .read_once(|_| Err(message_error("SSH read failed")));

    let error = migration.ensure("task-1", Some("remote-host")).unwrap_err();

    assert!(error.to_string().contains("SSH read failed"));
    assert_eq!(migration.writes(), 0);
    assert!(migration.ensure("task-1", Some("remote-host")).is_ok());
}

// parity: TS-lib-kanvibeHooksInstaller-28bc8e
#[test]
fn a_failed_write_stays_failed_and_is_retried_on_the_next_connection() {
    let migration = MigrationFixture::new();
    migration.remove_file(CLI_PATH);
    migration
        .fixture
        .operations
        .write_always(|_| Err(message_error("write denied")));

    let error = migration.ensure("task-1", Some("remote-host")).unwrap_err();
    assert!(error.to_string().contains("write denied"));
    let written_files = Arc::clone(&migration.files);
    migration.fixture.operations.write_always(move |written| {
        write_into(&written_files, written);
        Ok(())
    });

    assert!(migration.ensure("task-1", Some("remote-host")).is_ok());
    assert!(
        migration
            .file(CLI_PATH)
            .unwrap()
            .content
            .contains(TRANSPORT_MARKER)
    );
}
