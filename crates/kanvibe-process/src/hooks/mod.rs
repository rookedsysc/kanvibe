//! AI 도구 hook 설치와 hook 셸 스크립트 생성.

pub mod claude;
pub mod client_runtime;
pub mod codex;
pub mod endpoint;
pub mod file_writer;
pub mod gemini;
pub mod install_bundle;
pub mod installer;
mod installer_jobs;
pub mod installer_verification;
pub mod json_settings;
pub mod open_code;
mod open_code_plugin_source;
pub mod open_code_registry;
pub mod provider;
pub mod providers;
pub mod server_status;
pub mod shell_script;
pub mod socket_path;
pub mod target_registration;

#[cfg(test)]
mod claude_tests;
#[cfg(test)]
mod codex_tests;
#[cfg(test)]
mod file_writer_tests;
#[cfg(test)]
mod gemini_tests;
#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod install_bundle_tests;
#[cfg(test)]
mod installer_fakes;
#[cfg(test)]
mod installer_golden_tests;
#[cfg(test)]
mod installer_migration_tests;
#[cfg(test)]
pub(crate) mod installer_test_support;
#[cfg(test)]
mod installer_tests;
#[cfg(test)]
mod open_code_golden_tests;
#[cfg(test)]
mod open_code_registry_tests;
#[cfg(test)]
mod open_code_tests;
#[cfg(test)]
mod open_code_transport_tests;
#[cfg(test)]
pub(crate) mod script_test_support;
#[cfg(test)]
mod server_status_tests;
#[cfg(test)]
mod socket_transport_tests;
#[cfg(test)]
mod target_registration_tests;

/// 설치할 hook 파일 하나(TS `ShellHookProviderFile`). `mode`가 없으면 기본 권한으로 쓴다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookProviderFile {
    pub file_path: String,
    pub content: String,
    pub mode: Option<u32>,
}
