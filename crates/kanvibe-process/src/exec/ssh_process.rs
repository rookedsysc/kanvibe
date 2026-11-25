//! 시스템 `ssh` 프로세스를 띄우고 출력을 사건(event)으로 흘려보내는 경계.
//! 원격 실행 로직은 이 trait만 보므로, 테스트는 가짜 프로세스로 ssh 인자와 출력·종료 순서를 그대로 재현한다.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::ExecGitError;
use crate::shell_environment::ShellEnvironment;

/// Node `child_process`가 내보내던 사건. stdout·stderr는 UTF-8로 풀린 조각이다
#[derive(Debug, Clone)]
pub enum SshProcessEvent {
    Stdout(String),
    Stderr(String),
    /// 프로세스가 끝났다. `-f` 마스터는 stderr를 쥔 채 분기하므로 이 사건으로 준비 완료를 판정한다
    Exit(Option<i32>),
    /// 프로세스가 끝나고 출력 파이프도 모두 닫혔다
    Close {
        code: Option<i32>,
        signal: Option<&'static str>,
    },
    /// 프로세스를 띄우지 못했거나 실행 중 오류가 났다
    Error(ExecGitError),
}

/// 어떤 표준 입출력으로 띄울지. Node `stdio` 배열과 같다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshSpawnMode {
    /// `["ignore", "pipe", "pipe"]`
    CommandWithoutStdin,
    /// `["pipe", "pipe", "pipe"]`. 원격 stdin이나 stdin 응답기가 있을 때 쓴다
    CommandWithStdin,
    /// `["ignore", "ignore", "pipe"]`. `-N -f` 공용 연결 마스터
    Master,
}

pub type SshEventSink = Arc<dyn Fn(SshProcessEvent) + Send + Sync>;

/// 띄운 ssh 프로세스 하나
pub trait SshProcess: Send {
    /// stdin에 내용을 쓰고 닫는다. 내용이 없으면 닫기만 한다
    fn end_stdin(&mut self, content: Option<&str>);
    /// SIGTERM을 보낸다
    fn kill(&mut self);
}

pub trait SshProcessLauncher: Send + Sync {
    fn spawn(
        &self,
        args: &[String],
        mode: SshSpawnMode,
        environment: &ShellEnvironment,
        events: SshEventSink,
    ) -> Box<dyn SshProcess>;
}

/// 조각 경계에서 잘린 멀티바이트 문자를 다음 조각까지 기다렸다가 푸는 UTF-8 해독기(Node `StringDecoder`)
#[derive(Default)]
pub struct Utf8ChunkDecoder {
    pending: Vec<u8>,
}

impl Utf8ChunkDecoder {
    pub fn decode(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut text = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(valid) => {
                    text.push_str(valid);
                    self.pending.clear();
                    return text;
                }
                Err(error) => {
                    let valid_length = error.valid_up_to();
                    text.push_str(
                        std::str::from_utf8(&self.pending[..valid_length]).expect("검증된 구간"),
                    );
                    let Some(invalid_length) = error.error_len() else {
                        self.pending.drain(..valid_length);
                        return text;
                    };
                    text.push(char::REPLACEMENT_CHARACTER);
                    self.pending.drain(..valid_length + invalid_length);
                }
            }
        }
    }
}

/// 실제 `ssh` 바이너리를 띄우는 실행기
pub struct SystemSshLauncher;

struct SystemSshProcess {
    process_id: u32,
    stdin: Option<std::process::ChildStdin>,
    has_exited: Arc<AtomicBool>,
}

impl SshProcess for SystemSshProcess {
    fn end_stdin(&mut self, content: Option<&str>) {
        use std::io::Write;
        if let Some(mut stdin) = self.stdin.take() {
            // 원격이 입력을 다 읽기 전에 끝나면 EPIPE가 나지만, 실패 판정은 종료 코드가 맡는다
            let _ = stdin.write_all(content.unwrap_or_default().as_bytes());
        }
    }

