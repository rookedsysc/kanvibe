import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  taskRepo: {
    create: vi.fn(),
    createQueryBuilder: vi.fn(),
    save: vi.fn(),
  },
  projectRepo: {
    findOneBy: vi.fn(),
  },
  createWorktreeWithSession: vi.fn(),
  installKanvibeHooks: vi.fn(),
  broadcastBoardUpdate: vi.fn(),
}));

vi.mock("@/lib/database", () => ({
  getTaskRepository: vi.fn(async () => mocks.taskRepo),
  getProjectRepository: vi.fn(async () => mocks.projectRepo),
}));

vi.mock("@/entities/KanbanTask", () => ({
  TaskStatus: {
    TODO: "todo",
  },
  SessionType: {
    TMUX: "tmux",
    ZELLIJ: "zellij",
  },
}));

vi.mock("@/entities/TaskPriority", () => ({
  TaskPriority: {},
}));

vi.mock("@/lib/worktree", () => ({
  createWorktreeWithSession: mocks.createWorktreeWithSession,
  removeWorktreeAndBranch: vi.fn(),
  createSessionWithoutWorktree: vi.fn(),
  removeSessionOnly: vi.fn(),
}));

vi.mock("@/lib/kanvibeHooksInstaller", () => ({
  installKanvibeHooks: mocks.installKanvibeHooks,
}));

vi.mock("@/lib/boardNotifier", () => ({
  broadcastBoardUpdate: mocks.broadcastBoardUpdate,
}));

describe("kanbanService.createTask", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.taskRepo.create.mockImplementation((value) => value);
    mocks.taskRepo.createQueryBuilder.mockReturnValue({
      select: vi.fn().mockReturnThis(),
      where: vi.fn().mockReturnThis(),
      getRawOne: vi.fn().mockResolvedValue({ max: 2 }),
    });
  });

  it("로컬 worktree 태스크를 만들면 hooks를 자동 설치한다", async () => {
    // Given
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
      defaultBranch: "main",
      sshHost: null,
    });
    mocks.createWorktreeWithSession.mockResolvedValue({
      worktreePath: "/workspace/repo-worktrees/task-1",
      sessionName: "task-1",
    });
    mocks.taskRepo.save.mockImplementation(async (value) => ({ id: "task-1", ...value }));

    const { createTask } = await import("@/desktop/main/services/kanbanService");

    // When
    await createTask({
      title: "알림 회귀 수정",
      branchName: "fix/notifications",
      projectId: "project-1",
      sessionType: "tmux" as never,
    });

    // Then
    expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
      "/workspace/repo-worktrees/task-1",
      "task-1",
      null,
    );
  });
});
