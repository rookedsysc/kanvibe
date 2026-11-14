import { fetchAndSavePrUrl } from "@/desktop/renderer/actions/kanban";
import { ensureGitHubCliWithPrompt } from "@/desktop/renderer/utils/githubCliDependencyPrompt";
import type { KanbanTask } from "@/entities/KanbanTask";

interface TranslationFn {
  (key: string, values?: Record<string, string | number | Date>): string;
}

type TaskWithProject = Pick<KanbanTask, "id" | "branchName" | "prUrl" | "sshHost"> & {
  project?: Pick<NonNullable<KanbanTask["project"]>, "sshHost"> | null;
};

/**
 * @param options.isAutomatic 사용자가 누르지 않고 화면이 스스로 건 조회면 true. main이 조회 간격을 두고 이미 알린 인증 실패를 되풀이하지 않는다
 */
export async function fetchPrUrlWithPrompt(
  task: TaskWithProject,
  tCommon: TranslationFn,
  options?: { isAutomatic?: boolean },
): Promise<string | null> {
  if (!task.branchName) {
    return null;
  }

  if (task.prUrl) {
    return task.prUrl;
  }

  const sshHost = task.project?.sshHost ?? task.sshHost ?? null;
  if (sshHost) {
    const isReady = await ensureGitHubCliWithPrompt(sshHost, tCommon);
    if (!isReady) {
      return null;
    }
  }

  return options ? fetchAndSavePrUrl(task.id, options) : fetchAndSavePrUrl(task.id);
}
