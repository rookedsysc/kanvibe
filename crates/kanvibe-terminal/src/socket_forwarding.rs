//! 원격 Unix 소켓을 SSH `-L`로 로컬 임시 소켓에 잇는다(TS `codexAppServer.ts`의 `withHostSocket`).
//! Codex app-server 제어 소켓처럼 원격에만 있는 소켓에 붙을 때 쓴다. 공유 ControlMaster를 쓰지 않는 별도 연결이라
//! 터널을 닫아도 다른 SSH 명령에 영향이 없다.

use std::fs::{self, DirBuilder};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use kanvibe_ai::codex_app_server::{ForwardedUnixSocket, UnixSocketForwarder};
use kanvibe_ai::{AiError, AiResult};
use kanvibe_process::ssh_config::{
    SshArgsOptions, SshConfigReader, build_ssh_args, kanvibe_ssh_connection_health_options,
};

use crate::environment::TerminalHostEnvironment;

const CONNECTION_TIMEOUT: Duration = Duration::from_millis(4_000);
const SOCKET_POLL_INTERVAL: Duration = Duration::from_millis(25);
const FORCE_KILL_DELAY: Duration = Duration::from_millis(1_000);
const TEMPORARY_DIRECTORY_PREFIX: &str = "kvc-";

pub struct SshUnixSocketForwarder {
    ssh_config: Arc<SshConfigReader>,
    environment: Arc<TerminalHostEnvironment>,
    /// 터널 소켓을 둘 임시 디렉터리의 부모(TS `os.tmpdir()`)
    temporary_root: PathBuf,
}

impl SshUnixSocketForwarder {
    pub fn new(
        ssh_config: Arc<SshConfigReader>,
        environment: Arc<TerminalHostEnvironment>,
        temporary_root: PathBuf,
    ) -> Self {
        Self {
            ssh_config,
            environment,
            temporary_root,
        }
    }

    fn build_tunnel_args(
        &self,
        ssh_host: &str,
        local_socket: &Path,
        remote_socket: &str,
    ) -> AiResult<Vec<String>> {
        let host = self
            .ssh_config
            .parse_ssh_config()
            .into_iter()
            .find(|entry| entry.host == ssh_host)
            .ok_or_else(|| AiError::message("CLI의 SSH 호스트를 찾지 못했습니다."))?;
        let options = SshArgsOptions {
            is_tty_disabled: true,
            connection_health: Some(kanvibe_ssh_connection_health_options()),
            ..SshArgsOptions::default()
        };
        let mut args = build_ssh_args(&host, &options);
        let forward = format!("{}:{remote_socket}", local_socket.display());
        let tunnel_options = [
            "-N",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ControlMaster=no",
            "-o",
            "ControlPath=none",
            "-L",
            &forward,
        ];
        let destination_index = args.len() - 1;
        args.splice(
            destination_index..destination_index,
            tunnel_options.iter().map(|option| option.to_string()),
        );
        Ok(args)
    }

    fn spawn_tunnel(&self, args: &[String]) -> AiResult<Child> {
        let mut child = Command::new("ssh")
            .args(args)
            .env_clear()
            .envs(&self.environment.shell_environment)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| AiError::message(error.to_string()))?;
        // stderr를 비워 두지 않으면 ssh가 경고를 쓰다 막힐 수 있다
        if let Some(mut stderr) = child.stderr.take() {
            thread::spawn(move || {
                let mut discarded = Vec::new();
                let _ = stderr.read_to_end(&mut discarded);
            });
        }
        Ok(child)
    }
}

impl UnixSocketForwarder for SshUnixSocketForwarder {
    fn forward(
        &self,
        ssh_host: &str,
        remote_socket_path: &str,
    ) -> AiResult<Box<dyn ForwardedUnixSocket>> {
        let directory = create_private_temporary_directory(&self.temporary_root)
            .map_err(|error| AiError::from_io(&error, &self.temporary_root.to_string_lossy()))?;
        let local_socket = directory.join("api");
        let tunnel = self
            .build_tunnel_args(ssh_host, &local_socket, remote_socket_path)
            .and_then(|args| self.spawn_tunnel(&args));
        let mut tunnel = match tunnel {
            Ok(child) => SshTunnel::new(child, directory, local_socket),
            Err(error) => {
                let _ = fs::remove_dir_all(&directory);
                return Err(error);
            }
        };
        tunnel.wait_for_socket()?;
        Ok(Box::new(tunnel))
    }
}

/// 열려 있는 SSH 터널. 버리면 ssh를 끝내고 임시 디렉터리를 지운다
struct SshTunnel {
    child: Child,
    directory: PathBuf,
    local_socket: PathBuf,
    local_path: String,
}

