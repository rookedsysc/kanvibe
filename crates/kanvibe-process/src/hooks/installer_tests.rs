//! TS `kanvibeHooksInstaller.test.ts`의 첫 describe. provider setup·getStatus와 파일 읽기·쓰기·등록은 가짜로 바꾼다.
//! TS 가짜 타이머 자리는 재시도 간격 0과 예약 스레드 join, 바꿔 끼운 시계다.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use regex::Regex;
use serde_json::{Value, json};

use super::installer::HookInstallScheduleOptions;
use super::installer_fakes::*;
use super::installer_verification::HookLogLevel;
use super::providers::{KanvibeHookProvider, KanvibeHookStatus};

const HOOK_WRITE_BATCH_COUNT: usize = 5;
const DEFAULT_URL: &str = "http://192.168.0.8:9736";

fn fixture() -> InstallerFixture {
    InstallerFixture::new(DEFAULT_URL)
}

/// 성공·실패 callback이 불린 횟수와 실패 메시지
#[derive(Default)]
struct CallbackRecord {
    successes: AtomicUsize,
    failures: Mutex<Vec<String>>,
}

fn record_callbacks(record: &Arc<CallbackRecord>, delay: Duration) -> HookInstallScheduleOptions {
    let on_success = Arc::clone(record);
    let on_failure = Arc::clone(record);
    HookInstallScheduleOptions {
        delay,
        on_success: Some(Box::new(move || {
            on_success.successes.fetch_add(1, Ordering::SeqCst);
        })),
        on_failure: Some(Box::new(move |error| {
            on_failure.failures.lock().unwrap().push(error.to_string())
        })),
    }
}

fn written_paths(fixture: &InstallerFixture) -> Vec<String> {
    fixture
        .operations
        .last_install_written_files()
        .into_iter()
        .map(|file| file.file_path)
        .collect()
}

#[test]
fn invalidates_open_code_registration_after_a_bulk_file_installation() {
    let fixture = fixture();

    fixture
        .installer
        .install_hook_files("/repo", "task-1", None)
        .unwrap();

    assert!(
        fixture
            .registration
            .invalidated_paths
            .lock()
            .unwrap()
            .contains(&"/repo".to_string())
    );
}

