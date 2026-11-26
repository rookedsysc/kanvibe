//! 원격 실행 시나리오 테스트(TS `gitOperations.test.ts`).
//! TS는 가짜 `child_process.spawn`과 가짜 타이머로 순서를 맞췄다. 여기서는 가짜 ssh 실행기에 프로세스 출력을 직접 넣고,
//! 동시 호출은 스레드로 띄운 뒤 띄운 프로세스 수와 대기 줄 길이를 기다려 순서를 고정한다.
//! 가짜 타이머가 필요하던 시간 초과는 실제 시간을 짧게(수십 ms) 줄여 같은 순서를 만든다.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use regex::Regex;

use super::remote::{
    RemoteCommandFailureLog, RemoteCommandObserver, RemoteSshExecutor, RemoteSshParts,
    RemoteSshSettings, SlowRemoteCommandLog, WallClock,
};
use super::remote_command::{REMOTE_COMMAND_EXIT_MARKER, exit_error};
use super::remote_errors::is_ssh_transport_error;
use super::ssh_process::SshSpawnMode;
use super::ssh_process::fake::{FakeSshLauncher, FakeSshProcess};
use super::{ExecGitError, ExecGitOptions, RemoteStdinResponder, run_with_background_ssh_commands};
use crate::shell_environment::ShellEnvironment;

const HOST: &str = "remote-host";
const OTHER_HOST: &str = "other-host";

#[derive(Default)]
struct RecordingObserver {
    failures: Mutex<Vec<RemoteCommandFailureLog>>,
    slow_commands: Mutex<Vec<SlowRemoteCommandLog>>,
}

impl RemoteCommandObserver for RecordingObserver {
    fn command_failed(&self, log: &RemoteCommandFailureLog) {
        self.failures.lock().unwrap().push(log.clone());
    }

    fn slow_command(&self, log: &SlowRemoteCommandLog) {
        self.slow_commands.lock().unwrap().push(log.clone());
    }
}

struct Harness {
    executor: Arc<RemoteSshExecutor>,
    launcher: Arc<FakeSshLauncher>,
    observer: Arc<RecordingObserver>,
    home: tempfile::TempDir,
}

const SSH_CONFIG: &str = "Host remote-host\n  HostName example.com\n  Port 2202\n  User tester\n  IdentityFile /tmp/test-key\n\nHost other-host\n  HostName other.example.com\n  Port 2202\n  User tester\n  IdentityFile /tmp/test-key\n";

fn system_clock() -> WallClock {
    let started_at = Instant::now();
    Arc::new(move || 1_000_000 + started_at.elapsed().as_millis() as u64)
}

fn harness_with(settings: RemoteSshSettings, wall_clock: WallClock) -> Harness {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".ssh")).unwrap();
    std::fs::write(home.path().join(".ssh/config"), SSH_CONFIG).unwrap();
    let launcher = FakeSshLauncher::new();
    let observer = Arc::new(RecordingObserver::default());
    let parts = RemoteSshParts {
        launcher: launcher.clone(),
        observer: observer.clone(),
        wall_clock,
    };
    let executor = RemoteSshExecutor::with_parts(
        home.path().to_str().unwrap(),
        ShellEnvironment::new(),
        settings,
        parts,
    );
    Harness {
        executor: Arc::new(executor),
        launcher,
        observer,
        home,
    }
}

fn harness_with_concurrency(host_max_concurrency: usize) -> Harness {
    let settings = RemoteSshSettings {
        host_max_concurrency,
        ..RemoteSshSettings::default()
    };
    harness_with(settings, system_clock())
}

fn harness() -> Harness {
    harness_with_concurrency(8)
}

fn timeout_options(timeout_ms: u64) -> ExecGitOptions {
    ExecGitOptions {
        timeout: Some(Duration::from_millis(timeout_ms)),
        ..ExecGitOptions::default()
    }
}

fn retry_safe() -> ExecGitOptions {
    ExecGitOptions {
        is_retry_safe: true,
        ..ExecGitOptions::default()
    }
}

fn strict(timeout_ms: u64) -> ExecGitOptions {
    ExecGitOptions {
        should_check_host_key_strictly: true,
        ..timeout_options(timeout_ms)
    }
}

fn responder(
    respond: impl Fn(&str) -> Result<String, ExecGitError> + Send + Sync + 'static,
) -> RemoteStdinResponder {
    RemoteStdinResponder {
        marker: "READY".into(),
        respond: Arc::new(respond),
    }
}

impl Harness {
    fn exec(&self, command: &str, options: &ExecGitOptions) -> Result<String, ExecGitError> {
        self.executor.exec(command, HOST, options)
    }

    fn spawn_exec(
        &self,
        host: &str,
        command: &str,
        options: ExecGitOptions,
        is_background: bool,
    ) -> JoinHandle<Result<String, ExecGitError>> {
        let executor = Arc::clone(&self.executor);
        let host = host.to_string();
        let command = command.to_string();
        std::thread::spawn(move || {
            let run = || executor.exec(&command, &host, &options);
            if is_background {
                run_with_background_ssh_commands(run)
            } else {
                run()
            }
        })
    }

    /// 띄운 프로세스와 채널 대기 줄을 합친 수가 `count`가 될 때까지 기다린다
    fn wait_for_entered(&self, count: usize) {
        wait_until(|| self.launcher.command_count() + self.executor.queued_count(HOST) == count);
    }

    /// 앞 호출이 프로세스를 띄우거나 줄에 선 뒤에 다음 호출을 띄워 TS의 호출 순서를 그대로 만든다
    fn start_in_order(
        &self,
        commands: &[&str],
        options: &ExecGitOptions,
        is_background: bool,
    ) -> Vec<JoinHandle<Result<String, ExecGitError>>> {
        let already_entered = self.launcher.command_count() + self.executor.queued_count(HOST);
        let mut handles = Vec::new();
        for (index, command) in commands.iter().enumerate() {
            handles.push(self.spawn_exec(HOST, command, options.clone(), is_background));
            self.wait_for_entered(already_entered + index + 1);
        }
        handles
    }

