//! TS `hookFileWriter.test.ts`를 옮긴 테스트. 원격 설치 경로와 SSH 주입 폴백을 가짜 실행기로 확인한다.

use std::sync::Arc;
use std::time::Duration;

use regex::Regex;

use super::HookProviderFile;
use super::file_writer::write_hook_provider_files;
use super::install_bundle::HOOK_INSTALL_SUCCESS_MARKER;
use super::installer_test_support::{FakeHookClientRuntime, install_context};
use crate::exec::ExecGitError;
use crate::exec::testing::ScriptedCommandRunner;

fn hook_files() -> Vec<HookProviderFile> {
    vec![
        HookProviderFile {
            file_path: "/home/user/project/.claude/hooks/kanvibe-stop-hook.sh".into(),
            content: "#!/bin/bash\necho '작업 완료'\n".into(),
            mode: Some(0o755),
        },
        HookProviderFile {
            file_path: "/home/user/project/.claude/settings.json".into(),
            content: "{\n  \"hooks\": {}\n}\n".into(),
            mode: None,
        },
    ]
}

struct Scenario {
    runner: Arc<ScriptedCommandRunner>,
    runtime: Arc<FakeHookClientRuntime>,
}

impl Scenario {
    /// 첫 호출(소켓 설치)의 결과를 정하고, 이후 호출(SSH 주입)은 성공시킨다
    fn new(first_result: Result<&str, &str>) -> Self {
        let first_result = first_result.map(str::to_string).map_err(str::to_string);
        let runner = ScriptedCommandRunner::new(move |call| {
            if !call.command.contains("__kanvibe_install_script") {
                return Ok(String::new());
            }
            first_result.clone().map_err(ExecGitError::new)
        });
        let mut runtime = FakeHookClientRuntime::new();
        runtime.fixed_client_id = "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed".into();
        runtime.hook_server_url = Ok("http://192.168.0.10:9736".into());
        Self {
            runner: Arc::new(runner),
            runtime: Arc::new(runtime),
        }
    }

    fn write(&self, files: &[HookProviderFile]) {
        let context = install_context(self.runner.clone(), self.runtime.clone(), "/home/test");
        write_hook_provider_files(&context, files, Some("remote-host")).unwrap();
    }

    fn issued_token(&self) -> String {
        let token_pattern = Regex::new("x-kanvibe-install-token: ([0-9a-f]+)").unwrap();
        let first_command = &self.runner.commands()[0];
        token_pattern
            .captures(first_command)
            .expect("설치 토큰 헤더")[1]
            .to_string()
    }
}

fn success_output() -> String {
    format!("{HOOK_INSTALL_SUCCESS_MARKER}\n")
}

// parity: TS-lib-hookFileWriter-0f519c
#[test]
fn installs_through_a_single_short_bootstrap_command() {
    let output = success_output();
    let scenario = Scenario::new(Ok(&output));

    scenario.write(&hook_files());

    let calls = scenario.runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].ssh_host.as_deref(), Some("remote-host"));
    assert!(calls[0].command.len() < 1500);
    assert!(calls[0].command.contains("/api/install/hooks.sh'"));
    assert!(
        calls[0]
            .command
            .contains("1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed.sock")
    );
    assert!(!calls[0].command.contains("base64 -d"));
}

// parity: TS-lib-hookFileWriter-17ecc6
#[test]
fn bounds_the_install_command_so_fallback_is_quick() {
    let output = success_output();
    let scenario = Scenario::new(Ok(&output));

    scenario.write(&hook_files());

    let options = &scenario.runner.calls()[0].options;
    assert_eq!(options.timeout, Some(Duration::from_secs(30)));
    assert!(options.should_check_host_key_strictly);
}

// parity: TS-lib-hookFileWriter-23f395
#[test]
fn revokes_the_install_token_once_the_install_finishes() {
    let output = success_output();
    let scenario = Scenario::new(Ok(&output));

    scenario.write(&hook_files());

    let token = scenario.issued_token();
    assert_eq!(scenario.runtime.install_scripts.read(Some(&token)), None);
}

// parity: TS-lib-hookFileWriter-55a6af
#[test]
fn revokes_the_install_token_even_when_the_http_install_fails() {
    let scenario = Scenario::new(Err("curl: (7) Failed to connect"));

    scenario.write(&hook_files());

    let token = scenario.issued_token();
    assert_eq!(scenario.runtime.install_scripts.read(Some(&token)), None);
}

// parity: TS-lib-hookFileWriter-4e8432
#[test]
fn falls_back_to_ssh_injection_when_the_remote_cannot_reach_the_hook_server() {
    let scenario = Scenario::new(Err("curl: (7) Failed to connect"));

    scenario.write(&hook_files());

    let commands = scenario.runner.commands();
    assert_eq!(commands.len(), 2);
    assert!(commands[1].contains("base64 -d"));
    assert!(commands[1].contains("/home/user/project/.claude/hooks/kanvibe-stop-hook.sh"));
    assert!(
        scenario.runner.calls()[1]
            .options
            .should_check_host_key_strictly
    );
}

// parity: TS-lib-hookFileWriter-3bb71f
#[test]
fn falls_back_when_the_bootstrap_script_does_not_report_completion() {
    let scenario = Scenario::new(Ok("sh: curl: not found\n"));

    scenario.write(&hook_files());

    let commands = scenario.runner.commands();
    assert_eq!(commands.len(), 2);
    assert!(commands[1].contains("base64 -d"));
}

// parity: TS-lib-hookFileWriter-21006d
#[test]
fn does_not_touch_the_remote_when_there_are_no_files() {
    let scenario = Scenario::new(Ok(""));

    scenario.write(&[]);

    assert!(scenario.runner.calls().is_empty());
}

#[test]
fn local_install_writes_files_and_applies_modes() {
    use std::os::unix::fs::PermissionsExt;
    let repo = tempfile::tempdir().unwrap();
    let script_path = repo.path().join("nested/hooks/run.sh");
    let files = [HookProviderFile {
        file_path: script_path.to_str().unwrap().to_string(),
        content: "#!/bin/bash\n".into(),
        mode: Some(0o755),
    }];
    let scenario = Scenario::new(Ok(""));
    let context = install_context(
        scenario.runner.clone(),
        scenario.runtime.clone(),
        "/home/test",
    );

    write_hook_provider_files(&context, &files, None).unwrap();

    assert_eq!(
        std::fs::read_to_string(&script_path).unwrap(),
        "#!/bin/bash\n"
    );
    let mode = std::fs::metadata(&script_path)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755);
    assert!(scenario.runner.calls().is_empty());
}
