"use client";

import { useState, useCallback, useRef, useEffect } from "react";
import { useSearchParams } from "next/navigation";

const QUERY_PARAM_KEY = "projects";
const SESSION_STORAGE_KEY = "kanvibe_project_filter";

/**
 * URL query string 기반 프로젝트 필터 상태를 관리한다.
 * 탭마다 독립적인 필터를 유지하기 위해 localStorage 대신 URL을 사용한다.
 * redirect 등으로 URL query가 유실될 경우 sessionStorage에서 복원한다.
 * @param validProjectIds 유효한 프로젝트 ID 목록 (존재하지 않는 ID를 걸러내기 위해 사용)
 */
export function useProjectFilterParams(validProjectIds: string[]) {
  const searchParams = useSearchParams();
  const validIdSet = new Set(validProjectIds);

  const [selectedProjectIds, setSelectedProjectIdsState] = useState<string[]>(() => {
    const param = searchParams.get(QUERY_PARAM_KEY);
    if (!param) return [];
    return param.split(",").filter((id) => validIdSet.has(id));
  });

  /**
   * 클라이언트 하이드레이션 후 sessionStorage에서 필터를 복원한다.
   * SSR에서는 sessionStorage에 접근할 수 없으므로 useEffect에서 처리한다.
   */
  const hasRestoredRef = useRef(false);
  useEffect(() => {
    if (hasRestoredRef.current) return;
    hasRestoredRef.current = true;

    const param = searchParams.get(QUERY_PARAM_KEY);
    if (param) {
      /** URL에 필터가 이미 있으면 sessionStorage에 동기화만 수행 */
      const ids = param.split(",").filter((id) => validIdSet.has(id));
      syncToSessionStorage(ids);
      return;
    }

    /** URL에 필터가 없으면 sessionStorage에서 복원 */
    const restored = restoreFromSessionStorage(validIdSet);
    if (restored.length > 0) {
      setSelectedProjectIdsState(restored);
      const url = new URL(window.location.href);
      url.searchParams.set(QUERY_PARAM_KEY, restored.join(","));
      window.history.replaceState(null, "", url.toString());
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const isUpdatingUrl = useRef(false);
  const setSelectedProjectIds = useCallback(
    (idsOrUpdater: string[] | ((prev: string[]) => string[])) => {
      setSelectedProjectIdsState((prev) => {
        const nextIds = typeof idsOrUpdater === "function" ? idsOrUpdater(prev) : idsOrUpdater;

        syncToSessionStorage(nextIds);

        if (!isUpdatingUrl.current) {
          isUpdatingUrl.current = true;
          queueMicrotask(() => {
            const url = new URL(window.location.href);
            if (nextIds.length > 0) {
              url.searchParams.set(QUERY_PARAM_KEY, nextIds.join(","));
            } else {
              url.searchParams.delete(QUERY_PARAM_KEY);
            }
            window.history.replaceState(null, "", url.toString());
            isUpdatingUrl.current = false;
          });
        }

        return nextIds;
      });
    },
    [],
  );

  return [selectedProjectIds, setSelectedProjectIds] as const;
}

function syncToSessionStorage(ids: string[]) {
  try {
    if (ids.length > 0) {
      sessionStorage.setItem(SESSION_STORAGE_KEY, ids.join(","));
    } else {
      sessionStorage.removeItem(SESSION_STORAGE_KEY);
    }
  } catch {
    /* SSR 또는 sessionStorage 접근 불가 환경에서는 무시 */
  }
}

function restoreFromSessionStorage(validIdSet: Set<string>): string[] {
  try {
    const stored = sessionStorage.getItem(SESSION_STORAGE_KEY);
    if (!stored) return [];
    return stored.split(",").filter((id) => validIdSet.has(id));
  } catch {
    return [];
  }
}
