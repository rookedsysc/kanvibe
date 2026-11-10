import { TaskStatus } from "@/entities/KanbanTask";
import { getTaskRepository } from "@/lib/database";
import { broadcastBoardUpdate, broadcastHookStatusTargetMissing, broadcastTaskStatusChanged } from "@/lib/boardNotifier";
import { isTaskReachableFromTunnel } from "@/desktop/main/services/hookTunnelService";
import { persistTaskStateForTask } from "@/desktop/main/services/kanvibeTaskStateService";

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
