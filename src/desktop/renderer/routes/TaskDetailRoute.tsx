import { useEffect, useMemo, useState } from "react";
import { useTranslations } from "next-intl";
import { useParams } from "react-router-dom";
import AiSessionsCard from "@/components/AiSessionsCard";
import CollapsibleSidebar from "@/components/CollapsibleSidebar";
import ConnectTerminalForm from "@/components/ConnectTerminalForm";
import DeleteTaskButton from "@/components/DeleteTaskButton";
import DoneStatusButton from "@/components/DoneStatusButton";
import HooksStatusCard from "@/components/HooksStatusCard";
import NotificationCenterButton from "@/components/NotificationCenterButton";
import TaskDetailInfoCard from "@/components/TaskDetailInfoCard";
import TaskDetailTitleCard from "@/components/TaskDetailTitleCard";
import { Link, useRouter } from "@/desktop/renderer/navigation";
import { getDoneAlertDismissed, getSidebarDefaultCollapsed, getSidebarHintDismissed } from "@/desktop/renderer/actions/appSettings";
import { getGitDiffFiles } from "@/desktop/renderer/actions/diff";
import { deleteTask, fetchAndSavePrUrl, getTaskById, getTaskIdByProjectAndBranch, updateTaskStatus } from "@/desktop/renderer/actions/kanban";
import {
  getTaskAiSessions,
  getTaskCodexHooksStatus,
  getTaskGeminiHooksStatus,
  getTaskHooksStatus,
  getTaskOpenCodeHooksStatus,
} from "@/desktop/renderer/actions/project";
import TerminalLoader from "@/desktop/renderer/components/TerminalLoader";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";
import { TaskStatus } from "@/entities/KanbanTask";

const STATUS_TRANSITIONS = [
  { status: TaskStatus.TODO, labelKey: "moveToTodo" },
  { status: TaskStatus.PROGRESS, labelKey: "moveToProgress" },
  { status: TaskStatus.REVIEW, labelKey: "moveToReview" },
  { status: TaskStatus.DONE, labelKey: "moveToDone" },
] as const;

const AGENT_TAG_STYLES: Record<string, string> = {
  claude: "bg-tag-claude-bg text-tag-claude-text",
  gemini: "bg-tag-gemini-bg text-tag-gemini-text",
  codex: "bg-tag-codex-bg text-tag-codex-text",
};

interface TaskDetailState {
  task: NonNullable<Awaited<ReturnType<typeof getTaskById>>>;
  baseBranchTaskId: string | null;
  diffFiles: Awaited<ReturnType<typeof getGitDiffFiles>>;
  claudeHooksStatus: Awaited<ReturnType<typeof getTaskHooksStatus>>;
  geminiHooksStatus: Awaited<ReturnType<typeof getTaskGeminiHooksStatus>>;
  codexHooksStatus: Awaited<ReturnType<typeof getTaskCodexHooksStatus>>;
  openCodeHooksStatus: Awaited<ReturnType<typeof getTaskOpenCodeHooksStatus>>;
  aiSessions: Awaited<ReturnType<typeof getTaskAiSessions>>;
  sidebarDefaultCollapsed: boolean;
  sidebarHintDismissed: boolean;
  doneAlertDismissed: boolean;
}

const EMPTY_AI_SESSIONS: Awaited<ReturnType<typeof getTaskAiSessions>> = {
  isRemote: false,
  targetPath: null,
  repoPath: null,
  sessions: [],
  sources: [],
};

const DEFAULT_DETAIL_STATE: Omit<TaskDetailState, "task"> = {
  baseBranchTaskId: null,
  diffFiles: [],
  claudeHooksStatus: null,
  geminiHooksStatus: null,
  codexHooksStatus: null,
  openCodeHooksStatus: null,
  aiSessions: EMPTY_AI_SESSIONS,
  sidebarDefaultCollapsed: false,
  sidebarHintDismissed: false,
  doneAlertDismissed: false,
};

