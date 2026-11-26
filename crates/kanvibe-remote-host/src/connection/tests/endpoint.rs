//! 실제 `ssh -G`(임시 설정 파일, 네트워크 없음)로 본 실효 설정에서 원격 계정 식별자를 만든다.

use std::path::Path;
use std::process::{Command, Stdio};

use kanvibe_process::ssh_config::SshHostConfig;

use crate::ssh_transport::{
    EffectiveSshConfig, build_endpoint_probe_args, endpoint_key_from_effective_config,
};

/// 사용자 `~/.ssh/config` 대신 임시 설정 파일만 읽는 `ssh -G`
struct FixtureEffectiveSshConfig<'path> {
    config_file: &'path Path,
}

impl EffectiveSshConfig for FixtureEffectiveSshConfig<'_> {
    fn resolve(&self, ssh_arguments: &[String]) -> Result<String, String> {
        let output = Command::new("ssh")
            .arg("-F")
            .arg(self.config_file)
            .arg("-G")
            .args(ssh_arguments)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

fn endpoint_keys(proxy_command: &str) -> (String, String) {
    let directory = tempfile::Builder::new()
        .prefix("kvhost-identity-")
        .tempdir_in("/tmp")
        .unwrap();
    let config_file = directory.path().join("ssh-config");
    std::fs::write(
        &config_file,
        format!("Host route-a route-b\n  HostName 127.0.0.1\n  User fixture\n  {proxy_command}\n"),
    )
    .unwrap();
    let resolver = FixtureEffectiveSshConfig {
        config_file: &config_file,
    };
    let key = |host: &str| {
        let config = SshHostConfig {
            host: host.into(),
            hostname: "127.0.0.1".into(),
            port: 22,
            username: "fixture".into(),
            private_key_path: "/tmp/kvhost-identity-unused-key".into(),
        };
        let effective = resolver
            .resolve(&build_endpoint_probe_args(&config))
            .unwrap();
        (
            endpoint_key_from_effective_config(&config, &effective),
            effective,
        )
    };
    let (first, first_effective) = key("route-a");
    let (second, second_effective) = key("route-b");
    let strip = |effective: &str| {
        effective
            .lines()
            .filter(|line| !line.starts_with("host "))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(strip(&first_effective), strip(&second_effective));
    (first, second)
}

#[test]
fn ordinary_aliases_with_identical_effective_configuration_share_a_key() {
    let (first, second) = endpoint_keys("");

    assert_eq!(first, second);
}

#[test]
fn a_proxy_command_keeping_the_original_host_token_separates_aliases() {
    let (first, second) = endpoint_keys("ProxyCommand ssh gateway nc %n 22");

    assert_ne!(first, second);
}

#[test]
fn a_proxy_command_keeping_the_host_key_alias_token_separates_aliases() {
    let (first, second) = endpoint_keys("ProxyCommand ssh gateway nc %k 22");

    assert_ne!(first, second);
}
