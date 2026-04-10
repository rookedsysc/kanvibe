import { beforeEach, describe, expect, it, vi } from "vitest";

const entityMocks = vi.hoisted(() => ({
  TaskStatus: {
    TODO: "todo",
    PROGRESS: "progress",
    PENDING: "pending",
    REVIEW: "review",
    DONE: "done",
  },
  SessionType: {
    TMUX: "tmux",
    ZELLIJ: "zellij",
  },
}));

const mocks = vi.hoisted(() => ({
  projectRepo: {
    findOneBy: vi.fn(),
  },
  taskRepo: {
    findOneBy: vi.fn(),
    save: vi.fn(),
    create: vi.fn(),
  },
  createWorktreeWithSession: vi.fn(),
  broadcastBoardUpdate: vi.fn(),
  broadcastHookStatusTargetMissing: vi.fn(),
  broadcastTaskStatusChanged: vi.fn(),
  cleanupTaskResources: vi.fn(),
}));

vi.mock("@/entities/KanbanTask", () => entityMocks);

const { TaskStatus } = entityMocks;

vi.mock("@/lib/database", () => ({
  getProjectRepository: vi.fn(async () => mocks.projectRepo),
  getTaskRepository: vi.fn(async () => mocks.taskRepo),
}));

vi.mock("@/lib/worktree", () => ({
  createWorktreeWithSession: mocks.createWorktreeWithSession,
}));

vi.mock("@/lib/boardNotifier", () => ({
  broadcastBoardUpdate: mocks.broadcastBoardUpdate,
  broadcastHookStatusTargetMissing: mocks.broadcastHookStatusTargetMissing,
  broadcastTaskStatusChanged: mocks.broadcastTaskStatusChanged,
}));

vi.mock("@/desktop/main/services/kanbanService", () => ({
  cleanupTaskResources: mocks.cleanupTaskResources,
}));

describe("hookService.updateHookTaskStatus", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("projectName으로 작업 상태를 변경한다", async () => {
    const { updateHookTaskStatus } = await import("@/desktop/main/services/hookService");
    const project = { id: "project-1", name: "kanvibe" };
    const task = {
      id: "task-1",
      title: "Fix notification",
      description: "debug electron hook",
      branchName: "fix-electron-notification",
      projectId: project.id,
      status: TaskStatus.PROGRESS,
      sessionType: null,
      sessionName: null,
      worktreePath: null,
      sshHost: null,
    };

    mocks.projectRepo.findOneBy.mockResolvedValue(project);
    mocks.taskRepo.findOneBy.mockResolvedValue(task);
    mocks.taskRepo.save.mockImplementation(async (value) => value);

    const result = await updateHookTaskStatus({
      branchName: task.branchName,
      projectName: project.name,
      status: TaskStatus.REVIEW,
    });

    expect(mocks.projectRepo.findOneBy).toHaveBeenCalledWith({ name: project.name });
    expect(mocks.taskRepo.findOneBy).toHaveBeenCalledWith({
      branchName: task.branchName,
      projectId: project.id,
    });
    expect(mocks.broadcastTaskStatusChanged).toHaveBeenCalledWith({
      projectName: project.name,
      branchName: task.branchName,
      taskTitle: task.title,
      description: task.description,
      newStatus: TaskStatus.REVIEW,
      taskId: task.id,
    });
    expect(result).toEqual({
      success: true,
      data: {
        id: task.id,
        status: TaskStatus.REVIEW,
        branchName: task.branchName,
        projectName: project.name,
      },
    });
  });

  it("기존 projectId 필드에 프로젝트명이 들어와도 조회를 이어간다", async () => {
    const { updateHookTaskStatus } = await import("@/desktop/main/services/hookService");
    const project = { id: "project-1", name: "kanvibe" };
    const task = {
      id: "task-1",
      title: "Fix notification",
      description: null,
      branchName: "fix-electron-notification",
      projectId: project.id,
      status: TaskStatus.PROGRESS,
      sessionType: null,
      sessionName: null,
      worktreePath: null,
      sshHost: null,
    };

    mocks.projectRepo.findOneBy.mockResolvedValueOnce(null).mockResolvedValueOnce(project);
    mocks.taskRepo.findOneBy.mockResolvedValue(task);
    mocks.taskRepo.save.mockImplementation(async (value) => value);

    await updateHookTaskStatus({
      branchName: task.branchName,
      projectId: project.name,
      status: TaskStatus.REVIEW,
    });

    expect(mocks.projectRepo.findOneBy).toHaveBeenNthCalledWith(1, { id: project.name });
    expect(mocks.projectRepo.findOneBy).toHaveBeenNthCalledWith(2, { name: project.name });
    expect(mocks.broadcastTaskStatusChanged).toHaveBeenCalledWith(
      expect.objectContaining({ projectName: project.name })
    );
  });

  it("project 식별자가 없으면 400을 반환한다", async () => {
    const { updateHookTaskStatus } = await import("@/desktop/main/services/hookService");
    const result = await updateHookTaskStatus({
      branchName: "fix-electron-notification",
      status: TaskStatus.REVIEW,
    });

    expect(result).toEqual({
      success: false,
      error: "branchName, projectName 또는 projectId, status는 필수입니다.",
      status: 400,
    });
  });
});
