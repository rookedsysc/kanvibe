"use client";

import { useTranslations } from "next-intl";
import { AiProviderIcon } from "@/components/AiProviderIcon";
import { countByProvider } from "@/desktop/shared/liveAiSessions";
import type { LiveAiSession } from "@/lib/aiSessions/types";

interface LiveAiSessionPanelProps {
  sessions: LiveAiSession[];
  /** 세션이 붙어 있는 터미널로 이동한다. 터미널 창을 못 찾은 세션은 클릭할 수 없다 */
  onSelectSession?: (session: LiveAiSession) => void;
  /** 세션의 호출 그래프를 연다. 좁은 보드 팝오버는 이 동작을 붙이지 않는다 */
  onOpenGraph?: (session: LiveAiSession) => void;
  className?: string;
  testId?: string;
}

/**
 * 실행중인 AI 세션과 그 세션이 지금 돌리고 있는 서브에이전트를 계보로 보여준다.
 *
 * 소속은 색으로, 계층은 선으로 나눠 담는다. 한 채널이 둘을 같이 지면 "몇 개가 도는가"와
 * "무엇에 매달려 있는가"가 서로를 가리기 때문이다.
 * 보드 카드의 팝오버와 태스크 상세 dock 패널이 같은 화면을 써야 해서 한 컴포넌트로 둔다.
 */
export function LiveAiSessionPanel({
  sessions,
  onSelectSession,
  onOpenGraph,
  className,
  testId = "live-ai-session-panel",
}: LiveAiSessionPanelProps) {
  const t = useTranslations("taskDetail.liveSessions");

  if (sessions.length === 0) {
    return (
      <div className={className} data-testid={testId}>
        <p className="px-2 py-3 text-xs text-text-muted">{t("empty")}</p>
      </div>
    );
  }

  return (
    <div className={className} data-testid={testId}>
      <LiveSessionTally sessions={sessions} />
      <ul>
        {sessions.map((session, sessionIndex) => (
          <li
            key={`${session.provider}-${session.sessionId ?? sessionIndex}`}
            className="border-b border-border-subtle last:border-b-0"
          >
            <LiveAiSessionRow
              session={session}
              onSelectSession={onSelectSession}
              onOpenGraph={onOpenGraph}
            />
          </li>
        ))}
      </ul>
    </div>
  );
}

/** provider별 세션 수를 목록 위에 고정한다. 행을 세지 않고도 "claude 몇 개"가 읽혀야 한다 */
function LiveSessionTally({ sessions }: { sessions: LiveAiSession[] }) {
  const t = useTranslations("taskDetail.liveSessions");

  return (
    <div
      className="flex items-center gap-1.5 border-b border-border-subtle px-2 py-1.5"
      data-testid="live-ai-session-tally"
    >
      {countByProvider(sessions).map(([provider, count]) => (
        <span
          key={provider}
          aria-label={t("providerSessionCount", { provider, count })}
          className="inline-flex items-center gap-1 rounded border border-border-subtle bg-bg-page px-1.5 py-0.5"
          data-testid={`live-ai-session-tally-${provider}`}
        >
          <AiProviderIcon provider={provider} size={12} />
          <span className="text-[11px] font-semibold leading-none tabular-nums text-text-primary">
            {count}
          </span>
        </span>
      ))}
    </div>
  );
}

