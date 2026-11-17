import { getAiAccountRegistrations } from "@/desktop/main/services/aiAccountService";
import { getAppSetting, setAppSetting } from "@/desktop/main/services/appSettingsService";
import { aggregateAiUsage } from "@/lib/aiUsage/aggregateAiUsage";
import { AI_USAGE_REFRESH_INTERVAL_MS, SIGN_IN_FAILURE_REASONS } from "@/lib/aiUsage/shared";
import {
  fromCachedSnapshot,
  toCacheableSnapshot,
  toCachedAccountId,
} from "@/lib/aiUsage/usageSnapshotCache";
import type {
  AiUsageAccountResult,
  AiUsageSnapshot,
  AiUsageSnapshotOptions,
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

/**
 * 다시 로그인하기 전까지는 조회가 계속 실패하므로 이월한 값이 끝없이 늙는다.
 * 잠시 뒤 풀리는 실패와 달리 이월이 옛 값을 지켜 주지 못하고 새 값인 척하게만 만든다.
 */
function isSignInFailure(account: AiUsageAccountResult): boolean {
  return Boolean(account.reason && SIGN_IN_FAILURE_REASONS.has(account.reason));
}

function withLastKnownWindows(
  account: AiUsageAccountResult,
  cachedAccounts: Map<string, AiUsageAccountResult>,
  nowMs: number,
): AiUsageAccountResult {
  if (account.windows.length > 0 || isSignInFailure(account)) {
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

async function fetchAiUsageSnapshot(): Promise<AiUsageSnapshot> {
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

/**
 * 창과 패널이 나눠 쓰는 마지막 조회 결과와 지금 진행 중인 조회.
 *
 * 패널은 태스크를 옮길 때마다 새로 열리고 창도 여럿이라, 저마다 조회하면 사용량 엔드포인트가 금방 잠긴다.
 * 앱이 떠 있는 동안만 의미가 있는 값이라 저장하지 않고 모듈이 기억한다.
 */
let recentSnapshot: AiUsageSnapshot | null = null;
let snapshotInFlight: Promise<AiUsageSnapshot> | null = null;

/** 시계가 뒤로 돌아가 조회 시각이 미래가 된 결과는 얼마나 묵었는지 알 수 없으므로 신선하다고 보지 않는다 */
function isWithinRefreshInterval(snapshot: AiUsageSnapshot): boolean {
  const elapsedMs = Date.now() - Date.parse(snapshot.fetchedAt);
  return elapsedMs >= 0 && elapsedMs < AI_USAGE_REFRESH_INTERVAL_MS;
}

/**
 * 옵션 없이 부르면 언제나 새로 조회한다. 새로고침이나 로그인 직후처럼 사람이 방금 바꾼 상태를 봐야 하는 호출이
 * 이쪽이고, 진행 중인 조회에도 합류하지 않는다. 그 조회는 상태가 바뀌기 전에 출발했을 수 있다.
 */
export function getAiUsageSnapshot(options: AiUsageSnapshotOptions = {}): Promise<AiUsageSnapshot> {
  if (options.canReuseRecentSnapshot) {
    if (snapshotInFlight) {
      return snapshotInFlight;
    }

    if (recentSnapshot && isWithinRefreshInterval(recentSnapshot)) {
      return Promise.resolve(recentSnapshot);
    }
  }

  const snapshotRequest = fetchAiUsageSnapshot()
    .then((snapshot) => {
      recentSnapshot = snapshot;
      return snapshot;
    })
    .finally(() => {
      if (snapshotInFlight === snapshotRequest) {
        snapshotInFlight = null;
      }
    });
  snapshotInFlight = snapshotRequest;
  return snapshotRequest;
}
