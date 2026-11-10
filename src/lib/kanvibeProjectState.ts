import path from "path";
import { SessionType, TaskStatus } from "@/entities/KanbanTask";
import { TaskPriority } from "@/entities/TaskPriority";
import { isValidHookClientId } from "@/lib/hookSocketPath";
import { findSymbolicLinkPath, readTextFile, readTextFiles, updateTextFile, writeTextFile, writeTextFileIfAbsent } from "@/lib/hostFileAccess";

export const KANVIBE_DIR_NAME = ".kanvibe";
export const TASK_STATE_FILE_NAME = "status.json";
export const PROJECT_STATE_FILE_NAME = "project.json";
export const TARGETS_FILE_NAME = "targets.json";
export const TASK_DESCRIPTION_FILE_NAME = "task.json";

/** `#RRGGBB` 형태의 프로젝트 색상만 허용한다 */
const PROJECT_COLOR_PATTERN = /^#[0-9a-fA-F]{6}$/;

export interface KanvibeTaskState {
  schemaVersion: 1;
  status: TaskStatus;
  updatedAt?: string;
}

/**
 * 프로젝트 단위 공유 상태. root task priority와 색상을 task 상태 파일에서 분리해 hook(status.json 기록)과
 * KanVibe client(project.json 기록)가 서로의 값을 덮어쓸 수 없게 한다.
 */
export interface KanvibeProjectState {
  schemaVersion: 1;
  projectColor?: string;
  priority?: TaskPriority | null;
  updatedAt?: string;
}

/**
 * task 메타데이터 공유 상태. hook이 통째로 재작성하는 status.json과 파일을 분리해
 * agent가 상태를 기록해도 사용자가 남긴 설명과 priority가 지워지지 않게 한다.
 * description이 null이면 다른 기기에서도 설명을 지우라는 뜻이다.
 */
export interface KanvibeTaskDescription {
  schemaVersion: 1;
  description?: string | null;
  priority?: TaskPriority | null;
  session?: KanvibeTaskSession;
  updatedAt?: string;
}

/** 데스크탑이 정한 task 터미널 세션. 같은 저장소를 보는 다른 KanVibe가 이 값을 따라 세션을 맞춘다 */
export interface KanvibeTaskSession {
  sessionType: SessionType;
  sessionName: string;
}

/** 다른 기기의 상태를 한 번에 반영하기 위해 함께 읽는 값들 */
export interface KanvibeTaskSyncState {
  status: TaskStatus | null;
  description: KanvibeTaskDescription | null;
  priority?: TaskPriority | null;
  projectPriority?: TaskPriority | null;
  session?: KanvibeTaskSession;
}

export interface KanvibeHookTarget {
  url: string;
  taskId: string;
  /** 이 client가 task CLI 요청을 받을 때 확인하는 task 단위 자격 증명. 파일을 읽을 수 있는 사용자만 쓸 수 있다 */
  token?: string;
  /** 원격 hook·CLI가 찾는 이 client의 터널 소켓 이름. IP가 바뀌어도 그대로다 */
  clientId?: string;
}

/** targets.json은 task CLI 자격 증명을 담으므로 소유자만 읽게 한다 */
const TARGETS_FILE_MODE = 0o600;

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

