//! Herdr client 설정의 탭 바 위치 읽기.

use std::sync::Arc;

use kanvibe_process::exec::local::exec_local;
use kanvibe_process::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::shell_environment::{ShellEnvironment, ShellPlatform};
use kanvibe_process::ssh_config::SshConfigReader;
use tempfile::TempDir;

use super::fixtures::{fixture, parent_environment, respond_in_sequence};
use crate::environment::TerminalHostEnvironment;
use crate::herdr::{HerdrService, HerdrTabBarPosition};

/// 정해 둔 환경으로 로컬 `sh -c`를 실제로 실행하는 실행기
struct LocalShellRunner {
    environment: ShellEnvironment,
}

impl ShellCommandRunner for LocalShellRunner {
    fn exec_git(
        &self,
        command: &str,
        _ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<String, ExecGitError> {
        exec_local(command, &self.environment, options)
    }
}

fn local_shell_service(home: &str, entries: &[(&str, &str)]) -> HerdrService {
    let mut environment = parent_environment(&[("HOME", home), ("PATH", "/usr/bin:/bin")]);
    environment.extend(parent_environment(entries));
    let runner = Arc::new(LocalShellRunner {
        environment: environment.clone(),
    });
    let files = Arc::new(HostFileAccess::new(runner, home));
    let host_environment =
        TerminalHostEnvironment::from_parent_environment(&environment, ShellPlatform::Other, home);
    HerdrService::new(
        files,
        Arc::new(host_environment),
        Arc::new(SshConfigReader::new(home)),
    )
}

#[test]
fn config_content_resolves_to_a_tab_bar_position() {
    let cases = [
        ("", Some(HerdrTabBarPosition::Top)),
        (
            "[ui]\ntab_bar_position = \"bottom\" # client setting",
            Some(HerdrTabBarPosition::Bottom),
        ),
        (
            "[ui]\ntab_bar_position = 'top'",
            Some(HerdrTabBarPosition::Top),
        ),
        (
            "ui.tab_bar_position = \"bottom\"",
            Some(HerdrTabBarPosition::Bottom),
        ),
        (
            "[\"ui\"]\n\"tab_bar_position\" = \"bottom\"",
            Some(HerdrTabBarPosition::Bottom),
        ),
        (
            "[keys]\ntab_bar_position = \"bottom\"",
            Some(HerdrTabBarPosition::Top),
        ),
        ("[ui]\ntab_bar_position = \"left\"", None),
        ("ui = { tab_bar_position = \"bottom\" }", None),
        (
            "[ui]\ntab_bar_position = \"top\"\ntab_bar_position = \"bottom\"",
            None,
        ),
    ];
    for (content, expected) in cases {
        let herdr = fixture();
        respond_in_sequence(&herdr.runner, vec![Ok(content)]);

        assert_eq!(
            herdr
                .service
                .read_herdr_client_tab_bar_position(Some("remote")),
            expected,
            "{content}"
        );
        // 경로 계산과 내용 읽기를 한 원격 명령으로 끝내 터미널을 열 때 SSH 왕복이 하나만 든다.
        let calls = herdr.runner.calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].command.contains("HERDR_CONFIG_PATH"));
        assert!(calls[0].command.contains("XDG_CONFIG_HOME"));
        assert_eq!(calls[0].ssh_host.as_deref(), Some("remote"));
    }
}

#[test]
fn fails_closed_when_the_config_cannot_be_read() {
    let herdr = fixture();
    respond_in_sequence(&herdr.runner, vec![Err("unreachable")]);

    assert_eq!(
        herdr
            .service
            .read_herdr_client_tab_bar_position(Some("remote")),
        None
    );
}

#[test]
fn reads_the_config_in_a_real_shell_and_refuses_relative_paths() {
    let config_home = TempDir::new().unwrap();
    std::fs::create_dir(config_home.path().join("herdr")).unwrap();
    std::fs::write(
        config_home.path().join("herdr/config.toml"),
        "[ui]\ntab_bar_position = \"bottom\"\n",
    )
    .unwrap();
    let home = config_home.path().to_string_lossy().into_owned();
    let missing = config_home
        .path()
        .join("missing.toml")
        .to_string_lossy()
        .into_owned();

    let from_xdg = local_shell_service(
        &home,
        &[("HERDR_CONFIG_PATH", ""), ("XDG_CONFIG_HOME", &home)],
    );
    assert_eq!(
        from_xdg.read_herdr_client_tab_bar_position(None),
        Some(HerdrTabBarPosition::Bottom)
    );

    let from_missing_file = local_shell_service(
        &home,
        &[("HERDR_CONFIG_PATH", &missing), ("XDG_CONFIG_HOME", &home)],
    );
    assert_eq!(
        from_missing_file.read_herdr_client_tab_bar_position(None),
        Some(HerdrTabBarPosition::Top)
    );

    let from_relative_path = local_shell_service(
        &home,
        &[
            ("HERDR_CONFIG_PATH", "relative/config.toml"),
            ("XDG_CONFIG_HOME", &home),
        ],
    );
    assert_eq!(
        from_relative_path.read_herdr_client_tab_bar_position(None),
        None
    );
}
