import { useCallback, useEffect, useRef, useState } from "react";
import { getAiUsageSnapshot, getCachedAiUsageSnapshot } from "@/desktop/renderer/actions/aiUsage";
import type { AiUsageSnapshot } from "@/lib/aiUsage/types";

/**
 * 사용량 엔드포인트는 잦은 호출에 429로 응답하므로 패널을 여닫을 때마다 다시 부르지 않는다.
 * 사용량은 분 단위로 움직이는 값이라 이 정도 캐시로도 화면이 뒤처지지 않는다.
 */
const AI_USAGE_CACHE_DURATION_MS = 60_000;

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
  const lastFetchedAtRef = useRef(0);
  const isFetchingRef = useRef(false);
  const hasRequestedCacheRef = useRef(false);

  const loadSnapshot = useCallback(async () => {
    if (isFetchingRef.current) {
      return;
    }

    isFetchingRef.current = true;
    setIsRefreshing(true);

    try {
      const nextSnapshot = await getAiUsageSnapshot();
      setSnapshot(nextSnapshot);
      setHasFailed(false);
      lastFetchedAtRef.current = Date.now();
    } catch {
      // 조회가 실패해도 보여주던 값은 남긴다. 빈 화면보다 오래된 값이 낫다
      setHasFailed(true);
    } finally {
      isFetchingRef.current = false;
      setIsRefreshing(false);
    }
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

    if (Date.now() - lastFetchedAtRef.current < AI_USAGE_CACHE_DURATION_MS) {
      return;
    }

    void loadSnapshot();
  }, [isOpen, loadSnapshot, showCachedSnapshot]);

  const refresh = useCallback(() => {
    void loadSnapshot();
  }, [loadSnapshot]);

  return { snapshot, isLoading: isRefreshing && !snapshot, isRefreshing, hasFailed, refresh };
}
