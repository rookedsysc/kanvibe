//! 실제 tmux·zellij·Herdr를 띄우는 스모크 테스트. 기본으로는 돌지 않는다(`cargo test -p kanvibe-terminal --test multiplexer_smoke -- --ignored`).
//!
//! 사용자의 tmux·zellij·Herdr 서버를 건드리지 않도록, 자식에게만 넘기는 환경으로 소켓과 설정 위치를 임시 디렉터리에 가둔다.
//! tmux는 `TMUX_TMPDIR`, zellij는 `ZELLIJ_SOCKET_DIR`과 `XDG_*`, Herdr는 `XDG_CONFIG_HOME`(소켓이 그 아래에 생긴다)이다.
//! 이 프로세스의 환경은 읽기만 하고 바꾸지 않으며, 정리할 때도 이 테스트가 띄운 서버만 끝낸다.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kanvibe_domain::pane_layout::PaneLayoutType;
use kanvibe_process::exec::remote::RemoteSshSettings;
use kanvibe_process::exec::system::SystemShellCommandRunner;
use kanvibe_process::exec::{ExecGitOptions, ShellCommandRunner};
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::shell_environment::{ShellEnvironment, ShellPlatform};
use kanvibe_process::ssh_config::SshConfigReader;
use kanvibe_process::worktree::{
    TmuxSessionBootstrapOptions, ZELLIJ_LAYOUT_FILENAME, build_tmux_session_bootstrap_command,
    generate_zellij_layout_kdl,
};
use kanvibe_terminal::environment::TerminalHostEnvironment;
use kanvibe_terminal::herdr::{HerdrMirrorPaneOptions, HerdrService, build_herdr_command};
use kanvibe_terminal::pane_mirror;
use kanvibe_terminal::pty::{
    NativePtySpawner, PtyEventCallbacks, PtyProcess, PtySpawnRequest, PtySpawner,
};
use kanvibe_terminal::screen::TerminalScreen;

const SCREEN_TIMEOUT: Duration = Duration::from_secs(20);
const SMOKE_MARKER: &str = "kanvibe-smoke-42";
const SMOKE_INPUT: &str = "echo kanvibe-smoke-$((40+2))\r";

/// 임시 디렉터리에 가둔 자식 환경과 그 위의 명령 실행기
struct IsolatedHost {
    root: tempfile::TempDir,
    environment: ShellEnvironment,
    runner: Arc<SystemShellCommandRunner>,
}

impl IsolatedHost {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("kv-smoke-")
            .tempdir()
            .expect("임시 디렉터리");
        let path = root.path();
        for directory in [
            "home",
            "config/zellij",
            "run",
            "cache",
            "data",
            "state",
            "tmux",
            "zellij",
            "work",
        ] {
            std::fs::create_dir_all(path.join(directory)).expect("디렉터리");
        }
        set_private(&path.join("run"));
        std::fs::write(
            path.join("config/zellij/config.kdl"),
            "show_startup_tips false\nshow_release_notes false\nkeybinds preset=\"default\" primary=\"Ctrl\" secondary=\"Alt\" {\n}\n",
        )
        .expect("zellij 설정");
        let environment = isolated_environment(path);
        let runner = Arc::new(SystemShellCommandRunner::new(
            environment.clone(),
            &path.join("home").to_string_lossy(),
            RemoteSshSettings::default(),
        ));
        Self {
            root,
            environment,
            runner,
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    fn run(&self, command: &str) -> Result<String, String> {
        let options = ExecGitOptions {
            timeout: Some(Duration::from_secs(30)),
            ..ExecGitOptions::default()
        };
        self.runner
            .exec_git(command, None, &options)
            .map_err(|error| format!("{error} {}", error.stderr))
    }

    fn terminal_environment(&self) -> TerminalHostEnvironment {
        TerminalHostEnvironment::from_parent_environment(
            &self.environment,
            ShellPlatform::current(),
            "/",
        )
    }
}

fn set_private(directory: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).expect("권한");
}

/// 사용자 셸 환경에서 PATH만 빌려 오고 나머지는 모두 임시 위치를 가리킨다
fn isolated_environment(root: &Path) -> ShellEnvironment {
    let at = |relative: &str| root.join(relative).to_string_lossy().into_owned();
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string());
    [
        ("HOME", at("home")),
        ("PATH", path),
        ("SHELL", "/bin/sh".to_string()),
        ("TERM", "xterm-256color".to_string()),
        ("LANG", "C.UTF-8".to_string()),
        ("LC_ALL", "C.UTF-8".to_string()),
        ("XDG_CONFIG_HOME", at("config")),
        ("XDG_RUNTIME_DIR", at("run")),
        ("XDG_CACHE_HOME", at("cache")),
        ("XDG_DATA_HOME", at("data")),
        ("XDG_STATE_HOME", at("state")),
        ("TMUX_TMPDIR", at("tmux")),
        ("ZELLIJ_SOCKET_DIR", at("zellij")),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect()
}