    fn kill(&mut self) {
        if self.has_exited.load(Ordering::SeqCst) {
            return;
        }
        // SAFETY: 아직 회수하지 않은 자식 프로세스의 pid에만 신호를 보낸다
        unsafe {
            libc::kill(self.process_id as libc::pid_t, libc::SIGTERM);
        }
    }
}

/// 이미 실패를 알린 프로세스. stdin과 kill은 할 일이 없다
struct FailedSshProcess;

impl SshProcess for FailedSshProcess {
    fn end_stdin(&mut self, _content: Option<&str>) {}
    fn kill(&mut self) {}
}

fn forward_stream(
    mut stream: impl Read + Send + 'static,
    events: SshEventSink,
    wrap: fn(String) -> SshProcessEvent,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut decoder = Utf8ChunkDecoder::default();
        let mut chunk = [0_u8; 16 * 1024];
        while let Ok(read_length) = stream.read(&mut chunk) {
            if read_length == 0 {
                break;
            }
            let text = decoder.decode(&chunk[..read_length]);
            if !text.is_empty() {
                events(wrap(text));
            }
        }
    })
}

fn signal_name(status: &std::process::ExitStatus) -> Option<&'static str> {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map(|signal| match signal {
        libc::SIGTERM => "SIGTERM",
        libc::SIGKILL => "SIGKILL",
        libc::SIGINT => "SIGINT",
        libc::SIGHUP => "SIGHUP",
        libc::SIGPIPE => "SIGPIPE",
        _ => "SIGNAL",
    })
}

/// 종료되면 `Exit`를, 출력 파이프까지 모두 닫히면 `Close`를 보낸다
fn report_exit_and_close(
    mut child: std::process::Child,
    readers: Vec<std::thread::JoinHandle<()>>,
    has_exited: Arc<AtomicBool>,
    events: SshEventSink,
) {
    std::thread::spawn(move || {
        let status = child.wait();
        has_exited.store(true, Ordering::SeqCst);
        let (code, signal) = match &status {
            Ok(status) => (status.code(), signal_name(status)),
            Err(_) => (None, None),
        };
        events(SshProcessEvent::Exit(code));
        for reader in readers {
            let _ = reader.join();
        }
        events(SshProcessEvent::Close { code, signal });
    });
}

impl SshProcessLauncher for SystemSshLauncher {
    fn spawn(
        &self,
        args: &[String],
        mode: SshSpawnMode,
        environment: &ShellEnvironment,
        events: SshEventSink,
    ) -> Box<dyn SshProcess> {
        let stdin = if mode == SshSpawnMode::CommandWithStdin {
            Stdio::piped()
        } else {
            Stdio::null()
        };
        let stdout = if mode == SshSpawnMode::Master {
            Stdio::null()
        } else {
            Stdio::piped()
        };
        let spawned = Command::new("ssh")
            .args(args)
            .env_clear()
            .envs(environment)
            .stdin(stdin)
            .stdout(stdout)
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) => {
                events(SshProcessEvent::Error(ExecGitError::new(format!(
                    "spawn ssh {error}"
                ))));
                return Box::new(FailedSshProcess);
            }
        };
        let readers: Vec<_> =
            [
                child.stdout.take().map(|stream| {
                    forward_stream(stream, Arc::clone(&events), SshProcessEvent::Stdout)
                }),
                child.stderr.take().map(|stream| {
                    forward_stream(stream, Arc::clone(&events), SshProcessEvent::Stderr)
                }),
            ]
            .into_iter()
            .flatten()
            .collect();
        let has_exited = Arc::new(AtomicBool::new(false));
        let process = SystemSshProcess {
            process_id: child.id(),
            stdin: child.stdin.take(),
            has_exited: Arc::clone(&has_exited),
        };
        report_exit_and_close(child, readers, has_exited, events);
        Box::new(process)
    }
}

