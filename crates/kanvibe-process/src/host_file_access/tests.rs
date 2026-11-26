//! TS `hostFileAccess.test.ts`. 원격 분기는 생성한 셸 명령을 이 머신의 sh로 실제 실행해 검증한다.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use super::*;
use crate::exec::testing::{LoopbackShellRunner, ScriptedCommandRunner};
use crate::exec::{RemoteStdinResponder, ShellCommandRunner};
use crate::worktree::quote_for_posix_shell;

const REMOTE_HOST: Option<&str> = Some("remote-host");

/// 사용자 HOME과 로그인 프로파일이 끼어들지 않도록 PATH와 임시 HOME만 주고 셸을 실행해 날 stdout을 돌려준다
fn run_shell(shell: &str, command: &str) -> Result<String, ExecGitError> {
    let home = std::env::temp_dir();
    let output = Command::new(shell)
        .arg("-c")
        .arg(command)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| ExecGitError::new(error.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        return Ok(stdout);
    }
    Err(ExecGitError {
        message: format!("exit {:?}: {command}", output.status.code()),
        stdout,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        ..ExecGitError::default()
    })
}

/// 실제 SSH 경로처럼 명령을 `sh -lc`로 두 번 감싸고 종료 코드 marker를 붙인 뒤, marker 앞까지를 돌려준다
fn run_like_remote_exec_git(command: &str) -> Result<String, ExecGitError> {
    let marker = "__KANVIBE_TEST_REMOTE_COMMAND_EXIT__";
    let wrapped_command = [
        format!("sh -c {}", quote_for_posix_shell(command)),
        "__kanvibe_status=$?".into(),
        format!("printf '\\n{marker}:%s\\n' \"$__kanvibe_status\""),
        "exit \"$__kanvibe_status\"".into(),
    ]
    .join("; ");
    let stdout = run_shell(
        "sh",
        &format!("sh -c {}", quote_for_posix_shell(&wrapped_command)),
    )?;
    let marker_start = stdout
        .rfind(&format!("\n{marker}:"))
        .unwrap_or(stdout.len());
    Ok(stdout[..marker_start].to_string())
}

fn files_with(runner: Arc<dyn ShellCommandRunner>) -> HostFileAccess {
    HostFileAccess::new(runner, "/nonexistent-home")
}

