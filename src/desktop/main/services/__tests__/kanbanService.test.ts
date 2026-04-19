import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  taskRepo: {
    create: vi.fn(),
    createQueryBuilder: vi.fn(),
    findOneBy: vi.fn(),
    save: vi.fn(),
  },
  projectRepo: {
    findOneBy: vi.fn(),
  },
  execFile: vi.fn(),
  createWorktreeWithSession: vi.fn(),
  removeWorktreeAndBranch: vi.fn(),
  removeSessionOnly: vi.fn(),
  installKanvibeHooks: vi.fn(),
  broadcastBoardUpdate: vi.fn(),
}));

vi.mock("child_process", () => ({
  execFile: mocks.execFile,
  default: {
    execFile: mocks.execFile,
  },
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
  removeWorktreeAndBranch: mocks.removeWorktreeAndBranch,
  createSessionWithoutWorktree: vi.fn(),
  removeSessionOnly: mocks.removeSessionOnly,
  buildManagedWorktreePath: vi.fn((projectPath: string, branchName: string) => `${projectPath}__worktrees/${branchName}`),
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

  it("원격 stale task는 안전하지 않은 worktree/브랜치 삭제를 건너뛴다", async () => {
    // Given
    const consoleWarnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/remote/repo",
      sshHost: "remote-host",
    });

    const { cleanupTaskResources } = await import("@/desktop/main/services/kanbanService");

    // When
    await cleanupTaskResources({
      id: "task-1",
      projectId: "project-1",
      branchName: "dev",
      worktreePath: "/Users/local/repo__worktrees/dev",
      sshHost: "remote-host",
      sessionType: null,
      sessionName: null,
    } as never);

    // Then
    expect(mocks.removeWorktreeAndBranch).not.toHaveBeenCalled();
    expect(consoleWarnSpy).toHaveBeenCalled();
  });

  it("연결된 프로젝트를 찾을 수 없는 원격 stale task는 정리를 시도하지 않는다", async () => {
    // Given
    const consoleWarnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    mocks.projectRepo.findOneBy.mockResolvedValue(null);

    const { cleanupTaskResources } = await import("@/desktop/main/services/kanbanService");

    // When
    await cleanupTaskResources({
      id: "task-2",
      projectId: "missing-project",
      branchName: "main",
      worktreePath: "/home/rookedsysc/Downloads/prompt",
      sshHost: "roky-home",
      sessionType: "tmux" as never,
      sessionName: "prompt-main",
    } as never);

    // Then
    expect(mocks.removeSessionOnly).not.toHaveBeenCalled();
    expect(mocks.removeWorktreeAndBranch).not.toHaveBeenCalled();
    expect(consoleWarnSpy).not.toHaveBeenCalled();
  });

  it("프로젝트가 삭제되어 orphan 상태가 된 원격 stale task도 정리를 시도하지 않는다", async () => {
    // Given
    const consoleWarnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});

    const { cleanupTaskResources } = await import("@/desktop/main/services/kanbanService");

    // When
    await cleanupTaskResources({
      id: "task-3",
      projectId: null,
      branchName: "dev",
      worktreePath: "/home/rookedsysc/Downloads/techtaurant-be",
      sshHost: "roky-home",
      sessionType: "tmux" as never,
      sessionName: "techtaurant-be-dev",
    } as never);

    // Then
    expect(mocks.projectRepo.findOneBy).not.toHaveBeenCalled();
    expect(mocks.removeSessionOnly).not.toHaveBeenCalled();
    expect(mocks.removeWorktreeAndBranch).not.toHaveBeenCalled();
    expect(consoleWarnSpy).not.toHaveBeenCalled();
  });

  it("PR URL 조회는 셸 없이 gh CLI를 직접 실행한다", async () => {
    // Given
    mocks.taskRepo.findOneBy.mockResolvedValue({
      id: "task-4",
      projectId: "project-1",
      branchName: "main",
      prUrl: null,
    });
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
    });
    mocks.taskRepo.save.mockImplementation(async (value) => value);
    mocks.execFile.mockImplementation((file, args, options, callback) => {
      callback(null, "https://github.com/kanvibe/kanvibe/pull/1\n", "");
      return {} as never;
    });

    const { fetchAndSavePrUrl } = await import("@/desktop/main/services/kanbanService");

    // When
    const result = await fetchAndSavePrUrl("task-4");

    // Then
    expect(mocks.execFile).toHaveBeenCalledWith(
      "gh",
      ["pr", "list", "--head", "main", "--json", "url", "-q", ".[0].url"],
      { cwd: "/workspace/repo" },
      expect.any(Function),
    );
    expect(mocks.taskRepo.save).toHaveBeenCalledWith(expect.objectContaining({
      id: "task-4",
      prUrl: "https://github.com/kanvibe/kanvibe/pull/1",
    }));
    expect(result).toBe("https://github.com/kanvibe/kanvibe/pull/1");
  });
});
