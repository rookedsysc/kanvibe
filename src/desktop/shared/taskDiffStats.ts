import type { DiffFile } from "@/desktop/main/services/diffService";

export interface TaskDiffStats {
  fileCount: number;
  additions: number;
  deletions: number;
}

export const EMPTY_TASK_DIFF_STATS: TaskDiffStats = {
  fileCount: 0,
  additions: 0,
  deletions: 0,
};

/**
 * 변경 파일 목록을 카드와 패널이 함께 쓰는 집계로 접는다.
 *
 * 보드는 카드가 많아 main이 접어 둔 값만 받고, 태스크 상세는 목록 자체를 이미 들고 있어 직접 접는다.
 * 두 화면이 같은 숫자를 보여야 해서 셈은 양쪽이 공유하는 이 자리에 둔다.
 */
export function summarizeDiffFiles(files: DiffFile[]): TaskDiffStats {
  return files.reduce<TaskDiffStats>(
    (stats, file) => ({
      fileCount: stats.fileCount + 1,
      additions: stats.additions + file.additions,
      deletions: stats.deletions + file.deletions,
    }),
    EMPTY_TASK_DIFF_STATS,
  );
}
