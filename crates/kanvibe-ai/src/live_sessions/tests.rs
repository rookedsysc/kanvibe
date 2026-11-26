use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use super::*;
use crate::agent_call_graph::read_agent_call_graph;
use crate::javascript::now_ms;
use crate::node_path;
use crate::test_support::{json_lines, local_host, temp_home, write_file};
use crate::types::{AgentCallNode, LiveAiSubtask};

const RUNNING_WINDOW: u64 = 90_000;

/// TS 테스트의 임시 홈과 worktree. 파일 수정 시각은 지금으로부터 `age_ms` 전으로 맞춘다
struct LiveFixture {
    _directory: tempfile::TempDir,
    home: String,
    worktree: String,
}

impl LiveFixture {
    fn new() -> Self {
        let (directory, home) = temp_home();
        let worktree = format!("{home}/repo__worktrees/task");
        std::fs::create_dir_all(&worktree).unwrap();
        Self {
            _directory: directory,
            home,
            worktree,
        }
    }

    fn context(&self) -> AiSessionReaderContext {
        let mut context = AiSessionReaderContext::new(local_host(&self.home), Some(&self.worktree));
        context.repo_path = Some(self.worktree.clone());
        context
    }

    fn claude_project(&self) -> String {
        let directory = node_path::resolve(&[&self.worktree]).replace(['/', '_'], "-");
        format!("{}/.claude/projects/{directory}", self.home)
    }

    fn claude_session(&self, session_id: &str) -> String {
        format!("{}/{session_id}.jsonl", self.claude_project())
    }

    fn claude_subagent(&self, session_id: &str, agent_id: &str) -> String {
        format!(
            "{}/{session_id}/subagents/agent-{agent_id}.jsonl",
            self.claude_project()
        )
    }

    fn codex_rollout(&self, name: &str) -> String {
        format!("{}/.codex/sessions/2026/08/10/{name}", self.home)
    }

    fn read(&self, panes: &[RunningAgentPane]) -> Vec<LiveAiSession> {
        read_live_ai_sessions(&self.context(), panes, now_ms())
    }

    fn claude_graph(&self, session_id: &str) -> Vec<AgentCallNode> {
        read_agent_call_graph(
            &self.context(),
            AiSessionProvider::Claude,
            session_id,
            now_ms(),
        )
        .roots
    }
}

fn write_lines_aged(file_path: &str, values: &[Value], age_ms: u64) {
    write_file(file_path, &json_lines(values));
    let modified_at = SystemTime::now() - Duration::from_millis(age_ms);
    std::fs::File::options()
        .write(true)
        .open(file_path)
        .unwrap()
        .set_modified(modified_at)
        .unwrap();
}

fn user_event() -> Value {
    json!({"type": "user"})
}

fn claude_pane(worktree: &str) -> RunningAgentPane {
    RunningAgentPane {
        provider: AiSessionProvider::Claude,
        worktree_path: worktree.to_string(),
        session_name: "kanvibe-task".to_string(),
        window_id: "@7".to_string(),
        window_name: "claude".to_string(),
    }
}

fn of_provider(sessions: &[LiveAiSession], provider: AiSessionProvider) -> Vec<LiveAiSession> {
    sessions
        .iter()
        .filter(|session| session.provider == provider)
        .cloned()
        .collect()
}

fn claude_agent_call(
    tool_use_id: &str,
    agent_type: &str,
    task: &str,
    prompt: &str,
    started_at: &str,
) -> Value {
    json!({
        "type": "assistant", "timestamp": started_at,
        "message": {"role": "assistant", "content": [{
            "type": "tool_use", "id": tool_use_id, "name": "Agent",
            "input": {"subagent_type": agent_type, "description": task, "prompt": prompt}
        }]}
    })
}

fn claude_agent_result(tool_use_id: &str, agent_id: &str, ended_at: &str) -> Value {
    json!({
        "type": "user", "timestamp": ended_at,
        "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": tool_use_id}]},
        "toolUseResult": {"agentId": agent_id}
    })
}

fn claude_text(role: &str, text: &str, worktree: &str) -> Value {
    json!({"type": role, "sessionId": "session-a", "cwd": worktree, "message": {"role": role, "content": [{"type": "text", "text": text}]}})
}

#[test]
fn fresh_record_is_running_without_a_pane() {
    let fixture = LiveFixture::new();
    write_lines_aged(&fixture.claude_session("session-a"), &[user_event()], 1_000);
    let claude = of_provider(&fixture.read(&[]), AiSessionProvider::Claude).remove(0);
    assert_eq!(claude.state, LiveAiSessionState::Running);
    assert_eq!(claude.session_id.as_deref(), Some("session-a"));
    assert_eq!(claude.terminal_window, None);
}

#[test]
fn stale_record_is_idle() {
    let fixture = LiveFixture::new();
    write_lines_aged(
        &fixture.claude_session("session-a"),
        &[user_event()],
        RUNNING_WINDOW + 60_000,
    );
    let claude = of_provider(&fixture.read(&[]), AiSessionProvider::Claude).remove(0);
    assert_eq!(claude.state, LiveAiSessionState::Idle);
}

