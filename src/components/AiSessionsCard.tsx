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
  const [isDialogOpen, setIsDialogOpen] = useState(false);

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

  return (
    <>
      <div className="rounded-lg border border-border-default bg-bg-surface p-5 shadow-sm">
        <h3 className="mb-3 text-xs font-semibold uppercase tracking-wider text-text-muted">
          {t("aiSessions.title")}
        </h3>

        <button
          onClick={() => setIsDialogOpen(true)}
          className="flex w-full items-center gap-2 rounded-md border border-border-default bg-bg-page px-3 py-2 text-left transition-colors hover:border-brand-primary"
        >
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-medium text-text-primary">{summary}</p>
            <p className="mt-0.5 text-xs text-text-muted">{t("aiSessions.openDialog")}</p>
          </div>
          <span className="text-text-muted">→</span>
        </button>
      </div>

      <AiSessionsDialog
        taskId={taskId}
        isOpen={isDialogOpen}
        onClose={() => setIsDialogOpen(false)}
        data={data}
      />
    </>
  );
}