fn shell_files() -> (Arc<ScriptedCommandRunner>, HostFileAccess) {
    let runner = Arc::new(ScriptedCommandRunner::new(|call| {
        run_shell("sh", &call.command).map(|stdout| stdout.trim().to_string())
    }));
    (runner.clone(), files_with(runner))
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn file_mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// 원격 명령 앞에 셸 조각을 붙여 실행하고, 명령과 stdin 응답을 기록한다
struct PrefixedLoopbackRunner {
    prefix: String,
    inner: LoopbackShellRunner,
    commands: Mutex<Vec<String>>,
    replies: Arc<Mutex<Vec<String>>>,
}

impl PrefixedLoopbackRunner {
    fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
            inner: LoopbackShellRunner::new("/nonexistent-home"),
            commands: Mutex::new(Vec::new()),
            replies: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn without_chmod() -> Self {
        Self::new("umask 022; chmod() { :; }; ")
    }
}

impl ShellCommandRunner for PrefixedLoopbackRunner {
    fn exec_git(
        &self,
        command: &str,
        ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<String, ExecGitError> {
        self.commands.lock().unwrap().push(command.to_string());
        let mut recorded_options = options.clone();
        if let Some(responder) = options.remote_stdin_responder.clone() {
            let replies = Arc::clone(&self.replies);
            recorded_options.remote_stdin_responder = Some(RemoteStdinResponder {
                marker: responder.marker.clone(),
                respond: Arc::new(move |payload| {
                    let reply = (responder.respond)(payload)?;
                    replies.lock().unwrap().push(reply.clone());
                    Ok(reply)
                }),
            });
        }
        let prefixed_command = format!("{}{command}", self.prefix);
        self.inner
            .exec_git(&prefixed_command, ssh_host, &recorded_options)
    }
}

fn update_to(
    files: &HostFileAccess,
    target_path: &str,
    ssh_host: Option<&str>,
    next_content: &str,
) -> Result<(), HostFileError> {
    let mut update = |_: &str| Ok(next_content.to_string());
    files.update_text_file(
        target_path,
        ssh_host,
        Some(0o600),
        &ExecGitOptions::default(),
        &mut update,
    )
}

#[test]
fn only_a_missing_nested_path_is_reported_as_deleted() {
    let directory = tempfile::tempdir().unwrap();
    let (_, files) = shell_files();
    let missing = directory.path().join("repo's folder/worktree");

    for host in [None, REMOTE_HOST] {
        assert!(
            !files
                .is_path_missing(&path_text(directory.path()), host)
                .unwrap()
        );
        assert!(files.is_path_missing(&path_text(&missing), host).unwrap());
        assert!(!missing.exists());
    }
}

#[test]
fn a_broken_symbolic_link_is_not_treated_as_deleted() {
    let directory = tempfile::tempdir().unwrap();
    let (_, files) = shell_files();
    let link = directory.path().join("repo-link");
    std::os::unix::fs::symlink(directory.path().join("removed-target"), &link).unwrap();

    for host in [None, REMOTE_HOST] {
        assert!(!files.is_path_missing(&path_text(&link), host).unwrap());
    }
}

#[test]
fn a_denied_parent_directory_is_not_treated_as_deleted() {
    let directory = tempfile::tempdir().unwrap();
    let (_, files) = shell_files();
    let locked = directory.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    set_mode(&locked, 0o000);

    let results: Vec<bool> = [None, REMOTE_HOST]
        .into_iter()
        .map(|host| {
            files
                .is_path_missing(&path_text(&locked.join("repo")), host)
                .is_err()
        })
        .collect();

    set_mode(&locked, 0o700);
    assert_eq!(results, [true, true]);
}

#[test]
fn an_ssh_failure_is_not_treated_as_deleted() {
    let runner = Arc::new(ScriptedCommandRunner::new(|_| {
        Err(ExecGitError::new("SSH connection refused"))
    }));
    let files = files_with(runner);

    let error = files
        .is_path_missing("/workspace/repo", Some("offline-host"))
        .unwrap_err();

    assert!(error.to_string().contains("SSH connection refused"));
}

#[test]
fn remote_batch_read_command_runs_in_posix_sh() {
    let directory = tempfile::tempdir().unwrap();
    let file_path = path_text(&directory.path().join("hook file's config.txt"));
    let missing_path = path_text(&directory.path().join("missing.txt"));
    let content = "line 1\nline 2\n";
    std::fs::write(&file_path, content).unwrap();
    let (runner, files) = shell_files();

    let read = files
        .read_text_files(
            &[file_path.clone(), missing_path.clone()],
            REMOTE_HOST,
            &ExecGitOptions::default(),
        )
        .unwrap();

    assert_eq!(
        read[&file_path],
        TextFileReadResult {
            exists: true,
            content: content.into()
        }
    );
    assert_eq!(
        read[&missing_path],
        TextFileReadResult {
            exists: false,
            content: String::new()
        }
    );
    assert_eq!(runner.calls()[0].ssh_host.as_deref(), REMOTE_HOST);
    assert_eq!(std::fs::read_to_string(&file_path).unwrap(), content);
}

#[test]
fn remote_batch_read_command_survives_the_double_ssh_shell_wrapper() {
    let directory = tempfile::tempdir().unwrap();
    let file_path = path_text(&directory.path().join("hook file's config.txt"));
    let missing_path = path_text(&directory.path().join("missing file.txt"));
    let content = "line 1\nline 2\n";
    std::fs::write(&file_path, content).unwrap();
    let runner = Arc::new(ScriptedCommandRunner::new(|call| {
        run_like_remote_exec_git(&call.command).map(|stdout| stdout.trim().to_string())
    }));
    let files = files_with(runner);

    let read = files
        .read_text_files(
            &[file_path.clone(), missing_path.clone()],
            REMOTE_HOST,
            &ExecGitOptions::default(),
        )
        .unwrap();

    assert_eq!(
        read[&file_path],
        TextFileReadResult {
            exists: true,
            content: content.into()
        }
    );
    assert!(!read[&missing_path].exists);
}

#[test]
fn trimmed_ssh_output_keeps_an_empty_single_file() {
    for exists in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let file_path = path_text(&directory.path().join("empty.txt"));
        if exists {
            std::fs::write(&file_path, "").unwrap();
        }
        let (_, files) = shell_files();

        let read = files
            .read_text_files(
                std::slice::from_ref(&file_path),
                REMOTE_HOST,
                &ExecGitOptions::default(),
            )
            .unwrap();

        let expected = TextFileReadResult {
            exists,
            content: String::new(),
        };
        assert_eq!(read, BTreeMap::from([(file_path, expected)]));
    }
}

#[test]
fn remote_batch_read_keeps_the_last_manifest_file() {
    let directory = tempfile::tempdir().unwrap();
    let missing_path = path_text(&directory.path().join("missing.txt"));
    let final_file_path = path_text(&directory.path().join("hooks.json"));
    let final_content = r#"{"hooks":{"Stop":[{"command":"custom"}]}}"#;
    std::fs::write(&final_file_path, final_content).unwrap();
    let runner = Arc::new(ScriptedCommandRunner::new(|call| {
        run_shell("sh", &call.command)
    }));
    let files = files_with(runner);

    let read = files
        .read_text_files(
            &[missing_path.clone(), final_file_path.clone()],
            REMOTE_HOST,
            &ExecGitOptions::default(),
        )
        .unwrap();

    assert!(!read[&missing_path].exists);
    assert_eq!(read[&final_file_path].content, final_content);
}

#[test]
fn only_missing_local_files_are_absent_and_permission_errors_propagate() {
    let directory = tempfile::tempdir().unwrap();
    let missing_path = path_text(&directory.path().join("missing.txt"));
    let unreadable_path = directory.path().join("unreadable.txt");
    std::fs::write(&unreadable_path, "secret").unwrap();
    set_mode(&unreadable_path, 0o000);
    let (_, files) = shell_files();

    let read = files
        .read_text_files(
            std::slice::from_ref(&missing_path),
            None,
            &ExecGitOptions::default(),
        )
        .unwrap();
    let denied = files.read_text_files(
        &[path_text(&unreadable_path)],
        None,
        &ExecGitOptions::default(),
    );

    assert_eq!(
        read,
        BTreeMap::from([(
            missing_path,
            TextFileReadResult {
                exists: false,
                content: String::new()
            }
        )])
    );
    assert!(
        denied
            .unwrap_err()
            .to_string()
            .contains("Permission denied")
    );
}

#[test]
fn a_local_directory_read_failure_is_not_hidden_as_absence() {
    let directory = tempfile::tempdir().unwrap();
    let (_, files) = shell_files();

    let read = files.read_text_files(
        &[path_text(directory.path())],
        None,
        &ExecGitOptions::default(),
    );

    assert!(read.is_err());
}

#[test]
fn missing_or_corrupted_remote_records_are_not_treated_as_absent() {
    let outputs = [
        "",
        "__KANVIBE_FILE_RECORD__\t/remote/a\t1\t@@@\t__KANVIBE_FILE_RECORD_END__\n",
        "__KANVIBE_FILE_RECORD__\t/remote/a\t1\taGk=\t__KANVIBE_FILE_RECORD_END__\n",
        "__KANVIBE_FILE_RECORD__\t/remote/a\t2\t\t__KANVIBE_FILE_RECORD_END__\n__KANVIBE_FILE_RECORD__\t/remote/b\t0\t\t__KANVIBE_FILE_RECORD_END__\n",
        "__KANVIBE_FILE_RECORD__\t/remote/a\t0\t\tbroken\n__KANVIBE_FILE_RECORD__\t/remote/b\t0\t\t__KANVIBE_FILE_RECORD_END__\n",
    ];
    for output in outputs {
        let files = files_with(Arc::new(ScriptedCommandRunner::returning(output)));

        let read = files.read_text_files(
            &["/remote/a".into(), "/remote/b".into()],
            REMOTE_HOST,
            &ExecGitOptions::default(),
        );

        assert!(read.is_err(), "{output:?}");
    }
}

#[test]
fn an_unsearchable_remote_parent_is_not_hidden_as_missing() {
    let directory = tempfile::tempdir().unwrap();
    let blocked = directory.path().join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    let file_path = path_text(&blocked.join("config.txt"));
    std::fs::write(&file_path, "current").unwrap();
    set_mode(&blocked, 0o000);
    let runner = Arc::new(ScriptedCommandRunner::new(|call| {
        run_shell("sh", &call.command)
    }));

    let read =
        files_with(runner).read_text_files(&[file_path], REMOTE_HOST, &ExecGitOptions::default());

    set_mode(&blocked, 0o700);
    assert!(read.is_err());
}

#[test]
fn a_real_remote_base64_failure_is_not_hidden_as_success() {
    let directory = tempfile::tempdir().unwrap();
    let file_path = path_text(&directory.path().join("config.txt"));
    std::fs::write(&file_path, "current").unwrap();
    let runner = Arc::new(ScriptedCommandRunner::new(|call| {
        let fail_file_encoding = "base64() { case \"$*\" in \"-w 0 \"*) return 42;; *) if [ \"$#\" -eq 0 ]; then return 42; fi; command base64 \"$@\";; esac; }; ";
        run_shell("sh", &format!("{fail_file_encoding}{}", call.command))
    }));

    let read =
        files_with(runner).read_text_files(&[file_path], REMOTE_HOST, &ExecGitOptions::default());

    assert!(read.is_err());
}

#[test]
fn remote_chat_files_are_read_with_the_larger_output_limit() {
    let runner = Arc::new(ScriptedCommandRunner::returning("remote chat"));
    let files = files_with(runner.clone());

    let content = files
        .read_text_file(
            "/remote/chat.jsonl",
            REMOTE_HOST,
            &ExecGitOptions::default(),
        )
        .unwrap();

    let call = &runner.calls()[0];
    assert_eq!(content, "remote chat");
    assert_eq!(
        call.command,
        "test -f '/remote/chat.jsonl' && cat '/remote/chat.jsonl' || true"
    );
    assert_eq!(call.ssh_host.as_deref(), REMOTE_HOST);
    assert_eq!(call.options.max_buffer_bytes, Some(64 * 1024 * 1024));
    assert!(!call.options.should_check_host_key_strictly);
}

#[test]
fn a_remote_chat_read_failure_is_not_hidden_as_an_empty_file() {
    let runner = Arc::new(ScriptedCommandRunner::new(|_| {
        Err(ExecGitError::new("SSH command output exceeded maxBuffer"))
    }));

    let read = files_with(runner).read_text_file(
        "/remote/chat.jsonl",
        REMOTE_HOST,
        &ExecGitOptions::default(),
    );

    assert!(read.unwrap_err().to_string().contains("exceeded maxBuffer"));
}

#[test]
fn concurrent_updates_hold_the_lock_and_keep_every_change() {
    let directory = tempfile::tempdir().unwrap();
    let target_path = path_text(&directory.path().join("async-map.json"));
    let (_, files) = shell_files();

    std::thread::scope(|scope| {
        for letter in ["a", "b"] {
            let files = &files;
            let target_path = &target_path;
            scope.spawn(move || {
                let mut update = |content: &str| {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    Ok(format!("{content}{letter}"))
                };
                files
                    .update_text_file(
                        target_path,
                        None,
                        Some(0o600),
                        &ExecGitOptions::default(),
                        &mut update,
                    )
                    .unwrap();
            });
        }
    });

    let mut letters: Vec<char> = std::fs::read_to_string(&target_path)
        .unwrap()
        .chars()
        .collect();
    letters.sort();
    assert_eq!(letters, ['a', 'b']);
}

#[test]
fn credential_files_are_published_with_the_requested_mode_from_creation() {
    let directory = tempfile::tempdir().unwrap();
    let temporary_path = directory.path().join("targets.json.tmp");
    let target_path = directory.path().join("targets.json");
    let (_, files) = shell_files();

    file_update::create_exclusive_file(&path_text(&temporary_path), "{}", Some(0o600)).unwrap();
    update_to(&files, &path_text(&target_path), None, "{}").unwrap();

    assert_eq!(file_mode(&temporary_path), 0o600);
    assert_eq!(file_mode(&target_path), 0o600);
    assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "{}");
}

