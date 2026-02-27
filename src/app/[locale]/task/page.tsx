"use client";

import { Suspense, useState, useEffect, useCallback } from "react";
import { useSearchParams } from "next/navigation";
import { useTranslations } from "next-intl";
import { useRouter } from "@/i18n/navigation";
import { Link } from "@/i18n/navigation";
import { ipcKanban, ipcProject, ipcSettings, ipcDiff } from "@/lib/ipc";
import type { DiffFile } from "@/lib/ipc";
import { TaskStatus } from "@/entities/KanbanTask";
import type { KanbanTask } from "@/entities/KanbanTask";
import type { ClaudeHooksStatus } from "@/lib/claudeHooksSetup";
import type { GeminiHooksStatus } from "@/lib/geminiHooksSetup";
import type { CodexHooksStatus } from "@/lib/codexHooksSetup";
import type { OpenCodeHooksStatus } from "@/lib/openCodeHooksSetup";
import TerminalLoader from "@/components/TerminalLoader";
import ConnectTerminalForm from "@/components/ConnectTerminalForm";
import DeleteTaskButton from "@/components/DeleteTaskButton";
import DoneStatusButton from "@/components/DoneStatusButton";
import HooksStatusCard from "@/components/HooksStatusCard";
import CollapsibleSidebar from "@/components/CollapsibleSidebar";
import TaskDetailTitleCard from "@/components/TaskDetailTitleCard";
import TaskDetailInfoCard from "@/components/TaskDetailInfoCard";

const STATUS_TRANSITIONS: { status: TaskStatus; labelKey: string }[] = [
  { status: TaskStatus.TODO, labelKey: "moveToTodo" },
  { status: TaskStatus.PROGRESS, labelKey: "moveToProgress" },
  { status: TaskStatus.REVIEW, labelKey: "moveToReview" },
  { status: TaskStatus.DONE, labelKey: "moveToDone" },
];

const AGENT_TAG_STYLES: Record<string, string> = {
  claude: "bg-tag-claude-bg text-tag-claude-text",
  gemini: "bg-tag-gemini-bg text-tag-gemini-text",
  codex: "bg-tag-codex-bg text-tag-codex-text",
};

