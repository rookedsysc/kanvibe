//! 직접 띄운 로컬 sshd(임시 호스트 키·authorized_keys·known_hosts)로 바이너리 업로드, bridge, daemon 명령·PTY·hook,
//! 원격 터미널 agent 실행까지 실제 ssh 위에서 확인한다. 사용자 `~/.ssh` 설정·키·known_hosts·agent는 쓰지 않는다.
//!
//! 실행: `cargo build -p kanvibe-remote-agent` 뒤 `cargo nextest run -p kanvibe-remote-host --run-ignored only -E 'test(/sshd/)'`

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kanvibe_process::shell_environment::{ShellPlatform, create_current_local_shell_environment};
use kanvibe_process::ssh_config::SshHostConfig;
use kanvibe_remote_agent::bootstrap::AgentLaunchMode;
use kanvibe_remote_agent::protocol::current_uid;
use kanvibe_remote_host::agent_binaries::AgentBinaryCatalog;
use kanvibe_remote_host::connection::{HostExecuteOptions, HostTransport, HostTransportSpawner};
use kanvibe_remote_host::hook_tunnel::{HostHookResponse, SshHostLookup};
use kanvibe_remote_host::pool::{RemoteHostConnectionPool, RemoteHostConnectionPoolDependencies};
use kanvibe_remote_host::registry_bindings::RegistryHostConnections;
use kanvibe_remote_host::ssh_transport::{EffectiveSshConfig, build_host_ssh_args};
use kanvibe_remote_host::terminal_agent_seam::{
    RustRemoteTerminalAgentBundle, SelfSignedCertificateIssuer, UploadingRemoteTerminalAgentSpawner,
};
use kanvibe_terminal::environment::TerminalHostEnvironment;
use kanvibe_terminal::pty::PtyEventCallbacks;
use kanvibe_terminal::remote_terminal_service::{
    RemoteTerminalAgentBundle, RemoteTerminalAgentSpawner, RemoteTerminalCertificateIssuer,
    SystemRemoteTerminalAgentSpawner,
};
use kanvibe_terminal::terminal_registry::RemoteHostConnections;
use serde_json::{Value, json};

const HOST_ALIAS: &str = "kv-e2e";

/// 임시 sshd와 그 클라이언트 설정
struct SshdFixture {
    directory: tempfile::TempDir,
    sshd: Child,
    config: SshHostConfig,
}

impl Drop for SshdFixture {
    fn drop(&mut self) {
        let _ = self.sshd.kill();
        let _ = self.sshd.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn keygen(path: &Path) {
    let status = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

fn user_name() -> String {
    String::from_utf8(Command::new("id").arg("-un").output().unwrap().stdout)
        .unwrap()
        .trim()
        .to_string()
}

impl SshdFixture {
    fn start() -> Option<Self> {
        if !Path::new("/usr/sbin/sshd").exists() {
            eprintln!("sshd가 없어 건너뛴다");
            return None;
        }
        let directory = tempfile::Builder::new()
            .prefix("kv-sshd-")
            .tempdir()
            .unwrap();
        let path = |name: &str| directory.path().join(name);
        keygen(&path("host_key"));
        keygen(&path("client_key"));
        std::fs::copy(path("client_key.pub"), path("authorized_keys")).unwrap();
        let port = free_port();
        let host_key = std::fs::read_to_string(path("host_key.pub")).unwrap();
        std::fs::write(
            path("known_hosts"),
            format!("[127.0.0.1]:{port} {host_key}"),
        )
        .unwrap();
        let sshd_config = format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nAuthorizedKeysFile {}\nPidFile {}\nUsePAM no\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPubkeyAuthentication yes\nAllowUsers {}\n",
            path("host_key").display(),
            path("authorized_keys").display(),
            path("sshd.pid").display(),
            user_name(),
        );
        std::fs::write(path("sshd_config"), sshd_config).unwrap();
        let client_config = format!(
            "Host {HOST_ALIAS}\n  HostName 127.0.0.1\n  Port {port}\n  User {}\n  IdentityFile {}\n  IdentitiesOnly yes\n  IdentityAgent none\n  UserKnownHostsFile {}\n  GlobalKnownHostsFile /dev/null\n",
            user_name(),
            path("client_key").display(),
            path("known_hosts").display(),
        );
        std::fs::write(path("ssh_config"), client_config).unwrap();
        let sshd = Command::new("/usr/sbin/sshd")
            .args(["-D", "-e", "-f"])
            .arg(path("sshd_config"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let config = SshHostConfig {
            host: HOST_ALIAS.into(),
            hostname: "127.0.0.1".into(),
            port: i64::from(port),
            username: user_name(),
            private_key_path: path("client_key").display().to_string(),
        };
        let fixture = Self {
            directory,
            sshd,
            config,
        };
        fixture.wait_until_listening(port);
        Some(fixture)
    }

    fn wait_until_listening(&self, port: u16) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "sshd가 뜨지 않았다");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn ssh_config_file(&self) -> PathBuf {
        self.directory.path().join("ssh_config")
    }

    /// 사용자 설정 대신 임시 설정 파일만 읽도록 `-F`를 앞에 붙인 ssh 인자
    fn ssh_arguments(&self, remote_command: &str) -> Vec<String> {
        let mut arguments = vec![
            "-F".to_string(),
            self.ssh_config_file().display().to_string(),
        ];
        arguments.extend(build_host_ssh_args(&self.config));
        arguments.push(remote_command.to_string());
        arguments
    }
}

/// 운영 실행기와 같은 인자에 `-F <임시 설정>`만 더한 ssh 통로
struct FixtureTransportSpawner {
    ssh_config_file: PathBuf,
}

impl HostTransportSpawner for FixtureTransportSpawner {
    fn spawn(&self, config: &SshHostConfig, remote_command: &str) -> Result<HostTransport, String> {
        let mut child = Command::new("ssh")
            .arg("-F")
            .arg(&self.ssh_config_file)
            .args(build_host_ssh_args(config))
            .arg(remote_command)
            .env_clear()
            .envs(create_current_local_shell_environment())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| error.to_string())?;
        let (writer, reader) = (child.stdin.take().unwrap(), child.stdout.take().unwrap());
        let mut stderr = child.stderr.take().unwrap();
        let stderr_tail = Arc::new(Mutex::new(String::new()));
        let tail = Arc::clone(&stderr_tail);
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            *tail.lock().unwrap() = text;
        });
        let child = Arc::new(Mutex::new(child));
        Ok(HostTransport {
            writer: Box::new(writer),
            reader: Box::new(reader),
            terminate: Box::new(move || drop(child.lock().unwrap().kill())),
            stderr_tail,
        })
    }
}