#[test]
fn several_running_sessions_come_back_newest_first() {
    let fixture = LiveFixture::new();
    write_lines_aged(&fixture.claude_session("session-a"), &[user_event()], 5_000);
    write_lines_aged(&fixture.claude_session("session-b"), &[user_event()], 1_000);
    let claude = of_provider(&fixture.read(&[]), AiSessionProvider::Claude);
    let ids: Vec<_> = claude
        .iter()
        .map(|session| session.session_id.as_deref())
        .collect();
    assert_eq!(ids, [Some("session-b"), Some("session-a")]);
    assert!(
        claude
            .iter()
            .all(|session| session.state == LiveAiSessionState::Running)
    );
}

#[test]
fn without_running_records_only_the_latest_stays_idle() {
    let fixture = LiveFixture::new();
    write_lines_aged(
        &fixture.claude_session("session-a"),
        &[user_event()],
        RUNNING_WINDOW + 120_000,
    );
    write_lines_aged(
        &fixture.claude_session("session-b"),
        &[user_event()],
        RUNNING_WINDOW + 60_000,
    );
    let claude = of_provider(&fixture.read(&[]), AiSessionProvider::Claude);
    assert_eq!(claude.len(), 1);
    assert_eq!(claude[0].session_id.as_deref(), Some("session-b"));
    assert_eq!(claude[0].state, LiveAiSessionState::Idle);
}

#[test]
fn extra_sessions_get_no_terminal_window() {
    let fixture = LiveFixture::new();
    write_lines_aged(&fixture.claude_session("session-a"), &[user_event()], 5_000);
    write_lines_aged(&fixture.claude_session("session-b"), &[user_event()], 1_000);
    let claude = of_provider(
        &fixture.read(&[claude_pane(&fixture.worktree)]),
        AiSessionProvider::Claude,
    );
    assert_eq!(
        claude[0]
            .terminal_window
            .as_ref()
            .map(|window| window.window_id.as_str()),
        Some("@7")
    );
    assert_eq!(claude[1].terminal_window, None);
}

#[test]
fn call_graph_closes_finished_agents_and_nests_running_ones() {
    let fixture = LiveFixture::new();
    write_lines_aged(
        &fixture.claude_session("session-a"),
        &[
            claude_agent_call(
                "toolu-1",
                "general-purpose",
                "판정 로직 리뷰",
                "리뷰해라",
                "2026-08-15T00:00:00.000Z",
            ),
            claude_agent_result("toolu-1", "a1", "2026-08-15T00:07:00.000Z"),
        ],
        0,
    );
    write_lines_aged(
        &fixture.claude_subagent("session-a", "a1"),
        &[
            json!({"agentId": "a1", "attributionSkill": "context-loader", "message": {"role": "user", "content": "리뷰해라"}}),
            claude_agent_call(
                "toolu-2",
                "Explore",
                "머지 이력 조회",
                "이력 봐라",
                "2026-08-15T00:03:00.000Z",
            ),
        ],
        0,
    );
    write_lines_aged(
        &fixture.claude_subagent("session-a", "a2"),
        &[json!({"agentId": "a2", "message": {"role": "user", "content": "이력 봐라"}})],
        0,
    );

    let roots = fixture.claude_graph("session-a");
    assert_eq!(roots.len(), 1);
    let root = &roots[0];
    assert_eq!(root.id, "a1");
    assert_eq!(root.agent_type.as_deref(), Some("general-purpose"));
    assert_eq!(root.skill.as_deref(), Some("context-loader"));
    assert_eq!(root.task.as_deref(), Some("판정 로직 리뷰"));
    assert_eq!(root.started_at.as_deref(), Some("2026-08-15T00:00:00.000Z"));
    assert_eq!(root.ended_at.as_deref(), Some("2026-08-15T00:07:00.000Z"));
    assert_eq!(root.children.len(), 1);
    let child = &root.children[0];
    assert_eq!(
        (child.id.as_str(), child.agent_type.as_deref()),
        ("a2", Some("Explore"))
    );
    assert_eq!(
        (child.task.as_deref(), child.ended_at.as_deref()),
        (Some("머지 이력 조회"), None)
    );
}

#[test]
fn session_without_agents_has_an_empty_graph() {
    let fixture = LiveFixture::new();
    write_lines_aged(&fixture.claude_session("session-a"), &[user_event()], 0);
    assert!(fixture.claude_graph("session-a").is_empty());
}

#[test]
fn attached_pane_counts_as_running_while_idle() {
    let fixture = LiveFixture::new();
    write_lines_aged(
        &fixture.claude_session("session-a"),
        &[user_event()],
        RUNNING_WINDOW + 60_000,
    );
    let claude = of_provider(
        &fixture.read(&[claude_pane(&fixture.worktree)]),
        AiSessionProvider::Claude,
    )
    .remove(0);
    assert_eq!(claude.state, LiveAiSessionState::Running);
    assert_eq!(
        claude.terminal_window,
        Some(LiveSessionTerminalWindow {
            session_name: "kanvibe-task".to_string(),
            window_id: "@7".to_string(),
            window_name: "claude".to_string(),
        })
    );
}