fn has_binary(name: &str) -> bool {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .any(|directory| Path::new(directory).join(name).is_file())
}

/// PTY 출력을 그대로 headless 화면에 흘려 두는 화면 하나
struct AttachedScreen {
    pty: Arc<dyn PtyProcess>,
    screen: Arc<Mutex<TerminalScreen>>,
}

impl AttachedScreen {
    fn spawn(host: &IsolatedHost, program: &str, args: &[String]) -> Self {
        let screen = Arc::new(Mutex::new(TerminalScreen::new(100, 30)));
        let output_screen = Arc::clone(&screen);
        let callbacks = PtyEventCallbacks {
            on_output: Box::new(move |data| output_screen.lock().unwrap().write(data.as_bytes())),
            on_exit: Box::new(|_| {}),
        };
        let request = PtySpawnRequest {
            program: program.to_string(),
            args: args.to_vec(),
            terminal_name: "xterm-256color".to_string(),
            columns: 100,
            rows: 30,
            working_directory: host.path("work").to_string_lossy().into_owned(),
            environment: host.environment.clone(),
        };
        let pty = NativePtySpawner
            .spawn(request, callbacks)
            .expect("PTY 실행");
        Self { pty, screen }
    }

    fn wait_for_text(&self, needle: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.screen.lock().unwrap().read().screen.contains(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    /// 첫 화면이 그려진 뒤 입력을 넣고 결과가 화면에 나오는지 본다.
    /// 멀티플렉서가 프롬프트를 그린 직후에는 입력을 아직 pane에 넘기지 않을 수 있어, 결과가 없으면 몇 초 뒤 다시 넣는다
    fn type_and_expect(&self, settle_text: &str) {
        assert!(
            self.wait_for_text(settle_text, SCREEN_TIMEOUT),
            "첫 화면:\n{}",
            self.text()
        );
        let deadline = Instant::now() + SCREEN_TIMEOUT;
        while Instant::now() < deadline {
            self.pty.write(SMOKE_INPUT).expect("입력");
            if self.wait_for_text(SMOKE_MARKER, Duration::from_secs(3)) {
                return;
            }
        }
        panic!("입력 결과가 화면에 없다:\n{}", self.text());
    }

    fn text(&self) -> String {
        self.screen.lock().unwrap().read().screen
    }

    /// 직렬화한 화면을 새 화면에 다시 그려도 같은 글이 보여야 나중에 붙는 클라이언트가 같은 화면을 본다
    fn assert_snapshot_replays(&self) {
        let serialized = self.screen.lock().unwrap().serialize();
        let mut replayed = TerminalScreen::new(100, 30);
        replayed.write(serialized.as_bytes());
        assert!(
            replayed.read().screen.contains(SMOKE_MARKER),
            "직렬화 화면:\n{}",
            replayed.read().screen
        );
    }
}

impl Drop for AttachedScreen {
    fn drop(&mut self) {
        let _ = self.pty.kill();
    }
}

fn first_mirror_pane_id(panes: Vec<pane_mirror::MirrorPane>) -> String {
    panes.into_iter().next().expect("pane 하나").id
}

#[test]
#[ignore = "실제 tmux를 격리된 소켓으로 띄운다"]
fn isolated_tmux_session_round_trip() {
    if !has_binary("tmux") {
        eprintln!("tmux 없음: 건너뜀");
        return;
    }
    let host = IsolatedHost::new();
    let session = "kv-smoke";
    let options = TmuxSessionBootstrapOptions {
        should_skip_user_config_file: true,
        ..TmuxSessionBootstrapOptions::default()
    };
    let work = host.path("work").to_string_lossy().into_owned();
    host.run(&build_tmux_session_bootstrap_command(
        session, &work, None, options,
    ))
    .expect("tmux 세션 생성");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let attached = AttachedScreen::spawn(
            &host,
            "tmux",
            &["attach-session".into(), "-t".into(), session.into()],
        );
        attached.type_and_expect("$");
        attached.assert_snapshot_replays();
        let panes = host
            .run(&pane_mirror::build_tmux_list_mirror_panes_command(session))
            .expect("pane 목록");
        let pane_id = first_mirror_pane_id(pane_mirror::parse_tmux_mirror_pane_list(&panes));
        let captured = host
            .run(&pane_mirror::build_tmux_capture_pane_command(&pane_id))
            .expect("캡처");
        assert!(captured.contains(SMOKE_MARKER), "capture-pane:\n{captured}");
    }));
    let _ = host.run("tmux kill-server");
    result.unwrap();
}

