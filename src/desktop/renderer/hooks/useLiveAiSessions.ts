import { useCallback } from "react";
import {
  getRunningAgentPanes,
  getTaskAgentCallGraph,
  getTaskLiveAiSessions,
} from "@/desktop/renderer/actions/project";
import { usePolledValue } from "@/desktop/renderer/hooks/usePolledValue";
import type { AgentCallGraph, LiveAiSession, RunningAgentPane } from "@/lib/aiSessions/types";

/** 세션 패널은 서브태스크가 뜨고 지는 것을 눈으로 따라갈 수 있어야 해서 짧게 돈다 */
const LIVE_SESSION_POLL_INTERVAL_MS = 2_000;

/** 보드 배지는 provider가 붙었는지만 보여주므로 더 느리게 돌아도 된다 */
const RUNNING_PANE_POLL_INTERVAL_MS = 5_000;

const EMPTY_SESSIONS: LiveAiSession[] = [];
const EMPTY_PANES: RunningAgentPane[] = [];

/** 태스크 하나의 실행중 세션과 서브태스크. 패널이 열려 있는 동안에만 폴링한다 */
export function useTaskLiveAiSessions(taskId: string | null, isEnabled: boolean): LiveAiSession[] {
  const read = useCallback(
    async () => (taskId ? (await getTaskLiveAiSessions(taskId)).sessions : EMPTY_SESSIONS),
    [taskId],
  );

  return usePolledValue(read, EMPTY_SESSIONS, LIVE_SESSION_POLL_INTERVAL_MS, isEnabled && Boolean(taskId));
}

/** 보드 전체가 공유하는 실행중 에이전트 목록. 카드마다 조회하지 않도록 한 번만 읽는다 */
export function useRunningAgentPanes(isEnabled: boolean): RunningAgentPane[] {
  const read = useCallback(() => getRunningAgentPanes(), []);

  return usePolledValue(read, EMPTY_PANES, RUNNING_PANE_POLL_INTERVAL_MS, isEnabled);
}

/**
 * 호출 그래프는 기록 전문을 훑어야 만들어져서 목록 조회보다 훨씬 비싸다.
 * 그래서 목록 주기를 따라가지 않고, 그래프를 열어 둔 동안에만 느리게 다시 읽는다.
 */
const AGENT_CALL_GRAPH_POLL_INTERVAL_MS = 10_000;

const EMPTY_GRAPH: AgentCallGraph | null = null;

/** 세션 하나가 어떻게 갈라졌는지. 세션 상세를 열었을 때만 조회한다 */
export function useTaskAgentCallGraph(
  taskId: string | null,
  session: LiveAiSession | null,
): AgentCallGraph | null {
  const provider = session?.provider ?? null;
  const sessionId = session?.sessionId ?? null;

  const read = useCallback(
    async () => (taskId && provider && sessionId
      ? await getTaskAgentCallGraph(taskId, provider, sessionId)
      : EMPTY_GRAPH),
    [taskId, provider, sessionId],
  );

  return usePolledValue(
    read,
    EMPTY_GRAPH,
    AGENT_CALL_GRAPH_POLL_INTERVAL_MS,
    Boolean(taskId && sessionId),
  );
}
