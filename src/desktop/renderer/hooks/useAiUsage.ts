import { useCallback, useEffect, useRef, useState } from "react";
import { getAiUsageSnapshot, getCachedAiUsageSnapshot } from "@/desktop/renderer/actions/aiUsage";
import { useIsWindowActive } from "@/desktop/renderer/hooks/useIsWindowActive";
import { AI_USAGE_REFRESH_INTERVAL_MS } from "@/lib/aiUsage/shared";
import type { AiUsageSnapshot } from "@/lib/aiUsage/types";

/**
 * 결과가 만들어진 시각에서 세어야 같은 결과를 나눠 쓰는 창들이 같은 순간에 다시 조회한다.
 * 시계가 틀어져 그 시각이 이미 지나 있으면 받자마자 다시 조회하는 일이 끝없이 되풀이되므로, 그때는 지금부터 센다.
 */
function toNextAutoLoadAtMs(snapshot: AiUsageSnapshot): number {
  const sharedDueAtMs = Date.parse(snapshot.fetchedAt) + AI_USAGE_REFRESH_INTERVAL_MS;
  return sharedDueAtMs > Date.now() ? sharedDueAtMs : Date.now() + AI_USAGE_REFRESH_INTERVAL_MS;
}

export interface AiUsageState {
  snapshot: AiUsageSnapshot | null;
  /** 보여줄 값이 하나도 없어 기다리는 중 */
  isLoading: boolean;
  /** 저장된 값을 보여주면서 뒤에서 새 값을 받아 오는 중 */
  isRefreshing: boolean;
  hasFailed: boolean;
  refresh: () => void;
}

export function useAiUsage(isOpen: boolean): AiUsageState {
  const [snapshot, setSnapshot] = useState<AiUsageSnapshot | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [hasFailed, setHasFailed] = useState(false);
  /**
   * 다음 자동 조회 시각. 조회가 끝날 때마다 새 객체로 바꿔서, 같은 시각이 다시 나와도 예약이 새로 걸리게 한다.
   * 나눠 쓰는 결과를 주기가 끝나기 직전에 받으면 같은 시각이 나오는데, 그때 예약이 끊기면 자동 조회가 멈춘다.
   */
  const [nextAutoLoad, setNextAutoLoad] = useState({ dueAtMs: 0 });
  const isWindowActive = useIsWindowActive();
  const isFetchingRef = useRef(false);
  const hasRequestedCacheRef = useRef(false);
  const refreshQueuedRef = useRef(false);

  const loadSnapshot = useCallback(async (canReuseRecentSnapshot: boolean) => {
    if (isFetchingRef.current) {
      return;
    }

    isFetchingRef.current = true;
    setIsRefreshing(true);
    let canReuseOnThisRead = canReuseRecentSnapshot;

    do {
      refreshQueuedRef.current = false;
      try {
        const nextSnapshot = await getAiUsageSnapshot({ canReuseRecentSnapshot: canReuseOnThisRead });
        setSnapshot(nextSnapshot);
        setHasFailed(false);
        setNextAutoLoad({ dueAtMs: toNextAutoLoadAtMs(nextSnapshot) });
      } catch {
        // 조회가 실패해도 보여주던 값은 남긴다. 빈 화면보다 오래된 값이 낫다
        setHasFailed(true);
        setNextAutoLoad({ dueAtMs: Date.now() + AI_USAGE_REFRESH_INTERVAL_MS });
      }
      // 조회 중에 들어온 새로고침은 사람이 방금 바꾼 상태를 보려는 것이라 나눠 쓰는 결과로 답하지 않는다
      canReuseOnThisRead = false;
    } while (refreshQueuedRef.current);

    isFetchingRef.current = false;
    setIsRefreshing(false);
  }, []);

  const showCachedSnapshot = useCallback(async () => {
    try {
      const cachedSnapshot = await getCachedAiUsageSnapshot();
      if (!cachedSnapshot) {
        return;
      }

      // 캐시를 읽는 사이에 새 조회가 끝났을 수 있으므로 비어 있을 때만 채운다
      setSnapshot((currentSnapshot) => currentSnapshot ?? cachedSnapshot);
    } catch {
      // 저장된 값이 없거나 읽지 못하면 새 조회 결과만 기다린다
    }
  }, []);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    if (!hasRequestedCacheRef.current) {
      hasRequestedCacheRef.current = true;
      void showCachedSnapshot();
    }
  }, [isOpen, showCachedSnapshot]);

  /**
   * 화면이 열려 있고 창이 앞에 있는 동안만 다음 조회를 예약한다.
   * 볼 사람이 없는 동안 밀린 조회는 예약 시각이 이미 지나 있으므로 돌아오는 순간 바로 나간다.
   */
  useEffect(() => {
    if (!isOpen || !isWindowActive) {
      return;
    }

    const autoLoadTimer = window.setTimeout(() => {
      void loadSnapshot(true);
    }, Math.max(0, nextAutoLoad.dueAtMs - Date.now()));

    return () => window.clearTimeout(autoLoadTimer);
  }, [isOpen, isWindowActive, nextAutoLoad, loadSnapshot]);

  const refresh = useCallback(() => {
    if (isFetchingRef.current) {
      refreshQueuedRef.current = true;
      return;
    }
    void loadSnapshot(false);
  }, [loadSnapshot]);

  return { snapshot, isLoading: isRefreshing && !snapshot, isRefreshing, hasFailed, refresh };
}