struct FixtureEffectiveConfig {
    ssh_config_file: PathBuf,
}

impl EffectiveSshConfig for FixtureEffectiveConfig {
    fn resolve(&self, ssh_arguments: &[String]) -> Result<String, String> {
        let output = Command::new("ssh")
            .arg("-F")
            .arg(&self.ssh_config_file)
            .arg("-G")
            .args(ssh_arguments)
            .output()
            .map_err(|error| error.to_string())?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

struct FixtureHosts(SshHostConfig);

impl SshHostLookup for FixtureHosts {
    fn find_host(&self, ssh_host: &str) -> Option<SshHostConfig> {
        (ssh_host == self.0.host).then(|| self.0.clone())
    }
}

fn agent_binary_path() -> PathBuf {
    let executable = std::env::current_exe().unwrap();
    let profile_directory = executable.parent().and_then(Path::parent).unwrap();
    let binary = profile_directory.join("kanvibe-remote-agent");
    assert!(
        binary.is_file(),
        "먼저 `cargo build -p kanvibe-remote-agent`로 {}를 만든다",
        binary.display()
    );
    binary
}

fn platform_key() -> String {
    let uname = |flag: &str| {
        String::from_utf8(Command::new("uname").arg(flag).output().unwrap().stdout)
            .unwrap()
            .trim()
            .to_string()
    };
    format!("{}-{}", uname("-s"), uname("-m"))
}

#[test]
#[ignore = "로컬 sshd를 직접 띄우는 종단 테스트라 명시적으로만 돌린다"]
fn sshd_uploads_the_agent_and_serves_commands_pty_hooks_and_the_terminal_agent() {
    let Some(sshd) = SshdFixture::start() else {
        return;
    };
    let namespace = format!("qa-e2e-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let binaries = Arc::new(
        AgentBinaryCatalog::from_files(vec![(platform_key(), agent_binary_path())]).unwrap(),
    );
    let pool = RemoteHostConnectionPool::new(RemoteHostConnectionPoolDependencies {
        ssh_hosts: Arc::new(FixtureHosts(sshd.config.clone())),
        binaries: Arc::clone(&binaries),
        transport_spawner: Arc::new(FixtureTransportSpawner {
            ssh_config_file: sshd.ssh_config_file(),
        }),
        effective_ssh_config: Arc::new(FixtureEffectiveConfig {
            ssh_config_file: sshd.ssh_config_file(),
        }),
    });
    pool.configure_transport(&namespace, "e2e-client").unwrap();
    let namespace_directory = PathBuf::from(format!("/tmp/kv-{}/{namespace}", current_uid()));
    let cleanup = NamespaceCleanup(namespace_directory.clone());

    let connection = pool.connection(HOST_ALIAS).unwrap().expect("daemon 연결");
    assert_eq!(
        connection
            .execute("printf hello", &HostExecuteOptions::default())
            .unwrap(),
        "hello"
    );
    assert_pty_round_trip(&pool);
    assert_hook_round_trip(&connection, &namespace_directory);
    assert_terminal_agent_starts(&sshd, &binaries, &namespace);

    let instance = connection.instance_id.clone();
    pool.stop_connections();
    pool.configure_transport(&namespace, "e2e-client").unwrap();
    let again = pool
        .connection(HOST_ALIAS)
        .unwrap()
        .expect("다시 붙은 daemon 연결");
    assert_eq!(
        again.instance_id, instance,
        "SSH 연결이 바뀌어도 daemon은 그대로다"
    );
    again
        .send(json!({ "type": "stop", "id": "e2e-stop" }))
        .unwrap();
    drop(cleanup);
}

struct NamespaceCleanup(PathBuf);

impl Drop for NamespaceCleanup {
    fn drop(&mut self) {
        std::thread::sleep(Duration::from_millis(200));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn assert_pty_round_trip(pool: &Arc<RemoteHostConnectionPool>) {
    let connections = RegistryHostConnections(Arc::clone(pool));
    let connection = connections.connection(HOST_ALIAS).expect("레지스트리 연결");
    let (sender, outputs) = channel();
    let sender = Mutex::new(sender);
    let callbacks = PtyEventCallbacks {
        on_output: Box::new(move |text| drop(sender.lock().unwrap().send(text.to_string()))),
        on_exit: Box::new(|_| {}),
    };
    let pty = connection
        .open_pty(
            "e2e-tab",
            "printf 'E2E_%s\\n' PTY_OK; exec sleep 30",
            80,
            24,
            callbacks,
        )
        .unwrap();
    let mut output = String::new();
    while !output.contains("E2E_PTY_OK") {
        output.push_str(
            &outputs
                .recv_timeout(Duration::from_secs(10))
                .expect("PTY 출력"),
        );
    }
    assert!(pty.pid() > 0);
    pty.close_session().unwrap();
}

fn assert_hook_round_trip(
    connection: &Arc<kanvibe_remote_host::connection::HostConnection>,
    namespace_directory: &Path,
) {
    connection.set_hook_handler(Arc::new(|request| {
        assert_eq!(request.path, "/api/hooks/status");
        Ok(HostHookResponse {
            status: 200,
            headers: Vec::new(),
            body: b"e2e".to_vec(),
        })
    }));
    let mut stream = UnixStream::connect(namespace_directory.join("hooks-v1.sock")).unwrap();
    let body = "{\"taskId\":\"e2e\"}";
    let request = format!(
        "POST /api/hooks/status HTTP/1.1\r\nx-kanvibe-client-id: e2e-client\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.ends_with("e2e"), "{response}");
}

fn assert_terminal_agent_starts(
    sshd: &SshdFixture,
    binaries: &Arc<AgentBinaryCatalog>,
    namespace: &str,
) {
    let bundle = RustRemoteTerminalAgentBundle::new(Arc::clone(binaries), namespace);
    let loader = binaries
        .loader_script(namespace, AgentLaunchMode::TerminalAgent)
        .unwrap();
    assert_eq!(bundle.launch_command(), loader);
    let remote_command = format!(
        "sh -lc {}",
        kanvibe_process::worktree::quote_for_posix_shell(&loader)
    );
    let environment = Arc::new(TerminalHostEnvironment::from_parent_environment(
        &std::env::vars().collect(),
        ShellPlatform::current(),
        "/",
    ));
    let spawner = UploadingRemoteTerminalAgentSpawner::new(
        Arc::new(SystemRemoteTerminalAgentSpawner::new(environment)),
        Arc::clone(binaries),
    );
    let mut process = spawner
        .spawn(HOST_ALIAS, &sshd.ssh_arguments(&remote_command))
        .unwrap();
    let mut lines = BufReader::new(process.stdout);
    let mut line = String::new();
    lines.read_line(&mut line).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap()["type"],
        "bootstrap-ready"
    );
    let certificate = SelfSignedCertificateIssuer
        .issue(4_102_444_800_000)
        .unwrap();
    let bootstrap = json!({ "source": bundle.read_source().unwrap(), "certificate": { "cert": certificate.certificate_pem, "key": certificate.private_key_pem } });
    process
        .stdin
        .write_all(format!("{bootstrap}\n").as_bytes())
        .unwrap();
    line.clear();
    lines.read_line(&mut line).unwrap();
    let ready: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(ready["type"], "ready", "{line}");
    assert!(ready["port"].as_u64().is_some_and(|port| port > 0));
    process
        .stdin
        .write_all(b"{\"type\":\"shutdown\"}\n")
        .unwrap();
    process.termination.terminate();
}
