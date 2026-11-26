use std::io::{BufRead, BufReader, Cursor};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use kanvibe_terminal::remote_terminal_service::RemoteTerminalAgentTermination;
use serde_json::json;

use super::*;

fn catalog_with(entries: &[(&str, &[u8])]) -> (tempfile::TempDir, Arc<AgentBinaryCatalog>) {
    let directory = tempfile::tempdir().unwrap();
    let files: Vec<(String, PathBuf)> = entries
        .iter()
        .map(|(platform, bytes)| {
            let path = directory
                .path()
                .join(format!("kanvibe-remote-agent-{platform}"));
            std::fs::write(&path, bytes).unwrap();
            (platform.to_string(), path)
        })
        .collect();
    let catalog = Arc::new(AgentBinaryCatalog::from_files(files).unwrap());
    (directory, catalog)
}

#[test]
fn an_issued_certificate_is_served_by_the_rust_terminal_agent() {
    let certificate = SelfSignedCertificateIssuer
        .issue(4_102_444_800_000)
        .unwrap();
    let (owner_input, agent_input) = UnixStream::pair().unwrap();
    let (agent_output, owner_output) = UnixStream::pair().unwrap();

    let agent = kanvibe_remote_agent::terminal_agent::start_terminal_agent(
        &certificate.certificate_pem,
        &certificate.private_key_pem,
        Box::new(agent_input),
        Box::new(agent_output),
    )
    .unwrap();
    let mut ready = String::new();
    BufReader::new(owner_output).read_line(&mut ready).unwrap();

    let ready: Value = serde_json::from_str(&ready).unwrap();
    assert_eq!(ready["type"], "ready");
    assert_eq!(ready["port"].as_u64(), Some(u64::from(agent.port())));
    assert_eq!(
        ready["certificateFingerprint"].as_str().map(str::len),
        Some(64)
    );
    agent.stop();
    drop(owner_input);
}

#[test]
fn the_bundle_source_lists_every_known_binary_hash() {
    let (_directory, catalog) = catalog_with(&[("Linux-x86_64", b"x64"), ("Darwin-arm64", b"arm")]);
    let bundle = RustRemoteTerminalAgentBundle::new(Arc::clone(&catalog), "dev");

    let hashes: Vec<String> = catalog
        .digests()
        .into_iter()
        .map(|digest| digest.sha256)
        .collect();

    assert_eq!(bundle.read_source().unwrap(), hashes.join(","));
    assert!(bundle.launch_command().contains("terminal-agent dev"));
}

/// 첫 실행은 업로드를 요구하고, 두 번째 실행은 agent가 뜨는 ssh 흉내
struct ScriptedSpawner {
    launches: AtomicUsize,
    uploaded: Arc<Mutex<Vec<u8>>>,
    terminations: Arc<AtomicUsize>,
}

struct CapturedStdin(Arc<Mutex<Vec<u8>>>);

