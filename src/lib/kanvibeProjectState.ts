import path from "path";
import { TaskStatus } from "@/entities/KanbanTask";
import { readTextFile, writeTextFile } from "@/lib/hostFileAccess";

export const KANVIBE_DIR_NAME = ".kanvibe";
export const HOOK_TARGETS_FILE_NAME = "hooks-targets.json";
export const TASK_STATE_FILE_NAME = "task-state.json";

export interface KanvibeHookTarget {
  url: string;
  taskId: string;
}

export interface KanvibeHookTargetsDocument {
  version: 1;
  targets: KanvibeHookTarget[];
}

export interface KanvibeTaskState {
  version: 1;
  taskId?: string;
  status: TaskStatus;
  updatedAt?: string;
}

export function getKanvibeHookTargetsPath(repoPath: string, sshHost?: string | null): string {
  return getPathModule(sshHost).join(repoPath, KANVIBE_DIR_NAME, HOOK_TARGETS_FILE_NAME);
}

export function getKanvibeTaskStatePath(repoPath: string, sshHost?: string | null): string {
  return getPathModule(sshHost).join(repoPath, KANVIBE_DIR_NAME, TASK_STATE_FILE_NAME);
}

export async function upsertKanvibeHookTarget(
  repoPath: string,
  target: KanvibeHookTarget,
  sshHost?: string | null,
): Promise<void> {
  const filePath = getKanvibeHookTargetsPath(repoPath, sshHost);
  const currentContent = await readTextFile(filePath, sshHost);
  await writeTextFile(filePath, buildKanvibeHookTargetsContent(currentContent, target), sshHost);
}

export async function readKanvibeTaskState(
  repoPath: string,
  sshHost?: string | null,
): Promise<KanvibeTaskState | null> {
  const content = await readTextFile(getKanvibeTaskStatePath(repoPath, sshHost), sshHost);
  return parseKanvibeTaskState(content);
}

export async function writeKanvibeTaskState(
  repoPath: string,
  taskState: Pick<KanvibeTaskState, "status"> & { taskId?: string | null },
  sshHost?: string | null,
): Promise<void> {
  await writeTextFile(
    getKanvibeTaskStatePath(repoPath, sshHost),
    buildKanvibeTaskStateContent(taskState),
    sshHost,
  );
}

export function buildKanvibeHookTargetsContent(
  currentContent: string,
  target: KanvibeHookTarget,
): string {
  const parsed = parseKanvibeHookTargets(currentContent);
  const nextTargets = parsed.targets.filter((value) => !(
    value.url === target.url && value.taskId === target.taskId
  ));
  nextTargets.push(target);

  return JSON.stringify({ version: 1, targets: nextTargets } satisfies KanvibeHookTargetsDocument, null, 2) + "\n";
}

export function buildKanvibeTaskStateContent(
  taskState: Pick<KanvibeTaskState, "status"> & { taskId?: string | null },
): string {
  const payload: KanvibeTaskState = {
    version: 1,
    ...(taskState.taskId ? { taskId: taskState.taskId } : {}),
    status: taskState.status,
    updatedAt: new Date().toISOString(),
  };

  return JSON.stringify(payload, null, 2) + "\n";
}

export function parseKanvibeTaskState(content: string): KanvibeTaskState | null {
  if (typeof content !== "string" || !content.trim()) {
    return null;
  }

  try {
    const parsed = JSON.parse(content) as { taskId?: unknown; status?: unknown; updatedAt?: unknown };
    const status = parseTaskStatus(parsed.status);
    if (!status) {
      return null;
    }

    return {
      version: 1,
      ...(typeof parsed.taskId === "string" && parsed.taskId.length > 0 ? { taskId: parsed.taskId } : {}),
      status,
      ...(typeof parsed.updatedAt === "string" && parsed.updatedAt.length > 0 ? { updatedAt: parsed.updatedAt } : {}),
    };
  } catch {
    return null;
  }
}

export function parseTaskStatus(value: unknown): TaskStatus | null {
  if (typeof value !== "string") {
    return null;
  }

  const normalized = value.toLowerCase();
  const statuses = new Set<string>(Object.values(TaskStatus));
  return statuses.has(normalized) ? normalized as TaskStatus : null;
}

function parseKanvibeHookTargets(content: string): KanvibeHookTargetsDocument {
  if (!content.trim()) {
    return { version: 1, targets: [] };
  }

  try {
    const parsed = JSON.parse(content) as { targets?: unknown };
    const targets = Array.isArray(parsed.targets)
      ? parsed.targets.flatMap((value): KanvibeHookTarget[] => {
        if (!value || typeof value !== "object") {
          return [];
        }

        const candidate = value as { url?: unknown; taskId?: unknown };
        if (typeof candidate.url !== "string" || typeof candidate.taskId !== "string") {
          return [];
        }

        if (!candidate.url || !candidate.taskId) {
          return [];
        }

        return [{ url: candidate.url, taskId: candidate.taskId }];
      })
      : [];

    return { version: 1, targets };
  } catch {
    return { version: 1, targets: [] };
  }
}

function getPathModule(sshHost?: string | null): typeof path.posix | typeof path {
  return sshHost ? path.posix : path;
}
