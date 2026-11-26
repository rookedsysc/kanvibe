//! portable-pty로 띄우는 실제 PTY(TS `node-pty`). 자식은 요청의 환경만 받고(env_clear), 출력은 읽기 스레드가
//! UTF-8 조각 경계를 맞춰 넘기며, 종료는 따로 기다리는 스레드가 알린다.

use std::ffi::CStr;
use std::io::{Read, Write};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use kanvibe_process::exec::ssh_process::Utf8ChunkDecoder;
use portable_pty::{Child, ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::{PtyEventCallbacks, PtyExit, PtyProcess, PtySpawnRequest, PtySpawner};

/// 셸이 끝난 뒤 남은 출력을 읽기 스레드가 다 넘길 때까지 기다리는 상한. 백그라운드 자손이 PTY를 쥐고 있으면
/// 읽기가 끝나지 않으므로, 그때도 종료는 이 시간 뒤에 알린다
const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_millis(1_000);
const READ_BUFFER_BYTES: usize = 64 * 1024;
const INPUT_WRITER_CLOSED_MESSAGE: &str = "PTY input writer is closed";

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 앱이 한 번 만들어 레지스트리와 로그인 터미널에 넘긴다
#[derive(Debug, Default, Clone, Copy)]
pub struct NativePtySpawner;

impl PtySpawner for NativePtySpawner {
    fn spawn(
        &self,
        request: PtySpawnRequest,
        callbacks: PtyEventCallbacks,
    ) -> Result<Arc<dyn PtyProcess>, String> {
        let size = PtySize {
            rows: request.rows,
            cols: request.columns,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = native_pty_system()
            .openpty(size)
            .map_err(|error| error.to_string())?;
        let child = pair
            .slave
            .spawn_command(build_command(&request))
            .map_err(|error| error.to_string())?;
        drop(pair.slave);
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| error.to_string())?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| error.to_string())?;
        let process = Arc::new(NativePtyProcess {
            pid: child.process_id().unwrap_or(0),
            master: Mutex::new(pair.master),
            input: spawn_input_writer(writer),
            killer: Mutex::new(child.clone_killer()),
            size: Mutex::new((request.columns, request.rows)),
        });
        start_event_threads(reader, child, callbacks);
        Ok(process)
    }
}

/// 요청한 환경만 넘긴다. `TERM`은 터미널 이름으로 덮는다(node-pty `name`)
fn build_command(request: &PtySpawnRequest) -> CommandBuilder {
    let mut command = CommandBuilder::new(&request.program);
    command.args(&request.args);
    command.env_clear();
    for (key, value) in &request.environment {
        command.env(key, value);
    }
    command.env("TERM", &request.terminal_name);
    command.cwd(&request.working_directory);
    command
}

/// 출력은 읽기 스레드가, 종료는 기다리는 스레드가 알린다. 종료는 남은 출력을 넘긴 뒤에 알린다
fn start_event_threads(
    mut reader: Box<dyn Read + Send>,
    mut child: Box<dyn Child + Send + Sync>,
    callbacks: PtyEventCallbacks,
) {
    let callbacks = Arc::new(callbacks);
    let output_callbacks = Arc::clone(&callbacks);
    let (output_drained, wait_for_output_drain) = mpsc::channel::<()>();
    thread::spawn(move || {
        let mut decoder = Utf8ChunkDecoder::default();
        let mut buffer = vec![0u8; READ_BUFFER_BYTES];
        loop {
            let read_bytes = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read_bytes) => read_bytes,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                // 슬레이브가 모두 닫히면 Linux는 EIO로 끝을 알린다
                Err(_) => break,
            };
            let text = decoder.decode(&buffer[..read_bytes]);
            if !text.is_empty() {
                (output_callbacks.on_output)(&text);
            }
        }
        let _ = output_drained.send(());
    });
    thread::spawn(move || {
        let exit = match child.wait() {
            Ok(status) => PtyExit {
                exit_code: status.exit_code() as i32,
                signal: status.signal().and_then(signal_number),
                is_detached: false,
            },
            Err(_) => PtyExit {
                exit_code: 1,
                signal: None,
                is_detached: false,
            },
        };
        let _ = wait_for_output_drain.recv_timeout(OUTPUT_DRAIN_TIMEOUT);
        (callbacks.on_exit)(exit);
    });
}

