import { TaskStatus, SessionType } from "@/entities/KanbanTask";
import { getProjectRepository, getTaskRepository } from "@/lib/database";
import { createWorktreeWithSession } from "@/lib/worktree";
import { broadcastBoardUpdate, broadcastHookStatusTargetMissing, broadcastTaskStatusChanged } from "@/lib/boardNotifier";
import { cleanupTaskResources } from "@/desktop/main/services/kanbanService";

const STATUS_MAP: Record<string, TaskStatus> = {
  todo: TaskStatus.TODO,
  progress: TaskStatus.PROGRESS,
  pending: TaskStatus.PENDING,
  review: TaskStatus.REVIEW,
  done: TaskStatus.DONE,
};

export interface HookStartInput {
  title: string;
  branchName?: string;
  agentType?: string;
  sessionType?: SessionType;
  sshHost?: string;
  projectId?: string;
  baseBranch?: string;
}

export interface HookStatusInput {
  taskId: string;
  status: string;
}

export async function startHookTask(input: HookStartInput) {
  if (!input.title) {
    return { success: false, error: "title은 필수입니다.", status: 400 };
  }

  const repo = await getTaskRepository();
  const task = repo.create({
    title: input.title,
    branchName: input.branchName || null,
    agentType: input.agentType || null,
    sessionType: input.sessionType || null,
    sshHost: input.sshHost || null,
    projectId: input.projectId || null,
    baseBranch: input.baseBranch || null,
    status: TaskStatus.PROGRESS,
  });

  if (input.branchName && input.sessionType && input.projectId) {
    try {
      const projectRepo = await getProjectRepository();
      const project = await projectRepo.findOneBy({ id: input.projectId });

      if (project) {
        const baseBranch = input.baseBranch || project.defaultBranch;
        const session = await createWorktreeWithSession(
          project.repoPath,
          input.branchName,
          baseBranch,
          input.sessionType,
          project.sshHost,
          input.projectId,
        );
        task.baseBranch = baseBranch;
        task.worktreePath = session.worktreePath;
        task.sessionName = session.sessionName;
        task.sshHost = project.sshHost;
      }
    } catch (error) {
      console.error("Hook worktree/세션 생성 실패:", error);
    }
  }

  const saved = await repo.save(task);
  broadcastBoardUpdate();

  return {
    success: true,
    data: {
      id: saved.id,
      status: saved.status,
      sessionName: saved.sessionName,
    },
  };
}

export async function updateHookTaskStatus(input: HookStatusInput) {
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

  if (taskStatus === TaskStatus.DONE) {
    await cleanupTaskResources(task);
    task.sessionType = null;
    task.sessionName = null;
    task.worktreePath = null;
    task.sshHost = null;
  }

  task.status = taskStatus;
  const saved = await taskRepo.save(task);

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