function LiveAiSessionRow({
  session,
  onSelectSession,
  onOpenGraph,
}: {
  session: LiveAiSession;
  onSelectSession?: (session: LiveAiSession) => void;
  onOpenGraph?: (session: LiveAiSession) => void;
}) {
  const t = useTranslations("taskDetail.liveSessions");
  const isSelectable = Boolean(onSelectSession && session.terminalWindow);
  const stateLabel = session.state === "running" ? t("running") : t("idle");

  const summary = (
    <span className="flex min-w-0 flex-1 items-center gap-2">
      <LiveSessionStateDot state={session.state} label={stateLabel} />
      <AiProviderIcon provider={session.provider} size={14} />
      <span className="min-w-0 flex-1 truncate text-xs text-text-primary" title={session.currentTask ?? session.provider}>
        {session.currentTask ?? session.provider}
      </span>
      {session.runningSubtasks.length > 0 && (
        <span className="shrink-0 text-[10px] text-text-muted">
          {t("subtaskCount", { count: session.runningSubtasks.length })}
        </span>
      )}
      <LiveSessionElapsed lastActiveAt={session.lastActiveAt} />
    </span>
  );

  return (
    <div
      className="kv-agent-rail px-2 py-1.5"
      data-agent={session.provider}
      data-testid={`live-ai-session-${session.provider}`}
    >
      <div className="flex w-full items-center gap-1">
        {isSelectable ? (
          <button
            type="button"
            className="flex min-w-0 flex-1 items-center rounded px-1 py-0.5 text-left hover:bg-bg-page"
            onClick={() => onSelectSession?.(session)}
            title={t("focusTerminal", { window: session.terminalWindow?.windowName ?? "" })}
          >
            {summary}
          </button>
        ) : (
          <div className="flex min-w-0 flex-1 items-center px-1 py-0.5">{summary}</div>
        )}

        {onOpenGraph && session.sessionId && (
          <button
            type="button"
            className="shrink-0 rounded border border-border-subtle px-1 py-0.5 text-[10px] text-text-secondary hover:bg-bg-page"
            onClick={() => onOpenGraph(session)}
            data-testid="live-ai-session-open-graph"
          >
            {t("graph.open")}
          </button>
        )}
      </div>

      {session.state === "running" && (
        <div
          className="kv-live-progress mt-1 ml-6 h-0.5"
          role="progressbar"
          aria-label={stateLabel}
          data-testid="live-ai-session-progress"
        />
      )}

      {session.runningSubtasks.length > 0 && (
        <ul className="mt-1 ml-[13px]">
          {session.runningSubtasks.map((subtask, subtaskIndex) => (
            <li
              key={subtask.id}
              className="kv-subtask-branch relative flex items-center gap-1 py-0.5 pl-3.5 text-[10px] text-text-secondary"
              data-last={subtaskIndex === session.runningSubtasks.length - 1}
              data-testid="live-ai-subtask"
            >
              <span className="min-w-0 flex-1 truncate">{subtask.name ?? subtask.id}</span>
              <LiveSessionElapsed lastActiveAt={subtask.lastActiveAt} />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function LiveSessionStateDot({ state, label }: { state: LiveAiSession["state"]; label: string }) {
  return (
    <span
      aria-label={label}
      title={label}
      data-testid={`live-ai-session-state-${state}`}
      className={`h-1.5 w-1.5 shrink-0 rounded-full ${
        state === "running" ? "bg-status-success" : "bg-status-todo"
      }`}
    />
  );
}

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;

/**
 * 마지막 활동으로부터 지난 시간.
 * "돌고 있다"만으로는 지켜볼지 끼어들지를 정할 수 없어서, 얼마나 오래 그러고 있는지까지 붙인다.
 */
function LiveSessionElapsed({ lastActiveAt }: { lastActiveAt: string | null }) {
  const t = useTranslations("taskDetail.liveSessions");

  if (!lastActiveAt) {
    return null;
  }

  const elapsedMs = Date.now() - Date.parse(lastActiveAt);
  if (!Number.isFinite(elapsedMs) || elapsedMs < 0) {
    return null;
  }

  const elapsedLabel = elapsedMs >= HOUR_MS
    ? t("elapsedHours", { hours: Math.floor(elapsedMs / HOUR_MS) })
    : elapsedMs >= MINUTE_MS
      ? t("elapsedMinutes", { minutes: Math.floor(elapsedMs / MINUTE_MS) })
      : t("elapsedSeconds", { seconds: Math.floor(elapsedMs / 1_000) });

  return (
    <span className="shrink-0 tabular-nums text-[10px] text-text-muted" data-testid="live-ai-session-elapsed">
      {elapsedLabel}
    </span>
  );
}
