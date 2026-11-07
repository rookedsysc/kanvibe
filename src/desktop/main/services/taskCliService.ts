import { SessionType, type KanbanTask } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
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
import { syncRegisteredProjectWorktrees } from "@/desktop/main/services/projectService";
import { isValidTaskCliToken } from "@/desktop/main/services/taskCliTokenService";
import {
  propagateTaskRegistrationToPeers,
  type PeerTargetResult,
} from "@/desktop/main/services/taskTargetPropagationService";

/** CLI 요청을 거절할 때 HTTP 응답 코드와 함께 던진다 */
export class TaskCliRequestError extends Error {
  constructor(message: string, readonly statusCode: number) {
    super(message);
    this.name = "TaskCliRequestError";
  }
}

export interface CreatedCliTask {
  id: string;
  projectId: string | null;
  branchName: string | null;
  baseBranch: string | null;
  worktreePath: string | null;
  sessionType: SessionType | null;
  status: string;
  peers: PeerTargetResult[];
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
): Promise<CreatedCliTask> {
  const { contextTask, project } = await authorizeTaskContext(authorizationHeader, body);
  const branchName = readGitRefField(body, "branchName");
  const title = readOptionalStringField(body, "title");
  const requestedSessionType = readSessionTypeField(body);
  const creationBase = await resolveCreationBase(body, contextTask, project);

  if (await getTaskIdByProjectAndBranch(project.id, branchName)) {
    throw new TaskCliRequestError(`이미 같은 브랜치의 task가 있습니다: ${branchName}`, 409);
  }

  const createdTask = await createTask({
    title: title || branchName,
    branchName,
    baseBranch: creationBase.baseBranch,
    sessionType: requestedSessionType ?? creationBase.inheritedSessionType ?? SessionType.TERMINAL,
    projectId: project.id,
  });
  const peers = await propagateTaskRegistrationToPeers(project.repoPath, branchName, project.sshHost);

  return {
    id: createdTask.id,
    projectId: createdTask.projectId,
    branchName: createdTask.branchName,
    baseBranch: createdTask.baseBranch,
    worktreePath: createdTask.worktreePath,
    sessionType: createdTask.sessionType,
    status: createdTask.status,
    peers,
  };
}

/**
 * task를 삭제한다. 브랜치를 주지 않으면 요청한 worktree의 task 자신이 대상이다.
 * taskId는 client마다 다르므로 다른 task는 같은 프로젝트 안의 브랜치 이름으로 가리킨다.
 */
export async function deleteTaskFromCli(
  authorizationHeader: string | undefined,
  body: CliRequestBody,
): Promise<DeletedCliTask> {
  const { contextTask, project } = await authorizeTaskContext(authorizationHeader, body);
  if (body.confirmed !== true) {
    throw new TaskCliRequestError("삭제를 확인하는 confirmed: true가 필요합니다.", 400);
  }

  const targetTask = await resolveDeletionTarget(body, contextTask, project);

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
): Promise<RegisteredPeerTask> {
  const { project } = await authorizeTaskContext(authorizationHeader, body);
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

/** 토큰을 먼저 확인해 자격 없는 요청에는 task가 있는지조차 알려 주지 않는다 */
async function authorizeTaskContext(
  authorizationHeader: string | undefined,
  body: CliRequestBody,
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

async function resolveDeletionTarget(
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
