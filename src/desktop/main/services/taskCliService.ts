import { SessionType, type KanbanTask } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import { TaskPriority } from "@/entities/TaskPriority";
import { broadcastBoardUpdate } from "@/lib/boardNotifier";
import { getProjectRepository, getTaskRepository } from "@/lib/database";
import { isSafeGitRef } from "@/lib/gitRef";
import { parseBearerToken } from "@/lib/mobilePairing";
import {
  createTask,
  deleteTaskAcrossTargets,
  DirtyWorktreeError,
  getTaskIdByProjectAndBranch,
} from "@/desktop/main/services/kanbanService";
import { isTaskReachableFromTunnel } from "@/desktop/main/services/hookTunnelService";
import { syncRegisteredProjectWorktrees } from "@/desktop/main/services/projectService";
import { isValidTaskCliToken } from "@/desktop/main/services/taskCliTokenService";
import {
  propagateTaskRegistrationToPeers,
  type PeerTargetResult,
} from "@/desktop/main/services/taskTargetPropagationService";
import { prepareTaskTerminal } from "@/desktop/main/terminalBridge";

/** CLI 요청을 거절할 때 HTTP 응답 코드와 함께 던진다 */
export class TaskCliRequestError extends Error {
  constructor(message: string, readonly statusCode: number) {
    super(message);
    this.name = "TaskCliRequestError";
  }
}

/** 에이전트가 화면과 같은 세션에 접속하는 데 쓰는 준비 결과. 준비에 실패하면 error만 있고 task는 그대로 남는다 */
export interface CliTaskSession {
  isPrepared: boolean;
  skippedReason?: string;
  error?: string;
  herdrWorkspaceId?: string;
  herdrPanes?: { paneId: string; tabId: string }[];
}

export interface CreatedCliTask {
  id: string;
  projectId: string | null;
  branchName: string | null;
  baseBranch: string | null;
  worktreePath: string | null;
  sessionType: SessionType | null;
  sessionName: string | null;
  sshHost: string | null;
  status: string;
  session: CliTaskSession;
  peers: PeerTargetResult[];
}

export interface PreparedCliTaskSession {
  id: string;
  branchName: string | null;
  sessionType: SessionType | null;
  sessionName: string | null;
  sshHost: string | null;
  session: CliTaskSession;
}

export interface ListedCliTask {
  id: string;
  title: string;
  branchName: string | null;
  status: string;
  priority: TaskPriority | null;
  worktreePath: string | null;
  sessionType: SessionType | null;
  sessionName: string | null;
  sshHost: string | null;
}

export interface DeletedCliTask {
  id: string;
  branchName: string | null;
  deleted: true;
  peers: PeerTargetResult[];
}

export interface RegisteredPeerTask {
  id: string;
  branchName: string;
}

/** 요청을 보낸 worktree의 task. 이 task가 속한 프로젝트 안에서만 생성·삭제를 허용한다 */
interface AuthorizedTaskContext {
  contextTask: KanbanTask;
  project: Project;
}

interface CreationBase {
  baseBranch: string;
  inheritedSessionType: SessionType | null;
}

type CliRequestBody = Record<string, unknown>;

/**
 * 요청한 worktree의 브랜치(또는 지정한 task의 브랜치)에서 새 worktree와 task를 이 client에 만들고,
 * 같은 프로젝트를 다루는 다른 client에는 task 등록만 전파한다.
 */
