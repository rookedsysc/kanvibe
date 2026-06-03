import path from "path";
import { TaskStatus } from "@/entities/KanbanTask";
import { readTextFile, writeTextFile } from "@/lib/hostFileAccess";

export const KANVIBE_DIR_NAME = ".kanvibe";
export const TASK_STATE_FILE_NAME = "status.md";

export interface KanvibeTaskState {
  version: 1;
  status: TaskStatus;
}

export function getKanvibeTaskStatePath(repoPath: string, sshHost?: string | null): string {
  return getPathModule(sshHost).join(repoPath, KANVIBE_DIR_NAME, TASK_STATE_FILE_NAME);
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

export function buildKanvibeTaskStateContent(
  taskState: Pick<KanvibeTaskState, "status"> & { taskId?: string | null },
): string {
  return `${taskState.status}\n`;
}

export function parseKanvibeTaskState(content: string): KanvibeTaskState | null {
  if (typeof content !== "string" || !content.trim()) {
    return null;
  }

  const statusFromMarkdown = parseTaskStatus(content.trim());
  if (statusFromMarkdown) {
    return {
      version: 1,
      status: statusFromMarkdown,
    };
  }

  try {
    const parsed = JSON.parse(content) as { status?: unknown };
    const status = parseTaskStatus(parsed.status);
    if (!status) {
      return null;
    }

    return {
      version: 1,
      status,
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

function getPathModule(sshHost?: string | null): typeof path.posix | typeof path {
  return sshHost ? path.posix : path;
}
