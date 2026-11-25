//! bash hook 스크립트를 쓰는 provider(Claude Code, Codex, Gemini CLI)가 공유하는 생성·상태 판정(TS `shellHookProvider.ts`).
//! provider 모듈은 스크립트 정의와 설정 파일 병합만 갖고, 스크립트 본문과 설치 상태 지표는 여기서 만든다.

use std::collections::BTreeMap;

use super::HookProviderFile;
use super::client_runtime::{HookInstallContext, HookInstallError};
use super::server_status::{extract_shell_hook_server_url, validate_hook_server_configuration};
use super::shell_script::{
    ShellHookAgentProvider, ShellHookStatus, build_shell_kanvibe_status_updater,
    build_shell_task_id_resolver, get_shell_task_id_binding_status,
    has_shell_kanvibe_bounded_notify_timeout, has_shell_kanvibe_parallel_target_fanout,
    has_shell_kanvibe_status_json_persistence, has_shell_kanvibe_target_fanout,
};
use super::target_registration::verify_hook_target_registration;
use crate::exec::ExecGitOptions;
use crate::host_file_access::TextFileReadResult;
use crate::kanvibe_project_state::{get_kanvibe_targets_path, join_posix_path};

/// hook 스크립트는 실행 권한으로 기록한다
pub const HOOK_SCRIPT_FILE_MODE: u32 = 0o755;

/// 하나의 CLI 이벤트에 대응하는 KanVibe hook shell script 정의
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellHookScriptDefinition {
    pub file_name: &'static str,
    pub event_label: &'static str,
    pub description: &'static str,
    pub status: ShellHookStatus,
    /// stdout에 JSON만 허용하는 런타임(Gemini CLI)을 위해 종료 직전 출력할 내용
    pub trailing_output: Option<&'static str>,
    /// 스크립트 상단 주석에 덧붙일 런타임 제약 설명
    pub runtime_note: Option<&'static str>,
}

/// 모든 shell hook provider가 공유하는 설치 상태 지표
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellHookScriptsStatus {
    pub has_task_id_binding: bool,
    pub has_status_mappings: bool,
    pub has_status_json_persistence: bool,
    pub has_target_fanout: bool,
    pub has_bounded_notify_timeout: bool,
    pub has_parallel_target_fanout: bool,
    pub has_expected_hook_server_url: bool,
    pub has_reachable_hook_server: bool,
    pub bound_task_id: Option<String>,
    pub configured_hook_server_url: Option<String>,
    pub expected_hook_server_url: Option<String>,
    pub has_registered_hook_target: bool,
    pub registered_hook_target_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellHookProviderState {
    /// 정의 순서대로의 스크립트 존재 여부와 내용
    pub script_files: Vec<TextFileReadResult>,
    pub has_all_scripts: bool,
    /// 함께 읽은 파일 전체(스크립트, 추가 파일, targets.json)
    pub files: BTreeMap<String, TextFileReadResult>,
    pub status: ShellHookScriptsStatus,
}

/// provider 상태를 읽을 때 넘기는 값
pub struct ShellHookProviderStateRequest<'a> {
    pub repo_path: &'a str,
    pub hooks_dir: &'a str,
    pub definitions: &'a [ShellHookScriptDefinition],
    pub extra_file_paths: &'a [String],
    pub task_id: Option<&'a str>,
    pub ssh_host: Option<&'a str>,
    /// 이미 한 번에 읽어 둔 파일. 없으면 여기서 읽는다
    pub files: Option<BTreeMap<String, TextFileReadResult>>,
}

/// 스크립트가 hook stdin을 대화 메타데이터로 보낼 agent. TS처럼 agent 이름으로 고른다
fn agent_provider(agent_label: &str) -> Option<ShellHookAgentProvider> {
    match agent_label {
        "Claude Code" => Some(ShellHookAgentProvider::Claude),
        "Codex" => Some(ShellHookAgentProvider::Codex),
        "Gemini CLI" => Some(ShellHookAgentProvider::Gemini),
        _ => None,
    }
}

/// hook shell script 한 개의 내용
pub fn generate_shell_hook_script(
    agent_label: &str,
    definition: &ShellHookScriptDefinition,
    kanvibe_url: &str,
    task_id: &str,
) -> String {
    let mut header_comments = vec![
        format!("# KanVibe {agent_label} Hook: {}", definition.event_label),
        format!("# {}", definition.description),
    ];
    header_comments.extend(definition.runtime_note.map(|note| format!("# {note}")));
    let mut trailing_lines: Vec<&str> = definition.trailing_output.into_iter().collect();
    trailing_lines.push("exit 0");
    format!(
        "#!/bin/bash\n\n{}\n\nKANVIBE_URL=\"{kanvibe_url}\"\n{}\n{}\n\n{}\n",
        header_comments.join("\n"),
        build_shell_task_id_resolver(task_id),
        build_shell_kanvibe_status_updater(definition.status, agent_provider(agent_label)),
        trailing_lines.join("\n"),
    )
}

/// hook script 파일 목록. 로컬 설치와 원격 설치가 같은 내용을 공유한다
pub fn build_shell_hook_script_files(
    hooks_dir: &str,
    agent_label: &str,
    definitions: &[ShellHookScriptDefinition],
    kanvibe_url: &str,
    task_id: &str,
) -> Vec<HookProviderFile> {
    definitions
        .iter()
        .map(|definition| HookProviderFile {
            file_path: join_posix_path(&[hooks_dir, definition.file_name]),
            content: generate_shell_hook_script(agent_label, definition, kanvibe_url, task_id),
            mode: Some(HOOK_SCRIPT_FILE_MODE),
        })
        .collect()
}

