import { invokeDesktop } from "@/desktop/renderer/ipc";
import type { DiffFile } from "@/desktop/main/services/diffService";
import type { TaskDiffStats } from "@/desktop/shared/taskDiffStats";

export type { DiffFile };

export function getGitDiffFiles(taskId: string): Promise<DiffFile[]> {
  return invokeDesktop("diff", "getGitDiffFiles", taskId);
}

/**
 * 보드 카드가 나눠 쓸 태스크별 변경 집계. 카드마다 부르지 않고 보드가 한 번에 조회한다.
 * 넘긴 태스크만 git을 다시 돌리고, 나머지는 저장돼 있던 집계가 함께 실려 온다.
 */
export function getTaskDiffStats(taskIdsToRefresh: string[]): Promise<Record<string, TaskDiffStats>> {
  return invokeDesktop("diff", "getTaskDiffStats", taskIdsToRefresh);
}

export function getOriginalFileContent(taskId: string, filePath: string): Promise<string> {
  return invokeDesktop("diff", "getOriginalFileContent", taskId, filePath);
}

export function getFileContent(taskId: string, filePath: string): Promise<string> {
  return invokeDesktop("diff", "getFileContent", taskId, filePath);
}

export function saveFileContent(taskId: string, filePath: string, content: string): Promise<{ success: boolean; error?: string }> {
  return invokeDesktop("diff", "saveFileContent", taskId, filePath, content);
}
