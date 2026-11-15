import { Raw } from "typeorm";
import { TaskStatus } from "@/entities/KanbanTask";
import { getTaskRepository } from "@/lib/database";
import { broadcastBoardUpdate, broadcastHookStatusTargetMissing, broadcastTaskStatusChanged } from "@/lib/boardNotifier";
import { isTaskReachableFromTunnel } from "@/desktop/main/services/hookTunnelService";
import { persistTaskStateForTask } from "@/desktop/main/services/kanvibeTaskStateService";
import { readKanvibeTaskState } from "@/lib/kanvibeProjectState";
import { recordPaneChatSessionBinding } from "@/lib/aiSessions/paneChatSessionBindings";

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

/**
 * @param sourceSshHosts 요청이 들어온 터널이 맡은 호스트 별칭들. 터널로 온 요청은 그 호스트의 task만 바꿀 수 있다
 */
export async function updateHookTaskStatus(input: HookStatusInput, sourceSshHosts?: ReadonlySet<string>) {
  if (!input.taskId || !input.status) {
    return {
      success: false,
      error: "taskId, status는 필수입니다.",
      status: 400,
    };
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
export async function updateJournalHookTaskStatus(input: HookStatusInput, sourceSshHosts: ReadonlySet<string>) {
  if (!sourceSshHosts?.size || !input || typeof input.taskId !== "string" || !input.taskId || typeof input.status !== "string") {
    return { success: false, error: "Invalid journal hook scope or input", status: 400 };
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
