"use client";

import { useMemo } from "react";
import { useLocale } from "next-intl";
import { AiProviderIcon } from "@/components/AiProviderIcon";
import { selectTerminalTab } from "@/desktop/renderer/actions/terminalTabs";
import { LiveAiSessionPanel } from "@/desktop/renderer/components/LiveAiSessionPanel";
import { useTaskLiveAiSessions } from "@/desktop/renderer/hooks/useLiveAiSessions";
import { navigateToTaskDetail } from "@/desktop/renderer/utils/taskNavigation";
import { countByProvider, filterPanesByWorktree } from "@/desktop/shared/liveAiSessions";
import type { LiveAiSession, RunningAgentPane } from "@/lib/aiSessions/types";

interface TaskCardLiveSessionsProps {
  taskId: string;
  worktreePath: string | null;
  runningPanes: RunningAgentPane[];
  /** 카드가 세션 패널을 열어둘 만큼 오래 활성이었는지. 폴링도 이때만 돈다 */
  isPanelOpen: boolean;
}

/**
 * 보드 카드에 실행중 에이전트를 표시한다.
 * 배지는 tmux pane 목록만 보고 항상 그리고, 세션과 서브태스크까지 담은 패널은 카드가 활성일 때만 연다.
 */
export function TaskCardLiveSessions({
  taskId,
  worktreePath,
  runningPanes,
  isPanelOpen,
}: TaskCardLiveSessionsProps) {
  const locale = useLocale();
  const taskPanes = useMemo(
    () => filterPanesByWorktree(runningPanes, worktreePath),
    [runningPanes, worktreePath],
  );
  const runningCountByProvider = useMemo(() => countByProvider(taskPanes), [taskPanes]);
  const sessions = useTaskLiveAiSessions(taskId, isPanelOpen);

  if (taskPanes.length === 0 && !isPanelOpen) {
    return null;
  }

  const focusSessionTerminal = async (session: LiveAiSession) => {
    if (session.terminalWindow) {
      await selectTerminalTab(taskId, session.terminalWindow.windowId);
    }

    await navigateToTaskDetail(taskId, { currentLocale: locale });
  };

  return (
    <>
      {taskPanes.length > 0 && (
        <span
          className="inline-flex items-center gap-0.5"
          data-testid="task-card-running-agents"
        >
          {runningCountByProvider.map(([provider, count]) => (
            <span key={provider} className="inline-flex items-center gap-0.5" data-testid={`task-card-running-${provider}`}>
              <AiProviderIcon provider={provider} size={12} />
              <span className="text-[10px] font-semibold leading-none text-text-secondary">{count}</span>
            </span>
          ))}
        </span>
      )}

      {isPanelOpen && (
        <div
          role="presentation"
          onClick={(event) => {
            event.preventDefault();
            event.stopPropagation();
          }}
          className="absolute left-2 right-2 top-full z-40 mt-1 overflow-hidden rounded-md border border-border-default bg-bg-surface shadow-lg"
          data-testid="task-card-live-session-popover"
        >
          <LiveAiSessionPanel sessions={sessions} onSelectSession={focusSessionTerminal} />
        </div>
      )}
    </>
  );
}