#[test]
fn bulk_remote_status_passes_one_strict_snapshot_to_all_providers_without_writing() {
    let fixture = fixture();
    let snapshot_files = snapshot(&[("/remote/repo/.kanvibe/targets.json", r#"{"targets":[]}"#)]);
    let returned = snapshot_files.clone();
    fixture
        .operations
        .read_always(move |_| Ok(returned.clone()));
    let codex_status = status_with(KanvibeHookProvider::Codex, false, &["hasConfigEntry"]);
    fixture.codex.status_always(Ok(codex_status.clone()));

    let statuses = fixture
        .installer
        .hooks_statuses("/remote/repo", "task-1", Some("remote-host"))
        .unwrap();

    assert_eq!(statuses.codex, codex_status);
    let read_calls = fixture.operations.read_calls.lock().unwrap().clone();
    assert_eq!(read_calls.len(), 1);
    for path in [
        "/remote/repo/.claude/settings.json",
        "/remote/repo/.gemini/settings.json",
        "/remote/repo/.codex/config.toml",
        "/remote/repo/.kanvibe/targets.json",
    ] {
        assert!(read_calls[0].paths.contains(&path.to_string()), "{path}");
    }
    assert_eq!(read_calls[0].ssh_host.as_deref(), Some("remote-host"));
    for provider in fixture.providers() {
        let call = provider.status_calls.lock().unwrap()[0].clone();
        assert_eq!(call.repo_path, "/remote/repo");
        assert_eq!(call.task_id.as_deref(), Some("task-1"));
        assert_eq!(call.ssh_host.as_deref(), Some("remote-host"));
        assert_eq!(call.files.as_ref(), Some(&snapshot_files));
    }
    assert_eq!(fixture.operations.register_count(), 0);
    assert_eq!(fixture.operations.write_count(), 0);
    assert_eq!(fixture.runtime.client_id_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn bulk_remote_status_snapshot_failure_is_returned_without_installing() {
    let fixture = fixture();
    fixture
        .operations
        .read_always(|_| Err(message_error("SSH read failed")));

    let error = fixture
        .installer
        .hooks_statuses("/remote/repo", "task-1", Some("remote-host"))
        .unwrap_err();

    assert!(error.to_string().contains("SSH read failed"));
    assert_eq!(fixture.claude.status_count(), 0);
    assert_eq!(fixture.operations.write_count(), 0);
}

#[test]
fn full_install_writes_the_worktree_task_cli_as_executable() {
    let fixture = fixture();

    fixture
        .installer
        .install_hooks("/repo", "task-1", Some("remote-host"))
        .unwrap();

    let task_cli_file = fixture
        .operations
        .last_install_written_files()
        .into_iter()
        .find(|file| file.file_path == "/repo/.kanvibe/task.sh")
        .unwrap();
    assert_eq!(task_cli_file.mode, Some(0o755));
    assert!(task_cli_file.content.contains("/api/cli/tasks"));
    let write_calls = fixture.operations.write_calls.lock().unwrap().clone();
    assert!(
        write_calls
            .iter()
            .any(|call| call.files == [task_cli_file.clone()]
                && call.ssh_host.as_deref() == Some("remote-host"))
    );
}

#[test]
fn full_install_writes_every_hook_file_in_provider_batches() {
    let fixture = fixture();

    fixture
        .installer
        .install_hooks("/repo", "task-1", None)
        .unwrap();

    assert_eq!(fixture.operations.write_count(), HOOK_WRITE_BATCH_COUNT);
    assert!(
        fixture
            .operations
            .write_calls
            .lock()
            .unwrap()
            .iter()
            .all(|call| call.ssh_host.is_none())
    );
    let paths = written_paths(&fixture);
    for path in [
        "/repo/.claude/hooks/kanvibe-prompt-hook.sh",
        "/repo/.claude/settings.json",
        "/repo/.gemini/hooks/kanvibe-stop-hook.sh",
        "/repo/.gemini/settings.json",
        "/repo/.codex/hooks/kanvibe-permission-hook.sh",
        "/repo/.codex/hooks.json",
        "/repo/.codex/config.toml",
        "/repo/.opencode/plugins/kanvibe-plugin.ts",
    ] {
        assert!(paths.contains(&path.to_string()), "{path}");
    }
}

#[test]
fn install_registers_the_current_client_before_writing() {
    let fixture = fixture();

    fixture
        .installer
        .install_hooks("/repo", "task-1", None)
        .unwrap();

    assert_eq!(
        fixture.operations.register_calls.lock().unwrap()[0],
        (
            "/repo".to_string(),
            "task-1".to_string(),
            DEFAULT_URL.to_string(),
            None
        )
    );
    assert_eq!(fixture.operations.events.lock().unwrap()[0], "register");
}

#[test]
fn remote_install_uses_the_same_path_and_passes_the_ssh_host_to_writes() {
    let fixture = fixture();

    fixture
        .installer
        .install_hooks("/remote/repo", "task-2", Some("remote-host"))
        .unwrap();

    assert!(
        fixture
            .runtime
            .hook_server_url_calls
            .lock()
            .unwrap()
            .contains(&Some("remote-host".to_string()))
    );
    assert!(
        fixture
            .operations
            .write_calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| call.ssh_host.as_deref() == Some("remote-host"))
    );
    assert!(written_paths(&fixture).contains(&"/remote/repo/.claude/settings.json".to_string()));
}

#[test]
fn existing_settings_are_read_once_over_a_host_key_checked_connection() {
    let fixture = fixture();

    fixture
        .installer
        .install_hooks("/remote/repo", "task-2", Some("remote-host"))
        .unwrap();

    let read_calls = fixture.operations.read_calls.lock().unwrap().clone();
    assert_eq!(read_calls.len(), 2);
    assert_eq!(
        read_calls[0].paths,
        [
            "/remote/repo/.claude/settings.json",
            "/remote/repo/.gemini/settings.json",
            "/remote/repo/.codex/config.toml",
            "/remote/repo/.codex/hooks.json",
            "/remote/repo/.opencode/tui.json",
        ]
    );
    assert_eq!(read_calls[0].ssh_host.as_deref(), Some("remote-host"));
}

#[test]
fn remote_install_verification_shares_one_snapshot_with_every_provider() {
    let fixture = fixture();
    let verification_snapshot =
        snapshot(&[("/remote/repo/.kanvibe/targets.json", r#"{"targets":[]}"#)]);
    let returned = verification_snapshot.clone();
    fixture.operations.read_once(|_| Ok(snapshot(&[])));
    fixture.operations.read_once(move |_| Ok(returned));

    fixture
        .installer
        .install_hooks("/remote/repo", "task-2", Some("remote-host"))
        .unwrap();

    let second_read = fixture.operations.read_calls.lock().unwrap()[1].clone();
    assert!(
        second_read
            .paths
            .contains(&"/remote/repo/.claude/settings.json".to_string())
    );
    assert!(
        second_read
            .paths
            .contains(&"/remote/repo/.kanvibe/targets.json".to_string())
    );
    for provider in fixture.providers() {
        let calls = provider.status_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].ssh_host.as_deref(), Some("remote-host"));
        assert_eq!(calls[0].files.as_ref(), Some(&verification_snapshot));
    }
}

#[test]
fn failed_shared_verification_read_falls_back_to_per_provider_reads() {
    let fixture = fixture();
    fixture.operations.read_once(|_| Ok(snapshot(&[])));
    fixture
        .operations
        .read_once(|_| Err(message_error("원격 파일 검사 실패")));
    fixture
        .codex
        .status_always(Err("codex 설정을 읽을 수 없습니다"));
    fixture
        .installer
        .install_hook_files("/remote/repo", "task-2", Some("remote-host"))
        .unwrap();
    let record = Arc::new(CallbackRecord::default());

    fixture
        .installer
        .schedule_hooks_verification(
            "/remote/repo",
            "task-2",
            Some("remote-host"),
            record_callbacks(&record, Duration::ZERO),
        )
        .join()
        .unwrap();

    for provider in [&fixture.claude, &fixture.codex] {
        let call = provider.status_calls.lock().unwrap()[0].clone();
        assert_eq!(
            (
                call.task_id.as_deref(),
                call.ssh_host.as_deref(),
                call.files
            ),
            (Some("task-2"), Some("remote-host"), None)
        );
    }
    let failures = record.failures.lock().unwrap().clone();
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("Codex"));
    assert!(!failures[0].contains("Claude"));
}

#[test]
fn overlapping_status_reads_share_one_remote_read_and_reuse_it_for_thirty_seconds() {
    let fixture = fixture();
    let gate = Gate::new();
    let read_gate = Arc::clone(&gate);
    fixture.operations.read_once(move |_| {
        read_gate.wait();
        Ok(snapshot(&[]))
    });

    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            fixture
                .installer
                .hooks_statuses("/remote/repo", "task-1", Some("remote-host"))
        });
        wait_until(|| fixture.operations.read_count() == 1);
        let second = scope.spawn(|| {
            fixture
                .installer
                .hooks_statuses("/remote/repo", "task-1", Some("remote-host"))
        });
        let other_task = scope.spawn(|| {
            fixture
                .installer
                .hooks_statuses("/remote/repo", "task-other", Some("remote-host"))
        });
        wait_until(|| fixture.operations.read_count() == 2);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(fixture.operations.read_count(), 2);
        gate.open();
        assert_eq!(
            first.join().unwrap().unwrap(),
            second.join().unwrap().unwrap()
        );
        other_task.join().unwrap().unwrap();
    });
    assert_eq!(fixture.claude.status_count(), 2);

    fixture
        .installer
        .hooks_statuses("/remote/repo", "task-1", Some("remote-host"))
        .unwrap();
    assert_eq!(fixture.operations.read_count(), 2);
    fixture.clock.advance(Duration::from_secs(30));
    fixture
        .installer
        .hooks_statuses("/remote/repo", "task-1", Some("remote-host"))
        .unwrap();
    assert_eq!(fixture.operations.read_count(), 3);
}

