//! `~/.ssh/config` 파싱과 ssh·scp 인자 생성(TS `sshConfig.ts`).
//! 호스트 정의는 `Host`, `HostName`, `Port`, `User`, `IdentityFile`만 해석한다. 연결은 별칭으로 하므로 나머지 옵션은 OpenSSH가 직접 적용한다.
//! ControlMaster 소켓은 KanVibe 전용 디렉터리(`<HOME>/.kanvibe`) 아래 프로세스별 랜덤 이름으로 둔다.

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::shell_environment::ShellEnvironment;

const SSH_CONFIG_CACHE_TTL: Duration = Duration::from_millis(5000);
const KANVIBE_SSH_CONTROL_PERSIST: &str = "10m";
const KANVIBE_SSH_CONNECT_TIMEOUT_SECONDS: u32 = 8;
const KANVIBE_SSH_SERVER_ALIVE_INTERVAL_SECONDS: u32 = 5;
const KANVIBE_SSH_SERVER_ALIVE_COUNT_MAX: u32 = 2;

/// 해석한 호스트 하나. 필드 순서는 TS 객체와 같아 JSON 직렬화 결과(마스터 식별자)도 같다
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshHostConfig {
    pub host: String,
    pub hostname: String,
    pub port: i64,
    pub username: String,
    pub private_key_path: String,
}

impl SshHostConfig {
    /// TS `JSON.stringify(hostConfig)`. 인증 설정이 바뀌면 다른 마스터를 쓰도록 식별자로 쓴다
    pub fn identity_json(&self) -> String {
        serde_json::to_string(self).expect("문자열과 숫자만 담은 구조체")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConnectionReuseOptions {
    pub control_path: String,
    pub control_persist: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshConnectionHealthOptions {
    pub connect_timeout_seconds: u32,
    pub server_alive_interval_seconds: u32,
    pub server_alive_count_max: u32,
}

/// `buildSSHArgs`의 선택지. 기본값은 TS에서 옵션을 주지 않은 경우와 같다
#[derive(Debug, Clone, Default)]
pub struct SshArgsOptions {
    pub is_tty_forced: bool,
    pub is_tty_disabled: bool,
    pub has_trusted_x11_forwarding: bool,
    pub connection_reuse: Option<SshConnectionReuseOptions>,
    pub connection_health: Option<SshConnectionHealthOptions>,
    pub should_check_host_key_strictly: bool,
}

pub fn ssh_destination(config: &SshHostConfig) -> String {
    if !config.host.is_empty() {
        return config.host.clone();
    }
    format!("{}@{}", config.username, config.hostname)
}

fn push_options(args: &mut Vec<String>, options: &[String]) {
    for option in options {
        args.push("-o".into());
        args.push(option.clone());
    }
}

pub fn build_ssh_args(config: &SshHostConfig, options: &SshArgsOptions) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-i".into(),
        config.private_key_path.clone(),
        "-p".into(),
        config.port.to_string(),
    ];
    push_options(
        &mut args,
        &["BatchMode=yes".into(), "IdentitiesOnly=yes".into()],
    );
    if options.has_trusted_x11_forwarding {
        args.push("-Y".into());
    }
    if options.is_tty_forced {
        args.push("-tt".into());
    } else if options.is_tty_disabled {
        args.push("-T".into());
    }
    if options.should_check_host_key_strictly {
        let strict = [
            "StrictHostKeyChecking=yes",
            "ControlMaster=no",
            "ControlPath=none",
        ];
        push_options(&mut args, &strict.map(String::from));
    } else if let Some(reuse) = &options.connection_reuse {
        let reuse_options = [
            "ControlMaster=auto".to_string(),
            format!("ControlPersist={}", reuse.control_persist),
            format!("ControlPath={}", reuse.control_path),
        ];
        push_options(&mut args, &reuse_options);
    }
    if let Some(health) = &options.connection_health {
        let health_options = [
            format!("ConnectTimeout={}", health.connect_timeout_seconds),
            format!(
                "ServerAliveInterval={}",
                health.server_alive_interval_seconds
            ),
            format!("ServerAliveCountMax={}", health.server_alive_count_max),
        ];
        push_options(&mut args, &health_options);
    }
    args.push(ssh_destination(config));
    args
}

/// scp CLI 인자. scp는 ssh와 달리 포트 플래그가 `-P`(대문자)이고, `-p`는 "파일 속성 보존"이라는 다른 의미라 `build_ssh_args`를 재사용하지 않는다
pub fn build_scp_args(config: &SshHostConfig, local_path: &str, remote_path: &str) -> Vec<String> {
    vec![
        "-i".into(),
        config.private_key_path.clone(),
        "-P".into(),
        config.port.to_string(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "IdentitiesOnly=yes".into(),
        local_path.into(),
        format!("{}:{remote_path}", ssh_destination(config)),
    ]
}

pub fn kanvibe_ssh_connection_health_options() -> SshConnectionHealthOptions {
    SshConnectionHealthOptions {
        connect_timeout_seconds: KANVIBE_SSH_CONNECT_TIMEOUT_SECONDS,
        server_alive_interval_seconds: KANVIBE_SSH_SERVER_ALIVE_INTERVAL_SECONDS,
        server_alive_count_max: KANVIBE_SSH_SERVER_ALIVE_COUNT_MAX,
    }
}

pub fn has_local_x11_display(environment: &ShellEnvironment) -> bool {
    environment
        .get("DISPLAY")
        .is_some_and(|display| !display.is_empty())
}

/// ControlMaster 제어 소켓 디렉터리와 이 실행기의 소유자 이름.
/// 소유자 이름은 실행기마다 랜덤으로 정해, 안정판·개발판이 함께 떠 있거나 PID가 재사용돼도 소켓을 나눠 쓰지 않는다
pub struct SshControlDirectory {
    directory: String,
    owner: String,
}

impl SshControlDirectory {
    pub fn new(home_directory: &str) -> Self {
        let random = uuid::Uuid::new_v4();
        Self {
            directory: format!("{}/.kanvibe", home_directory.trim_end_matches('/')),
            owner: hex::encode(&random.as_bytes()[..6]),
        }
    }

    pub fn path(&self) -> &str {
        &self.directory
    }

    /// 다른 사용자가 소켓 경로를 엿보지 못하게 0700으로 만든다
    pub fn ensure(&self) -> std::io::Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.directory)
    }

