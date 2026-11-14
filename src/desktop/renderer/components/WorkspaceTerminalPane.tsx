"use client";

import { useTranslations } from "next-intl";
import AutoConnectTerminalSession from "@/components/AutoConnectTerminalSession";
import TerminalLoader from "@/desktop/renderer/components/TerminalLoader";
import TerminalTabBar from "@/desktop/renderer/components/TerminalTabBar";
import { useTerminalTabs } from "@/desktop/renderer/hooks/useTerminalTabs";
import { SessionType, type KanbanTask } from "@/entities/KanbanTask";

interface WorkspaceTerminalPaneProps {
  /** 왼쪽에서 고른 task. 고른 것이 없으면 null이다 */
  task: KanbanTask | null;
  projectName: string | null;
  defaultSessionType: SessionType;
  onTaskConnected: (task: KanbanTask) => void;
}

/** 워크스페이스 오른쪽 열. 고른 task의 터미널을 보여 주고, 세션이 없으면 바로 연결한다 */
export default function WorkspaceTerminalPane({
  task,
  projectName,
  defaultSessionType,
  onTaskConnected,
}: WorkspaceTerminalPaneProps) {
  const t = useTranslations("workspace");
  const tTaskDetail = useTranslations("taskDetail");
  const hasTerminal = Boolean(task?.sessionType && task.sessionName);
  const isTerminalSession = task?.sessionType === SessionType.TERMINAL;
  const terminalTabs = useTerminalTabs({
    taskId: task?.id ?? "",
    sessionType: (task?.sessionType as SessionType | undefined) ?? null,
    isRemote: Boolean(task?.sshHost),
    isVisible: hasTerminal,
  });

  if (!task) {
    return (
      <section className="flex min-w-0 flex-1 items-center justify-center bg-bg-page">
        <p className="text-sm text-text-muted">{t("noTaskSelected")}</p>
      </section>
    );
  }

  return (
    <section aria-label={task.title} className="flex min-w-0 flex-1 flex-col bg-terminal-bg">
      <div className="flex h-10 shrink-0 items-center gap-3 border-b border-white/10 px-3 text-terminal-text">
        <span className="max-w-[45%] shrink-0 truncate text-sm">
          <span className="font-medium">{task.title}</span>
          {projectName ? <span className="opacity-60"> · {projectName}</span> : null}
        </span>
        {isTerminalSession && terminalTabs.tabs.length > 0 ? (
          <TerminalTabBar
            tabs={terminalTabs.tabs}
            onSelect={(tabId) => { void terminalTabs.selectTab(tabId); }}
            onCreate={() => { void terminalTabs.createTab(); }}
            onClose={(tabId) => { void terminalTabs.closeTab(tabId); }}
            onRename={(tabId, name) => { void terminalTabs.renameTab(tabId, name); }}
            onMove={(tabId, targetIndex) => { void terminalTabs.moveTab(tabId, targetIndex); }}
            pendingAction={terminalTabs.pendingAction}
            actionError={terminalTabs.actionError}
            onRetryAction={() => { void terminalTabs.retryAction(); }}
          />
        ) : null}
      </div>
      <div className="min-h-0 flex-1">
        {hasTerminal ? (
          <TerminalLoader
            key={task.id}
            taskId={task.id}
            tabs={isTerminalSession ? terminalTabs.tabs : undefined}
            tabsError={terminalTabs.error}
            isTabListLoaded={terminalTabs.isTabListLoaded}
            onRetryTabs={() => { void terminalTabs.refresh(); }}
            onCreateTab={() => { void terminalTabs.createTab(); }}
            isRemote={Boolean(task.sshHost)}
          />
        ) : (
          <div className="flex h-full items-center justify-center bg-bg-page">
            {task.projectId ? (
              <AutoConnectTerminalSession
                key={task.id}
                taskId={task.id}
                sshHost={task.sshHost}
                defaultSessionType={defaultSessionType}
                onConnected={onTaskConnected}
              />
            ) : (
              <p className="text-sm text-text-muted">{tTaskDetail("noTerminal")}</p>
            )}
          </div>
        )}
      </div>
    </section>
  );
}