#[test]
fn local_hook_statuses_are_read_every_time() {
    let fixture = fixture();

    fixture
        .installer
        .hooks_statuses("/repo", "task-1", None)
        .unwrap();
    fixture
        .installer
        .hooks_statuses("/repo", "task-1", None)
        .unwrap();

    assert_eq!(fixture.operations.read_count(), 2);
}

#[test]
fn installing_forgets_the_remembered_remote_status() {
    let fixture = fixture();
    let not_installed = status_with(KanvibeHookProvider::Claude, false, &[]);
    let returned = not_installed.clone();
    fixture.claude.status_once(move || Ok(returned));
    let before = fixture
        .installer
        .hooks_statuses("/remote/repo", "task-1", Some("remote-host"))
        .unwrap();
    assert_eq!(before.claude, not_installed);

    fixture
        .installer
        .install_hook_files("/remote/repo", "task-1", Some("remote-host"))
        .unwrap();

    let after = fixture
        .installer
        .hooks_statuses("/remote/repo", "task-1", Some("remote-host"))
        .unwrap();
    assert_eq!(after.claude, installed_status(KanvibeHookProvider::Claude));
}

#[test]
fn status_reads_started_before_an_install_are_not_inherited_after_it() {
    let fixture = fixture();
    let gate = Gate::new();
    let read_gate = Arc::clone(&gate);
    fixture.operations.read_once(move |_| {
        read_gate.wait();
        Ok(snapshot(&[]))
    });

    std::thread::scope(|scope| {
        let stale_read = scope.spawn(|| {
            fixture
                .installer
                .hooks_statuses("/remote/repo", "task-1", Some("remote-host"))
        });
        wait_until(|| fixture.operations.read_count() == 1);
        fixture
            .installer
            .install_hook_files("/remote/repo", "task-1", Some("remote-host"))
            .unwrap();
        let read_count_after_install = fixture.operations.read_count();
        let fresh_read = scope.spawn(|| {
            fixture
                .installer
                .hooks_statuses("/remote/repo", "task-1", Some("remote-host"))
        });
        wait_until(|| fixture.operations.read_count() == read_count_after_install + 1);
        gate.open();
        stale_read.join().unwrap().unwrap();
        fresh_read.join().unwrap().unwrap();
    });
}

