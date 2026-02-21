"use client";

import { useState, useCallback, useRef } from "react";
import { useSearchParams } from "next/navigation";

const QUERY_PARAM_KEY = "projects";

/**
 * URL query string 기반 프로젝트 필터 상태를 관리한다.
 * 탭마다 독립적인 필터를 유지하기 위해 localStorage 대신 URL을 사용한다.
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

  const isUpdatingUrl = useRef(false);
  const setSelectedProjectIds = useCallback(
    (idsOrUpdater: string[] | ((prev: string[]) => string[])) => {
      setSelectedProjectIdsState((prev) => {
        const nextIds = typeof idsOrUpdater === "function" ? idsOrUpdater(prev) : idsOrUpdater;

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
