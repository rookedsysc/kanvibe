//! 계정 하나가 디스크의 어디에 놓이고 그 위치를 CLI에 어떻게 알리는지를 provider마다 한곳에 모은다.
//! 탐색과 로그인과 계정 생성이 각자 경로 규칙을 들고 있으면 한쪽만 틀린 채로 남는다.

use regex::Regex;
use std::sync::LazyLock;

use crate::node_path;
use crate::usage::types::AiUsageProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiProviderConfigDirSpec {
    pub provider: AiUsageProvider,
    /// CLI에 계정 위치를 알릴 때 쓰는 환경 변수
    pub home_env_var: &'static str,
    /// 기본 계정의 루트를 홈 기준으로 적은 이름. 빈 문자열이면 홈 자체가 루트다
    pub default_root_name: &'static str,
    /// 계정을 나눠 쓰는 형제 루트를 알아보는 접두사
    pub sibling_root_prefix: &'static str,
    /// 루트 아래에서 자격증명이 실제로 놓이는 하위 경로. 루트가 곧 config dir면 빈 문자열이다
    pub config_dir_sub_path: &'static str,
    pub credentials_file_name: &'static str,
    pub default_label: &'static str,
}

const CLAUDE_CONFIG_DIR_SPEC: AiProviderConfigDirSpec = AiProviderConfigDirSpec {
    provider: AiUsageProvider::Claude,
    home_env_var: "CLAUDE_CONFIG_DIR",
    default_root_name: ".claude",
    sibling_root_prefix: ".claude-",
    config_dir_sub_path: "",
    credentials_file_name: ".credentials.json",
    default_label: "Claude",
};

const CODEX_CONFIG_DIR_SPEC: AiProviderConfigDirSpec = AiProviderConfigDirSpec {
    provider: AiUsageProvider::Codex,
    home_env_var: "CODEX_HOME",
    default_root_name: ".codex",
    sibling_root_prefix: ".codex-",
    config_dir_sub_path: "",
    credentials_file_name: "auth.json",
    default_label: "Codex",
};

/// Gemini CLI는 config dir를 직접 받지 않는다. `GEMINI_CLI_HOME`은 루트를 받고 그 아래에 `.gemini`를 만든다.
/// 그래서 기본 계정의 루트는 홈 자체이고, 계정을 나눈 루트도 그 아래에 `.gemini`를 한 겹 더 가진다
const GEMINI_CONFIG_DIR_SPEC: AiProviderConfigDirSpec = AiProviderConfigDirSpec {
    provider: AiUsageProvider::Gemini,
    home_env_var: "GEMINI_CLI_HOME",
    default_root_name: "",
    sibling_root_prefix: ".gemini-",
    config_dir_sub_path: ".gemini",
    credentials_file_name: "oauth_creds.json",
    default_label: "Gemini",
};

pub fn config_dir_spec(provider: AiUsageProvider) -> &'static AiProviderConfigDirSpec {
    match provider {
        AiUsageProvider::Claude => &CLAUDE_CONFIG_DIR_SPEC,
        AiUsageProvider::Codex => &CODEX_CONFIG_DIR_SPEC,
        AiUsageProvider::Gemini => &GEMINI_CONFIG_DIR_SPEC,
    }
}

/// 자격증명이 놓이는 디렉터리. 루트가 곧 config dir인 provider는 루트를 그대로 돌려준다
pub fn to_account_config_dir(spec: &AiProviderConfigDirSpec, account_root: &str) -> String {
    if spec.config_dir_sub_path.is_empty() {
        account_root.to_string()
    } else {
        node_path::join(&[account_root, spec.config_dir_sub_path])
    }
}

/// 기본 계정의 루트
pub fn to_default_account_root(spec: &AiProviderConfigDirSpec, home_directory: &str) -> String {
    if spec.default_root_name.is_empty() {
        home_directory.to_string()
    } else {
        node_path::join(&[home_directory, spec.default_root_name])
    }
}