/// 테스트용 가짜 ssh 실행기. 띄운 인자와 방식을 기록하고, 프로세스의 출력·종료는 테스트가 직접 낸다
#[cfg(test)]
pub(crate) mod fake {
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    use super::*;

    type StdinHook = Box<dyn Fn(&FakeSshProcess) + Send + Sync>;

    pub(crate) struct FakeSshProcessState {
        events: SshEventSink,
        pub args: Vec<String>,
        pub mode: SshSpawnMode,
        pub stdin_writes: Mutex<Vec<Option<String>>>,
        pub kill_count: AtomicUsize,
        on_stdin_end: Mutex<Option<StdinHook>>,
    }

    /// 테스트가 쥐고 출력을 흘려보내는 프로세스 손잡이
    #[derive(Clone)]
    pub(crate) struct FakeSshProcess(pub Arc<FakeSshProcessState>);

    impl FakeSshProcess {
        pub fn write_stdout(&self, chunk: &str) {
            (self.0.events)(SshProcessEvent::Stdout(chunk.into()));
        }

        pub fn write_stderr(&self, chunk: &str) {
            (self.0.events)(SshProcessEvent::Stderr(chunk.into()));
        }

        pub fn exit(&self, code: i32) {
            (self.0.events)(SshProcessEvent::Exit(Some(code)));
        }

        pub fn close(&self, code: i32) {
            (self.0.events)(SshProcessEvent::Exit(Some(code)));
            (self.0.events)(SshProcessEvent::Close {
                code: Some(code),
                signal: None,
            });
        }

        pub fn emit_error(&self, error: ExecGitError) {
            (self.0.events)(SshProcessEvent::Error(error));
        }

        /// 원격 명령이 종료 코드 marker를 찍고 끝난 것처럼 출력한다
        pub fn complete(&self, stdout: &str, exit_code: i32) {
            self.write_stdout(&format!(
                "{stdout}\n{}:{exit_code}\n",
                super::super::remote_command::REMOTE_COMMAND_EXIT_MARKER
            ));
        }

        /// stdin이 닫힐 때 부를 동작. 응답을 받은 원격 셸이 이어서 출력하는 상황을 만든다
        pub fn on_stdin_end(&self, hook: impl Fn(&FakeSshProcess) + Send + Sync + 'static) {
            *self.0.on_stdin_end.lock().unwrap() = Some(Box::new(hook));
        }

        pub fn stdin_writes(&self) -> Vec<Option<String>> {
            self.0.stdin_writes.lock().unwrap().clone()
        }

        pub fn kill_count(&self) -> usize {
            self.0.kill_count.load(Ordering::SeqCst)
        }

        pub fn remote_command(&self) -> String {
            self.0.args.last().cloned().unwrap_or_default()
        }

        pub fn control_path(&self) -> String {
            self.0
                .args
                .iter()
                .find_map(|arg| arg.strip_prefix("ControlPath="))
                .unwrap_or_default()
                .to_string()
        }
    }

    impl SshProcess for FakeSshProcess {
        fn end_stdin(&mut self, content: Option<&str>) {
            self.0
                .stdin_writes
                .lock()
                .unwrap()
                .push(content.map(String::from));
            let hook = self.0.on_stdin_end.lock().unwrap().take();
            if let Some(hook) = hook {
                hook(self);
            }
        }

