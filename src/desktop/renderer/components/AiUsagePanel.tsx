import { useTranslations } from "next-intl";
import { AiProviderIcon } from "@/components/AiProviderIcon";
import { Link } from "@/desktop/renderer/navigation";
import { useAiUsage } from "@/desktop/renderer/hooks/useAiUsage";
import type {
  AiUsageAccountResult,
  AiUsageFailureReason,
  AiUsageProvider,
  AiUsageSnapshot,
  AiUsageWindow,
  AiUsageWindowKind,
} from "@/lib/aiUsage/types";

/** 한도가 가까워지는 구간. 이 위부터는 강조색이 아니라 심각도 색으로 알린다 */
const HIGH_USAGE_PERCENT = 90;
const ELEVATED_USAGE_PERCENT = 75;

interface ProviderAccountGroup {
  provider: AiUsageProvider;
  results: AiUsageAccountResult[];
}

interface UsageWindowGroup {
  kind: AiUsageWindowKind;
  /** 기간 전체 한도. 모델 몫만 주는 provider는 비어 있다 */
  total: AiUsageWindow | null;
  /** 그 기간 안에서 모델 몫만 따로 센 창 */
  scoped: AiUsageWindow[];
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

/**
 * 같은 기간의 창을 한 묶음으로 모은다.
 * 모델별 한도는 이름만으로는 어느 기간에 딸린 것인지 드러나지 않아, 그 기간 한도 아래에 세워야 읽힌다.
 */
function groupWindowsByPeriod(windows: AiUsageWindow[]): UsageWindowGroup[] {
  const groups = new Map<AiUsageWindowKind, UsageWindowGroup>();

  for (const usageWindow of windows) {
    let group = groups.get(usageWindow.kind);
    if (!group) {
      group = { kind: usageWindow.kind, total: null, scoped: [] };
      groups.set(usageWindow.kind, group);
    }

    if (usageWindow.modelName) {
      group.scoped.push(usageWindow);
    } else {
      group.total = usageWindow;
    }
  }

  return [...groups.values()];
}

function UsageWindowRow({ usageWindow }: { usageWindow: AiUsageWindow }) {
  const t = useTranslations("taskDetail.aiUsage");
  const label = usageWindow.modelName ?? t(usageWindow.kind);
  const resetText = usageWindow.resetsAt ? formatResetTime(usageWindow.resetsAt) : null;

  return (
    <div data-testid={`ai-usage-window-${usageWindow.modelName ?? usageWindow.kind}`}>
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

function UsageWindowGroupRows({ group }: { group: UsageWindowGroup }) {
  const scopedRows = group.scoped.map((usageWindow) => (
    <UsageWindowRow key={usageWindow.modelName} usageWindow={usageWindow} />
  ));

  // 딸릴 기간 한도가 없는 모델 쿼터는 들여쓸 대상이 없어 그대로 나열한다
  if (!group.total) {
    return <div className="space-y-2">{scopedRows}</div>;
  }

  return (
    <div className="space-y-2">
      <UsageWindowRow usageWindow={group.total} />
      {scopedRows.length > 0 ? (
        <div
          className="space-y-2 border-l border-border-subtle pl-2"
          data-testid={`ai-usage-scoped-${group.kind}`}
        >
          {scopedRows}
        </div>
      ) : null}
    </div>
  );
}

/** 앱 안에서 로그인하면 풀리는 사유들. 나머지는 로그인과 무관해 계정 화면으로 보낼 이유가 없다 */
const SIGN_IN_FAILURE_REASONS = new Set<AiUsageFailureReason>([
  "missing-credentials",
  "expired-credentials",
  "gemini-cli-not-found",
]);

/** 라벨이 provider 이름뿐이면 카드 제목과 같은 말이라 자리만 차지한다 */
function hasDistinctAccountLabel(result: AiUsageAccountResult): boolean {
  return result.label.toLowerCase() !== result.provider;
}

/**
 * 조회에 실패했는데도 막대가 있으면 그 값은 직전 조회에서 이어 붙인 것이다.
 * 이월된 창은 이미 초기화된 것부터 빠지므로, 옛 값임을 밝히지 않으면 "한도가 사라졌다"로 읽힌다.
 */
function toCarriedUsageTime(result: AiUsageAccountResult): string | null {
  return result.reason && result.windows.length > 0 ? formatResetTime(result.fetchedAt) : null;
}

function AccountUsageBlock({ result }: { result: AiUsageAccountResult }) {
  const t = useTranslations("taskDetail.aiUsage");
  const showLabel = hasDistinctAccountLabel(result);
  const carriedUsageTime = toCarriedUsageTime(result);

  return (
    <div data-testid="ai-usage-account">
      {/* 어느 계정의 사용량인지와 그 계정의 등급은 한 줄에 붙어야 계정마다 갈라 읽힌다 */}
      {showLabel || result.planName ? (
        <div className="mb-1 flex items-baseline gap-2">
          {showLabel ? (
            <p
              className="truncate text-[11px] text-text-muted"
              title={result.label}
              data-testid="ai-usage-account-label"
            >
              {result.label}
            </p>
          ) : null}
          {result.planName ? (
            <span className="ml-auto shrink-0 rounded bg-bg-page px-1.5 py-0.5 text-[10px] uppercase text-text-muted">
              {result.planName}
            </span>
          ) : null}
        </div>
      ) : null}

      {result.windows.length > 0 ? (
        <div className="space-y-2">
          {groupWindowsByPeriod(result.windows).map((group) => (
            <UsageWindowGroupRows key={group.kind} group={group} />
          ))}
        </div>
      ) : null}

      {/* 직전 값을 이어 붙인 카드는 막대와 사유가 함께 보여야 옛 값이 새 값으로 읽히지 않는다 */}
      {result.reason ? (
        <div className={`text-xs text-text-muted${result.windows.length > 0 ? " mt-2" : ""}`}>
          {carriedUsageTime ? (
            <span className="mr-1" data-testid="ai-usage-carried">
              {t("carriedValues", { time: carriedUsageTime })}
            </span>
          ) : null}
          <span>{t(`reasons.${result.reason}`)}</span>
          {/* 로그인으로 풀리는 사유는 문구로 끝내지 않는다. 여기서 계정 화면으로 바로 갈 수 있어야 한다 */}
          {SIGN_IN_FAILURE_REASONS.has(result.reason) ? (
            <Link
              href="/ai-accounts"
              className="ml-1 text-brand-primary hover:underline"
              data-testid="ai-usage-manage-accounts"
            >
              {t("manageAccounts")}
            </Link>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function ProviderUsageCard({ group }: { group: ProviderAccountGroup }) {
  return (
    // 터미널이 반투명이면 배경 없는 카드는 뒤의 터미널 글자와 겹쳐 읽히지 않는다
    <section
      className="rounded-md border border-border-subtle bg-bg-surface p-2.5"
      data-testid={`ai-usage-provider-${group.provider}`}
    >
      <header className="mb-2 flex items-center gap-1.5">
        <AiProviderIcon provider={group.provider} size={15} />
        <span className="text-xs font-semibold capitalize text-text-primary">{group.provider}</span>
      </header>

      <div className="space-y-2.5">
        {group.results.map((result) => (
          <AccountUsageBlock key={`${result.provider}-${result.accountId}`} result={result} />
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