#[test]
fn updating_a_widely_readable_file_narrows_it_to_the_requested_mode() {
    let directory = tempfile::tempdir().unwrap();
    let target_path = directory.path().join("targets.json");
    std::fs::write(&target_path, "old").unwrap();
    set_mode(&target_path, 0o644);
    let (_, files) = shell_files();

    let mut update = |content: &str| Ok(format!("{content}-next"));
    files
        .update_text_file(
            &path_text(&target_path),
            None,
            Some(0o600),
            &ExecGitOptions::default(),
            &mut update,
        )
        .unwrap();

    assert_eq!(file_mode(&target_path), 0o600);
    assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "old-next");
}

#[test]
fn remote_updates_create_the_file_with_the_requested_mode_before_chmod() {
    let directory = tempfile::tempdir().unwrap();
    let target_path = directory.path().join("targets.json");
    let files = files_with(Arc::new(PrefixedLoopbackRunner::without_chmod()));

    update_to(
        &files,
        &path_text(&target_path),
        REMOTE_HOST,
        "{\"token\":\"secret\"}",
    )
    .unwrap();

    assert_eq!(file_mode(&target_path), 0o600);
    assert_eq!(
        std::fs::read_to_string(&target_path).unwrap(),
        "{\"token\":\"secret\"}"
    );
}