        fn kill(&mut self) {
            self.0.kill_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    type SpawnBehavior = Box<dyn Fn(&FakeSshProcess, usize) + Send + Sync>;

    /// 명령 프로세스와 마스터 프로세스를 따로 기록한다. 동작은 띄운 순번과 함께 받는다
    pub(crate) struct FakeSshLauncher {
        pub commands: Mutex<Vec<FakeSshProcess>>,
        pub masters: Mutex<Vec<FakeSshProcess>>,
        command_behavior: Mutex<SpawnBehavior>,
        master_behavior: Mutex<SpawnBehavior>,
    }

    impl FakeSshLauncher {
        /// 마스터는 곧바로 준비되고 명령 프로세스는 테스트가 끝낼 때까지 열려 있다
        pub fn new() -> Arc<Self> {
            Arc::new(Self {
                commands: Mutex::new(Vec::new()),
                masters: Mutex::new(Vec::new()),
                command_behavior: Mutex::new(Box::new(|_, _| {})),
                master_behavior: Mutex::new(Box::new(|master, _| master.exit(0))),
            })
        }

        pub fn on_command(
            &self,
            behavior: impl Fn(&FakeSshProcess, usize) + Send + Sync + 'static,
        ) {
            *self.command_behavior.lock().unwrap() = Box::new(behavior);
        }

        pub fn on_master(&self, behavior: impl Fn(&FakeSshProcess, usize) + Send + Sync + 'static) {
            *self.master_behavior.lock().unwrap() = Box::new(behavior);
        }

        pub fn command(&self, index: usize) -> FakeSshProcess {
            self.commands.lock().unwrap()[index].clone()
        }

        pub fn command_count(&self) -> usize {
            self.commands.lock().unwrap().len()
        }

        pub fn master_count(&self) -> usize {
            self.masters.lock().unwrap().len()
        }

        pub fn master(&self, index: usize) -> FakeSshProcess {
            self.masters.lock().unwrap()[index].clone()
        }
    }

    impl SshProcessLauncher for FakeSshLauncher {
        fn spawn(
            &self,
            args: &[String],
            mode: SshSpawnMode,
            _environment: &ShellEnvironment,
            events: SshEventSink,
        ) -> Box<dyn SshProcess> {
            let process = FakeSshProcess(Arc::new(FakeSshProcessState {
                events,
                args: args.to_vec(),
                mode,
                stdin_writes: Mutex::new(Vec::new()),
                kill_count: AtomicUsize::new(0),
                on_stdin_end: Mutex::new(None),
            }));
            let (registry, behavior) = match mode {
                SshSpawnMode::Master => (&self.masters, &self.master_behavior),
                _ => (&self.commands, &self.command_behavior),
            };
            let index = {
                let mut registry = registry.lock().unwrap();
                registry.push(process.clone());
                registry.len() - 1
            };
            (behavior.lock().unwrap())(&process, index);
            Box::new(process)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    // parity: TS-lib-gitOperations-5ee9d9
    #[test]
    fn multibyte_characters_split_across_chunks_are_decoded_once_whole() {
        let mut decoder = Utf8ChunkDecoder::default();
        let emoji = "🙂".as_bytes();

        let first = decoder.decode(&emoji[..2]);
        let empty = decoder.decode(&[]);
        let last = decoder.decode(&emoji[2..]);

        assert_eq!(
            (first.as_str(), empty.as_str(), last.as_str()),
            ("", "", "🙂")
        );
        assert_eq!(last.len(), 4);
    }

    #[test]
    fn invalid_bytes_become_replacement_characters() {
        let mut decoder = Utf8ChunkDecoder::default();

        assert_eq!(decoder.decode(b"a\xffb"), "a\u{FFFD}b");
    }

    #[test]
    fn system_launcher_reports_output_exit_and_close_in_order() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let sender = Mutex::new(sender);
        let sink: SshEventSink = Arc::new(move |event| {
            let _ = sender.lock().unwrap().send(event);
        });
        let environment: ShellEnvironment =
            [("PATH".to_string(), "/usr/bin:/bin".to_string())].into();

        let _process = SystemSshLauncher.spawn(
            &["-V".into()],
            SshSpawnMode::CommandWithoutStdin,
            &environment,
            sink,
        );

        let mut has_exited = false;
        while let Ok(event) = receiver.recv_timeout(std::time::Duration::from_secs(10)) {
            match event {
                SshProcessEvent::Exit(_) => has_exited = true,
                SshProcessEvent::Close { .. } => break,
                SshProcessEvent::Error(_) => return,
                _ => {}
            }
        }
        assert!(has_exited);
    }
}
