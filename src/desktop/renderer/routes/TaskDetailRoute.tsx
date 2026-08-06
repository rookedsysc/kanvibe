import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from "react";
import { HugeiconsIcon } from "@hugeicons/react";
import {
  Chatting01Icon,
  InformationCircleIcon,
} from "@hugeicons/core-free-icons";
import { useTranslations } from "next-intl";
import { useParams } from "react-router-dom";
import ConnectTerminalForm from "@/components/ConnectTerminalForm";
import CreateTaskModal from "@/components/CreateTaskModal";
import DeleteTaskButton from "@/components/DeleteTaskButton";
import DoneStatusButton from "@/components/DoneStatusButton";
import HooksStatusCard from "@/components/HooksStatusCard";
import { AiProviderIcon } from "@/components/AiProviderIcon";
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
import { AiSessionMessageContent } from "@/desktop/renderer/components/AiSessionMessageContent";
import {
  useBoardCommands,
  useHasBoardShortcutBlocker,
  type BranchTodoDefaults,
} from "@/desktop/renderer/components/BoardCommandProvider";
import TerminalLoader from "@/desktop/renderer/components/TerminalLoader";
import TerminalTabBar from "@/desktop/renderer/components/TerminalTabBar";
import { useTerminalTabs } from "@/desktop/renderer/hooks/useTerminalTabs";
import type { TerminalTabShortcutCommand } from "@/desktop/shared/terminalTabs";
import { fetchPrUrlWithPrompt } from "@/desktop/renderer/utils/fetchPrUrlWithPrompt";
import {
  SHORTCUTS,
  TASK_DETAIL_DOCK_SHORTCUT_INDEXES,
  createTaskDetailDockShortcut,
  formatShortcutForDisplay,
  getCurrentShortcutPlatform,
  matchShortcutEvent,
  resolveTerminalTabShortcutEvent,
  matchTaskDetailDockShortcutEvent,
} from "@/desktop/renderer/utils/keyboardShortcut";
import { INITIAL_DESKTOP_LOAD_TIMEOUT_MS, logDesktopInitialLoadTimeout } from "@/desktop/renderer/utils/loadingTimeout";
import { rememberBoardFocusTask } from "@/desktop/renderer/utils/boardFocusTarget";
import { buildRouteCacheKey, readRouteCache, removeRouteCache, writeRouteCache } from "@/desktop/renderer/utils/routeCache";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";
import { requestActiveTerminalFocusAfterUiSettles } from "@/desktop/renderer/utils/terminalFocus";
import { SessionType, TaskStatus } from "@/entities/KanbanTask";
import { useEscapeKey } from "@/hooks/useEscapeKey";
import type {
  AiMessageRole,
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
const INLINE_CHAT_SESSION_LIMIT = 20;

const AGENT_TAG_STYLES: Record<string, string> = {
  claude: "bg-tag-claude-bg text-tag-claude-text",
  gemini: "bg-tag-gemini-bg text-tag-gemini-text",
  codex: "bg-tag-codex-bg text-tag-codex-text",
};

type DetailPanel = "overview" | "status";
type MainView = "terminal" | "chat";
type TaskDetailDockItem = {
  id: string;
  label: string;
  isActive: boolean;
  renderIcon: () => ReactNode;
  onActivate: () => void;
  href?: string;
};

const PR_DOCK_INSERT_INDEX = 3;

function PullRequestIcon() {
  return (
    <svg
      width="17"
      height="17"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      data-testid="task-detail-pr-icon"
    >
      <circle cx="6" cy="18" r="3" />
      <circle cx="18" cy="6" r="3" />
      <path d="M6 15V6" />
      <path d="M18 9v1.5A5.5 5.5 0 0 1 12.5 16H9" />
      <path d="m12 13-3 3 3 3" />
    </svg>
  );
}

function AntennaSignalIcon({ testId }: { testId?: string }) {
  return (
    <svg
      width="17"
      height="17"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.7"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      data-testid={testId}
      data-icon-name="AntennaSignalIcon"
    >
      <path d="M12 19v-7" />
      <path d="m9 22 3-3 3 3" />
      <circle cx="12" cy="10" r="1.6" fill="currentColor" stroke="none" />
      <path d="M8.7 13.1a5 5 0 0 1 0-6.2" />
      <path d="M15.3 6.9a5 5 0 0 1 0 6.2" />
      <path d="M5.8 15.8a9 9 0 0 1 0-11.6" />
      <path d="M18.2 4.2a9 9 0 0 1 0 11.6" />
    </svg>
  );
}

interface TaskDetailState {
  task: NonNullable<Awaited<ReturnType<typeof getTaskById>>>;
  baseBranchTaskId: string | null;
  diffFiles: Awaited<ReturnType<typeof getGitDiffFiles>>;
  claudeHooksStatus: Awaited<ReturnType<typeof getTaskHooksStatus>>;
  geminiHooksStatus: Awaited<ReturnType<typeof getTaskGeminiHooksStatus>>;
  codexHooksStatus: Awaited<ReturnType<typeof getTaskCodexHooksStatus>>;
  openCodeHooksStatus: Awaited<ReturnType<typeof getTaskOpenCodeHooksStatus>>;
  projects: Awaited<ReturnType<typeof getAllProjects>>;
  sidebarDefaultCollapsed: boolean;
  defaultSessionType: Awaited<ReturnType<typeof getDefaultSessionType>>;
  doneAlertDismissed: boolean;
}

interface TaskDetailRouteCache extends TaskDetailState {
  defaultPanelDismissed?: boolean;
}

interface NormalizedTaskDetailRouteCache {
  state: TaskDetailState;
  defaultPanelDismissed: boolean;
}

const DEFAULT_DETAIL_STATE: Omit<TaskDetailState, "task"> = {
  baseBranchTaskId: null,
  diffFiles: [],
  claudeHooksStatus: null,
  geminiHooksStatus: null,
  codexHooksStatus: null,
  openCodeHooksStatus: null,
  projects: [],
  sidebarDefaultCollapsed: false,
  defaultSessionType: SessionType.TMUX,
  doneAlertDismissed: false,
};

function getBranchTodoDefaultsFromTask(task: TaskDetailState["task"] | null): BranchTodoDefaults | null {
  if (!task?.projectId) {
    return null;
  }

  return {
    projectId: task.projectId,
    baseBranch: task.branchName || task.baseBranch || "",
  };
}

function getTaskDetailRouteCacheKey(taskId: string) {
  return buildRouteCacheKey("task-detail", taskId);
}

function normalizeCachedTaskDetailRouteCache(cachedRoute: TaskDetailRouteCache | null): NormalizedTaskDetailRouteCache | null {
  if (!cachedRoute) {
    return null;
  }

  const routeState = { ...cachedRoute } as TaskDetailRouteCache & {
    sidebarHintDismissed?: boolean;
    aiSessions?: unknown;
  };
  const defaultPanelDismissed = routeState.defaultPanelDismissed === true;
  delete routeState.sidebarHintDismissed;
  delete routeState.defaultPanelDismissed;
  delete routeState.aiSessions;
  return {
    state: {
      ...DEFAULT_DETAIL_STATE,
      ...routeState,
      sidebarDefaultCollapsed: routeState.sidebarDefaultCollapsed ?? DEFAULT_DETAIL_STATE.sidebarDefaultCollapsed,
    },
    defaultPanelDismissed,
  };
}

const AI_SESSION_PROVIDER_STYLES: Record<AggregatedAiSession["provider"], string> = {
  claude: "border-tag-claude-text/30 bg-tag-claude-bg text-tag-claude-text",
  gemini: "border-tag-gemini-text/30 bg-tag-gemini-bg text-tag-gemini-text",
  codex: "border-tag-codex-text/30 bg-tag-codex-bg text-tag-codex-text",
  opencode: "border-tag-neutral-text/30 bg-tag-neutral-bg text-tag-neutral-text",
};

const AI_SESSION_PROVIDER_ICON_STYLES: Record<AggregatedAiSession["provider"], string> = {
  claude: "border-tag-claude-text/40 bg-tag-claude-bg text-tag-claude-text",
  gemini: "border-tag-gemini-text/40 bg-tag-gemini-bg text-tag-gemini-text",
  codex: "border-tag-codex-text/40 bg-tag-codex-bg text-tag-codex-text",
  opencode: "border-tag-neutral-text/40 bg-tag-neutral-bg text-tag-neutral-text",
};

const AI_SESSION_PROVIDER_ORDER: AggregatedAiSession["provider"][] = ["claude", "opencode", "gemini", "codex"];

const AI_SESSION_PROVIDER_META: Record<AggregatedAiSession["provider"], { label: string }> = {
  claude: { label: "Claude" },
  opencode: { label: "OpenCode" },
  gemini: { label: "Gemini" },
  codex: { label: "Codex" },
};

const INLINE_CHAT_ROLE_FILTERS: AiMessageRole[] = [
  "user",
  "assistant",
  "system",
  "developer",
  "reasoning",
  "tool",
  "unknown",
];

function getAiSessionKey(session: AggregatedAiSession): string {
  return `${session.provider}:${session.id}:${session.sourceRef ?? ""}`;
}

function normalizeAiSessionSearchQuery(value: string): string | undefined {
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : undefined;
}

function InlineAiChatView({ taskId }: { taskId: string }) {
  const t = useTranslations("taskDetail");
  const detailErrorMessage = t("aiSessions.detailError");
  const [history, setHistory] = useState<AggregatedAiSessionsResult | null>(null);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [isHistoryLoading, setIsHistoryLoading] = useState(true);
  const [selectedSessionKey, setSelectedSessionKey] = useState<string | null>(null);
  const [detail, setDetail] = useState<AggregatedAiSessionDetail | null>(null);
  const [detailError, setDetailError] = useState<{ sessionId: string; message: string } | null>(null);
  const [isOlderMessagesLoading, setIsOlderMessagesLoading] = useState(false);
  const [activeRoles, setActiveRoles] = useState<AiMessageRole[]>([]);
  const [activeProviders, setActiveProviders] = useState<AggregatedAiSession["provider"][]>([]);
  const [searchQuery, setSearchQuery] = useState("");
  const [appliedSearchQuery, setAppliedSearchQuery] = useState<string | undefined>(undefined);
  const loadedTaskIdRef = useRef<string | null>(null);
  const latestMessageAnchorRef = useRef<HTMLDivElement | null>(null);
  const shouldScrollToLatestMessageRef = useRef(false);
  const historyRequestIdRef = useRef(0);
  const olderMessagesRequestIdRef = useRef(0);

  const selectedSession = useMemo(
    () => history?.sessions.find((session) => getAiSessionKey(session) === selectedSessionKey) ?? null,
    [history?.sessions, selectedSessionKey],
  );

  const providerCounts = useMemo(() => {
    const counts: Record<AggregatedAiSession["provider"], number> = {
      claude: 0,
      codex: 0,
      opencode: 0,
      gemini: 0,
    };
    for (const session of history?.sessions ?? []) {
      counts[session.provider] += 1;
    }
    return counts;
  }, [history?.sessions]);

  const filteredSessions = useMemo(() => {
    if (!history) return [];
    if (activeProviders.length === 0) return history.sessions;
    const activeProviderSet = new Set(activeProviders);
    return history.sessions.filter((session) => activeProviderSet.has(session.provider));
  }, [activeProviders, history]);

  const loadHistory = useCallback(async (queryOverride?: string, options?: { cursor?: string | null; append?: boolean }) => {
    const normalizedQuery = normalizeAiSessionSearchQuery(queryOverride ?? searchQuery);
    const isAppending = options?.append === true;
    const requestId = historyRequestIdRef.current + 1;
    historyRequestIdRef.current = requestId;
    setIsHistoryLoading(true);
    setHistoryError(null);
    try {
      const result = await getTaskAiSessions(
        taskId,
        normalizedQuery,
        options?.cursor ?? null,
        INLINE_CHAT_SESSION_LIMIT,
      );
      if (historyRequestIdRef.current !== requestId) {
        return;
      }

      setHistory((current) => {
        if (!isAppending || !current) {
          return result;
        }

        const seenKeys = new Set(current.sessions.map(getAiSessionKey));
        const mergedSessions = [...current.sessions];
        for (const session of result.sessions) {
          const sessionKey = getAiSessionKey(session);
          if (!seenKeys.has(sessionKey)) {
            seenKeys.add(sessionKey);
            mergedSessions.push(session);
          }
        }

        return {
          ...result,
          sessions: mergedSessions,
          sources: result.sources.length > 0 ? result.sources : current.sources,
        };
      });
      setAppliedSearchQuery(normalizedQuery);
      if (!isAppending) {
        olderMessagesRequestIdRef.current += 1;
        setSelectedSessionKey(null);
        setDetail(null);
        setDetailError(null);
        setActiveRoles([]);
      }
    } catch {
      if (historyRequestIdRef.current === requestId) {
        setHistoryError(detailErrorMessage);
      }
    } finally {
      if (historyRequestIdRef.current === requestId) {
        setIsHistoryLoading(false);
      }
    }
  }, [detailErrorMessage, searchQuery, taskId]);

  useEffect(() => {
    loadedTaskIdRef.current = null;
    historyRequestIdRef.current += 1;
    olderMessagesRequestIdRef.current += 1;
    setHistory(null);
    setHistoryError(null);
    setIsHistoryLoading(true);
    setSelectedSessionKey(null);
    setDetail(null);
    setDetailError(null);
    setIsOlderMessagesLoading(false);
    setActiveRoles([]);
    setActiveProviders([]);
    setSearchQuery("");
    setAppliedSearchQuery(undefined);
  }, [taskId]);

  useEffect(() => {
    if (loadedTaskIdRef.current === taskId) {
      return;
    }

    loadedTaskIdRef.current = taskId;
    void loadHistory("");
  }, [loadHistory, taskId]);

  useEffect(() => {
    if (!selectedSessionKey) return;
    const selectedSessionStillVisible = filteredSessions.some((session) => getAiSessionKey(session) === selectedSessionKey);
    if (selectedSessionStillVisible) return;

    setSelectedSessionKey(null);
    setDetail(null);
    setDetailError(null);
    olderMessagesRequestIdRef.current += 1;
    setActiveRoles([]);
  }, [filteredSessions, selectedSessionKey]);

  useEffect(() => {
    if (!selectedSession) {
      return;
    }

    let cancelled = false;
    olderMessagesRequestIdRef.current += 1;
    setDetail(null);
    setDetailError(null);
    setIsOlderMessagesLoading(false);
    shouldScrollToLatestMessageRef.current = true;

    getTaskAiSessionDetail(
      taskId,
      selectedSession.provider,
      selectedSession.id,
      selectedSession.sourceRef ?? null,
      null,
      INLINE_CHAT_DETAIL_LIMIT,
      appliedSearchQuery,
      activeRoles.length > 0 ? activeRoles : undefined,
    ).then((result) => {
      if (cancelled) return;
      if (!result) {
        setDetailError({ sessionId: selectedSession.id, message: detailErrorMessage });
        return;
      }

      setDetail(result);
      setDetailError(null);
    }).catch(() => {
      if (cancelled) return;
      setDetailError({ sessionId: selectedSession.id, message: detailErrorMessage });
    });

    return () => {
      cancelled = true;
    };
  }, [activeRoles, appliedSearchQuery, detailErrorMessage, selectedSession, taskId]);

  const selectedSessionId = selectedSession?.id ?? null;
  const messages = useMemo(() => {
    if (!selectedSessionId || detail?.sessionId !== selectedSessionId) {
      return [];
    }

    return detail.messages;
  }, [detail?.messages, detail?.sessionId, selectedSessionId]);
  const displayedMessages = useMemo(() => [...messages].reverse(), [messages]);
  const error = selectedSession && detailError?.sessionId === selectedSession.id ? detailError.message : null;
  const isDetailLoading = Boolean(selectedSession && detail?.sessionId !== selectedSession.id && !error);

  useEffect(() => {
    if (!shouldScrollToLatestMessageRef.current || isDetailLoading || !selectedSession || displayedMessages.length === 0) {
      return;
    }

    latestMessageAnchorRef.current?.scrollIntoView?.({ block: "end" });
    shouldScrollToLatestMessageRef.current = false;
  }, [displayedMessages.length, isDetailLoading, selectedSession]);

  function toggleRole(role: AiMessageRole) {
    olderMessagesRequestIdRef.current += 1;
    setActiveRoles((current) => (
      current.includes(role)
        ? current.filter((candidate) => candidate !== role)
        : [...current, role]
    ));
  }

  function toggleProvider(provider: AggregatedAiSession["provider"]) {
    setActiveProviders((current) => (
      current.includes(provider)
        ? current.filter((candidate) => candidate !== provider)
        : [...current, provider]
    ));
  }

  function handleSearchSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void loadHistory(searchQuery);
  }

  async function handleLoadMoreSessions() {
    if (!history?.nextCursor || isHistoryLoading) {
      return;
    }

    await loadHistory(appliedSearchQuery ?? "", { cursor: history.nextCursor, append: true });
  }

  async function handleLoadOlderMessages() {
    if (!selectedSession || !detail?.nextCursor || isOlderMessagesLoading) {
      return;
    }

    const requestId = olderMessagesRequestIdRef.current + 1;
    olderMessagesRequestIdRef.current = requestId;
    const requestedSession = selectedSession;
    const requestedSearchQuery = appliedSearchQuery;
    const requestedRoles = activeRoles.length > 0 ? activeRoles : undefined;
    setIsOlderMessagesLoading(true);
    setDetailError(null);
    try {
      const result = await getTaskAiSessionDetail(
        taskId,
        requestedSession.provider,
        requestedSession.id,
        requestedSession.sourceRef ?? null,
        detail.nextCursor,
        INLINE_CHAT_DETAIL_LIMIT,
        requestedSearchQuery,
        requestedRoles,
      );

      if (olderMessagesRequestIdRef.current !== requestId) {
        return;
      }

      if (!result) {
        setDetailError({ sessionId: requestedSession.id, message: t("aiSessions.detailError") });
        return;
      }

      setDetail((current) => {
        if (!current || current.sessionId !== result.sessionId) {
          return result;
        }

        return {
          ...result,
          messages: [...current.messages, ...result.messages],
        };
      });
    } catch {
      if (olderMessagesRequestIdRef.current === requestId) {
        setDetailError({ sessionId: requestedSession.id, message: t("aiSessions.detailError") });
      }
    } finally {
      if (olderMessagesRequestIdRef.current === requestId) {
        setIsOlderMessagesLoading(false);
      }
    }
  }

  const historyCountLabel = history
    ? activeProviders.length > 0
      ? `${filteredSessions.length}/${history.sessions.length}`
      : `${history.sessions.length}`
    : t("aiSessions.noPreview");

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
            {selectedSession ? AI_SESSION_PROVIDER_META[selectedSession.provider].label : historyCountLabel}
          </p>
        </div>
        {selectedSession ? <AiSessionProviderBadge provider={selectedSession.provider} /> : null}
        <form onSubmit={handleSearchSubmit} className="flex w-[320px] items-center gap-2">
          <label htmlFor="ai-session-search" className="sr-only">
            {t("aiSessions.searchLabel")}
          </label>
          <input
            id="ai-session-search"
            value={searchQuery}
            onChange={(event) => setSearchQuery(event.target.value)}
            placeholder={t("aiSessions.searchPlaceholder")}
            className="min-w-0 flex-1 rounded-md border border-terminal-text/20 bg-black/10 px-3 py-1.5 text-[11px] text-terminal-text placeholder:text-terminal-text/45 outline-none transition-colors focus:border-terminal-text/50"
          />
          <button
            type="submit"
            disabled={isHistoryLoading}
            className="rounded-md border border-terminal-text/20 px-3 py-1.5 text-[11px] font-semibold text-terminal-text transition-colors hover:bg-white/10 disabled:opacity-50"
          >
            {t("aiSessions.search")}
          </button>
        </form>
      </div>

      <div className="flex min-h-0 flex-1">
        <div className="flex w-[340px] shrink-0 border-r border-border-default bg-bg-surface/45">
          <div className="flex w-14 shrink-0 flex-col items-center gap-2 border-r border-border-default bg-bg-page/70 px-2 py-3">
            {AI_SESSION_PROVIDER_ORDER.map((provider) => {
              const isActive = activeProviders.includes(provider);
              const count = providerCounts[provider];
              return (
                <button
                  key={provider}
                  type="button"
                  data-testid={`ai-session-filter-${provider}`}
                  aria-label={t("aiSessions.providerFilterLabel", { provider: AI_SESSION_PROVIDER_META[provider].label })}
                  aria-pressed={isActive}
                  title={t("aiSessions.providerFilterLabel", { provider: AI_SESSION_PROVIDER_META[provider].label })}
                  onClick={() => toggleProvider(provider)}
                  className={`relative flex size-10 items-center justify-center rounded-full border text-[11px] font-bold shadow-sm transition-all ${
                    isActive
                      ? `${AI_SESSION_PROVIDER_ICON_STYLES[provider]} ring-2 ring-brand-primary/45 ring-offset-2 ring-offset-bg-page`
                      : `${AI_SESSION_PROVIDER_ICON_STYLES[provider]} opacity-65 hover:opacity-100`
                  }`}
                >
                  <AiProviderIcon
                    provider={provider}
                    className="inline-flex h-[18px] w-[18px] items-center justify-center"
                    imageClassName="block h-[18px] w-[18px] object-contain"
                    size={18}
                  />
                  {history ? (
                    <span className="absolute -bottom-1 -right-1 flex min-w-4 items-center justify-center rounded-full border border-bg-page bg-bg-surface px-1 text-[9px] leading-4 text-text-muted">
                      {count}
                    </span>
                  ) : null}
                </button>
              );
            })}
          </div>

          <div className="min-w-0 flex-1 overflow-y-auto p-3">
            {!history && !isHistoryLoading && !historyError ? <InlineAiChatEmpty compact text={t("aiSessions.noPreview")} /> : null}
            {historyError ? <InlineAiChatEmpty compact text={historyError} /> : null}
            {isHistoryLoading ? <InlineAiChatEmpty compact text={t("aiSessions.loadingDetail")} /> : null}
            {history ? (
              filteredSessions.length > 0 || history.nextCursor ? (
                <div data-testid="ai-session-list" className="space-y-2">
                  {filteredSessions.length > 0 ? (
                    filteredSessions.map((session) => {
                      const sessionKey = getAiSessionKey(session);
                      const isSelected = selectedSessionKey === sessionKey;
                      return (
                        <button
                          key={sessionKey}
                          type="button"
                          onClick={() => {
                            shouldScrollToLatestMessageRef.current = true;
                            setSelectedSessionKey(sessionKey);
                            setActiveRoles([]);
                          }}
                          className={`w-full rounded-xl border p-3 text-left transition-colors ${
                            isSelected
                              ? "border-brand-primary bg-brand-subtle"
                              : "border-border-default bg-bg-page hover:border-brand-primary/70"
                          }`}
                          aria-label={`${AI_SESSION_PROVIDER_META[session.provider].label} ${session.title ?? session.firstUserPrompt ?? session.id}`}
                        >
                          <div className="mb-2 flex items-start gap-2">
                            <AiSessionProviderIcon provider={session.provider} />
                            <span className="min-w-0 flex-1 truncate text-xs font-semibold text-text-primary">
                              {session.title ?? session.firstUserPrompt ?? session.id}
                            </span>
                          </div>
                          {session.firstUserPrompt ? (
                            <p className="line-clamp-2 text-[11px] leading-4 text-text-muted">{session.firstUserPrompt}</p>
                          ) : null}
                          <p className="mt-2 text-[10px] text-text-muted">{session.messageCount}</p>
                        </button>
                      );
                    })
                  ) : (
                    <InlineAiChatEmpty compact text={history.isRemote ? t("aiSessions.remoteBadge") : t("aiSessions.noPreview")} />
                  )}
                  {history.nextCursor ? (
                    <button
                      type="button"
                      onClick={() => void handleLoadMoreSessions()}
                      disabled={isHistoryLoading}
                      className="w-full rounded-lg border border-dashed border-border-default px-3 py-2 text-[11px] font-semibold text-text-muted transition-colors hover:border-brand-primary hover:text-text-brand disabled:opacity-50"
                    >
                      {isHistoryLoading ? t("aiSessions.loadingDetail") : t("aiSessions.loadMoreSessions")}
                    </button>
                  ) : null}
                </div>
              ) : <InlineAiChatEmpty compact text={history.isRemote ? t("aiSessions.remoteBadge") : t("aiSessions.noPreview")} />
            ) : null}
          </div>
        </div>

        <div className="flex min-w-0 flex-1 flex-col">
          {selectedSession ? (
            <div className="flex shrink-0 flex-wrap gap-2 border-b border-border-default px-4 py-2">
              {INLINE_CHAT_ROLE_FILTERS.map((role) => {
                const isActive = activeRoles.includes(role);
                return (
                  <button
                    key={role}
                    type="button"
                    aria-label={t(`aiSessions.roles.${role}`)}
                    aria-pressed={isActive}
                    onClick={() => toggleRole(role)}
                    className={`rounded-full border px-2.5 py-1 text-[11px] font-semibold transition-colors ${
                      isActive
                        ? "border-brand-primary bg-brand-subtle text-text-brand"
                        : "border-border-default bg-bg-surface text-text-muted hover:border-brand-primary/70 hover:text-text-primary"
                    }`}
                  >
                    {t(`aiSessions.roles.${role}`)}
                  </button>
                );
              })}
            </div>
          ) : null}

          <div data-testid="ai-session-messages" className="flex-1 space-y-4 overflow-y-auto px-5 py-5">
            {!history && !isHistoryLoading && !historyError ? <InlineAiChatEmpty text={t("aiSessions.noPreview")} /> : null}
            {historyError ? <InlineAiChatEmpty text={historyError} /> : null}
            {history && !selectedSession ? <InlineAiChatEmpty text={t("aiSessions.selectSession")} /> : null}
            {selectedSession && isDetailLoading ? <InlineAiChatEmpty text={t("aiSessions.loadingDetail")} /> : null}
            {selectedSession && !isDetailLoading && error ? <InlineAiChatEmpty text={error} /> : null}
            {selectedSession && !isDetailLoading && !error && messages.length === 0 ? <InlineAiChatEmpty text={t("aiSessions.noPreview")} /> : null}
            {selectedSession && !isDetailLoading && !error && detail?.sessionId === selectedSession.id && detail.nextCursor ? (
              <div className="flex justify-center">
                <button
                  type="button"
                  onClick={() => void handleLoadOlderMessages()}
                  disabled={isOlderMessagesLoading}
                  className="rounded-full border border-border-default bg-bg-surface px-4 py-2 text-xs font-semibold text-text-muted transition-colors hover:border-brand-primary hover:text-text-brand disabled:opacity-50"
                >
                  {isOlderMessagesLoading ? t("aiSessions.loadingDetail") : t("aiSessions.loadOlderMessages")}
                </button>
              </div>
            ) : null}
            {selectedSession && !isDetailLoading && !error && displayedMessages.map((message, index) => (
              <InlineAiChatMessage
                key={`${message.role}-${message.timestamp ?? index}-${index}`}
                message={message}
                provider={selectedSession.provider}
              />
            ))}
            <div ref={latestMessageAnchorRef} aria-hidden="true" />
          </div>
        </div>
      </div>
    </div>
  );
}
function AiSessionProviderIcon({ provider, testId = true }: { provider: AggregatedAiSession["provider"]; testId?: boolean }) {
  return (
    <AiProviderIcon
      provider={provider}
      testId={testId ? `ai-session-provider-${provider}` : undefined}
      className={`flex size-6 shrink-0 items-center justify-center rounded-full border ${AI_SESSION_PROVIDER_ICON_STYLES[provider]}`}
      imageClassName="block h-4 w-4 object-contain"
      size={16}
    />
  );
}

