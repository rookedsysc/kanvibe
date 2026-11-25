//! OpenCode hook 설치(TS `openCodeHooksSetup.ts`). OpenCode는 `.opencode/plugins/`의 TypeScript 플러그인으로 hook을 등록한다.
//! message.updated(user) → progress, question.asked → pending, question.replied → progress,
//! session.idle → review, session.deleted → done 상태를 보낸다. pane 채팅 연결은 `.opencode/kanvibe-tui.mjs` TUI 플러그인이 맡는다.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use super::HookProviderFile;
use super::client_runtime::{HookInstallContext, HookInstallError};
use super::file_writer::write_hook_provider_files;
use super::open_code_plugin_source::{
    PLUGIN_EVENT_BODY, PLUGIN_HEAD, PLUGIN_STATE_BODY, TUI_PLUGIN_HEAD, TUI_PLUGIN_TAIL,
};
use super::open_code_registry::{OpenCodePluginRegistration, file_url_from_path};
use super::server_status::{
    HookServerValidation, extract_plugin_hook_server_url, validate_hook_server_configuration,
};
use super::socket_path::{HOOK_LOOPBACK_URL_PATTERN, HOOK_TRANSPORT_VERSION_MARKER};
use super::target_registration::{
    HookTargetRegistrationStatus, register_kanvibe_hook_target, verify_hook_target_registration,
};
use crate::exec::ExecGitOptions;
use crate::git_exclude::{KANVIBE_GIT_EXCLUDE_MARKER, KANVIBE_STATE_DIR_EXCLUDE_PATTERN};
use crate::host_file_access::TextFileReadResult;
use crate::kanvibe_project_state::{get_kanvibe_targets_path, join_posix_path};

pub const PLUGIN_FILE_NAME: &str = "kanvibe-plugin.ts";
pub const PLUGIN_DIR_NAME: &str = "plugins";
const CONFIG_DIR_NAME: &str = ".opencode";
const TUI_PLUGIN_FILE_NAME: &str = "kanvibe-tui.mjs";
const TUI_CONFIG_FILE_NAME: &str = "tui.json";

const OPEN_CODE_EVENT_FRAGMENTS: [&str; 9] = [
    "progress",
    "pending",
    "review",
    "done",
    "message.updated",
    "question.asked",
    "question.replied",
    "session.idle",
    "session.deleted",
];

static PLUGIN_TASK_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"const TASK_ID = ("(?:\\.|[^"\\])*");"#).expect("유효한 정규식"));

/// `JSON.stringify(문자열)`
fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("문자열은 항상 직렬화된다")
}

/// OpenCode 서버 플러그인 TypeScript 파일 내용
pub fn generate_plugin_script(kanvibe_url: &str, task_id: &str) -> String {
    [
        PLUGIN_HEAD.to_string(),
        format!("  const KANVIBE_URL = \"{kanvibe_url}\";\n"),
        format!("  // {HOOK_TRANSPORT_VERSION_MARKER}\n"),
        format!(
            "  const KANVIBE_LOOPBACK_URL = new RegExp({}, \"i\");\n",
            json_string(HOOK_LOOPBACK_URL_PATTERN)
        ),
        format!("  const TASK_ID = {};\n", json_string(task_id)),
        PLUGIN_STATE_BODY.to_string(),
        format!(
            "  const KANVIBE_STATE_DIR_EXCLUDE_PATTERN = {};\n",
            json_string(KANVIBE_STATE_DIR_EXCLUDE_PATTERN)
        ),
        format!(
            "  const KANVIBE_GIT_EXCLUDE_MARKER = {};\n",
            json_string(KANVIBE_GIT_EXCLUDE_MARKER)
        ),
        PLUGIN_EVENT_BODY.to_string(),
    ]
    .concat()
}

/// OpenCode TUI가 지금 보는 세션을 `.kanvibe/pane-chat/opencode-<pid>.json`에 남기는 TUI 플러그인
fn generate_open_code_tui_plugin(repo_path: &str) -> String {
    format!(
        "{TUI_PLUGIN_HEAD}{}{TUI_PLUGIN_TAIL}",
        json_string(repo_path)
    )
}

