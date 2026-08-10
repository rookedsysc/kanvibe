import { useCallback, useEffect, useRef, useState } from "react";
import { getAiUsageSnapshot } from "@/desktop/renderer/actions/aiUsage";
import type { AiUsageSnapshot } from "@/lib/aiUsage/types";

/**
 * 사용량 엔드포인트는 잦은 호출에 429로 응답하므로 패널을 여닫을 때마다 다시 부르지 않는다.
 * 사용량은 분 단위로 움직이는 값이라 이 정도 캐시로도 화면이 뒤처지지 않는다.
 */
const AI_USAGE_CACHE_DURATION_MS = 60_000;

export interface AiUsageState {
  snapshot: AiUsageSnapshot | null;
  isLoading: boolean;
  hasFailed: boolean;
  refresh: () => void;
}

export function useAiUsage(isOpen: boolean): AiUsageState {
  const [snapshot, setSnapshot] = useState<AiUsageSnapshot | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [hasFailed, setHasFailed] = useState(false);
  const lastFetchedAtRef = useRef(0);
  const isFetchingRef = useRef(false);

  const loadSnapshot = useCallback(async () => {
    if (isFetchingRef.current) {
      return;
    }

    isFetchingRef.current = true;
    setIsLoading(true);

    try {
      const nextSnapshot = await getAiUsageSnapshot();
      setSnapshot(nextSnapshot);
      setHasFailed(false);
      lastFetchedAtRef.current = Date.now();
    } catch {
      setHasFailed(true);
    } finally {
      isFetchingRef.current = false;
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    if (Date.now() - lastFetchedAtRef.current < AI_USAGE_CACHE_DURATION_MS) {
      return;
    }

    void loadSnapshot();
  }, [isOpen, loadSnapshot]);

  const refresh = useCallback(() => {
    void loadSnapshot();
  }, [loadSnapshot]);

  return { snapshot, isLoading, hasFailed, refresh };
}