function AiSessionProviderBadge({ provider }: { provider: AggregatedAiSession["provider"] }) {
  return (
    <span
      data-testid={`ai-session-provider-${provider}`}
      className={`rounded border px-2 py-0.5 text-[10px] font-semibold ${AI_SESSION_PROVIDER_STYLES[provider]}`}
    >
      {AI_SESSION_PROVIDER_META[provider].label}
    </span>
  );
}

function InlineAiChatEmpty({ text, compact = false }: { text: string; compact?: boolean }) {
  return (
    <div className={`flex ${compact ? "py-6" : "h-full"} items-center justify-center`}>
      <p className="rounded-md border border-border-default bg-bg-surface px-4 py-3 text-sm text-text-muted">
        {text}
      </p>
    </div>
  );
}

function InlineAiChatMessage({ message, provider }: { message: AggregatedAiMessage; provider: AggregatedAiSession["provider"] }) {
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
        <div className={`mb-2 flex items-center gap-2 text-[11px] font-semibold ${isUserMessage ? "text-white/75" : "text-text-muted"}`}>
          <AiSessionProviderIcon provider={provider} testId={false} />
          <span>{message.role}</span>
        </div>
        <AiSessionMessageContent text={displayedText} isUserMessage={isUserMessage} />
      </div>
    </div>
  );
}