fn plugin_path(repo_path: &str) -> String {
    join_posix_path(&[
        repo_path,
        CONFIG_DIR_NAME,
        PLUGIN_DIR_NAME,
        PLUGIN_FILE_NAME,
    ])
}

fn tui_plugin_path(repo_path: &str) -> String {
    join_posix_path(&[repo_path, CONFIG_DIR_NAME, TUI_PLUGIN_FILE_NAME])
}

/// 설치 파일을 만들기 전에 읽어야 하는 기존 설정 파일 경로
pub fn get_open_code_hook_install_input_paths(repo_path: &str) -> Vec<String> {
    vec![join_posix_path(&[
        repo_path,
        CONFIG_DIR_NAME,
        TUI_CONFIG_FILE_NAME,
    ])]
}

/// 사용자 tui.json에 KanVibe TUI 플러그인을 맨 뒤 한 번만 넣는다. 해석할 수 없는 설정은 덮어쓰지 않고 오류로 돌려준다
fn upsert_tui_config(
    tui_content: &str,
    tui_plugin_path: &str,
) -> Result<Map<String, Value>, HookInstallError> {
    let invalid = || HookInstallError::Message("Invalid OpenCode tui.json".into());
    let config = match tui_content.trim() {
        "" => Value::Object(Map::new()),
        content => serde_json::from_str(content)
            .map_err(|error| HookInstallError::Message(error.to_string()))?,
    };
    let Value::Object(mut config) = config else {
        return Err(invalid());
    };
    let mut plugins = match config.get("plugin") {
        None => Vec::new(),
        Some(Value::Array(plugins)) => plugins.clone(),
        Some(_) => return Err(invalid()),
    };
    plugins.retain(|entry| entry.as_str() != Some(tui_plugin_path));
    plugins.push(Value::String(tui_plugin_path.to_string()));
    config.insert("plugin".into(), Value::Array(plugins));
    Ok(config)
}

/// OpenCode 플러그인 설치 산출물(TUI 플러그인, tui.json, 서버 플러그인)
pub fn build_open_code_hook_files(
    repo_path: &str,
    task_id: &str,
    kanvibe_url: &str,
    tui_content: &str,
) -> Result<Vec<HookProviderFile>, HookInstallError> {
    let tui_path = get_open_code_hook_install_input_paths(repo_path).remove(0);
    let tui_plugin_path = tui_plugin_path(repo_path);
    let config = upsert_tui_config(tui_content, &tui_plugin_path)?;
    let serialized_config =
        serde_json::to_string_pretty(&config).expect("JSON 값은 항상 직렬화된다");
    Ok(vec![
        HookProviderFile {
            file_path: tui_plugin_path,
            content: generate_open_code_tui_plugin(repo_path),
            mode: None,
        },
        HookProviderFile {
            file_path: tui_path,
            content: format!("{serialized_config}\n"),
            mode: None,
        },
        HookProviderFile {
            file_path: plugin_path(repo_path),
            content: generate_plugin_script(kanvibe_url, task_id),
            mode: None,
        },
    ])
}

/// 지정된 repo에 OpenCode 플러그인을 설치한다. 로컬 설치는 성공·실패와 상관없이 등록 조회 결과를 버린다
pub fn setup_open_code_hooks(
    context: &HookInstallContext,
    registration: &dyn OpenCodePluginRegistration,
    repo_path: &str,
    task_id: &str,
    kanvibe_url: &str,
    ssh_host: Option<&str>,
) -> Result<(), HookInstallError> {
    let ssh_host = ssh_host.filter(|host| !host.is_empty());
    let result = install_open_code_files(context, repo_path, task_id, kanvibe_url, ssh_host);
    if ssh_host.is_none() {
        registration.invalidate(repo_path);
    }
    result
}

fn install_open_code_files(
    context: &HookInstallContext,
    repo_path: &str,
    task_id: &str,
    kanvibe_url: &str,
    ssh_host: Option<&str>,
) -> Result<(), HookInstallError> {
    let tui_path = get_open_code_hook_install_input_paths(repo_path).remove(0);
    let options = ExecGitOptions {
        should_check_host_key_strictly: true,
        ..ExecGitOptions::default()
    };
    let existing_files =
        context
            .files
            .read_text_files(std::slice::from_ref(&tui_path), ssh_host, &options)?;
    let tui_content = existing_files
        .get(&tui_path)
        .map_or("", |file| file.content.as_str());
    let files = build_open_code_hook_files(repo_path, task_id, kanvibe_url, tui_content)?;
    write_hook_provider_files(context, &files, ssh_host)?;
    register_kanvibe_hook_target(context, repo_path, task_id, kanvibe_url, ssh_host)
}

