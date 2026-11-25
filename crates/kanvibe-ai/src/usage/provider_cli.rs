//! provider CLI로 로그인 상태를 묻고 로그아웃을 맡긴다. 로그인 자체는 PTY를 다루는 쪽이 이 모듈이 준 명령을 띄운다.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::javascript::parse_json;
use crate::usage::context::{ProcessRequest, UsageContext, UsagePlatform};
use crate::usage::provider_config_dir::{
    AiProviderConfigDirSpec, config_dir_spec, to_default_account_root,
};
use crate::usage::shared::read_non_empty_string;
use crate::usage::types::AiUsageProvider;

/// 상태 조회는 토큰 갱신을 위해 네트워크를 한 번 타므로 사용량 조회보다 넉넉히 준다
const PROVIDER_CLI_TIMEOUT_MS: u64 = 30_000;
const PROVIDER_CLI_OUTPUT_MAX_BYTES: usize = 1024 * 1024;

/// macOS GUI 앱은 로그인 셸의 PATH를 물려받지 못해 흔한 설치 위치를 더한다(TS `createLocalShellEnvironment`)
const MAC_LOCAL_COMMAND_PATHS: [&str; 6] = [
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/opt/local/bin",
    "/opt/local/sbin",
];

/// CLI 자식 환경에 앱 환경에서 골라 옮기는 값. 셸·로캘·터미널(`USER`, `LOGNAME`, `SHELL`, `TMPDIR`, `TERM`, `COLORTERM`,
/// `LC_CTYPE`), XDG 디렉터리, 로그인 화면이 브라우저를 여는 데 쓰는 데스크톱 세션(`DISPLAY`, `WAYLAND_DISPLAY`,
/// `DBUS_SESSION_BUS_ADDRESS`, `BROWSER`), 회사망에서 OAuth가 통하도록 하는 프록시·인증서 변수만 담는다.
/// `PATH`, `HOME`, `LANG`, `LC_ALL`은 아래에서 따로 정하고, provider 위치 변수는 계정마다 새로 얹는다
const PROVIDER_CLI_INHERITED_VARIABLES: [&str; 25] = [
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "TERM",
    "COLORTERM",
    "LC_CTYPE",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
    "XDG_RUNTIME_DIR",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "DBUS_SESSION_BUS_ADDRESS",
    "BROWSER",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAuthStatus {
    pub is_logged_in: bool,
    /// 계정을 알아볼 표시 이름. CLI가 알려줄 때만 채운다
    pub label: Option<String>,
    /// 구독 등급 표시값. CLI가 알려줄 때만 채운다
    pub plan_name: Option<String>,
}

struct AiProviderCliSpec {
    command: &'static str,
    login_args: &'static [&'static str],
    /// 저장된 자격증명을 지우는 명령을 가진 provider만 채운다
    logout_args: Option<&'static [&'static str]>,
    /// 로그인 여부를 물을 수 있는 provider만 채운다
    status_args: Option<&'static [&'static str]>,
}

/// Gemini CLI에는 로그인 여부를 물을 하위 명령이 없고 로그인도 첫 실행 화면에서 고르므로, 상태는 자격증명 파일로만 판단한다
fn cli_spec(provider: AiUsageProvider) -> AiProviderCliSpec {
    match provider {
        AiUsageProvider::Claude => AiProviderCliSpec {
            command: "claude",
            login_args: &["auth", "login", "--claudeai"],
            logout_args: Some(&["auth", "logout"]),
            status_args: Some(&["auth", "status", "--json"]),
        },
        AiUsageProvider::Codex => AiProviderCliSpec {
            command: "codex",
            login_args: &["login"],
            logout_args: Some(&["logout"]),
            status_args: Some(&["login", "status"]),
        },
        AiUsageProvider::Gemini => AiProviderCliSpec {
            command: "gemini",
            login_args: &[],
            logout_args: None,
            status_args: None,
        },
    }
}

/// `claude auth status --json`은 로그인 여부와 계정 이메일과 구독 등급을 한 번에 준다
fn parse_claude_auth_status(stdout: &str) -> Option<ProviderAuthStatus> {
    let parsed = parse_json(stdout)?;
    Some(ProviderAuthStatus {
        is_logged_in: parsed.get("loggedIn") == Some(&Value::Bool(true)),
        label: read_non_empty_string(parsed.get("email")),
        plan_name: read_non_empty_string(parsed.get("subscriptionType")),
    })
}

/// `codex login status`는 사람이 읽는 한 줄만 준다. 등급과 계정은 사용량 응답 쪽이 알려준다
fn parse_codex_auth_status(stdout: &str) -> Option<ProviderAuthStatus> {
    let output = stdout.to_lowercase();
    let has_logged_in = output.contains("logged in");
    let has_not_logged_in = output.contains("not logged in");
    if !has_logged_in && !has_not_logged_in {
        return None;
    }
    Some(ProviderAuthStatus {
        is_logged_in: has_logged_in && !has_not_logged_in,
        label: None,
        plan_name: None,
    })
}

fn parse_auth_status(provider: AiUsageProvider, stdout: &str) -> Option<ProviderAuthStatus> {
    match provider {
        AiUsageProvider::Claude => parse_claude_auth_status(stdout),
        AiUsageProvider::Codex => parse_codex_auth_status(stdout),
        AiUsageProvider::Gemini => None,
    }
}

