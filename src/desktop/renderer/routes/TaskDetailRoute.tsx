import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import { useParams } from "react-router-dom";
import ConnectTerminalForm from "@/components/ConnectTerminalForm";
import CreateTaskModal from "@/components/CreateTaskModal";
import DeleteTaskButton from "@/components/DeleteTaskButton";
import DoneStatusButton from "@/components/DoneStatusButton";
import HooksStatusCard from "@/components/HooksStatusCard";
import NotificationCenterButton, { type NotificationCenterButtonHandle } from "@/components/NotificationCenterButton";
import TaskDetailInfoCard from "@/components/TaskDetailInfoCard";
import TaskDetailTitleCard from "@/components/TaskDetailTitleCard";
import { Link, useRouter } from "@/desktop/renderer/navigation";
import { getDefaultSessionType, getDoneAlertDismissed, getSidebarDefaultCollapsed } from "@/desktop/renderer/actions/appSettings";
import { getGitDiffFiles } from "@/desktop/renderer/actions/diff";
import { deleteTask, getTaskById, getTaskIdByProjectAndBranch, updateTaskStatus } from "@/desktop/renderer/actions/kanban";
import {
  getTaskAiSessions,
  getTaskAiSessionDetail,
  getTaskCodexHooksStatus,
  getTaskGeminiHooksStatus,
  getTaskHooksStatus,
  getTaskOpenCodeHooksStatus,
  getAllProjects,
} from "@/desktop/renderer/actions/project";
import { useBoardCommands, type BranchTodoDefaults } from "@/desktop/renderer/components/BoardCommandProvider";
import TerminalLoader from "@/desktop/renderer/components/TerminalLoader";
import { fetchPrUrlWithPrompt } from "@/desktop/renderer/utils/fetchPrUrlWithPrompt";
import { INITIAL_DESKTOP_LOAD_TIMEOUT_MS, logDesktopInitialLoadTimeout } from "@/desktop/renderer/utils/loadingTimeout";
import { buildRouteCacheKey, readRouteCache, removeRouteCache, writeRouteCache } from "@/desktop/renderer/utils/routeCache";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";
import { requestActiveTerminalFocusAfterUiSettles } from "@/desktop/renderer/utils/terminalFocus";
import { SessionType, TaskStatus } from "@/entities/KanbanTask";
import { useEscapeKey } from "@/hooks/useEscapeKey";
import type {
  AggregatedAiMessage,
  AggregatedAiSession,
  AggregatedAiSessionDetail,
  AggregatedAiSessionsResult,
} from "@/lib/aiSessions/types";

const STATUS_TRANSITIONS = [
  { status: TaskStatus.TODO, labelKey: "moveToTodo" },
  { status: TaskStatus.PROGRESS, labelKey: "moveToProgress" },
  { status: TaskStatus.REVIEW, labelKey: "moveToReview" },
  { status: TaskStatus.DONE, labelKey: "moveToDone" },
] as const;

const INLINE_CHAT_DETAIL_LIMIT = 40;

const AGENT_TAG_STYLES: Record<string, string> = {
  claude: "bg-tag-claude-bg text-tag-claude-text",
  gemini: "bg-tag-gemini-bg text-tag-gemini-text",
  codex: "bg-tag-codex-bg text-tag-codex-text",
};

type DetailPanel = "overview" | "actions" | "hooks";
type MainView = "terminal" | "chat";

interface TaskDetailState {
  task: NonNullable<Awaited<ReturnType<typeof getTaskById>>>;
  baseBranchTaskId: string | null;
  diffFiles: Awaited<ReturnType<typeof getGitDiffFiles>>;
  claudeHooksStatus: Awaited<ReturnType<typeof getTaskHooksStatus>>;
  geminiHooksStatus: Awaited<ReturnType<typeof getTaskGeminiHooksStatus>>;
  codexHooksStatus: Awaited<ReturnType<typeof getTaskCodexHooksStatus>>;
  openCodeHooksStatus: Awaited<ReturnType<typeof getTaskOpenCodeHooksStatus>>;
  aiSessions: Awaited<ReturnType<typeof getTaskAiSessions>>;
  projects: Awaited<ReturnType<typeof getAllProjects>>;
  sidebarDefaultCollapsed: boolean;
  defaultSessionType: Awaited<ReturnType<typeof getDefaultSessionType>>;
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
  projects: [],
  sidebarDefaultCollapsed: false,
  defaultSessionType: SessionType.TMUX,
  doneAlertDismissed: false,
};

