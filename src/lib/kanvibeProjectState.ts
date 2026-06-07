import path from "path";
import { TaskStatus } from "@/entities/KanbanTask";
import { readTextFile, writeTextFile } from "@/lib/hostFileAccess";

export const KANVIBE_DIR_NAME = ".kanvibe";
export const TASK_STATE_FILE_NAME = "status.json";

export interface KanvibeTaskState {
  schemaVersion: 1;
  status: TaskStatus;
  updatedAt?: string;
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
  taskState: Pick<KanvibeTaskState, "status">,
  sshHost?: string | null,
): Promise<void> {
  await writeTextFile(
    getKanvibeTaskStatePath(repoPath, sshHost),
    buildKanvibeTaskStateContent(taskState),
    sshHost,
  );
}

export function buildKanvibeTaskStateContent(
  taskState: Pick<KanvibeTaskState, "status">,
): string {
  const payload: KanvibeTaskState = {
    schemaVersion: 1,
    status: taskState.status,
    updatedAt: new Date().toISOString(),
  };

  return JSON.stringify(payload, null, 2) + "\n";
}

export function parseKanvibeTaskState(content: string): KanvibeTaskState | null {
  if (typeof content !== "string" || !content.trim()) {
    return null;
  }

  const statusFromMarkdown = parseTaskStatus(content.trim());
  if (statusFromMarkdown) {
    return {
      schemaVersion: 1,
      status: statusFromMarkdown,
    };
  }

  try {
    const parsed = JSON.parse(content) as { status?: unknown; updatedAt?: unknown };
    const status = parseTaskStatus(parsed.status);
    if (!status) {
      return null;
    }

    return {
      schemaVersion: 1,
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

function getPathModule(sshHost?: string | null): typeof path.posix | typeof path {
  return sshHost ? path.posix : path;
}