#[test]
fn remote_update_of_a_wide_file_with_large_content_keeps_content_and_mode() {
    let directory = tempfile::tempdir().unwrap();
    let target_path = directory.path().join("targets.json");
    std::fs::write(&target_path, "old").unwrap();
    set_mode(&target_path, 0o644);
    let large_content = "x".repeat(100 * 1024);
    let files = files_with(Arc::new(PrefixedLoopbackRunner::without_chmod()));

    update_to(
        &files,
        &path_text(&target_path),
        REMOTE_HOST,
        &large_content,
    )
    .unwrap();

    assert_eq!(file_mode(&target_path), 0o600);
    assert_eq!(
        std::fs::read_to_string(&target_path).unwrap(),
        large_content
    );
}

#[test]
fn remote_update_sends_content_through_stdin_not_the_command_line() {
    let directory = tempfile::tempdir().unwrap();
    let target_path = directory.path().join("targets.json");
    let secret_content = "{\"token\":\"secret-token\"}";
    let encoded_secret =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, secret_content);
    let runner = Arc::new(PrefixedLoopbackRunner::without_chmod());
    let files = files_with(runner.clone());

    update_to(
        &files,
        &path_text(&target_path),
        REMOTE_HOST,
        secret_content,
    )
    .unwrap();

    let commands = runner.commands.lock().unwrap().clone();
    assert!(
        !commands
            .iter()
            .any(|command| command.contains(&encoded_secret) || command.contains("secret-token"))
    );
    assert!(
        runner
            .replies
            .lock()
            .unwrap()
            .iter()
            .any(|reply| reply.contains(&encoded_secret))
    );
    assert_eq!(
        std::fs::read_to_string(&target_path).unwrap(),
        secret_content
    );
}