#[test]
fn install_never_reads_or_writes_the_removed_hooks_targets_file() {
    let fixture = fixture();

    fixture
        .installer
        .install_hooks("/remote/repo", "task-2", Some("remote-host"))
        .unwrap();

    let mut touched = fixture.operations.read_calls.lock().unwrap()[0]
        .paths
        .clone();
    touched.extend(written_paths(&fixture));
    assert!(!touched.join("\n").contains("hooks-targets.json"));
}

fn stale_claude_and_gemini_settings() -> Vec<(String, String)> {
    let claude = json!({ "hooks": {
        "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "\"/tmp/old/.claude/hooks/kanvibe-prompt-hook.sh\"", "timeout": 10 }] }],
        "PreToolUse": [{ "matcher": "AskUserQuestion", "hooks": [{ "type": "command", "command": "\"/tmp/old/.claude/hooks/kanvibe-question-hook.sh\"", "timeout": 10 }] }],
        "PostToolUse": [{ "matcher": "AskUserQuestion", "hooks": [{ "type": "command", "command": "\"/tmp/old/.claude/hooks/kanvibe-prompt-hook.sh\"", "timeout": 10 }] }],
        "Stop": [{ "hooks": [{ "type": "command", "command": "\"/tmp/old/.claude/hooks/kanvibe-stop-hook.sh\"", "timeout": 10 }] }],
    } });
    let gemini = json!({ "hooks": {
        "BeforeAgent": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "\"/tmp/old/.gemini/hooks/kanvibe-prompt-hook.sh\"", "timeout": 10000 }] }],
        "AfterAgent": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "\"/tmp/old/.gemini/hooks/kanvibe-stop-hook.sh\"", "timeout": 10000 }] }],
    } });
    vec![
        (
            "/remote/repo/.claude/settings.json".into(),
            claude.to_string(),
        ),
        (
            "/remote/repo/.gemini/settings.json".into(),
            gemini.to_string(),
        ),
    ]
}