export async function createTaskFromCli(
  authorizationHeader: string | undefined,
  body: CliRequestBody,
  sourceSshHosts?: ReadonlySet<string>,
): Promise<CreatedCliTask> {
  const { contextTask, project } = await authorizeTaskContext(authorizationHeader, body, sourceSshHosts);
  const branchName = readGitRefField(body, "branchName");
  const title = readOptionalStringField(body, "title");
  const description = readOptionalStringField(body, "description");
  const priority = readPriorityField(body);
  const requestedSessionType = readSessionTypeField(body);
  const creationBase = await resolveCreationBase(body, contextTask, project);

  if (await getTaskIdByProjectAndBranch(project.id, branchName)) {
    throw new TaskCliRequestError(`이미 같은 브랜치의 task가 있습니다: ${branchName}`, 409);
  }

  const createdTask = await createTask({
    title: title || branchName,
    description,
    priority,
    branchName,
    baseBranch: creationBase.baseBranch,
    sessionType: requestedSessionType ?? creationBase.inheritedSessionType ?? SessionType.TERMINAL,
    projectId: project.id,
  });
  const peers = await propagateTaskRegistrationToPeers(project.repoPath, branchName, project.sshHost);
  const createdTaskEntity = await findTaskInProject({ id: createdTask.id }, project, `만든 task를 찾을 수 없습니다: ${createdTask.id}`);
  const session = await prepareCliTaskSession(createdTaskEntity);

  return {
    id: createdTask.id,
    projectId: createdTask.projectId,
    branchName: createdTask.branchName,
    baseBranch: createdTask.baseBranch,
    worktreePath: createdTask.worktreePath,
    sessionType: createdTask.sessionType,
    sessionName: createdTaskEntity.sessionName,
    sshHost: createdTaskEntity.sshHost,
    status: createdTask.status,
    session,
    peers,
  };
}

/** 같은 프로젝트의 task를 돌려준다. 토큰 하나로 볼 수 있는 범위를 생성·삭제와 같은 프로젝트 안으로 묶는다 */
export async function listTasksFromCli(
  authorizationHeader: string | undefined,
  body: CliRequestBody,
  sourceSshHosts?: ReadonlySet<string>,
): Promise<ListedCliTask[]> {
  const { project } = await authorizeTaskContext(authorizationHeader, body, sourceSshHosts);
  const projectTasks = await (await getTaskRepository()).find({ where: { projectId: project.id }, order: { createdAt: "ASC" } });

  return projectTasks.map((task) => ({
    id: task.id,
    title: task.title,
    branchName: task.branchName,
    status: task.status,
    priority: task.priority,
    worktreePath: task.worktreePath,
    sessionType: task.sessionType,
    sessionName: task.sessionName,
    sshHost: task.sshHost,
  }));
}

/**
 * task의 세션을 준비한다. 브랜치를 주지 않으면 요청한 worktree의 task가 대상이다.
 * 세션 생성 전에 만든 task나 준비가 실패했던 task를 에이전트가 다시 준비할 때 쓴다.
 */
export async function prepareTaskSessionFromCli(
  authorizationHeader: string | undefined,
  body: CliRequestBody,
  sourceSshHosts?: ReadonlySet<string>,
): Promise<PreparedCliTaskSession> {
  const { contextTask, project } = await authorizeTaskContext(authorizationHeader, body, sourceSshHosts);
  const targetTask = await resolveBranchTarget(body, contextTask, project);
  const session = await prepareCliTaskSession(targetTask);

  return {
    id: targetTask.id,
    branchName: targetTask.branchName,
    sessionType: targetTask.sessionType,
    sessionName: targetTask.sessionName,
    sshHost: targetTask.sshHost,
    session,
  };
}

/** 세션 준비 실패는 task 생성 실패가 아니므로 응답에만 남기고, 에이전트가 session 명령으로 다시 시도하게 한다 */
async function prepareCliTaskSession(task: KanbanTask): Promise<CliTaskSession> {
  if (!task.sessionType || !task.sessionName) {
    return { isPrepared: false, error: "task에 연결된 세션이 없습니다." };
  }

  try {
    return await prepareTaskTerminal(task);
  } catch (error) {
    console.error("[task-cli] 세션을 준비하지 못했습니다", { taskId: task.id, error });
    return { isPrepared: false, error: error instanceof Error ? error.message : "세션을 준비하지 못했습니다." };
  }
}

/**
 * task를 삭제한다. 브랜치를 주지 않으면 요청한 worktree의 task 자신이 대상이다.
 * taskId는 client마다 다르므로 다른 task는 같은 프로젝트 안의 브랜치 이름으로 가리킨다.
 */