    /// 설정이 있으면 그 JSON의 sha256 앞 20자로, 없으면 OpenSSH `%C`로 소켓 이름을 만든다
    pub fn connection_reuse_options(
        &self,
        shard_index: Option<u64>,
        config: Option<&SshHostConfig>,
    ) -> SshConnectionReuseOptions {
        let identity = match config {
            Some(config) => {
                hex::encode(Sha256::digest(config.identity_json().as_bytes()))[..20].to_string()
            }
            None => "%C".to_string(),
        };
        let generation = shard_index
            .map(|index| format!("-{index}"))
            .unwrap_or_default();
        SshConnectionReuseOptions {
            control_path: format!(
                "{}/ssh-{}-{identity}{generation}",
                self.directory, self.owner
            ),
            control_persist: KANVIBE_SSH_CONTROL_PERSIST.to_string(),
        }
    }
}

/// Node `parseInt(value, 10)`. 앞쪽 숫자만 읽고, 숫자가 없으면 `None`(NaN)이다
fn parse_leading_integer(value: &str) -> Option<i64> {
    let trimmed = value.trim_start();
    let (sign, digits_start) = match trimmed.chars().next() {
        Some('-') => (-1, 1),
        Some('+') => (1, 1),
        _ => (1, 0),
    };
    let digits: String = trimmed[digits_start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse::<i64>().ok().map(|number| sign * number)
}

#[derive(Default)]
struct PartialHost {
    aliases: Vec<String>,
    hostname: Option<String>,
    username: Option<String>,
    port: Option<i64>,
    private_key_path: Option<String>,
}

/// 와일드카드가 아닌 별칭마다 기본값(포트 22, 사용자 root, `~/.ssh/id_rsa`)을 채운 설정을 만든다
fn expand_host_aliases(partial: &PartialHost, home_directory: &str) -> Vec<SshHostConfig> {
    if partial.aliases.is_empty() {
        return Vec::new();
    }
    let Some(hostname) = partial
        .hostname
        .as_ref()
        .filter(|hostname| !hostname.is_empty())
    else {
        return Vec::new();
    };
    let concrete_aliases = partial
        .aliases
        .iter()
        .filter(|alias| !alias.is_empty() && !alias.contains(['*', '!', '?']));
    concrete_aliases
        .map(|alias| SshHostConfig {
            host: alias.clone(),
            hostname: hostname.clone(),
            port: partial.port.filter(|port| *port != 0).unwrap_or(22),
            username: partial
                .username
                .clone()
                .filter(|user| !user.is_empty())
                .unwrap_or_else(|| "root".into()),
            private_key_path: partial
                .private_key_path
                .clone()
                .filter(|path| !path.is_empty())
                .unwrap_or_else(|| format!("{home_directory}/.ssh/id_rsa")),
        })
        .collect()
}

fn apply_host_setting(current: &mut PartialHost, key: &str, value: String, home_directory: &str) {
    match key {
        "hostname" => current.hostname = Some(value),
        "user" => current.username = Some(value),
        "port" => current.port = parse_leading_integer(&value),
        "identityfile" => current.private_key_path = Some(value.replacen('~', home_directory, 1)),
        _ => {}
    }
}

/// ssh config 본문을 호스트 목록으로 바꾼다
pub fn parse_ssh_config_content(content: &str, home_directory: &str) -> Vec<SshHostConfig> {
    let mut hosts = Vec::new();
    let mut current: Option<PartialHost> = None;
    for raw_line in content.split('\n') {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let key = parts.next().unwrap_or_default().to_lowercase();
        let value_parts: Vec<String> = parts.map(String::from).collect();
        if key == "host" {
            if let Some(previous) = &current {
                hosts.extend(expand_host_aliases(previous, home_directory));
            }
            current = Some(PartialHost {
                aliases: value_parts,
                ..PartialHost::default()
            });
        } else if let Some(current) = current.as_mut() {
            apply_host_setting(current, &key, value_parts.join(" "), home_directory);
        }
    }
    if let Some(last) = &current {
        hosts.extend(expand_host_aliases(last, home_directory));
    }
    hosts
}

/// `~/.ssh/config`를 5초 동안 캐시해서 읽는다. 읽는 동안 잠금을 쥐어 동시에 들어온 호출이 결과를 함께 쓴다
pub struct SshConfigReader {
    config_path: String,
    home_directory: String,
    cache: Mutex<Option<(Instant, Vec<SshHostConfig>)>>,
}

impl SshConfigReader {
    pub fn new(home_directory: &str) -> Self {
        let home_directory = home_directory.trim_end_matches('/').to_string();
        Self {
            config_path: format!("{home_directory}/.ssh/config"),
            home_directory,
            cache: Mutex::new(None),
        }
    }

    pub fn parse_ssh_config(&self) -> Vec<SshHostConfig> {
        let mut cache = self.cache.lock().unwrap();
        if let Some((expires_at, hosts)) = cache.as_ref()
            && *expires_at > Instant::now()
        {
            return hosts.clone();
        }
        let hosts = match std::fs::read_to_string(Path::new(&self.config_path)) {
            Ok(content) => parse_ssh_config_content(&content, &self.home_directory),
            Err(_) => Vec::new(),
        };
        *cache = Some((Instant::now() + SSH_CONFIG_CACHE_TTL, hosts.clone()));
        hosts
    }

    /// 사용 가능한 SSH 호스트 이름 목록
    pub fn available_hosts(&self) -> Vec<String> {
        self.parse_ssh_config()
            .into_iter()
            .map(|config| config.host)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host_config(private_key_path: &str) -> SshHostConfig {
        SshHostConfig {
            host: "remote-host".into(),
            hostname: "example.com".into(),
            port: 2202,
            username: "tester".into(),
            private_key_path: private_key_path.into(),
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn write_ssh_config(home: &Path, content: &str) {
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        std::fs::write(home.join(".ssh/config"), content).unwrap();
    }

    // parity: TS-lib-sshConfig-031382
    #[test]
    fn expands_concrete_host_aliases_and_skips_wildcard_aliases() {
        let content = "Host app-prod app-bastion *.internal\n  HostName example.com\n  User tester\n  Port 2202\n  IdentityFile ~/.ssh/custom_key\n";

        let hosts = parse_ssh_config_content(content, "/home/local-user");

        let expected = |host: &str| SshHostConfig {
            host: host.into(),
            hostname: "example.com".into(),
            port: 2202,
            username: "tester".into(),
            private_key_path: "/home/local-user/.ssh/custom_key".into(),
        };
        assert_eq!(hosts, [expected("app-prod"), expected("app-bastion")]);
    }

    // parity: TS-lib-sshConfig-e9d305
    #[test]
    fn reuses_parsed_ssh_config_on_repeated_calls() {
        let home = tempfile::tempdir().unwrap();
        write_ssh_config(
            home.path(),
            "Host app-prod\n  HostName example.com\n  User tester\n",
        );
        let reader = SshConfigReader::new(home.path().to_str().unwrap());

        let first = reader.parse_ssh_config();
        write_ssh_config(home.path(), "Host changed\n  HostName other.example.com\n");
        let second = reader.parse_ssh_config();

        assert_eq!(first, second);
        assert_eq!(second[0].host, "app-prod");
        assert_eq!(second[0].port, 22);
        assert_eq!(second[0].username, "tester");
    }

    // parity: TS-lib-sshConfig-15c626
    #[test]
    fn adds_trusted_x11_forwarding_and_forced_tty_before_the_ssh_destination() {
        let options = SshArgsOptions {
            has_trusted_x11_forwarding: true,
            is_tty_forced: true,
            ..SshArgsOptions::default()
        };

        let args = build_ssh_args(&host_config("/tmp/test-key"), &options);

        assert_eq!(
            args,
            strings(&[
                "-i",
                "/tmp/test-key",
                "-p",
                "2202",
                "-o",
                "BatchMode=yes",
                "-o",
                "IdentitiesOnly=yes",
                "-Y",
                "-tt",
                "remote-host"
            ])
        );
    }

    // parity: TS-lib-sshConfig-dcab84
    #[test]
    fn adds_connection_reuse_options_before_the_ssh_destination() {
        let options = SshArgsOptions {
            is_tty_disabled: true,
            connection_reuse: Some(SshConnectionReuseOptions {
                control_path: "/home/local-user/.kanvibe/ssh-%C".into(),
                control_persist: "10m".into(),
            }),
            ..SshArgsOptions::default()
        };

        let args = build_ssh_args(&host_config("/tmp/test-key"), &options);

        assert_eq!(
            args,
            strings(&[
                "-i",
                "/tmp/test-key",
                "-p",
                "2202",
                "-o",
                "BatchMode=yes",
                "-o",
                "IdentitiesOnly=yes",
                "-T",
                "-o",
                "ControlMaster=auto",
                "-o",
                "ControlPersist=10m",
                "-o",
                "ControlPath=/home/local-user/.kanvibe/ssh-%C",
                "remote-host",
            ])
        );
    }

    fn assert_control_path(path: &str, suffix: &str) {
        let pattern = format!("^/home/local-user/\\.kanvibe/ssh-[a-f0-9]{{12}}-{suffix}$");
        assert!(
            regex::Regex::new(&pattern).unwrap().is_match(path),
            "{path}"
        );
    }

    // parity: TS-lib-sshConfig-6d4dce
    #[test]
    fn builds_kanvibe_connection_reuse_options_under_the_app_local_directory() {
        let result =
            SshControlDirectory::new("/home/local-user").connection_reuse_options(None, None);

        assert_control_path(&result.control_path, "%C");
        assert_eq!(result.control_persist, "10m");
    }

    // parity: TS-lib-sshConfig-cf22b7
    #[test]
    fn builds_shard_specific_kanvibe_connection_reuse_options() {
        let result =
            SshControlDirectory::new("/home/local-user").connection_reuse_options(Some(3), None);

        assert_control_path(&result.control_path, "%C-3");
        assert_eq!(result.control_persist, "10m");
    }

    // parity: TS-lib-sshConfig-fe5e93
    #[test]
    fn creates_the_kanvibe_ssh_control_directory_with_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let control = SshControlDirectory::new(home.path().to_str().unwrap());

        control.ensure().unwrap();

        let metadata = std::fs::metadata(control.path()).unwrap();
        assert_eq!(
            control.path(),
            format!("{}/.kanvibe", home.path().display())
        );
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    }

    // parity: TS-lib-sshConfig-55a19e
    #[test]
    fn detects_local_x11_availability_from_display() {
        let with_display: ShellEnvironment = [("DISPLAY".to_string(), ":0".to_string())].into();

        assert!(has_local_x11_display(&with_display));
        assert!(!has_local_x11_display(&ShellEnvironment::new()));
    }

    // parity: TS-lib-sshConfig-a5a4d8
    #[test]
    fn uses_the_uppercase_port_flag_and_appends_host_remote_path_as_the_last_argument() {
        let mut config = host_config("/tmp/test-key");
        config.host = "app-prod".into();

        let args = build_scp_args(&config, "/local/tmp/a.png", "/tmp/kanvibe-paste-abc.png");

        assert_eq!(
            args,
            strings(&[
                "-i",
                "/tmp/test-key",
                "-P",
                "2202",
                "-o",
                "BatchMode=yes",
                "-o",
                "IdentitiesOnly=yes",
                "/local/tmp/a.png",
                "app-prod:/tmp/kanvibe-paste-abc.png",
            ])
        );
    }

    // parity: TS-lib-sshConfig-9d1812
    #[test]
    fn isolates_control_sockets_across_runner_lifetimes() {
        let first =
            SshControlDirectory::new("/home/local-user").connection_reuse_options(None, None);
        let second =
            SshControlDirectory::new("/home/local-user").connection_reuse_options(None, None);

        assert_ne!(first.control_path, second.control_path);
        assert_ne!(first.control_path, "/home/local-user/.kanvibe/ssh-%C");
    }

    // parity: TS-lib-sshConfig-1af4d0
    #[test]
    fn does_not_reuse_the_same_master_identity_after_authentication_settings_change() {
        let control = SshControlDirectory::new("/home/local-user");

        let first = control.connection_reuse_options(Some(0), Some(&host_config("/tmp/key-a")));
        let second = control.connection_reuse_options(Some(0), Some(&host_config("/tmp/key-b")));

        assert_ne!(first.control_path, second.control_path);
        assert!(!first.control_path.contains("key-a"));
    }

    #[test]
    fn identity_json_matches_the_ts_object_key_order() {
        assert_eq!(
            host_config("/tmp/k").identity_json(),
            r#"{"host":"remote-host","hostname":"example.com","port":2202,"username":"tester","privateKeyPath":"/tmp/k"}"#
        );
    }
}
