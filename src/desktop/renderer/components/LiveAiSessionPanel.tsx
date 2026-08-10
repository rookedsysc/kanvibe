"use client";

import { useTranslations } from "next-intl";
import { AiProviderIcon } from "@/components/AiProviderIcon";
import type { LiveAiSession } from "@/lib/aiSessions/types";

interface LiveAiSessionPanelProps {
  sessions: LiveAiSession[];
  /** 세션이 붙어 있는 터미널로 이동한다. 터미널 창을 못 찾은 세션은 클릭할 수 없다 */
  onSelectSession?: (session: LiveAiSession) => void;
  className?: string;
  testId?: string;
}

/**
 * 실행중인 AI 세션과 그 세션이 지금 돌리고 있는 서브태스크를 보여준다.
 * 보드 카드의 팝오버와 태스크 상세 dock 패널이 같은 화면을 써야 해서 한 컴포넌트로 둔다.
 */
export function LiveAiSessionPanel({
  sessions,
  onSelectSession,
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
    <ul className={className} data-testid={testId}>
      {sessions.map((session) => (
        <li key={session.provider} className="border-b border-border-subtle last:border-b-0">
          <LiveAiSessionRow session={session} onSelectSession={onSelectSession} />
        </li>
      ))}
    </ul>
  );
}

function LiveAiSessionRow({
  session,
  onSelectSession,
}: {
  session: LiveAiSession;
  onSelectSession?: (session: LiveAiSession) => void;
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
      <span className="shrink-0 text-[10px] text-text-muted">
        {session.runningSubtasks.length > 0
          ? t("subtaskCount", { count: session.runningSubtasks.length })
          : stateLabel}
      </span>
    </span>
  );

  return (
    <div className="px-2 py-1.5" data-testid={`live-ai-session-${session.provider}`}>
      {isSelectable ? (
        <button
          type="button"
          className="flex w-full items-center rounded px-1 py-0.5 text-left hover:bg-bg-page"
          onClick={() => onSelectSession?.(session)}
          title={t("focusTerminal", { window: session.terminalWindow?.windowName ?? "" })}
        >
          {summary}
        </button>
      ) : (
        <div className="flex w-full items-center px-1 py-0.5">{summary}</div>
      )}

      {session.state === "running" && (
        <div
          className="kv-live-progress mt-1 ml-6 h-0.5"
          role="progressbar"
          aria-label={stateLabel}
          data-testid="live-ai-session-progress"
        />
      )}

      {session.runningSubtasks.length > 0 && (
        <ul className="mt-1 space-y-0.5 pl-6">
          {session.runningSubtasks.map((subtask) => (
            <li
              key={subtask.id}
              className="truncate text-[10px] text-text-secondary"
              data-testid="live-ai-subtask"
            >
              {subtask.name ?? subtask.id}
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
