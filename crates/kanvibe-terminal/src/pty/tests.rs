//! 실제 PTY로 `/bin/sh`를 임시 디렉터리에서 띄워 본다. 멀티플렉서는 띄우지 않는다.

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;

const EVENT_TIMEOUT: Duration = Duration::from_secs(10);

/// 출력은 모아 두고 종료는 채널로 알린다
fn collecting_callbacks() -> (
    PtyEventCallbacks,
    Arc<Mutex<String>>,
    mpsc::Receiver<PtyExit>,
) {
    let output = Arc::new(Mutex::new(String::new()));
    let (exit_sender, exit_receiver) = mpsc::channel();
    let exit_sender = Mutex::new(exit_sender);
    let collected = Arc::clone(&output);
    let callbacks = PtyEventCallbacks {
        on_output: Box::new(move |data| collected.lock().unwrap().push_str(data)),
        on_exit: Box::new(move |exit| {
            let _ = exit_sender.lock().unwrap().send(exit);
        }),
    };
    (callbacks, output, exit_receiver)
}

fn shell_request(
    script: &str,
    working_directory: &str,
    environment: BTreeMap<String, String>,
) -> PtySpawnRequest {
    PtySpawnRequest {
        program: "/bin/sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        terminal_name: "xterm-256color".to_string(),
        columns: 80,
        rows: 24,
        working_directory: working_directory.to_string(),
        environment,
    }
}

#[test]
fn native_pty_runs_the_program_in_the_working_directory_and_reports_exit_after_output() {
    // Given
    let directory = tempfile::tempdir().unwrap();
    let (callbacks, output, exits) = collecting_callbacks();
    let request = shell_request(
        "printf 'cwd=%s\\n' \"$PWD\"; exit 3",
        directory.path().to_str().unwrap(),
        BTreeMap::new(),
    );

    // When
    let process = NativePtySpawner.spawn(request, callbacks).unwrap();
    let exit = exits.recv_timeout(EVENT_TIMEOUT).unwrap();

    // Then
    assert!(process.pid() > 0);
    assert_eq!(exit.exit_code, 3);
    assert!(!exit.is_detached);
    let expected_directory = directory.path().canonicalize().unwrap();
    assert!(
        output
            .lock()
            .unwrap()
            .contains(&format!("cwd={}", expected_directory.display())),
        "{}",
        output.lock().unwrap()
    );
}

#[test]
fn native_pty_passes_only_the_requested_environment_and_terminal_name() {
    // Given: 부모 프로세스 환경이 무엇이든 요청한 환경과 TERM만 자식에게 간다
    let directory = tempfile::tempdir().unwrap();
    let (callbacks, output, exits) = collecting_callbacks();
    let environment = BTreeMap::from([("KANVIBE_PTY_PROBE".to_string(), "visible".to_string())]);
    let script = "printf '[%s|%s|%s|%s]' \"$TERM\" \"${KANVIBE_PTY_PROBE-unset}\" \"${PORT-unset}\" \"${PATH-unset}\"";
    let request = shell_request(script, directory.path().to_str().unwrap(), environment);

    // When
    NativePtySpawner.spawn(request, callbacks).unwrap();
    exits.recv_timeout(EVENT_TIMEOUT).unwrap();

    // Then
    let output = output.lock().unwrap().clone();
    assert!(
        output.contains("[xterm-256color|visible|unset|"),
        "{output}"
    );
}

#[test]
fn native_pty_resize_updates_the_reported_size_and_accepts_input() {
    // Given
    let directory = tempfile::tempdir().unwrap();
    let (callbacks, output, exits) = collecting_callbacks();
    let request = shell_request(
        "read line; printf 'got=%s' \"$line\"",
        directory.path().to_str().unwrap(),
        BTreeMap::new(),
    );
    let process = NativePtySpawner.spawn(request, callbacks).unwrap();

    // When
    process.resize(100, 40).unwrap();
    process.write("hello\r").unwrap();
    exits.recv_timeout(EVENT_TIMEOUT).unwrap();

    // Then
    assert_eq!((process.columns(), process.rows()), (100, 40));
    assert!(
        output.lock().unwrap().contains("got=hello"),
        "{}",
        output.lock().unwrap()
    );
}

#[test]
fn native_pty_write_returns_without_waiting_for_a_child_that_does_not_read() {
    // Given: raw 모드라 줄 단위 버퍼가 없고 입력을 읽지 않는 자식. PTY 입력 버퍼가 곧 찬다
    let directory = tempfile::tempdir().unwrap();
    let (callbacks, _output, _exits) = collecting_callbacks();
    let request = shell_request(
        "stty raw -echo; sleep 30",
        directory.path().to_str().unwrap(),
        BTreeMap::new(),
    );
    let process = NativePtySpawner.spawn(request, callbacks).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let large_paste = "x".repeat(4 * 1024 * 1024);

    // When
    let started = std::time::Instant::now();
    let written = process.write(&large_paste);
    let elapsed = started.elapsed();
    process.kill().unwrap();

    // Then
    assert!(written.is_ok());
    assert!(
        elapsed < Duration::from_secs(2),
        "write blocked for {elapsed:?}"
    );
}

#[test]
fn native_pty_delivers_separate_writes_in_order() {
    // Given
    let directory = tempfile::tempdir().unwrap();
    let (callbacks, output, exits) = collecting_callbacks();
    let request = shell_request(
        "read line; printf 'got=%s' \"$line\"",
        directory.path().to_str().unwrap(),
        BTreeMap::new(),
    );
    let process = NativePtySpawner.spawn(request, callbacks).unwrap();

    // When
    for chunk in ["first-", "second-", "third\r"] {
        process.write(chunk).unwrap();
    }
    exits.recv_timeout(EVENT_TIMEOUT).unwrap();

    // Then
    assert!(
        output.lock().unwrap().contains("got=first-second-third"),
        "{}",
        output.lock().unwrap()
    );
}