/// PTY 입력은 쓰기 전용 스레드가 받은 순서대로 쓴다(node-pty `write` 큐). 자식이 입력을 읽지 않아 PTY 입력 버퍼가
/// 차도 호출한 스레드(GPUI 메인 스레드, 출력 읽기 스레드의 질의 응답)는 막히지 않는다. 쓰기가 실패하면 스레드를 끝낸다
fn spawn_input_writer(mut writer: Box<dyn Write + Send>) -> mpsc::Sender<Vec<u8>> {
    let (input, pending_inputs) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        for pending_input in pending_inputs {
            let written = writer
                .write_all(&pending_input)
                .and_then(|_| writer.flush());
            if written.is_err() {
                break;
            }
        }
    });
    input
}

/// portable-pty는 시그널을 `strsignal` 이름으로만 준다. 같은 이름의 번호로 되돌린다
fn signal_number(signal_name: &str) -> Option<i32> {
    (1..65).find(|&number| {
        // strsignal은 정적 버퍼의 NUL 종료 문자열을 돌려준다. 바로 복사해 쓴다
        let description = unsafe { libc::strsignal(number) };
        !description.is_null()
            && unsafe { CStr::from_ptr(description) }.to_string_lossy() == signal_name
    })
}

struct NativePtyProcess {
    pid: u32,
    master: Mutex<Box<dyn MasterPty + Send>>,
    /// 쓰기 전용 스레드로 가는 입력 줄
    input: mpsc::Sender<Vec<u8>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    size: Mutex<(u16, u16)>,
}

impl PtyProcess for NativePtyProcess {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn foreground_process_name(&self) -> Option<String> {
        let process_group = lock(&self.master).process_group_leader()?;
        read_process_name(process_group)
    }

    fn columns(&self) -> u16 {
        lock(&self.size).0
    }

    fn rows(&self) -> u16 {
        lock(&self.size).1
    }

    fn write(&self, data: &str) -> Result<(), String> {
        self.input
            .send(data.as_bytes().to_vec())
            .map_err(|_| INPUT_WRITER_CLOSED_MESSAGE.to_string())
    }

    fn resize(&self, columns: u16, rows: u16) -> Result<(), String> {
        let size = PtySize {
            rows,
            cols: columns,
            pixel_width: 0,
            pixel_height: 0,
        };
        lock(&self.master)
            .resize(size)
            .map_err(|error| error.to_string())?;
        *lock(&self.size) = (columns, rows);
        Ok(())
    }

    /// node-pty `kill()`처럼 SIGHUP을 보낸다
    fn kill(&self) -> Result<(), String> {
        lock(&self.killer).kill().map_err(|error| error.to_string())
    }
}

/// 앞에서 도는 프로세스 그룹 리더의 이름
#[cfg(target_os = "linux")]
fn read_process_name(process_id: libc::pid_t) -> Option<String> {
    let name = std::fs::read_to_string(format!("/proc/{process_id}/comm")).ok()?;
    Some(name.trim_end_matches('\n').to_string()).filter(|name| !name.is_empty())
}

/// macOS는 `/proc`이 없어 `ps`로 실행 파일 경로를 읽고 이름만 남긴다
#[cfg(not(target_os = "linux"))]
fn read_process_name(process_id: libc::pid_t) -> Option<String> {
    let output = std::process::Command::new("/bin/ps")
        .env_clear()
        .args(["-o", "comm=", "-p", &process_id.to_string()])
        .output()
        .ok()?;
    let command = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let name = kanvibe_domain::project::posix_basename(&command).to_string();
    Some(name).filter(|name| !name.is_empty())
}
