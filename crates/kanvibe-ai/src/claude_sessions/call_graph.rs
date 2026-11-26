//! 세션 하나가 띄운 서브에이전트 호출 그래프.
//!
//! 간선은 두 가지로 이어지고 둘 다 추측이 없다. 끝난 자식은 부모 기록의 `toolUseResult.agentId`가 자식 기록 파일 이름을
//! 그대로 가리키고, 아직 도는 자식은 부모가 넘긴 위임 프롬프트가 자식 기록의 첫 줄과 같은 문자열이다. 서브에이전트 기록도
//! 자기가 띄운 자식을 같은 규칙으로 가리키므로 손자 이하까지 저절로 이어진다. 기록 전문을 훑어야 하므로 상세보기를 열 때만 부른다.

use serde_json::Value;
use std::collections::{HashMap, HashSet};

use super::events::{message_content, string_field};
use super::live::CLAUDE_SUBTASK_LABEL_LENGTH;
use super::{claude_projects_directory, to_claude_project_dir_name};
use crate::error::AiResult;
use crate::javascript::iso_string_from_ms;
use crate::jsonl::read_json_lines;
use crate::message_text::truncate_text_to;
use crate::node_path;
use crate::session_listing::{get_candidate_paths, map_with_concurrency, parse_concurrency};
use crate::types::{AgentCallNode, AiSessionReaderContext, LiveAiSessionWindows};

/// 서브에이전트를 띄우는 도구 이름. 배포판에 따라 둘 중 하나로 기록된다
const CLAUDE_SUBAGENT_TOOL_NAMES: [&str; 2] = ["Agent", "Task"];

pub fn read_claude_agent_call_graph(
    context: &AiSessionReaderContext,
    session_id: &str,
    windows: LiveAiSessionWindows,
    now_ms: f64,
) -> AiResult<Vec<AgentCallNode>> {
    let Some(session_directory) = find_claude_session_directory(context, session_id)? else {
        return Ok(Vec::new());
    };
    let subagent_files = context.host.list_files_modified_within(
        &node_path::join(&[&session_directory, session_id, "subagents"]),
        ".jsonl",
        windows.recent_window_ms as f64,
        now_ms,
    )?;
    let session_file = node_path::join(&[&session_directory, &format!("{session_id}.jsonl")]);
    // TS처럼 세션 기록과 서브에이전트 기록을 함께 읽는다. 첫 항목이 세션 기록이다
    let transcript_files: Vec<(&str, f64)> = std::iter::once((session_file.as_str(), 0.0))
        .chain(
            subagent_files
                .iter()
                .map(|file| (file.file_path.as_str(), file.mtime_ms)),
        )
        .collect();
    let mut transcripts = map_with_concurrency(
        &transcript_files,
        parse_concurrency(context),
        |(file_path, mtime_ms), _| read_claude_transcript(file_path, *mtime_ms, context),
    )
    .into_iter();
    let session_transcript = transcripts.next().expect("세션 기록을 첫 항목으로 넣었다");
    let identified: Vec<ClaudeTranscript> = transcripts
        .filter(|transcript| transcript.agent_id.is_some())
        .collect();

    let mut index = ClaudeGraphIndex {
        transcripts_by_agent_id: HashMap::new(),
        agent_id_by_delegated_prompt: HashMap::new(),
        running_since: now_ms - windows.running_window_ms as f64,
        visited_agent_ids: HashSet::new(),
    };
    for transcript in &identified {
        let agent_id = transcript
            .agent_id
            .clone()
            .expect("거른 기록에는 agentId가 있다");
        index
            .transcripts_by_agent_id
            .insert(agent_id.clone(), transcript);
        if let Some(prompt) = &transcript.delegated_prompt {
            index
                .agent_id_by_delegated_prompt
                .insert(prompt.clone(), agent_id);
        }
    }
    Ok(to_claude_agent_call_nodes(&session_transcript, &mut index))
}

struct ClaudeGraphIndex<'a> {
    transcripts_by_agent_id: HashMap<String, &'a ClaudeTranscript>,
    agent_id_by_delegated_prompt: HashMap<String, String>,
    running_since: f64,
    /// 기록이 서로를 가리키는 이상한 경우에도 재귀가 멈추도록 이미 그린 에이전트를 기억한다
    visited_agent_ids: HashSet<String>,
}

fn to_claude_agent_call_nodes(
    transcript: &ClaudeTranscript,
    index: &mut ClaudeGraphIndex<'_>,
) -> Vec<AgentCallNode> {
    let mut nodes = Vec::new();
    for call in &transcript.calls {
        let finished_call = transcript
            .finished_calls_by_tool_use_id
            .get(&call.tool_use_id);
        let agent_id = match finished_call {
            Some(finished) => Some(finished.agent_id.clone()),
            None => call
                .prompt
                .as_ref()
                .and_then(|prompt| index.agent_id_by_delegated_prompt.get(prompt).cloned()),
        };
        let Some(agent_id) = agent_id.filter(|id| !id.is_empty()) else {
            continue;
        };
        if !index.visited_agent_ids.insert(agent_id.clone()) {
            continue;
        }
        let child_transcript = index.transcripts_by_agent_id.get(&agent_id).copied();
        let ended_at = finished_call
            .and_then(|finished| finished.ended_at.clone())
            .or_else(|| abandoned_ended_at(child_transcript, index.running_since));
        nodes.push(AgentCallNode {
            skill: child_transcript.and_then(|child| child.skill.clone()),
            agent_type: call.agent_type.clone(),
            task: call.task.clone(),
            started_at: call.started_at.clone(),
            ended_at,
            children: child_transcript
                .map(|child| to_claude_agent_call_nodes(child, index))
                .unwrap_or_default(),
            id: agent_id,
        });
    }
    nodes
}

