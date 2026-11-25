//! Codex CLI hook 설치(TS `codexHooksSetup.ts`).
//! Codex 최신 hooks 설정은 `.codex/config.toml`의 `[features] hooks = true`와 `.codex/hooks.json` 조합을 쓴다.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::HookProviderFile;
use super::client_runtime::{HookInstallContext, HookInstallError};
use super::file_writer::write_hook_provider_files;
use super::json_settings::{
    build_command_hook_entry, build_matcher_command_hook_entry, ensure_json_hook_buckets,
    has_command_hook_entry, has_matcher_command_hook_entry, parse_json_hook_settings,
    serialize_json_hook_settings, upsert_json_hook_entries,
};
use super::provider::{
    ShellHookProviderStateRequest, ShellHookScriptDefinition, ShellHookScriptsStatus,
    build_shell_hook_script_files, generate_shell_hook_script, is_shell_hook_provider_installed,
    read_shell_hook_provider_state,
};
use super::shell_script::ShellHookStatus;
use super::target_registration::register_kanvibe_hook_target;
use crate::exec::ExecGitOptions;
use crate::host_file_access::TextFileReadResult;
use crate::kanvibe_project_state::join_posix_path;

const AGENT_LABEL: &str = "Codex";
const CONFIG_DIR_NAME: &str = ".codex";
const HOOK_TIMEOUT_SECONDS: u32 = 10;
const BASH_MATCHER: &str = "Bash";
const CODEX_HOOKS_FEATURE_FLAG: &str = "hooks";

pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const HOOKS_FILE_NAME: &str = "hooks.json";
pub const PROMPT_HOOK_SCRIPT_NAME: &str = "kanvibe-prompt-hook.sh";
pub const PERMISSION_HOOK_SCRIPT_NAME: &str = "kanvibe-permission-hook.sh";
pub const PRE_TOOL_HOOK_SCRIPT_NAME: &str = "kanvibe-pre-tool-hook.sh";
pub const STOP_HOOK_SCRIPT_NAME: &str = "kanvibe-stop-hook.sh";

const CODEX_HOOK_SCRIPTS: [ShellHookScriptDefinition; 4] = [
    ShellHookScriptDefinition {
        file_name: PROMPT_HOOK_SCRIPT_NAME,
        event_label: "UserPromptSubmit",
        description: "사용자가 prompt를 입력하면 현재 task를 PROGRESS로 변경한다.",
        status: ShellHookStatus::Progress,
        trailing_output: None,
        runtime_note: None,
    },
    ShellHookScriptDefinition {
        file_name: PERMISSION_HOOK_SCRIPT_NAME,
        event_label: "PermissionRequest(Bash)",
        description: "Codex가 Bash 실행 승인을 요청하면 현재 task를 PENDING으로 변경한다.",
        status: ShellHookStatus::Pending,
        trailing_output: None,
        runtime_note: None,
    },
    ShellHookScriptDefinition {
        file_name: PRE_TOOL_HOOK_SCRIPT_NAME,
        event_label: "PreToolUse(Bash)",
        description: "Codex가 Bash 실행을 재개하면 현재 task를 PROGRESS로 변경한다.",
        status: ShellHookStatus::Progress,
        trailing_output: None,
        runtime_note: None,
    },
    ShellHookScriptDefinition {
        file_name: STOP_HOOK_SCRIPT_NAME,
        event_label: "Stop",
        description: "Codex 응답이 완료되면 현재 task를 REVIEW로 변경한다.",
        status: ShellHookStatus::Review,
        trailing_output: None,
        runtime_note: None,
    },
];

/// JS 정규식의 `.`은 줄 끝 문자(`\r`, U+2028, U+2029)를 받지 않으므로 같은 문자 집합을 쓴다
const ANY_EXCEPT_LINE_TERMINATOR: &str = "[^\\n\\r\\u{2028}\\u{2029}]";

