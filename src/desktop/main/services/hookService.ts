import { Raw } from "typeorm";
import { TaskStatus } from "@/entities/KanbanTask";
import { getTaskRepository } from "@/lib/database";
import { broadcastBoardUpdate, broadcastHookStatusTargetMissing, broadcastTaskStatusChanged } from "@/lib/boardNotifier";
import { isTaskReachableFromTunnel } from "@/desktop/main/services/hookTunnelService";
import { persistTaskStateForTask } from "@/desktop/main/services/kanvibeTaskStateService";
import { readKanvibeTaskState } from "@/lib/kanvibeProjectState";
import { recordPaneChatSessionBinding } from "@/lib/aiSessions/paneChatSessionBindings";
import { parseBearerToken } from "@/lib/mobilePairing";
import { isValidTaskCliToken } from "@/desktop/main/services/taskCliTokenService";

const STATUS_MAP: Record<string, TaskStatus> = {
  todo: TaskStatus.TODO,
  progress: TaskStatus.PROGRESS,
  pending: TaskStatus.PENDING,
  review: TaskStatus.REVIEW,
  done: TaskStatus.DONE,
};

export interface HookStatusInput {
  taskId: string;
  status: string;
  agentSession?: unknown;
}

const HOOK_AUTHENTICATION_FAILURE = {
  success: false,
  error: "hook 인증에 실패했습니다. KanVibe에서 이 task에 접속하면 hooks가 다시 설치됩니다.",
  status: 401,
} as const;

/**
 * 상태 hook도 task CLI와 같은 task 토큰으로 인증한다. 토큰은 그 worktree의 targets.json을 읽을 수 있어야 얻으므로,
 * 소켓이나 평문 포트에 닿기만 한 프로세스가 아무 task의 상태를 바꾸지 못한다.
 */
async function isAuthorizedHookRequest(taskId: string, authorizationHeader: string | undefined): Promise<boolean> {
  const token = parseBearerToken(authorizationHeader);
  return token !== null && await isValidTaskCliToken(taskId, token);
}

/**
 * @param authorizationHeader `Bearer <task 토큰>`. 없거나 다른 task의 토큰이면 task를 조회하지 않고 401로 거절한다
 * @param sourceSshHosts 요청이 들어온 터널이 맡은 호스트 별칭들. 터널로 온 요청은 그 호스트의 task만 바꿀 수 있다
 */
export async function updateHookTaskStatus(
  input: HookStatusInput,
  authorizationHeader: string | undefined,
  sourceSshHosts?: ReadonlySet<string>,
) {
  if (!input.taskId || !input.status) {
    return {
      success: false,
      error: "taskId, status는 필수입니다.",
      status: 400,
    };
  }
  if (!await isAuthorizedHookRequest(input.taskId, authorizationHeader)) {
    return HOOK_AUTHENTICATION_FAILURE;
  }

  const taskStatus = STATUS_MAP[input.status.toLowerCase()];
  if (!taskStatus) {
    return {
      success: false,
      error: `유효하지 않은 상태입니다: ${input.status}`,
      status: 400,
    };
  }

  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({
    where: { id: input.taskId },
    relations: ["project"],
  });

  if (task && !isTaskReachableFromTunnel(task, task.project, sourceSshHosts)) {
    return {
      success: false,
      error: `작업을 찾을 수 없습니다: ${input.taskId}`,
      status: 404,
    };
  }

  if (!task) {
    broadcastHookStatusTargetMissing({
      taskId: input.taskId,
      requestedStatus: taskStatus,
      reason: "task-not-found",
    });

    return {
      success: false,
      error: `작업을 찾을 수 없습니다: ${input.taskId}`,
      status: 404,
    };
  }

  const projectName = task.project?.name || task.projectId || "Unknown project";
  const sshHost = task.sshHost || task.project?.sshHost || null;
  if (input.agentSession && (!sshHost || sourceSshHosts?.has(sshHost))) {
    await recordPaneChatSessionBinding({
      taskId: task.id,
      worktreePath: task.worktreePath || task.project?.repoPath || null,
      repoPath: task.project?.repoPath ?? null,
      sshHost,
    }, input.agentSession);
  }
  task.status = taskStatus;

  const saved = await taskRepo.save(task);

  await persistTaskStateForTask(saved);

  broadcastBoardUpdate();
  broadcastTaskStatusChanged({
    projectName,
    branchName: task.branchName || "",
    taskTitle: saved.title,
    description: saved.description,
    newStatus: taskStatus,
    taskId: saved.id,
  });

  return {
    success: true,
    data: {
      id: saved.id,
      status: saved.status,
      branchName: saved.branchName,
      projectName,
    },
  };
}

/** 재전달은 현재 원격 상태를 확인하고 DB만 갱신해 오래된 hook이 상태 파일을 되돌리지 않게 한다. */
export async function updateJournalHookTaskStatus(
  input: HookStatusInput,
  authorizationHeader: string | undefined,
  sourceSshHosts: ReadonlySet<string>,
) {
  if (!sourceSshHosts?.size || !input || typeof input.taskId !== "string" || !input.taskId || typeof input.status !== "string") {
    return { success: false, error: "Invalid journal hook scope or input", status: 400 };
  }
  if (!await isAuthorizedHookRequest(input.taskId, authorizationHeader)) {
    return HOOK_AUTHENTICATION_FAILURE;
  }
  const statusKey = input.status.toLowerCase();
  const taskStatus = Object.hasOwn(STATUS_MAP, statusKey) ? STATUS_MAP[statusKey] : undefined;
  if (!taskStatus) return { success: false, error: "Invalid journal hook status", status: 400 };
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({ where: { id: input.taskId }, relations: ["project"] });
  if (!task || !isTaskReachableFromTunnel(task, task.project, sourceSshHosts)) {
    return { success: false, error: "Journal hook target not found", status: 404 };
  }
  if (task.status === taskStatus) return { success: true, delivery: "ignored" as const };
  const sshHost = task.sshHost ?? task.project?.sshHost;
  const repoPath = task.worktreePath || (task.branchName === task.project?.defaultBranch ? task.project?.repoPath : null);
  if (!sshHost || !repoPath) return { success: false, error: "Journal hook target has no remote state", status: 404 };
  let currentState;
  try { currentState = await readKanvibeTaskState(repoPath, sshHost); }
  catch { return { success: false, error: "Journal hook current state unavailable", status: 503 }; }
  if (!currentState) return { success: false, error: "Journal hook current state unavailable", status: 503 };
  if (currentState.status !== taskStatus) return { success: true, delivery: "ignored" as const };
  const updatedAt = Raw((column) => `julianday(${column}) = julianday(:journalSnapshotUpdatedAt)`, { journalSnapshotUpdatedAt: task.updatedAt.toISOString() });
  const result = await taskRepo.update({ id: task.id, status: task.status, updatedAt }, { status: taskStatus });
  if (!result.affected) return { success: true, delivery: "ignored" as const };
  if (input.agentSession) {
    await recordPaneChatSessionBinding({ taskId: task.id, worktreePath: repoPath, repoPath: task.project?.repoPath ?? null, sshHost }, input.agentSession);
  }
  broadcastBoardUpdate();
  broadcastTaskStatusChanged({
    projectName: task.project?.name || task.projectId || "Unknown project",
    branchName: task.branchName || "",
    taskTitle: task.title,
    description: task.description,
    newStatus: taskStatus,
    taskId: task.id,
  });
  return { success: true, delivery: "applied" as const };
}
