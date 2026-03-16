"use client";

import React, { useEffect, useMemo, useRef, useState } from "react";
import * as Switch from "@radix-ui/react-switch";
import { useTranslations } from "next-intl";
import { getTaskAiSessionDetail, getTaskAiSessions } from "@/app/actions/project";
import { fuzzyMatch } from "@/utils/fuzzySearch";
import type {
  AggregatedAiMessage,
  AggregatedAiSession,
  AggregatedAiSessionDetail,
  AggregatedAiSessionsResult,
  AiSessionProvider,
  AiSessionSourceStatus,
  AiMessageRole,
} from "@/lib/aiSessions/types";

interface AiSessionsDialogProps {
  taskId: string;
  isOpen: boolean;
  onClose: () => void;
  data: AggregatedAiSessionsResult;
}

const PROVIDERS: AiSessionProvider[] = ["claude", "codex", "opencode", "gemini"];
const DETAIL_PAGE_SIZE = 20;
const MAX_VISIBLE_PROVIDER_CHIPS = 2;

export default function AiSessionsDialog({ taskId, isOpen, onClose, data }: AiSessionsDialogProps) {
  const t = useTranslations("taskDetail");
  const [includeRepoSessions, setIncludeRepoSessions] = useState(false);
  const [selectedProviders, setSelectedProviders] = useState<AiSessionProvider[]>(PROVIDERS);
  const [sessionSearchQuery, setSessionSearchQuery] = useState("");
  const [messageSearchQuery, setMessageSearchQuery] = useState("");
  const [selectedRoles, setSelectedRoles] = useState<AiMessageRole[]>(["user", "assistant"]);
  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);
  const [sessionsData, setSessionsData] = useState<AggregatedAiSessionsResult>(data);
  const [isSessionsLoading, setIsSessionsLoading] = useState(false);
  const [detail, setDetail] = useState<AggregatedAiSessionDetail | null>(null);
  const [isDetailLoading, setIsDetailLoading] = useState(false);
  const [isLoadingMore, setIsLoadingMore] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);
  const [sessionsError, setSessionsError] = useState<string | null>(null);
  const latestDetailRequestId = useRef<string | null>(null);

  useEffect(() => {
    setSessionsData(data);
  }, [data]);

  // 세션 목록 로드 (검색어 포함)
  useEffect(() => {
    if (!isOpen) {
      setIncludeRepoSessions(false);
      setSelectedProviders(PROVIDERS);
      setSessionSearchQuery("");
      setMessageSearchQuery("");
      setSelectedRoles(["user", "assistant"]);
      setSessionsData(data);
      setSessionsError(null);
      setIsSessionsLoading(false);
      setIsDetailLoading(false);
      return;
    }

    let cancelled = false;

    async function loadSessions(): Promise<void> {
      setIsSessionsLoading(true);
      setSessionsError(null);

      try {
        const result = await getTaskAiSessions(taskId, includeRepoSessions, sessionSearchQuery);
        if (!cancelled) {
          setSessionsData(result);
        }
      } catch {
        if (!cancelled) {
          setSessionsError(t("aiSessions.sessionsError"));
        }
      } finally {
        if (!cancelled) {
          setIsSessionsLoading(false);
        }
      }
    }

    const timer = setTimeout(() => {
      void loadSessions();
    }, 300); // 디바운스 적용

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [includeRepoSessions, isOpen, t, taskId, sessionSearchQuery, data]);

  // 상세 로드 (검색어 및 역할 필터 포함)
  useEffect(() => {
    if (!isOpen || !selectedSessionId) return;

    const selectedSession = sessionsData.sessions.find((s) => s.id === selectedSessionId);
    if (!selectedSession) return;

    let cancelled = false;

    async function reloadDetail() {
      await loadSessionDetail({
        taskId,
        includeRepoSessions,
        session: selectedSession!,
        latestDetailRequestId,
        setSelectedSessionId,
        setDetail,
        setIsDetailLoading,
        setDetailError,
        errorMessage: t("aiSessions.detailError"),
        query: messageSearchQuery,
        roles: selectedRoles,
      });
    }

    const timer = setTimeout(() => {
      void reloadDetail();
    }, 300);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [messageSearchQuery, selectedRoles, selectedSessionId, taskId, includeRepoSessions, isOpen, t]);

  const providerCounts = useMemo(() => buildProviderCounts(sessionsData.sources), [sessionsData.sources]);

  const filteredSessions = useMemo(() => {
    if (selectedProviders.length === 0) return [];
    return sessionsData.sessions.filter((session) => selectedProviders.includes(session.provider));
  }, [selectedProviders, sessionsData.sessions]);

  const selectedSession = filteredSessions.find((session) => session.id === selectedSessionId) ?? null;

  useEffect(() => {
    if (!isOpen || !selectedSessionId) {
      setDetail(null);
      setDetailError(null);
    }
  }, [isOpen, selectedSessionId]);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-[500] flex items-center justify-center bg-black/20 p-4">
      <div className="w-full max-w-6xl rounded-xl border border-border-default bg-bg-surface shadow-lg">
        <div className="flex items-start justify-between gap-4 border-b border-border-default px-6 py-4">
          <div>
            <h2 className="text-lg font-semibold text-text-primary">{t("aiSessions.title")}</h2>
            <p className="mt-1 text-sm text-text-secondary">{t("aiSessions.description")}</p>
          </div>
          <button onClick={onClose} className="text-lg text-text-muted transition-colors hover:text-text-primary">
            {t("hooksStatusDialog.close")}
          </button>
        </div>

        <div className="space-y-4 px-6 py-5">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div className="flex flex-1 flex-col gap-3 sm:flex-row sm:items-center">
              <ProviderMultiSelectFilter
                selectedProviders={selectedProviders}
                onChange={setSelectedProviders}
                providerCounts={providerCounts}
              />
              <div className="relative min-w-0 flex-1 sm:max-w-[280px]">
                <input
                  type="text"
                  value={sessionSearchQuery}
                  onChange={(e) => setSessionSearchQuery(e.target.value)}
                  placeholder={t("aiSessions.searchPlaceholder")}
                  className="w-full rounded-md border border-border-default bg-bg-page px-3 py-2 pl-9 text-sm text-text-primary focus:border-brand-primary focus:outline-none"
                />
                <svg className="absolute left-3 top-2.5 text-text-muted" width="16" height="16" viewBox="0 0 16 16" fill="none">
                  <path d="M14.5 14.5L11 11M12.5 7C12.5 10.0376 10.0376 12.5 7 12.5C3.96243 12.5 1.5 10.0376 1.5 7C1.5 3.96243 3.96243 1.5 7 1.5C10.0376 1.5 12.5 3.96243 12.5 7Z" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
              </div>
            </div>
            <div className="flex items-center gap-3">
              <div className="flex shrink-0 items-center gap-1 rounded-md border border-border-default bg-bg-page p-1">
                <button
                  onClick={() => {
                    const role = "user";
                    setSelectedRoles(selectedRoles.includes(role) ? selectedRoles.filter((r) => r !== role) : [...selectedRoles, role]);
                  }}
                  className={`rounded px-2 py-1 text-[10px] font-medium transition-colors ${
                    selectedRoles.includes("user") ? "bg-brand-primary text-text-inverse" : "text-text-muted hover:bg-bg-surface"
                  }`}
                >
                  {t("aiSessions.filterUser")}
                </button>
                <button
                  onClick={() => {
                    const role = "assistant";
                    setSelectedRoles(selectedRoles.includes(role) ? selectedRoles.filter((r) => r !== role) : [...selectedRoles, role]);
                  }}
                  className={`rounded px-2 py-1 text-[10px] font-medium transition-colors ${
                    selectedRoles.includes("assistant") ? "bg-brand-primary text-text-inverse" : "text-text-muted hover:bg-bg-surface"
                  }`}
                >
                  {t("aiSessions.filterAssistant")}
                </button>
              </div>
              <CompactScopeToggle checked={includeRepoSessions} onChange={setIncludeRepoSessions} />
            </div>
          </div>

          {sessionsData.isRemote ? (
            <EmptyState text={t("aiSessions.remoteUnsupported")} />
          ) : isSessionsLoading ? (
            <EmptyState text={t("aiSessions.loadingSessions")} />
          ) : sessionsError ? (
            <EmptyState text={sessionsError} />
          ) : filteredSessions.length === 0 && !sessionSearchQuery ? (
            <EmptyState text={t("aiSessions.empty")} />
          ) : (
            <div className="grid gap-4 lg:grid-cols-[320px_minmax(0,1fr)]">
              <div className="flex flex-col gap-2 overflow-hidden">
                <SessionList
                  sessions={filteredSessions}
                  selectedSessionId={selectedSession?.id ?? null}
                  onSelect={(session) => {
                    void loadSessionDetail({
                      taskId,
                      includeRepoSessions,
                      session,
                      latestDetailRequestId,
                      setSelectedSessionId,
                      setDetail,
                      setIsDetailLoading,
                      setDetailError,
                      errorMessage: t("aiSessions.detailError"),
                    });
                  }}
                />
              </div>
              <SessionPreview
                session={selectedSession}
                detail={detail}
                messages={detail?.messages ?? []}
                isLoading={isDetailLoading}
                isLoadingMore={isLoadingMore}
                error={detailError}
                searchQuery={messageSearchQuery}
                setSearchQuery={setMessageSearchQuery}
                selectedRoles={selectedRoles}
                onLoadMore={() =>
                  handleLoadMore(
                    taskId,
                    detail,
                    includeRepoSessions,
                    setDetail,
                    setIsLoadingMore,
                    setDetailError,
                    t,
                    messageSearchQuery,
                    selectedRoles
                  )
                }
              />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

async function handleLoadMore(
  taskId: string,
  detail: AggregatedAiSessionDetail | null,
  includeRepoSessions: boolean,
  setDetail: (value: AggregatedAiSessionDetail | ((current: AggregatedAiSessionDetail | null) => AggregatedAiSessionDetail | null) | null) => void,
  setIsLoadingMore: (value: boolean) => void,
  setDetailError: (value: string | null) => void,
  t: ReturnType<typeof useTranslations>,
  query?: string,
  roles?: AiMessageRole[]
): Promise<void> {
  if (!detail?.nextCursor) return;

  setIsLoadingMore(true);
  setDetailError(null);

  try {
    const nextPage = await getTaskAiSessionDetail(
      taskId,
      detail.provider,
      detail.sessionId,
      detail.sourceRef ?? null,
      detail.nextCursor,
      DETAIL_PAGE_SIZE,
      includeRepoSessions,
      query,
      roles
    );
    if (!nextPage) return;

    setDetail((current) => {
      if (!current) return nextPage;
      return {
        ...current,
        messages: [...current.messages, ...nextPage.messages],
        nextCursor: nextPage.nextCursor,
      };
    });
  } catch {
    setDetailError(t("aiSessions.detailError"));
  } finally {
    setIsLoadingMore(false);
  }
}

async function loadSessionDetail({
  taskId,
  includeRepoSessions,
  session,
  latestDetailRequestId,
  setSelectedSessionId,
  setDetail,
  setIsDetailLoading,
  setDetailError,
  errorMessage,
  query,
  roles,
}: {
  taskId: string;
  includeRepoSessions: boolean;
  session: AggregatedAiSession;
  latestDetailRequestId: React.MutableRefObject<string | null>;
  setSelectedSessionId: (sessionId: string) => void;
  setDetail: (value: AggregatedAiSessionDetail | null) => void;
  setIsDetailLoading: (value: boolean) => void;
  setDetailError: (value: string | null) => void;
  errorMessage: string;
  query?: string;
  roles?: AiMessageRole[];
}): Promise<void> {
  const expectedId = session.id;
  latestDetailRequestId.current = expectedId;
  setSelectedSessionId(session.id);
  setIsDetailLoading(true);
  setDetail(null);
  setDetailError(null);

  try {
    const result = await getTaskAiSessionDetail(
      taskId,
      session.provider,
      session.id,
      session.sourceRef ?? null,
      null,
      DETAIL_PAGE_SIZE,
      includeRepoSessions,
      query,
      roles
    );

    if (latestDetailRequestId.current !== expectedId) return;

    if (!result) {
      setDetail(null);
      setDetailError(errorMessage);
      return;
    }

    setDetail(result);
  } catch {
    if (latestDetailRequestId.current !== expectedId) return;
    setDetail(null);
    setDetailError(errorMessage);
  } finally {
    if (latestDetailRequestId.current === expectedId) {
      setIsDetailLoading(false);
    }
  }
}

function ProviderMultiSelectFilter({
  selectedProviders,
  onChange,
  providerCounts,
}: {
  selectedProviders: AiSessionProvider[];
  onChange: (providers: AiSessionProvider[]) => void;
  providerCounts: Record<AiSessionProvider, number>;
}) {
  const t = useTranslations("taskDetail");
  const [isOpen, setIsOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!isOpen) return;

    function handleOutsideClick(event: MouseEvent): void {
      if (!containerRef.current?.contains(event.target as Node)) {
        setIsOpen(false);
      }
    }

    document.addEventListener("mousedown", handleOutsideClick);
    return () => document.removeEventListener("mousedown", handleOutsideClick);
  }, [isOpen]);

  const filteredProviders = PROVIDERS.filter((provider) => {
    const label = t(`aiSessions.providers.${provider}`).toLowerCase();
    return label.includes(searchQuery.toLowerCase());
  });

  const toggleProvider = (provider: AiSessionProvider) => {
    onChange(
      selectedProviders.includes(provider)
        ? selectedProviders.filter((value) => value !== provider)
        : [...selectedProviders, provider]
    );
  };

  const visibleProviders = selectedProviders.slice(0, MAX_VISIBLE_PROVIDER_CHIPS);
  const remainingCount = selectedProviders.length - visibleProviders.length;

  return (
    <div ref={containerRef} className="relative min-w-0 flex-1 sm:min-w-[200px] sm:max-w-[300px]">
      <button
        type="button"
        onClick={() => setIsOpen((previous) => !previous)}
        className={`flex min-h-[40px] w-full items-center gap-1 overflow-hidden rounded-md border bg-bg-page px-2 py-1.5 text-left text-text-primary transition-colors ${
          isOpen ? "border-brand-primary" : "border-border-default hover:border-brand-primary"
        }`}
      >
        {selectedProviders.length === 0 ? (
          <span className="px-1 text-sm text-text-muted">{t("aiSessions.providerFilterEmpty")}</span>
        ) : (
          <>
            {visibleProviders.map((provider) => (
              <span
                key={provider}
                className="inline-flex max-w-[120px] flex-shrink-0 items-center gap-1 rounded bg-brand-primary/10 px-1.5 py-0.5 text-xs font-medium text-brand-primary"
              >
                <span className="truncate">{t(`aiSessions.providers.${provider}`)}</span>
                <span className="text-[11px] text-text-secondary">{providerCounts[provider]}</span>
              </span>
            ))}
            {remainingCount > 0 ? (
              <span className="inline-flex flex-shrink-0 items-center rounded bg-brand-primary/10 px-1.5 py-0.5 text-xs font-medium text-brand-primary">
                +{remainingCount}
              </span>
            ) : null}
          </>
        )}
        <svg className="ml-auto flex-shrink-0 text-text-muted" width="12" height="12" viewBox="0 0 12 12" fill="none">
          <path d="M3 5L6 8L9 5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </button>

      {isOpen ? (
        <div className="absolute z-50 mt-1 w-full rounded-md border border-border-default bg-bg-surface shadow-md">
          <div className="border-b border-border-default p-2">
            <input
              type="text"
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              placeholder={t("aiSessions.providerSearchPlaceholder")}
              className="w-full rounded border border-border-default bg-bg-page px-2 py-1 text-sm text-text-primary focus:outline-none focus:border-brand-primary"
            />
          </div>
          <ul className="max-h-56 overflow-y-auto py-1">
            {filteredProviders.length === 0 ? (
              <li className="px-3 py-2 text-sm text-text-muted">{t("aiSessions.noProviderMatch")}</li>
            ) : (
              filteredProviders.map((provider) => {
                const checked = selectedProviders.includes(provider);
                return (
                  <li
                    key={provider}
                    onMouseDown={(event) => {
                      event.preventDefault();
                      toggleProvider(provider);
                    }}
                    className="flex cursor-pointer items-center gap-2 px-3 py-2 text-sm text-text-primary transition-colors hover:bg-bg-page"
                  >
                    <span
                      className={`flex h-4 w-4 flex-shrink-0 items-center justify-center rounded border transition-colors ${
                        checked ? "border-brand-primary bg-brand-primary text-text-inverse" : "border-border-default"
                      }`}
                    >
                      {checked ? (
                        <svg width="10" height="8" viewBox="0 0 10 8" fill="none">
                          <path d="M1 4L3.5 6.5L9 1" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                        </svg>
                      ) : null}
                    </span>
                    <span className="flex min-w-0 flex-1 items-center justify-between gap-3">
                      <span className="truncate">{t(`aiSessions.providers.${provider}`)}</span>
                      <span className="text-xs text-text-muted">{providerCounts[provider]}</span>
                    </span>
                  </li>
                );
              })
            )}
          </ul>
        </div>
      ) : null}
    </div>
  );
}

function CompactScopeToggle({ checked, onChange }: { checked: boolean; onChange: (checked: boolean) => void }) {
  const t = useTranslations("taskDetail");

  return (
    <label
      className={`inline-flex shrink-0 cursor-pointer items-center gap-2 self-start whitespace-nowrap rounded-full border px-3 py-1.5 text-xs font-medium transition-colors sm:self-center ${
        checked
          ? "border-brand-primary bg-brand-primary/10 text-brand-primary"
          : "border-border-default bg-bg-page text-text-secondary hover:border-brand-primary hover:text-text-primary"
      }`}
      title={t("aiSessions.includeRepoHint")}
    >
      <Switch.Root
        checked={checked}
        onCheckedChange={onChange}
        aria-label={t("aiSessions.includeRepoToggleShort")}
        className={`relative h-4 w-7 rounded-full outline-none transition-colors ${checked ? "bg-brand-primary" : "bg-border-default"}`}
      >
        <Switch.Thumb className="block h-3 w-3 translate-x-0.5 rounded-full bg-white shadow-sm transition-transform duration-100 will-change-transform data-[state=checked]:translate-x-3.5" />
      </Switch.Root>
      <span>{t("aiSessions.includeRepoToggleShort")}</span>
    </label>
  );
}

function SessionList({ sessions, selectedSessionId, onSelect }: { sessions: AggregatedAiSession[]; selectedSessionId: string | null; onSelect: (session: AggregatedAiSession) => void }) {
  const t = useTranslations("taskDetail");

  return (
    <div className="max-h-[440px] space-y-2 overflow-y-auto rounded-lg border border-border-default bg-bg-page p-3">
      {sessions.map((session) => {
        const isSelected = session.id === selectedSessionId;

        return (
          <button
            key={`${session.provider}-${session.id}`}
            onClick={() => onSelect(session)}
            className={[
              "w-full overflow-hidden rounded-lg border p-3 text-left transition-colors",
              isSelected ? "border-brand-primary bg-bg-surface" : "border-border-default bg-bg-surface hover:border-brand-primary",
            ].join(" ")}
          >
            <div className="flex min-w-0 items-center justify-between gap-2">
              <div className="flex min-w-0 items-center gap-2">
                <ProviderBadge provider={session.provider} />
                <span className={`shrink-0 rounded-full px-2 py-0.5 text-[10px] font-medium ${session.matchScope === "repo" ? "bg-brand-primary/10 text-brand-primary" : "bg-bg-page text-text-muted border border-border-default"}`}>
                  {session.matchScope}
                </span>
              </div>
              <span className="shrink-0 text-xs text-text-muted">{formatDate(session.updatedAt ?? session.startedAt)}</span>
            </div>
            <p className="mt-2 line-clamp-2 break-words text-sm font-medium text-text-primary">{session.title ?? t("aiSessions.untitled")}</p>
            <p className="mt-1 line-clamp-3 break-all text-xs text-text-secondary">{getSessionSubtitle(session, t)}</p>
          </button>
        );
      })}
    </div>
  );
}

function SessionPreview({
  session,
  detail,
  messages,
  isLoading,
  isLoadingMore,
  error,
  searchQuery,
  setSearchQuery,
  selectedRoles,
  onLoadMore,
}: {
  session: AggregatedAiSession | null;
  detail: AggregatedAiSessionDetail | null;
  messages: AggregatedAiMessage[];
  isLoading: boolean;
  isLoadingMore: boolean;
  error: string | null;
  searchQuery: string;
  setSearchQuery: (query: string) => void;
  selectedRoles: AiMessageRole[];
  onLoadMore: () => void;
}) {
  const t = useTranslations("taskDetail");

  if (!session) return <EmptyState text={t("aiSessions.selectSession")} />;

  return (
    <div className="flex max-h-[440px] flex-col overflow-hidden rounded-lg border border-border-default bg-bg-page">
      <div className="border-b border-border-subtle bg-bg-surface p-4 pb-3">
        <div className="flex items-center justify-between gap-4">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <ProviderBadge provider={session.provider} />
              <span className="truncate text-xs text-text-muted">{session.id}</span>
            </div>
            <p className="mt-2 truncate text-sm font-medium text-text-primary">{session.title}</p>
          </div>
        </div>

        <div className="relative mt-3">
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder={t("aiSessions.messageSearchPlaceholder")}
            className="w-full rounded-md border border-border-default bg-bg-page px-3 py-1.5 pl-8 text-xs text-text-primary focus:border-brand-primary focus:outline-none"
          />
          <svg className="absolute left-2.5 top-2 text-text-muted" width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path d="M14.5 14.5L11 11M12.5 7C12.5 10.0376 10.0376 12.5 7 12.5C3.96243 12.5 1.5 10.0376 1.5 7C1.5 3.96243 3.96243 1.5 7 1.5C10.0376 1.5 12.5 3.96243 12.5 7Z" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </div>
      </div>

      <div className="flex-1 space-y-3 overflow-y-auto p-4">
        {isLoading ? <LoadingDetailState session={session} /> : null}
        {!isLoading && error ? <EmptyState text={error} compact /> : null}
        {!isLoading && !error && messages.length === 0 ? (
          <EmptyState text={searchQuery || selectedRoles.length < 2 ? t("aiSessions.noMessageMatch") : t("aiSessions.noPreview")} compact />
        ) : null}
        {!isLoading && !error && messages.length > 0
          ? messages.map((message, index) => (
              <PreviewMessageCard key={`${message.role}-${index}-${message.timestamp ?? index}`} index={index} message={message} />
            ))
          : null}
        {!isLoading && detail?.nextCursor ? (
          <button
            type="button"
            onClick={onLoadMore}
            disabled={isLoadingMore}
            className="w-full rounded-md border border-border-default bg-bg-surface px-3 py-2 text-sm text-text-primary transition-colors hover:border-brand-primary disabled:cursor-not-allowed disabled:opacity-60"
          >
            {isLoadingMore ? t("aiSessions.loadingMore") : t("aiSessions.loadMore")}
          </button>
        ) : null}
      </div>
    </div>
  );
}

function PreviewMessageCard({ message, index }: { message: AggregatedAiMessage; index: number }) {
  const t = useTranslations("taskDetail");
  const [isExpanded, setIsExpanded] = useState(false);
  const displayedText = isExpanded ? message.fullText : message.text;
  const toggleLabel = isExpanded ? t("aiSessions.showLess") : t("aiSessions.showMore");

  return (
    <div className="rounded-lg border border-border-default bg-bg-surface p-3">
      <div className="flex items-center justify-between gap-2 text-xs text-text-muted">
        <span className="font-medium text-text-secondary">{t(`aiSessions.roles.${message.role}`)}</span>
        <span>{formatDate(message.timestamp)}</span>
      </div>
      <p id={`ai-session-message-${index}`} className="mt-2 whitespace-pre-wrap break-words text-sm text-text-primary">{displayedText}</p>
      {message.isTruncated ? (
        <button
          type="button"
          onClick={() => setIsExpanded((previous) => !previous)}
          className="mt-3 inline-flex items-center gap-1.5 rounded-md border border-brand-primary bg-brand-primary/10 px-3 py-1.5 text-xs font-medium text-brand-primary transition-colors hover:bg-brand-primary/15 focus:outline-none focus:ring-2 focus:ring-brand-primary/25"
          aria-expanded={isExpanded}
          aria-controls={`ai-session-message-${index}`}
        >
          <ToggleIcon isExpanded={isExpanded} />
          <span>{toggleLabel}</span>
        </button>
      ) : null}
    </div>
  );
}

function ToggleIcon({ isExpanded }: { isExpanded: boolean }) {
  return isExpanded ? (
    <svg aria-hidden="true" viewBox="0 0 16 16" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 10l4-4 4 4" />
    </svg>
  ) : (
    <svg aria-hidden="true" viewBox="0 0 16 16" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 6l4 4 4-4" />
    </svg>
  );
}

function ProviderBadge({ provider }: { provider: AiSessionProvider }) {
  const t = useTranslations("taskDetail");
  const classNameByProvider: Record<AiSessionProvider, string> = {
    claude: "bg-tag-claude-bg text-tag-claude-text",
    codex: "bg-tag-branch-bg text-tag-branch-text",
    opencode: "bg-status-progress/15 text-status-progress",
    gemini: "bg-status-review/15 text-status-review",
  };

  return <span className={`rounded-full px-2 py-0.5 text-xs font-medium ${classNameByProvider[provider]}`}>{t(`aiSessions.providers.${provider}`)}</span>;
}

function EmptyState({ text, compact = false }: { text: string; compact?: boolean }) {
  return <div className={compact ? "text-sm text-text-muted" : "rounded-lg border border-dashed border-border-default bg-bg-page p-6 text-sm text-text-muted"}>{text}</div>;
}

function LoadingDetailState({ session }: { session: AggregatedAiSession }) {
  const t = useTranslations("taskDetail");
  const loadingTargets = getLoadingTargets(session, t);

  return (
    <div className="rounded-lg border border-border-default bg-bg-surface p-4">
      <div className="flex items-center gap-3">
        <div className="flex items-center gap-1">
          <span className="h-2 w-2 animate-pulse rounded-full bg-brand-primary [animation-delay:0ms]" />
          <span className="h-2 w-2 animate-pulse rounded-full bg-brand-primary/70 [animation-delay:180ms]" />
          <span className="h-2 w-2 animate-pulse rounded-full bg-brand-primary/50 [animation-delay:360ms]" />
        </div>
        <span className="text-sm font-medium text-text-primary">{t("aiSessions.loadingDetail")}</span>
      </div>

      <div className="mt-4 space-y-2">
        {loadingTargets.map((target, index) => (
          <div key={`${target.label}-${index}`} className="rounded-md border border-border-subtle bg-bg-page px-3 py-2">
            <p className="text-xs font-medium text-text-secondary">{target.label}</p>
            <p className="mt-1 truncate font-mono text-xs text-text-muted">{target.value}</p>
          </div>
        ))}
      </div>
    </div>
  );
}

function getSessionSubtitle(session: AggregatedAiSession, t: ReturnType<typeof useTranslations>): string {
  if (session.firstUserPrompt) {
    return session.firstUserPrompt;
  }

  if (session.matchedPath) {
    const parts = session.matchedPath.split(/[\\/]/).filter(Boolean);
    return parts.at(-1) ?? session.matchedPath;
  }

  return session.id || t("aiSessions.openDetailHint");
}

function getLoadingTargets(
  session: AggregatedAiSession,
  t: ReturnType<typeof useTranslations>
): Array<{ label: string; value: string }> {
  const targets: Array<{ label: string; value: string }> = [];

  if (session.sourceRef) {
    targets.push({
      label: t("aiSessions.loadingSource"),
      value: session.sourceRef,
    });
  } else {
    targets.push({
      label: t("aiSessions.loadingSource"),
      value: getProviderLoadingPath(session.provider),
    });
  }

  if (session.matchedPath) {
    targets.push({
      label: t("aiSessions.loadingWorkspace"),
      value: session.matchedPath,
    });
  }

  return targets;
}

function getProviderLoadingPath(provider: AiSessionProvider): string {
  switch (provider) {
    case "claude":
      return "~/.claude/projects/...";
    case "codex":
      return "~/.codex/sessions/...";
    case "opencode":
      return "~/.local/share/opencode/opencode.db";
    case "gemini":
      return "~/.gemini/tmp/...";
    default:
      return "local session storage";
  }
}

function formatDate(value: string | null): string {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function buildProviderCounts(sources: AiSessionSourceStatus[]): Record<AiSessionProvider, number> {
  return sources.reduce<Record<AiSessionProvider, number>>(
    (accumulator, source) => {
      accumulator[source.provider] = source.sessionCount;
      return accumulator;
    },
    { claude: 0, codex: 0, opencode: 0, gemini: 0 }
  );
}
