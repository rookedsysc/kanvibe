//! AI CLI 하나의 hook 설치 규약과 네 provider 목록(TS `kanvibeHookProviders.ts`).
//! 로컬·원격, 단일 provider·일괄 설치가 같은 산출물 생성 규칙을 쓰도록 묶는다.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::HookProviderFile;
use super::claude::{
    ClaudeHooksStatus, build_claude_hook_files, get_claude_hook_install_input_paths,
    get_claude_hooks_status, setup_claude_hooks,
};
use super::client_runtime::{HookInstallContext, HookInstallError};
use super::codex::{
    CodexHooksStatus, build_codex_hook_files, get_codex_hook_install_input_paths,
    get_codex_hooks_status, setup_codex_hooks,
};
use super::gemini::{
    GeminiHooksStatus, build_gemini_hook_files, get_gemini_hook_install_input_paths,
    get_gemini_hooks_status, setup_gemini_hooks,
};
use super::open_code::{
    OpenCodeHooksStatus, build_open_code_hook_files, get_open_code_hook_install_input_paths,
    get_open_code_hooks_status, setup_open_code_hooks,
};
use super::open_code_registry::OpenCodePluginRegistration;
use super::provider::ShellHookScriptsStatus;
use crate::host_file_access::TextFileReadResult;

/// provider가 함께 읽은 파일 묶음
pub type HookFileSnapshot = BTreeMap<String, TextFileReadResult>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KanvibeHookProvider {
    Claude,
    Gemini,
    Codex,
    OpenCode,
}

impl KanvibeHookProvider {
    /// 설치·검증 순서. TS `KANVIBE_HOOK_PROVIDER_MODULES`의 키 순서와 같다
    pub const ALL: [Self; 4] = [Self::Claude, Self::Gemini, Self::Codex, Self::OpenCode];

    /// IPC·설치 키에 쓰는 TS 이름
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::Codex => "codex",
            Self::OpenCode => "openCode",
        }
    }

    /// 로그와 실패 메시지에 쓰는 이름
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
        }
    }
}

/// provider 하나의 설치 상태
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KanvibeHookStatus {
    Claude(ClaudeHooksStatus),
    Gemini(GeminiHooksStatus),
    Codex(CodexHooksStatus),
    OpenCode(OpenCodeHooksStatus),
}

