//! 지금 돌고 있는 Codex 스레드와 서브에이전트 호출 그래프.
//!
//! Codex는 서브에이전트도 자기 rollout 파일을 따로 만들고 첫 줄 `session_meta`에 부모 스레드를 남긴다.
//! 그래서 최근에 수정된 rollout의 첫 줄만 읽으면 부모·자식 관계를 전부 복원할 수 있다.

use std::collections::HashSet;

use serde_json::Value;

use super::{CodexThreadMeta, codex_sessions_directory, read_codex_session_meta};
use crate::error::AiResult;
use crate::host::ModifiedFile;
use crate::javascript::iso_string_from_ms;
use crate::jsonl::read_json_lines_tail;
use crate::message_text::{extract_plain_text, truncate_text_to};
use crate::session_listing::{
    determine_match_scope, map_with_concurrency, parse_concurrency, pick_live_session_files,
};
use crate::types::{
    AgentCallNode, AiSessionReaderContext, LiveAiSessionWindows, LiveAiSubtask,
    LiveProviderSnapshot,
};

const CODEX_TAIL_EVENT_COUNT: usize = 60;
const CODEX_CURRENT_TASK_LENGTH: usize = 80;

#[derive(Clone)]
struct CodexThreadFile {
    file: ModifiedFile,
    meta: CodexThreadMeta,
}

/// 최근에 수정된 rollout과 그 첫 줄 메타데이터. 메타데이터를 못 읽은 파일은 뺀다
fn read_recent_threads(
    context: &AiSessionReaderContext,
    windows: LiveAiSessionWindows,
    now_ms: f64,
) -> AiResult<Vec<CodexThreadFile>> {
    let recent_files = context.host.list_files_modified_within(
        &codex_sessions_directory(context)?,
        ".jsonl",
        windows.recent_window_ms as f64,
        now_ms,
    )?;
    let threads = map_with_concurrency(&recent_files, parse_concurrency(context), |file, _| {
        read_codex_session_meta(&file.file_path, context).map(|meta| CodexThreadFile {
            file: file.clone(),
            meta,
        })
    });
    Ok(threads.into_iter().flatten().collect())
}

fn is_root_thread(meta: &CodexThreadMeta) -> bool {
    meta.parent_thread_id.as_deref().is_none_or(str::is_empty)
}

/// 이 worktree에서 지금 돌고 있는 Codex 스레드와, 각 스레드가 띄운 서브에이전트를 찾는다
pub fn read_codex_live_sessions(
    context: &AiSessionReaderContext,
    windows: LiveAiSessionWindows,
    now_ms: f64,
) -> AiResult<Vec<LiveProviderSnapshot>> {
    let threads = read_recent_threads(context, windows, now_ms)?;
    let own_threads: Vec<CodexThreadFile> = threads
        .iter()
        .filter(|thread| is_root_thread(&thread.meta))
        .filter(|thread| determine_match_scope(thread.meta.cwd.as_deref(), context).is_some())
        .cloned()
        .collect();
    let live_threads = pick_live_session_files(
        &own_threads,
        windows.running_window_ms as f64,
        now_ms,
        |thread| thread.file.mtime_ms,
    );
    let running_since = now_ms - windows.running_window_ms as f64;
    // TS처럼 스레드마다 현재 작업 읽기를 함께 돌린다
    Ok(map_with_concurrency(
        &live_threads,
        parse_concurrency(context),
        |own_thread, _| LiveProviderSnapshot {
            session_id: Some(own_thread.meta.thread_id.clone()),
            current_task: read_codex_current_activity(&own_thread.file.file_path, context),
            last_active_at: iso_string_from_ms(own_thread.file.mtime_ms),
            running_subtasks: running_subtasks(&threads, &own_thread.meta.thread_id, running_since),
        },
    ))
}

fn running_subtasks(
    threads: &[CodexThreadFile],
    parent_thread_id: &str,
    running_since: f64,
) -> Vec<LiveAiSubtask> {
    threads
        .iter()
        .filter(|thread| thread.meta.parent_thread_id.as_deref() == Some(parent_thread_id))
        .filter(|thread| thread.file.mtime_ms >= running_since)
        .map(|thread| LiveAiSubtask {
            id: thread.meta.thread_id.clone(),
            name: thread.meta.agent_nickname.clone(),
            last_active_at: iso_string_from_ms(thread.file.mtime_ms),
        })
        .collect()
}

