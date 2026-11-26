//! 미러 자식 프로세스를 실제 `sh`로 띄워 확인한다. tmux·ssh는 띄우지 않는다

use std::collections::BTreeMap;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use kanvibe_process::shell_environment::ShellPlatform;
use kanvibe_process::ssh_config::SshConfigReader;
use kanvibe_terminal::environment::TerminalHostEnvironment;

use crate::ports::MirrorProcessSpawner;
use crate::process::{SystemMirrorProcessSpawner, WatchedProcess};

/// 서버 런타임 값이 섞인 부모 환경에서 만든 실행기
fn spawner_with_server_environment() -> SystemMirrorProcessSpawner {
    let parent: BTreeMap<String, String> = [
        ("HOME", "/tmp"),
        ("PATH", "/usr/bin:/bin"),
        ("SHELL", "/bin/sh"),
        ("PORT", "9736"),
        ("HOST", "0.0.0.0"),
        ("NODE_ENV", "production"),
        ("KANVIBE_APP_DATA_DIR", "/tmp/kanvibe-app-data"),
        ("KANVIBE_QA_MODE", "1"),
        ("MIRROR_VISIBLE_MARKER", "kept"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value.to_string()))
    .collect();
    let environment =
        TerminalHostEnvironment::from_parent_environment(&parent, ShellPlatform::Other, "/tmp");
    SystemMirrorProcessSpawner::new(
        Arc::new(environment),
        Arc::new(SshConfigReader::new("/nonexistent-home")),
    )
}

fn read_all(command: &str) -> String {
    let mut process = spawner_with_server_environment()
        .spawn(command, None)
        .unwrap();
    let mut output = String::new();
    process.stdout.read_to_string(&mut output).unwrap();
    process.control.wait_for_exit();
    output
}

#[test]
fn server_runtime_variables_never_reach_mirror_child_processes() {
    let output = read_all("env");

    for key in [
        "PORT",
        "HOST",
        "NODE_ENV",
        "KANVIBE_APP_DATA_DIR",
        "KANVIBE_QA_MODE",
    ] {
        assert!(
            !output
                .lines()
                .any(|line| line.starts_with(&format!("{key}="))),
            "{key} leaked: {output}"
        );
    }
    assert!(
        output
            .lines()
            .any(|line| line == "MIRROR_VISIBLE_MARKER=kept")
    );
}

#[test]
fn child_stderr_is_drained_so_a_full_pipe_never_stalls_the_stream() {
    // 읽지 않는 파이프라면 64KB에서 차서 자식이 stderr 쓰기에 멈추고 stdout도 오지 않는다
    let output = read_all("head -c 262144 /dev/zero >&2; echo stream-alive");

    assert_eq!(output, "stream-alive\n");
}

#[test]
fn terminating_a_mirror_stops_its_whole_process_group() {
    let process = spawner_with_server_environment()
        .spawn("sleep 30 & wait", None)
        .unwrap();
    let watched = WatchedProcess::start(process, |_| {}, || {});

    watched.stop();

    assert!(watched.wait_for_exit(Duration::from_secs(5)));
}
