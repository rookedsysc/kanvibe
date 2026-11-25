//! 로컬 자식 프로세스·셸이 받을 환경 변수를 만든다.
//! 서버 런타임 값(`PORT`, `HOST`, `NODE_ENV`, `KANVIBE_*`)이 사용자 셸로 새지 않도록 부모 환경에서 걸러 내고,
//! 프로세스 전역 환경은 읽기만 하고 절대 바꾸지 않는다.

use std::collections::BTreeMap;

/// 자식 프로세스에 그대로 넘길 환경 변수 전체. `Command::env_clear().envs(..)`로 쓴다
pub type ShellEnvironment = BTreeMap<String, String>;

const PATH_DELIMITER: char = ':';

const MAC_LOCAL_COMMAND_PATHS: [&str; 6] = [
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/opt/local/bin",
    "/opt/local/sbin",
];

const BLOCKED_LOCAL_SHELL_ENV_KEYS: [&str; 3] = ["PORT", "HOST", "NODE_ENV"];

const DEFAULT_LOCALE: &str = "en_US.UTF-8";

/// macOS GUI 앱은 로그인 셸의 PATH를 받지 못하므로 Homebrew와 사용자 bin 경로를 덧붙인다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellPlatform {
    Mac,
    Other,
}

impl ShellPlatform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Other
        }
    }
}

fn user_local_command_paths(home_directory: &str) -> Vec<String> {
    [
        ".local/bin",
        ".cargo/bin",
        ".opencode/bin",
        "Library/pnpm",
        ".bun/bin",
    ]
    .iter()
    .map(|suffix| format!("{}/{suffix}", home_directory.trim_end_matches('/')))
    .collect()
}

fn should_include_local_shell_environment_key(key: &str) -> bool {
    !BLOCKED_LOCAL_SHELL_ENV_KEYS.contains(&key) && !key.starts_with("KANVIBE_")
}

/// 여러 PATH 값을 이어 붙이고, 빈 항목과 중복을 앞에 나온 순서대로 정리한다
pub fn merge_shell_path_entries(path_values: &[Option<&str>]) -> String {
    let mut merged_entries: Vec<&str> = Vec::new();
    let path_entries = path_values
        .iter()
        .flat_map(|path_value| path_value.unwrap_or_default().split(PATH_DELIMITER))
        .map(str::trim)
        .filter(|path_entry| !path_entry.is_empty());
    for path_entry in path_entries {
        if !merged_entries.contains(&path_entry) {
            merged_entries.push(path_entry);
        }
    }
    merged_entries.join(&PATH_DELIMITER.to_string())
}

fn non_empty<'a>(environment: &'a ShellEnvironment, key: &str) -> Option<&'a str> {
    environment
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
}