static SECTION_HEADER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^\s*(?:\[[^\[\]]+\]|\[\[[^\[\]]+\]\])\s*(?:#{ANY_EXCEPT_LINE_TERMINATOR}*)?$"
    ))
    .expect("유효한 정규식")
});
static FEATURES_HEADER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^\s*\[features\]\s*(?:#{ANY_EXCEPT_LINE_TERMINATOR}*)?$"
    ))
    .expect("유효한 정규식")
});
static LEGACY_KANVIBE_NOTIFY_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^notify\s*=\s*\["\.codex/hooks/kanvibe-notify-hook\.sh"\]\s*\n?"#)
        .expect("유효한 정규식")
});
static CODEX_HOOKS_FLAG_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*codex_hooks\s*=").expect("유효한 정규식"));
static STALE_HOOKS_FLAG_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:hooks|codex_hook)\s*=").expect("유효한 정규식"));
static ENABLED_HOOKS_FLAG_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*hooks\s*=\s*true\s*$").expect("유효한 정규식"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexHooksStatus {
    pub is_installed: bool,
    pub has_prompt_hook: bool,
    pub has_permission_hook: bool,
    pub has_pre_tool_hook: bool,
    pub has_stop_hook: bool,
    pub has_hooks_file: bool,
    pub has_hook_entries: bool,
    pub has_config_entry: bool,
    pub scripts: ShellHookScriptsStatus,
}

fn build_codex_hook_command(script_name: &str) -> String {
    format!("bash \"$(git rev-parse --show-toplevel)/{CONFIG_DIR_NAME}/hooks/{script_name}\"")
}

/// UserPromptSubmit hook bash 스크립트
pub fn generate_prompt_hook_script(kanvibe_url: &str, task_id: &str) -> String {
    generate_shell_hook_script(AGENT_LABEL, &CODEX_HOOK_SCRIPTS[0], kanvibe_url, task_id)
}

/// PermissionRequest(Bash) hook bash 스크립트
pub fn generate_permission_hook_script(kanvibe_url: &str, task_id: &str) -> String {
    generate_shell_hook_script(AGENT_LABEL, &CODEX_HOOK_SCRIPTS[1], kanvibe_url, task_id)
}

/// PreToolUse(Bash) hook bash 스크립트
pub fn generate_pre_tool_hook_script(kanvibe_url: &str, task_id: &str) -> String {
    generate_shell_hook_script(AGENT_LABEL, &CODEX_HOOK_SCRIPTS[2], kanvibe_url, task_id)
}

/// Stop hook bash 스크립트
pub fn generate_stop_hook_script(kanvibe_url: &str, task_id: &str) -> String {
    generate_shell_hook_script(AGENT_LABEL, &CODEX_HOOK_SCRIPTS[3], kanvibe_url, task_id)
}

fn hooks_dir(repo_path: &str) -> String {
    join_posix_path(&[repo_path, CONFIG_DIR_NAME, "hooks"])
}

fn config_path(repo_path: &str) -> String {
    join_posix_path(&[repo_path, CONFIG_DIR_NAME, CONFIG_FILE_NAME])
}

fn hooks_path(repo_path: &str) -> String {
    join_posix_path(&[repo_path, CONFIG_DIR_NAME, HOOKS_FILE_NAME])
}

/// 설치 파일을 만들기 전에 읽어야 하는 기존 설정 파일 경로
pub fn get_codex_hook_install_input_paths(repo_path: &str) -> Vec<String> {
    vec![config_path(repo_path), hooks_path(repo_path)]
}

/// Codex CLI hooks 설치 산출물. 기존 config.toml·hooks.json이 있으면 KanVibe 관련 항목만 갱신하고 나머지는 보존한다
pub fn build_codex_hook_files(
    repo_path: &str,
    task_id: &str,
    kanvibe_url: &str,
    config_content: &str,
    hooks_content: &str,
) -> Vec<HookProviderFile> {
    let mut files = build_shell_hook_script_files(
        &hooks_dir(repo_path),
        AGENT_LABEL,
        &CODEX_HOOK_SCRIPTS,
        kanvibe_url,
        task_id,
    );
    files.push(HookProviderFile {
        file_path: config_path(repo_path),
        content: upsert_codex_config_toml(config_content),
        mode: None,
    });
    files.push(HookProviderFile {
        file_path: hooks_path(repo_path),
        content: upsert_codex_hooks_json(hooks_content),
        mode: None,
    });
    files
}