export async function deleteTaskFromCli(
  authorizationHeader: string | undefined,
  body: CliRequestBody,
  sourceSshHosts?: ReadonlySet<string>,
): Promise<DeletedCliTask> {
  const { contextTask, project } = await authorizeTaskContext(authorizationHeader, body, sourceSshHosts);
  if (body.confirmed !== true) {
    throw new TaskCliRequestError("삭제를 확인하는 confirmed: true가 필요합니다.", 400);
  }

  const targetTask = await resolveBranchTarget(body, contextTask, project);
  if (targetTask.branchName === project.defaultBranch) {
    throw new TaskCliRequestError(`프로젝트 기본 브랜치(${project.defaultBranch})의 task는 CLI로 지울 수 없습니다. KanVibe 화면에서 지우세요.`, 403);
  }

  try {
    const deletion = await deleteTaskAcrossTargets(targetTask.id, {
      shouldRejectDirtyWorktree: body.force !== true,
      shouldPropagateToPeers: body.propagateToPeers !== false,
    });

    if (!deletion.isDeleted) {
      throw new TaskCliRequestError(`task를 찾을 수 없습니다: ${targetTask.id}`, 404);
    }

    return { id: targetTask.id, branchName: targetTask.branchName, deleted: true, peers: deletion.peerResults };
  } catch (error) {
    if (error instanceof DirtyWorktreeError) {
      throw new TaskCliRequestError(`${error.message} 그래도 지우려면 --force를 붙이세요.`, 409);
    }
    throw error;
  }
}

/**
 * 다른 client가 만든 worktree를 이 client의 task로 등록한다. worktree는 만들지 않는다.
 * 배경 동기화가 새 worktree를 등록하는 경로를 그대로 써서 hooks 설치와 상태 복원이 같은 규칙을 따르게 한다.
 */
export async function registerTaskFromPeer(
  authorizationHeader: string | undefined,
  body: CliRequestBody,
  sourceSshHosts?: ReadonlySet<string>,
): Promise<RegisteredPeerTask> {
  const { project } = await authorizeTaskContext(authorizationHeader, body, sourceSshHosts);
  const branchName = readGitRefField(body, "branchName");

  const existingTaskId = await getTaskIdByProjectAndBranch(project.id, branchName);
  if (existingTaskId) {
    return { id: existingTaskId, branchName };
  }

  const syncResult = await syncRegisteredProjectWorktrees([project]);
  if (syncResult.changed) {
    broadcastBoardUpdate();
  }

  const registeredTaskId = await getTaskIdByProjectAndBranch(project.id, branchName);
  if (!registeredTaskId) {
    throw new TaskCliRequestError(`프로젝트에서 ${branchName} 브랜치의 worktree를 찾지 못했습니다.`, 404);
  }

  return { id: registeredTaskId, branchName };
}

/**
 * 토큰을 먼저 확인해 자격 없는 요청에는 task가 있는지조차 알려 주지 않는다.
 * 터널로 온 요청은 그 터널 호스트의 task 토큰만 인정한다. 다른 호스트에서 새어 나온 토큰으로 이 호스트의 task를 다루지 못하게 한다.
 */
async function authorizeTaskContext(
  authorizationHeader: string | undefined,
  body: CliRequestBody,
  sourceSshHosts: ReadonlySet<string> | undefined,
): Promise<AuthorizedTaskContext> {
  const contextTaskId = readRequiredStringField(body, "contextTaskId");
  const token = parseBearerToken(authorizationHeader);
  if (!token || !(await checkTaskCliToken(contextTaskId, token))) {
    throw new TaskCliRequestError("task CLI 인증에 실패했습니다. KanVibe에서 hooks를 다시 설치하세요.", 401);
  }

  const contextTask = await (await getTaskRepository()).findOneBy({ id: contextTaskId });
  if (!contextTask) {
    throw new TaskCliRequestError(`task를 찾을 수 없습니다: ${contextTaskId}`, 404);
  }

  const project = contextTask.projectId
    ? await (await getProjectRepository()).findOneBy({ id: contextTask.projectId })
    : null;
  if (!isTaskReachableFromTunnel(contextTask, project, sourceSshHosts)) {
    throw new TaskCliRequestError("task CLI 인증에 실패했습니다. KanVibe에서 hooks를 다시 설치하세요.", 401);
  }
  if (!project) {
    throw new TaskCliRequestError("프로젝트에 연결되지 않은 task에서는 task를 만들거나 지울 수 없습니다.", 409);
  }

  return { contextTask, project };
}

