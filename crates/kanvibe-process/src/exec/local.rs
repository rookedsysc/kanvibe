//! 로컬 `sh -c` 실행. Node `child_process.exec`의 오류 형식과 기본 출력 상한(1 MiB)을 그대로 따른다.
//! 환경은 호출자가 만든 좁은 셸 환경만 넘기고 부모 프로세스 환경을 물려주지 않는다.

use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread::JoinHandle;

use super::{ExecGitError, ExecGitOptions};
use crate::shell_environment::ShellEnvironment;

/// TS `execLocal`은 maxBuffer를 넘기지 않으므로 Node `exec` 기본값 1 MiB가 적용된다
pub const LOCAL_EXEC_MAX_BUFFER_BYTES: usize = 1024 * 1024;

struct CapturedStream {
    bytes: Vec<u8>,
    has_exceeded_limit: bool,
}

/// 상한을 넘으면 프로세스를 죽여 무한 출력에도 멈춘다. Node도 maxBuffer를 넘기면 자식을 SIGTERM으로 끝낸다
fn capture_stream(
    mut stream: impl Read + Send + 'static,
    process_id: u32,
) -> JoinHandle<CapturedStream> {
    std::thread::spawn(move || {
        let mut captured = CapturedStream {
            bytes: Vec::new(),
            has_exceeded_limit: false,
        };
        let mut chunk = [0_u8; 8192];
        while let Ok(read_length) = stream.read(&mut chunk) {
            if read_length == 0 {
                break;
            }
            captured.bytes.extend_from_slice(&chunk[..read_length]);
            if captured.bytes.len() > LOCAL_EXEC_MAX_BUFFER_BYTES && !captured.has_exceeded_limit {
                captured.has_exceeded_limit = true;
                terminate_process(process_id);
            }
        }
        captured
    })
}

fn terminate_process(process_id: u32) {
    // SAFETY: 아직 회수하지 않은 자식 프로세스의 pid에만 신호를 보낸다
    unsafe {
        libc::kill(process_id as libc::pid_t, libc::SIGTERM);
    }
}

/// 시간 상한이 있으면 그 안에 끝나지 않은 프로세스를 SIGTERM으로 끝내고 `true`를 함께 돌려준다
fn wait_with_timeout(
    mut child: Child,
    options: &ExecGitOptions,
) -> (std::io::Result<ExitStatus>, bool) {
    let Some(timeout) = options.timeout else {
        return (child.wait(), false);
    };
    let process_id = child.id();
    let (sender, receiver) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let _ = sender.send(child.wait());
    });
    let (status, has_timed_out) = match receiver.recv_timeout(timeout) {
        Ok(status) => (status, false),
        Err(_) => {
            terminate_process(process_id);
            let status = receiver
                .recv()
                .unwrap_or_else(|_| Err(std::io::Error::other("wait failed")));
            (status, true)
        }
    };
    let _ = waiter.join();
    (status, has_timed_out)
}

fn signal_name(status: &ExitStatus) -> Option<&'static str> {
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

fn build_failure(
    command: &str,
    status: &ExitStatus,
    stdout: String,
    stderr: String,
) -> ExecGitError {
    ExecGitError {
        message: format!("Command failed: {command}\n{stderr}"),
        exit_code: status.code(),
        signal: signal_name(status),
        stdout,
        stderr,
        ..ExecGitError::default()
    }
}

/// 로컬 셸에서 명령을 실행하고 앞뒤 공백을 뺀 stdout을 돌려준다
pub fn exec_local(
    command: &str,
    environment: &ShellEnvironment,
    options: &ExecGitOptions,
) -> Result<String, ExecGitError> {
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| ExecGitError::new(format!("spawn /bin/sh ENOENT: {error}")))?;
    let process_id = child.id();
    let stdout_reader = capture_stream(child.stdout.take().expect("piped stdout"), process_id);
    let stderr_reader = capture_stream(child.stderr.take().expect("piped stderr"), process_id);

    let (status, has_timed_out) = wait_with_timeout(child, options);
    let stdout = stdout_reader.join().expect("stdout reader");
    let stderr = stderr_reader.join().expect("stderr reader");
    let status = status.map_err(|error| ExecGitError::new(error.to_string()))?;
    let stdout_text = String::from_utf8_lossy(&stdout.bytes).into_owned();
    let stderr_text = String::from_utf8_lossy(&stderr.bytes).into_owned();

    if stdout.has_exceeded_limit || stderr.has_exceeded_limit {
        let stream_name = if stdout.has_exceeded_limit {
            "stdout"
        } else {
            "stderr"
        };
        return Err(ExecGitError {
            message: format!("{stream_name} maxBuffer length exceeded"),
            error_code: Some("ERR_CHILD_PROCESS_STDIO_MAXBUFFER"),
            was_killed: true,
            stdout: stdout_text,
            stderr: stderr_text,
            ..ExecGitError::default()
        });
    }
    if has_timed_out || !status.success() {
        let mut failure = build_failure(command, &status, stdout_text, stderr_text);
        failure.was_killed = has_timed_out;
        return Err(failure);
    }
    Ok(stdout_text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn environment() -> ShellEnvironment {
        [("PATH".to_string(), "/usr/bin:/bin".to_string())].into()
    }

    #[test]
    fn returns_trimmed_stdout() {
        let output = exec_local(
            "printf '  hello\\n\\n'",
            &environment(),
            &ExecGitOptions::default(),
        );

        assert_eq!(output.unwrap(), "hello");
    }

    #[test]
    fn non_zero_exit_reports_node_style_failure() {
        let error = exec_local(
            "echo out; echo err >&2; exit 3",
            &environment(),
            &ExecGitOptions::default(),
        )
        .unwrap_err();

        assert_eq!(
            error.message,
            "Command failed: echo out; echo err >&2; exit 3\nerr\n"
        );
        assert_eq!(error.exit_code, Some(3));
        assert_eq!(error.stdout, "out\n");
        assert_eq!(error.stderr, "err\n");
    }

    #[test]
    fn timeout_kills_the_command() {
        let options = ExecGitOptions {
            timeout: Some(Duration::from_millis(100)),
            ..ExecGitOptions::default()
        };

        let error = exec_local("exec sleep 5", &environment(), &options).unwrap_err();

        assert!(error.was_killed);
        assert_eq!(error.signal, Some("SIGTERM"));
    }

    #[test]
    fn only_the_given_environment_reaches_the_command() {
        let output = exec_local(
            "printf '%s' \"${PORT-unset}\"",
            &environment(),
            &ExecGitOptions::default(),
        );

        assert_eq!(output.unwrap(), "unset");
    }
}