pub fn upsert_codex_hooks_json(hooks_content: &str) -> String {
    let mut settings = parse_json_hook_settings(hooks_content);
    let hooks = ensure_json_hook_buckets(&mut settings);
    let upserts = [
        (
            "UserPromptSubmit",
            PROMPT_HOOK_SCRIPT_NAME,
            build_command_hook_entry(
                &build_codex_hook_command(PROMPT_HOOK_SCRIPT_NAME),
                HOOK_TIMEOUT_SECONDS,
            ),
        ),
        (
            "PermissionRequest",
            PERMISSION_HOOK_SCRIPT_NAME,
            build_matcher_command_hook_entry(
                BASH_MATCHER,
                &build_codex_hook_command(PERMISSION_HOOK_SCRIPT_NAME),
                HOOK_TIMEOUT_SECONDS,
            ),
        ),
        (
            "PreToolUse",
            PRE_TOOL_HOOK_SCRIPT_NAME,
            build_matcher_command_hook_entry(
                BASH_MATCHER,
                &build_codex_hook_command(PRE_TOOL_HOOK_SCRIPT_NAME),
                HOOK_TIMEOUT_SECONDS,
            ),
        ),
        (
            "Stop",
            STOP_HOOK_SCRIPT_NAME,
            build_command_hook_entry(
                &build_codex_hook_command(STOP_HOOK_SCRIPT_NAME),
                HOOK_TIMEOUT_SECONDS,
            ),
        ),
    ];
    for (event, script_name, entry) in upserts {
        let entries = upsert_json_hook_entries(hooks.get(event), script_name, entry);
        hooks.insert(event.to_string(), Value::Array(entries));
    }
    serialize_json_hook_settings(&settings)
}

/// 지정된 repo에 Codex CLI hooks를 설정한다. 로컬과 원격 repo 모두 같은 산출물을 기록한다
pub fn setup_codex_hooks(
    context: &HookInstallContext,
    repo_path: &str,
    task_id: &str,
    kanvibe_url: &str,
    ssh_host: Option<&str>,
) -> Result<(), HookInstallError> {
    let config_path = config_path(repo_path);
    let hooks_path = hooks_path(repo_path);
    let options = ExecGitOptions {
        should_check_host_key_strictly: true,
        ..ExecGitOptions::default()
    };
    let input_paths = [config_path.clone(), hooks_path.clone()];
    let existing_files = context
        .files
        .read_text_files(&input_paths, ssh_host, &options)?;
    let read_content = |path: &str| {
        existing_files
            .get(path)
            .map_or("", |file| file.content.as_str())
    };
    let files = build_codex_hook_files(
        repo_path,
        task_id,
        kanvibe_url,
        read_content(&config_path),
        read_content(&hooks_path),
    );
    write_hook_provider_files(context, &files, ssh_host)?;
    register_kanvibe_hook_target(context, repo_path, task_id, kanvibe_url, ssh_host)
}

/// 지정된 repo의 Codex CLI hooks 설치 상태
pub fn get_codex_hooks_status(
    context: &HookInstallContext,
    repo_path: &str,
    task_id: Option<&str>,
    ssh_host: Option<&str>,
    existing_files: Option<BTreeMap<String, TextFileReadResult>>,
) -> Result<CodexHooksStatus, HookInstallError> {
    let config_path = config_path(repo_path);
    let hooks_path = hooks_path(repo_path);
    let extra_file_paths = [config_path.clone(), hooks_path.clone()];
    let state = read_shell_hook_provider_state(
        context,
        ShellHookProviderStateRequest {
            repo_path,
            hooks_dir: &hooks_dir(repo_path),
            definitions: &CODEX_HOOK_SCRIPTS,
            extra_file_paths: &extra_file_paths,
            task_id,
            ssh_host,
            files: existing_files,
        },
    )?;
    let hooks_file = state.files.get(&hooks_path);
    let has_hooks_file = hooks_file.is_some_and(|file| file.exists);
    let has_hook_entries =
        has_codex_hook_entries(hooks_file.map_or("", |file| file.content.as_str()));
    let config_content = state
        .files
        .get(&config_path)
        .map_or("", |file| file.content.as_str());
    let has_config_entry = has_codex_feature_flag(config_content);
    Ok(CodexHooksStatus {
        is_installed: is_shell_hook_provider_installed(
            &state,
            &[has_hooks_file, has_hook_entries, has_config_entry],
        ),
        has_prompt_hook: state.script_files[0].exists,
        has_permission_hook: state.script_files[1].exists,
        has_pre_tool_hook: state.script_files[2].exists,
        has_stop_hook: state.script_files[3].exists,
        has_hooks_file,
        has_hook_entries,
        has_config_entry,
        scripts: state.status,
    })
}

