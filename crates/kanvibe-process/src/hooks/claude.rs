//! Claude Code hook 설치(TS `claudeHooksSetup.ts`). `<repo>/.claude/hooks/*.sh`와 `<repo>/.claude/settings.json`의 hooks 항목을 쓴다.

use std::collections::BTreeMap;

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

const AGENT_LABEL: &str = "Claude Code";
const CONFIG_DIR_NAME: &str = ".claude";
const SETTINGS_FILE_NAME: &str = "settings.json";
const HOOK_TIMEOUT_SECONDS: u32 = 10;
const ASK_USER_QUESTION_MATCHER: &str = "AskUserQuestion";

pub const PROMPT_HOOK_SCRIPT_NAME: &str = "kanvibe-prompt-hook.sh";
pub const STOP_HOOK_SCRIPT_NAME: &str = "kanvibe-stop-hook.sh";
pub const QUESTION_HOOK_SCRIPT_NAME: &str = "kanvibe-question-hook.sh";

const CLAUDE_PROMPT_COMMAND: &str = "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/kanvibe-prompt-hook.sh";
const CLAUDE_STOP_COMMAND: &str = "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/kanvibe-stop-hook.sh";
const CLAUDE_QUESTION_COMMAND: &str =
    "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/kanvibe-question-hook.sh";

