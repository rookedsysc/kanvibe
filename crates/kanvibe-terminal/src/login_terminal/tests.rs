use std::collections::BTreeMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kanvibe_ai::accounts::login_terminal::{
    LoginTerminalCallbacks, LoginTerminalRequest, LoginTerminalSpawner,
};

use super::PtyLoginTerminalSpawner;
use crate::pty::{
    NativePtySpawner, PtyEventCallbacks, PtyExit, PtyProcess, PtySpawnRequest, PtySpawner,
};

/// 받은 요청을 기록하고 콜백을 쥐어 두는 PTY 띄우개
#[derive(Default)]
struct RecordingPtySpawner {
    requests: Mutex<Vec<PtySpawnRequest>>,
    callbacks: Mutex<Vec<PtyEventCallbacks>>,
    writes: Arc<Mutex<Vec<String>>>,
}

struct RecordingPty {
    writes: Arc<Mutex<Vec<String>>>,
}

impl PtyProcess for RecordingPty {
    fn pid(&self) -> u32 {
        7
    }
    fn foreground_process_name(&self) -> Option<String> {
        None
    }
    fn columns(&self) -> u16 {
        80
    }
    fn rows(&self) -> u16 {
        24
    }
    fn write(&self, data: &str) -> Result<(), String> {
        self.writes.lock().unwrap().push(data.to_string());
        Ok(())
    }
    fn resize(&self, _columns: u16, _rows: u16) -> Result<(), String> {
        Ok(())
    }
    fn kill(&self) -> Result<(), String> {
        Ok(())
    }
}

impl PtySpawner for RecordingPtySpawner {
    fn spawn(
        &self,
        request: PtySpawnRequest,
        callbacks: PtyEventCallbacks,
    ) -> Result<Arc<dyn PtyProcess>, String> {
        self.requests.lock().unwrap().push(request);
        self.callbacks.lock().unwrap().push(callbacks);
        Ok(Arc::new(RecordingPty {
            writes: Arc::clone(&self.writes),
        }))
    }
}

fn login_request(environment: BTreeMap<String, String>) -> LoginTerminalRequest {
    LoginTerminalRequest {
        program: "claude".to_string(),
        args: vec!["/login".to_string()],
        terminal_name: "xterm-color".to_string(),
        columns: 100,
        rows: 30,
        working_directory: "/home/user/accounts/a".to_string(),
        environment,
    }
}

fn exit_collecting_callbacks() -> (
    LoginTerminalCallbacks,
    mpsc::Receiver<i32>,
    Arc<Mutex<String>>,
) {
    let (exit_sender, exit_receiver) = mpsc::channel();
    let exit_sender = Mutex::new(exit_sender);
    let output = Arc::new(Mutex::new(String::new()));
    let collected = Arc::clone(&output);
    let callbacks = LoginTerminalCallbacks {
        on_output: Box::new(move |data| collected.lock().unwrap().push_str(data)),
        on_exit: Box::new(move |exit_code| {
            let _ = exit_sender.lock().unwrap().send(exit_code);
        }),
    };
    (callbacks, exit_receiver, output)
}

#[test]
fn login_terminal_never_passes_server_runtime_variables_to_the_pty() {
    // Given: 로그인 CLI 환경에 서버 런타임 값이 섞여 들어온 경우
    let pty_spawner = Arc::new(RecordingPtySpawner::default());
    let spawner = PtyLoginTerminalSpawner::new(pty_spawner.clone());
    let environment = BTreeMap::from([
        ("HOME".to_string(), "/home/user/accounts/a".to_string()),
        ("PATH".to_string(), "/usr/bin".to_string()),
        ("PORT".to_string(), "9736".to_string()),
        ("HOST".to_string(), "0.0.0.0".to_string()),
        ("NODE_ENV".to_string(), "production".to_string()),
        (
            "KANVIBE_APP_DATA_DIR".to_string(),
            "/tmp/kanvibe".to_string(),
        ),
    ]);
    let (callbacks, _exits, _output) = exit_collecting_callbacks();

    // When
    spawner
        .spawn(login_request(environment), callbacks)
        .unwrap();

    // Then
    let request = pty_spawner.requests.lock().unwrap()[0].clone();
    assert_eq!(
        request.environment.keys().cloned().collect::<Vec<_>>(),
        vec!["HOME".to_string(), "PATH".to_string()]
    );
    assert_eq!(request.program, "claude");
    assert_eq!(request.args, vec!["/login".to_string()]);
    assert_eq!(request.terminal_name, "xterm-color");
    assert_eq!((request.columns, request.rows), (100, 30));
    assert_eq!(request.working_directory, "/home/user/accounts/a");
}

#[test]
fn login_terminal_forwards_output_exit_code_and_input() {
    // Given
    let pty_spawner = Arc::new(RecordingPtySpawner::default());
    let spawner = PtyLoginTerminalSpawner::new(pty_spawner.clone());
    let (callbacks, exits, output) = exit_collecting_callbacks();
    let terminal = spawner
        .spawn(login_request(BTreeMap::new()), callbacks)
        .unwrap();

    // When
    terminal.write("code\r").unwrap();
    let pty_callbacks = pty_spawner.callbacks.lock().unwrap().pop().unwrap();
    (pty_callbacks.on_output)("Paste code:");
    (pty_callbacks.on_exit)(PtyExit {
        exit_code: 2,
        signal: None,
        is_detached: false,
    });

    // Then
    assert_eq!(
        *pty_spawner.writes.lock().unwrap(),
        vec!["code\r".to_string()]
    );
    assert_eq!(*output.lock().unwrap(), "Paste code:");
    assert_eq!(exits.recv_timeout(Duration::from_secs(1)).unwrap(), 2);
}

#[test]
fn login_terminal_runs_on_a_native_pty() {
    // Given
    let directory = tempfile::tempdir().unwrap();
    let spawner = PtyLoginTerminalSpawner::new(Arc::new(NativePtySpawner));
    let (callbacks, exits, output) = exit_collecting_callbacks();
    let request = LoginTerminalRequest {
        program: "/bin/sh".to_string(),
        args: vec!["-c".to_string(), "printf 'login-ready'; exit 4".to_string()],
        working_directory: directory.path().to_string_lossy().into_owned(),
        ..login_request(BTreeMap::new())
    };

    // When
    spawner.spawn(request, callbacks).unwrap();
    let exit_code = exits.recv_timeout(Duration::from_secs(10)).unwrap();

    // Then
    assert_eq!(exit_code, 4);
    assert!(output.lock().unwrap().contains("login-ready"));
}
