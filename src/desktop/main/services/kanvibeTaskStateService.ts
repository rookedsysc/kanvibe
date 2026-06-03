import type { KanbanTask, TaskStatus } from "@/entities/KanbanTask";
import { getProjectRepository } from "@/lib/database";
import { readKanvibeTaskState, writeKanvibeTaskState } from "@/lib/kanvibeProjectState";

type TaskStateTask = Pick<KanbanTask, "id" | "status">;

type TaskWithOptionalLocation = Pick<KanbanTask, "id" | "status" | "worktreePath" | "sshHost"> & {
  projectId?: string | null;
  branchName?: string | null;
};

type ProjectTaskStateLocation = {
  repoPath: string;
  defaultBranch: string;
  sshHost?: string | null;
};

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export async function readPersistedTaskStatusAtPath(
  repoPath: string | null | undefined,
  sshHost?: string | null,
): Promise<TaskStatus | null> {
  if (!repoPath) {
    return null;
  }

  return (await readKanvibeTaskState(repoPath, sshHost))?.status ?? null;
}

export async function persistTaskStateAtPath(
  repoPath: string | null | undefined,
  task: TaskStateTask,
  sshHost?: string | null,
): Promise<void> {
  if (!repoPath) {
    return;
  }

  try {
    await writeKanvibeTaskState(repoPath, { taskId: task.id, status: task.status }, sshHost);
  } catch (error) {
    console.error(".kanvibe task 상태 저장 실패:", {
      repoPath,
      targetPath: repoPath,
      taskId: task.id,
      status: task.status,
      sshHost: sshHost ?? null,
      error: getErrorMessage(error),
    });
  }
}

export async function persistTaskStateForTask(task: TaskWithOptionalLocation): Promise<void> {
  const resolvedLocation = await resolveTaskStateLocation(task);
  await persistTaskStateAtPath(resolvedLocation?.repoPath, task, resolvedLocation?.sshHost ?? null);
}

async function resolveTaskStateLocation(
  task: Pick<TaskWithOptionalLocation, "worktreePath" | "sshHost" | "projectId" | "branchName">,
): Promise<{ repoPath: string; sshHost: string | null } | null> {
  if (task.worktreePath) {
    return { repoPath: task.worktreePath, sshHost: task.sshHost ?? null };
  }

  const project = await resolveProjectRootTaskStateLocation(task);
  if (!project || !task.branchName || task.branchName !== project.defaultBranch) {
    return null;
  }

  return {
    repoPath: project.repoPath,
    sshHost: task.sshHost || project.sshHost || null,
  };
}

async function resolveProjectRootTaskStateLocation(
  task: Pick<TaskWithOptionalLocation, "projectId">,
): Promise<ProjectTaskStateLocation | null> {
  if (!task.projectId) {
    return null;
  }

  const projectRepo = await getProjectRepository();
  return projectRepo.findOneBy({ id: task.projectId });
}
