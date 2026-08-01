import path from "path";
import { TaskStatus } from "@/entities/KanbanTask";
import { readTextFile, writeTextFile, writeTextFileIfAbsent } from "@/lib/hostFileAccess";

export const KANVIBE_DIR_NAME = ".kanvibe";
export const TASK_STATE_FILE_NAME = "status.json";
export const PROJECT_STATE_FILE_NAME = "project.json";
export const TARGETS_FILE_NAME = "targets.json";

/** `#RRGGBB` 형태의 프로젝트 색상만 허용한다 */
const PROJECT_COLOR_PATTERN = /^#[0-9a-fA-F]{6}$/;

export interface KanvibeTaskState {
  schemaVersion: 1;
  status: TaskStatus;
  updatedAt?: string;
}

/**
 * 프로젝트 단위 공유 상태. task 상태와 파일을 분리해 hook(status.json 기록)과
 * KanVibe client(project.json 기록)가 서로의 값을 덮어쓸 수 없게 한다.
 */
export interface KanvibeProjectState {
  schemaVersion: 1;
  projectColor: string;
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

export function getKanvibeProjectStatePath(repoPath: string, sshHost?: string | null): string {
  return getPathModule(sshHost).join(repoPath, KANVIBE_DIR_NAME, PROJECT_STATE_FILE_NAME);
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

/** 다른 KanVibe client가 project.json에 기록한 프로젝트 색상을 읽는다 */
export async function readKanvibeProjectColor(
  repoPath: string,
  sshHost?: string | null,
): Promise<string | null> {
  const content = await readTextFile(getKanvibeProjectStatePath(repoPath, sshHost), sshHost);
  return parseKanvibeProjectState(content)?.projectColor ?? null;
}

export async function writeKanvibeTaskStatus(
  repoPath: string,
  status: TaskStatus,
  sshHost?: string | null,
): Promise<void> {
  await writeTextFile(
    getKanvibeTaskStatePath(repoPath, sshHost),
    buildKanvibeTaskStateContent({ status }),
    sshHost,
  );
}

export async function writeKanvibeProjectColor(
  repoPath: string,
  projectColor: string,
  sshHost?: string | null,
): Promise<void> {
  const normalizedColor = parseProjectColor(projectColor);
  if (!normalizedColor) {
    return;
  }

  await writeTextFile(
    getKanvibeProjectStatePath(repoPath, sshHost),
    buildKanvibeProjectStateContent(normalizedColor),
    sshHost,
  );
}

/**
 * 아직 색상 파일이 없는 저장소에만 씨앗 색상을 기록한다.
 * 이미 파일이 있으면 그 값이 권위이므로 기록하지 않는다.
 */
export async function writeKanvibeProjectColorIfAbsent(
  repoPath: string,
  projectColor: string,
  sshHost?: string | null,
): Promise<void> {
  const normalizedColor = parseProjectColor(projectColor);
  if (!normalizedColor) {
    return;
  }

  await writeTextFileIfAbsent(
    getKanvibeProjectStatePath(repoPath, sshHost),
    buildKanvibeProjectStateContent(normalizedColor),
    sshHost,
  );
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
  taskState: Pick<KanvibeTaskState, "status">,
): string {
  const payload: KanvibeTaskState = {
    schemaVersion: 1,
    status: taskState.status,
    updatedAt: new Date().toISOString(),
  };

  return JSON.stringify(payload, null, 2) + "\n";
}

export function buildKanvibeProjectStateContent(projectColor: string): string {
  const payload: KanvibeProjectState = {
    schemaVersion: 1,
    projectColor,
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
    return { schemaVersion: 1, status: statusFromMarkdown };
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
      ...(isNonEmptyString(parsed.updatedAt) ? { updatedAt: parsed.updatedAt } : {}),
    };
  } catch {
    return null;
  }
}

export function parseKanvibeProjectState(content: string): KanvibeProjectState | null {
  if (typeof content !== "string" || !content.trim()) {
    return null;
  }

  try {
    const parsed = JSON.parse(content) as { projectColor?: unknown; updatedAt?: unknown };
    const projectColor = parseProjectColor(parsed.projectColor);
    if (!projectColor) {
      return null;
    }

    return {
      schemaVersion: 1,
      projectColor,
      ...(isNonEmptyString(parsed.updatedAt) ? { updatedAt: parsed.updatedAt } : {}),
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

export function parseProjectColor(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }

  const normalized = value.trim();
  return PROJECT_COLOR_PATTERN.test(normalized) ? normalized : null;
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
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