    fn command(&self, index: usize) -> FakeSshProcess {
        self.launcher.command(index)
    }

    fn control_path_pattern(&self) -> Regex {
        let home = regex::escape(self.home.path().to_str().unwrap());
        Regex::new(&format!(
            "^{home}/\\.kanvibe/ssh-[a-f0-9]{{12}}-[a-f0-9]{{20}}-0$"
        ))
        .unwrap()
    }

    fn master_args_contain(&self, value: &str) -> bool {
        (0..self.launcher.master_count()).any(|index| {
            self.launcher
                .master(index)
                .0
                .args
                .iter()
                .any(|arg| arg == value)
        })
    }
}

fn wait_until(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "조건을 기다리다 시간이 다 됐습니다"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn settle() {
    std::thread::sleep(Duration::from_millis(40));
}

fn join(handle: JoinHandle<Result<String, ExecGitError>>) -> Result<String, ExecGitError> {
    handle.join().unwrap()
}

fn assert_message_matches(error: &ExecGitError, pattern: &str) {
    assert!(
        Regex::new(pattern).unwrap().is_match(&error.message),
        "{}",
        error.message
    );
}

#[test]
fn many_multibyte_chunks_are_measured_incrementally() {
    let h = harness();
    let chunk = "한글🙂".repeat(64);
    let written = chunk.clone();
    h.launcher.on_command(move |child, _| {
        for _ in 0..128 {
            child.write_stdout(&written);
        }
        child.complete("", 0);
    });

    assert_eq!(
        h.exec("stream", &ExecGitOptions::default()).unwrap(),
        chunk.repeat(128)
    );
}

#[test]
fn late_output_after_settlement_is_not_processed() {
    for is_success in [true, false] {
        let h = harness();
        h.launcher.on_command(move |child, _| {
            if is_success {
                child.complete("ok", 0);
            } else {
                child.write_stdout(&"x".repeat(101));
            }
            child.write_stdout("late stdout");
            child.write_stderr("late stderr");
        });
        let options = ExecGitOptions {
            max_buffer_bytes: Some(100),
            ..ExecGitOptions::default()
        };

        let result = h.exec("stream", &options);

        match is_success {
            true => assert_eq!(result.unwrap(), "ok"),
            false => assert!(result.unwrap_err().message.contains("exceeded maxBuffer")),
        }
        assert_eq!(h.command(0).kill_count(), 1);
    }
}

#[test]
fn completion_marker_chunk_boundaries_and_last_prefix_win() {
    let marker = REMOTE_COMMAND_EXIT_MARKER;
    let first_text = format!("한글🙂\n{marker}:0\r\n");
    let cases: Vec<(Vec<String>, String)> = vec![
        (
            first_text.chars().map(String::from).collect(),
            "한글🙂".into(),
        ),
        (
            vec![format!("before\n{marker}:0\r"), String::new(), "\n".into()],
            "before".into(),
        ),
        (
            vec![format!("before\n{marker}:9\nignored\n{marker}:0\n")],
            format!("before\n{marker}:9\nignored"),
        ),
        (
            vec![format!("before\n{marker}:0\nlast\n{marker}:")],
            format!("before\n{marker}:0\nlast\n{marker}:"),
        ),
        (
            vec![format!("before\n{marker}:x"), "ignored".into()],
            format!("before\n{marker}:xignored"),
        ),
    ];
    for (chunks, expected) in cases {
        let h = harness();
        h.launcher.on_command(move |child, _| {
            for chunk in &chunks {
                child.write_stdout(chunk);
            }
            child.close(0);
        });

        assert_eq!(
            h.exec("stream", &ExecGitOptions::default()).unwrap(),
            expected
        );
    }
}

#[test]
fn utf8_stdout_and_stderr_are_summed_up_to_the_exact_limit() {
    let h = harness();
    h.launcher.on_command(|child, _| {
        child.write_stdout("한");
        child.write_stderr("🙂");
        child.write_stdout("글");
        child.close(0);
    });
    let options = ExecGitOptions {
        max_buffer_bytes: Some(10),
        ..ExecGitOptions::default()
    };

    assert_eq!(h.exec("stream", &options).unwrap(), "한글");
}

#[test]
fn removing_the_responder_request_line_frees_output_capacity() {
    let h = harness();
    let payload = "x".repeat(80);
    let output = "한".repeat(25);
    let (request_line, final_output) = (format!("READY\t{payload}\n"), output.clone());
    h.launcher.on_command(move |child, _| {
        let final_output = final_output.clone();
        child.on_stdin_end(move |child| {
            child.write_stdout(&final_output);
            child.close(0);
        });
        child.write_stdout(&request_line);
    });
    let payloads = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&payloads);
    let options = ExecGitOptions {
        max_buffer_bytes: Some(100),
        remote_stdin_responder: Some(responder(move |payload| {
            recorded.lock().unwrap().push(payload.to_string());
            Ok("reply".into())
        })),
        ..ExecGitOptions::default()
    };

    assert_eq!(h.exec("transaction", &options).unwrap(), output);
    assert_eq!(*payloads.lock().unwrap(), [payload]);
}

