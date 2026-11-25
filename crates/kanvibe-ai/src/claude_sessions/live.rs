//! 이 worktree에서 지금 돌고 있는 Claude 세션과 각 세션이 띄운 서브에이전트.
//!
//! 세션 본문은 읽지 않는다. 세션 파일 이름이 곧 세션 id이고 서브에이전트는 세션 id 아래 별도 디렉터리에 파일로 떨어지므로,
//! 수정 시각만 보면 폴링 비용 없이 실행 여부와 개수를 알 수 있다. 서브에이전트 이름만 첫 줄을 읽어 채우는데,
//! 그 줄에 위임받은 작업 프롬프트가 들어 있기 때문이다.

use serde_json::Value;

use super::events::{is_claude_image_companion, message_content, message_role};
use super::{candidate_project_directories, claude_projects_directory};
use crate::error::AiResult;
use crate::host::ModifiedFile;
use crate::javascript::iso_string_from_ms;
use crate::jsonl::{read_json_lines_head, read_json_lines_tail};
use crate::message_text::{extract_plain_text, truncate_text_to};
use crate::node_path;
use crate::session_listing::pick_live_session_files;
use crate::types::{
    AiSessionReaderContext, LiveAiSessionWindows, LiveAiSubtask, LiveProviderSnapshot,
};

const CLAUDE_TAIL_EVENT_COUNT: usize = 60;
const CLAUDE_CURRENT_TASK_LENGTH: usize = 80;
pub(super) const CLAUDE_SUBTASK_LABEL_LENGTH: usize = 40;

pub fn read_claude_live_sessions(
    context: &AiSessionReaderContext,
    windows: LiveAiSessionWindows,
    now_ms: f64,
) -> AiResult<Vec<LiveProviderSnapshot>> {
    let projects_directory = claude_projects_directory(context)?;
    let mut recent_files: Vec<ModifiedFile> = Vec::new();
    for directory in candidate_project_directories(context, &projects_directory) {
        let files = context.host.list_files_modified_within(
            &directory,
            ".jsonl",
            windows.recent_window_ms as f64,
            now_ms,
        )?;
        // 서브에이전트 기록도 같은 트리 아래 있으므로, 세션 파일은 디렉터리 바로 밑에 있는 것만 본다
        recent_files.extend(
            files
                .into_iter()
                .filter(|file| node_path::dirname(&file.file_path) == directory),
        );
    }
    let live_files = pick_live_session_files(
        &recent_files,
        windows.running_window_ms as f64,
        now_ms,
        |file| file.mtime_ms,
    );
    live_files
        .iter()
        .map(|session_file| to_claude_live_snapshot(session_file, windows, context, now_ms))
        .collect()
}

fn to_claude_live_snapshot(
    session_file: &ModifiedFile,
    windows: LiveAiSessionWindows,
    context: &AiSessionReaderContext,
    now_ms: f64,
) -> AiResult<LiveProviderSnapshot> {
    let session_id = node_path::basename_without(&session_file.file_path, ".jsonl");
    let subagents_directory = node_path::join(&[
        &node_path::dirname(&session_file.file_path),
        &session_id,
        "subagents",
    ]);
    let running_subagent_files = context.host.list_files_modified_within(
        &subagents_directory,
        ".jsonl",
        windows.running_window_ms as f64,
        now_ms,
    )?;
    Ok(LiveProviderSnapshot {
        current_task: read_current_activity(&session_file.file_path, context),
        session_id: Some(session_id),
        last_active_at: iso_string_from_ms(session_file.mtime_ms),
        running_subtasks: running_subagent_files
            .iter()
            .map(|file| to_claude_subtask(file, context))
            .collect(),
    })
}

/// 세션이 지금 무엇을 하는지는 마지막 AI 응답이 가장 잘 말해 준다. 아직 응답이 없는 갓 시작한 세션은 마지막 사용자 요청으로 되돌린다.
/// 세션 기록을 읽지 못해도 실행중 여부는 파일 활동만으로 판정할 수 있어 오류는 `None`으로 삼킨다
fn read_current_activity(file_path: &str, context: &AiSessionReaderContext) -> Option<String> {
    let events = read_json_lines_tail(&context.host, file_path, CLAUDE_TAIL_EVENT_COUNT).ok()?;
    find_latest_role_text(&events, "assistant").or_else(|| find_latest_role_text(&events, "user"))
}

fn find_latest_role_text(events: &[Value], role: &str) -> Option<String> {
    events.iter().rev().find_map(|event| {
        if message_role(event) != Some(role) || is_claude_image_companion(event) {
            return None;
        }
        let text = extract_plain_text(message_content(event));
        (!text.is_empty()).then(|| truncate_text_to(&text, CLAUDE_CURRENT_TASK_LENGTH))
    })
}

/// `agent-<agentId>.jsonl`의 첫 줄에는 위임받은 작업 프롬프트가 들어 있어, 이름 대신 그 앞부분을 보여 준다
fn to_claude_subtask(file: &ModifiedFile, context: &AiSessionReaderContext) -> LiveAiSubtask {
    let base_name = node_path::basename_without(&file.file_path, ".jsonl");
    let agent_id = base_name
        .strip_prefix("agent-")
        .unwrap_or(&base_name)
        .to_string();
    LiveAiSubtask {
        id: agent_id,
        name: read_claude_subagent_task_label(&file.file_path, context),
        last_active_at: iso_string_from_ms(file.mtime_ms),
    }
}

fn read_claude_subagent_task_label(
    file_path: &str,
    context: &AiSessionReaderContext,
) -> Option<String> {
    let events = read_json_lines_head(&context.host, file_path, 1).ok()?;
    let first_event = events.first().unwrap_or(&Value::Null);
    let prompt_text = extract_plain_text(message_content(first_event));
    (!prompt_text.is_empty()).then(|| truncate_text_to(&prompt_text, CLAUDE_SUBTASK_LABEL_LENGTH))
}