#[test]
fn remote_update_without_stdin_does_not_replace_the_existing_file() {
    let directory = tempfile::tempdir().unwrap();
    let target_path = directory.path().join("targets.json");
    std::fs::write(&target_path, "existing").unwrap();
    let runner = Arc::new(ScriptedCommandRunner::new(|call| {
        if call.command.contains("mv -f") {
            let without_stdin = run_shell("sh", &format!("{{ {}; }} < /dev/null", call.command));
            assert!(without_stdin.is_err());
            return Err(ExecGitError::new("publish failed"));
        }
        run_shell("sh", &call.command)
    }));

    let result = update_to(
        &files_with(runner),
        &path_text(&target_path),
        REMOTE_HOST,
        "next",
    );

    assert!(result.unwrap_err().to_string().contains("publish failed"));
    assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "existing");
    assert!(!Path::new(&format!("{}.lock", path_text(&target_path))).exists());
}

#[test]
fn remote_update_retries_when_the_lock_was_released_right_after_a_lost_race() {
    let directory = tempfile::tempdir().unwrap();
    let target_path = path_text(&directory.path().join("targets.json"));
    let lock_path = format!("{target_path}.lock");
    let lost_race_marker = path_text(&directory.path().join("lost-race"));
    let prefix = format!(
        "mkdir() {{ if [ \"$3\" = '{lock_path}' ] && [ ! -e '{lost_race_marker}' ]; then : > '{lost_race_marker}'; return 1; fi; command mkdir \"$@\"; }}; umask 022; "
    );
    let runner = Arc::new(PrefixedLoopbackRunner::new(&prefix));
    let files = files_with(runner.clone());

    update_to(&files, &target_path, REMOTE_HOST, "next").unwrap();

    assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "next");
    assert_eq!(runner.commands.lock().unwrap().len(), 2);
    assert!(!Path::new(&lock_path).exists());
}

