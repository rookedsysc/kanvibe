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
    findOne: vi.fn(),
    findOneBy: vi.fn(),
    save: vi.fn(),
    create: vi.fn(),
  },
  createWorktreeWithSession: vi.fn(),
  broadcastBoardUpdate: vi.fn(),
  broadcastHookStatusTargetMissing: vi.fn(),
  broadcastTaskStatusChanged: vi.fn(),
  cleanupTaskResources: vi.fn(),
  installKanvibeHooks: vi.fn(),
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

vi.mock("@/lib/kanvibeHooksInstaller", () => ({
  installKanvibeHooks: mocks.installKanvibeHooks,
}));

describe("hookService.updateHookTaskStatus", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("taskId로 작업 상태를 변경한다", async () => {
    const { updateHookTaskStatus } = await import("@/desktop/main/services/hookService");
    const project = { id: "project-1", name: "kanvibe" };
    const task = {
      id: "task-1",
      title: "Fix notification",
      description: "debug electron hook",
      branchName: "fix-electron-notification",
      projectId: project.id,
      project,
      status: TaskStatus.PROGRESS,
      sessionType: null,
      sessionName: null,
      worktreePath: null,
      sshHost: null,
    };
    mocks.taskRepo.findOne.mockResolvedValue(task);
    mocks.taskRepo.save.mockImplementation(async (value) => value);

    const result = await updateHookTaskStatus({
      taskId: task.id,
      status: TaskStatus.REVIEW,
    });

    expect(mocks.taskRepo.findOne).toHaveBeenCalledWith({
      where: { id: task.id },
      relations: ["project"],
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

  it("task 식별자가 없으면 400을 반환한다", async () => {
    const { updateHookTaskStatus } = await import("@/desktop/main/services/hookService");
    const result = await updateHookTaskStatus({
      taskId: "",
      status: TaskStatus.REVIEW,
    });

    expect(result).toEqual({
      success: false,
      error: "taskId, status는 필수입니다.",
      status: 400,
    });
  });
});

describe("hookService.startHookTask", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.taskRepo.create.mockImplementation((value) => value);
  });

  it("원격 worktree task를 만들면 hooks를 자동 설치한다", async () => {
    const project = {
      id: "project-1",
      repoPath: "/remote/repo",
      defaultBranch: "main",
      sshHost: "remote-host",
    };
    mocks.projectRepo.findOneBy.mockResolvedValue(project);
    mocks.createWorktreeWithSession.mockResolvedValue({
      worktreePath: "/remote/repo__worktrees/feature-task",
      sessionName: "feature-task",
    });
    mocks.taskRepo.save.mockImplementation(async (value) => ({ id: "task-1", ...value }));

    const { startHookTask } = await import("@/desktop/main/services/hookService");

    await startHookTask({
      title: "remote task",
      branchName: "feature-task",
      sessionType: "tmux" as never,
      sshHost: "remote-host",
      projectId: "project-1",
    });

    expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
      "/remote/repo__worktrees/feature-task",
      "task-1",
      "remote-host",
    );
  });
});