const CLAUDE_HOOK_SCRIPTS: [ShellHookScriptDefinition; 3] = [
    ShellHookScriptDefinition {
        file_name: PROMPT_HOOK_SCRIPT_NAME,
        event_label: "UserPromptSubmit",
        description: "사용자가 prompt를 입력하면 현재 task를 PROGRESS로 변경한다.",
        status: ShellHookStatus::Progress,
        trailing_output: None,
        runtime_note: None,
    },
    ShellHookScriptDefinition {
        file_name: STOP_HOOK_SCRIPT_NAME,
        event_label: "Stop",
        description: "AI 응답이 완료되면 현재 task를 REVIEW로 변경한다.",
        status: ShellHookStatus::Review,
        trailing_output: None,
        runtime_note: None,
    },
    ShellHookScriptDefinition {
        file_name: QUESTION_HOOK_SCRIPT_NAME,
        event_label: "PreToolUse (AskUserQuestion)",
        description: "Claude가 사용자에게 질문할 때 현재 task를 PENDING으로 변경한다.",
        status: ShellHookStatus::Pending,
        trailing_output: None,
        runtime_note: None,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeHooksStatus {
    pub is_installed: bool,
    pub has_prompt_hook: bool,
    pub has_stop_hook: bool,
    pub has_question_hook: bool,
    pub has_settings_entry: bool,
    pub scripts: ShellHookScriptsStatus,
}

/// UserPromptSubmit hook bash 스크립트
pub fn generate_prompt_hook_script(kanvibe_url: &str, task_id: &str) -> String {
    generate_shell_hook_script(AGENT_LABEL, &CLAUDE_HOOK_SCRIPTS[0], kanvibe_url, task_id)
}

/// Stop hook bash 스크립트
pub fn generate_stop_hook_script(kanvibe_url: &str, task_id: &str) -> String {
    generate_shell_hook_script(AGENT_LABEL, &CLAUDE_HOOK_SCRIPTS[1], kanvibe_url, task_id)
}

/// PreToolUse(AskUserQuestion) hook bash 스크립트
pub fn generate_question_hook_script(kanvibe_url: &str, task_id: &str) -> String {
    generate_shell_hook_script(AGENT_LABEL, &CLAUDE_HOOK_SCRIPTS[2], kanvibe_url, task_id)
}

fn hooks_dir(repo_path: &str) -> String {
    join_posix_path(&[repo_path, CONFIG_DIR_NAME, "hooks"])
}

fn settings_path(repo_path: &str) -> String {
    join_posix_path(&[repo_path, CONFIG_DIR_NAME, SETTINGS_FILE_NAME])
}

/// 설치 파일을 만들기 전에 읽어야 하는 기존 설정 파일 경로
pub fn get_claude_hook_install_input_paths(repo_path: &str) -> Vec<String> {
    vec![settings_path(repo_path)]
}

/// settings.json의 이벤트별로 갱신할 (이벤트, 스크립트 이름, 새 항목)
fn claude_hook_entries() -> [(&'static str, &'static str, Value); 4] {
    [
        (
            "UserPromptSubmit",
            PROMPT_HOOK_SCRIPT_NAME,
            build_command_hook_entry(CLAUDE_PROMPT_COMMAND, HOOK_TIMEOUT_SECONDS),
        ),
        (
            "PreToolUse",
            QUESTION_HOOK_SCRIPT_NAME,
            build_matcher_command_hook_entry(
                ASK_USER_QUESTION_MATCHER,
                CLAUDE_QUESTION_COMMAND,
                HOOK_TIMEOUT_SECONDS,
            ),
        ),
        (
            "PostToolUse",
            PROMPT_HOOK_SCRIPT_NAME,
            build_matcher_command_hook_entry(
                ASK_USER_QUESTION_MATCHER,
                CLAUDE_PROMPT_COMMAND,
                HOOK_TIMEOUT_SECONDS,
            ),
        ),
        (
            "Stop",
            STOP_HOOK_SCRIPT_NAME,
            build_command_hook_entry(CLAUDE_STOP_COMMAND, HOOK_TIMEOUT_SECONDS),
        ),
    ]
}

/// Claude Code hooks 설치 산출물. 기존 settings.json이 있으면 kanvibe hooks 항목만 갱신하고 나머지는 보존한다
pub fn build_claude_hook_files(
    repo_path: &str,
    task_id: &str,
    kanvibe_url: &str,
    settings_content: &str,
) -> Result<Vec<HookProviderFile>, HookInstallError> {
    let mut settings = parse_json_hook_settings(settings_content, "Claude settings.json")?;
    let hooks = ensure_json_hook_buckets(&mut settings);
    for (event, script_name, entry) in claude_hook_entries() {
        let script_path = format!("{CONFIG_DIR_NAME}/hooks/{script_name}");
        let entries = upsert_json_hook_entries(hooks.get(event), &script_path, entry);
        hooks.insert(event.to_string(), Value::Array(entries));
    }
    let mut files = build_shell_hook_script_files(
        &hooks_dir(repo_path),
        AGENT_LABEL,
        &CLAUDE_HOOK_SCRIPTS,
        kanvibe_url,
        task_id,
    );
    files.push(HookProviderFile {
        file_path: settings_path(repo_path),
        content: serialize_json_hook_settings(&settings),
        mode: None,
    });
    Ok(files)
}

/// 지정된 repo에 Claude Code hooks를 설정한다. 로컬과 원격 repo 모두 같은 산출물을 기록한다
pub fn setup_claude_hooks(
    context: &HookInstallContext,
    repo_path: &str,
    task_id: &str,
    kanvibe_url: &str,
    ssh_host: Option<&str>,
) -> Result<(), HookInstallError> {
    let settings_path = settings_path(repo_path);
    let options = ExecGitOptions {
        should_check_host_key_strictly: true,
        ..ExecGitOptions::default()
    };
    let existing_files =
        context
            .files
            .read_text_files(std::slice::from_ref(&settings_path), ssh_host, &options)?;
    let settings_content = existing_files
        .get(&settings_path)
        .map_or("", |file| file.content.as_str());
    let files = build_claude_hook_files(repo_path, task_id, kanvibe_url, settings_content)?;
    write_hook_provider_files(context, &files, ssh_host)?;
    register_kanvibe_hook_target(context, repo_path, task_id, kanvibe_url, ssh_host)
}

/// 지정된 repo의 Claude Code hooks 설치 상태
pub fn get_claude_hooks_status(
    context: &HookInstallContext,
    repo_path: &str,
    task_id: Option<&str>,
    ssh_host: Option<&str>,
    existing_files: Option<BTreeMap<String, TextFileReadResult>>,
) -> Result<ClaudeHooksStatus, HookInstallError> {
    let settings_path = settings_path(repo_path);
    let extra_file_paths = [settings_path.clone()];
    let state = read_shell_hook_provider_state(
        context,
        ShellHookProviderStateRequest {
            repo_path,
            hooks_dir: &hooks_dir(repo_path),
            definitions: &CLAUDE_HOOK_SCRIPTS,
            extra_file_paths: &extra_file_paths,
            task_id,
            ssh_host,
            files: existing_files,
        },
    )?;
    let settings_content = state
        .files
        .get(&settings_path)
        .map_or("", |file| file.content.as_str());
    let has_settings_entry = has_claude_settings_entry(settings_content);
    Ok(ClaudeHooksStatus {
        is_installed: is_shell_hook_provider_installed(&state, &[has_settings_entry]),
        has_prompt_hook: state.script_files[0].exists,
        has_stop_hook: state.script_files[1].exists,
        has_question_hook: state.script_files[2].exists,
        has_settings_entry,
        scripts: state.status,
    })
}

fn has_claude_settings_entry(settings_content: &str) -> bool {
    // 해석할 수 없는 설정에는 KanVibe hook이 등록돼 있다고 볼 수 없다
    let settings =
        parse_json_hook_settings(settings_content, "Claude settings.json").unwrap_or_default();
    // hooks가 없거나 객체가 아니면 아래 조회가 모두 Null이 되어 거짓이다
    let hooks = settings.get("hooks").unwrap_or(&Value::Null);
    has_command_hook_entry(&hooks["UserPromptSubmit"], CLAUDE_PROMPT_COMMAND)
        && has_command_hook_entry(&hooks["Stop"], CLAUDE_STOP_COMMAND)
        && has_matcher_command_hook_entry(
            &hooks["PreToolUse"],
            ASK_USER_QUESTION_MATCHER,
            CLAUDE_QUESTION_COMMAND,
        )
        && has_matcher_command_hook_entry(
            &hooks["PostToolUse"],
            ASK_USER_QUESTION_MATCHER,
            CLAUDE_PROMPT_COMMAND,
        )
}