export default function TaskDetailRoute() {
  const { id = "" } = useParams();
  const router = useRouter();
  const t = useTranslations("taskDetail");
  const refreshSignal = useRefreshSignal(["all", "task-detail"]);
  const [state, setState] = useState<TaskDetailState | null | undefined>(undefined);
  const [needsMacDesktopHeaderOffset, setNeedsMacDesktopHeaderOffset] = useState(false);

  useEffect(() => {
    const isDesktopApp = window.kanvibeDesktop?.isDesktop === true;
    const isMacDesktop = navigator.userAgent.includes("Mac") || navigator.platform.toLowerCase().includes("mac");
    setNeedsMacDesktopHeaderOffset(isDesktopApp && isMacDesktop);
  }, []);

  useEffect(() => {
    let cancelled = false;

    (async () => {
      const task = await getTaskById(id);
      if (!task) {
        if (!cancelled) {
          setState(null);
        }
        return;
      }

      if (cancelled) {
        return;
      }

      document.title = [task.branchName, task.project?.name].filter(Boolean).join(" - ");
      setState({
        task,
        ...DEFAULT_DETAIL_STATE,
      });

      if (task.branchName && !task.prUrl) {
        void fetchAndSavePrUrl(task.id).then((prUrl) => {
          if (!prUrl || cancelled) {
            return;
          }

          setState((current) => current && current.task.id === task.id
            ? {
                ...current,
                task: {
                  ...current.task,
                  prUrl,
                },
              }
            : current);
        });
      }

      void (async () => {
        const baseBranchName = task.baseBranch ?? "main";
        const foundTaskId = task.projectId ? await getTaskIdByProjectAndBranch(task.projectId, baseBranchName) : null;
        const baseBranchTaskId = foundTaskId !== task.id ? foundTaskId : null;
        const diffFiles = task.branchName && task.worktreePath ? await getGitDiffFiles(id) : [];
        const [claudeHooksStatus, geminiHooksStatus, codexHooksStatus, openCodeHooksStatus, aiSessions, sidebarDefaultCollapsed, sidebarHintDismissed, doneAlertDismissed] = await Promise.all([
          task.projectId ? getTaskHooksStatus(id) : Promise.resolve(null),
          task.projectId ? getTaskGeminiHooksStatus(id) : Promise.resolve(null),
          task.projectId ? getTaskCodexHooksStatus(id) : Promise.resolve(null),
          task.projectId ? getTaskOpenCodeHooksStatus(id) : Promise.resolve(null),
          task.projectId ? getTaskAiSessions(id) : Promise.resolve(EMPTY_AI_SESSIONS),
          getSidebarDefaultCollapsed(),
          getSidebarHintDismissed(),
          getDoneAlertDismissed(),
        ]);

        if (cancelled) {
          return;
        }

        setState((current) => current && current.task.id === task.id
          ? {
              ...current,
              baseBranchTaskId,
              diffFiles,
              claudeHooksStatus,
              geminiHooksStatus,
              codexHooksStatus,
              openCodeHooksStatus,
              aiSessions,
              sidebarDefaultCollapsed,
              sidebarHintDismissed,
              doneAlertDismissed,
            }
          : current);
      })();
    })();

    return () => {
      cancelled = true;
    };
  }, [id, refreshSignal]);

  const agentTagStyle = useMemo(
    () => (state?.task.agentType ? AGENT_TAG_STYLES[state.task.agentType] ?? "bg-tag-neutral-bg text-tag-neutral-text" : null),
    [state?.task.agentType],
  );

  if (state === undefined) {
    return <div className="min-h-screen flex items-center justify-center bg-bg-page text-text-muted">Loading...</div>;
  }

  if (state === null) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-bg-page px-4">
        <div className="flex flex-col items-center gap-4 text-center">
          <p className="text-text-muted">{t("taskNotFound")}</p>
          <button
            type="button"
            onClick={() => router.back()}
            className="rounded-md border border-border-default bg-bg-surface px-4 py-2 text-sm text-text-secondary transition-colors hover:border-brand-primary hover:text-text-primary"
          >
            {t("goBack")}
          </button>
        </div>
      </div>
    );
  }

  const hasTerminal = !!(state.task.sessionType && state.task.sessionName);

  async function handleStatusChange(formData: FormData) {
    const newStatus = formData.get("status") as TaskStatus;
    const updatedTask = await updateTaskStatus(id, newStatus);
    if (newStatus === TaskStatus.DONE) {
      router.push("/");
      return;
    }

    if (updatedTask) {
      setState((current) => current
        ? {
            ...current,
            task: {
              ...current.task,
              ...updatedTask,
            },
          }
        : current);
    }
  }

  async function handleDelete() {
    await deleteTask(id);
    router.push("/");
  }

  return (
    <div className="h-screen flex flex-col lg:flex-row bg-bg-page p-4 gap-4">
      <CollapsibleSidebar defaultCollapsed={state.sidebarDefaultCollapsed} showHint={!state.sidebarHintDismissed}>
        <div className={`flex flex-col gap-4 ${needsMacDesktopHeaderOffset ? "pt-10" : ""}`}>
          <div className="flex items-center justify-between gap-3">
            <Link href="/" className="flex items-center gap-2 text-sm text-text-secondary hover:text-text-primary transition-colors">
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none" className="shrink-0">
                <path d="M10 12L6 8L10 4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
              {t("backToBoard")}
            </Link>
            {!hasTerminal ? <NotificationCenterButton buttonClassName="hover:bg-bg-page" /> : null}
          </div>

          <TaskDetailTitleCard task={state.task} taskId={state.task.id} />

          <TaskDetailInfoCard
            task={state.task}
            agentTagStyle={agentTagStyle}
            baseBranchTaskId={state.baseBranchTaskId}
            diffFileCount={state.diffFiles.length}
          />

          <div className="bg-bg-surface rounded-lg p-5 shadow-sm border border-border-default">
            <h3 className="text-xs font-semibold text-text-muted uppercase tracking-wider mb-3">{t("actions")}</h3>
            <div className="flex flex-wrap gap-2">
              {STATUS_TRANSITIONS.filter((transition) => transition.status !== state.task.status).map((transition) => (
                transition.status === TaskStatus.DONE ? (
                  <DoneStatusButton
                    key={transition.status}
                    statusChangeAction={handleStatusChange}
                    label={t(transition.labelKey)}
                    hasCleanableResources={!!(state.task.branchName || state.task.sessionType)}
                    doneAlertDismissed={state.doneAlertDismissed}
                  />
                ) : (
                  <form key={transition.status} action={handleStatusChange}>
                    <input type="hidden" name="status" value={transition.status} />
                    <button type="submit" className="px-3 py-1.5 text-xs bg-bg-page border border-border-default hover:border-brand-primary hover:text-text-brand text-text-secondary rounded-md transition-colors">
                      {t(transition.labelKey)}
                    </button>
                  </form>
                )
              ))}
            </div>
            <div className="mt-3 pt-3 border-t border-border-subtle">
              <DeleteTaskButton deleteAction={handleDelete} />
            </div>
          </div>

          <HooksStatusCard
            taskId={state.task.id}
            initialClaudeStatus={state.claudeHooksStatus}
            initialGeminiStatus={state.geminiHooksStatus}
            initialCodexStatus={state.codexHooksStatus}
            initialOpenCodeStatus={state.openCodeHooksStatus}
            isRemote={!!state.task.sshHost}
          />

          <AiSessionsCard taskId={state.task.id} data={state.aiSessions} />
        </div>
      </CollapsibleSidebar>

      <main className="flex-1 flex flex-col min-h-0 min-w-0">
        {hasTerminal ? (
          <div className="flex-1 flex flex-col min-h-0 rounded-lg overflow-hidden shadow-md">
            <div className="bg-terminal-chrome flex items-center gap-2 px-4 py-2.5 shrink-0">
              <span className="w-3 h-3 rounded-full bg-traffic-close" />
              <span className="w-3 h-3 rounded-full bg-traffic-minimize" />
              <span className="w-3 h-3 rounded-full bg-traffic-maximize" />
              <span className="ml-3 text-xs text-terminal-text font-mono truncate">{state.task.sessionName ?? t("terminal")}</span>
              <div className="ml-auto">
                <NotificationCenterButton buttonClassName="text-terminal-text hover:text-white hover:bg-white/10" panelClassName="mt-3" />
              </div>
            </div>
            <div className="flex-1 min-h-0 bg-terminal-bg">
              <TerminalLoader taskId={state.task.id} />
            </div>
          </div>
        ) : (
          <div className="flex-1 flex items-center justify-center border border-dashed border-border-default rounded-lg bg-bg-surface">
            {state.task.projectId ? (
              <ConnectTerminalForm
                taskId={state.task.id}
                sshHost={state.task.sshHost}
                onConnected={(connectedTask) => {
                  setState((current) => current && current.task.id === connectedTask.id
                    ? {
                        ...current,
                        task: {
                          ...current.task,
                          ...connectedTask,
                        },
                      }
                    : current);
                }}
              />
            ) : <p className="text-text-muted text-sm">{t("noTerminal")}</p>}
          </div>
        )}
      </main>
    </div>
  );
}