/// 설치 확인 항목 이름과 값. 값이 없으면(TS `undefined`) 확인하지 못한 항목이다
pub type HookStatusCheck = (&'static str, Option<bool>);

fn script_checks(scripts: &ShellHookScriptsStatus) -> Vec<HookStatusCheck> {
    vec![
        ("hasTaskIdBinding", Some(scripts.has_task_id_binding)),
        ("hasStatusMappings", Some(scripts.has_status_mappings)),
        (
            "hasStatusJsonPersistence",
            Some(scripts.has_status_json_persistence),
        ),
        ("hasTargetFanout", Some(scripts.has_target_fanout)),
        (
            "hasBoundedNotifyTimeout",
            Some(scripts.has_bounded_notify_timeout),
        ),
        (
            "hasParallelTargetFanout",
            Some(scripts.has_parallel_target_fanout),
        ),
        (
            "hasExpectedHookServerUrl",
            Some(scripts.has_expected_hook_server_url),
        ),
        (
            "hasReachableHookServer",
            Some(scripts.has_reachable_hook_server),
        ),
        (
            "hasRegisteredHookTarget",
            Some(scripts.has_registered_hook_target),
        ),
    ]
}

fn with_script_checks(
    provider_checks: &[(&'static str, bool)],
    scripts: &ShellHookScriptsStatus,
) -> Vec<HookStatusCheck> {
    let mut checks: Vec<HookStatusCheck> = provider_checks
        .iter()
        .map(|(name, value)| (*name, Some(*value)))
        .collect();
    checks.extend(script_checks(scripts));
    checks
}

fn open_code_checks(status: &OpenCodeHooksStatus) -> Vec<HookStatusCheck> {
    vec![
        ("hasPlugin", Some(status.has_plugin)),
        ("hasTaskIdBinding", Some(status.has_task_id_binding)),
        ("hasStatusEndpoint", Some(status.has_status_endpoint)),
        ("hasEventMappings", Some(status.has_event_mappings)),
        (
            "hasStatusJsonPersistence",
            Some(status.has_status_json_persistence),
        ),
        (
            "hasBoundedNotifyTimeout",
            Some(status.has_bounded_notify_timeout),
        ),
        ("hasTargetFanout", Some(status.has_target_fanout)),
        (
            "hasParallelTargetFanout",
            Some(status.has_parallel_target_fanout),
        ),
        ("hasMainSessionGuard", Some(status.has_main_session_guard)),
        (
            "hasDuplicateProgressGuard",
            Some(status.has_duplicate_progress_guard),
        ),
        ("hasPaneChatBinding", Some(status.has_pane_chat_binding)),
        (
            "hasExpectedHookServerUrl",
            Some(status.has_expected_hook_server_url),
        ),
        (
            "hasReachableHookServer",
            Some(status.has_reachable_hook_server),
        ),
        (
            "hasRegisteredPlugin",
            status.registration.has_registered_plugin,
        ),
        (
            "hasDuplicateKanvibePlugins",
            status.registration.has_duplicate_kanvibe_plugins,
        ),
        (
            "hasRegisteredHookTarget",
            Some(status.target_registration.has_registered_hook_target),
        ),
    ]
}

impl KanvibeHookStatus {
    pub fn is_installed(&self) -> bool {
        match self {
            Self::Claude(status) => status.is_installed,
            Self::Gemini(status) => status.is_installed,
            Self::Codex(status) => status.is_installed,
            Self::OpenCode(status) => status.is_installed,
        }
    }

    /// TS 상태 객체의 `has*` 키를 객체에 놓인 순서대로. 검증 실패 항목과 메시지가 이 순서를 따른다
    pub fn checks(&self) -> Vec<HookStatusCheck> {
        match self {
            Self::Claude(status) => with_script_checks(
                &[
                    ("hasPromptHook", status.has_prompt_hook),
                    ("hasStopHook", status.has_stop_hook),
                    ("hasQuestionHook", status.has_question_hook),
                    ("hasSettingsEntry", status.has_settings_entry),
                ],
                &status.scripts,
            ),
            Self::Gemini(status) => with_script_checks(
                &[
                    ("hasPromptHook", status.has_prompt_hook),
                    ("hasStopHook", status.has_stop_hook),
                    ("hasSettingsEntry", status.has_settings_entry),
                ],
                &status.scripts,
            ),
            Self::Codex(status) => with_script_checks(
                &[
                    ("hasPromptHook", status.has_prompt_hook),
                    ("hasPermissionHook", status.has_permission_hook),
                    ("hasPreToolHook", status.has_pre_tool_hook),
                    ("hasStopHook", status.has_stop_hook),
                    ("hasHooksFile", status.has_hooks_file),
                    ("hasHookEntries", status.has_hook_entries),
                    ("hasConfigEntry", status.has_config_entry),
                ],
                &status.scripts,
            ),
            Self::OpenCode(status) => open_code_checks(status),
        }
    }

    /// 이름이 같은 확인 항목의 값
    pub fn check(&self, name: &str) -> Option<bool> {
        self.checks()
            .into_iter()
            .find(|(check_name, _)| *check_name == name)
            .and_then(|(_, value)| value)
    }

    fn scripts(&self) -> Option<&ShellHookScriptsStatus> {
        match self {
            Self::Claude(status) => Some(&status.scripts),
            Self::Gemini(status) => Some(&status.scripts),
            Self::Codex(status) => Some(&status.scripts),
            Self::OpenCode(_) => None,
        }
    }

    pub fn bound_task_id(&self) -> Option<&str> {
        match self {
            Self::OpenCode(status) => status.bound_task_id.as_deref(),
            _ => self
                .scripts()
                .and_then(|scripts| scripts.bound_task_id.as_deref()),
        }
    }

    pub fn registered_hook_target_url(&self) -> Option<&str> {
        match self {
            Self::OpenCode(status) => status
                .target_registration
                .registered_hook_target_url
                .as_deref(),
            _ => self
                .scripts()
                .and_then(|scripts| scripts.registered_hook_target_url.as_deref()),
        }
    }

    pub fn configured_hook_server_url(&self) -> Option<&str> {
        match self {
            Self::OpenCode(status) => status.configured_hook_server_url.as_deref(),
            _ => self
                .scripts()
                .and_then(|scripts| scripts.configured_hook_server_url.as_deref()),
        }
    }

    pub fn expected_hook_server_url(&self) -> Option<&str> {
        match self {
            Self::OpenCode(status) => status.expected_hook_server_url.as_deref(),
            _ => self
                .scripts()
                .and_then(|scripts| scripts.expected_hook_server_url.as_deref()),
        }
    }

    pub fn registered_plugin_urls(&self) -> Option<&[String]> {
        match self {
            Self::OpenCode(status) => status.registration.registered_plugin_urls.as_deref(),
            _ => None,
        }
    }

    pub fn plugin_registration_error(&self) -> Option<&str> {
        match self {
            Self::OpenCode(status) => status.registration.plugin_registration_error.as_deref(),
            _ => None,
        }
    }
}

/// 네 provider의 상태
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KanvibeHookStatuses {
    pub claude: KanvibeHookStatus,
    pub gemini: KanvibeHookStatus,
    pub codex: KanvibeHookStatus,
    pub open_code: KanvibeHookStatus,
}

/// AI CLI 하나의 hook 설치 규약
pub trait KanvibeHookProviderModule: Send + Sync {
    fn provider(&self) -> KanvibeHookProvider;
    /// 산출물을 만들기 전에 읽어야 하는 기존 설정 파일 경로
    fn install_input_paths(&self, repo_path: &str) -> Vec<String>;
    /// `existing_files`에서 설정 파일 내용을 꺼내 설치 산출물을 만든다
    fn build_files(
        &self,
        repo_path: &str,
        task_id: &str,
        hook_server_url: &str,
        existing_files: &HookFileSnapshot,
    ) -> Result<Vec<HookProviderFile>, HookInstallError>;
    fn install(
        &self,
        repo_path: &str,
        task_id: &str,
        hook_server_url: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError>;
    fn status(
        &self,
        repo_path: &str,
        task_id: Option<&str>,
        ssh_host: Option<&str>,
        existing_files: Option<HookFileSnapshot>,
    ) -> Result<KanvibeHookStatus, HookInstallError>;
}

fn read_existing_content<'a>(files: &'a HookFileSnapshot, file_path: Option<&String>) -> &'a str {
    file_path
        .and_then(|path| files.get(path))
        .map_or("", |file| file.content.as_str())
}

pub struct ClaudeHookProviderModule {
    pub context: HookInstallContext,
}

impl KanvibeHookProviderModule for ClaudeHookProviderModule {
    fn provider(&self) -> KanvibeHookProvider {
        KanvibeHookProvider::Claude
    }

    fn install_input_paths(&self, repo_path: &str) -> Vec<String> {
        get_claude_hook_install_input_paths(repo_path)
    }

    fn build_files(
        &self,
        repo_path: &str,
        task_id: &str,
        hook_server_url: &str,
        existing_files: &HookFileSnapshot,
    ) -> Result<Vec<HookProviderFile>, HookInstallError> {
        let settings_path = self.install_input_paths(repo_path);
        let settings_content = read_existing_content(existing_files, settings_path.first());
        build_claude_hook_files(repo_path, task_id, hook_server_url, settings_content)
    }

    fn install(
        &self,
        repo_path: &str,
        task_id: &str,
        hook_server_url: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError> {
        setup_claude_hooks(&self.context, repo_path, task_id, hook_server_url, ssh_host)
    }

    fn status(
        &self,
        repo_path: &str,
        task_id: Option<&str>,
        ssh_host: Option<&str>,
        existing_files: Option<HookFileSnapshot>,
    ) -> Result<KanvibeHookStatus, HookInstallError> {
        get_claude_hooks_status(&self.context, repo_path, task_id, ssh_host, existing_files)
            .map(KanvibeHookStatus::Claude)
    }
}

pub struct GeminiHookProviderModule {
    pub context: HookInstallContext,
}

impl KanvibeHookProviderModule for GeminiHookProviderModule {
    fn provider(&self) -> KanvibeHookProvider {
        KanvibeHookProvider::Gemini
    }

    fn install_input_paths(&self, repo_path: &str) -> Vec<String> {
        get_gemini_hook_install_input_paths(repo_path)
    }

    fn build_files(
        &self,
        repo_path: &str,
        task_id: &str,
        hook_server_url: &str,
        existing_files: &HookFileSnapshot,
    ) -> Result<Vec<HookProviderFile>, HookInstallError> {
        let settings_path = self.install_input_paths(repo_path);
        let settings_content = read_existing_content(existing_files, settings_path.first());
        build_gemini_hook_files(repo_path, task_id, hook_server_url, settings_content)
    }

    fn install(
        &self,
        repo_path: &str,
        task_id: &str,
        hook_server_url: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError> {
        setup_gemini_hooks(&self.context, repo_path, task_id, hook_server_url, ssh_host)
    }

    fn status(
        &self,
        repo_path: &str,
        task_id: Option<&str>,
        ssh_host: Option<&str>,
        existing_files: Option<HookFileSnapshot>,
    ) -> Result<KanvibeHookStatus, HookInstallError> {
        get_gemini_hooks_status(&self.context, repo_path, task_id, ssh_host, existing_files)
            .map(KanvibeHookStatus::Gemini)
    }
}

pub struct CodexHookProviderModule {
    pub context: HookInstallContext,
}

impl KanvibeHookProviderModule for CodexHookProviderModule {
    fn provider(&self) -> KanvibeHookProvider {
        KanvibeHookProvider::Codex
    }

    fn install_input_paths(&self, repo_path: &str) -> Vec<String> {
        get_codex_hook_install_input_paths(repo_path)
    }

    fn build_files(
        &self,
        repo_path: &str,
        task_id: &str,
        hook_server_url: &str,
        existing_files: &HookFileSnapshot,
    ) -> Result<Vec<HookProviderFile>, HookInstallError> {
        let input_paths = self.install_input_paths(repo_path);
        let config_content = read_existing_content(existing_files, input_paths.first());
        let hooks_content = read_existing_content(existing_files, input_paths.get(1));
        build_codex_hook_files(
            repo_path,
            task_id,
            hook_server_url,
            config_content,
            hooks_content,
        )
    }

    fn install(
        &self,
        repo_path: &str,
        task_id: &str,
        hook_server_url: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError> {
        setup_codex_hooks(&self.context, repo_path, task_id, hook_server_url, ssh_host)
    }

    fn status(
        &self,
        repo_path: &str,
        task_id: Option<&str>,
        ssh_host: Option<&str>,
        existing_files: Option<HookFileSnapshot>,
    ) -> Result<KanvibeHookStatus, HookInstallError> {
        get_codex_hooks_status(&self.context, repo_path, task_id, ssh_host, existing_files)
            .map(KanvibeHookStatus::Codex)
    }
}

pub struct OpenCodeHookProviderModule {
    pub context: HookInstallContext,
    pub registration: Arc<dyn OpenCodePluginRegistration>,
}

impl KanvibeHookProviderModule for OpenCodeHookProviderModule {
    fn provider(&self) -> KanvibeHookProvider {
        KanvibeHookProvider::OpenCode
    }

    fn install_input_paths(&self, repo_path: &str) -> Vec<String> {
        get_open_code_hook_install_input_paths(repo_path)
    }

    fn build_files(
        &self,
        repo_path: &str,
        task_id: &str,
        hook_server_url: &str,
        existing_files: &HookFileSnapshot,
    ) -> Result<Vec<HookProviderFile>, HookInstallError> {
        let tui_path = self.install_input_paths(repo_path);
        let tui_content = read_existing_content(existing_files, tui_path.first());
        build_open_code_hook_files(repo_path, task_id, hook_server_url, tui_content)
    }

    fn install(
        &self,
        repo_path: &str,
        task_id: &str,
        hook_server_url: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError> {
        setup_open_code_hooks(
            &self.context,
            self.registration.as_ref(),
            repo_path,
            task_id,
            hook_server_url,
            ssh_host,
        )
    }

    fn status(
        &self,
        repo_path: &str,
        task_id: Option<&str>,
        ssh_host: Option<&str>,
        existing_files: Option<HookFileSnapshot>,
    ) -> Result<KanvibeHookStatus, HookInstallError> {
        get_open_code_hooks_status(
            &self.context,
            self.registration.as_ref(),
            repo_path,
            task_id,
            ssh_host,
            existing_files,
        )
        .map(KanvibeHookStatus::OpenCode)
    }
}

/// 네 provider를 설치 순서대로 만든다
pub fn kanvibe_hook_provider_modules(
    context: &HookInstallContext,
    registration: Arc<dyn OpenCodePluginRegistration>,
) -> Vec<Arc<dyn KanvibeHookProviderModule>> {
    vec![
        Arc::new(ClaudeHookProviderModule {
            context: context.clone(),
        }),
        Arc::new(GeminiHookProviderModule {
            context: context.clone(),
        }),
        Arc::new(CodexHookProviderModule {
            context: context.clone(),
        }),
        Arc::new(OpenCodeHookProviderModule {
            context: context.clone(),
            registration,
        }),
    ]
}