/// 세션이 지금 무엇을 하는지는 마지막 AI 응답이 가장 잘 말해 준다. 아직 응답이 없는 갓 시작한 세션만 마지막 사용자 입력으로 되돌린다.
/// rollout을 읽지 못해도 실행중 여부는 파일 활동만으로 판정할 수 있어 오류는 `None`으로 삼킨다
fn read_codex_current_activity(
    file_path: &str,
    context: &AiSessionReaderContext,
) -> Option<String> {
    let events = read_json_lines_tail(&context.host, file_path, CODEX_TAIL_EVENT_COUNT).ok()?;
    find_latest_codex_role_text(&events, "assistant")
        .or_else(|| find_latest_codex_role_text(&events, "user"))
}

fn find_latest_codex_role_text(events: &[Value], role: &str) -> Option<String> {
    events.iter().rev().find_map(|event| {
        let payload = event.get("payload")?;
        if payload.get("role").and_then(Value::as_str) != Some(role) {
            return None;
        }
        let text = extract_plain_text(payload.get("content").unwrap_or(&Value::Null));
        (!text.is_empty()).then(|| truncate_text_to(&text, CODEX_CURRENT_TASK_LENGTH))
    })
}

struct CodexGraphIndex {
    /// 부모 스레드 ID와 그 자식들. 기록에 나온 순서를 지킨다
    threads_by_parent_id: Vec<(String, Vec<CodexThreadFile>)>,
    running_since: f64,
    /// 기록이 서로를 부모로 가리키는 이상한 경우에도 재귀가 멈추도록 이미 그린 스레드를 기억한다
    visited_thread_ids: HashSet<String>,
}

/// 스레드 하나가 띄운 Codex 서브에이전트 호출 그래프를 만든다. 서브에이전트의 `parent_thread_id`를 계속 타고 내려가 깊이 제한 없이 복원한다.
/// 끝났다는 기록은 남지 않아, 한동안 움직이지 않은 스레드를 끝난 것으로 본다
pub fn read_codex_agent_call_graph(
    context: &AiSessionReaderContext,
    session_id: &str,
    windows: LiveAiSessionWindows,
    now_ms: f64,
) -> AiResult<Vec<AgentCallNode>> {
    let threads = read_recent_threads(context, windows, now_ms)?;
    let mut threads_by_parent_id: Vec<(String, Vec<CodexThreadFile>)> = Vec::new();
    for thread in threads
        .into_iter()
        .filter(|thread| !is_root_thread(&thread.meta))
    {
        let parent_id = thread.meta.parent_thread_id.clone().unwrap_or_default();
        match threads_by_parent_id
            .iter_mut()
            .find(|(known, _)| *known == parent_id)
        {
            Some((_, siblings)) => siblings.push(thread),
            None => threads_by_parent_id.push((parent_id, vec![thread])),
        }
    }
    let mut index = CodexGraphIndex {
        threads_by_parent_id,
        running_since: now_ms - windows.running_window_ms as f64,
        visited_thread_ids: HashSet::new(),
    };
    Ok(to_codex_agent_call_nodes(session_id, &mut index))
}

fn to_codex_agent_call_nodes(
    parent_thread_id: &str,
    index: &mut CodexGraphIndex,
) -> Vec<AgentCallNode> {
    let child_threads: Vec<CodexThreadFile> = index
        .threads_by_parent_id
        .iter()
        .find(|(known, _)| known == parent_thread_id)
        .map(|(_, children)| children.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|child| !index.visited_thread_ids.contains(&child.meta.thread_id))
        .collect();
    child_threads
        .into_iter()
        .map(|child| {
            index
                .visited_thread_ids
                .insert(child.meta.thread_id.clone());
            let is_running = child.file.mtime_ms >= index.running_since;
            AgentCallNode {
                id: child.meta.thread_id.clone(),
                agent_type: None,
                skill: None,
                task: child.meta.agent_nickname.clone(),
                started_at: child.meta.started_at.clone(),
                ended_at: if is_running {
                    None
                } else {
                    iso_string_from_ms(child.file.mtime_ms)
                },
                children: to_codex_agent_call_nodes(&child.meta.thread_id, index),
            }
        })
        .collect()
}
