import { useCallback } from "react";
import { getGitDiffFiles, getTaskDiffStats, type DiffFile } from "@/desktop/renderer/actions/diff";
import { usePolledValue } from "@/desktop/renderer/hooks/usePolledValue";
import type { TaskDiffStats } from "@/desktop/shared/taskDiffStats";

/** 보드는 카드마다 git을 돌려야 해서, 변경을 눈으로 따라갈 수 있는 선에서 가장 느리게 잡는다 */
const BOARD_DIFF_STATS_POLL_INTERVAL_MS = 5_000;

/** 상세는 한 태스크만 보므로 보드보다 촘촘히 읽어 저장 직후의 변화를 바로 보여준다 */
const TASK_DIFF_FILES_POLL_INTERVAL_MS = 3_000;

const EMPTY_STATS_BY_TASK_ID: Record<string, TaskDiffStats> = {};
const EMPTY_DIFF_FILES: DiffFile[] = [];

/**
 * 보드 카드에 그릴 변경 집계를 한 번의 조회로 받아 온다.
 * `taskIdsToRefresh`에 넘긴 태스크만 git을 다시 돌리고, 나머지 카드 몫은 저장돼 있던 집계로 채워져 온다.
 *
 * 호출자가 매 렌더 새 배열을 만들어도 조회가 다시 돌지 않도록, 목록 자체가 아니라 목록의 내용으로 조회를 묶는다.
 * 다시 돌릴 태스크가 하나도 없어도 조회는 계속한다. 저장된 집계는 다른 창이 갱신할 수 있다.
 */
export function useBoardTaskDiffStats(
  taskIdsToRefresh: string[],
  isEnabled: boolean,
): Record<string, TaskDiffStats> {
  const taskIdsKey = taskIdsToRefresh.join(",");
  const read = useCallback(
    () => getTaskDiffStats(taskIdsKey ? taskIdsKey.split(",") : []),
    [taskIdsKey],
  );

  return usePolledValue(read, EMPTY_STATS_BY_TASK_ID, BOARD_DIFF_STATS_POLL_INTERVAL_MS, isEnabled);
}

/** 태스크 하나의 변경 파일 목록. 목록을 보여주는 패널이 열려 있는 동안에만 폴링한다 */
export function useTaskDiffFiles(taskId: string | null, isEnabled: boolean): DiffFile[] {
  const read = useCallback(
    () => (taskId ? getGitDiffFiles(taskId) : Promise.resolve(EMPTY_DIFF_FILES)),
    [taskId],
  );

  return usePolledValue(
    read,
    EMPTY_DIFF_FILES,
    TASK_DIFF_FILES_POLL_INTERVAL_MS,
    isEnabled && Boolean(taskId),
  );
}
