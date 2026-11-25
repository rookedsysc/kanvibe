//! 원격에서 띄우는 사용자 명령·PTY·daemon의 환경(TS 원격 `createLocalShellEnvironment()`).
//!
//! daemon 자신은 SSH 로그인 셸에서 물려받은 환경으로 돌지만, 그 환경에 섞였을 수 있는 서버 런타임 값
//! (`PORT`, `HOST`, `NODE_ENV`, `KANVIBE_*`)은 사용자 명령과 터미널에 넘기지 않는다. 프로세스 전역 환경은 읽기만 한다.
//! 데스크탑 쪽 `kanvibe_process::shell_environment`와 같은 규칙이지만, 정적 바이너리가 rusqlite까지 끌고 오지 않도록 따로 둔다.

use std::collections::BTreeMap;

pub type ChildEnvironment = BTreeMap<String, String>;

const BLOCKED_KEYS: [&str; 3] = ["PORT", "HOST", "NODE_ENV"];
const DEFAULT_LOCALE: &str = "en_US.UTF-8";
#[cfg(target_os = "macos")]
const MAC_LOCAL_COMMAND_PATHS: [&str; 6] = [
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/opt/local/bin",
    "/opt/local/sbin",
];

fn is_forwarded_key(key: &str) -> bool {
    !BLOCKED_KEYS.contains(&key) && !key.starts_with("KANVIBE_")
}

/// 부모 환경에서 서버 런타임 값을 걷어 내고 HOME·PATH·로케일을 채운다
pub fn create_child_environment(parent: &ChildEnvironment) -> ChildEnvironment {
    let mut environment: ChildEnvironment = parent
        .iter()
        .filter(|(key, _)| is_forwarded_key(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let non_empty = |key: &str| parent.get(key).filter(|value| !value.is_empty()).cloned();
    let home = non_empty("HOME").unwrap_or_else(|| "/".to_string());
    environment.insert("PATH".into(), merged_path(parent.get("PATH"), &home));
    environment.insert("HOME".into(), home);
    for key in ["LANG", "LC_ALL"] {
        let locale = non_empty(key).unwrap_or_else(|| DEFAULT_LOCALE.to_string());
        environment.insert(key.into(), locale);
    }
    environment
}

/// 지금 프로세스 환경을 읽기만 해서 만든다
pub fn current_child_environment() -> ChildEnvironment {
    create_child_environment(&std::env::vars().collect())
}

#[cfg(target_os = "macos")]
fn extra_path_entries(home: &str) -> Vec<String> {
    let user_paths = [
        ".local/bin",
        ".cargo/bin",
        ".opencode/bin",
        "Library/pnpm",
        ".bun/bin",
    ];
    MAC_LOCAL_COMMAND_PATHS
        .iter()
        .map(|entry| entry.to_string())
        .chain(
            user_paths
                .iter()
                .map(|suffix| format!("{}/{suffix}", home.trim_end_matches('/'))),
        )
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn extra_path_entries(_home: &str) -> Vec<String> {
    Vec::new()
}

/// PATH 항목을 이어 붙이고, 빈 항목과 중복을 앞에 나온 순서대로 정리한다
fn merged_path(parent_path: Option<&String>, home: &str) -> String {
    let extra = extra_path_entries(home);
    let mut merged: Vec<&str> = Vec::new();
    let entries = parent_path
        .map(|path| path.split(':').collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(extra.iter().map(String::as_str))
        .map(str::trim)
        .filter(|entry| !entry.is_empty());
    for entry in entries {
        if !merged.contains(&entry) {
            merged.push(entry);
        }
    }
    merged.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_runtime_values_are_removed_and_locale_is_filled() {
        let parent: ChildEnvironment = [
            ("PORT", "4000"),
            ("HOST", "0.0.0.0"),
            ("NODE_ENV", "production"),
            ("KANVIBE_SECRET", "x"),
            ("HOME", "/home/fixture"),
            ("PATH", "/usr/bin::/bin:/usr/bin"),
            ("TERM", "xterm"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();

        let environment = create_child_environment(&parent);

        for key in ["PORT", "HOST", "NODE_ENV", "KANVIBE_SECRET"] {
            assert!(!environment.contains_key(key), "{key} leaked");
        }
        assert_eq!(environment["TERM"], "xterm");
        assert_eq!(environment["LANG"], DEFAULT_LOCALE);
        assert!(environment["PATH"].starts_with("/usr/bin:/bin"));
    }
}