function getTaskDetailRouteCacheKey(taskId: string) {
  return buildRouteCacheKey("task-detail", taskId);
}

function normalizeCachedTaskDetailState(cachedState: TaskDetailState | null): TaskDetailState | null {
  if (!cachedState) {
    return null;
  }

  const routeState = { ...cachedState } as TaskDetailState & {
    sidebarHintDismissed?: boolean;
  };
  delete routeState.sidebarHintDismissed;
  return {
    ...DEFAULT_DETAIL_STATE,
    ...routeState,
    sidebarDefaultCollapsed: routeState.sidebarDefaultCollapsed ?? DEFAULT_DETAIL_STATE.sidebarDefaultCollapsed,
  };
}

function selectInlineChatSession(sessions: AggregatedAiSession[]) {
  return sessions.find((session) => session.provider === "claude") ?? sessions[0] ?? null;
}

function InlineAiChatView({ taskId, data }: { taskId: string; data: AggregatedAiSessionsResult }) {
  const t = useTranslations("taskDetail");
  const selectedSession = useMemo(() => selectInlineChatSession(data.sessions), [data.sessions]);
  const [detail, setDetail] = useState<AggregatedAiSessionDetail | null>(null);
  const [detailError, setDetailError] = useState<{ sessionId: string; message: string } | null>(null);

  useEffect(() => {
    if (!selectedSession) {
      return;
    }

    let cancelled = false;

    getTaskAiSessionDetail(
      taskId,
      selectedSession.provider,
      selectedSession.id,
      selectedSession.sourceRef ?? null,
      null,
      INLINE_CHAT_DETAIL_LIMIT,
      false,
    ).then((result) => {
      if (cancelled) return;
      if (!result) {
        setDetailError({ sessionId: selectedSession.id, message: t("aiSessions.detailError") });
        return;
      }

      setDetail(result);
      setDetailError(null);
    }).catch(() => {
      if (cancelled) return;
      setDetailError({ sessionId: selectedSession.id, message: t("aiSessions.detailError") });
    });

    return () => {
      cancelled = true;
    };
  }, [selectedSession, taskId, t]);

  const messages = selectedSession && detail?.sessionId === selectedSession.id ? detail.messages : [];
  const error = selectedSession && detailError?.sessionId === selectedSession.id ? detailError.message : null;
  const isLoading = Boolean(selectedSession && detail?.sessionId !== selectedSession.id && !error);

  return (
    <div
      data-testid="inline-ai-chat"
      className="flex h-full min-h-0 flex-1 translate-y-0 flex-col overflow-hidden rounded-lg border border-border-default bg-bg-page opacity-100 shadow-md transition-all duration-200 ease-out"
    >
      <div className="flex shrink-0 items-center gap-3 border-b border-border-default bg-terminal-chrome px-4 py-2.5">
        <div className="min-w-0 flex-1">
          <p className="truncate text-xs font-semibold text-terminal-text">
            {selectedSession?.title ?? t("aiSessions.title")}
          </p>
          <p className="mt-0.5 truncate text-[11px] text-terminal-text/70">
            {selectedSession ? selectedSession.provider : t("aiSessions.noPreview")}
          </p>
        </div>
        {selectedSession ? (
          <span className="rounded border border-tag-claude-text/30 bg-tag-claude-bg px-2 py-0.5 text-[10px] font-semibold text-tag-claude-text">
            {selectedSession.provider}
          </span>
        ) : null}
      </div>

      <div className="flex-1 space-y-4 overflow-y-auto px-5 py-5">
        {!selectedSession ? <InlineAiChatEmpty text={data.isRemote ? t("aiSessions.remoteBadge") : t("aiSessions.noPreview")} /> : null}
        {selectedSession && isLoading ? <InlineAiChatEmpty text={t("aiSessions.loadingDetail")} /> : null}
        {selectedSession && !isLoading && error ? <InlineAiChatEmpty text={error} /> : null}
        {selectedSession && !isLoading && !error && messages.length === 0 ? <InlineAiChatEmpty text={t("aiSessions.noPreview")} /> : null}
        {!isLoading && !error && messages.map((message, index) => (
          <InlineAiChatMessage key={`${message.role}-${message.timestamp ?? index}-${index}`} message={message} />
        ))}
      </div>
    </div>
  );
}