export default function TaskDetailRoute() {
  const { id = "" } = useParams();
  const router = useRouter();
  const boardCommands = useBoardCommands();
  const hasShortcutBlocker = useHasBoardShortcutBlocker();
  const t = useTranslations("taskDetail");
  const tc = useTranslations("common");
  const refreshSignal = useRefreshSignal(["all", "task-detail"]);
  const cachedRoute = useMemo(
    () => (id ? normalizeCachedTaskDetailRouteCache(readRouteCache<TaskDetailRouteCache>(getTaskDetailRouteCacheKey(id))) : null),
    [id],
  );
  const cachedState = cachedRoute?.state ?? null;
  const [state, setState] = useState<TaskDetailState | null | undefined>(cachedState ?? undefined);
  const [isCreateTaskModalOpen, setIsCreateTaskModalOpen] = useState(false);
  const [createTaskDefaults, setCreateTaskDefaults] = useState<BranchTodoDefaults | null>(null);
  const currentTaskRef = useRef<TaskDetailState["task"] | null>(cachedState?.task ?? null);
  const needsMacDesktopHeaderOffset = useMemo(() => {
    const isDesktopApp = window.kanvibeDesktop?.isDesktop === true;
    const isMacDesktop = navigator.userAgent.includes("Mac") || navigator.platform.toLowerCase().includes("mac");
    return isDesktopApp && isMacDesktop;
  }, []);
  const [defaultPanelDismissed, setDefaultPanelDismissed] = useState(cachedRoute?.defaultPanelDismissed ?? false);
  const [resolvedSidebarDefaultCollapsed, setResolvedSidebarDefaultCollapsed] = useState<boolean | null>(null);
  const [activePanel, setActivePanel] = useState<DetailPanel | null>(null);
  const [mainView, setMainView] = useState<MainView>("terminal");
  const notificationCenterRef = useRef<NotificationCenterButtonHandle>(null);
  const currentTaskIdRef = useRef(id);
  const dockItemsRef = useRef<TaskDetailDockItem[]>([]);
  const commonTranslationsRef = useRef(tc);
  const hasTerminal = !!(state?.task.sessionType && state.task.sessionName);
  const terminalTabs = useTerminalTabs({
    taskId: id ?? "",
    sessionType: (state?.task.sessionType as SessionType | undefined) ?? null,
    isRemote: !!state?.task.sshHost,
    isVisible: hasTerminal && mainView === "terminal",
  });
  const shortcutPlatform = getCurrentShortcutPlatform();
  const statusPanelLabel = `${t("actions")} · ${t("hooksStatus")}`;
  currentTaskRef.current = state?.task ?? null;
  const shouldShowDefaultOverviewPanel = !!state
    && state !== null
    && resolvedSidebarDefaultCollapsed === false
    && !defaultPanelDismissed;
  const visiblePanel = activePanel ?? (shouldShowDefaultOverviewPanel ? "overview" : null);

  const markDefaultPanelDismissed = useCallback(() => {
    setDefaultPanelDismissed(true);
  }, []);

  /**
   * 패널은 터미널 위를 덮는 오버레이지만, 터미널 크롬의 탭 바까지 덮으면 탭을 누를 수 없다.
   * 터미널 화면일 때만 크롬 바 높이만큼 내려 시작해 탭 바를 항상 노출한다.
   */
  const panelTopOffsetClassName = hasTerminal && mainView === "terminal" ? "top-[4.25rem]" : "top-3";

  const closeDetailPanel = useCallback(() => {
    markDefaultPanelDismissed();
    setActivePanel(null);
    requestActiveTerminalFocusAfterUiSettles();
  }, [markDefaultPanelDismissed]);

  const toggleDetailPanel = useCallback((panel: DetailPanel) => {
    markDefaultPanelDismissed();
    setActivePanel(visiblePanel === panel ? null : panel);
  }, [markDefaultPanelDismissed, visiblePanel]);

  const toggleChatView = useCallback(() => {
    setMainView((current) => {
      const nextView = current === "chat" ? "terminal" : "chat";
      if (nextView === "chat") {
        markDefaultPanelDismissed();
        setActivePanel(null);
      } else {
        requestActiveTerminalFocusAfterUiSettles();
      }
      return nextView;
    });
  }, [markDefaultPanelDismissed]);

  const dockItems = useMemo<TaskDetailDockItem[]>(() => {
    if (!state) {
      return [];
    }

    const items: TaskDetailDockItem[] = [
      {
        id: "overview",
        label: t("info"),
        isActive: visiblePanel === "overview",
        renderIcon: () => (
          <HugeiconsIcon
            icon={InformationCircleIcon}
            size={17}
            strokeWidth={1.6}
            aria-hidden="true"
          />
        ),
        onActivate: () => toggleDetailPanel("overview"),
      },
      {
        id: "status",
        label: statusPanelLabel,
        isActive: visiblePanel === "status",
        renderIcon: () => <AntennaSignalIcon testId="task-status-panel-icon" />,
        onActivate: () => toggleDetailPanel("status"),
      },
      {
        id: "chat",
        label: t("aiSessions.inlineChat"),
        isActive: mainView === "chat",
        renderIcon: () => (
          <HugeiconsIcon
            icon={Chatting01Icon}
            size={17}
            strokeWidth={1.6}
            aria-hidden="true"
          />
        ),
        onActivate: toggleChatView,
      },
    ];

    if (state.task.prUrl) {
      items.splice(PR_DOCK_INSERT_INDEX, 0, {
        id: "pull-request",
        label: "PR",
        isActive: false,
        renderIcon: () => <PullRequestIcon />,
        onActivate: () => {
          window.open(state.task.prUrl!, "_blank", "noopener,noreferrer");
        },
        href: state.task.prUrl,
      });
    }

    return items;
  }, [mainView, state, statusPanelLabel, t, toggleChatView, toggleDetailPanel, visiblePanel]);

  // dock 항목을 렌더 시점에 ref로 넘겨두면 shortcut 구독을 다시 등록하지 않아도 항상 최신 dock을 실행한다.
  // 구독을 dock 변경마다 재등록하면 커밋과 effect flush 사이에 들어온 shortcut이 이전 dock(빈 배열)을 보고 무시된다.
  dockItemsRef.current = dockItems;

  const activateDockItem = useCallback((shortcutIndex: number) => {
    if (!Number.isInteger(shortcutIndex)) {
      return false;
    }

    const item = dockItemsRef.current[shortcutIndex - 1];
    if (!item) {
      return false;
    }

    item.onActivate();
    return true;
  }, []);

  useEffect(() => {
    if (id) {
      rememberBoardFocusTask(id);
    }
  }, [id]);

  useEffect(() => boardCommands.registerNotificationCenterHandler(() => {
    notificationCenterRef.current?.toggle();
  }), [boardCommands]);

  useEffect(() => boardCommands.registerBoardHandlers({
    toggleNotificationCenter() {
      notificationCenterRef.current?.toggle();
    },
    openProjectFilter() {},
    openCreateTaskModal(defaults) {
      setCreateTaskDefaults(defaults ?? getBranchTodoDefaultsFromTask(currentTaskRef.current));
      setIsCreateTaskModalOpen(true);
    },
  }), [boardCommands]);

  useEffect(() => {
    commonTranslationsRef.current = tc;
  }, [tc]);

  useEffect(() => {
    function consumeHistoryShortcut(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
    }

    function handlePriorityHistoryShortcut(event: KeyboardEvent) {
      if (matchShortcutEvent(event, SHORTCUTS.pageBack, shortcutPlatform)) {
        if (hasShortcutBlocker) {
          consumeHistoryShortcut(event);
          return;
        }

        consumeHistoryShortcut(event);
        router.back();
        return;
      }

      if (matchShortcutEvent(event, SHORTCUTS.pageForward, shortcutPlatform)) {
        if (hasShortcutBlocker) {
          consumeHistoryShortcut(event);
          return;
        }

        consumeHistoryShortcut(event);
        router.forward();
      }
    }

    window.addEventListener("keydown", handlePriorityHistoryShortcut, { capture: true });
    return () => {
      window.removeEventListener("keydown", handlePriorityHistoryShortcut, { capture: true });
    };
  }, [hasShortcutBlocker, router, shortcutPlatform]);

  useEffect(() => {
    function consumeDockShortcut(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
    }

    function handlePriorityDockShortcut(event: KeyboardEvent) {
      const shortcutIndex = matchTaskDetailDockShortcutEvent(event, shortcutPlatform);
      if (shortcutIndex === null) {
        return;
      }

      if (hasShortcutBlocker) {
        consumeDockShortcut(event);
        return;
      }

      const handled = activateDockItem(shortcutIndex);
      if (!handled) {
        return;
      }

      consumeDockShortcut(event);
    }

    window.addEventListener("keydown", handlePriorityDockShortcut, { capture: true });
    return () => {
      window.removeEventListener("keydown", handlePriorityDockShortcut, { capture: true });
    };
  }, [activateDockItem, hasShortcutBlocker, shortcutPlatform]);

  /** 마지막 탭을 닫으면 남길 화면이 없으므로 창까지 닫는다 */
  const closeTerminalTabOrWindow = useCallback(async (tabId: string) => {
    const remainingCount = await terminalTabs.closeTab(tabId);
    if (remainingCount === 0) {
      window.kanvibeDesktop?.closeCurrentWindow?.();
    }
  }, [terminalTabs]);

  const runTerminalTabCommand = useCallback((command: TerminalTabShortcutCommand) => {
    if (command.type === "close-window") {
      window.kanvibeDesktop?.closeCurrentWindow?.();
      return;
    }

    if (!hasTerminal || mainView !== "terminal") {
      return;
    }

    switch (command.type) {
      case "new-tab":
        void terminalTabs.createTab();
        return;
      case "close-tab": {
        const activeTabId = terminalTabs.activeTab?.id;
        if (activeTabId) {
          void closeTerminalTabOrWindow(activeTabId);
        }
        return;
      }
      case "previous-tab":
        void terminalTabs.selectRelativeTab(-1);
        return;
      case "next-tab":
        void terminalTabs.selectRelativeTab(1);
        return;
      case "go-to-tab":
        void terminalTabs.selectTabByPosition(command.position - 1);
    }
  }, [closeTerminalTabOrWindow, hasTerminal, mainView, terminalTabs]);

  useEffect(() => (
    window.kanvibeDesktop?.onTerminalTabShortcut?.(runTerminalTabCommand) ?? undefined
  ), [runTerminalTabCommand]);

  /**
   * main의 `before-input-event`가 놓친 입력을 렌더러가 받는 두 번째 경로.
   * 여기서 이벤트를 소비해야 xterm이 같은 키를 셸로 흘려보내지 않는다.
   */
  useEffect(() => {
    function handleTerminalTabShortcut(event: KeyboardEvent) {
      if (hasShortcutBlocker) {
        return;
      }

      const command = resolveTerminalTabShortcutEvent(event, shortcutPlatform, true);
      if (!command) {
        return;
      }

      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
      runTerminalTabCommand(command);
    }

    window.addEventListener("keydown", handleTerminalTabShortcut, { capture: true });
    return () => {
      window.removeEventListener("keydown", handleTerminalTabShortcut, { capture: true });
    };
  }, [hasShortcutBlocker, runTerminalTabCommand, shortcutPlatform]);

  useEffect(() => (
    window.kanvibeDesktop?.onTaskDetailDockShortcut?.((shortcutIndex: number) => {
      if (hasShortcutBlocker) {
        return;
      }

      activateDockItem(shortcutIndex);
    }) ?? undefined
  ), [activateDockItem, hasShortcutBlocker]);

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
      const nextDefaultPanelDismissed = cachedRoute?.defaultPanelDismissed ?? false;
      setDefaultPanelDismissed(nextDefaultPanelDismissed);
      setResolvedSidebarDefaultCollapsed(null);
      setActivePanel(null);
      setMainView("terminal");
    });

    return () => {
      cancelled = true;
    };
  }, [cachedRoute, cachedState, id]);

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
      writeRouteCache<TaskDetailRouteCache>(cacheKey, {
        ...state,
        defaultPanelDismissed,
      });
    }
  }, [defaultPanelDismissed, id, state]);

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

        setResolvedSidebarDefaultCollapsed(sidebarDefaultCollapsed);
        setState((current) => current && current.task.id === task.id
          ? {
              ...current,
              sidebarDefaultCollapsed,
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

        if (task.branchName && !task.prUrl) {
          void (async () => {
            try {
              const prUrl = await fetchPrUrlWithPrompt(task, commonTranslationsRef.current);
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

        const applySupplementalState = (updates: Partial<Omit<TaskDetailState, "task">>) => {
          if (cancelled) {
            return;
          }

          setState((current) => current && current.task.id === task.id
            ? {
                ...current,
                ...updates,
              }
            : current);
        };

        void (async () => {
          try {
            const baseBranchName = task.baseBranch ?? "main";
            const [foundTaskId, diffFiles, projects, defaultSessionType, doneAlertDismissed] = await Promise.all([
              task.projectId ? getTaskIdByProjectAndBranch(task.projectId, baseBranchName) : Promise.resolve(null),
              task.branchName && task.worktreePath ? getGitDiffFiles(id) : Promise.resolve([]),
              getAllProjects(),
              getDefaultSessionType(),
              getDoneAlertDismissed(),
            ]);
            const baseBranchTaskId = foundTaskId !== task.id ? foundTaskId : null;

            applySupplementalState({
              baseBranchTaskId,
              diffFiles,
              projects,
              defaultSessionType,
              doneAlertDismissed,
            });
          } catch (error) {
            console.error("Failed to load task detail reference data:", error);
          }
        })();

        void (async () => {
          try {
            const [claudeHooksStatus, geminiHooksStatus, codexHooksStatus, openCodeHooksStatus] = await Promise.all([
              task.projectId ? getTaskHooksStatus(id) : Promise.resolve(null),
              task.projectId ? getTaskGeminiHooksStatus(id) : Promise.resolve(null),
              task.projectId ? getTaskCodexHooksStatus(id) : Promise.resolve(null),
              task.projectId ? getTaskOpenCodeHooksStatus(id) : Promise.resolve(null),
            ]);

            applySupplementalState({
              claudeHooksStatus,
              geminiHooksStatus,
              codexHooksStatus,
              openCodeHooksStatus,
            });
          } catch (error) {
            console.error("Failed to load task hook statuses:", error);
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
  }, [id, refreshSignal]);

  const agentTagStyle = useMemo(
    () => (state?.task.agentType ? AGENT_TAG_STYLES[state.task.agentType] ?? "bg-tag-neutral-bg text-tag-neutral-text" : null),
    [state?.task.agentType],
  );

  useEscapeKey(() => {
    closeDetailPanel();
  }, { enabled: visiblePanel !== null });

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

  return (
    <div className="relative h-screen overflow-hidden bg-bg-page p-3">
      <aside className={`absolute bottom-3 left-3 top-3 z-40 flex w-12 flex-col items-center rounded-lg border border-border-default bg-bg-surface/95 p-1.5 shadow-sm ${needsMacDesktopHeaderOffset ? "pt-10" : ""}`}>
        <Link href="/" className="mb-2 rounded-md p-2 text-text-muted transition-colors hover:bg-bg-page hover:text-text-primary" title={t("backToBoard")}>
          <svg width="17" height="17" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <path d="M10 12L6 8L10 4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </Link>

        {dockItems.map((item, itemIndex) => {
          const shortcutIndex = TASK_DETAIL_DOCK_SHORTCUT_INDEXES[itemIndex];
          const shortcutText = shortcutIndex
            ? formatShortcutForDisplay(createTaskDetailDockShortcut(shortcutIndex), shortcutPlatform)
            : null;
          const title = shortcutText ? `${item.label} (${shortcutText})` : item.label;

          if (item.href) {
            return (
              <a
                key={item.id}
                href={item.href}
                target="_blank"
                rel="noopener noreferrer"
                className="mb-1 flex h-8 w-8 items-center justify-center rounded-md border border-tag-pr-text/30 bg-tag-pr-bg text-tag-pr-text transition-opacity hover:opacity-80"
                title={title}
                aria-label={item.label}
              >
                {item.renderIcon()}
              </a>
            );
          }

          return (
            <button
              key={item.id}
              type="button"
              onClick={item.onActivate}
              className={`mb-1 rounded-md p-2 transition-colors ${
                item.isActive
                  ? "bg-brand-subtle text-text-brand"
                  : "text-text-muted hover:bg-bg-page hover:text-text-primary"
              }`}
              title={title}
              aria-label={item.label}
            >
              {item.renderIcon()}
            </button>
          );
        })}

        <div className="mt-auto" />
      </aside>

      {visiblePanel ? (
        <section
          data-testid="task-detail-panel"
          className={`absolute bottom-3 left-[4.5rem] z-30 w-[360px] max-w-[calc(100vw-5.5rem)] overflow-y-auto rounded-lg border border-border-default bg-bg-surface/95 p-3 shadow-lg ${panelTopOffsetClassName} ${needsMacDesktopHeaderOffset ? "pt-10" : ""}`}
        >
          <div className="mb-3 flex items-center justify-between border-b border-border-subtle pb-2">
            <h2 className="text-xs font-semibold uppercase text-text-muted">
              {visiblePanel === "overview" && t("info")}
              {visiblePanel === "status" && statusPanelLabel}
            </h2>
            <button
              type="button"
              onClick={closeDetailPanel}
              className="rounded-md p-1 text-text-muted transition-colors hover:bg-bg-page hover:text-text-primary"
              aria-label={tc("close")}
            >
              ×
            </button>
          </div>

          {visiblePanel === "overview" ? (
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

          {visiblePanel === "status" ? (
            <div className="space-y-3">
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
              <HooksStatusCard
                taskId={state.task.id}
                initialClaudeStatus={state.claudeHooksStatus}
                initialGeminiStatus={state.geminiHooksStatus}
                initialCodexStatus={state.codexHooksStatus}
                initialOpenCodeStatus={state.openCodeHooksStatus}
                isRemote={!!state.task.sshHost}
                onStatusesChange={(updates) => {
                  setState((current) => current
                    ? {
                        ...current,
                        claudeHooksStatus: updates.claudeStatus !== undefined ? updates.claudeStatus : current.claudeHooksStatus,
                        geminiHooksStatus: updates.geminiStatus !== undefined ? updates.geminiStatus : current.geminiHooksStatus,
                        codexHooksStatus: updates.codexStatus !== undefined ? updates.codexStatus : current.codexHooksStatus,
                        openCodeHooksStatus: updates.openCodeStatus !== undefined ? updates.openCodeStatus : current.openCodeHooksStatus,
                      }
                    : current);
                }}
              />
            </div>
          ) : null}

        </section>
      ) : null}

      <main className="ml-14 flex h-full min-w-0 flex-col">
        {mainView === "chat" ? (
          <InlineAiChatView taskId={state.task.id} />
        ) : hasTerminal ? (
          <div className="flex-1 flex flex-col min-h-0 rounded-lg overflow-hidden shadow-md transition-all duration-200 ease-out">
            <div className="bg-terminal-chrome flex items-center gap-3 px-4 py-2.5 shrink-0">
              {terminalTabs.tabs.length > 0 ? (
                <TerminalTabBar
                  tabs={terminalTabs.tabs}
                  onSelect={(tabId) => { void terminalTabs.selectTab(tabId); }}
                  onCreate={() => { void terminalTabs.createTab(); }}
                  onClose={(tabId) => { void closeTerminalTabOrWindow(tabId); }}
                  onRename={(tabId, name) => { void terminalTabs.renameTab(tabId, name); }}
                  onMove={(tabId, targetIndex) => { void terminalTabs.moveTab(tabId, targetIndex); }}
                />
              ) : (
                <span className="text-xs text-terminal-text font-mono truncate">{state.task.sessionName ?? t("terminal")}</span>
              )}
              <div className="ml-auto">
                <NotificationCenterButton ref={notificationCenterRef} buttonClassName="text-terminal-text hover:text-white hover:bg-white/10" panelClassName="mt-3" />
              </div>
            </div>
            <div
              className="flex-1 min-h-0 bg-terminal-bg"
              onClick={() => {
                if (visiblePanel !== null) {
                  closeDetailPanel();
                }
              }}
            >
              <TerminalLoader
                taskId={state.task.id}
                tabs={state.task.sessionType === SessionType.TERMINAL ? terminalTabs.tabs : undefined}
              />
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