#[test]
fn only_recently_active_subagents_are_running_subtasks() {
    let fixture = LiveFixture::new();
    write_lines_aged(&fixture.claude_session("session-a"), &[user_event()], 1_000);
    write_lines_aged(
        &fixture.claude_subagent("session-a", "running-agent"),
        &[
            json!({"agentId": "running-agent", "message": {"role": "user", "content": [{"type": "text", "text": "코드베이스를 조사한다"}]}}),
        ],
        5_000,
    );
    write_lines_aged(
        &fixture.claude_subagent("session-a", "finished-agent"),
        &[
            json!({"agentId": "finished-agent", "message": {"role": "user", "content": [{"type": "text", "text": "이미 끝난 작업"}]}}),
        ],
        RUNNING_WINDOW + 60_000,
    );
    let claude = of_provider(&fixture.read(&[]), AiSessionProvider::Claude).remove(0);
    let subtasks: Vec<_> = claude
        .running_subtasks
        .iter()
        .map(|task| (task.id.as_str(), task.name.as_deref()))
        .collect();
    assert_eq!(subtasks, [("running-agent", Some("코드베이스를 조사한다"))]);
}

#[test]
fn codex_counts_only_children_of_the_same_parent_thread() {
    let fixture = LiveFixture::new();
    let meta = |payload: Value| json!({"type": "session_meta", "payload": payload});
    write_lines_aged(
        &fixture.codex_rollout("rollout-parent.jsonl"),
        &[meta(
            json!({"id": "parent-thread", "cwd": fixture.worktree}),
        )],
        1_000,
    );
    write_lines_aged(
        &fixture.codex_rollout("rollout-child.jsonl"),
        &[meta(
            json!({"id": "child-thread", "cwd": fixture.worktree, "parent_thread_id": "parent-thread", "agent_nickname": "Fermat"}),
        )],
        2_000,
    );
    write_lines_aged(
        &fixture.codex_rollout("rollout-other.jsonl"),
        &[meta(
            json!({"id": "other-child", "cwd": fixture.worktree, "parent_thread_id": "someone-else"}),
        )],
        2_000,
    );
    let codex = of_provider(&fixture.read(&[]), AiSessionProvider::Codex).remove(0);
    assert_eq!(codex.session_id.as_deref(), Some("parent-thread"));
    let subtasks: Vec<(&str, Option<&str>)> = codex
        .running_subtasks
        .iter()
        .map(|task: &LiveAiSubtask| (task.id.as_str(), task.name.as_deref()))
        .collect();
    assert_eq!(subtasks, [("child-thread", Some("Fermat"))]);
}

#[test]
fn current_task_is_the_last_ai_answer() {
    let fixture = LiveFixture::new();
    let worktree = fixture.worktree.clone();
    write_lines_aged(
        &fixture.claude_session("session-a"),
        &[
            claude_text("user", "처음 요청", &worktree),
            claude_text("assistant", "리더를 먼저 고칩니다", &worktree),
            claude_text("user", "나중 요청", &worktree),
            claude_text("assistant", "테스트를 돌리는 중입니다", &worktree),
        ],
        1_000,
    );
    let claude = of_provider(&fixture.read(&[]), AiSessionProvider::Claude).remove(0);
    assert_eq!(
        claude.current_task.as_deref(),
        Some("테스트를 돌리는 중입니다")
    );
}

#[test]
fn current_task_falls_back_to_last_user_request() {
    let fixture = LiveFixture::new();
    let worktree = fixture.worktree.clone();
    write_lines_aged(
        &fixture.claude_session("session-a"),
        &[claude_text("user", "방금 시킨 일", &worktree)],
        1_000,
    );
    let claude = of_provider(&fixture.read(&[]), AiSessionProvider::Claude).remove(0);
    assert_eq!(claude.current_task.as_deref(), Some("방금 시킨 일"));
}

#[test]
fn codex_current_task_is_the_last_ai_answer() {
    let fixture = LiveFixture::new();
    write_lines_aged(
        &fixture.codex_rollout("rollout-parent.jsonl"),
        &[
            json!({"type": "session_meta", "payload": {"id": "parent-thread", "cwd": fixture.worktree}}),
            json!({"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "코덱스 요청"}]}}),
            json!({"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "코덱스가 지금 하는 일"}]}}),
        ],
        1_000,
    );
    let codex = of_provider(&fixture.read(&[]), AiSessionProvider::Codex).remove(0);
    assert_eq!(codex.current_task.as_deref(), Some("코덱스가 지금 하는 일"));
}

#[test]
fn providers_without_records_or_panes_are_left_out() {
    let fixture = LiveFixture::new();
    assert!(fixture.read(&[]).is_empty());
}