function InlineAiChatEmpty({ text }: { text: string }) {
  return (
    <div className="flex h-full items-center justify-center">
      <p className="rounded-md border border-border-default bg-bg-surface px-4 py-3 text-sm text-text-muted">
        {text}
      </p>
    </div>
  );
}

function InlineAiChatMessage({ message }: { message: AggregatedAiMessage }) {
  const isUserMessage = message.role === "user";
  const displayedText = message.fullText || message.text;

  return (
    <div className={`flex ${isUserMessage ? "justify-end" : "justify-start"}`}>
      <div
        className={`max-w-[74%] rounded-2xl px-4 py-3 text-sm leading-6 shadow-sm ${
          isUserMessage
            ? "rounded-br-md bg-brand-primary text-white"
            : "rounded-bl-md border border-border-default bg-bg-surface text-text-primary"
        }`}
      >
        <div className={`mb-1 text-[11px] font-semibold ${isUserMessage ? "text-white/75" : "text-text-muted"}`}>
          {message.role}
        </div>
        <p className="whitespace-pre-wrap break-words">{displayedText}</p>
      </div>
    </div>
  );
}

export default function TaskDetailRoute() {
  const { id = "" } = useParams();
  const router = useRouter();
  const boardCommands = useBoardCommands();
  const t = useTranslations("taskDetail");
  const tc = useTranslations("common");
  const refreshSignal = useRefreshSignal(["all", "task-detail"]);
  const cachedState = useMemo(
    () => (id ? normalizeCachedTaskDetailState(readRouteCache<TaskDetailState>(getTaskDetailRouteCacheKey(id))) : null),
    [id],
  );
  const [state, setState] = useState<TaskDetailState | null | undefined>(cachedState ?? undefined);
  const [isCreateTaskModalOpen, setIsCreateTaskModalOpen] = useState(false);
  const [createTaskDefaults, setCreateTaskDefaults] = useState<BranchTodoDefaults | null>(null);
  const needsMacDesktopHeaderOffset = useMemo(() => {
    const isDesktopApp = window.kanvibeDesktop?.isDesktop === true;
    const isMacDesktop = navigator.userAgent.includes("Mac") || navigator.platform.toLowerCase().includes("mac");
    return isDesktopApp && isMacDesktop;
  }, []);
  const [activePanel, setActivePanel] = useState<DetailPanel | null>(
    cachedState && !cachedState.sidebarDefaultCollapsed ? "overview" : null,
  );
  const [mainView, setMainView] = useState<MainView>("terminal");
  const notificationCenterRef = useRef<NotificationCenterButtonHandle>(null);
  const currentTaskIdRef = useRef(id);
  const hasTerminal = !!(state?.task.sessionType && state.task.sessionName);

  useEffect(() => boardCommands.registerNotificationCenterHandler(() => {
    notificationCenterRef.current?.toggle();
  }), [boardCommands]);

  useEffect(() => boardCommands.registerBoardHandlers({
    toggleNotificationCenter() {
      notificationCenterRef.current?.toggle();
    },
    openProjectFilter() {},
    openCreateTaskModal(defaults) {
      setCreateTaskDefaults(defaults ?? (state?.task.projectId
        ? {
            projectId: state.task.projectId,
            baseBranch: state.task.branchName || state.task.baseBranch || "",
          }
        : null));
      setIsCreateTaskModalOpen(true);
    },
  }), [boardCommands, state?.task.baseBranch, state?.task.branchName, state?.task.projectId]);

  useEffect(() => {
    if (currentTaskIdRef.current === id) {
      return;
    }

    currentTaskIdRef.current = id;
    let cancelled = false;
    window.queueMicrotask(() => {
      if (cancelled) {
        return;
      }

      setState(cachedState ?? undefined);
      setActivePanel(cachedState && !cachedState.sidebarDefaultCollapsed ? "overview" : null);
      setMainView("terminal");
    });

    return () => {
      cancelled = true;
    };
  }, [cachedState, id]);

  useEffect(() => {
    if (!state || state === null) {
      return;
    }

    document.title = [state.task.branchName, state.task.project?.name].filter(Boolean).join(" - ");
  }, [state]);

  useEffect(() => {
    if (!hasTerminal || isCreateTaskModalOpen || mainView !== "terminal") {
      return;
    }

    requestActiveTerminalFocusAfterUiSettles();
  }, [hasTerminal, id, isCreateTaskModalOpen, mainView]);

  useEffect(() => {
    if (!id) {
      return;
    }

    const cacheKey = getTaskDetailRouteCacheKey(id);
    if (state === null) {
      removeRouteCache(cacheKey);
      return;
    }

    if (state !== undefined) {
      writeRouteCache(cacheKey, state);
    }
  }, [id, state]);

  useEffect(() => {
    let cancelled = false;
    let loadingTimeout: number | null = window.setTimeout(() => {
      loadingTimeout = null;
      if (!cancelled) {
        logDesktopInitialLoadTimeout("task-detail", { taskId: id });
        setState((current) => current === undefined ? null : current);
      }
    }, INITIAL_DESKTOP_LOAD_TIMEOUT_MS);

    const clearLoadingTimeout = () => {
      if (loadingTimeout === null) {
        return;
      }

      window.clearTimeout(loadingTimeout);
      loadingTimeout = null;
    };

    (async () => {
      try {
        const [task, sidebarDefaultCollapsed] = await Promise.all([
          getTaskById(id),
          getSidebarDefaultCollapsed().catch(() => DEFAULT_DETAIL_STATE.sidebarDefaultCollapsed),
        ]);
        clearLoadingTimeout();

        if (!task) {
          if (!cancelled) {
            setState(null);
          }
          return;
        }

        if (cancelled) {
          return;
        }

        setState((current) => current && current.task.id === task.id
          ? {
              ...current,
              task: {
                ...current.task,
                ...task,
              },
            }
          : {
              task,
              ...DEFAULT_DETAIL_STATE,
              sidebarDefaultCollapsed,
            });
        if (!cachedState && !sidebarDefaultCollapsed) {
          setActivePanel("overview");
        }

        if (task.branchName && !task.prUrl) {
          void (async () => {
            try {
              const prUrl = await fetchPrUrlWithPrompt(task, tc);
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
            } catch (error) {
              console.error("PR URL 자동 조회 실패:", error);
            }
          })();
        }

        void (async () => {
          try {
            const baseBranchName = task.baseBranch ?? "main";
            const foundTaskId = task.projectId ? await getTaskIdByProjectAndBranch(task.projectId, baseBranchName) : null;
            const baseBranchTaskId = foundTaskId !== task.id ? foundTaskId : null;
            const diffFiles = task.branchName && task.worktreePath ? await getGitDiffFiles(id) : [];
            const [claudeHooksStatus, geminiHooksStatus, codexHooksStatus, openCodeHooksStatus, aiSessions, projects, defaultSessionType, doneAlertDismissed] = await Promise.all([
              task.projectId ? getTaskHooksStatus(id) : Promise.resolve(null),
              task.projectId ? getTaskGeminiHooksStatus(id) : Promise.resolve(null),
              task.projectId ? getTaskCodexHooksStatus(id) : Promise.resolve(null),
              task.projectId ? getTaskOpenCodeHooksStatus(id) : Promise.resolve(null),
              task.projectId ? getTaskAiSessions(id) : Promise.resolve(EMPTY_AI_SESSIONS),
              getAllProjects(),
              getDefaultSessionType(),
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
                  projects,
                  defaultSessionType,
                  doneAlertDismissed,
                }
              : current);
          } catch (error) {
            console.error("Failed to load task detail supplemental data:", error);
          }
        })();
      } catch (error) {
        clearLoadingTimeout();
        console.error("Failed to load task detail:", error);
        if (!cancelled) {
          setState((current) => current === undefined ? null : current);
        }
      }
    })();

    return () => {
      cancelled = true;
      clearLoadingTimeout();
    };
  }, [cachedState, id, refreshSignal, tc]);

  const agentTagStyle = useMemo(
    () => (state?.task.agentType ? AGENT_TAG_STYLES[state.task.agentType] ?? "bg-tag-neutral-bg text-tag-neutral-text" : null),
    [state?.task.agentType],
  );

  useEscapeKey(() => {
    setActivePanel(null);
    requestActiveTerminalFocusAfterUiSettles();
  }, { enabled: activePanel !== null });

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

  function closeCreateTaskModal() {
    setIsCreateTaskModalOpen(false);
    setCreateTaskDefaults(null);
    requestActiveTerminalFocusAfterUiSettles();
  }

  function toggleChatView() {
    setMainView((current) => {
      const nextView = current === "chat" ? "terminal" : "chat";
      if (nextView === "chat") {
        setActivePanel(null);
      } else {
        requestActiveTerminalFocusAfterUiSettles();
      }
      return nextView;
    });
  }

  return (
    <div className="relative h-screen overflow-hidden bg-bg-page p-3">
      <aside className={`absolute bottom-3 left-3 top-3 z-40 flex w-12 flex-col items-center rounded-lg border border-border-default bg-bg-surface/95 p-1.5 shadow-sm ${needsMacDesktopHeaderOffset ? "pt-10" : ""}`}>
        <Link href="/" className="mb-2 rounded-md p-2 text-text-muted transition-colors hover:bg-bg-page hover:text-text-primary" title={t("backToBoard")}>
          <svg width="17" height="17" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <path d="M10 12L6 8L10 4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </Link>

        {([
          { panel: "overview", label: t("info"), path: "M8 2.5a5.5 5.5 0 1 0 0 11 5.5 5.5 0 0 0 0-11Zm0 4.75v3.25M8 5.5h.01" },
          { panel: "actions", label: t("actions"), path: "M3 4h10M3 8h10M3 12h10" },
          { panel: "hooks", label: t("hooksStatus"), path: "M5.5 3.5 3 6l2.5 2.5M10.5 3.5 13 6l-2.5 2.5M9.5 2.5l-3 11" },
        ] satisfies Array<{ panel: DetailPanel; label: string; path: string }>).map(({ panel, label, path }) => (
          <button
            key={panel}
            type="button"
            onClick={() => setActivePanel((current) => current === panel ? null : panel)}
            className={`mb-1 rounded-md p-2 transition-colors ${
              activePanel === panel
                ? "bg-brand-subtle text-text-brand"
                : "text-text-muted hover:bg-bg-page hover:text-text-primary"
            }`}
            title={label}
            aria-label={label}
          >
            <svg width="17" height="17" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d={path} />
            </svg>
          </button>
        ))}

        <button
          type="button"
          onClick={toggleChatView}
          className={`mb-1 rounded-md p-2 transition-colors ${
            mainView === "chat"
              ? "bg-brand-subtle text-text-brand"
              : "text-text-muted hover:bg-bg-page hover:text-text-primary"
          }`}
          title={t("aiSessions.title")}
          aria-label={t("aiSessions.title")}
        >
          <svg width="17" height="17" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M3 4.5h10v6H6l-3 2v-8Z" />
          </svg>
        </button>

        {state.task.prUrl ? (
          <a
            href={state.task.prUrl}
            target="_blank"
            rel="noopener noreferrer"
            className="mb-1 flex h-8 w-8 items-center justify-center rounded-md border border-tag-pr-text/30 bg-tag-pr-bg text-[10px] font-semibold text-tag-pr-text transition-opacity hover:opacity-80"
            title="PR"
            aria-label="PR"
          >
            PR
          </a>
        ) : null}

        <div className="mt-auto">
          <NotificationCenterButton ref={notificationCenterRef} buttonClassName="hover:bg-bg-page" panelClassName="left-0 right-auto" />
        </div>
      </aside>

      {activePanel ? (
        <section
          className={`absolute bottom-3 left-[4.5rem] top-3 z-30 w-[360px] max-w-[calc(100vw-5.5rem)] overflow-y-auto rounded-lg border border-border-default bg-bg-surface/95 p-3 shadow-lg ${needsMacDesktopHeaderOffset ? "pt-10" : ""}`}
        >
          <div className="mb-3 flex items-center justify-between border-b border-border-subtle pb-2">
            <h2 className="text-xs font-semibold uppercase text-text-muted">
              {activePanel === "overview" && t("info")}
              {activePanel === "actions" && t("actions")}
              {activePanel === "hooks" && t("hooksStatus")}
            </h2>
            <button
              type="button"
              onClick={() => {
                setActivePanel(null);
                requestActiveTerminalFocusAfterUiSettles();
              }}
              className="rounded-md p-1 text-text-muted transition-colors hover:bg-bg-page hover:text-text-primary"
              aria-label={tc("close")}
            >
              ×
            </button>
          </div>

          {activePanel === "overview" ? (
            <div className="space-y-3">
              <TaskDetailTitleCard task={state.task} taskId={state.task.id} />
              <TaskDetailInfoCard
                task={state.task}
                agentTagStyle={agentTagStyle}
                baseBranchTaskId={state.baseBranchTaskId}
                diffFileCount={state.diffFiles.length}
              />
            </div>
          ) : null}

          {activePanel === "actions" ? (
            <div className="rounded-lg border border-border-default bg-bg-surface p-4">
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
                      <button type="submit" className="rounded-md border border-border-default bg-bg-page px-3 py-1.5 text-xs text-text-secondary transition-colors hover:border-brand-primary hover:text-text-brand">
                        {t(transition.labelKey)}
                      </button>
                    </form>
                  )
                ))}
              </div>
              <div className="mt-3 border-t border-border-subtle pt-3">
                <DeleteTaskButton deleteAction={handleDelete} />
              </div>
            </div>
          ) : null}

          {activePanel === "hooks" ? (
            <HooksStatusCard
              taskId={state.task.id}
              initialClaudeStatus={state.claudeHooksStatus}
              initialGeminiStatus={state.geminiHooksStatus}
              initialCodexStatus={state.codexHooksStatus}
              initialOpenCodeStatus={state.openCodeHooksStatus}
              isRemote={!!state.task.sshHost}
            />
          ) : null}

        </section>
      ) : null}

      <main className="ml-14 flex h-full min-w-0 flex-col">
        {mainView === "chat" ? (
          <InlineAiChatView taskId={state.task.id} data={state.aiSessions} />
        ) : hasTerminal ? (
          <div className="flex-1 flex flex-col min-h-0 rounded-lg overflow-hidden shadow-md transition-all duration-200 ease-out">
            <div className="bg-terminal-chrome flex items-center gap-2 px-4 py-2.5 shrink-0">
              <span className="text-xs text-terminal-text font-mono truncate">{state.task.sessionName ?? t("terminal")}</span>
              <div className="ml-auto">
                <NotificationCenterButton ref={notificationCenterRef} buttonClassName="text-terminal-text hover:text-white hover:bg-white/10" panelClassName="mt-3" />
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

      <CreateTaskModal
        isOpen={isCreateTaskModalOpen}
        onClose={closeCreateTaskModal}
        sshHosts={[]}
        projects={state.projects}
        defaultProjectId={createTaskDefaults?.projectId ?? state.task.projectId ?? ""}
        defaultBaseBranch={createTaskDefaults?.baseBranch}
        defaultSessionType={state.defaultSessionType}
      />
    </div>
  );
}