fn has_codex_hook_entries(hooks_content: &str) -> bool {
    let settings = parse_json_hook_settings(hooks_content);
    // hooks가 없거나 객체가 아니면 아래 조회가 모두 Null이 되어 거짓이다
    let hooks = settings.get("hooks").unwrap_or(&Value::Null);
    has_command_hook_entry(
        &hooks["UserPromptSubmit"],
        &build_codex_hook_command(PROMPT_HOOK_SCRIPT_NAME),
    ) && has_matcher_command_hook_entry(
        &hooks["PermissionRequest"],
        BASH_MATCHER,
        &build_codex_hook_command(PERMISSION_HOOK_SCRIPT_NAME),
    ) && has_matcher_command_hook_entry(
        &hooks["PreToolUse"],
        BASH_MATCHER,
        &build_codex_hook_command(PRE_TOOL_HOOK_SCRIPT_NAME),
    ) && has_command_hook_entry(
        &hooks["Stop"],
        &build_codex_hook_command(STOP_HOOK_SCRIPT_NAME),
    )
}

/// `[features]` 머리줄의 위치와 다음 섹션 머리줄(없으면 끝)의 위치
fn find_features_section(lines: &[&str]) -> Option<(usize, usize)> {
    let start = lines
        .iter()
        .position(|line| FEATURES_HEADER_PATTERN.is_match(line))?;
    let end = lines[start + 1..]
        .iter()
        .position(|line| SECTION_HEADER_PATTERN.is_match(line))
        .map_or(lines.len(), |offset| start + 1 + offset);
    Some((start, end))
}

fn split_lines(content: &str) -> Vec<&str> {
    if content.is_empty() {
        Vec::new()
    } else {
        content.split('\n').collect()
    }
}

/// features 표 안에서 옛 `codex_hooks` 자리를 `hooks = true`로 바꾸고, 다른 hooks 플래그는 지운다
fn rewrite_features_body(features_body: &[&str]) -> Vec<String> {
    let enabled_flag = format!("{CODEX_HOOKS_FEATURE_FLAG} = true");
    let mut next_features_body = Vec::new();
    let mut has_codex_hooks_flag = false;
    for line in features_body {
        if CODEX_HOOKS_FLAG_PATTERN.is_match(line) {
            if !has_codex_hooks_flag {
                next_features_body.push(enabled_flag.clone());
                has_codex_hooks_flag = true;
            }
            continue;
        }
        if !STALE_HOOKS_FLAG_PATTERN.is_match(line) {
            next_features_body.push(line.to_string());
        }
    }
    if !has_codex_hooks_flag {
        next_features_body.push(enabled_flag);
    }
    next_features_body
}

/// config.toml에 `[features] hooks = true`를 넣고 옛 KanVibe notify 항목과 낡은 hooks 플래그를 정리한다
pub fn upsert_codex_config_toml(config_content: &str) -> String {
    let without_legacy_notify = LEGACY_KANVIBE_NOTIFY_PATTERN.replace_all(config_content, "");
    let unified_newlines = without_legacy_notify.replace("\r\n", "\n");
    let normalized = unified_newlines.trim_end();
    let lines = split_lines(normalized);
    let Some((start, end)) = find_features_section(&lines) else {
        let prefix = if normalized.is_empty() {
            String::new()
        } else {
            format!("{normalized}\n\n")
        };
        return format!("{prefix}[features]\n{CODEX_HOOKS_FEATURE_FLAG} = true\n");
    };
    let mut next_lines: Vec<String> = lines[..=start]
        .iter()
        .map(|line| line.to_string())
        .collect();
    next_lines.extend(rewrite_features_body(&lines[start + 1..end]));
    next_lines.extend(lines[end..].iter().map(|line| line.to_string()));
    format!("{}\n", next_lines.join("\n").trim_end())
}

fn has_codex_feature_flag(config_content: &str) -> bool {
    let normalized = config_content.replace("\r\n", "\n");
    let lines = split_lines(&normalized);
    let Some((start, end)) = find_features_section(&lines) else {
        return false;
    };
    lines[start + 1..end]
        .iter()
        .any(|line| ENABLED_HOOKS_FLAG_PATTERN.is_match(line))
}
