import { useEffect, useMemo } from "react";
import { useSearchParams } from "react-router-dom";

export const TASK_KIND_FILTER_VALUES = ["project", "task", "all"] as const;
export type TaskKindFilter = typeof TASK_KIND_FILTER_VALUES[number];

const QUERY_PARAM_KEY = "taskKind";
const SESSION_STORAGE_KEY = "kanvibe_task_kind_filter";
const DEFAULT_TASK_KIND_FILTER: TaskKindFilter = "all";

function normalizeTaskKindFilter(value: string | null): TaskKindFilter {
  if (value && TASK_KIND_FILTER_VALUES.includes(value as TaskKindFilter)) {
    return value as TaskKindFilter;
  }

  return DEFAULT_TASK_KIND_FILTER;
}

function syncToSessionStorage(filter: TaskKindFilter) {
  try {
    if (filter === DEFAULT_TASK_KIND_FILTER) {
      sessionStorage.removeItem(SESSION_STORAGE_KEY);
    } else {
      sessionStorage.setItem(SESSION_STORAGE_KEY, filter);
    }
  } catch {
    /* sessionStorage 접근 불가 시 무시 */
  }
}

function restoreFromSessionStorage(): TaskKindFilter {
  try {
    return normalizeTaskKindFilter(sessionStorage.getItem(SESSION_STORAGE_KEY));
  } catch {
    return DEFAULT_TASK_KIND_FILTER;
  }
}

export function useTaskKindFilterParams() {
  const [searchParams, setSearchParams] = useSearchParams();
  const paramValue = searchParams.get(QUERY_PARAM_KEY);
  const taskKindFilter = useMemo(
    () => normalizeTaskKindFilter(paramValue),
    [paramValue],
  );

  useEffect(() => {
    if (paramValue) {
      const normalizedParam = normalizeTaskKindFilter(paramValue);
      if (normalizedParam !== paramValue) {
        const nextParams = new URLSearchParams(searchParams);
        nextParams.delete(QUERY_PARAM_KEY);
        setSearchParams(nextParams, { replace: true });
        return;
      }

      syncToSessionStorage(normalizedParam);
      return;
    }

    const restored = restoreFromSessionStorage();
    if (restored !== DEFAULT_TASK_KIND_FILTER) {
      const nextParams = new URLSearchParams(searchParams);
      nextParams.set(QUERY_PARAM_KEY, restored);
      setSearchParams(nextParams, { replace: true });
    }
  }, [paramValue, searchParams, setSearchParams]);

  const setTaskKindFilter = (filter: TaskKindFilter) => {
    syncToSessionStorage(filter);

    const nextParams = new URLSearchParams(searchParams);
    if (filter === DEFAULT_TASK_KIND_FILTER) {
      nextParams.delete(QUERY_PARAM_KEY);
    } else {
      nextParams.set(QUERY_PARAM_KEY, filter);
    }
    setSearchParams(nextParams, { replace: true });
  };

  return [taskKindFilter, setTaskKindFilter] as const;
}
