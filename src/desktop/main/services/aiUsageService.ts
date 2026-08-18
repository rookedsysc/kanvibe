import { getAiAccountRegistrations } from "@/desktop/main/services/aiAccountService";
import { getAppSetting, setAppSetting } from "@/desktop/main/services/appSettingsService";
import { aggregateAiUsage } from "@/lib/aiUsage/aggregateAiUsage";
import {
  fromCachedSnapshot,
  toCacheableSnapshot,
  toCachedAccountId,
} from "@/lib/aiUsage/usageSnapshotCache";
import type {
  AiUsageAccountResult,
  AiUsageSnapshot,
  AiUsageWindow,
} from "@/lib/aiUsage/types";

/** 앱을 껐다 켜도 패널이 빈 화면으로 열리지 않도록 마지막 조회 결과를 남겨 두는 자리 */
const AI_USAGE_SNAPSHOT_CACHE_KEY = "ai_usage_snapshot_cache";

/** 마지막으로 저장된 조회 결과. 저장된 값이 없거나 형식이 맞지 않으면 null */
export async function getCachedAiUsageSnapshot(): Promise<AiUsageSnapshot | null> {
  return fromCachedSnapshot(await getAppSetting(AI_USAGE_SNAPSHOT_CACHE_KEY));
}

/** 이미 초기화된 창의 사용률은 지금 값이 아니다. 이월할 수 있는 건 아직 살아 있는 창뿐이다 */
function selectLiveWindows(windows: AiUsageWindow[], nowMs: number): AiUsageWindow[] {
  return windows.filter(({ resetsAt }) => !resetsAt || Date.parse(resetsAt) > nowMs);
}

/**
 * 조회에 실패한 계정에 직전 조회의 값을 이어 붙인다.
 *
 * 사용량 엔드포인트는 몇 번만 연달아 부르면 몇 분씩 429로 잠긴다. 그때 결과를 그대로 쓰면
 * 잘 보이던 퍼센트가 오류 문구로 바뀌는데, 사용자가 알고 싶은 값은 그 사이에 거의 변하지 않는다.
 * 실패 사유는 지우지 않고 함께 남겨 화면이 옛 값을 새 값인 척하지 않게 한다.
 */
function carryLastKnownUsage(
  snapshot: AiUsageSnapshot,
  cachedSnapshot: AiUsageSnapshot | null,
): AiUsageSnapshot {
  if (!cachedSnapshot) {
    return snapshot;
  }

  const nowMs = Date.parse(snapshot.fetchedAt);
  const cachedAccounts = new Map(
    cachedSnapshot.accounts.map((account) => [`${account.provider}:${account.accountId}`, account]),
  );

  return {
    ...snapshot,
    accounts: snapshot.accounts.map((account) => withLastKnownWindows(account, cachedAccounts, nowMs)),
  };
}

function withLastKnownWindows(
  account: AiUsageAccountResult,
  cachedAccounts: Map<string, AiUsageAccountResult>,
  nowMs: number,
): AiUsageAccountResult {
  if (account.windows.length > 0) {
    return account;
  }

  const cached = cachedAccounts.get(`${account.provider}:${toCachedAccountId(account.accountId)}`);
  const liveWindows = cached ? selectLiveWindows(cached.windows, nowMs) : [];
  if (liveWindows.length === 0) {
    return account;
  }

  // 조회 시각은 값이 실제로 만들어진 때로 되돌린다. 실패한 조회의 시각을 붙이면 새 값처럼 보인다
  return {
    ...account,
    planName: account.planName ?? cached?.planName ?? null,
    windows: liveWindows,
    fetchedAt: cached?.fetchedAt ?? account.fetchedAt,
  };
}

export async function getAiUsageSnapshot(): Promise<AiUsageSnapshot> {
  const registrations = await getAiAccountRegistrations();
  const snapshot = carryLastKnownUsage(
    await aggregateAiUsage(registrations),
    await getCachedAiUsageSnapshot(),
  );

  try {
    await setAppSetting(AI_USAGE_SNAPSHOT_CACHE_KEY, toCacheableSnapshot(snapshot));
  } catch (error) {
    // 캐시는 다음 조회를 빠르게 보여주기 위한 편의값이라 저장 실패로 조회까지 실패시키지 않는다
    console.error(
      "[ai-usage] 사용량 스냅샷을 캐시에 저장하지 못했습니다:",
      error instanceof Error ? error.message : error,
    );
  }

  return snapshot;
}
