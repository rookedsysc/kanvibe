import { LIVE_SESSION_WINDOWS } from "@/lib/aiSessions/liveAiSessions";
import { readClaudeAgentCallGraph } from "@/lib/aiSessions/readClaudeSessions";
import { readCodexAgentCallGraph } from "@/lib/aiSessions/readCodexSessions";
import { readOpenCodeAgentCallGraph } from "@/lib/aiSessions/readOpenCodeSessions";
import type {
  AgentCallGraph,
  AgentCallNode,
  AiSessionProvider,
  AiSessionReaderContext,
  LiveAiSessionWindows,
} from "@/lib/aiSessions/types";

/** Gemini CLI 기록에는 서브에이전트를 가리키는 필드가 없어 그래프를 만들 수 없다 */
async function readEmptyAgentCallGraph(): Promise<AgentCallNode[]> {
  return [];
}

const AGENT_CALL_GRAPH_READERS: Record<
  AiSessionProvider,
  (
    context: AiSessionReaderContext,
    sessionId: string,
    windows: LiveAiSessionWindows,
  ) => Promise<AgentCallNode[]>
> = {
  claude: readClaudeAgentCallGraph,
  codex: readCodexAgentCallGraph,
  opencode: readOpenCodeAgentCallGraph,
  gemini: readEmptyAgentCallGraph,
};

/**
 * 세션 하나가 어떻게 갈라졌는지를 담은 호출 그래프를 만든다.
 *
 * 실행중 목록은 파일 꼬리 몇 줄만 읽어 싸지만, 그래프는 기록 전문에서 호출과 결과를 찾아야 해서
 * 대화가 길수록 비용이 는다. 그래서 주기 조회에 얹지 않고 상세보기를 열 때만 부른다.
 * 리더가 실패해도 화면은 빈 그래프로 열려야 하므로 오류를 삼킨다.
 */
export async function readAgentCallGraph(
  context: AiSessionReaderContext,
  provider: AiSessionProvider,
  sessionId: string,
): Promise<AgentCallGraph> {
  const roots = await AGENT_CALL_GRAPH_READERS[provider](context, sessionId, LIVE_SESSION_WINDOWS)
    .catch(() => [] as AgentCallNode[]);

  return {
    provider,
    sessionId,
    startedAt: findEarliestStartedAt(roots),
    readAt: new Date().toISOString(),
    roots,
  };
}

/**
 * 시간축의 왼쪽 끝.
 * 세션 자체의 시작 시각은 provider마다 근거가 달라, 가장 먼저 시작한 자식을 기준으로 삼는다.
 */
function findEarliestStartedAt(nodes: AgentCallNode[]): string | null {
  const startedAtValues = flattenAgentCallNodes(nodes)
    .map((node) => node.startedAt)
    .filter((startedAt): startedAt is string => startedAt !== null);

  return startedAtValues.length > 0
    ? startedAtValues.reduce((earliest, value) => (value < earliest ? value : earliest))
    : null;
}

export function flattenAgentCallNodes(nodes: AgentCallNode[]): AgentCallNode[] {
  return nodes.flatMap((node) => [node, ...flattenAgentCallNodes(node.children)]);
}
