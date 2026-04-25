import { fetchAndSavePrUrl } from "@/desktop/renderer/actions/kanban";
import { ensureGitHubCliWithPrompt } from "@/desktop/renderer/utils/githubCliDependencyPrompt";
import type { KanbanTask } from "@/entities/KanbanTask";

interface TranslationFn {
  (key: string, values?: Record<string, string | number | Date>): string;
}

type TaskWithProject = Pick<KanbanTask, "id" | "branchName" | "prUrl" | "sshHost"> & {
  project?: Pick<NonNullable<KanbanTask["project"]>, "sshHost"> | null;
};

export async function fetchPrUrlWithPrompt(task: TaskWithProject, tCommon: TranslationFn): Promise<string | null> {
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

  return fetchAndSavePrUrl(task.id);
}