fn first_command(settings: &Value, event: &str) -> String {
    settings["hooks"][event][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn reinstall_overwrites_stale_claude_and_gemini_entries_with_the_current_project_path() {
    let fixture = fixture();
    let existing = stale_claude_and_gemini_settings();
    fixture.operations.read_always(move |_| {
        let entries: Vec<(&str, &str)> = existing
            .iter()
            .map(|(path, content)| (path.as_str(), content.as_str()))
            .collect();
        Ok(snapshot(&entries))
    });

    fixture
        .installer
        .install_hooks("/remote/repo", "task-2", Some("remote-host"))
        .unwrap();

    let claude: Value = serde_json::from_str(
        &fixture
            .operations
            .written_content("/remote/repo/.claude/settings.json"),
    )
    .unwrap();
    for (event, script) in [
        ("UserPromptSubmit", "prompt"),
        ("PreToolUse", "question"),
        ("Stop", "stop"),
    ] {
        assert_eq!(claude["hooks"][event].as_array().unwrap().len(), 1);
        assert_eq!(
            first_command(&claude, event),
            format!("\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/kanvibe-{script}-hook.sh")
        );
    }
    let gemini: Value = serde_json::from_str(
        &fixture
            .operations
            .written_content("/remote/repo/.gemini/settings.json"),
    )
    .unwrap();
    for (event, script) in [("BeforeAgent", "prompt"), ("AfterAgent", "stop")] {
        assert_eq!(gemini["hooks"][event].as_array().unwrap().len(), 1);
        assert_eq!(
            first_command(&gemini, event),
            format!("\"$GEMINI_PROJECT_DIR\"/.gemini/hooks/kanvibe-{script}-hook.sh")
        );
    }
}

#[test]
fn codex_reinstall_preserves_settings_and_updates_hooks_json_and_config_toml() {
    let fixture = fixture();
    let hooks_json = json!({ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "old-stop" }] }] } }).to_string();
    fixture.operations.read_always(move |_| {
        Ok(snapshot(&[
            (
                "/remote/repo/.codex/config.toml",
                "model = \"gpt-5\"\nnotify = [\"other-notify.sh\"]\n",
            ),
            ("/remote/repo/.codex/hooks.json", hooks_json.as_str()),
        ]))
    });

    fixture
        .installer
        .install_hooks("/remote/repo", "task-2", Some("remote-host"))
        .unwrap();

    let config = fixture
        .operations
        .written_content("/remote/repo/.codex/config.toml");
    assert!(config.contains("model = \"gpt-5\""));
    assert!(config.contains("[features]"));
    assert!(Regex::new(r"(?m)^hooks = true$").unwrap().is_match(&config));
    assert!(
        !Regex::new(r"(?m)^codex_hooks\s*=")
            .unwrap()
            .is_match(&config)
    );
    let hooks = fixture
        .operations
        .written_content("/remote/repo/.codex/hooks.json");
    for event in [
        "UserPromptSubmit",
        "PermissionRequest",
        "PreToolUse",
        "Stop",
    ] {
        assert!(hooks.contains(event), "{event}");
    }
}

#[test]
fn hook_file_install_does_not_wait_for_provider_verification() {
    let fixture = fixture();

    fixture
        .installer
        .install_hook_files("/repo", "task-1", None)
        .unwrap();

    assert_eq!(fixture.operations.write_count(), HOOK_WRITE_BATCH_COUNT);
    assert!(
        fixture
            .providers()
            .iter()
            .all(|provider| provider.status_count() == 0)
    );
}