impl Write for CapturedStdin {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct CountedTermination(Arc<AtomicUsize>);

impl RemoteTerminalAgentTermination for CountedTermination {
    fn terminate(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

impl RemoteTerminalAgentSpawner for ScriptedSpawner {
    fn spawn(
        &self,
        _ssh_host: &str,
        _ssh_arguments: &[String],
    ) -> Result<RemoteTerminalAgentProcess, String> {
        let stdout = match self.launches.fetch_add(1, Ordering::SeqCst) {
            0 => {
                "{\"type\":\"upload-required\",\"platform\":\"Linux-x86_64\"}\n{\"type\":\"uploaded\"}\n"
            }
            _ => "{\"type\":\"bootstrap-ready\"}\n{\"type\":\"ready\"}\n",
        };
        Ok(RemoteTerminalAgentProcess {
            stdin: Box::new(CapturedStdin(Arc::clone(&self.uploaded))),
            stdout: Box::new(Cursor::new(stdout.as_bytes().to_vec())),
            termination: Box::new(CountedTermination(Arc::clone(&self.terminations))),
        })
    }
}

#[test]
fn a_missing_remote_binary_is_uploaded_and_the_agent_is_launched_again() {
    let (_directory, catalog) = catalog_with(&[("Linux-x86_64", b"agent bytes")]);
    let inner = Arc::new(ScriptedSpawner {
        launches: AtomicUsize::new(0),
        uploaded: Arc::default(),
        terminations: Arc::default(),
    });
    let spawner = UploadingRemoteTerminalAgentSpawner::new(inner.clone(), catalog);

    let process = spawner.spawn("fixture", &["fixture".into()]).unwrap();

    let lines: Vec<String> = BufReader::new(process.stdout)
        .lines()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        lines,
        vec![
            "{\"type\":\"bootstrap-ready\"}".to_string(),
            "{\"type\":\"ready\"}".to_string()
        ]
    );
    assert_eq!(*inner.uploaded.lock().unwrap(), b"agent bytes");
    assert_eq!(inner.launches.load(Ordering::SeqCst), 2);
    assert_eq!(inner.terminations.load(Ordering::SeqCst), 1);
}

/// 정적 바이너리는 kanvibe-terminal을 쓰지 못해 같은 규칙을 따로 갖는다. 두 구현이 같은 결과를 내는지 고정한다
#[test]
fn the_agent_wire_helpers_match_the_desktop_protocol() {
    use kanvibe_remote_agent::terminal_agent::wire as agent_wire;
    use kanvibe_terminal::remote_terminal_protocol as desktop_wire;

    for screen in ["", "line\n", "first\nsecond\n\nthird", "한글\r\n🌊"] {
        assert_eq!(
            agent_wire::build_wheel_delegated_screen_redraw(screen),
            desktop_wire::build_wheel_delegated_screen_redraw(screen)
        );
    }
    let scrolls = [
        json!({ "type": "scroll", "direction": "up", "column": 4, "row": 2 }),
        json!({ "type": "scroll", "direction": "down", "column": 0, "row": 499 }),
        json!({ "type": "scroll", "direction": "left", "column": 0, "row": 0 }),
        json!({ "type": "scroll", "direction": "up", "column": 500, "row": 0 }),
        json!({ "type": "resize", "cols": 80, "rows": 24 }),
    ];
    for scroll in scrolls {
        let desktop = desktop_wire::parse_terminal_scroll_request(&scroll)
            .map(|request| desktop_wire::build_herdr_scroll_control_record(&request));
        assert_eq!(
            agent_wire::parse_scroll_control_record(&scroll),
            desktop,
            "{scroll}"
        );
    }
    for size in [
        json!([80, 24]),
        json!([0, 24]),
        json!([501, 24]),
        json!([80.5, 24]),
        json!(["80", 24]),
    ] {
        assert_eq!(
            agent_wire::is_remote_terminal_size(size.get(0), size.get(1)),
            desktop_wire::is_remote_terminal_size(size.get(0), size.get(1)),
            "{size}"
        );
    }
}

/// daemon 연결 흉내. `has_connection`이 거짓이면 daemon을 쓸 수 없는 호스트다
struct FakeDaemonLauncher {
    has_connection: bool,
    launched_commands: Mutex<Vec<(String, String)>>,
}

impl DaemonAgentLauncher for FakeDaemonLauncher {
    fn launch(
        &self,
        ssh_host: &str,
        command: &str,
    ) -> Result<Option<RemoteTerminalAgentProcess>, String> {
        if !self.has_connection {
            return Ok(None);
        }
        self.launched_commands
            .lock()
            .unwrap()
            .push((ssh_host.to_string(), command.to_string()));
        Ok(Some(RemoteTerminalAgentProcess {
            stdin: Box::new(CapturedStdin(Arc::default())),
            stdout: Box::new(Cursor::new(b"{\"type\":\"bootstrap-ready\"}\n".to_vec())),
            termination: Box::new(CountedTermination(Arc::default())),
        }))
    }
}

fn daemon_first_spawner(
    has_connection: bool,
) -> (
    DaemonFirstRemoteTerminalAgentSpawner,
    Arc<FakeDaemonLauncher>,
    Arc<ScriptedSpawner>,
) {
    let daemon = Arc::new(FakeDaemonLauncher {
        has_connection,
        launched_commands: Mutex::default(),
    });
    let ssh = Arc::new(ScriptedSpawner {
        launches: AtomicUsize::new(0),
        uploaded: Arc::default(),
        terminations: Arc::default(),
    });
    let spawner = DaemonFirstRemoteTerminalAgentSpawner::new(daemon.clone(), ssh.clone());
    (spawner, daemon, ssh)
}

#[test]
fn a_host_with_a_daemon_connection_runs_the_agent_command_through_it_without_ssh() {
    let (spawner, daemon, ssh) = daemon_first_spawner(true);
    let arguments = vec![
        "-T".to_string(),
        "fixture".to_string(),
        "sh -lc 'agent'".to_string(),
    ];

    let process = spawner.spawn("fixture", &arguments).unwrap();

    let first_line = BufReader::new(process.stdout)
        .lines()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(first_line, "{\"type\":\"bootstrap-ready\"}");
    assert_eq!(
        *daemon.launched_commands.lock().unwrap(),
        [("fixture".to_string(), "sh -lc 'agent'".to_string())]
    );
    assert_eq!(ssh.launches.load(Ordering::SeqCst), 0);
}

#[test]
fn a_host_without_a_daemon_connection_falls_back_to_system_ssh() {
    let (spawner, daemon, ssh) = daemon_first_spawner(false);

    spawner
        .spawn(
            "fixture",
            &["fixture".to_string(), "sh -lc 'agent'".to_string()],
        )
        .unwrap();

    assert!(daemon.launched_commands.lock().unwrap().is_empty());
    assert_eq!(ssh.launches.load(Ordering::SeqCst), 1);
}