function TaskDetailContent() {
  const searchParams = useSearchParams();
  const taskId = searchParams.get("id");
  const router = useRouter();
  const t = useTranslations("taskDetail");

  const [task, setTask] = useState<KanbanTask | null>(null);
  const [loading, setLoading] = useState(true);
  const [baseBranchTaskId, setBaseBranchTaskId] = useState<string | null>(null);
  const [diffFiles, setDiffFiles] = useState<DiffFile[]>([]);
  const [claudeHooksStatus, setClaudeHooksStatus] = useState<ClaudeHooksStatus | null>(null);
  const [geminiHooksStatus, setGeminiHooksStatus] = useState<GeminiHooksStatus | null>(null);
  const [codexHooksStatus, setCodexHooksStatus] = useState<CodexHooksStatus | null>(null);
  const [openCodeHooksStatus, setOpenCodeHooksStatus] = useState<OpenCodeHooksStatus | null>(null);
  const [sidebarDefaultCollapsed, setSidebarDefaultCollapsed] = useState(false);
  const [sidebarHintDismissed, setSidebarHintDismissed] = useState(false);
  const [doneAlertDismissed, setDoneAlertDismissed] = useState(false);

  useEffect(() => {
    if (!taskId) return;

    async function load() {
      try {
        const taskData = await ipcKanban.getTaskById(taskId!);
        if (!taskData) {
          setLoading(false);
          return;
        }

        /** PR URL이 아직 없으면 자동 조회 */
        if (taskData.branchName && !taskData.prUrl) {
          const prUrl = await ipcKanban.fetchAndSavePrUrl(taskData.id);
          if (prUrl) taskData.prUrl = prUrl;
        }

        setTask(taskData);
        document.title = [taskData.branchName, (taskData as unknown as { project?: { name: string } }).project?.name]
          .filter(Boolean)
          .join(" - ") || "KanVibe";

        /** 병렬로 부가 데이터를 로드한다 */
        const baseBranchName = taskData.baseBranch ?? "main";

        const [
          foundTaskId,
          files,
          claude,
          gemini,
          codex,
          openCode,
          collapsed,
          hintDismissed,
          doneDismissed,
        ] = await Promise.all([
          taskData.projectId
            ? ipcKanban.getTaskIdByProjectAndBranch(taskData.projectId, baseBranchName)
            : Promise.resolve(null),
          taskData.branchName ? ipcDiff.getGitDiffFiles(taskId!) : Promise.resolve([]),
          taskData.projectId ? ipcProject.getTaskHooksStatus(taskId!) : Promise.resolve(null),
          taskData.projectId ? ipcProject.getTaskGeminiHooksStatus(taskId!) : Promise.resolve(null),
          taskData.projectId ? ipcProject.getTaskCodexHooksStatus(taskId!) : Promise.resolve(null),
          taskData.projectId ? ipcProject.getTaskOpenCodeHooksStatus(taskId!) : Promise.resolve(null),
          ipcSettings.getSidebarDefaultCollapsed(),
          ipcSettings.getSidebarHintDismissed(),
          ipcSettings.getDoneAlertDismissed(),
        ]);

        setBaseBranchTaskId(foundTaskId !== taskData.id ? foundTaskId : null);
        setDiffFiles(files);
        setClaudeHooksStatus(claude);
        setGeminiHooksStatus(gemini);
        setCodexHooksStatus(codex);
        setOpenCodeHooksStatus(openCode);
        setSidebarDefaultCollapsed(collapsed);
        setSidebarHintDismissed(hintDismissed);
        setDoneAlertDismissed(doneDismissed);
      } catch (error) {
        console.error("태스크 데이터 로딩 실패:", error);
      } finally {
        setLoading(false);
      }
    }
    load();
  }, [taskId]);

  const handleStatusChange = useCallback(async (newStatus: TaskStatus) => {
    if (!taskId) return;
    await ipcKanban.updateTaskStatus(taskId, newStatus);
    if (newStatus === TaskStatus.DONE) {
      router.push("/");
    } else {
      const updated = await ipcKanban.getTaskById(taskId);
      if (updated) setTask(updated);
    }
  }, [taskId, router]);

  const handleDelete = useCallback(async () => {
    if (!taskId) return;
    await ipcKanban.deleteTask(taskId);
    router.push("/");
  }, [taskId, router]);

  if (!taskId) {
    return (
      <div className="h-screen flex items-center justify-center bg-bg-page">
        <p className="text-text-muted text-sm">Task ID가 필요합니다.</p>
      </div>
    );
  }

  if (loading) {
    return (
      <div className="h-screen flex items-center justify-center bg-bg-page">
        <div className="animate-spin rounded-full h-8 w-8 border-2 border-brand-primary border-t-transparent" />
      </div>
    );
  }

  if (!task) {
    return (
      <div className="h-screen flex items-center justify-center bg-bg-page">
        <p className="text-text-muted text-sm">작업을 찾을 수 없습니다.</p>
      </div>
    );
  }

  const hasTerminal = task.sessionType && task.sessionName;
  const agentTagStyle = task.agentType
    ? AGENT_TAG_STYLES[task.agentType] ?? "bg-tag-neutral-bg text-tag-neutral-text"
    : null;

  return (
    <div className="h-screen flex flex-col lg:flex-row bg-bg-page p-4 gap-4">
      <CollapsibleSidebar defaultCollapsed={sidebarDefaultCollapsed} showHint={!sidebarHintDismissed}>
        <Link
          href="/"
          className="flex items-center gap-2 text-sm text-text-secondary hover:text-text-primary transition-colors"
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" className="shrink-0">
            <path d="M10 12L6 8L10 4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          {t("backToBoard")}
        </Link>

        <TaskDetailTitleCard task={task} taskId={task.id} />

        <TaskDetailInfoCard task={task} agentTagStyle={agentTagStyle} baseBranchTaskId={baseBranchTaskId} diffFileCount={diffFiles.length} />

        <div className="bg-bg-surface rounded-lg p-5 shadow-sm border border-border-default">
          <h3 className="text-xs font-semibold text-text-muted uppercase tracking-wider mb-3">
            {t("actions")}
          </h3>
          <div className="flex flex-wrap gap-2">
            {STATUS_TRANSITIONS.filter(
              (transition) => transition.status !== task.status
            ).map((transition) =>
              transition.status === TaskStatus.DONE ? (
                <DoneStatusButton
                  key={transition.status}
                  onStatusChange={() => handleStatusChange(TaskStatus.DONE)}
                  label={t(transition.labelKey)}
                  hasCleanableResources={!!(task.branchName || task.sessionType)}
                  doneAlertDismissed={doneAlertDismissed}
                />
              ) : (
                <button
                  key={transition.status}
                  type="button"
                  onClick={() => handleStatusChange(transition.status)}
                  className="px-3 py-1.5 text-xs bg-bg-page border border-border-default hover:border-brand-primary hover:text-text-brand text-text-secondary rounded-md transition-colors"
                >
                  {t(transition.labelKey)}
                </button>
              )
            )}
          </div>
          <div className="mt-3 pt-3 border-t border-border-subtle">
            <DeleteTaskButton onDelete={handleDelete} />
          </div>
        </div>

        <HooksStatusCard
          taskId={task.id}
          initialClaudeStatus={claudeHooksStatus}
          initialGeminiStatus={geminiHooksStatus}
          initialCodexStatus={codexHooksStatus}
          initialOpenCodeStatus={openCodeHooksStatus}
          isRemote={!!task.sshHost}
        />
      </CollapsibleSidebar>

      <main className="flex-1 flex flex-col min-h-0 min-w-0">
        {hasTerminal ? (
          <div className="flex-1 flex flex-col min-h-0 rounded-lg overflow-hidden shadow-md">
            <div className="bg-terminal-chrome flex items-center gap-2 px-4 py-2.5 shrink-0">
              <span className="w-3 h-3 rounded-full bg-traffic-close" />
              <span className="w-3 h-3 rounded-full bg-traffic-minimize" />
              <span className="w-3 h-3 rounded-full bg-traffic-maximize" />
              <span className="ml-3 text-xs text-terminal-text font-mono truncate">
                {task.sessionName ?? t("terminal")}
              </span>
            </div>
            <div className="flex-1 min-h-0 bg-terminal-bg">
              <TerminalLoader taskId={task.id} />
            </div>
          </div>
        ) : (
          <div className="flex-1 flex items-center justify-center border border-dashed border-border-default rounded-lg bg-bg-surface">
            {task.projectId ? (
              <ConnectTerminalForm taskId={task.id} />
            ) : (
              <p className="text-text-muted text-sm">{t("noTerminal")}</p>
            )}
          </div>
        )}
      </main>
    </div>
  );
}

export default function TaskDetailPage() {
  return (
    <Suspense
      fallback={
        <div className="h-screen flex items-center justify-center bg-bg-page">
          <div className="animate-spin rounded-full h-8 w-8 border-2 border-brand-primary border-t-transparent" />
        </div>
      }
    >
      <TaskDetailContent />
    </Suspense>
  );
}