#[test]
fn verification_scheduler_only_checks_provider_status_without_rewriting_files() {
    let fixture = fixture();
    let verification_snapshot =
        snapshot(&[("/remote/repo/.kanvibe/targets.json", r#"{"targets":[]}"#)]);
    let returned = verification_snapshot.clone();
    fixture
        .operations
        .read_always(move |_| Ok(returned.clone()));
    let record = Arc::new(CallbackRecord::default());

    let handle = fixture.installer.schedule_hooks_verification(
        "/remote/repo",
        "task-2",
        Some("remote-host"),
        record_callbacks(&record, Duration::from_millis(200)),
    );
    assert_eq!(fixture.claude.status_count(), 0);
    handle.join().unwrap();

    assert_eq!(fixture.operations.read_count(), 1);
    for provider in fixture.providers() {
        let call = provider.status_calls.lock().unwrap()[0].clone();
        assert_eq!(
            (call.ssh_host.as_deref(), call.files.as_ref()),
            (Some("remote-host"), Some(&verification_snapshot))
        );
    }
    assert_eq!(fixture.operations.write_count(), 0);
    assert_eq!(record.successes.load(Ordering::SeqCst), 1);
    assert!(record.failures.lock().unwrap().is_empty());
}

#[test]
fn verification_scheduler_reports_provider_failures_to_the_failure_callback() {
    let fixture = fixture();
    fixture.codex.status_always(Ok(status_with(
        KanvibeHookProvider::Codex,
        false,
        &["hasConfigEntry"],
    )));
    let record = Arc::new(CallbackRecord::default());

    fixture
        .installer
        .schedule_hooks_verification(
            "/repo",
            "task-1",
            None,
            record_callbacks(&record, Duration::ZERO),
        )
        .join()
        .unwrap();

    assert_eq!(record.successes.load(Ordering::SeqCst), 0);
    let failures = record.failures.lock().unwrap().clone();
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("hooks 검증 실패: Codex(hasConfigEntry)"));
    assert_eq!(fixture.operations.write_count(), 0);
}

#[test]
fn provider_install_runs_only_the_selected_provider_setup() {
    let fixture = fixture();

    fixture
        .installer
        .install_hook_provider("/repo", "task-1", KanvibeHookProvider::Codex, None)
        .unwrap();

    assert_eq!(
        fixture.codex.install_calls.lock().unwrap().clone(),
        [(
            "/repo".to_string(),
            "task-1".to_string(),
            DEFAULT_URL.to_string(),
            None
        )]
    );
    for provider in [&fixture.claude, &fixture.gemini, &fixture.open_code] {
        assert_eq!(provider.install_count(), 0);
    }
    assert_eq!(fixture.operations.write_count(), 0);
}

#[test]
fn a_missing_open_code_registration_alone_does_not_fail_the_full_install() {
    let fixture = fixture();
    fixture.open_code.status_always(Ok(status_with(
        KanvibeHookProvider::OpenCode,
        false,
        &["hasRegisteredPlugin"],
    )));

    fixture
        .installer
        .install_hooks("/repo", "task-1", None)
        .unwrap();

    assert_eq!(fixture.open_code.status_count(), 1);
    let record = fixture
        .log
        .verification_for(HookLogLevel::Warn, "OpenCode")
        .unwrap();
    assert_eq!(record.failed_checks, ["hasRegisteredPlugin"]);
    assert!(!record.is_installed);
    assert_eq!(record.target_path, "/repo");
}

#[test]
fn open_code_only_install_does_not_fail_on_a_missing_registration() {
    let fixture = fixture();
    fixture.open_code.status_always(Ok(status_with(
        KanvibeHookProvider::OpenCode,
        false,
        &["hasRegisteredPlugin"],
    )));

    fixture
        .installer
        .install_hook_provider("/repo", "task-1", KanvibeHookProvider::OpenCode, None)
        .unwrap();

    assert_eq!(fixture.open_code.install_count(), 1);
    assert_eq!(fixture.open_code.status_count(), 1);
}

#[test]
fn open_code_registration_query_failures_do_not_fail_the_install() {
    for registration_error in [
        "Command failed: opencode debug config (timed out)",
        "spawn opencode ENOENT",
    ] {
        let fixture = fixture();
        fixture
            .open_code
            .status_always(Ok(open_code_registration_failure(registration_error, &[])));

        fixture
            .installer
            .install_hook_provider("/repo", "task-1", KanvibeHookProvider::OpenCode, None)
            .unwrap();

        assert_eq!(fixture.open_code.install_count(), 1);
        let record = fixture
            .log
            .verification_for(HookLogLevel::Warn, "OpenCode")
            .unwrap();
        assert_eq!(record.failed_checks, ["hasRegisteredPlugin"]);
        assert_eq!(
            record.plugin_registration_error.as_deref(),
            Some(registration_error)
        );
    }
}

#[test]
fn registration_query_failure_still_propagates_other_missing_checks() {
    let fixture = fixture();
    fixture
        .open_code
        .status_always(Ok(open_code_registration_failure(
            "spawn opencode ENOENT",
            &["hasTaskIdBinding"],
        )));

    let error = fixture
        .installer
        .install_hook_provider("/repo", "task-1", KanvibeHookProvider::OpenCode, None)
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("hooks 검증 실패: OpenCode(hasTaskIdBinding)")
    );
}

#[test]
fn open_code_only_install_propagates_a_plugin_file_verification_failure() {
    let fixture = fixture();
    fixture.open_code.status_always(Ok(status_with(
        KanvibeHookProvider::OpenCode,
        false,
        &["hasPlugin", "hasRegisteredPlugin"],
    )));

    let error = fixture
        .installer
        .install_hook_provider("/repo", "task-1", KanvibeHookProvider::OpenCode, None)
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("hooks 검증 실패: OpenCode(hasPlugin)")
    );
    assert_eq!(fixture.open_code.install_count(), 3);
    assert_eq!(fixture.open_code.status_count(), 3);
}

