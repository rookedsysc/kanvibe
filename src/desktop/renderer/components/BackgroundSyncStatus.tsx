import { useTranslations } from "next-intl";
import type { useBackgroundTaskSync } from "@/desktop/renderer/hooks/useBackgroundTaskSync";

export default function BackgroundSyncStatus({ sync, reviewCount = 0, isCleaning = false, onOpenReview }: {
  sync: ReturnType<typeof useBackgroundTaskSync>;
  reviewCount?: number;
  isCleaning?: boolean;
  onOpenReview?: () => void;
}) {
  const t = useTranslations("common.backgroundSyncStatus");
  const { status, error, isStarting, start, refresh } = sync;
  const isRunning = isStarting || status?.isRunning;
  const phase = status?.phase ?? "idle";
  const isFailed = phase === "failed" || Boolean(error);
  let message = !status ? t("loading") : isStarting && !status.isRunning ? t("starting") : t(phase);
  if (phase === "completed") {
    message = t(status?.reviewNeeded ? "reviewNeeded" : status?.boardUpdated ? "boardUpdated" : "noChanges");
  }

  return (
    <section aria-label={t("title")} className="border-b border-border-default bg-brand-primary/5 px-4 py-3">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <p className="text-sm font-semibold text-text-primary">{t("title")}</p>
          <p role="status" aria-live="polite" className={`mt-1 text-xs ${isFailed ? "text-status-error" : "text-text-secondary"}`}>
            {error ?? status?.error ?? message}
          </p>
        </div>
        <button
          type="button"
          disabled={isRunning || (!status && !error)}
          onClick={() => void (error && !status ? refresh() : start())}
          className="shrink-0 rounded-md border border-border-default px-3 py-2 text-xs text-text-primary hover:bg-bg-page disabled:cursor-progress disabled:opacity-60"
        >
          {t(isRunning ? "running" : isFailed ? "retry" : "start")}
        </button>
      </div>
      {isCleaning || onOpenReview ? (
        <div className="mt-3">
          <button type="button" onClick={onOpenReview} className="rounded-md bg-brand-primary px-3 py-2 text-xs font-medium text-text-inverse hover:bg-brand-hover">
            {t(isCleaning ? "cleaning" : "detected", { count: reviewCount })}
          </button>
          <p className="mt-2 text-xs text-text-muted">{t("cleanupContinues")}</p>
        </div>
      ) : null}
      {isRunning ? (
        <>
          <div role="progressbar" aria-label={t("title")} aria-valuemin={0} aria-valuemax={3} aria-valuenow={status?.completedStages ?? 0} aria-valuetext={message} className="mt-3 flex gap-1">
            {[0, 1, 2].map((stage) => (
              <span key={stage} className={`h-1 flex-1 rounded-full ${stage < (status?.completedStages ?? 0) ? "bg-brand-primary" : stage === (status?.completedStages ?? 0) ? "bg-brand-primary/50 motion-safe:animate-pulse" : "bg-border-default"}`} />
            ))}
          </div>
          <p className="mt-2 text-xs text-text-muted">{t("continues")}</p>
        </>
      ) : null}
    </section>
  );
}