/**
 * 토큰 검증 중의 내부 오류는 인증 전이라 요청자가 누구인지 모른다.
 * 라우팅은 오류 메시지를 그대로 돌려주므로, 저장소 경로 같은 원인은 로그에만 남기고 감춘 메시지로 바꾼다.
 */
async function checkTaskCliToken(contextTaskId: string, token: string): Promise<boolean> {
  try {
    return await isValidTaskCliToken(contextTaskId, token);
  } catch (error) {
    console.error("[task-cli] CLI 토큰을 확인하지 못했습니다", error);
    throw new TaskCliRequestError("task CLI 인증을 확인하지 못했습니다. 잠시 뒤 다시 시도하세요.", 500);
  }
}

async function resolveCreationBase(
  body: CliRequestBody,
  contextTask: KanbanTask,
  project: Project,
): Promise<CreationBase> {
  const fromTaskId = readOptionalStringField(body, "fromTaskId");
  if (!fromTaskId) {
    return {
      baseBranch: readGitRefField(body, "baseBranch"),
      inheritedSessionType: contextTask.sessionType,
    };
  }

  const fromTask = await findTaskInProject({ id: fromTaskId }, project, `기준 task를 찾을 수 없습니다: ${fromTaskId}`);
  if (!fromTask.branchName || !isSafeGitRef(fromTask.branchName)) {
    throw new TaskCliRequestError("기준 task에 기준으로 쓸 수 있는 브랜치가 없습니다.", 409);
  }

  return { baseBranch: fromTask.branchName, inheritedSessionType: fromTask.sessionType };
}

async function resolveBranchTarget(
  body: CliRequestBody,
  contextTask: KanbanTask,
  project: Project,
): Promise<KanbanTask> {
  if (body.branchName === undefined) {
    return contextTask;
  }

  const branchName = readGitRefField(body, "branchName");
  return findTaskInProject({ branchName }, project, `${branchName} 브랜치의 task를 찾을 수 없습니다.`);
}

/** 다른 프로젝트의 task는 없는 것과 같게 다뤄 토큰 하나로 다룰 수 있는 범위를 프로젝트 안으로 묶는다 */
async function findTaskInProject(
  where: { id: string } | { branchName: string },
  project: Project,
  notFoundMessage: string,
): Promise<KanbanTask> {
  const task = await (await getTaskRepository()).findOneBy({ ...where, projectId: project.id });
  if (!task) {
    throw new TaskCliRequestError(notFoundMessage, 404);
  }
  return task;
}

function readRequiredStringField(body: CliRequestBody, fieldName: string): string {
  const value = readOptionalStringField(body, fieldName);
  if (!value) {
    throw new TaskCliRequestError(`${fieldName}는 필수입니다.`, 400);
  }
  return value;
}

function readOptionalStringField(body: CliRequestBody, fieldName: string): string | undefined {
  const value = body[fieldName];
  if (value === undefined || value === null) {
    return undefined;
  }
  if (typeof value !== "string") {
    throw new TaskCliRequestError(`${fieldName}는 문자열이어야 합니다.`, 400);
  }
  return value.trim() || undefined;
}

function readGitRefField(body: CliRequestBody, fieldName: string): string {
  const value = readRequiredStringField(body, fieldName);
  if (!isSafeGitRef(value)) {
    throw new TaskCliRequestError(`${fieldName}에 쓸 수 없는 문자가 있습니다: ${value}`, 400);
  }
  return value;
}

function readPriorityField(body: CliRequestBody): TaskPriority | undefined {
  const value = readOptionalStringField(body, "priority");
  if (value === undefined) {
    return undefined;
  }

  const priority = Object.values(TaskPriority).find((candidate) => candidate === value);
  if (!priority) {
    throw new TaskCliRequestError(`priority 값이 올바르지 않습니다: ${value} (low, medium, high 중 하나)`, 400);
  }
  return priority;
}

function readSessionTypeField(body: CliRequestBody): SessionType | undefined {
  const value = readOptionalStringField(body, "sessionType");
  if (value === undefined) {
    return undefined;
  }

  const sessionType = Object.values(SessionType).find((candidate) => candidate === value);
  if (!sessionType) {
    throw new TaskCliRequestError(`sessionType 값이 올바르지 않습니다: ${value}`, 400);
  }
  return sessionType;
}
