import { getAppSetting, setAppSetting } from "@/desktop/main/services/appSettingsService";
import { aggregateAiUsage } from "@/lib/aiUsage/aggregateAiUsage";
import { fromCachedSnapshot, toCacheableSnapshot } from "@/lib/aiUsage/usageSnapshotCache";
import type { AiUsageSnapshot } from "@/lib/aiUsage/types";

/** 앱을 껐다 켜도 패널이 빈 화면으로 열리지 않도록 마지막 조회 결과를 남겨 두는 자리 */
const AI_USAGE_SNAPSHOT_CACHE_KEY = "ai_usage_snapshot_cache";

/** 마지막으로 저장된 조회 결과. 저장된 값이 없거나 형식이 맞지 않으면 null */
export async function getCachedAiUsageSnapshot(): Promise<AiUsageSnapshot | null> {
  return fromCachedSnapshot(await getAppSetting(AI_USAGE_SNAPSHOT_CACHE_KEY));
}

export async function getAiUsageSnapshot(): Promise<AiUsageSnapshot> {
  const snapshot = await aggregateAiUsage();

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
