"use client";

import { useMemo, useState } from "react";
import { useTranslations } from "next-intl";
import AiSessionsDialog from "@/components/AiSessionsDialog";
import type { AggregatedAiSessionsResult } from "@/lib/aiSessions/types";

interface AiSessionsCardProps {
  taskId: string;
  data: AggregatedAiSessionsResult;
}

export default function AiSessionsCard({ taskId, data }: AiSessionsCardProps) {
  const t = useTranslations("taskDetail");
  const [openTaskId, setOpenTaskId] = useState<string | null>(null);
  const isDialogOpen = openTaskId === taskId;

  const summary = useMemo(() => {
    const providerCount = data.sources.filter((source) => source.sessionCount > 0).length;

    if (data.isRemote) {
      return t("aiSessions.remoteBadge");
    }

    if (data.sessions.length === 0) {
      return t("aiSessions.emptyBadge");
    }

    return t("aiSessions.summary", {
      sessions: data.sessions.length,
      providers: providerCount,
    });
  }, [data, t]);
  const previewSessions = useMemo(() => data.sessions.slice(0, 3), [data.sessions]);

  return (
    <>
      <button
        type="button"
        onClick={() => setOpenTaskId(taskId)}
        className="block w-full rounded-lg border border-border-default bg-bg-surface p-4 text-left shadow-sm transition-colors hover:border-brand-primary"
      >
        <div className="mb-3 flex items-center justify-between gap-3">
          <div className="min-w-0">
            <h3 className="text-xs font-semibold uppercase tracking-wider text-text-muted">
              {t("aiSessions.title")}
            </h3>
            <p className="mt-1 truncate text-sm font-medium text-text-primary">{summary}</p>
          </div>
          <span className="shrink-0 text-text-muted">→</span>
        </div>

        <div
          data-testid="ai-sessions-chat-preview"
          className="space-y-2 rounded-md border border-border-default bg-terminal-bg px-3 py-3 shadow-inner"
        >
          {previewSessions.length === 0 ? (
            <p className="font-mono text-xs text-terminal-text">{t("aiSessions.openDialog")}</p>
          ) : previewSessions.map((session, index) => (
            <div
              key={`${session.provider}-${session.id}`}
              className="animate-[ai-session-line-in_180ms_ease-out_both] rounded border border-white/10 bg-white/[0.03] px-2.5 py-2"
              style={{ animationDelay: `${index * 70}ms` }}
            >
              <div className="mb-1 flex items-center justify-between gap-2">
                <span className="text-[10px] font-medium uppercase text-terminal-text">{session.provider}</span>
                <span className="text-[10px] text-terminal-text/70">{session.messageCount}</span>
              </div>
              <p className="line-clamp-2 break-words text-xs text-text-primary">
                {session.firstUserPrompt || session.title || t("aiSessions.untitled")}
              </p>
            </div>
          ))}
        </div>

        <p className="mt-2 text-xs text-text-muted">{t("aiSessions.openDialog")}</p>
      </button>

      <AiSessionsDialog
        key={taskId}
        taskId={taskId}
        isOpen={isDialogOpen}
        onClose={() => setOpenTaskId(null)}
        data={data}
      />
    </>
  );
}
