import { useTranslations } from "next-intl";
import { AiProviderIcon } from "@/components/AiProviderIcon";
import { useAiUsage } from "@/desktop/renderer/hooks/useAiUsage";
import type {
  AiUsageAccountResult,
  AiUsageProvider,
  AiUsageSnapshot,
  AiUsageWindow,
} from "@/lib/aiUsage/types";

/** 한도가 가까워지는 구간. 이 위부터는 강조색이 아니라 심각도 색으로 알린다 */
const HIGH_USAGE_PERCENT = 90;
const ELEVATED_USAGE_PERCENT = 75;

interface ProviderAccountGroup {
  provider: AiUsageProvider;
  results: AiUsageAccountResult[];
}

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

/** provider가 나타난 순서를 그대로 지켜 조회할 때마다 카드가 자리를 바꾸지 않게 한다 */
function groupAccountsByProvider(accounts: AiUsageAccountResult[]): ProviderAccountGroup[] {
  const groups = new Map<AiUsageProvider, AiUsageAccountResult[]>();

  for (const account of accounts) {
    const existingResults = groups.get(account.provider);
    if (existingResults) {
      existingResults.push(account);
    } else {
      groups.set(account.provider, [account]);
    }
  }

  return [...groups].map(([provider, results]) => ({ provider, results }));
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

/** 계정 라벨은 같은 provider에 계정이 여럿일 때만 붙인다. 하나뿐이면 이메일이 자리만 차지한다 */
function AccountUsageBlock({
  result,
  showLabel,
}: {
  result: AiUsageAccountResult;
  showLabel: boolean;
}) {
  const t = useTranslations("taskDetail.aiUsage");

  return (
    <div data-testid="ai-usage-account">
      {showLabel ? (
        <p className="mb-1 truncate text-[11px] text-text-muted" title={result.label}>
          {result.label}
        </p>
      ) : null}

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
    </div>
  );
}

function ProviderUsageCard({ group }: { group: ProviderAccountGroup }) {
  const planName = group.results.find((result) => result.planName)?.planName ?? null;
  const showAccountLabels = group.results.length > 1;

  return (
    <section
      className="rounded-md border border-border-subtle p-2.5"
      data-testid={`ai-usage-provider-${group.provider}`}
    >
      <header className="mb-2 flex items-center gap-1.5">
        <AiProviderIcon provider={group.provider} size={15} />
        <span className="text-xs font-semibold capitalize text-text-primary">{group.provider}</span>
        {planName ? (
          <span className="rounded bg-bg-page px-1.5 py-0.5 text-[10px] uppercase text-text-muted">
            {planName}
          </span>
        ) : null}
      </header>

      <div className="space-y-2.5">
        {group.results.map((result) => (
          <AccountUsageBlock
            key={`${result.provider}-${result.accountId}`}
            result={result}
            showLabel={showAccountLabels}
          />
        ))}
      </div>
    </section>
  );
}

/**
 * 조회 상태 한 줄.
 * 보여줄 값이 있으면 화면을 비우지 않고 위에 갱신 중이라고만 알린다.
 */
function UsageStatusLine({
  snapshot,
  isLoading,
  isRefreshing,
}: {
  snapshot: AiUsageSnapshot | null;
  isLoading: boolean;
  isRefreshing: boolean;
}) {
  const t = useTranslations("taskDetail.aiUsage");

  if (isLoading) {
    return <span className="text-[11px] text-text-muted">{t("loading")}</span>;
  }

  if (isRefreshing) {
    return (
      <span className="text-[11px] text-text-muted" data-testid="ai-usage-refreshing">
        {t("refreshing")}
      </span>
    );
  }

  const updatedAtText = snapshot ? formatResetTime(snapshot.fetchedAt) : null;
  return (
    <span className="text-[11px] text-text-muted">
      {updatedAtText ? t("updatedAt", { time: updatedAtText }) : null}
    </span>
  );
}

export default function AiUsagePanel({ isOpen }: { isOpen: boolean }) {
  const t = useTranslations("taskDetail.aiUsage");
  const { snapshot, isLoading, isRefreshing, hasFailed, refresh } = useAiUsage(isOpen);

  return (
    <div className="space-y-3" data-testid="ai-usage-panel">
      <div className="flex items-center justify-between">
        <UsageStatusLine snapshot={snapshot} isLoading={isLoading} isRefreshing={isRefreshing} />
        <button
          type="button"
          className="rounded-md border border-border-default px-2 py-1 text-[11px] text-text-secondary transition-colors hover:bg-bg-page hover:text-text-primary disabled:opacity-50"
          disabled={isRefreshing}
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
        {snapshot ? groupAccountsByProvider(snapshot.accounts).map((group) => (
          <ProviderUsageCard key={group.provider} group={group} />
        )) : null}
      </div>
    </div>
  );
}
