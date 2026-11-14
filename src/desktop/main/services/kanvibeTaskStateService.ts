import type { KanbanTask, TaskStatus } from "@/entities/KanbanTask";
import { getProjectRepository } from "@/lib/database";
import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude";
import { persistProjectPriorityToKanvibeState } from "@/desktop/main/services/kanvibeProjectColorService";
import type { KanvibeTaskSyncState } from "@/lib/kanvibeProjectState";
import {
  readKanvibeTaskState,
  readKanvibeTaskSyncState,
  writeKanvibeTaskMetadata,
  writeKanvibeTaskSession,
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

/**
 * git exclude 확인은 원격에서 호스트 키를 검증하는 새 SSH 접속을 두세 번 쓴다. 에이전트 hook 이벤트마다
 * 되풀이하면 그 접속들이 호스트별 인증 순서를 채워, 터미널 준비와 화면 조회가 그 뒤에서 기다린다.
 * 확인에 성공한 저장소는 이 시간 동안 다시 확인하지 않는다. 누군가 exclude를 지워도 이 시간 뒤에는 복구된다.
 */
const STATE_DIRECTORY_EXCLUDE_RECHECK_MS = 10 * 60_000;
const stateDirectoryExcludeChecks = new Map<string, { startedAt: number; check: Promise<void> }>();

function ensureKanvibeStateDirectoryExcluded(
  repoPath: string,
  sshHost?: string | null,
): Promise<void> {
  const checkKey = JSON.stringify([sshHost ?? null, repoPath]);
  const recentCheck = stateDirectoryExcludeChecks.get(checkKey);
  if (recentCheck && Date.now() - recentCheck.startedAt < STATE_DIRECTORY_EXCLUDE_RECHECK_MS) {
    return recentCheck.check;
  }

  const startedCheck = { startedAt: Date.now(), check: Promise.resolve() };
  stateDirectoryExcludeChecks.set(checkKey, startedCheck);
  startedCheck.check = (async () => {
    try {
      await addAiToolPatternsToGitExclude(repoPath, sshHost);
    } catch (error) {
      // 실패한 확인은 기억하지 않아 다음 기록이 다시 시도한다.
      if (stateDirectoryExcludeChecks.get(checkKey) === startedCheck) {
        stateDirectoryExcludeChecks.delete(checkKey);
      }
      console.warn(".kanvibe 상태 디렉터리 git exclude 갱신 실패:", {
        repoPath,
        sshHost: sshHost ?? null,
        error: getErrorMessage(error),
      });
    }
  })();
  return startedCheck.check;
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

type TaskSessionTask = Pick<KanbanTask, "id" | "sessionType" | "sessionName" | "worktreePath" | "sshHost"> & {
  projectId?: string | null;
  branchName?: string | null;
};

/** 데스크탑이 정한 세션을 task.json에 남겨 같은 저장소를 보는 다른 KanVibe가 따라오게 한다 */
export async function persistTaskSessionForTask(task: TaskSessionTask): Promise<void> {
  if (!task.sessionType || !task.sessionName) {
    return;
  }

  const resolvedLocation = await resolveTaskStateLocation(task);
  if (!resolvedLocation) {
    return;
  }

  try {
    await ensureKanvibeStateDirectoryExcluded(resolvedLocation.repoPath, resolvedLocation.sshHost);
    await writeKanvibeTaskSession(
      resolvedLocation.repoPath,
      { sessionType: task.sessionType, sessionName: task.sessionName },
      resolvedLocation.sshHost,
    );
  } catch (error) {
    console.error(".kanvibe task 세션 저장 실패:", {
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
