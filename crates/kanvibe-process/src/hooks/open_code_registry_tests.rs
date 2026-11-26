//! TS `openCodePluginRegistry.test.ts`. `vi.mock("node:child_process")` 자리는 가짜 CLI 실행기, 가짜 타이머 자리는 바꿔 끼운 시계다.

use std::os::unix::fs::PermissionsExt;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::open_code_registry::*;
use crate::shell_environment::{ShellEnvironment, ShellPlatform, create_local_shell_environment};

type CommandScript = dyn Fn(&str) -> Result<String, String> + Send + Sync;

/// 호출한 작업 디렉터리를 기록하고 정해 둔 결과를 돌려주는 CLI
struct FakeDebugConfigCommand {
    calls: Mutex<Vec<String>>,
    script: Mutex<Arc<CommandScript>>,
}

impl FakeDebugConfigCommand {
    fn new(script: impl Fn(&str) -> Result<String, String> + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            script: Mutex::new(Arc::new(script)),
        })
    }

    fn replace(&self, script: impl Fn(&str) -> Result<String, String> + Send + Sync + 'static) {
        *self.script.lock().unwrap() = Arc::new(script);
    }

    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl OpenCodeDebugConfigCommand for FakeDebugConfigCommand {
    fn run(&self, working_directory: &str) -> Result<String, String> {
        self.calls
            .lock()
            .unwrap()
            .push(working_directory.to_string());
        let script = Arc::clone(&self.script.lock().unwrap());
        script(working_directory)
    }
}

/// 테스트가 직접 앞으로 돌리는 시계
#[derive(Clone)]
struct ManualClock {
    now: Arc<Mutex<Instant>>,
}

impl ManualClock {
    fn new() -> Self {
        Self {
            now: Arc::new(Mutex::new(Instant::now())),
        }
    }

    fn advance(&self, duration: Duration) {
        *self.now.lock().unwrap() += duration;
    }

    fn as_clock(&self) -> Arc<dyn Fn() -> Instant + Send + Sync> {
        let now = Arc::clone(&self.now);
        Arc::new(move || *now.lock().unwrap())
    }
}

fn registry_with(
    command: &Arc<FakeDebugConfigCommand>,
    clock: &ManualClock,
) -> Arc<OpenCodePluginRegistry> {
    let command: Arc<dyn OpenCodeDebugConfigCommand> = command.clone();
    Arc::new(OpenCodePluginRegistry::with_clock(
        command,
        clock.as_clock(),
    ))
}

#[test]
fn extracts_plugin_urls_from_valid_json_output() {
    let output =
        r#"{"plugin":["file:///tmp/alpha.ts","file:///tmp/kanvibe-plugin.ts"],"agent":{}}"#;

    assert_eq!(
        extract_registered_plugin_urls(output),
        ["file:///tmp/alpha.ts", "file:///tmp/kanvibe-plugin.ts"]
    );
}

#[test]
fn falls_back_to_the_plugin_block_when_debug_output_is_not_valid_json() {
    let output = "{\n  \"plugin\": [\n    \"file:///tmp/alpha.ts\",\n    \"file:///tmp/kanvibe-plugin.ts\"\n  ],\n  \"agent\": {\n    \"custom\": {\n      \"template\": \"line 1\nline 2\"\n    }\n  }\n}";

    assert_eq!(
        extract_registered_plugin_urls(output),
        ["file:///tmp/alpha.ts", "file:///tmp/kanvibe-plugin.ts"]
    );
}

