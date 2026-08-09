import type { KanbanTask, TaskStatus } from "@/entities/KanbanTask";
import { getProjectRepository } from "@/lib/database";
import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude";
import { persistProjectPriorityToKanvibeState } from "@/desktop/main/services/kanvibeProjectColorService";
import type { KanvibeTaskSyncState } from "@/lib/kanvibeProjectState";
import {
  readKanvibeTaskState,
  readKanvibeTaskSyncState,
  writeKanvibeTaskMetadata,
  writeKanvibeTaskStatus,
} from "@/lib/kanvibeProjectState";

type TaskStateTask = Pick<KanbanTask, "id" | "status">;

type TaskWithOptionalLocation = Pick<KanbanTask, "id" | "status" | "worktreePath" | "sshHost"> & {
  projectId?: string | null;
  branchName?: string | null;
};

type TaskMetadataTask = Pick<KanbanTask, "id" | "description" | "priority" | "worktreePath" | "sshHost"> & {
  projectId?: string | null;
  branchName?: string | null;
};

type ProjectTaskStateLocation = {
  id: string;
  repoPath: string;
  defaultBranch: string;
  sshHost: string | null;
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

/** 다른 기기가 남긴 상태와 설명을 함께 읽는다. 기록이 없으면 null이라 DB 값을 그대로 둔다 */
export async function readPersistedTaskSyncStateAtPath(
  repoPath: string | null | undefined,
  sshHost?: string | null,
  projectRepoPath?: string,
): Promise<KanvibeTaskSyncState> {
  if (!repoPath) {
    return { status: null, description: null };
  }

  return readKanvibeTaskSyncState(repoPath, sshHost, projectRepoPath);
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
    await ensureKanvibeStateDirectoryExcluded(repoPath, sshHost);
    await writeKanvibeTaskStatus(repoPath, task.status, sshHost);
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

async function ensureKanvibeStateDirectoryExcluded(
  repoPath: string,
  sshHost?: string | null,
): Promise<void> {
  try {
    await addAiToolPatternsToGitExclude(repoPath, sshHost);
  } catch (error) {
    console.warn(".kanvibe 상태 디렉터리 git exclude 갱신 실패:", {
      repoPath,
      sshHost: sshHost ?? null,
      error: getErrorMessage(error),
    });
  }
}

export async function persistTaskStateForTask(task: TaskWithOptionalLocation): Promise<void> {
  const resolvedLocation = await resolveTaskStateLocation(task);
  await persistTaskStateAtPath(resolvedLocation?.repoPath, task, resolvedLocation?.sshHost ?? null);
}

/** task 설명과 priority를 책임에 맞는 공유 파일에 기록한다 */
export async function persistTaskMetadataForTask(task: TaskMetadataTask): Promise<void> {
  const resolvedLocation = await resolveTaskStateLocation(task);
  if (!resolvedLocation) {
    return;
  }

  try {
    await ensureKanvibeStateDirectoryExcluded(resolvedLocation.repoPath, resolvedLocation.sshHost);
    const project = await resolveProjectRootTaskStateLocation(task);
    const isProjectRootTask = project !== null && task.branchName === project.defaultBranch;

    await writeKanvibeTaskMetadata(
      resolvedLocation.repoPath,
      {
        description: task.description,
        ...(!isProjectRootTask ? { priority: task.priority } : {}),
      },
      resolvedLocation.sshHost,
    );

    if (isProjectRootTask) {
      await persistProjectPriorityToKanvibeState(project, task.priority);
    }
  } catch (error) {
    console.error(".kanvibe task 메타데이터 저장 실패:", {
      repoPath: resolvedLocation.repoPath,
      taskId: task.id,
      sshHost: resolvedLocation.sshHost,
      error: getErrorMessage(error),
    });
  }
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
