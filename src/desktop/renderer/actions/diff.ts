import { invokeDesktop } from "@/desktop/renderer/ipc";
import type { DiffFile, TaskWorkspaceFile, TaskWorkspaceFileFailureReason } from "@/desktop/main/services/diffService";
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

export type TaskWorkspaceFileResult =
  | { ok: true; file: TaskWorkspaceFile }
  | { ok: false; reason: TaskWorkspaceFileFailureReason };

/** main이 던진 오류는 IPC를 건너며 `...: TaskWorkspaceFileError: <사유>` 꼴의 메시지 글만 남으므로 거기서 사유를 다시 읽는다 */
const TASK_WORKSPACE_FILE_FAILURE_PATTERN = /TaskWorkspaceFileError: (outside-worktree|not-found|too-large|unreadable)/;

/** 미리보기용 파일 읽기. 실패는 던지지 않고 사유와 함께 돌려주며, 사유를 읽을 수 없는 실패는 `unreadable`이다 */
export async function readTaskWorkspaceFile(taskId: string, filePath: string): Promise<TaskWorkspaceFileResult> {
  try {
    return { ok: true, file: await invokeDesktop<TaskWorkspaceFile>("diff", "readTaskWorkspaceFile", taskId, filePath) };
  } catch (error) {
    const failure = TASK_WORKSPACE_FILE_FAILURE_PATTERN.exec(error instanceof Error ? error.message : String(error));
    return { ok: false, reason: (failure?.[1] as TaskWorkspaceFileFailureReason | undefined) ?? "unreadable" };
  }
}

export function saveFileContent(taskId: string, filePath: string, content: string): Promise<{ success: boolean; error?: string }> {
  return invokeDesktop("diff", "saveFileContent", taskId, filePath, content);
}