/// hook script가 자신의 상태를 실제로 payload에 담고 있는지 확인할 문자열
fn build_status_payload_fragment(status: ShellHookStatus) -> String {
    format!("\\\"status\\\": \\\"{}\\\"", status.as_str())
}

fn empty_file() -> TextFileReadResult {
    TextFileReadResult {
        exists: false,
        content: String::new(),
    }
}

fn read_provider_files(
    context: &HookInstallContext,
    request: &mut ShellHookProviderStateRequest<'_>,
    script_paths: &[String],
    targets_path: &str,
) -> Result<BTreeMap<String, TextFileReadResult>, HookInstallError> {
    if let Some(files) = request.files.take() {
        return Ok(files);
    }
    let mut paths = script_paths.to_vec();
    paths.extend(request.extra_file_paths.iter().cloned());
    paths.push(targets_path.to_string());
    let options = ExecGitOptions {
        should_check_host_key_strictly: true,
        ..ExecGitOptions::default()
    };
    Ok(context
        .files
        .read_text_files(&paths, request.ssh_host, &options)?)
}

/// hook script와 provider별 설정 파일, `.kanvibe/targets.json`을 한 번에 읽어 설치 상태를 계산한다
pub fn read_shell_hook_provider_state(
    context: &HookInstallContext,
    mut request: ShellHookProviderStateRequest<'_>,
) -> Result<ShellHookProviderState, HookInstallError> {
    request.ssh_host = request.ssh_host.filter(|host| !host.is_empty());
    let script_paths: Vec<String> = request
        .definitions
        .iter()
        .map(|definition| join_posix_path(&[request.hooks_dir, definition.file_name]))
        .collect();
    let targets_path = get_kanvibe_targets_path(request.repo_path);
    let files = read_provider_files(context, &mut request, &script_paths, &targets_path)?;
    let script_files: Vec<TextFileReadResult> = script_paths
        .iter()
        .map(|script_path| files.get(script_path).cloned().unwrap_or_else(empty_file))
        .collect();
    let script_contents: Vec<&str> = script_files
        .iter()
        .map(|file| file.content.as_str())
        .collect();
    let targets_content = files
        .get(&targets_path)
        .map_or("", |file| file.content.as_str());
    let status =
        build_shell_hook_scripts_status(context, &request, &script_contents, targets_content);
    Ok(ShellHookProviderState {
        has_all_scripts: script_files.iter().all(|file| file.exists),
        script_files,
        files,
        status,
    })
}

/// 스크립트 내용과 targets.json으로 공통 설치 상태 지표를 계산한다
fn build_shell_hook_scripts_status(
    context: &HookInstallContext,
    request: &ShellHookProviderStateRequest<'_>,
    script_contents: &[&str],
    targets_content: &str,
) -> ShellHookScriptsStatus {
    let configured_urls: Vec<Option<String>> = script_contents
        .iter()
        .map(|content| extract_shell_hook_server_url(content))
        .collect();
    let should_validate = request.task_id.is_some_and(|task_id| !task_id.is_empty());
    let validation = validate_hook_server_configuration(
        context,
        &configured_urls,
        should_validate,
        request.ssh_host,
    );
    let binding = get_shell_task_id_binding_status(script_contents);
    let registration = verify_hook_target_registration(
        targets_content,
        request.task_id,
        validation.expected_hook_server_url.as_deref(),
        request.ssh_host,
    );
    let has_status_mappings =
        request
            .definitions
            .iter()
            .zip(script_contents)
            .all(|(definition, content)| {
                content.contains(&build_status_payload_fragment(definition.status))
            });
    let all_scripts =
        |check: fn(&str) -> bool| script_contents.iter().all(|content| check(content));
    ShellHookScriptsStatus {
        has_task_id_binding: binding.has_task_id_binding,
        has_status_mappings,
        has_status_json_persistence: all_scripts(has_shell_kanvibe_status_json_persistence),
        has_target_fanout: all_scripts(has_shell_kanvibe_target_fanout),
        has_bounded_notify_timeout: all_scripts(has_shell_kanvibe_bounded_notify_timeout),
        has_parallel_target_fanout: all_scripts(has_shell_kanvibe_parallel_target_fanout),
        has_expected_hook_server_url: validation.has_expected_hook_server_url,
        has_reachable_hook_server: validation.has_reachable_hook_server,
        bound_task_id: binding.bound_task_id,
        configured_hook_server_url: validation.configured_hook_server_url,
        expected_hook_server_url: validation.expected_hook_server_url,
        has_registered_hook_target: registration.has_registered_hook_target,
        registered_hook_target_url: registration.registered_hook_target_url,
    }
}

/// hook script와 설정이 모두 정상인지 판정한다. provider별 추가 조건은 호출부가 결합한다.
/// 알림 대상은 script에 박힌 URL이 아니라 targets.json 등록 여부로 판정하므로,
/// 다른 client가 script의 fallback URL·TASK_ID를 덮어써도 설치 상태가 깨지지 않는다.
pub fn is_shell_hook_provider_installed(
    state: &ShellHookProviderState,
    provider_checks: &[bool],
) -> bool {
    let status = &state.status;
    state.has_all_scripts
        && provider_checks.iter().all(|check| *check)
        && status.has_task_id_binding
        && status.has_registered_hook_target
        && status.has_status_mappings
        && status.has_status_json_persistence
        && status.has_target_fanout
        && status.has_bounded_notify_timeout
        && status.has_parallel_target_fanout
}