#[test]
fn remote_update_keeps_whitespace_and_trailing_newlines_in_one_ssh_transaction() {
    let directory = tempfile::tempdir().unwrap();
    let target_path = directory.path().join("exact.txt");
    std::fs::write(&target_path, "  saved\n\n").unwrap();
    let runner = Arc::new(PrefixedLoopbackRunner::without_chmod());
    let files = files_with(runner.clone());
    let mut received = Vec::new();

    let mut update = |content: &str| {
        received.push(content.to_string());
        Ok(format!("{content}next\n"))
    };
    files
        .update_text_file(
            &path_text(&target_path),
            REMOTE_HOST,
            Some(0o600),
            &ExecGitOptions::default(),
            &mut update,
        )
        .unwrap();

    assert_eq!(received, ["  saved\n\n"]);
    assert_eq!(runner.commands.lock().unwrap().len(), 1);
    assert_eq!(
        std::fs::read_to_string(&target_path).unwrap(),
        "  saved\n\nnext\n"
    );
}

#[test]
fn an_8_mib_current_file_reaches_the_callback_and_is_replaced_by_a_smaller_result() {
    let directory = tempfile::tempdir().unwrap();
    let target_path = directory.path().join("large-current.txt");
    let current = "x".repeat(8 * 1024 * 1024);
    std::fs::write(&target_path, &current).unwrap();
    let files = files_with(Arc::new(PrefixedLoopbackRunner::without_chmod()));
    let mut call_count = 0;

    let mut update = |content: &str| {
        call_count += 1;
        assert!(content == current);
        Ok("small result".to_string())
    };
    files
        .update_text_file(
            &path_text(&target_path),
            REMOTE_HOST,
            Some(0o600),
            &ExecGitOptions::default(),
            &mut update,
        )
        .unwrap();

    assert_eq!(call_count, 1);
    assert_eq!(
        std::fs::read_to_string(&target_path).unwrap(),
        "small result"
    );
}

#[test]
fn write_if_absent_creates_parent_directories() {
    let directory = tempfile::tempdir().unwrap();
    let target_path = directory.path().join("nested/project.json");
    let (_, files) = shell_files();

    files
        .write_text_file_if_absent(&path_text(&target_path), "seed\n", None)
        .unwrap();

    assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "seed\n");
}

#[test]
fn write_if_absent_does_not_overwrite_an_existing_file() {
    let directory = tempfile::tempdir().unwrap();
    let target_path = directory.path().join("project.json");
    std::fs::write(&target_path, "authoritative\n").unwrap();
    let (_, files) = shell_files();

    files
        .write_text_file_if_absent(&path_text(&target_path), "seed\n", None)
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(&target_path).unwrap(),
        "authoritative\n"
    );
}

