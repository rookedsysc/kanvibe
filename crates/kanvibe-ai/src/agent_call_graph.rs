//! 세션 하나가 어떻게 갈라졌는지를 담은 호출 그래프. 기록 전문에서 호출과 결과를 찾아야 해서 대화가 길수록 비용이 늘므로,
//! 주기 조회에 얹지 않고 상세보기를 열 때만 부른다.

use crate::claude_sessions::read_claude_agent_call_graph;
use crate::codex_sessions::read_codex_agent_call_graph;
use crate::javascript::iso_string_from_ms;
use crate::live_sessions::LIVE_SESSION_WINDOWS;
use crate::opencode_sessions::read_opencode_agent_call_graph;
use crate::providers::AiSessionProvider;
use crate::running_agent_panes::flatten_agent_call_nodes;
use crate::types::{AgentCallGraph, AgentCallNode, AiSessionReaderContext};

/// 리더가 실패해도 화면은 빈 그래프로 열려야 하므로 오류를 삼킨다. Gemini CLI 기록에는 서브에이전트를 가리키는 필드가 없어 늘 비어 있다
pub fn read_agent_call_graph(
    context: &AiSessionReaderContext,
    provider: AiSessionProvider,
    session_id: &str,
    now_ms: f64,
) -> AgentCallGraph {
    let roots = match provider {
        AiSessionProvider::Claude => {
            read_claude_agent_call_graph(context, session_id, LIVE_SESSION_WINDOWS, now_ms)
        }
        AiSessionProvider::Codex => {
            read_codex_agent_call_graph(context, session_id, LIVE_SESSION_WINDOWS, now_ms)
        }
        AiSessionProvider::Opencode => {
            read_opencode_agent_call_graph(context, session_id, LIVE_SESSION_WINDOWS, now_ms)
        }
        _ => Ok(Vec::new()),
    }
    .unwrap_or_default();
    AgentCallGraph {
        provider,
        session_id: session_id.to_string(),
        started_at: find_earliest_started_at(&roots),
        read_at: iso_string_from_ms(now_ms).unwrap_or_default(),
        roots,
    }
}

/// 시간축의 왼쪽 끝. 세션 자체의 시작 시각은 provider마다 근거가 달라 가장 먼저 시작한 자식을 기준으로 삼는다
fn find_earliest_started_at(nodes: &[AgentCallNode]) -> Option<String> {
    flatten_agent_call_nodes(nodes)
        .into_iter()
        .filter_map(|node| node.started_at.as_deref())
        .reduce(|earliest, value| if value < earliest { value } else { earliest })
        .map(str::to_string)
}