#[test]
fn provider_install_failure_is_not_retried_with_other_providers() {
    let fixture = fixture();
    fixture.codex.install_fails_with("codex busy");

    let error = fixture
        .installer
        .install_hook_provider("/repo", "task-1", KanvibeHookProvider::Codex, None)
        .unwrap_err();

    assert!(error.to_string().contains("codex busy"));
    assert_eq!(fixture.codex.install_count(), 3);
    for provider in [&fixture.claude, &fixture.gemini, &fixture.open_code] {
        assert_eq!(provider.install_count(), 0);
    }
}

#[test]
fn a_transient_hook_file_write_failure_succeeds_after_a_retry() {
    let fixture = fixture();
    fixture
        .operations
        .write_once(|_| Err(message_error("remote host busy")));

    fixture
        .installer
        .install_hooks("/repo", "task-1", None)
        .unwrap();

    assert_eq!(fixture.operations.write_count(), HOOK_WRITE_BATCH_COUNT * 2);
}

#[test]
fn a_persistent_hook_file_write_failure_propagates_after_retries() {
    let fixture = fixture();
    fixture
        .operations
        .write_always(|_| Err(message_error("remote host unavailable")));

    let error = fixture
        .installer
        .install_hooks("/remote/repo", "task-2", Some("remote-host"))
        .unwrap_err();

    assert!(error.to_string().contains("remote host unavailable"));
    assert_eq!(fixture.operations.write_count(), HOOK_WRITE_BATCH_COUNT * 3);
}

#[test]
fn concurrent_installs_for_the_same_target_share_one_job() {
    let fixture = fixture();
    let gate = Gate::new();
    let write_gate = Arc::clone(&gate);
    fixture.operations.write_always(move |_| {
        write_gate.wait();
        Ok(())
    });

    std::thread::scope(|scope| {
        let first = scope.spawn(|| fixture.installer.install_hooks("/repo", "task-1", None));
        let second = scope.spawn(|| fixture.installer.install_hooks("/repo", "task-1", None));
        wait_until(|| fixture.operations.write_count() == HOOK_WRITE_BATCH_COUNT);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(fixture.operations.write_count(), HOOK_WRITE_BATCH_COUNT);
        gate.open();
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
    });
}