#[test]
fn remote_write_if_absent_runs_in_posix_sh_and_keeps_existing_files() {
    let directory = tempfile::tempdir().unwrap();
    let created_path = directory.path().join("created's file.json");
    let existing_path = directory.path().join("existing.json");
    std::fs::write(&existing_path, "authoritative\n").unwrap();
    let runner = Arc::new(ScriptedCommandRunner::new(|call| {
        run_like_remote_exec_git(&call.command)
    }));
    let files = files_with(runner);

    files
        .write_text_file_if_absent(&path_text(&created_path), "seed\n", REMOTE_HOST)
        .unwrap();
    files
        .write_text_file_if_absent(&path_text(&existing_path), "seed\n", REMOTE_HOST)
        .unwrap();

    assert_eq!(std::fs::read_to_string(&created_path).unwrap(), "seed\n");
    assert_eq!(
        std::fs::read_to_string(&existing_path).unwrap(),
        "authoritative\n"
    );
}

#[test]
fn remote_write_if_absent_keeps_a_file_another_writer_created_right_after_the_check() {
    let directory = tempfile::tempdir().unwrap();
    let target_path = path_text(&directory.path().join("project's state.json"));
    let quoted_target = quote_for_posix_shell(&target_path);
    let runner = Arc::new(ScriptedCommandRunner::new(move |call| {
        let barrier = format!(
            "function [() {{\n  builtin [ \"$@\"\n  result=$?\n  if builtin [ \"$1\" = \"-e\" ] && builtin [ \"$result\" -ne 0 ]; then\n    printf 'authoritative\\n' > {quoted_target}\n  fi\n  return \"$result\"\n}}\n"
        );
        run_shell("bash", &format!("{barrier}{}", call.command))
    }));

    files_with(runner)
        .write_text_file_if_absent(&target_path, "seed\n", Some("fixture-host"))
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(&target_path).unwrap(),
        "authoritative\n"
    );
}

#[test]
fn hook_file_reads_and_writes_pass_the_strict_option_to_the_ssh_boundary() {
    let runner = Arc::new(ScriptedCommandRunner::new(answer_strict_boundary_call));
    let files = files_with(runner.clone());
    let options = ExecGitOptions {
        should_check_host_key_strictly: true,
        ..ExecGitOptions::default()
    };
    let config_path = "/remote/config".to_string();
    let config_paths = std::slice::from_ref(&config_path);

    files
        .read_text_file(&config_path, REMOTE_HOST, &options)
        .unwrap();
    files
        .read_text_files(config_paths, REMOTE_HOST, &options)
        .unwrap();
    files
        .find_symbolic_link_path(config_paths, REMOTE_HOST, &options)
        .unwrap();
    files
        .write_text_file(&config_path, "content", REMOTE_HOST, &options)
        .unwrap();
    let mut update = |_: &str| Ok("content".to_string());
    files
        .update_text_file(
            &config_path,
            REMOTE_HOST,
            Some(0o600),
            &options,
            &mut update,
        )
        .unwrap();

    let calls = runner.calls();
    assert_eq!(calls.len(), 5);
    assert_eq!(
        calls[0].options.max_buffer_bytes,
        Some(REMOTE_TEXT_FILE_MAX_BUFFER_BYTES)
    );
    assert!(
        calls
            .iter()
            .all(|call| call.options.should_check_host_key_strictly)
    );
    assert!(
        calls
            .iter()
            .any(|call| call.options.remote_stdin_responder.is_some())
    );
}

/// 갱신 요청에는 빈 현재 내용으로 한 번 응답하고, 묶음 읽기에는 없는 파일 레코드를 돌려준다
fn answer_strict_boundary_call(
    call: &crate::exec::testing::RecordedCommand,
) -> Result<String, ExecGitError> {
    if let Some(responder) = &call.options.remote_stdin_responder {
        (responder.respond)("")?;
        return Ok("__KANVIBE_FILE_UPDATED__".into());
    }
    if call.command.starts_with("__kanvibe_manifest") {
        return Ok(
            "__KANVIBE_FILE_RECORD__\t/remote/config\t0\t\t__KANVIBE_FILE_RECORD_END__\n".into(),
        );
    }
    Ok(String::new())
}