impl SshTunnel {
    fn new(child: Child, directory: PathBuf, local_socket: PathBuf) -> Self {
        let local_path = local_socket.to_string_lossy().into_owned();
        Self {
            child,
            directory,
            local_socket,
            local_path,
        }
    }

    /// ssh가 로컬 소켓을 만들 때까지 기다린다. 소켓 생성을 알려 주는 신호가 없어 TS와 같은 간격으로 확인한다
    fn wait_for_socket(&mut self) -> AiResult<()> {
        let deadline = Instant::now() + CONNECTION_TIMEOUT;
        while !is_socket(&self.local_socket) {
            let has_ended = !matches!(self.child.try_wait(), Ok(None));
            if has_ended || Instant::now() >= deadline {
                return Err(AiError::message("CLI의 Codex 소켓에 연결하지 못했습니다."));
            }
            thread::sleep(SOCKET_POLL_INTERVAL);
        }
        Ok(())
    }

    /// TS `tunnel.kill()`처럼 SIGTERM을 먼저 보내고, 1초 안에 끝나지 않으면 SIGKILL로 끝낸다
    fn stop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            // SAFETY: 이 구조체가 소유한 자식 프로세스에만 신호를 보낸다
            unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM) };
        }
        let deadline = Instant::now() + FORCE_KILL_DELAY;
        while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
            thread::sleep(SOCKET_POLL_INTERVAL);
        }
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

impl ForwardedUnixSocket for SshTunnel {
    fn local_path(&self) -> &str {
        &self.local_path
    }
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        self.stop();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn is_socket(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.file_type().is_socket())
}

/// `mkdtemp`처럼 이름이 겹치지 않는 0700 디렉터리를 만든다
fn create_private_temporary_directory(root: &Path) -> std::io::Result<PathBuf> {
    loop {
        let suffix = &uuid::Uuid::new_v4().simple().to_string()[..6];
        let directory = root.join(format!("{TEMPORARY_DIRECTORY_PREFIX}{suffix}"));
        match DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_process::shell_environment::ShellPlatform;

    fn forwarder_with_config(home: &Path, temporary_root: &Path) -> SshUnixSocketForwarder {
        let environment = TerminalHostEnvironment::from_parent_environment(
            &[("HOME".to_string(), home.to_string_lossy().into_owned())].into(),
            ShellPlatform::Other,
            "/",
        );
        SshUnixSocketForwarder::new(
            Arc::new(SshConfigReader::new(&home.to_string_lossy())),
            Arc::new(environment),
            temporary_root.to_path_buf(),
        )
    }

    #[test]
    fn tunnel_args_insert_the_socket_forward_before_the_destination() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".ssh")).unwrap();
        fs::write(
            home.path().join(".ssh/config"),
            "Host dev\n  HostName dev.example\n  User kanvibe\n",
        )
        .unwrap();
        let forwarder = forwarder_with_config(home.path(), home.path());

        let args = forwarder
            .build_tunnel_args(
                "dev",
                Path::new("/tmp/kvc-abc/api"),
                "/home/kanvibe/.codex/app.sock",
            )
            .unwrap();

        let tunnel_start = args.iter().position(|arg| arg == "-N").unwrap();
        assert_eq!(
            &args[tunnel_start..],
            &[
                "-N",
                "-o",
                "ExitOnForwardFailure=yes",
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=none",
                "-L",
                "/tmp/kvc-abc/api:/home/kanvibe/.codex/app.sock",
                "dev",
            ]
        );
        assert!(args.contains(&"-T".to_string()));
    }

    #[test]
    fn unknown_host_is_rejected_and_leaves_no_temporary_directory() {
        let home = tempfile::tempdir().unwrap();
        let temporary_root = tempfile::tempdir().unwrap();
        let forwarder = forwarder_with_config(home.path(), temporary_root.path());

        let error = forwarder
            .forward("missing", "/tmp/remote.sock")
            .err()
            .unwrap();

        assert_eq!(error.to_string(), "CLI의 SSH 호스트를 찾지 못했습니다.");
        assert_eq!(fs::read_dir(temporary_root.path()).unwrap().count(), 0);
    }

    #[test]
    fn temporary_directories_are_private() {
        let root = tempfile::tempdir().unwrap();

        let directory = create_private_temporary_directory(root.path()).unwrap();

        let mode = fs::metadata(&directory).unwrap().permissions();
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777,
            0o700
        );
        assert!(
            directory
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("kvc-")
        );
    }
}