#[test]
fn background_scheduler_merges_identical_requests_and_calls_every_callback() {
    let fixture = fixture();
    let first_record = Arc::new(CallbackRecord::default());
    let second_record = Arc::new(CallbackRecord::default());

    let handle = fixture.installer.schedule_hooks_install(
        "/repo",
        "task-1",
        None,
        record_callbacks(&first_record, Duration::from_millis(100)),
    );
    let merged = fixture.installer.schedule_hooks_install(
        "/repo",
        "task-1",
        None,
        record_callbacks(&second_record, Duration::ZERO),
    );
    handle.unwrap().join().unwrap();

    assert!(merged.is_none());
    assert_eq!(fixture.operations.write_count(), HOOK_WRITE_BATCH_COUNT);
    assert_eq!(first_record.successes.load(Ordering::SeqCst), 1);
    assert_eq!(second_record.successes.load(Ordering::SeqCst), 1);
}

#[test]
fn install_logs_each_provider_verification_result() {
    let fixture = fixture();

    fixture
        .installer
        .install_hooks("/repo", "task-1", None)
        .unwrap();

    let call = fixture.claude.status_calls.lock().unwrap()[0].clone();
    assert_eq!(
        (
            call.repo_path.as_str(),
            call.task_id.as_deref(),
            call.ssh_host,
            call.files
        ),
        ("/repo", Some("task-1"), None, None)
    );
    let record = fixture
        .log
        .verification_for(HookLogLevel::Info, "Claude")
        .unwrap();
    assert!(record.is_installed);
    assert_eq!(
        (record.target_path.as_str(), record.task_id.as_str()),
        ("/repo", "task-1")
    );
}

#[test]
fn an_uninstalled_provider_in_verification_fails_the_install_after_retries() {
    let fixture = fixture();
    fixture.codex.status_always(Ok(status_with(
        KanvibeHookProvider::Codex,
        false,
        &["hasConfigEntry"],
    )));

    let error = fixture
        .installer
        .install_hooks("/repo", "task-1", None)
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("hooks 검증 실패: Codex(hasConfigEntry)")
    );
    assert_eq!(fixture.codex.status_count(), 3);
    let record = fixture
        .log
        .verification_for(HookLogLevel::Warn, "Codex")
        .unwrap();
    assert!(!record.is_installed);
    assert_eq!(record.failed_checks, ["hasConfigEntry"]);
    assert_eq!(record.target_path, "/repo");
}

#[test]
fn remote_install_returns_only_after_provider_verification_finishes() {
    let fixture = fixture();
    let gate = Gate::new();
    let status_gate = Arc::clone(&gate);
    fixture.claude.status_once(move || {
        status_gate.wait();
        Ok(installed_status(KanvibeHookProvider::Claude))
    });

    std::thread::scope(|scope| {
        let install = scope.spawn(|| {
            fixture
                .installer
                .install_hooks("/remote/repo", "task-2", Some("remote-host"))
        });
        wait_until(|| fixture.claude.status_count() == 1);
        let call = fixture.claude.status_calls.lock().unwrap()[0].clone();
        assert_eq!(call.ssh_host.as_deref(), Some("remote-host"));
        assert!(call.files.is_some());
        std::thread::sleep(Duration::from_millis(50));
        assert!(!install.is_finished());
        gate.open();
        install.join().unwrap().unwrap();
    });
}

#[test]
fn statuses_are_returned_per_provider() {
    let fixture = fixture();

    let statuses = fixture
        .installer
        .hooks_statuses("/repo", "task-1", None)
        .unwrap();

    assert!(matches!(statuses.claude, KanvibeHookStatus::Claude(_)));
    assert!(matches!(statuses.open_code, KanvibeHookStatus::OpenCode(_)));
}
