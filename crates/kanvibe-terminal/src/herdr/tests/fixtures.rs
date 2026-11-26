//! Herdr 테스트 공용 준비물. 서비스, 가짜 실행기, 공유 서버 흉내, 가짜 프로그램 작성기를 둔다.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kanvibe_process::exec::ExecGitError;
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::shell_environment::{ShellEnvironment, ShellPlatform};
use kanvibe_process::ssh_config::SshConfigReader;
use serde_json::{Value, json};
use tempfile::TempDir;

use crate::environment::TerminalHostEnvironment;
use crate::herdr::{HerdrService, build_herdr_command};
use crate::test_support::ScriptedShellRunner;

pub(super) const RESTORE_UNAVAILABLE: &str =
    "Saved directory is unavailable. Restore the directory and restart this session.";

pub(super) struct HerdrFixture {
    pub runner: Arc<ScriptedShellRunner>,
    pub service: HerdrService,
    pub home: TempDir,
}

pub(super) fn parent_environment(entries: &[(&str, &str)]) -> ShellEnvironment {
    entries
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// 부모 환경에 HOME과 기본 PATH를 채워 서비스를 만든다. 뒤에 준 항목이 앞선 값을 덮는다
pub(super) fn fixture_with_environment(entries: &[(&str, &str)]) -> HerdrFixture {
    let home = TempDir::new().unwrap();
    let home_path = home.path().to_string_lossy().into_owned();
    let mut parent = parent_environment(&[("HOME", &home_path), ("PATH", "/usr/bin:/bin")]);
    parent.extend(parent_environment(entries));
    let environment =
        TerminalHostEnvironment::from_parent_environment(&parent, ShellPlatform::Other, &home_path);
    let runner = ScriptedShellRunner::new();
    let files = Arc::new(HostFileAccess::new(runner.clone(), home_path.clone()));
    let ssh_config = Arc::new(SshConfigReader::new(&home_path));
    let service = HerdrService::new(files, Arc::new(environment), ssh_config);
    HerdrFixture {
        runner,
        service,
        home,
    }
}

pub(super) fn fixture() -> HerdrFixture {
    fixture_with_environment(&[])
}

/// 테스트가 직접 앞당기는 시계. 다른 경로의 대기 루프가 멈추지 않도록 실제 경과 시간에 더한다
pub(super) fn install_adjustable_clock(service: &mut HerdrService) -> Arc<AtomicU64> {
    let offset_ms = Arc::new(AtomicU64::new(0));
    let start = Instant::now();
    let clock_offset = Arc::clone(&offset_ms);
    service.clock = Arc::new(move || {
        start + start.elapsed() + Duration::from_millis(clock_offset.load(Ordering::SeqCst))
    });
    offset_ms
}

/// 시간이 테스트가 앞당길 때만 흐르는 시계
pub(super) fn install_frozen_clock(service: &mut HerdrService) -> Arc<AtomicU64> {
    let offset_ms = Arc::new(AtomicU64::new(0));
    let start = Instant::now();
    let clock_offset = Arc::clone(&offset_ms);
    service.clock =
        Arc::new(move || start + Duration::from_millis(clock_offset.load(Ordering::SeqCst)));
    offset_ms
}

pub(super) fn result(value: Value) -> String {
    json!({ "id": "test", "result": value }).to_string()
}

pub(super) fn command_for(arguments: &[&str]) -> String {
    build_herdr_command("feature", arguments).unwrap()
}

pub(super) fn shared_command_for(arguments: &[&str]) -> String {
    build_herdr_command("kanvibe", arguments).unwrap()
}

/// vitest `mockResolvedValueOnce`·`mockRejectedValueOnce` 이어 붙이기. 다 쓰면 앞서 등록한 응답기로 넘긴다
pub(super) fn respond_in_sequence(runner: &ScriptedShellRunner, outputs: Vec<Result<&str, &str>>) {
    let queue: VecDeque<Result<String, String>> = outputs
        .into_iter()
        .map(|output| output.map(str::to_string).map_err(str::to_string))
        .collect();
    let queue = Mutex::new(queue);
    runner.respond(move |_, _| {
        let next = queue.lock().unwrap().pop_front()?;
        Some(next.map_err(ExecGitError::new))
    });
}

/// 정상 pane 하나가 있는 workspace w1만 가진 task 전용 서버
pub(super) fn respond_as_healthy_server(runner: &ScriptedShellRunner) {
    runner.respond(|command, _| {
        Some(Ok(match command.contains("'pane' 'list'") {
            true => result(json!({ "panes": [{ "pane_id": "w1:p1" }] })),
            false => result(json!({ "workspaces": [{ "workspace_id": "w1" }] })),
        }))
    });
}

pub(super) fn session_list(sessions: Value) -> String {
    json!({ "sessions": sessions }).to_string()
}

/// 공유 서버 `kanvibe`의 세션 경로를 임시 디렉터리로 두고 그 안의 Space 연결 파일을 쓴다
pub(super) struct SharedSessionDirectory {
    pub directory: TempDir,
}

impl SharedSessionDirectory {
    pub fn new(mapping: Value) -> Self {
        let shared = Self {
            directory: TempDir::new().unwrap(),
        };
        shared.write_mapping(&mapping);
        shared
    }

    pub fn path(&self) -> String {
        self.directory.path().to_string_lossy().into_owned()
    }

    pub fn mapping_path(&self) -> PathBuf {
        self.directory.path().join("kanvibe-workspaces.json")
    }

    pub fn write_mapping(&self, mapping: &Value) {
        std::fs::write(self.mapping_path(), mapping.to_string()).unwrap();
    }

    pub fn read_mapping(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.mapping_path()).unwrap()).unwrap()
    }

    pub fn session_list(&self) -> String {
        let path = self.path();
        session_list(
            json!([{ "name": "kanvibe", "running": true, "session_dir": path, "socket_path": format!("{path}/herdr.sock") }]),
        )
    }
}

/// 실행 권한을 준 스크립트를 만든다
pub(super) fn write_executable(path: &Path, content: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, content).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// 파일이 생기고 내용이 찰 때까지 기다린다
pub(super) fn wait_for_file(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(content) = std::fs::read_to_string(path)
            && !content.is_empty()
        {
            return content;
        }
        assert!(
            Instant::now() < deadline,
            "{} 파일이 생기지 않았습니다",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 조건이 참이 될 때까지 기다린다(vitest `waitFor`)
pub(super) fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "조건을 기다리다 시간이 초과되었습니다"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn count_containing(commands: &[String], needle: &str) -> usize {
    commands
        .iter()
        .filter(|command| command.contains(needle))
        .count()
}
