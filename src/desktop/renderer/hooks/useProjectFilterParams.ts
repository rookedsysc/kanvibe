import { useEffect, useMemo } from "react";
import { useSearchParams } from "react-router-dom";

const QUERY_PARAM_KEY = "projects";
const SESSION_STORAGE_KEY = "kanvibe_project_filter";

export function useProjectFilterParams(validProjectIds: string[]) {
  const validIdSet = useMemo(() => new Set(validProjectIds), [validProjectIds]);
  const [searchParams, setSearchParams] = useSearchParams();
  const selectedProjectIds = useMemo(() => {
    const param = searchParams.get(QUERY_PARAM_KEY);
    if (!param) {
      return [];
    }

    return param.split(",").filter((id) => validIdSet.has(id));
  }, [searchParams, validIdSet]);

  useEffect(() => {
    const param = searchParams.get(QUERY_PARAM_KEY);
    if (param) {
      syncToSessionStorage(param.split(",").filter((id) => validIdSet.has(id)));
      return;
    }

    const restored = restoreFromSessionStorage(validIdSet);
    if (restored.length > 0) {
      const nextParams = new URLSearchParams(searchParams);
      nextParams.set(QUERY_PARAM_KEY, restored.join(","));
      setSearchParams(nextParams, { replace: true });
    }
  }, [searchParams, setSearchParams, validIdSet]);

  const setSelectedProjectIds = (idsOrUpdater: string[] | ((prev: string[]) => string[])) => {
    const nextIds = typeof idsOrUpdater === "function" ? idsOrUpdater(selectedProjectIds) : idsOrUpdater;
    syncToSessionStorage(nextIds);

    const nextParams = new URLSearchParams(searchParams);
    if (nextIds.length > 0) {
      nextParams.set(QUERY_PARAM_KEY, nextIds.join(","));
    } else {
      nextParams.delete(QUERY_PARAM_KEY);
    }
    setSearchParams(nextParams, { replace: true });
  };

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
    /* sessionStorage 접근 불가 시 무시 */
  }
}

function restoreFromSessionStorage(validIdSet: Set<string>): string[] {
  try {
    const stored = sessionStorage.getItem(SESSION_STORAGE_KEY);
    if (!stored) {
      return [];
    }
    return stored.split(",").filter((id) => validIdSet.has(id));
  } catch {
    return [];
  }
}
