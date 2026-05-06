import { buildRouteCacheKey, readRouteCache, removeRouteCache, writeRouteCache } from "@/desktop/renderer/utils/routeCache";

const BOARD_FOCUS_TASK_CACHE_KEY = buildRouteCacheKey("board-focus-task");

function normalizeTaskId(taskId: unknown) {
  return typeof taskId === "string" && taskId.trim() ? taskId : null;
}

export function rememberBoardFocusTask(taskId: string) {
  const normalizedTaskId = normalizeTaskId(taskId);
  if (!normalizedTaskId) {
    return;
  }

  writeRouteCache(BOARD_FOCUS_TASK_CACHE_KEY, normalizedTaskId);
}

export function consumeBoardFocusTask() {
  const taskId = normalizeTaskId(readRouteCache<string>(BOARD_FOCUS_TASK_CACHE_KEY));
  removeRouteCache(BOARD_FOCUS_TASK_CACHE_KEY);
  return taskId;
}
