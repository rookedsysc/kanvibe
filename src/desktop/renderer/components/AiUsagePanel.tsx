import { useTranslations } from "next-intl";
import { AiProviderIcon } from "@/components/AiProviderIcon";
import { useAiUsage } from "@/desktop/renderer/hooks/useAiUsage";
import type { AiUsageProviderResult, AiUsageWindow } from "@/lib/aiUsage/types";

/** 한도가 가까워지는 구간. 이 위부터는 강조색이 아니라 심각도 색으로 알린다 */
const HIGH_USAGE_PERCENT = 90;
const ELEVATED_USAGE_PERCENT = 75;

function getUsageBarClassName(usedPercent: number): string {
  if (usedPercent >= HIGH_USAGE_PERCENT) {
    return "bg-status-error";
  }

  if (usedPercent >= ELEVATED_USAGE_PERCENT) {
    return "bg-status-warning";
  }

  return "bg-brand-primary";
}

function formatResetTime(resetsAt: string): string | null {
  const resetDate = new Date(resetsAt);
  if (Number.isNaN(resetDate.getTime())) {
    return null;
  }

  const isToday = resetDate.toDateString() === new Date().toDateString();
  return isToday
    ? resetDate.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })
    : resetDate.toLocaleDateString(undefined, {
        month: "numeric",
        day: "numeric",
        hour: "numeric",
        minute: "2-digit",
      });
}

function UsageWindowRow({ usageWindow }: { usageWindow: AiUsageWindow }) {
  const t = useTranslations("taskDetail.aiUsage");
  const label = usageWindow.kind === "model" ? usageWindow.modelName : t(usageWindow.kind);
  const resetText = usageWindow.resetsAt ? formatResetTime(usageWindow.resetsAt) : null;

  return (
    <div data-testid={`ai-usage-window-${usageWindow.kind}`}>
      <div className="flex items-baseline justify-between text-xs">
        <span className="text-text-secondary">{label}</span>
        <span className="font-medium text-text-primary">{usageWindow.usedPercent}%</span>
      </div>
      <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-bg-page">
        <div
          className={`h-full rounded-full ${getUsageBarClassName(usageWindow.usedPercent)}`}
          style={{ width: `${usageWindow.usedPercent}%` }}
        />
      </div>
      {resetText ? (
        <p className="mt-1 text-[11px] text-text-muted">{t("resetsAt", { time: resetText })}</p>
      ) : null}
    </div>
  );
}

function ProviderUsageCard({ result }: { result: AiUsageProviderResult }) {
  const t = useTranslations("taskDetail.aiUsage");

  return (
    <section
      className="rounded-md border border-border-subtle p-2.5"
      data-testid={`ai-usage-provider-${result.provider}`}
    >
      <header className="mb-2 flex items-center gap-1.5">
        <AiProviderIcon provider={result.provider} size={15} />
        <span className="text-xs font-semibold capitalize text-text-primary">{result.provider}</span>
        {result.planName ? (
          <span className="rounded bg-bg-page px-1.5 py-0.5 text-[10px] uppercase text-text-muted">
            {result.planName}
          </span>
        ) : null}
      </header>

      {result.status === "ok" ? (
        <div className="space-y-2">
          {result.windows.map((usageWindow) => (
            <UsageWindowRow
              key={`${usageWindow.kind}-${usageWindow.modelName ?? ""}`}
              usageWindow={usageWindow}
            />
          ))}
        </div>
      ) : (
        <p className="text-xs text-text-muted">{result.reason ? t(`reasons.${result.reason}`) : null}</p>
      )}
    </section>
  );
}

export default function AiUsagePanel({ isOpen }: { isOpen: boolean }) {
  const t = useTranslations("taskDetail.aiUsage");
  const { snapshot, isLoading, hasFailed, refresh } = useAiUsage(isOpen);
  const updatedAtText = snapshot ? formatResetTime(snapshot.fetchedAt) : null;

  return (
    <div className="space-y-3" data-testid="ai-usage-panel">
      <div className="flex items-center justify-between">
        <span className="text-[11px] text-text-muted">
          {isLoading ? t("loading") : updatedAtText ? t("updatedAt", { time: updatedAtText }) : null}
        </span>
        <button
          type="button"
          className="rounded-md border border-border-default px-2 py-1 text-[11px] text-text-secondary transition-colors hover:bg-bg-page hover:text-text-primary disabled:opacity-50"
          disabled={isLoading}
          onClick={refresh}
          data-testid="ai-usage-refresh"
        >
          {t("refresh")}
        </button>
      </div>

      {hasFailed && !snapshot ? (
        <p className="text-xs text-status-error">{t("snapshotFailed")}</p>
      ) : null}

      <div className="space-y-2">
        {snapshot?.providers.map((result) => (
          <ProviderUsageCard key={result.provider} result={result} />
        ))}
      </div>
    </div>
  );
}
