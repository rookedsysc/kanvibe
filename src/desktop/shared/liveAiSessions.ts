import type { RunningAgentPane } from "@/lib/aiSessions/types";

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
