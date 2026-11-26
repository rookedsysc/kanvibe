//! 원격 `tab.move`의 단독 SSH 터널 수명과 소켓 시간 제한. ssh는 PATH 맨 앞에 둔 가짜 프로그램이 대신한다.

use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::fixtures::{
    HerdrFixture, fixture, fixture_with_environment, install_adjustable_clock, session_list,
    wait_for_file, write_executable,
};

const SSH_CONFIG: &str =
    "Host remote\n  HostName remote.test\n  Port 2202\n  User user\n  IdentityFile /key\n";

/// 인자·환경 변수 이름·PID를 남긴 뒤 `behavior`대로 움직이는 가짜 ssh
fn write_fake_ssh(directory: &Path, behavior: &str) -> PathBuf {
    let record_path = directory.join("ssh-record");
    let script = format!(
        "#!/usr/bin/env python3\nimport json, os, socket, sys, time\nargs = sys.argv[1:]\nlocal = args[args.index('-L') + 1].split(':')[0]\nrecord = {record:?}\nopen(record + '.partial', 'w').write(json.dumps({{'args': args, 'env': sorted(os.environ), 'pid': os.getpid(), 'local': local}}))\nos.rename(record + '.partial', record)\n{behavior}\n",
        record = record_path.to_string_lossy()
    );
    write_executable(&directory.join("ssh"), &script);
    record_path
}

const SERVE_ONE_REQUEST: &str = "server = socket.socket(socket.AF_UNIX)\nserver.bind(local)\nserver.listen(1)\nconnection, _ = server.accept()\nconnection.recv(65536)\nconnection.sendall(b'{\"id\":\"kanvibe\",\"result\":{\"type\":\"ok\"}}\\n')\nconnection.close()\ntime.sleep(600)";

struct TunnelFixture {
    herdr: HerdrFixture,
    record_path: PathBuf,
    _fake_bin: TempDir,
    _temporary_root: TempDir,
}

fn tunnel_fixture(behavior: &str, extra_environment: &[(&str, &str)]) -> TunnelFixture {
    let fake_bin = TempDir::new().unwrap();
    let temporary_root = TempDir::new().unwrap();
    let record_path = write_fake_ssh(fake_bin.path(), behavior);
    let path = format!("{}:/usr/bin:/bin", fake_bin.path().to_string_lossy());
    let tmpdir = temporary_root.path().to_string_lossy().into_owned();
    let mut environment = vec![("PATH", path.as_str()), ("TMPDIR", tmpdir.as_str())];
    environment.extend_from_slice(extra_environment);
    let herdr = fixture_with_environment(&environment);
    std::fs::create_dir_all(herdr.home.path().join(".ssh")).unwrap();
    std::fs::write(herdr.home.path().join(".ssh/config"), SSH_CONFIG).unwrap();
    let sessions = session_list(
        json!([{ "name": "feature", "running": true, "socket_path": "/remote/config/herdr.sock" }]),
    );
    herdr.runner.respond(move |_, _| Some(Ok(sessions.clone())));
    TunnelFixture {
        herdr,
        record_path,
        _fake_bin: fake_bin,
        _temporary_root: temporary_root,
    }
}

fn is_process_gone(process_id: i64) -> bool {
    // SAFETY: 신호 0은 존재만 확인한다
    let result = unsafe { libc::kill(process_id as i32, 0) };
    result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

fn recorded_socket_directory(record: &Value) -> PathBuf {
    Path::new(record["local"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn requests_through_a_dedicated_tunnel_and_cleans_up_the_process_and_directory() {
    let tunnel = tunnel_fixture(
        SERVE_ONE_REQUEST,
        &[
            ("PORT", "4999"),
            ("HOST", "internal-host"),
            ("NODE_ENV", "test"),
            ("KANVIBE_INTERNAL_SECRET", "not-for-shell"),
        ],
    );

    tunnel
        .herdr
        .service
        .move_herdr_tab("feature", "w1:t2", 0, Some("remote"))
        .unwrap();

    let record: Value = serde_json::from_str(&wait_for_file(&tunnel.record_path)).unwrap();
    let arguments: Vec<&str> = record["args"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for expected in [
        "-i",
        "/key",
        "-p",
        "2202",
        "BatchMode=yes",
        "ControlMaster=no",
        "ControlPath=none",
        "ExitOnForwardFailure=yes",
    ] {
        assert!(arguments.contains(&expected), "{expected}");
    }
    assert_eq!(arguments.last(), Some(&"remote"));
    let environment_keys: Vec<&str> = record["env"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for key in ["PORT", "HOST", "NODE_ENV", "KANVIBE_INTERNAL_SECRET"] {
        assert!(!environment_keys.contains(&key), "{key}");
    }
    assert!(is_process_gone(record["pid"].as_i64().unwrap()));
    assert!(!recorded_socket_directory(&record).exists());
}

#[test]
fn tunnel_launch_failure_reaches_the_caller_and_removes_the_socket_directory() {
    let tunnel = tunnel_fixture("sys.stderr.write('ssh unavailable')\nsys.exit(255)", &[]);

    let error = tunnel
        .herdr
        .service
        .move_herdr_tab("feature", "w1:t1", 0, Some("remote"))
        .unwrap_err();

    assert!(error.to_string().contains("ssh unavailable"), "{error}");
    let record: Value = serde_json::from_str(&wait_for_file(&tunnel.record_path)).unwrap();
    assert!(!recorded_socket_directory(&record).exists());
}

#[test]
fn unresponsive_server_times_out_without_resending_the_mutation() {
    let herdr = fixture();
    let directory = TempDir::new().unwrap();
    let socket_path = directory.path().join("api");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let request_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&request_count);
    std::thread::spawn(move || {
        while let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0_u8; 1024];
            if std::io::Read::read(&mut stream, &mut buffer).is_ok_and(|read| read > 0) {
                counter.fetch_add(1, Ordering::SeqCst);
            }
            std::thread::sleep(Duration::from_secs(10));
        }
    });
    let sessions =
        session_list(json!([{ "name": "feature", "running": true, "socket_path": socket_path }]));
    herdr.runner.respond(move |_, _| Some(Ok(sessions.clone())));

    let error = herdr
        .service
        .move_herdr_tab("feature", "w1:t1", 0, None)
        .unwrap_err();

    assert!(error.to_string().contains("시간 초과"), "{error}");
    assert_eq!(request_count.load(Ordering::SeqCst), 1);
}

#[test]
fn tunnel_that_never_becomes_ready_times_out_and_cleans_up() {
    let mut tunnel = tunnel_fixture("time.sleep(600)", &[]);
    let clock_offset = install_adjustable_clock(&mut tunnel.herdr.service);

    let error = std::thread::scope(|scope| {
        let moving = scope.spawn(|| {
            tunnel
                .herdr
                .service
                .move_herdr_tab("feature", "w1:t1", 0, Some("remote"))
        });
        wait_for_file(&tunnel.record_path);
        clock_offset.store(31_000, Ordering::SeqCst);
        moving.join().unwrap().unwrap_err()
    });

    assert!(error.to_string().contains("시간 초과"), "{error}");
    let record: Value = serde_json::from_str(&wait_for_file(&tunnel.record_path)).unwrap();
    assert!(is_process_gone(record["pid"].as_i64().unwrap()));
    assert!(!recorded_socket_directory(&record).exists());
}