export function getKanvibeTaskDescriptionPath(repoPath: string, sshHost?: string | null): string {
  return getPathModule(sshHost).join(repoPath, KANVIBE_DIR_NAME, TASK_DESCRIPTION_FILE_NAME);
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

/**
 * 다른 기기가 남긴 task 상태와 설명을 함께 읽는다.
 * 두 값은 늘 같은 시점에 필요하므로 원격 저장소에서도 한 번의 왕복으로 가져온다.
 */
export async function readKanvibeTaskSyncState(
  repoPath: string,
  sshHost?: string | null,
  projectRepoPath = repoPath,
): Promise<KanvibeTaskSyncState> {
  const taskStatePath = getKanvibeTaskStatePath(repoPath, sshHost);
  const taskDescriptionPath = getKanvibeTaskDescriptionPath(repoPath, sshHost);
  const projectStatePath = getKanvibeProjectStatePath(projectRepoPath, sshHost);
  const files = await readTextFiles([taskStatePath, taskDescriptionPath, projectStatePath], sshHost);
  const taskMetadata = parseKanvibeTaskDescription(files.get(taskDescriptionPath)?.content ?? "");
  const projectState = parseKanvibeProjectState(files.get(projectStatePath)?.content ?? "");

  return {
    status: parseKanvibeTaskState(files.get(taskStatePath)?.content ?? "")?.status ?? null,
    description: taskMetadata && Object.hasOwn(taskMetadata, "description") ? taskMetadata : null,
    ...(taskMetadata && Object.hasOwn(taskMetadata, "priority")
      ? { priority: taskMetadata.priority ?? null }
      : {}),
    ...(projectState && Object.hasOwn(projectState, "priority")
      ? { projectPriority: projectState.priority ?? null }
      : {}),
    ...(taskMetadata?.session ? { session: taskMetadata.session } : {}),
  };
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

export async function writeKanvibeTaskMetadata(
  repoPath: string,
  task: Pick<KanvibeTaskDescription, "description" | "priority">,
  sshHost?: string | null,
): Promise<void> {
  /** 세션 정보는 다른 경로가 기록하므로 설명과 priority를 쓰는 동안 그대로 남긴다 */
  await updateTextFile(
    getKanvibeTaskDescriptionPath(repoPath, sshHost),
    (content) => buildKanvibeTaskDescriptionContent(
      task.description,
      task.priority,
      parseKanvibeTaskDescription(content)?.session,
    ),
    sshHost,
  );
}

/** task 세션 정보를 task.json에 기록한다. 이미 기록된 설명과 priority는 남긴다 */
export async function writeKanvibeTaskSession(
  repoPath: string,
  session: KanvibeTaskSession,
  sshHost?: string | null,
): Promise<void> {
  await updateTextFile(
    getKanvibeTaskDescriptionPath(repoPath, sshHost),
    (content) => {
      const existingMetadata = parseKanvibeTaskDescription(content);
      return buildKanvibeTaskDescriptionContent(existingMetadata?.description, existingMetadata?.priority, session);
    },
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

  await updateTextFile(
    getKanvibeProjectStatePath(repoPath, sshHost),
    (content) => buildKanvibeProjectStateContent(normalizedColor, parseKanvibeProjectState(content)?.priority),
    sshHost,
  );
}

export async function writeKanvibeProjectPriority(
  repoPath: string,
  priority: TaskPriority | null,
  sshHost?: string | null,
): Promise<void> {
  const projectStatePath = getKanvibeProjectStatePath(repoPath, sshHost);
  await updateTextFile(
    projectStatePath,
    (content) => buildKanvibeProjectStateContent(parseKanvibeProjectState(content)?.projectColor, priority),
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
 * `.kanvibe`나 그 안의 파일이 심볼릭 링크면 기록을 거부한다.
 * 브랜치가 링크를 추적하면 데스크탑이 worktree 밖의 파일을 덮어쓰거나 권한을 바꾸고, 토큰을 그쪽에 남기게 된다.
 */
export async function assertKanvibeStatePathsAreNotSymlinks(
  repoPath: string,
  fileNames: string[],
  sshHost?: string | null,
): Promise<void> {
  const pathModule = getPathModule(sshHost);
  const stateDirectory = pathModule.join(repoPath, KANVIBE_DIR_NAME);
  const symbolicLinkPath = await findSymbolicLinkPath(
    [stateDirectory, ...fileNames.map((fileName) => pathModule.join(stateDirectory, fileName))],
    sshHost,
  );

  if (symbolicLinkPath) {
    throw new Error(`KanVibe 상태 경로가 심볼릭 링크라 기록하지 않습니다: ${symbolicLinkPath}`);
  }
}

/**
 * hook 알림 대상을 client 단위로 등록한다.
 * 같은 client가 다시 설치하면 그 항목을 교체하고, 새로운 client는 뒤에 추가된다.
 * IP가 바뀐 뒤 재설치하면 url이 달라지므로 client ID가 같은 항목과, client ID 없이 같은 taskId를 가진 옛 항목도
 * 같은 client로 보고 교체한다. taskId는 client가 만드는 값이라 다른 client의 항목과 겹치지 않는다.
 * 옛 항목을 남기면 CLI가 그 주소로 토큰을 먼저 보낸다.
 */
export async function upsertKanvibeHookTarget(
  repoPath: string,
  target: KanvibeHookTarget,
  sshHost?: string | null,
): Promise<void> {
  await assertKanvibeStatePathsAreNotSymlinks(repoPath, [], sshHost);
  await updateTextFile(
    getKanvibeTargetsPath(repoPath, sshHost),
    (content) => buildKanvibeTargetsContent(upsertKanvibeTarget(parseKanvibeTargets(content).targets, target)),
    sshHost,
    TARGETS_FILE_MODE,
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

export function buildKanvibeTaskDescriptionContent(
  description: string | null | undefined,
  priority?: TaskPriority | null,
  session?: KanvibeTaskSession,
): string {
  const payload: KanvibeTaskDescription = {
    schemaVersion: 1,
    ...(description !== undefined ? { description } : {}),
    ...(priority !== undefined ? { priority } : {}),
    ...(session ? { session } : {}),
    updatedAt: new Date().toISOString(),
  };

  return JSON.stringify(payload, null, 2) + "\n";
}

export function buildKanvibeProjectStateContent(
  projectColor: string | undefined,
  priority?: TaskPriority | null,
): string {
  const payload: KanvibeProjectState = {
    schemaVersion: 1,
    ...(projectColor !== undefined ? { projectColor } : {}),
    ...(priority !== undefined ? { priority } : {}),
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

/**
 * description이 문자열이거나 명시적 null일 때만 유효한 기록으로 본다.
 * 파일이 없거나 형식이 깨진 경우는 "정보 없음"이므로 null을 돌려 DB 값을 유지하게 한다.
 */
export function parseKanvibeTaskDescription(content: string): KanvibeTaskDescription | null {
  if (typeof content !== "string" || !content.trim()) {
    return null;
  }

  try {
    const parsed = JSON.parse(content) as { description?: unknown; priority?: unknown; session?: unknown; updatedAt?: unknown };
    const hasDescription = Object.hasOwn(parsed, "description")
      && (typeof parsed.description === "string" || parsed.description === null);
    const hasPriority = Object.hasOwn(parsed, "priority")
      && (parsed.priority === null || parseTaskPriority(parsed.priority) !== null);
    const session = parseKanvibeTaskSession(parsed.session);
    if (!hasDescription && !hasPriority && !session) {
      return null;
    }

    return {
      schemaVersion: 1,
      ...(hasDescription ? { description: parsed.description as string | null } : {}),
      ...(hasPriority ? { priority: parsed.priority === null ? null : parseTaskPriority(parsed.priority)! } : {}),
      ...(session ? { session } : {}),
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
    const parsed = JSON.parse(content) as { projectColor?: unknown; priority?: unknown; updatedAt?: unknown };
    const projectColor = parseProjectColor(parsed.projectColor);
    const hasPriority = Object.hasOwn(parsed, "priority")
      && (parsed.priority === null || parseTaskPriority(parsed.priority) !== null);
    if (!projectColor && !hasPriority) {
      return null;
    }

    return {
      schemaVersion: 1,
      ...(projectColor ? { projectColor } : {}),
      ...(hasPriority ? { priority: parsed.priority === null ? null : parseTaskPriority(parsed.priority)! } : {}),
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

export function parseTaskPriority(value: unknown): TaskPriority | null {
  if (typeof value !== "string") {
    return null;
  }

  return Object.values(TaskPriority).includes(value as TaskPriority) ? value as TaskPriority : null;
}

/** 이 버전이 모르는 세션 타입은 따라갈 수 없으므로 기록이 없는 것으로 본다 */
function parseKanvibeTaskSession(value: unknown): KanvibeTaskSession | null {
  if (!value || typeof value !== "object") {
    return null;
  }

  const { sessionType, sessionName } = value as { sessionType?: unknown; sessionName?: unknown };
  const isKnownSessionType = Object.values(SessionType).includes(sessionType as SessionType);
  if (!isKnownSessionType || !isNonEmptyString(sessionName)) {
    return null;
  }

  return { sessionType: sessionType as SessionType, sessionName };
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
  const isSameClient = (value: KanvibeHookTarget) => value.url === normalizedTarget.url
    || (normalizedTarget.clientId !== undefined && (
      value.clientId === normalizedTarget.clientId
      || (value.clientId === undefined && value.taskId === normalizedTarget.taskId)
    ));
  const targetIndex = nextTargets.findIndex(isSameClient);
  if (targetIndex === -1) {
    return [...nextTargets, normalizedTarget];
  }

  return nextTargets.flatMap((value, index) => {
    if (index === targetIndex) {
      return [normalizedTarget];
    }
    return isSameClient(value) ? [] : [value];
  });
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

  const token = typeof candidate.token === "string" ? candidate.token.trim() : "";
  return {
    url,
    taskId,
    ...(token ? { token } : {}),
    ...(isValidHookClientId(candidate.clientId) ? { clientId: candidate.clientId } : {}),
  };
}

function normalizeKanvibeTargetUrl(url: string): string {
  return url.trim().replace(/\/+$/, "");
}

function getPathModule(sshHost?: string | null): typeof path.posix | typeof path {
  return sshHost ? path.posix : path;
}