/// TS `mergeShellPathEntries`. 빈 항목을 버리고 처음 나온 순서대로 겹치지 않게 잇는다
fn merge_path_entries(path_values: &[&str]) -> String {
    let mut entries: Vec<&str> = Vec::new();
    for entry in path_values
        .iter()
        .flat_map(|value| value.split(':'))
        .map(str::trim)
    {
        if !entry.is_empty() && !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    entries.join(":")
}

fn create_local_path(context: &UsageContext) -> String {
    let process_path = context.variable("PATH").unwrap_or_default();
    if context.platform != UsagePlatform::Darwin {
        return merge_path_entries(&[process_path]);
    }
    let home = context.home_directory.as_str();
    let user_paths = [
        ".local/bin",
        ".cargo/bin",
        ".opencode/bin",
        "Library/pnpm",
        ".bun/bin",
    ]
    .map(|suffix| crate::node_path::join(&[home, suffix]));
    let extra_paths: Vec<&str> = MAC_LOCAL_COMMAND_PATHS
        .iter()
        .copied()
        .chain(user_paths.iter().map(String::as_str))
        .collect();
    merge_path_entries(&[process_path, &extra_paths.join(":")])
}

/// 계정 위치를 알리는 변수 하나만 얹은 CLI 자식 환경. 서버 런타임 값과 KanVibe 내부 값은 처음부터 고르지 않는다.
/// 기본 루트일 때는 그 변수를 넣지 않는다. Claude Code는 `CLAUDE_CONFIG_DIR`가 있으면 설정 파일을 config dir 안에 새로 만들어
/// 같은 계정의 신원이 홈과 config dir 두 곳으로 갈리기 때문이다
pub fn create_provider_cli_environment(
    context: &UsageContext,
    spec: &AiProviderConfigDirSpec,
    account_root: &str,
) -> BTreeMap<String, String> {
    let mut environment: BTreeMap<String, String> = PROVIDER_CLI_INHERITED_VARIABLES
        .iter()
        .filter_map(|name| {
            context
                .variable(name)
                .map(|value| (name.to_string(), value.to_string()))
        })
        .collect();
    let locale = |name: &str| {
        context
            .variable(name)
            .filter(|value| !value.is_empty())
            .unwrap_or("en_US.UTF-8")
            .to_string()
    };
    environment.insert("HOME".to_string(), context.home_directory.clone());
    environment.insert("PATH".to_string(), create_local_path(context));
    environment.insert("LANG".to_string(), locale("LANG"));
    environment.insert("LC_ALL".to_string(), locale("LC_ALL"));
    if account_root != to_default_account_root(spec, &context.home_directory) {
        environment.insert(spec.home_env_var.to_string(), account_root.to_string());
    }
    environment
}

/// 로그인 세션이 띄울 명령. PTY를 다루는 쪽이 실행 방식을 정하므로 여기서는 이름과 인자만 준다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderLoginCommand {
    pub command: String,
    pub args: Vec<String>,
}

pub fn get_provider_login_command(provider: AiUsageProvider) -> ProviderLoginCommand {
    let spec = cli_spec(provider);
    ProviderLoginCommand {
        command: spec.command.to_string(),
        args: spec.login_args.iter().map(|arg| arg.to_string()).collect(),
    }
}

fn run_provider_cli(
    context: &UsageContext,
    provider: AiUsageProvider,
    account_root: &str,
    args: &[&str],
) -> Option<i32> {
    run_provider_cli_with_output(context, provider, account_root, args).0
}

fn run_provider_cli_with_output(
    context: &UsageContext,
    provider: AiUsageProvider,
    account_root: &str,
    args: &[&str],
) -> (Option<i32>, String) {
    let output = context.processes.run(&ProcessRequest {
        program: cli_spec(provider).command.to_string(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        environment: create_provider_cli_environment(
            context,
            config_dir_spec(provider),
            account_root,
        ),
        timeout_ms: PROVIDER_CLI_TIMEOUT_MS,
        max_output_bytes: PROVIDER_CLI_OUTPUT_MAX_BYTES,
    });
    (output.exit_code, output.stdout)
}

/// CLI에게 이 계정의 로그인 상태를 묻는다. 물을 수 없는 provider이거나 CLI를 실행하지 못하면 `None`이고,
/// `None`은 "로그아웃"이 아니라 "모른다"이므로 호출부는 자격증명 파일로 판단한 결과를 덮어쓰지 않아야 한다
pub fn read_provider_auth_status(
    context: &UsageContext,
    provider: AiUsageProvider,
    account_root: &str,
) -> Option<ProviderAuthStatus> {
    let status_args = cli_spec(provider).status_args?;
    let (exit_code, stdout) =
        run_provider_cli_with_output(context, provider, account_root, status_args);
    exit_code?;
    parse_auth_status(provider, &stdout)
}

/// 저장된 자격증명을 CLI에게 지우게 한다. 지울 명령이 없는 provider는 아무것도 하지 않고 `false`다
pub fn logout_through_cli(
    context: &UsageContext,
    provider: AiUsageProvider,
    account_root: &str,
) -> bool {
    let Some(logout_args) = cli_spec(provider).logout_args else {
        return false;
    };
    run_provider_cli(context, provider, account_root, logout_args) == Some(0)
}

#[cfg(test)]
mod tests;
