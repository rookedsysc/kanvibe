//! hook 설치 검증 결과를 실패 항목과 로그로 바꾼다(TS `kanvibeHooksInstaller.ts`의 검증 판정 부분).

use super::providers::{KanvibeHookProvider, KanvibeHookStatus};

/// 설치 실패로 보지 않는 확인 항목. script의 KANVIBE_URL은 targets.json이 없을 때만 쓰는 fallback이다
const NON_BLOCKING_HOOK_STATUS_CHECKS: [&str; 3] = [
    "hasReachableHookServer",
    "hasDuplicateKanvibePlugins",
    "hasExpectedHookServerUrl",
];
const OPEN_CODE_NON_BLOCKING_INSTALL_CHECKS: [&str; 1] = ["hasRegisteredPlugin"];
const REGISTERED_PLUGIN_CHECK: &str = "hasRegisteredPlugin";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookLogLevel {
    Info,
    Warn,
    Error,
}

/// provider 하나의 검증 결과 로그(TS `[hooks] <provider> verification` payload)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookVerificationRecord {
    pub provider: &'static str,
    pub target_path: String,
    pub task_id: String,
    pub ssh_host: Option<String>,
    pub is_installed: bool,
    pub failed_checks: Vec<String>,
    pub bound_task_id: Option<String>,
    pub registered_hook_target_url: Option<String>,
    pub configured_hook_server_url: Option<String>,
    pub expected_hook_server_url: Option<String>,
    pub registered_plugin_urls: Option<Vec<String>>,
    pub plugin_registration_error: Option<String>,
}

/// 설치기 로그. 앱은 표준 오류로 남기고 테스트는 기록을 검사한다
pub trait HookInstallLog: Send + Sync {
    fn verification(&self, level: HookLogLevel, record: &HookVerificationRecord);
    /// 재시도, 검증 불가, provider 설치 실패 같은 그 밖의 경고
    fn event(&self, level: HookLogLevel, message: &str, detail: &str);
}

pub struct StderrHookInstallLog;

impl HookInstallLog for StderrHookInstallLog {
    fn verification(&self, _level: HookLogLevel, record: &HookVerificationRecord) {
        eprintln!("[hooks] {} verification {record:?}", record.provider);
    }

    fn event(&self, _level: HookLogLevel, message: &str, detail: &str) {
        eprintln!("{message} {detail}");
    }
}

/// 검증에 실패한 provider. 상태를 읽지 못했으면 오류 메시지, 읽었으면 실패한 확인 항목을 담는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HookVerificationFailure {
    Checks {
        provider: &'static str,
        failed_checks: Vec<String>,
    },
    Unavailable {
        provider: &'static str,
        error: String,
    },
}

pub(super) fn format_hook_verification_failures(failures: &[HookVerificationFailure]) -> String {
    let formatted: Vec<String> = failures
        .iter()
        .map(|failure| match failure {
            HookVerificationFailure::Unavailable { provider, error } => {
                format!("{provider}({error})")
            }
            HookVerificationFailure::Checks {
                provider,
                failed_checks,
            } if !failed_checks.is_empty() => {
                format!("{provider}({})", failed_checks.join(", "))
            }
            HookVerificationFailure::Checks { provider, .. } => provider.to_string(),
        })
        .collect();
    formatted.join(", ")
}

/// 값이 거짓인 차단 항목. OpenCode 등록 조회가 실패해 등록 여부를 모르면 그것도 등록 누락과 같은 비차단 항목으로 센다
pub(super) fn hook_verification_failed_checks(status: &KanvibeHookStatus) -> Vec<String> {
    let mut failed_checks: Vec<String> = status
        .checks()
        .into_iter()
        .filter(|(name, value)| {
            *value == Some(false) && !NON_BLOCKING_HOOK_STATUS_CHECKS.contains(name)
        })
        .map(|(name, _)| name.to_string())
        .collect();
    let has_registration_error = status
        .plugin_registration_error()
        .is_some_and(|error| !error.is_empty());
    if has_registration_error
        && !failed_checks
            .iter()
            .any(|check| check == REGISTERED_PLUGIN_CHECK)
    {
        failed_checks.push(REGISTERED_PLUGIN_CHECK.into());
    }
    failed_checks
}

/// 설치된 provider와 OpenCode가 아닌 provider는 실패 항목이 모두 치명적이다. 미설치 OpenCode는 등록 누락을 빼고 본다
pub(super) fn fatal_hook_verification_failed_checks(
    provider: KanvibeHookProvider,
    status: &KanvibeHookStatus,
    failed_checks: &[String],
) -> Vec<String> {
    if status.is_installed() || provider != KanvibeHookProvider::OpenCode {
        return failed_checks.to_vec();
    }
    failed_checks
        .iter()
        .filter(|check| !OPEN_CODE_NON_BLOCKING_INSTALL_CHECKS.contains(&check.as_str()))
        .cloned()
        .collect()
}

/// 미설치인데 치명적인 항목이 있거나, 실패 항목이 없어 원인을 알 수 없으면 실패다
pub(super) fn should_fail_hook_verification(
    status: &KanvibeHookStatus,
    failed_checks: &[String],
    fatal_failed_checks: &[String],
) -> bool {
    !status.is_installed() && (!fatal_failed_checks.is_empty() || failed_checks.is_empty())
}

/// 검증 로그 payload
pub(super) fn build_verification_record(
    provider: KanvibeHookProvider,
    status: &KanvibeHookStatus,
    failed_checks: &[String],
    target: (&str, &str, Option<&str>),
) -> HookVerificationRecord {
    let (target_path, task_id, ssh_host) = target;
    HookVerificationRecord {
        provider: provider.label(),
        target_path: target_path.to_string(),
        task_id: task_id.to_string(),
        ssh_host: ssh_host.map(str::to_string),
        is_installed: status.is_installed(),
        failed_checks: failed_checks.to_vec(),
        bound_task_id: status.bound_task_id().map(str::to_string),
        registered_hook_target_url: status.registered_hook_target_url().map(str::to_string),
        configured_hook_server_url: status.configured_hook_server_url().map(str::to_string),
        expected_hook_server_url: status.expected_hook_server_url().map(str::to_string),
        registered_plugin_urls: status.registered_plugin_urls().map(<[String]>::to_vec),
        plugin_registration_error: status.plugin_registration_error().map(str::to_string),
    }
}