#[test]
fn exceeding_the_limit_before_removing_the_request_skips_the_callback() {
    let h = harness();
    h.launcher
        .on_command(|child, _| child.write_stdout("READY\toversized\n"));
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let options = ExecGitOptions {
        max_buffer_bytes: Some(10),
        remote_stdin_responder: Some(responder(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok("reply".into())
        })),
        ..ExecGitOptions::default()
    };

    assert!(
        h.exec("transaction", &options)
            .unwrap_err()
            .message
            .contains("exceeded maxBuffer")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn a_split_request_line_is_answered_once_and_secrets_go_through_stdin() {
    let h = harness();
    h.launcher.on_command(|child, _| {
        child.on_stdin_end(|child| child.complete("done", 0));
        child.write_stdout("login banner\nREADY\tb2");
        child.write_stdout("xk\n");
        child.write_stdout("READY\tb2xk\n");
    });
    let launcher = Arc::clone(&h.launcher);
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let options = ExecGitOptions {
        remote_stdin_responder: Some(responder(move |payload| {
            assert!(launcher.command(0).stdin_writes().is_empty());
            assert_eq!(payload, "b2xk");
            counted.fetch_add(1, Ordering::SeqCst);
            Ok("secret-response\n".into())
        })),
        ..timeout_options(100)
    };

    assert!(h.exec("transaction", &options).unwrap().contains("done"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        h.command(0).stdin_writes(),
        [Some("secret-response\n".to_string())]
    );
    assert!(!h.command(0).0.args.join(" ").contains("secret-response"));
}

#[test]
fn nested_same_host_commands_in_a_callback_share_the_shard_without_deadlock() {
    let h = harness_with_concurrency(1);
    h.launcher.on_command(|child, index| match index {
        0 => {
            child.on_stdin_end(|child| child.complete("committed", 0));
            child.write_stdout("READY\t\n");
        }
        _ => child.complete("nested-result", 0),
    });
    let executor = Arc::clone(&h.executor);
    let options = ExecGitOptions {
        remote_stdin_responder: Some(responder(move |_| {
            assert_eq!(
                executor
                    .exec("herdr workspace list", HOST, &timeout_options(100))
                    .unwrap(),
                "nested-result"
            );
            Ok("response\n".into())
        })),
        ..timeout_options(150)
    };

    assert!(
        h.exec("transaction", &options)
            .unwrap()
            .contains("committed")
    );
    assert_eq!(h.launcher.command_count(), 2);
    assert_eq!(h.command(1).control_path(), h.command(0).control_path());
}

#[test]
fn a_callback_error_closes_input_and_the_process_without_replaying() {
    let h = harness();
    h.launcher
        .on_command(|child, _| child.write_stdout("READY\t\n"));
    let options = ExecGitOptions {
        remote_stdin_responder: Some(responder(|_| Err(ExecGitError::new("update rejected")))),
        ..timeout_options(100)
    };

    assert!(
        h.exec("transaction", &options)
            .unwrap_err()
            .message
            .contains("update rejected")
    );
    assert_eq!(h.command(0).stdin_writes(), [None]);
    assert_eq!(h.command(0).kill_count(), 1);
    assert_eq!(h.launcher.command_count(), 1);
}

#[test]
fn a_transport_drop_after_the_response_does_not_replay_the_transaction() {
    let h = harness();
    h.launcher.on_command(|child, _| {
        child.on_stdin_end(|child| {
            child.write_stderr("Connection reset by peer");
            child.close(255);
        });
        child.write_stdout("READY\t\n");
    });
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let options = ExecGitOptions {
        remote_stdin_responder: Some(responder(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok("response\n".into())
        })),
        ..timeout_options(100)
    };

    assert!(
        h.exec("transaction", &options)
            .unwrap_err()
            .message
            .contains("Connection reset by peer")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(h.launcher.command_count(), 1);
}

#[test]
fn ssh_exiting_right_after_the_request_skips_the_unstarted_callback() {
    let h = harness();
    h.launcher.on_command(|child, _| {
        child.write_stdout("READY\t\n");
        child.write_stderr("Connection reset by peer");
        child.close(255);
    });
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let options = ExecGitOptions {
        remote_stdin_responder: Some(responder(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok("response\n".into())
        })),
        ..timeout_options(100)
    };

    assert!(
        h.exec("transaction", &options)
            .unwrap_err()
            .message
            .contains("Connection reset by peer")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn transport_and_cooldown_errors_carry_the_actual_remaining_retry_time() {
    let now_ms = Arc::new(AtomicU64::new(1000));
    let clock_ms = Arc::clone(&now_ms);
    let settings = RemoteSshSettings {
        transport_failure_cooldown: Duration::from_millis(5000),
        ..RemoteSshSettings::default()
    };
    let h = harness_with(settings, Arc::new(move || clock_ms.load(Ordering::SeqCst)));
    h.launcher.on_command(|child, _| {
        child.write_stderr("Connection reset by peer");
        child.close(255);
    });

    assert_eq!(
        h.exec("command", &ExecGitOptions::default())
            .unwrap_err()
            .retry_after_ms
            .map(u32::from),
        Some(5000)
    );
    now_ms.store(2500, Ordering::SeqCst);
    assert_eq!(
        h.exec("following", &ExecGitOptions::default())
            .unwrap_err()
            .retry_after_ms
            .map(u32::from),
        Some(3500)
    );
    assert_eq!(h.launcher.command_count(), 1);
}

#[test]
fn remote_stdin_is_piped_and_kept_out_of_the_arguments() {
    let h = harness();
    h.launcher.on_command(|child, _| child.complete("ok", 0));
    let options = ExecGitOptions {
        remote_stdin: Some("secret-body".into()),
        ..ExecGitOptions::default()
    };

    h.exec("cat > /tmp/target", &options).unwrap();

    assert_eq!(
        h.command(0).stdin_writes(),
        [Some("secret-body".to_string())]
    );
    assert_eq!(h.command(0).0.mode, SshSpawnMode::CommandWithStdin);
    assert!(!h.command(0).0.args.join(" ").contains("secret-body"));
}

#[test]
fn host_key_verified_commands_do_not_reuse_existing_connections() {
    let h = harness();
    h.launcher.on_command(|child, _| child.complete("ok", 0));

    h.exec("printf ok", &strict(45_000)).unwrap();

    let args = &h.command(0).0.args;
    for expected in [
        "StrictHostKeyChecking=yes",
        "ControlMaster=no",
        "ControlPath=none",
    ] {
        assert!(args.iter().any(|arg| arg == expected), "{expected}");
    }
    assert!(!args.iter().any(|arg| arg == "ControlMaster=auto"));
}

#[test]
fn remote_commands_use_the_ssh_binary_and_options() {
    let h = harness();
    h.launcher.on_command(|child, _| child.complete("ok", 0));

    assert_eq!(h.exec("pwd", &ExecGitOptions::default()).unwrap(), "ok");

    let child = h.command(0);
    let args = &child.0.args;
    let control_path = args[16].strip_prefix("ControlPath=").unwrap();
    assert!(
        h.control_path_pattern().is_match(control_path),
        "{control_path}"
    );
    let expected_head = [
        "-o",
        "ProxyCommand=false",
        "-i",
        "/tmp/test-key",
        "-p",
        "2202",
        "-o",
        "BatchMode=yes",
        "-o",
        "IdentitiesOnly=yes",
        "-T",
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPersist=10m",
        "-o",
    ];
    assert_eq!(args[..16], expected_head.map(String::from));
    let expected_tail = [
        "-o",
        "ConnectTimeout=8",
        "-o",
        "ServerAliveInterval=5",
        "-o",
        "ServerAliveCountMax=2",
        "remote-host",
    ];
    assert_eq!(args[17..24], expected_tail.map(String::from));
    assert_eq!(args.len(), 25);
    assert!(!args.iter().any(|arg| arg == "-Y"));
    assert_eq!(child.0.mode, SshSpawnMode::CommandWithoutStdin);
    assert_eq!(child.kill_count(), 1);
    assert!(child.remote_command().contains("pwd"));
    assert!(child.remote_command().contains(REMOTE_COMMAND_EXIT_MARKER));
}

#[test]
fn the_per_command_output_limit_applies() {
    let h = harness();
    h.launcher
        .on_command(|child, _| child.complete("output larger than the command limit", 0));
    let options = ExecGitOptions {
        max_buffer_bytes: Some(16),
        ..ExecGitOptions::default()
    };

    assert!(
        h.exec("cat chat.jsonl", &options)
            .unwrap_err()
            .message
            .contains("exceeded maxBuffer")
    );
}

#[test]
fn following_commands_skip_ssh_right_after_a_transport_failure() {
    let h = harness();
    h.launcher.on_command(|child, _| {
        child.write_stderr(
            "mux_client_request_session: session request failed: Session open refused by peer\nConnection closed by 100.73.171.123 port 22",
        );
        child.close(255);
    });

    assert!(
        h.exec(
            "git -C /repo worktree list --porcelain",
            &ExecGitOptions::default()
        )
        .is_err()
    );
    assert!(
        h.exec("git -C /repo status --short", &ExecGitOptions::default())
            .is_err()
    );

    assert_eq!(h.launcher.command_count(), 1);
    assert_eq!(h.launcher.master_count(), 1);
    let master_args = &h.launcher.master(0).0.args;
    assert_eq!(master_args[..2], ["-N".to_string(), "-f".to_string()]);
    assert!(!h.master_args_contain("exit"));
}

#[test]
fn a_remote_timeout_is_not_replayed_and_keeps_the_master_for_the_next_command() {
    let h = harness();
    h.launcher.on_command(|child, _| {
        child.emit_error(ExecGitError {
            message: "Command timed out".into(),
            was_killed: true,
            signal: Some("SIGTERM"),
            ..ExecGitError::default()
        })
    });

    assert_message_matches(
        &h.exec("long-running-command", &timeout_options(1000))
            .unwrap_err(),
        "1초",
    );
    assert_message_matches(
        &h.exec("next-command", &ExecGitOptions::default())
            .unwrap_err(),
        "완료되지 않았",
    );
    assert_eq!(h.launcher.command_count(), 2);
    assert_eq!(h.launcher.master_count(), 1);
    assert!(!h.master_args_contain("exit"));
}

#[test]
fn clearing_the_cooldown_lets_the_next_command_connect_again() {
    let h = harness();
    let is_transport_down = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let transport = Arc::clone(&is_transport_down);
    h.launcher.on_command(move |child, _| {
        if transport.load(Ordering::SeqCst) {
            child.write_stderr("Connection reset by 100.99.109.53 port 22\n");
            child.close(255);
        } else {
            child.complete("attached", 0);
        }
    });

    assert_message_matches(
        &h.exec("background-sync", &ExecGitOptions::default())
            .unwrap_err(),
        "Connection reset",
    );
    assert_message_matches(
        &h.exec("blocked-command", &ExecGitOptions::default())
            .unwrap_err(),
        "최근 SSH transport 실패",
    );
    let spawn_count_before_clear = h.launcher.command_count();
    is_transport_down.store(false, Ordering::SeqCst);
    h.executor.clear_transport_cooldown(HOST);

    assert_eq!(
        h.exec("terminal-attach", &ExecGitOptions::default())
            .unwrap(),
        "attached"
    );
    assert_eq!(h.launcher.command_count(), spawn_count_before_clear + 1);
    assert!(
        h.command(spawn_count_before_clear)
            .remote_command()
            .contains("terminal-attach")
    );
}

#[test]
fn commands_with_expected_failures_leave_no_error_log() {
    let h = harness();
    h.launcher.on_command(|child, _| {
        child.write_stderr("can't find session: kanvibe-dev\n");
        child.complete("", 1);
    });
    let quiet = ExecGitOptions {
        is_failure_expected: true,
        ..ExecGitOptions::default()
    };

    assert!(
        h.exec("tmux list-windows -t 'kanvibe-dev'", &quiet)
            .is_err()
    );
    let logged_after_quiet_command = h.observer.failures.lock().unwrap().len();
    assert!(
        h.exec(
            "tmux list-windows -t 'kanvibe-dev'",
            &ExecGitOptions::default()
        )
        .is_err()
    );

    assert_eq!(logged_after_quiet_command, 0);
    assert_eq!(h.observer.failures.lock().unwrap()[0].ssh_host, HOST);
}

#[test]
fn a_transport_failure_recovered_within_three_attempts_returns_the_result() {
    let h = harness();
    h.launcher.on_command(|child, index| {
        if index == 0 {
            child.write_stderr("Connection reset by peer\n");
            child.close(255);
        } else {
            child.complete("recovered", 0);
        }
    });

    assert_eq!(
        h.exec("recoverable-command", &retry_safe()).unwrap(),
        "recovered"
    );
    assert_eq!(h.launcher.command_count(), 2);
    assert_eq!(h.launcher.master_count(), 2);
    assert_eq!(
        h.exec("next-command", &ExecGitOptions::default()).unwrap(),
        "recovered"
    );
}

/// 상한까지만 동시에 띄우고, 하나가 끝나면 줄의 다음 명령을 띄우는지 확인한다
fn assert_limited_concurrency(h: &Harness, total: usize, limit: usize) {
    let commands: Vec<String> = (0..total).map(|index| format!("command-{index}")).collect();
    let command_refs: Vec<&str> = commands.iter().map(String::as_str).collect();
    let mut handles = h
        .start_in_order(&command_refs, &ExecGitOptions::default(), false)
        .into_iter();
    assert_eq!(h.launcher.command_count(), limit);
    for (index, command) in commands.iter().take(limit).enumerate() {
        assert!(h.command(index).remote_command().contains(command.as_str()));
    }
    h.command(0).complete("done", 0);
    assert_eq!(join(handles.next().unwrap()).unwrap(), "done");
    wait_until(|| h.launcher.command_count() == limit + 1);
    assert!(h.command(limit).remote_command().contains(&commands[limit]));
    for (offset, handle) in handles.enumerate() {
        h.command(offset + 1).complete(&offset.to_string(), 0);
        assert_eq!(join(handle).unwrap(), offset.to_string());
    }
}

#[test]
fn same_host_commands_run_with_limited_concurrency() {
    assert_limited_concurrency(&harness_with_concurrency(4), 5, 4);
}

#[test]
fn default_host_concurrency_is_eight_channels() {
    assert_limited_concurrency(
        &harness_with(RemoteSshSettings::default(), system_clock()),
        9,
        8,
    );
}

#[test]
fn the_environment_can_raise_the_concurrency_limit() {
    let environment: ShellEnvironment = [(
        "KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY".to_string(),
        "6".to_string(),
    )]
    .into();
    let h = harness_with(
        RemoteSshSettings::from_environment(&environment),
        system_clock(),
    );

    assert_limited_concurrency(&h, 7, 6);
}

#[test]
fn environment_concurrency_never_exceeds_eight_channels() {
    let environment: ShellEnvironment = [(
        "KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY".to_string(),
        "12".to_string(),
    )]
    .into();
    let h = harness_with(
        RemoteSshSettings::from_environment(&environment),
        system_clock(),
    );

    assert_limited_concurrency(&h, 9, 8);
}

#[test]
fn concurrent_commands_share_one_process_owned_control_master() {
    let h = harness_with_concurrency(6);
    let commands: Vec<String> = (0..6).map(|index| format!("command-{index}")).collect();
    let command_refs: Vec<&str> = commands.iter().map(String::as_str).collect();

    let handles = h.start_in_order(&command_refs, &ExecGitOptions::default(), false);

    let control_paths: std::collections::HashSet<String> = (0..6)
        .map(|index| h.command(index).control_path())
        .collect();
    assert_eq!(control_paths.len(), 1);
    assert!(
        h.control_path_pattern()
            .is_match(&h.command(0).control_path())
    );
    assert_eq!(h.launcher.master_count(), 1);
    for (index, handle) in handles.into_iter().enumerate() {
        h.command(index).complete(&index.to_string(), 0);
        assert_eq!(join(handle).unwrap(), index.to_string());
    }
}

#[test]
fn later_commands_keep_sharing_the_master_of_completed_commands() {
    let h = harness_with_concurrency(2);

    let mut handles = h
        .start_in_order(
            &["first", "second", "third"],
            &ExecGitOptions::default(),
            false,
        )
        .into_iter();

    assert_eq!(h.launcher.command_count(), 2);
    let first_control_path = h.command(0).control_path();
    assert!(h.control_path_pattern().is_match(&first_control_path));
    assert_eq!(h.command(1).control_path(), first_control_path);
    h.command(0).complete("one", 0);
    assert_eq!(join(handles.next().unwrap()).unwrap(), "one");
    wait_until(|| h.launcher.command_count() == 3);
    assert_eq!(h.command(2).control_path(), first_control_path);
    h.command(1).complete("two", 0);
    h.command(2).complete("three", 0);
    assert_eq!(join(handles.next().unwrap()).unwrap(), "two");
    assert_eq!(join(handles.next().unwrap()).unwrap(), "three");
}

#[test]
fn quiet_probe_failures_leave_no_error_log() {
    let h = harness();
    h.launcher.on_command(|child, _| child.close(255));

    assert!(
        h.exec(
            "tmux has-session -t \"remote-session\" 2>/dev/null",
            &ExecGitOptions::default()
        )
        .is_err()
    );

    assert!(h.observer.failures.lock().unwrap().is_empty());
}

#[test]
fn file_probe_failures_leave_no_error_log() {
    let h = harness();
    h.launcher.on_command(|child, _| {
        child.write_stderr("Connection reset by peer\n");
        child.close(255);
    });

    let command = "test -f '/repo/.kanvibe/task-id' && cat '/repo/.kanvibe/task-id' || true";
    assert!(h.exec(command, &ExecGitOptions::default()).is_err());

    assert!(h.observer.failures.lock().unwrap().is_empty());
}

#[test]
fn permanent_failures_are_not_transient_and_never_replayed() {
    for message in [
        "Permission denied (publickey)",
        "Host key verification failed",
        "REMOTE HOST IDENTIFICATION HAS CHANGED",
    ] {
        let h = harness();
        let stderr = message.to_string();
        h.launcher.on_command(move |child, _| {
            child.write_stderr(&stderr);
            child.close(255);
        });

        assert!(!is_ssh_transport_error(&ExecGitError::new(message)));
        assert!(
            h.exec("cat config", &retry_safe())
                .unwrap_err()
                .message
                .contains(message)
        );
        assert_eq!(h.launcher.command_count(), 1);
        assert!(
            h.exec("cat config", &retry_safe())
                .unwrap_err()
                .message
                .contains(message)
        );
        assert_eq!(h.launcher.command_count(), 2);
        assert!(!h.master_args_contain("exit"));
    }
}

#[test]
fn a_mutation_with_an_unknown_result_is_not_replayed() {
    let h = harness();
    h.launcher.on_command(|child, _| {
        child.write_stderr("Connection reset by peer");
        child.close(255);
    });

    assert!(
        h.exec("herdr workspace create", &ExecGitOptions::default())
            .unwrap_err()
            .message
            .contains("Connection reset")
    );
    assert_eq!(h.launcher.command_count(), 1);
    assert!(!h.master_args_contain("exit"));
}

#[test]
fn background_saturation_reserves_capacity_for_interactive_detail_reads() {
    let h = harness_with_concurrency(3);
    let repairs = ["repair-0", "repair-1", "repair-2", "repair-3"];
    let mut background = Vec::new();
    for (index, repair) in repairs.iter().enumerate() {
        background.push(h.spawn_exec(HOST, repair, ExecGitOptions::default(), true));
        h.wait_for_entered(index + 1);
    }
    assert_eq!(h.launcher.command_count(), 2);

    let detail = h.spawn_exec(HOST, "interactive-detail", ExecGitOptions::default(), false);
    wait_until(|| h.launcher.command_count() == 3);
    assert!(h.command(2).remote_command().contains("interactive-detail"));
    h.command(2).complete("usable", 0);
    assert_eq!(join(detail).unwrap(), "usable");
    assert_eq!(h.launcher.command_count(), 3);

    let mut background = background.into_iter();
    h.command(0).complete("repaired", 0);
    join(background.next().unwrap()).unwrap();
    wait_until(|| h.launcher.command_count() == 4);
    h.command(1).complete("repaired", 0);
    join(background.next().unwrap()).unwrap();
    wait_until(|| h.launcher.command_count() == 5);
    h.command(3).complete("repaired", 0);
    h.command(4).complete("repaired", 0);
    for handle in background {
        join(handle).unwrap();
    }
}

#[test]
fn interactive_queued_commands_precede_background_work_without_raising_the_ceiling() {
    let h = harness_with_concurrency(1);
    let active = h.spawn_exec(HOST, "active", ExecGitOptions::default(), false);
    h.wait_for_entered(1);
    let background = h.spawn_exec(HOST, "repair", ExecGitOptions::default(), true);
    h.wait_for_entered(2);
    let detail = h.spawn_exec(HOST, "interactive-detail", ExecGitOptions::default(), false);
    h.wait_for_entered(3);

    h.command(0).complete("done", 0);
    join(active).unwrap();
    wait_until(|| h.launcher.command_count() == 2);
    assert!(h.command(1).remote_command().contains("interactive-detail"));
    h.command(1).complete("usable", 0);
    join(detail).unwrap();
    wait_until(|| h.launcher.command_count() == 3);
    h.command(2).complete("repaired", 0);
    join(background).unwrap();
}

#[test]
fn a_queued_command_that_expired_is_never_dispatched_later() {
    let h = harness_with_concurrency(1);
    h.launcher.on_command(|child, index| {
        if index > 0 {
            child.complete("next", 0);
        }
    });
    let active = h.spawn_exec(HOST, "active", timeout_options(500), false);
    h.wait_for_entered(1);
    let expired = h.spawn_exec(HOST, "expired", timeout_options(20), false);

    assert_message_matches(&join(expired).unwrap_err(), "SSH 명령이 .*완료되지 않았");
    assert_eq!(h.launcher.command_count(), 1);
    h.command(0).complete("done", 0);
    assert_eq!(join(active).unwrap(), "done");
    assert_eq!(h.exec("next", &timeout_options(100)).unwrap(), "next");
    assert_eq!(h.launcher.command_count(), 2);
    assert!(h.command(1).remote_command().contains("next"));
}

#[test]
fn a_saturated_repair_host_does_not_block_another_host() {
    let h = harness_with_concurrency(1);
    h.launcher.on_command(|child, _| {
        if !child.0.args.iter().any(|arg| arg == HOST) {
            child.complete("other-host-ready", 0);
        }
    });
    let repair = h.spawn_exec(HOST, "repair", ExecGitOptions::default(), true);
    wait_until(|| h.launcher.command_count() == 1);

    assert_eq!(
        h.executor
            .exec("detail", OTHER_HOST, &ExecGitOptions::default())
            .unwrap(),
        "other-host-ready"
    );
    h.command(0).complete("repaired", 0);
    assert_eq!(join(repair).unwrap(), "repaired");
}

#[test]
fn a_command_timeout_preserves_a_concurrent_command_and_allows_the_next() {
    let h = harness();
    h.launcher.on_command(|child, index| {
        if index > 1 {
            child.complete("next", 0);
        }
    });
    let first = h.spawn_exec(HOST, "mutation", timeout_options(25), false);
    h.wait_for_entered(1);
    let second = h.spawn_exec(HOST, "working", timeout_options(500), false);
    h.wait_for_entered(2);

    assert_message_matches(&join(first).unwrap_err(), "완료되지 않았");
    h.command(1).complete("survived", 0);
    assert_eq!(join(second).unwrap(), "survived");
    assert_eq!(h.exec("next", &ExecGitOptions::default()).unwrap(), "next");
    assert_eq!(h.launcher.command_count(), 3);
    assert!(!h.master_args_contain("exit"));
}

#[test]
fn cold_master_preparation_is_joined_and_shared_across_concurrent_commands() {
    let h = harness();
    h.launcher.on_master(|_, _| {});
    h.launcher.on_command(|child, _| child.complete("done", 0));
    let operations: Vec<_> = ["one", "two", "three"]
        .iter()
        .map(|command| h.spawn_exec(HOST, command, ExecGitOptions::default(), false))
        .collect();
    wait_until(|| h.launcher.master_count() == 1);
    settle();

    assert_eq!(h.launcher.master_count(), 1);
    assert_eq!(h.launcher.command_count(), 0);
    h.launcher.master(0).exit(0);
    for operation in operations {
        assert_eq!(join(operation).unwrap(), "done");
    }
    let control_paths: std::collections::HashSet<String> = (0..3)
        .map(|index| h.command(index).control_path())
        .collect();
    assert_eq!(control_paths.len(), 1);
    assert!(
        h.command(0)
            .0
            .args
            .iter()
            .any(|arg| arg == "ProxyCommand=false")
    );
    assert!(
        h.command(0)
            .0
            .args
            .iter()
            .any(|arg| arg == "ControlMaster=no")
    );
}

#[test]
fn strict_connections_are_independent_and_admitted_one_at_a_time() {
    let h = harness();
    let one = h.spawn_exec(HOST, "read one", strict(45_000), false);
    wait_until(|| h.launcher.command_count() == 1);
    let two = h.spawn_exec(HOST, "read two", strict(45_000), false);
    settle();

    assert_eq!(h.launcher.command_count(), 1);
    h.command(0).complete("one", 0);
    assert_eq!(join(one).unwrap(), "one");
    wait_until(|| h.launcher.command_count() == 2);
    assert_eq!(h.command(1).control_path(), "none");
    h.command(1).complete("two", 0);
    assert_eq!(join(two).unwrap(), "two");
    assert_eq!(h.launcher.master_count(), 0);
}

fn assert_near(actual: u64, expected: u64) {
    assert!(
        actual.abs_diff(expected) <= 80,
        "{actual}ms != {expected}ms"
    );
}

/// 터미널 준비는 채널을, 둘째 검증 접속은 인증 차례를 기다렸다. 기준보다 빨리 끝난 첫 검증 접속은 남기지 않는다
fn assert_slow_command_logs(logs: &[SlowRemoteCommandLog]) {
    let terminal_log = logs
        .iter()
        .find(|log| log.command == "terminal-prepare")
        .unwrap();
    assert_eq!(
        (terminal_log.ssh_host.as_str(), terminal_log.priority),
        (HOST, "foreground")
    );
    assert_near(terminal_log.total_ms, 390);
    assert_near(terminal_log.slot_wait_ms, 240);
    assert_near(terminal_log.connection_wait_ms, 0);
    assert_near(terminal_log.command_ms, 150);
    let strict_log = logs
        .iter()
        .find(|log| log.command == "strict-read-2")
        .unwrap();
    assert_near(strict_log.slot_wait_ms, 0);
    assert_near(
        strict_log.connection_wait_ms,
        strict_log.total_ms - strict_log.command_ms,
    );
    assert_near(strict_log.command_ms, 200);
    assert!(strict_log.connection_wait_ms >= 200);
    assert!(!logs.iter().any(|log| log.command == "strict-read-1"));
}

#[test]
fn slow_commands_split_slot_connection_and_command_time_and_fast_ones_are_not_logged() {
    let mut settings = RemoteSshSettings {
        host_max_concurrency: 2,
        ..RemoteSshSettings::default()
    };
    settings.slow_command_log_threshold = Duration::from_millis(300);
    let h = harness_with(settings, system_clock());
    let warm_up = h.spawn_exec(HOST, "warm-up", ExecGitOptions::default(), false);
    wait_until(|| h.launcher.command_count() == 1);
    h.command(0).complete("warm", 0);
    assert_eq!(join(warm_up).unwrap(), "warm");

    // 호스트 키를 검증하는 접속 둘이 채널 두 개를 모두 잡는다. 둘째는 인증 차례를 기다리는 동안에도 채널을 쥐고 있다
    let first_strict = h.spawn_exec(HOST, "strict-read-1", strict(45_000), false);
    wait_until(|| h.launcher.command_count() == 2);
    let second_strict = h.spawn_exec(HOST, "strict-read-2", strict(45_000), false);
    settle();
    let terminal_preparation =
        h.spawn_exec(HOST, "terminal-prepare", ExecGitOptions::default(), false);
    wait_until(|| h.executor.queued_count(HOST) == 1);
    std::thread::sleep(Duration::from_millis(200));
    h.command(1).complete("one", 0);
    assert_eq!(join(first_strict).unwrap(), "one");
    // 채널이 비자 터미널 준비 명령과 둘째 검증 접속이 함께 시작한다
    wait_until(|| h.launcher.command_count() == 4);
    let find = |command: &str| {
        (0..4)
            .map(|index| h.command(index))
            .find(|child| child.remote_command().contains(command))
            .unwrap()
    };
    std::thread::sleep(Duration::from_millis(150));
    find("terminal-prepare").complete("prepared", 0);
    assert_eq!(join(terminal_preparation).unwrap(), "prepared");
    std::thread::sleep(Duration::from_millis(50));
    find("strict-read-2").complete("two", 0);
    assert_eq!(join(second_strict).unwrap(), "two");

    assert_slow_command_logs(&h.observer.slow_commands.lock().unwrap());
}

#[test]
fn a_completed_remote_application_error_is_not_a_transport_failure_or_replayed() {
    let h = harness();
    h.launcher.on_command(|child, _| {
        child.write_stderr("Connection reset by peer");
        child.complete("", 1);
    });

    assert!(
        h.exec("application", &retry_safe())
            .unwrap_err()
            .message
            .contains("Connection reset")
    );
    assert!(
        h.exec("application", &retry_safe())
            .unwrap_err()
            .message
            .contains("Connection reset")
    );
    assert_eq!(h.launcher.command_count(), 2);
}

#[test]
fn a_cold_callers_deadline_does_not_cancel_shared_master_preparation() {
    let h = harness();
    h.launcher.on_master(|_, _| {});
    h.launcher
        .on_command(|child, _| child.complete("healthy", 0));
    let short = h.spawn_exec(HOST, "short", timeout_options(20), false);
    wait_until(|| h.launcher.master_count() == 1);
    let long = h.spawn_exec(HOST, "long", timeout_options(500), false);

    assert_message_matches(&join(short).unwrap_err(), "완료되지 않았");
    assert_eq!(h.launcher.command_count(), 0);
    h.launcher.master(0).exit(0);
    assert_eq!(join(long).unwrap(), "healthy");
    assert_eq!(
        h.exec("following", &ExecGitOptions::default()).unwrap(),
        "healthy"
    );
    assert_eq!(h.launcher.master_count(), 1);
    assert_eq!(h.launcher.command_count(), 2);
}

#[test]
fn cold_nested_commands_in_an_authenticated_strict_responder_cannot_deadlock() {
    let h = harness_with_concurrency(1);
    h.launcher.on_command(|child, index| match index {
        0 => {
            child.on_stdin_end(|child| child.complete("committed", 0));
            child.write_stdout("READY\t\n");
        }
        _ => child.complete("read", 0),
    });
    let executor = Arc::clone(&h.executor);
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let options = ExecGitOptions {
        remote_stdin_responder: Some(responder(move |_| {
            assert_eq!(
                executor.exec("read", HOST, &timeout_options(100)).unwrap(),
                "read"
            );
            counted.fetch_add(1, Ordering::SeqCst);
            Ok("response\n".into())
        })),
        ..strict(200)
    };

    assert_eq!(h.exec("transaction", &options).unwrap(), "committed");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(h.launcher.master_count(), 1);
    assert_eq!(h.command(0).control_path(), "none");
    assert_ne!(h.command(1).control_path(), "none");
}

#[test]
fn a_stale_preparation_failure_cannot_poison_a_newer_recovery() {
    let h = harness();
    h.launcher.on_master(|master, index| match index {
        0 => {
            master.write_stderr("Connection reset by peer");
            master.close(255);
        }
        _ => master.exit(0),
    });
    h.launcher
        .on_command(|child, _| child.complete("recovered", 0));

    // 읽기가 먼저 실패를 보고 새 세대의 마스터로 복구한다
    assert_eq!(h.exec("read", &retry_safe()).unwrap(), "recovered");
    // 같은 첫 세대를 기다리던 변경 명령이 뒤늦게 실패를 처리해도 새 마스터를 지우거나 쿨다운을 걸지 않는다
    let stale_failure = exit_error(Some(255), None, "", "Connection reset by peer");
    let write_error = h
        .executor
        .judge_stale_master_failure(HOST, "mutation", stale_failure, 0);

    assert!(write_error.message.contains("Connection reset"));
    assert_eq!(write_error.retry_after_ms, None);
    assert_eq!(h.executor.cached_masters(), [(1, false)]);
    assert_eq!(
        h.exec("following", &ExecGitOptions::default()).unwrap(),
        "recovered"
    );
    assert_eq!(h.launcher.master_count(), 2);
    assert_eq!(h.launcher.command_count(), 2);
}

#[test]
fn a_permanent_master_authentication_failure_never_dispatches_or_replays() {
    let h = harness();
    h.launcher.on_master(|master, _| {
        master.write_stderr("Permission denied (publickey)");
        master.close(255);
    });

    assert!(
        h.exec("mutation", &retry_safe())
            .unwrap_err()
            .message
            .contains("Permission denied")
    );
    assert!(
        h.exec("following", &ExecGitOptions::default())
            .unwrap_err()
            .message
            .contains("Permission denied")
    );
    assert_eq!(h.launcher.master_count(), 2);
    assert_eq!(h.launcher.command_count(), 0);
}

#[test]
fn strict_callers_expire_while_queued_for_admission_and_are_never_dispatched() {
    let h = harness();
    let first = h.spawn_exec(HOST, "active", strict(500), false);
    wait_until(|| h.launcher.command_count() == 1);
    let second = h.spawn_exec(HOST, "expired", strict(20), false);

    assert_message_matches(&join(second).unwrap_err(), "완료되지 않았");
    h.command(0).complete("done", 0);
    assert_eq!(join(first).unwrap(), "done");
    assert_eq!(h.launcher.command_count(), 1);
}

#[test]
fn a_strict_responder_releases_authentication_before_joining_a_queued_master() {
    let h = harness();
    h.launcher.on_command(|child, index| match index {
        0 => child.on_stdin_end(|child| child.complete("committed", 0)),
        _ => child.complete("read", 0),
    });
    let executor = Arc::clone(&h.executor);
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let options = ExecGitOptions {
        remote_stdin_responder: Some(responder(move |_| {
            assert_eq!(
                executor
                    .exec("nested read", HOST, &timeout_options(300))
                    .unwrap(),
                "read"
            );
            counted.fetch_add(1, Ordering::SeqCst);
            Ok("response\n".into())
        })),
        ..strict(200)
    };
    let transaction = h.spawn_exec(HOST, "transaction", options, false);
    wait_until(|| h.launcher.command_count() == 1);
    let concurrent = h.spawn_exec(HOST, "background read", timeout_options(500), false);
    settle();

    assert_eq!(h.launcher.master_count(), 0);
    h.command(0).write_stdout("READY\t\n");
    assert_eq!(join(transaction).unwrap(), "committed");
    assert_eq!(join(concurrent).unwrap(), "read");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(h.launcher.master_count(), 1);
}

#[test]
fn a_master_failure_after_every_caller_left_does_not_trap_the_next_request() {
    let h = harness();
    h.launcher.on_master(|master, index| {
        if index > 0 {
            master.exit(0);
        }
    });
    h.launcher.on_command(|child, _| child.complete("fresh", 0));

    assert_message_matches(
        &h.exec("expired", &timeout_options(20)).unwrap_err(),
        "완료되지 않았",
    );
    let abandoned = h.launcher.master(0);
    abandoned.write_stderr("Permission denied (publickey)");
    abandoned.close(255);
    wait_until(|| h.executor.cached_masters() == [(0, true)]);

    assert_eq!(
        h.exec("fresh", &ExecGitOptions::default()).unwrap(),
        "fresh"
    );
    assert_eq!(h.launcher.master_count(), 2);
    assert_eq!(h.launcher.command_count(), 1);
}
