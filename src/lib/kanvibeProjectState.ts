import path from "path";
import { TaskStatus } from "@/entities/KanbanTask";
import { readTextFile, writeTextFile } from "@/lib/hostFileAccess";

export const KANVIBE_DIR_NAME = ".kanvibe";
export const TASK_STATE_FILE_NAME = "status.json";
export const TARGETS_FILE_NAME = "targets.json";

/** `#RRGGBB` 형태의 프로젝트 색상만 허용한다. hook shell script가 같은 규칙으로 검증한다 */
const PROJECT_COLOR_PATTERN = /^#[0-9a-fA-F]{6}$/;

export interface KanvibeTaskState {
  schemaVersion: 1;
  status: TaskStatus;
  updatedAt?: string;
  projectColor?: string;
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

/** status.json에 실제로 담기는 필드. status와 projectColor는 서로 독립적으로 존재할 수 있다 */
interface KanvibeTaskStateFields {
  status: TaskStatus | null;
  projectColor: string | null;
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

/**
 * 다른 KanVibe client가 기록한 프로젝트 색상을 읽는다.
 * status가 아직 없는 status.json에서도 색상만 단독으로 읽을 수 있다.
 */
export async function readKanvibeProjectColor(
  repoPath: string,
  sshHost?: string | null,
): Promise<string | null> {
  const content = await readTextFile(getKanvibeTaskStatePath(repoPath, sshHost), sshHost);
  return parseKanvibeTaskStateFields(content).projectColor;
}

/** task 상태만 갱신하고 다른 client가 기록한 프로젝트 색상은 그대로 보존한다 */
export async function writeKanvibeTaskStatus(
  repoPath: string,
  status: TaskStatus,
  sshHost?: string | null,
): Promise<void> {
  await mergeKanvibeTaskStateFile(repoPath, sshHost, (current) => ({ ...current, status }));
}

/** 프로젝트 색상만 갱신하고 기존 task 상태는 그대로 보존한다 */
export async function writeKanvibeProjectColor(
  repoPath: string,
  projectColor: string,
  sshHost?: string | null,
): Promise<void> {
  const normalizedColor = parseProjectColor(projectColor);
  if (!normalizedColor) {
    return;
  }

  await mergeKanvibeTaskStateFile(repoPath, sshHost, (current) => ({
    ...current,
    projectColor: normalizedColor,
  }));
}

/**
 * hook 알림 대상을 client(url) 단위로 등록한다.
 * 같은 client가 다른 task로 재설치하면 taskId만 교체되고, 새로운 client는 뒤에 추가된다.
 */
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

/** targets.json에 해당 client url과 taskId 쌍이 등록되어 있는지 확인한다 */
export function hasKanvibeHookTarget(content: string, target: KanvibeHookTarget): boolean {
  const expectedTarget = normalizeKanvibeTarget(target);
  if (!expectedTarget) {
    return false;
  }

  return parseKanvibeTargets(content).targets.some(
    (value) => value.url === expectedTarget.url && value.taskId === expectedTarget.taskId,
  );
}

export function buildKanvibeTaskStateContent(
  taskState: Partial<Pick<KanvibeTaskState, "status" | "projectColor">>,
): string {
  const payload: Partial<KanvibeTaskState> = {
    schemaVersion: 1,
    ...(taskState.status ? { status: taskState.status } : {}),
    updatedAt: new Date().toISOString(),
    ...(taskState.projectColor ? { projectColor: taskState.projectColor } : {}),
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
  const { status, projectColor } = parseKanvibeTaskStateFields(content);
  if (!status) {
    return null;
  }

  const updatedAt = parseUpdatedAt(content);
  return {
    schemaVersion: 1,
    status,
    ...(updatedAt ? { updatedAt } : {}),
    ...(projectColor ? { projectColor } : {}),
  };
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

export function parseProjectColor(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }

  const normalized = value.trim();
  return PROJECT_COLOR_PATTERN.test(normalized) ? normalized : null;
}

async function mergeKanvibeTaskStateFile(
  repoPath: string,
  sshHost: string | null | undefined,
  merge: (current: KanvibeTaskStateFields) => KanvibeTaskStateFields,
): Promise<void> {
  const statePath = getKanvibeTaskStatePath(repoPath, sshHost);
  const currentFields = parseKanvibeTaskStateFields(await readTextFile(statePath, sshHost));
  const nextFields = merge(currentFields);

  await writeTextFile(
    statePath,
    buildKanvibeTaskStateContent({
      ...(nextFields.status ? { status: nextFields.status } : {}),
      ...(nextFields.projectColor ? { projectColor: nextFields.projectColor } : {}),
    }),
    sshHost,
  );
}

function parseKanvibeTaskStateFields(content: string): KanvibeTaskStateFields {
  if (typeof content !== "string" || !content.trim()) {
    return { status: null, projectColor: null };
  }

  const statusFromMarkdown = parseTaskStatus(content.trim());
  if (statusFromMarkdown) {
    return { status: statusFromMarkdown, projectColor: null };
  }

  try {
    const parsed = JSON.parse(content) as { status?: unknown; projectColor?: unknown };
    return {
      status: parseTaskStatus(parsed.status),
      projectColor: parseProjectColor(parsed.projectColor),
    };
  } catch {
    return { status: null, projectColor: null };
  }
}

function parseUpdatedAt(content: string): string | null {
  try {
    const parsed = JSON.parse(content) as { updatedAt?: unknown };
    return typeof parsed.updatedAt === "string" && parsed.updatedAt.length > 0 ? parsed.updatedAt : null;
  } catch {
    return null;
  }
}

function upsertKanvibeTarget(targets: unknown[], target: KanvibeHookTarget): KanvibeHookTarget[] {
  const normalizedTarget = normalizeKanvibeTarget(target);
  if (!normalizedTarget) {
    return normalizeKanvibeTargets(targets);
  }

  const nextTargets = normalizeKanvibeTargets(targets);
  const targetIndex = nextTargets.findIndex((value) => value.url === normalizedTarget.url);
  if (targetIndex === -1) {
    return [...nextTargets, normalizedTarget];
  }

  return nextTargets.map((value, index) => index === targetIndex ? normalizedTarget : value);
}

function normalizeKanvibeTargets(targets: unknown[]): KanvibeHookTarget[] {
  const nextTargets: KanvibeHookTarget[] = [];
  const seenUrls = new Set<string>();

  for (const target of targets) {
    const normalizedTarget = normalizeKanvibeTarget(target);
    if (!normalizedTarget || seenUrls.has(normalizedTarget.url)) {
      continue;
    }

    seenUrls.add(normalizedTarget.url);
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