/// OpenCode CLI 등록 조회 결과. 원격은 조회할 수 없어 등록된 것으로 본다. 조회 실패는 `error`만 채운다
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OpenCodePluginRegistrationStatus {
    pub has_registered_plugin: Option<bool>,
    pub has_duplicate_kanvibe_plugins: Option<bool>,
    pub registered_plugin_urls: Option<Vec<String>>,
    pub plugin_registration_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeHooksStatus {
    pub is_installed: bool,
    pub has_plugin: bool,
    pub has_task_id_binding: bool,
    pub has_status_endpoint: bool,
    pub has_event_mappings: bool,
    pub has_status_json_persistence: bool,
    pub has_bounded_notify_timeout: bool,
    pub has_target_fanout: bool,
    pub has_parallel_target_fanout: bool,
    pub has_main_session_guard: bool,
    pub has_duplicate_progress_guard: bool,
    pub has_pane_chat_binding: bool,
    pub has_expected_hook_server_url: bool,
    pub has_reachable_hook_server: bool,
    pub bound_task_id: Option<String>,
    pub target_path: String,
    pub plugin_path: String,
    pub configured_hook_server_url: Option<String>,
    pub expected_hook_server_url: Option<String>,
    pub registration: OpenCodePluginRegistrationStatus,
    pub target_registration: HookTargetRegistrationStatus,
}

/// 플러그인 파일 내용만으로 판정하는 설치 지표
struct PluginContentChecks {
    has_plugin: bool,
    has_task_id_binding: bool,
    has_status_endpoint: bool,
    has_event_mappings: bool,
    has_status_json_persistence: bool,
    has_bounded_notify_timeout: bool,
    has_target_fanout: bool,
    has_parallel_target_fanout: bool,
    has_main_session_guard: bool,
    has_duplicate_progress_guard: bool,
    bound_task_id: Option<String>,
}

impl PluginContentChecks {
    fn inspect(content: &str) -> Self {
        let bound_task_id = extract_plugin_task_id(content);
        Self {
            has_plugin: content.contains("KanvibePlugin") && content.contains("/api/hooks/status"),
            has_task_id_binding: bound_task_id.is_some() && content.contains("taskId: TASK_ID"),
            has_status_endpoint: content.contains("/api/hooks/status"),
            has_event_mappings: OPEN_CODE_EVENT_FRAGMENTS
                .iter()
                .all(|fragment| content.contains(fragment)),
            has_status_json_persistence: has_status_json_persistence(content),
            has_bounded_notify_timeout: content.contains("KANVIBE_NOTIFY_TIMEOUT_MS")
                && content.contains("AbortSignal.timeout(KANVIBE_NOTIFY_TIMEOUT_MS)"),
            has_target_fanout: has_target_fanout(content),
            has_parallel_target_fanout: content
                .contains("await Promise.all(readKanvibeTargets().map("),
            has_main_session_guard: content.contains("isMainSession(message)")
                && content.contains("isMainSession(event.properties)"),
            has_duplicate_progress_guard: content.contains("lastUserMessageBySession")
                && content.contains("buildMessageSignature")
                && content.contains("dedupeMessage: true"),
            bound_task_id,
        }
    }

    fn are_all_present(&self) -> bool {
        self.has_plugin
            && self.has_task_id_binding
            && self.has_status_endpoint
            && self.has_event_mappings
            && self.has_status_json_persistence
            && self.has_bounded_notify_timeout
            && self.has_target_fanout
            && self.has_parallel_target_fanout
            && self.has_main_session_guard
            && self.has_duplicate_progress_guard
    }
}

fn has_status_json_persistence(content: &str) -> bool {
    [
        "status.json",
        "KANVIBE_STATE_DIR_EXCLUDE_PATTERN",
        "--git-common-dir",
        "includes(KANVIBE_STATE_DIR_EXCLUDE_PATTERN)",
        "schemaVersion: 1",
        "updatedAt: new Date().toISOString()",
    ]
    .iter()
    .all(|fragment| content.contains(fragment))
}

fn has_target_fanout(content: &str) -> bool {
    [
        HOOK_TRANSPORT_VERSION_MARKER,
        "targets.json",
        "KANVIBE_TARGETS_FILE",
        "readKanvibeTargets",
        "fanoutKanvibeStatus",
        "postKanvibeStatus",
        "findKanvibeHookSocket(target.clientId, target.remoteHostNamespace)",
        "taskId: target.taskId",
        "/api/hooks/status",
    ]
    .iter()
    .all(|fragment| content.contains(fragment))
}

fn extract_plugin_task_id(content: &str) -> Option<String> {
    let literal = PLUGIN_TASK_ID_PATTERN.captures(content)?.get(1)?;
    serde_json::from_str::<String>(literal.as_str()).ok()
}

/// 원격 repo는 OpenCode 설정 레지스트리를 조회할 수 없으므로 등록 여부를 검증하지 않는다
fn resolve_plugin_registration(
    registration: &dyn OpenCodePluginRegistration,
    repo_path: &str,
    plugin_path: &str,
    ssh_host: Option<&str>,
) -> OpenCodePluginRegistrationStatus {
    if ssh_host.is_some() {
        return OpenCodePluginRegistrationStatus {
            has_registered_plugin: Some(true),
            has_duplicate_kanvibe_plugins: Some(false),
            registered_plugin_urls: Some(Vec::new()),
            plugin_registration_error: None,
        };
    }
    let expected_plugin_url = file_url_from_path(plugin_path);
    match registration.registered_kanvibe_plugin_urls(repo_path) {
        Ok(urls) => OpenCodePluginRegistrationStatus {
            has_registered_plugin: Some(urls.contains(&expected_plugin_url)),
            has_duplicate_kanvibe_plugins: Some(urls.len() > 1),
            registered_plugin_urls: Some(urls),
            plugin_registration_error: None,
        },
        Err(error) => OpenCodePluginRegistrationStatus {
            plugin_registration_error: Some(error),
            ..OpenCodePluginRegistrationStatus::default()
        },
    }
}

/// TUI 플러그인 파일이 있고 tui.json이 그 경로를 plugin으로 등록했는지. 해석할 수 없는 설정은 다시 설치할 대상이다
fn has_pane_chat_binding(
    files: &BTreeMap<String, TextFileReadResult>,
    tui_path: &str,
    tui_plugin_path: &str,
) -> bool {
    let tui_content = files
        .get(tui_path)
        .map_or("{}", |file| file.content.as_str());
    let Ok(config) = serde_json::from_str::<Value>(tui_content) else {
        return false;
    };
    let is_plugin_listed = config
        .get("plugin")
        .and_then(Value::as_array)
        .is_some_and(|plugins| {
            plugins
                .iter()
                .any(|entry| entry.as_str() == Some(tui_plugin_path))
        });
    let has_observer = files
        .get(tui_plugin_path)
        .is_some_and(|observer| observer.exists && observer.content.contains("kanvibe-pane-chat"));
    has_observer && is_plugin_listed
}

/// 상태 판정에 필요한 파일 가운데 넘겨받지 못한 것만 읽는다
fn read_missing_status_files(
    context: &HookInstallContext,
    paths: &[String],
    ssh_host: Option<&str>,
    existing_files: Option<BTreeMap<String, TextFileReadResult>>,
) -> Result<BTreeMap<String, TextFileReadResult>, HookInstallError> {
    let mut files = existing_files.unwrap_or_default();
    let missing_paths: Vec<String> = paths
        .iter()
        .filter(|path| !files.contains_key(*path))
        .cloned()
        .collect();
    if !missing_paths.is_empty() {
        let options = ExecGitOptions {
            should_check_host_key_strictly: true,
            ..ExecGitOptions::default()
        };
        files.extend(
            context
                .files
                .read_text_files(&missing_paths, ssh_host, &options)?,
        );
    }
    Ok(files)
}

/// 지정된 repo의 OpenCode 플러그인 설치 상태
pub fn get_open_code_hooks_status(
    context: &HookInstallContext,
    registration: &dyn OpenCodePluginRegistration,
    repo_path: &str,
    task_id: Option<&str>,
    ssh_host: Option<&str>,
    existing_files: Option<BTreeMap<String, TextFileReadResult>>,
) -> Result<OpenCodeHooksStatus, HookInstallError> {
    let ssh_host = ssh_host.filter(|host| !host.is_empty());
    let task_id = task_id.filter(|task_id| !task_id.is_empty());
    let plugin_path = plugin_path(repo_path);
    let targets_path = get_kanvibe_targets_path(repo_path);
    let tui_path = get_open_code_hook_install_input_paths(repo_path).remove(0);
    let tui_plugin_path = tui_plugin_path(repo_path);
    let paths = [
        plugin_path.clone(),
        targets_path.clone(),
        tui_path.clone(),
        tui_plugin_path.clone(),
    ];
    let files = read_missing_status_files(context, &paths, ssh_host, existing_files)?;
    let plugin_file = files.get(&plugin_path);
    let plugin_content = plugin_file.map_or("", |file| file.content.as_str());
    let checks = PluginContentChecks::inspect(plugin_content);
    let registration_status =
        resolve_plugin_registration(registration, repo_path, &plugin_path, ssh_host);
    let targets_content = files
        .get(&targets_path)
        .map_or("", |file| file.content.as_str());
    let (validation, target_registration) =
        validate_hook_target(context, plugin_content, targets_content, task_id, ssh_host);
    let has_pane_chat_binding = has_pane_chat_binding(&files, &tui_path, &tui_plugin_path);
    let is_installed = plugin_file.is_some_and(|file| file.exists)
        && checks.are_all_present()
        && target_registration.has_registered_hook_target
        && has_pane_chat_binding;
    Ok(build_status(StatusParts {
        is_installed,
        checks,
        has_pane_chat_binding,
        validation,
        target_path: repo_path.to_string(),
        plugin_path,
        registration: registration_status,
        target_registration,
    }))
}

/// 플러그인이 가리키는 hook 서버 주소와 targets.json 등록을 함께 검사한다
fn validate_hook_target(
    context: &HookInstallContext,
    plugin_content: &str,
    targets_content: &str,
    task_id: Option<&str>,
    ssh_host: Option<&str>,
) -> (HookServerValidation, HookTargetRegistrationStatus) {
    let validation = validate_hook_server_configuration(
        context,
        &[extract_plugin_hook_server_url(plugin_content)],
        task_id.is_some(),
        ssh_host,
    );
    let target_registration = verify_hook_target_registration(
        targets_content,
        task_id,
        validation.expected_hook_server_url.as_deref(),
        ssh_host,
    );
    (validation, target_registration)
}

struct StatusParts {
    is_installed: bool,
    checks: PluginContentChecks,
    has_pane_chat_binding: bool,
    validation: HookServerValidation,
    target_path: String,
    plugin_path: String,
    registration: OpenCodePluginRegistrationStatus,
    target_registration: HookTargetRegistrationStatus,
}

fn build_status(parts: StatusParts) -> OpenCodeHooksStatus {
    let checks = parts.checks;
    OpenCodeHooksStatus {
        is_installed: parts.is_installed,
        has_plugin: checks.has_plugin,
        has_task_id_binding: checks.has_task_id_binding,
        has_status_endpoint: checks.has_status_endpoint,
        has_event_mappings: checks.has_event_mappings,
        has_status_json_persistence: checks.has_status_json_persistence,
        has_bounded_notify_timeout: checks.has_bounded_notify_timeout,
        has_target_fanout: checks.has_target_fanout,
        has_parallel_target_fanout: checks.has_parallel_target_fanout,
        has_main_session_guard: checks.has_main_session_guard,
        has_duplicate_progress_guard: checks.has_duplicate_progress_guard,
        has_pane_chat_binding: parts.has_pane_chat_binding,
        has_expected_hook_server_url: parts.validation.has_expected_hook_server_url,
        has_reachable_hook_server: parts.validation.has_reachable_hook_server,
        bound_task_id: checks.bound_task_id,
        target_path: parts.target_path,
        plugin_path: parts.plugin_path,
        configured_hook_server_url: parts.validation.configured_hook_server_url,
        expected_hook_server_url: parts.validation.expected_hook_server_url,
        registration: parts.registration,
        target_registration: parts.target_registration,
    }
}
