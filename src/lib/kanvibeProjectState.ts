import path from "path";
import { TaskStatus } from "@/entities/KanbanTask";
import { readTextFile, writeTextFile } from "@/lib/hostFileAccess";

export const KANVIBE_DIR_NAME = ".kanvibe";
export const TASK_STATE_FILE_NAME = "status.json";
export const TARGETS_FILE_NAME = "targets.json";

export interface KanvibeTaskState {
  schemaVersion: 1;
  status: TaskStatus;
  updatedAt?: string;
}

export interface KanvibeHookTarget {
  url: string;
  taskId: string;
}

export interface KanvibeTargetsState {
  schemaVersion: 1;
  targets: KanvibeHookTarget[];
  updatedAt?: string;
}

export function getKanvibeTaskStatePath(repoPath: string, sshHost?: string | null): string {
  return getPathModule(sshHost).join(repoPath, KANVIBE_DIR_NAME, TASK_STATE_FILE_NAME);
}

export function getKanvibeTargetsPath(repoPath: string, sshHost?: string | null): string {
  return getPathModule(sshHost).join(repoPath, KANVIBE_DIR_NAME, TARGETS_FILE_NAME);
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

export async function upsertKanvibeHookTarget(
  repoPath: string,
  target: KanvibeHookTarget,
  sshHost?: string | null,
): Promise<void> {
  const currentState = parseKanvibeTargets(
    await readTextFile(getKanvibeTargetsPath(repoPath, sshHost), sshHost),
  );
  const nextTargets = upsertKanvibeTarget(currentState.targets, target);

  await writeTextFile(
    getKanvibeTargetsPath(repoPath, sshHost),
    buildKanvibeTargetsContent(nextTargets),
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

export function buildKanvibeTargetsContent(targets: KanvibeHookTarget[]): string {
  const payload: KanvibeTargetsState = {
    schemaVersion: 1,
    targets: normalizeKanvibeTargets(targets),
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

export function parseKanvibeTargets(content: string): KanvibeTargetsState {
  if (typeof content !== "string" || !content.trim()) {
    return { schemaVersion: 1, targets: [] };
  }

  try {
    const parsed = JSON.parse(content) as { targets?: unknown; updatedAt?: unknown };
    return {
      schemaVersion: 1,
      targets: normalizeKanvibeTargets(Array.isArray(parsed.targets) ? parsed.targets : []),
      ...(typeof parsed.updatedAt === "string" && parsed.updatedAt.length > 0 ? { updatedAt: parsed.updatedAt } : {}),
    };
  } catch {
    return { schemaVersion: 1, targets: [] };
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

function upsertKanvibeTarget(targets: unknown[], target: KanvibeHookTarget): KanvibeHookTarget[] {
  const normalizedTarget = normalizeKanvibeTarget(target);
  if (!normalizedTarget) {
    return normalizeKanvibeTargets(targets);
  }

  const nextTargets = normalizeKanvibeTargets(targets);
  const targetIndex = nextTargets.findIndex((value) => value.taskId === normalizedTarget.taskId);
  if (targetIndex === -1) {
    return [...nextTargets, normalizedTarget];
  }

  return nextTargets.map((value, index) => index === targetIndex ? normalizedTarget : value);
}

function normalizeKanvibeTargets(targets: unknown[]): KanvibeHookTarget[] {
  const nextTargets: KanvibeHookTarget[] = [];
  const seenTaskIds = new Set<string>();

  for (const target of targets) {
    const normalizedTarget = normalizeKanvibeTarget(target);
    if (!normalizedTarget || seenTaskIds.has(normalizedTarget.taskId)) {
      continue;
    }

    seenTaskIds.add(normalizedTarget.taskId);
    nextTargets.push(normalizedTarget);
  }

  return nextTargets;
}

function normalizeKanvibeTarget(target: unknown): KanvibeHookTarget | null {
  const candidate = target as Partial<KanvibeHookTarget> | null;
  if (!candidate || typeof candidate.url !== "string" || typeof candidate.taskId !== "string") {
    return null;
  }

  const url = normalizeKanvibeTargetUrl(candidate.url);
  const taskId = candidate.taskId.trim();
  if (!url || !taskId) {
    return null;
  }

  return { url, taskId };
}

function normalizeKanvibeTargetUrl(url: string): string {
  return url.trim().replace(/\/+$/, "");
}

function getPathModule(sshHost?: string | null): typeof path.posix | typeof path {
  return sshHost ? path.posix : path;
}
