import type { AgentCallNode, AiSessionProvider, RunningAgentPane } from "@/lib/aiSessions/types";

/**
 * 같은 worktree에서 도는 pane만 남긴다. 하위 디렉터리에서 실행한 에이전트도 그 worktree의 것으로 본다.
 *
 * main은 태스크 하나의 실행중 세션을 만들 때, 렌더러는 보드 카드 배지를 그릴 때 같은 판정을 써야 해서
 * 양쪽이 공유하는 이 자리에 둔다.
 */
export function filterPanesByWorktree(
  panes: RunningAgentPane[],
  worktreePath: string | null,
): RunningAgentPane[] {
  if (!worktreePath) {
    return [];
  }

  return panes.filter((pane) =>
    pane.worktreePath === worktreePath || pane.worktreePath.startsWith(`${worktreePath}/`));
}

/**
 * provider별 개수를 센다. 어떤 에이전트가 몇 개 도는지가 총합보다 알아보기 쉽다.
 *
 * pane 목록과 세션 목록의 순서는 tmux 상태나 파일 수정 시각에 따라 흔들리므로, provider 이름으로
 * 정렬해 보드 배지와 세션 패널의 칩이 같은 자리를 지키게 한다.
 */
export function countByProvider<T extends { provider: AiSessionProvider }>(
  items: T[],
): [AiSessionProvider, number][] {
  const counts = new Map<AiSessionProvider, number>();
  for (const item of items) {
    counts.set(item.provider, (counts.get(item.provider) ?? 0) + 1);
  }

  return [...counts.entries()].sort(([left], [right]) => left.localeCompare(right));
}

/**
 * 호출 그래프의 트리를 깊이 우선 순서의 평면 목록으로 편다.
 *
 * main은 가장 이른 시작 시각을 찾을 때, 렌더러는 그래프 패널의 행을 그릴 때 같은 순서를 봐야 한다.
 * 세션 기록 리더는 Node 전용 모듈을 끌고 오므로, 두 쪽이 공유하는 이 순수 함수만 여기에 둔다.
 */
export function flattenAgentCallNodes(nodes: AgentCallNode[]): AgentCallNode[] {
  return nodes.flatMap((node) => [node, ...flattenAgentCallNodes(node.children)]);
}