/// 완료 기록이 없는 자식의 끝 시각. 부모가 결과를 적기 전에 세션이 끊기면 자식은 영원히 도는 것처럼 보이므로,
/// 그 기록이 한동안 움직이지 않았다면 마지막 활동을 끝으로 보고 아직 움직이고 있으면 끝을 비워 둔다
fn abandoned_ended_at(transcript: Option<&ClaudeTranscript>, running_since: f64) -> Option<String> {
    let transcript = transcript?;
    if transcript.mtime_ms >= running_since {
        return None;
    }
    iso_string_from_ms(transcript.mtime_ms)
}

/// 기록 파일 하나가 띄운 서브에이전트들. 세션 본체와 서브에이전트 기록이 같은 모양이라 한 타입으로 다룬다
#[derive(Default)]
struct ClaudeTranscript {
    /// 세션 본체면 `None`
    agent_id: Option<String>,
    mtime_ms: f64,
    /// 이 기록이 위임받은 프롬프트. 부모의 미완료 호출과 이어 붙이는 열쇠다
    delegated_prompt: Option<String>,
    skill: Option<String>,
    calls: Vec<ClaudeAgentCall>,
    finished_calls_by_tool_use_id: HashMap<String, FinishedAgentCall>,
}

struct ClaudeAgentCall {
    tool_use_id: String,
    agent_type: Option<String>,
    task: Option<String>,
    prompt: Option<String>,
    started_at: Option<String>,
}

struct FinishedAgentCall {
    agent_id: String,
    ended_at: Option<String>,
}

/// 기록을 읽지 못하면 빈 기록으로 본다
fn read_claude_transcript(
    file_path: &str,
    mtime_ms: f64,
    context: &AiSessionReaderContext,
) -> ClaudeTranscript {
    let mut transcript = ClaudeTranscript {
        mtime_ms,
        ..ClaudeTranscript::default()
    };
    let events = read_json_lines(&context.host, file_path).unwrap_or_default();
    for event in &events {
        consume_transcript_event(&mut transcript, event);
    }
    transcript
}

fn consume_transcript_event(transcript: &mut ClaudeTranscript, event: &Value) {
    if transcript.agent_id.is_none() {
        transcript.agent_id = string_field(event, "agentId").map(str::to_string);
    }
    if transcript.skill.is_none() {
        transcript.skill = string_field(event, "attributionSkill").map(str::to_string);
    }
    if transcript.delegated_prompt.is_none() {
        transcript.delegated_prompt = message_content(event).as_str().map(str::to_string);
    }
    let parts = message_content(event)
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    transcript.calls.extend(
        parts
            .iter()
            .filter_map(|part| to_claude_agent_call(event, part)),
    );
    if let Some((tool_use_id, finished)) = to_claude_finished_call(event, parts) {
        transcript
            .finished_calls_by_tool_use_id
            .insert(tool_use_id, finished);
    }
}

fn to_claude_agent_call(event: &Value, part: &Value) -> Option<ClaudeAgentCall> {
    let tool_use_id = string_field(part, "id").filter(|id| !id.is_empty())?;
    let name = string_field(part, "name").unwrap_or_default();
    if string_field(part, "type") != Some("tool_use") || !CLAUDE_SUBAGENT_TOOL_NAMES.contains(&name)
    {
        return None;
    }
    let input = part.get("input").unwrap_or(&Value::Null);
    Some(ClaudeAgentCall {
        tool_use_id: tool_use_id.to_string(),
        agent_type: string_field(input, "subagent_type").map(str::to_string),
        task: string_field(input, "description")
            .map(|description| truncate_text_to(description, CLAUDE_SUBTASK_LABEL_LENGTH)),
        prompt: string_field(input, "prompt").map(str::to_string),
        started_at: string_field(event, "timestamp").map(str::to_string),
    })
}

fn to_claude_finished_call(event: &Value, parts: &[Value]) -> Option<(String, FinishedAgentCall)> {
    let agent_id = event
        .get("toolUseResult")
        .and_then(|result| result.get("agentId"))
        .and_then(Value::as_str)?;
    let tool_use_id = parts
        .iter()
        .find(|part| string_field(part, "type") == Some("tool_result"))
        .and_then(|part| string_field(part, "tool_use_id"))?;
    Some((
        tool_use_id.to_string(),
        FinishedAgentCall {
            agent_id: agent_id.to_string(),
            ended_at: string_field(event, "timestamp").map(str::to_string),
        },
    ))
}

fn find_claude_session_directory(
    context: &AiSessionReaderContext,
    session_id: &str,
) -> AiResult<Option<String>> {
    let projects_directory = claude_projects_directory(context)?;
    for candidate_path in get_candidate_paths(context) {
        let directory = node_path::join(&[
            &projects_directory,
            &to_claude_project_dir_name(&candidate_path),
        ]);
        let session_file = node_path::join(&[&directory, &format!("{session_id}.jsonl")]);
        if context.host.path_exists(&session_file)? {
            return Ok(Some(directory));
        }
    }
    Ok(None)
}