#[test]
#[ignore = "실제 zellij를 격리된 소켓 디렉터리로 띄운다"]
fn isolated_zellij_session_round_trip() {
    if !has_binary("zellij") {
        eprintln!("zellij 없음: 건너뜀");
        return;
    }
    let host = IsolatedHost::new();
    let session = "kv-smoke";
    // KanVibe가 worktree에 남기는 레이아웃 파일로 새 세션을 만드는 로컬 attach 경로와 같게 띄운다
    let work = host.path("work").to_string_lossy().into_owned();
    let layout_file = host.path("work").join(ZELLIJ_LAYOUT_FILENAME);
    std::fs::write(
        &layout_file,
        generate_zellij_layout_kdl(PaneLayoutType::Single, &[], &work),
    )
    .expect("레이아웃");
    let args = [
        "--session".to_string(),
        session.to_string(),
        "--new-session-with-layout".to_string(),
        layout_file.to_string_lossy().into_owned(),
    ];

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let attached = AttachedScreen::spawn(&host, "zellij", &args);
        assert!(
            attached.wait_for_text("$", SCREEN_TIMEOUT),
            "첫 화면:\n{}",
            attached.text()
        );
        let panes = host
            .run(&pane_mirror::build_zellij_list_mirror_panes_command(
                session,
            ))
            .expect("pane 목록");
        let pane_id = first_mirror_pane_id(pane_mirror::parse_zellij_mirror_pane_list(&panes));
        // 헤드리스 PTY에서 친 키는 zellij 0.46이 pane으로 넘기지 않아, pane chat·모바일이 쓰는 `action write` 경로로 넣는다
        let write = pane_mirror::build_zellij_write_pane_command(session, &pane_id, SMOKE_INPUT);
        host.run(&write).expect("입력");
        assert!(
            attached.wait_for_text(SMOKE_MARKER, SCREEN_TIMEOUT),
            "attach 화면:\n{}",
            attached.text()
        );
        attached.assert_snapshot_replays();
        let dumped = host
            .run(&pane_mirror::build_zellij_dump_pane_command(
                session, &pane_id,
            ))
            .expect("dump");
        assert!(dumped.contains(SMOKE_MARKER), "dump-screen:\n{dumped}");
    }));
    let _ = host.run(&format!(
        "zellij kill-session {session}; zellij delete-session --force {session}"
    ));
    result.unwrap();
}

#[test]
#[ignore = "실제 Herdr 서버를 격리된 설정 디렉터리로 띄운다"]
fn isolated_herdr_session_round_trip() {
    if !has_binary("herdr") {
        eprintln!("herdr 없음: 건너뜀");
        return;
    }
    let host = IsolatedHost::new();
    let home = host.path("home").to_string_lossy().into_owned();
    let files = Arc::new(HostFileAccess::new(host.runner.clone(), home.clone()));
    let herdr = HerdrService::new(
        files,
        Arc::new(host.terminal_environment()),
        Arc::new(SshConfigReader::new(&home)),
    );
    let session = "kanvibe:smoke";
    let work = host.path("work").to_string_lossy().into_owned();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let workspace_id = herdr
            .ensure_herdr_session(session, &work, None, None)
            .expect("Space 준비");
        assert!(workspace_id.is_some(), "공유 서버는 Space ID를 돌려준다");
        let panes = herdr
            .list_herdr_mirror_panes(session, None, &HerdrMirrorPaneOptions::default())
            .expect("pane 목록");
        let pane_id = first_mirror_pane_id(panes);
        let write = pane_mirror::build_herdr_write_pane_command(session, &pane_id, SMOKE_INPUT)
            .expect("입력 명령");
        host.run(&write).expect("입력");
        let read =
            pane_mirror::build_herdr_recent_screen_command(session, &pane_id).expect("읽기 명령");
        let deadline = Instant::now() + SCREEN_TIMEOUT;
        let mut screen = String::new();
        while Instant::now() < deadline && !screen.contains(SMOKE_MARKER) {
            std::thread::sleep(Duration::from_millis(200));
            screen = host.run(&read).unwrap_or_default();
        }
        assert!(screen.contains(SMOKE_MARKER), "pane read:\n{screen}");
        let mut replayed = TerminalScreen::new(100, 30);
        replayed.write(screen.as_bytes());
        assert!(replayed.read().screen.contains(SMOKE_MARKER));
    }));
    let stop = build_herdr_command(session, &["server", "stop"]).expect("종료 명령");
    let _ = host.run(&stop);
    result.unwrap();
}