/// 부모 환경에서 서버 런타임 값을 걷어 내고 HOME·PATH·로케일을 채운 로컬 셸 환경.
/// `fallback_home_directory`는 부모 환경에 HOME이 없을 때 쓰는 OS 사용자 홈이다.
pub fn create_local_shell_environment(
    parent_environment: &ShellEnvironment,
    platform: ShellPlatform,
    fallback_home_directory: &str,
) -> ShellEnvironment {
    let home_directory = non_empty(parent_environment, "HOME")
        .unwrap_or(fallback_home_directory)
        .to_string();
    let extra_path = match platform {
        ShellPlatform::Mac => MAC_LOCAL_COMMAND_PATHS
            .iter()
            .map(|path_entry| path_entry.to_string())
            .chain(user_local_command_paths(&home_directory))
            .collect::<Vec<_>>()
            .join(&PATH_DELIMITER.to_string()),
        ShellPlatform::Other => String::new(),
    };

    let mut environment: ShellEnvironment = parent_environment
        .iter()
        .filter(|(key, _)| should_include_local_shell_environment_key(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let path = merge_shell_path_entries(&[
        parent_environment.get("PATH").map(String::as_str),
        Some(&extra_path),
    ]);
    let lang = non_empty(parent_environment, "LANG").unwrap_or(DEFAULT_LOCALE);
    let lc_all = non_empty(parent_environment, "LC_ALL").unwrap_or(DEFAULT_LOCALE);
    environment.insert("HOME".into(), home_directory.clone());
    environment.insert("PATH".into(), path);
    environment.insert("LANG".into(), lang.to_string());
    environment.insert("LC_ALL".into(), lc_all.to_string());
    environment
}

/// 지금 프로세스의 환경을 읽기만 해서 만든 로컬 셸 환경
pub fn create_current_local_shell_environment() -> ShellEnvironment {
    let parent_environment: ShellEnvironment = std::env::vars().collect();
    create_local_shell_environment(
        &parent_environment,
        ShellPlatform::current(),
        &current_user_home_directory(),
    )
}

/// HOME 환경 변수가 없을 때 쓰는 OS 사용자 홈. Node `os.homedir()`처럼 passwd 항목을 본다
pub fn current_user_home_directory() -> String {
    if let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) {
        return home.to_string_lossy().into_owned();
    }
    passwd_home_directory().unwrap_or_default()
}

fn passwd_home_directory() -> Option<String> {
    // SAFETY: getpwuid는 정적 버퍼를 돌려주므로 바로 복사하고, 다른 스레드가 같은 버퍼를 쓰기 전에 읽기를 끝낸다
    unsafe {
        let entry = libc::getpwuid(libc::getuid());
        if entry.is_null() || (*entry).pw_dir.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr((*entry).pw_dir)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(entries: &[(&str, &str)]) -> ShellEnvironment {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    // parity: TS-lib-shellEnvironment-10e84e
    #[test]
    fn does_not_leak_kanvibe_or_generic_runtime_variables_into_local_shells() {
        let parent_environment = environment(&[
            ("PORT", "9736"),
            ("HOST", "0.0.0.0"),
            ("NODE_ENV", "production"),
            ("KANVIBE_HOST", "0.0.0.0"),
            ("KANVIBE_APP_DATA_DIR", "/tmp/kanvibe-app-data"),
            ("KANVIBE_DESKTOP", "true"),
            ("KANVIBE_REMOTE_SSH_COMMAND_TIMEOUT_MS", "1000"),
            ("PATH", "/usr/bin"),
            ("HOME", "/home/terminal-user"),
        ]);

        let shell_environment =
            create_local_shell_environment(&parent_environment, ShellPlatform::Other, "/unused");

        for blocked_key in [
            "PORT",
            "HOST",
            "NODE_ENV",
            "KANVIBE_HOST",
            "KANVIBE_APP_DATA_DIR",
            "KANVIBE_DESKTOP",
            "KANVIBE_REMOTE_SSH_COMMAND_TIMEOUT_MS",
        ] {
            assert!(
                !shell_environment.contains_key(blocked_key),
                "{blocked_key}"
            );
        }
        assert!(shell_environment["PATH"].contains("/usr/bin"));
        assert_eq!(shell_environment["HOME"], "/home/terminal-user");
        assert_eq!(shell_environment["LANG"], "en_US.UTF-8");
        assert_eq!(shell_environment["LC_ALL"], "en_US.UTF-8");
    }

    #[test]
    fn mac_shells_get_homebrew_and_user_command_paths_without_duplicates() {
        let parent_environment = environment(&[
            ("PATH", "/usr/bin:/opt/homebrew/bin:"),
            ("HOME", "/Users/me"),
        ]);

        let shell_environment =
            create_local_shell_environment(&parent_environment, ShellPlatform::Mac, "/unused");

        assert_eq!(
            shell_environment["PATH"],
            "/usr/bin:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/local/sbin:/opt/local/bin:/opt/local/sbin:/Users/me/.local/bin:/Users/me/.cargo/bin:/Users/me/.opencode/bin:/Users/me/Library/pnpm:/Users/me/.bun/bin"
        );
    }

    #[test]
    fn missing_home_falls_back_to_the_os_user_home() {
        let shell_environment =
            create_local_shell_environment(&environment(&[]), ShellPlatform::Other, "/home/os");

        assert_eq!(shell_environment["HOME"], "/home/os");
        assert_eq!(shell_environment["PATH"], "");
    }
}