#[test]
fn plugin_block_fallback_decodes_json_escapes_like_ts() {
    let output = "{\n  \"plugin\": [\"file:///tmp/a\\\"b.ts\", \"file:///tmp/\\u006banvibe-plugin.ts\", \"x\\\\y\"],\n  \"agent\": \"bad\njson\"\n}";
    let golden = std::fs::read_to_string(format!(
        "{}/tests/fixtures/hooks/opencode-registry-escaped-block.golden",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();

    assert_eq!(
        serde_json::to_string(&extract_registered_plugin_urls(output)).unwrap(),
        golden
    );
}

#[test]
fn filters_only_kanvibe_plugin_urls() {
    let output = r#"{"plugin":["file:///tmp/alpha.ts","file:///tmp/kanvibe-plugin.ts","file:///tmp/kanvibe-plugin.js"],"agent":{}}"#;

    assert_eq!(
        extract_registered_kanvibe_plugin_urls(output),
        [
            "file:///tmp/kanvibe-plugin.ts",
            "file:///tmp/kanvibe-plugin.js"
        ]
    );
}

#[test]
fn matches_kanvibe_plugin_urls_with_ts_or_js_extensions() {
    assert!(is_kanvibe_plugin_url("file:///tmp/kanvibe-plugin.ts"));
    assert!(is_kanvibe_plugin_url("file:///tmp/kanvibe-plugin.js"));
    assert!(!is_kanvibe_plugin_url("file:///tmp/not-kanvibe.ts"));
}

/// PATH 앞에 둔 가짜 `opencode`. 받은 환경과 작업 디렉터리를 파일에 남긴다
fn install_fake_open_code(bin_directory: &std::path::Path, body: &str) {
    let script_path = bin_directory.join("opencode");
    std::fs::write(&script_path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn fake_cli_environment(
    bin_directory: &std::path::Path,
    platform: ShellPlatform,
) -> ShellEnvironment {
    let parent_environment: ShellEnvironment = [
        (
            "PATH".to_string(),
            format!("{}:/usr/bin:/bin", bin_directory.display()),
        ),
        ("HOME".to_string(), "/home/opencode-user".to_string()),
        ("PORT".to_string(), "9736".to_string()),
        (
            "KANVIBE_APP_DATA_DIR".to_string(),
            "/tmp/kanvibe-app-data".to_string(),
        ),
    ]
    .into();
    create_local_shell_environment(&parent_environment, platform, "/unused")
}

#[test]
fn runs_opencode_debug_config_with_the_local_shell_environment() {
    let workspace = tempfile::tempdir().unwrap();
    let bin_directory = workspace.path().join("bin");
    let project = workspace.path().join("project");
    std::fs::create_dir_all(&bin_directory).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let record_path = workspace.path().join("record");
    install_fake_open_code(
        &bin_directory,
        &format!(
            "printf '%s\\n%s\\n%s\\n%s\\n' \"$*\" \"$PWD\" \"$PATH\" \"${{PORT-unset}}${{KANVIBE_APP_DATA_DIR-unset}}\" > '{}'\nprintf '%s' '{{\"plugin\":[\"file:///tmp/kanvibe-plugin.ts\"]}}'",
            record_path.display()
        ),
    );
    let command = SystemOpenCodeDebugConfigCommand::new(fake_cli_environment(
        &bin_directory,
        ShellPlatform::Mac,
    ));
    let registry = OpenCodePluginRegistry::new(Arc::new(command));

    let result = registry.registered_kanvibe_plugin_urls(project.to_str().unwrap());

    assert_eq!(result.unwrap(), ["file:///tmp/kanvibe-plugin.ts"]);
    let record = std::fs::read_to_string(&record_path).unwrap();
    let lines: Vec<&str> = record.lines().collect();
    assert_eq!(lines[0], "debug config");
    assert_eq!(lines[1], project.to_str().unwrap());
    assert!(lines[2].contains("/home/opencode-user/.opencode/bin"));
    assert_eq!(lines[3], "unsetunset");
}

#[test]
fn shares_concurrent_requests_and_caches_sequential_requests_for_five_seconds() {
    let (release_sender, release_receiver) = mpsc::channel::<()>();
    let release_receiver = Mutex::new(release_receiver);
    let command = FakeDebugConfigCommand::new(move |_| {
        let _ = release_receiver
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_millis(200));
        Ok(r#"{"plugin":[]}"#.into())
    });
    let clock = ManualClock::new();
    let registry = registry_with(&command, &clock);

    std::thread::scope(|scope| {
        let queries: Vec<_> = (0..10)
            .map(|_| scope.spawn(|| registry.registered_kanvibe_plugin_urls("/same/repo")))
            .collect();
        std::thread::sleep(Duration::from_millis(50));
        drop(release_sender);
        for query in queries {
            assert_eq!(query.join().unwrap().unwrap(), Vec::<String>::new());
        }
    });
    registry
        .registered_kanvibe_plugin_urls("/same/repo/.")
        .unwrap();
    assert_eq!(command.call_count(), 1);
    clock.advance(Duration::from_millis(5000));
    registry
        .registered_kanvibe_plugin_urls("/same/repo")
        .unwrap();
    assert_eq!(command.call_count(), 2);
    registry
        .registered_kanvibe_plugin_urls("/other/repo")
        .unwrap();
    assert_eq!(command.call_count(), 3);
}

#[test]
fn bounds_a_slow_cli_and_retains_failure_instead_of_reporting_an_empty_registration() {
    let command = FakeDebugConfigCommand::new(|_| {
        std::thread::sleep(Duration::from_millis(50));
        Err("OpenCode timed out".into())
    });
    let clock = ManualClock::new();
    let registry = registry_with(&command, &clock);

    std::thread::scope(|scope| {
        let first = scope.spawn(|| registry.registered_kanvibe_plugin_urls("/slow/repo"));
        let second = scope.spawn(|| registry.registered_kanvibe_plugin_urls("/slow/repo"));
        assert_eq!(first.join().unwrap(), Err("OpenCode timed out".to_string()));
        assert_eq!(
            second.join().unwrap(),
            Err("OpenCode timed out".to_string())
        );
    });
    assert_eq!(command.call_count(), 1);
    assert_eq!(
        registry.registered_kanvibe_plugin_urls("/slow/repo"),
        Err("OpenCode timed out".to_string())
    );
    assert_eq!(command.call_count(), 1);
    clock.advance(Duration::from_millis(5000));
    command.replace(|_| Ok(r#"{"plugin":[]}"#.into()));
    assert_eq!(
        registry.registered_kanvibe_plugin_urls("/slow/repo"),
        Ok(Vec::new())
    );
    assert_eq!(command.call_count(), 2);

    // 실제 CLI는 3초 상한을 넘기면 SIGKILL로 끝나고 실패로 돌아온다
    let workspace = tempfile::tempdir().unwrap();
    install_fake_open_code(workspace.path(), "exec sleep 10");
    let system_command = SystemOpenCodeDebugConfigCommand::new(fake_cli_environment(
        workspace.path(),
        ShellPlatform::Other,
    ));
    let started_at = Instant::now();
    let error = system_command
        .run(workspace.path().to_str().unwrap())
        .unwrap_err();
    assert!(started_at.elapsed() < Duration::from_secs(6));
    assert!(error.starts_with("Command failed: opencode debug config"));
}

#[test]
fn invalidates_a_completed_query_after_installation() {
    let command = FakeDebugConfigCommand::new(|_| Ok(r#"{"plugin":[]}"#.into()));
    let registry = registry_with(&command, &ManualClock::new());
    registry
        .registered_kanvibe_plugin_urls("/installed/repo")
        .unwrap();

    registry.invalidate("/installed/repo");
    command.replace(|_| {
        Ok(r#"{"plugin":["file:///installed/repo/.opencode/plugins/kanvibe-plugin.ts"]}"#.into())
    });

    assert_eq!(
        registry
            .registered_kanvibe_plugin_urls("/installed/repo")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(command.call_count(), 2);
}

#[test]
fn waits_for_an_invalidated_in_flight_query_before_sharing_one_fresh_query() {
    let (finish_sender, finish_receiver) = mpsc::channel::<()>();
    let finish_receiver = Mutex::new(finish_receiver);
    let command = FakeDebugConfigCommand::new(move |_| {
        finish_receiver.lock().unwrap().recv().unwrap();
        Ok(r#"{"plugin":[]}"#.into())
    });
    let registry = registry_with(&command, &ManualClock::new());

    std::thread::scope(|scope| {
        let old_query = scope.spawn(|| registry.registered_kanvibe_plugin_urls("/installing/repo"));
        while command.call_count() == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
        registry.invalidate("/installing/repo");
        let fresh_a = scope.spawn(|| registry.registered_kanvibe_plugin_urls("/installing/repo"));
        let fresh_b = scope.spawn(|| registry.registered_kanvibe_plugin_urls("/installing/repo"));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(command.call_count(), 1);
        command
            .replace(|_| Ok(r#"{"plugin":["file:///installing/repo/kanvibe-plugin.ts"]}"#.into()));
        finish_sender.send(()).unwrap();

        assert_eq!(old_query.join().unwrap().unwrap(), Vec::<String>::new());
        let fresh_a = fresh_a.join().unwrap().unwrap();
        assert_eq!(fresh_a, fresh_b.join().unwrap().unwrap());
        assert_eq!(fresh_a.len(), 1);
    });
    assert_eq!(command.call_count(), 2);
}

#[test]
fn file_urls_follow_electron_path_to_file_url_encoding() {
    assert_eq!(
        file_url_from_path("/a b/c^d`e{f}g#h?i%j\\k\"l<m>né|x[y]z'@!$&()*+,;=:~\t"),
        "file:///a%20b/c^d%60e%7Bf%7Dg%23h%3Fi%25j%5Ck%22l%3Cm%3En%C3%A9|x[y]z'@!$&()*+,;=:~%09"
    );
    assert_eq!(resolve_absolute_path("/same/repo/."), "/same/repo");
    assert_eq!(resolve_absolute_path("/a//b/../c/"), "/a/c");
}