/// 사용자가 붙인 계정 이름으로 형제 루트 경로를 만든다
pub fn to_named_account_root(
    spec: &AiProviderConfigDirSpec,
    account_name: &str,
    home_directory: &str,
) -> String {
    node_path::join(&[
        home_directory,
        &format!("{}{account_name}", spec.sibling_root_prefix),
    ])
}

/// 형제 루트에서 사용자가 붙였던 계정 이름을 되읽는다. 기본 계정 루트면 `None`
pub fn to_account_name_from_root(
    spec: &AiProviderConfigDirSpec,
    account_root: &str,
) -> Option<String> {
    let root_name = node_path::basename(account_root);
    let account_name = root_name.strip_prefix(spec.sibling_root_prefix)?;
    (!account_name.is_empty()).then(|| account_name.to_string())
}

static ACCOUNT_NAME_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9._-]{1,40}$").expect("계정 이름 패턴"));

/// 계정 이름은 홈 아래 디렉터리 이름이 되므로 경로를 벗어날 수 있는 글자를 받지 않는다
pub fn is_valid_account_name(account_name: &str) -> bool {
    ACCOUNT_NAME_PATTERN.is_match(account_name) && !account_name.starts_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE_HOME: &str = "/home/tester";

    #[test]
    fn root_is_config_dir_for_claude_and_codex() {
        let claude_root = node_path::join(&[FAKE_HOME, ".claude-work"]);
        assert_eq!(
            to_account_config_dir(config_dir_spec(AiUsageProvider::Claude), &claude_root),
            claude_root
        );
        assert_eq!(
            to_account_config_dir(config_dir_spec(AiUsageProvider::Codex), &claude_root),
            claude_root
        );
    }

    #[test]
    fn gemini_config_dir_is_dot_gemini_under_root() {
        let gemini_root = node_path::join(&[FAKE_HOME, ".gemini-work"]);
        assert_eq!(
            to_account_config_dir(config_dir_spec(AiUsageProvider::Gemini), &gemini_root),
            format!("{gemini_root}/.gemini")
        );
    }

    #[test]
    fn claude_and_codex_default_roots_are_dedicated_directories() {
        assert_eq!(
            to_default_account_root(config_dir_spec(AiUsageProvider::Claude), FAKE_HOME),
            "/home/tester/.claude"
        );
        assert_eq!(
            to_default_account_root(config_dir_spec(AiUsageProvider::Codex), FAKE_HOME),
            "/home/tester/.codex"
        );
    }

    #[test]
    fn gemini_default_root_is_home() {
        let spec = config_dir_spec(AiUsageProvider::Gemini);
        let default_root = to_default_account_root(spec, FAKE_HOME);
        assert_eq!(default_root, FAKE_HOME);
        assert_eq!(
            to_account_config_dir(spec, &default_root),
            "/home/tester/.gemini"
        );
    }

    #[test]
    fn named_root_reads_back_its_name() {
        let spec = config_dir_spec(AiUsageProvider::Gemini);
        let account_root = to_named_account_root(spec, "work", FAKE_HOME);
        assert_eq!(account_root, "/home/tester/.gemini-work");
        assert_eq!(
            to_account_name_from_root(spec, &account_root).as_deref(),
            Some("work")
        );
    }

    #[test]
    fn default_root_has_no_name() {
        let spec = config_dir_spec(AiUsageProvider::Claude);
        assert_eq!(
            to_account_name_from_root(spec, &to_default_account_root(spec, FAKE_HOME)),
            None
        );
    }

    #[test]
    fn accepts_safe_directory_names() {
        assert!(is_valid_account_name("work"));
        assert!(is_valid_account_name("work-2"));
        assert!(is_valid_account_name("work_2.a"));
    }

    #[test]
    fn rejects_escaping_or_hidden_names() {
        for name in ["../escape", "work/child", "with space", ".hidden", ""] {
            assert!(!is_valid_account_name(name), "{name}");
        }
    }
}
