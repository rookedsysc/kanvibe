import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  taskRepo: {
    create: vi.fn(),
    createQueryBuilder: vi.fn(),
    find: vi.fn(),
    findOneBy: vi.fn(),
    remove: vi.fn(),
    save: vi.fn(),
    update: vi.fn(),
  },
  projectRepo: {
    find: vi.fn(),
    findOneBy: vi.fn(),
    save: vi.fn(),
  },
  execFile: vi.fn(),
  createWorktreeWithSession: vi.fn(),
  createSessionWithoutWorktree: vi.fn(),
  removeWorktreeAndBranch: vi.fn(),
  removeSessionOnly: vi.fn(),
  detachSession: vi.fn(),
  installKanvibeHooks: vi.fn(),
  installKanvibeHookFiles: vi.fn(),
  scheduleKanvibeHooksVerification: vi.fn(),
  scheduleKanvibeHooksInstall: vi.fn(),
  broadcastBoardUpdate: vi.fn(),
  execGit: vi.fn(),
  pullCurrentBranch: vi.fn(),
  remoteBranchExists: vi.fn(),
  broadcastTaskHookInstallFailed: vi.fn(),
  broadcastTaskPrMergedDetectedBatch: vi.fn(),
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

vi.mock("@/entities/TaskPriority", () => ({
  TaskPriority: {},
}));

vi.mock("@/lib/worktree", () => ({
  createWorktreeWithSession: mocks.createWorktreeWithSession,
  removeWorktreeAndBranch: mocks.removeWorktreeAndBranch,
  createSessionWithoutWorktree: mocks.createSessionWithoutWorktree,
  removeSessionOnly: mocks.removeSessionOnly,
}));

vi.mock("@/lib/terminal", () => ({
  detachSession: mocks.detachSession,
}));

vi.mock("@/lib/kanvibeHooksInstaller", () => ({
  installKanvibeHooks: mocks.installKanvibeHooks,
  installKanvibeHookFiles: mocks.installKanvibeHookFiles,
  scheduleKanvibeHooksVerification: mocks.scheduleKanvibeHooksVerification,
  scheduleKanvibeHooksInstall: mocks.scheduleKanvibeHooksInstall,
}));

vi.mock("@/lib/boardNotifier", () => ({
  broadcastBoardUpdate: mocks.broadcastBoardUpdate,
  broadcastTaskHookInstallFailed: mocks.broadcastTaskHookInstallFailed,
  broadcastTaskPrMergedDetectedBatch: mocks.broadcastTaskPrMergedDetectedBatch,
}));

vi.mock("@/lib/gitOperations", () => ({
  execGit: mocks.execGit,
  pullCurrentBranch: mocks.pullCurrentBranch,
  remoteBranchExists: mocks.remoteBranchExists,
}));

function nextMacrotask<T>(value: T): Promise<T> {
  return new Promise((resolve) => {
    setTimeout(() => resolve(value), 0);
  });
}

describe("kanbanService.createTask", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.taskRepo.create.mockImplementation((value) => value);
    mocks.createSessionWithoutWorktree.mockResolvedValue({ sessionName: "repo-feature-remote" });
    mocks.installKanvibeHooks.mockResolvedValue(undefined);
    mocks.installKanvibeHookFiles.mockResolvedValue(undefined);
    mocks.scheduleKanvibeHooksVerification.mockImplementation(() => {});
    mocks.scheduleKanvibeHooksInstall.mockImplementation((
      targetPath: string,
      taskId: string,
      sshHost: string | null | undefined,
      options: { delayMs?: number; onSuccess?: () => void; onFailure?: (error: unknown) => void } = {},
    ) => {
      setTimeout(() => {
        void mocks.installKanvibeHooks(targetPath, taskId, sshHost)
          .then(() => options.onSuccess?.())
          .catch((error: unknown) => options.onFailure?.(error));
      }, options.delayMs ?? 0);
    });
    mocks.removeSessionOnly.mockResolvedValue(undefined);
    mocks.removeWorktreeAndBranch.mockResolvedValue(undefined);
    mocks.remoteBranchExists.mockResolvedValue(true);
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
    expect(mocks.installKanvibeHookFiles).toHaveBeenCalledWith(
      "/workspace/repo-worktrees/task-1",
      "task-1",
      null,
    );
    expect(mocks.scheduleKanvibeHooksVerification).toHaveBeenCalledWith(
      "/workspace/repo-worktrees/task-1",
      "task-1",
      null,
      expect.objectContaining({
        onSuccess: expect.any(Function),
        onFailure: expect.any(Function),
      }),
    );
    expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
  });

  it("worktree 태스크 생성은 hooks 설치 완료를 기다린 뒤 반환한다", async () => {
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
    let resolveInstall: () => void = () => {};
    mocks.installKanvibeHookFiles.mockImplementation(() => new Promise<void>((resolve) => {
      resolveInstall = resolve;
    }));
    mocks.taskRepo.save.mockImplementation(async (value) => ({ id: "task-1", ...value }));

    const { createTask } = await import("@/desktop/main/services/kanbanService");

    // When
    let resolved = false;
    const resultPromise = createTask({
      title: "알림 회귀 수정",
      branchName: "fix/notifications",
      projectId: "project-1",
      sessionType: "tmux" as never,
    }).then((result) => {
      resolved = true;
      return result;
    });
    for (let index = 0; index < 6; index += 1) {
      await Promise.resolve();
    }

    // Then
    expect(mocks.installKanvibeHookFiles).toHaveBeenCalledWith(
      "/workspace/repo-worktrees/task-1",
      "task-1",
      null,
    );
    expect(resolved).toBe(false);
    expect(mocks.broadcastBoardUpdate).not.toHaveBeenCalled();
    expect(mocks.scheduleKanvibeHooksVerification).not.toHaveBeenCalled();

    resolveInstall();
    await expect(resultPromise).resolves.toEqual(expect.objectContaining({ id: "task-1" }));
    expect(mocks.scheduleKanvibeHooksVerification).toHaveBeenCalledWith(
      "/workspace/repo-worktrees/task-1",
      "task-1",
      null,
      expect.objectContaining({
        onSuccess: expect.any(Function),
        onFailure: expect.any(Function),
      }),
    );
    expect(mocks.installKanvibeHookFiles.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.scheduleKanvibeHooksVerification.mock.invocationCallOrder[0],
    );
    expect(mocks.scheduleKanvibeHooksVerification.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.broadcastBoardUpdate.mock.invocationCallOrder[0],
    );
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
  });

  it("원격 worktree 태스크 생성은 hooks 설치 완료를 기다린 뒤 반환한다", async () => {
    // Given
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/remote/repo",
      defaultBranch: "main",
      sshHost: "remote-host",
    });
    mocks.createWorktreeWithSession.mockResolvedValue({
      worktreePath: "/remote/repo-worktrees/task-1",
      sessionName: "task-1",
    });
    let resolveInstall: () => void = () => {};
    mocks.installKanvibeHookFiles.mockImplementation(() => new Promise<void>((resolve) => {
      resolveInstall = resolve;
    }));
    mocks.taskRepo.save.mockImplementation(async (value) => ({ id: "task-1", ...value }));

    const { createTask } = await import("@/desktop/main/services/kanbanService");

    // When
    let resolved = false;
    const resultPromise = createTask({
      title: "원격 hooks 보장",
      branchName: "fix/remote-hooks",
      projectId: "project-1",
      sessionType: "tmux" as never,
    }).then((result) => {
      resolved = true;
      return result;
    });
    for (let index = 0; index < 6; index += 1) {
      await Promise.resolve();
    }

    // Then
    expect(mocks.installKanvibeHookFiles).toHaveBeenCalledWith(
      "/remote/repo-worktrees/task-1",
      "task-1",
      "remote-host",
    );
    expect(resolved).toBe(false);
    expect(mocks.broadcastBoardUpdate).not.toHaveBeenCalled();
    expect(mocks.scheduleKanvibeHooksVerification).not.toHaveBeenCalled();

    resolveInstall();
    await expect(resultPromise).resolves.toEqual(expect.objectContaining({ id: "task-1" }));
    expect(resolved).toBe(true);
    expect(mocks.scheduleKanvibeHooksVerification).toHaveBeenCalledWith(
      "/remote/repo-worktrees/task-1",
      "task-1",
      "remote-host",
      expect.objectContaining({
        onSuccess: expect.any(Function),
        onFailure: expect.any(Function),
      }),
    );
    expect(mocks.installKanvibeHookFiles.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.scheduleKanvibeHooksVerification.mock.invocationCallOrder[0],
    );
    expect(mocks.scheduleKanvibeHooksVerification.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.broadcastBoardUpdate.mock.invocationCallOrder[0],
    );
    expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
  });

  it("동기 hooks 설치 실패는 실패 이벤트로 브로드캐스트하고 태스크는 반환한다", async () => {
    // Given
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
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
    mocks.installKanvibeHookFiles.mockRejectedValueOnce(new Error("codex config failed"));
    mocks.taskRepo.save.mockImplementation(async (value) => ({ id: "task-1", ...value }));

    const { createTask } = await import("@/desktop/main/services/kanbanService");

    // When
    const result = await createTask({
      title: "알림 회귀 수정",
      branchName: "fix/notifications",
      projectId: "project-1",
      sessionType: "tmux" as never,
    });

    // Then
    expect(result).toEqual(expect.objectContaining({ id: "task-1" }));
    expect(mocks.broadcastTaskHookInstallFailed).toHaveBeenCalledWith({
      taskId: "task-1",
      taskTitle: "알림 회귀 수정",
      error: "codex config failed",
    });
    expect(mocks.scheduleKanvibeHooksVerification).not.toHaveBeenCalled();
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
    consoleErrorSpy.mockRestore();
  });

  it("즉시 hooks 전체 설치는 검증 포함 설치를 사용하고 실패를 전파한다", async () => {
    // Given
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const installError = new Error("verification unavailable");
    mocks.installKanvibeHooks.mockRejectedValueOnce(installError);

    const { installTaskHooksImmediately } = await import("@/desktop/main/services/kanbanService");

    // When & Then
    await expect(installTaskHooksImmediately(
      "/remote/repo__worktrees/feature-task",
      {
        id: "task-1",
        title: "원격 hooks 보장",
        sshHost: "remote-host",
      } as never,
      "새 태스크 hooks 동기 설치 실패",
    )).rejects.toBe(installError);

    expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
      "/remote/repo__worktrees/feature-task",
      "task-1",
      "remote-host",
    );
    expect(mocks.installKanvibeHookFiles).not.toHaveBeenCalled();
    expect(mocks.scheduleKanvibeHooksVerification).not.toHaveBeenCalled();
    expect(mocks.broadcastTaskHookInstallFailed).toHaveBeenCalledWith({
      taskId: "task-1",
      taskTitle: "원격 hooks 보장",
      error: "verification unavailable",
    });
    consoleErrorSpy.mockRestore();
  });

  it("동기 hooks 설치가 성공하면 설치 이후 board update를 한 번 브로드캐스트한다", async () => {
    // Given
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/remote/repo",
      defaultBranch: "main",
      sshHost: "remote-host",
    });
    mocks.createWorktreeWithSession.mockResolvedValue({
      worktreePath: "/remote/repo-worktrees/task-1",
      sessionName: "task-1",
    });
    mocks.installKanvibeHookFiles.mockResolvedValue(undefined);
    mocks.taskRepo.save.mockImplementation(async (value) => ({ id: "task-1", ...value }));

    const { createTask } = await import("@/desktop/main/services/kanbanService");

    // When
    await createTask({
      title: "원격 hooks 성공",
      branchName: "fix/remote-hooks-success",
      projectId: "project-1",
      sessionType: "tmux" as never,
    });

    // Then
    expect(mocks.installKanvibeHookFiles).toHaveBeenCalledWith(
      "/remote/repo-worktrees/task-1",
      "task-1",
      "remote-host",
    );
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
  });

  it("원격 기존 브랜치 task에 터미널을 연결하면 hooks 설치 완료 후 세션을 생성한다", async () => {
    // Given
    const task = {
      id: "task-connect",
      title: "원격 브랜치 연결",
      projectId: "project-remote",
      branchName: "feature/remote",
      baseBranch: "main",
      worktreePath: "/remote/repo__worktrees/feature-remote",
      sshHost: "remote-host",
      sessionType: null,
      sessionName: null,
      status: "todo",
    };
    mocks.taskRepo.findOneBy.mockResolvedValue(task);
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-remote",
      repoPath: "/remote/repo",
      defaultBranch: "main",
      sshHost: "remote-host",
    });
    let resolveInstall: () => void = () => {};
    mocks.installKanvibeHookFiles.mockImplementation(() => new Promise<void>((resolve) => {
      resolveInstall = resolve;
    }));
    mocks.createSessionWithoutWorktree.mockResolvedValue({
      sessionName: "repo-feature-remote",
    });
    mocks.taskRepo.save.mockImplementation(async (value) => value);

    const { connectTerminalSession } = await import("@/desktop/main/services/kanbanService");

    // When
    const resultPromise = connectTerminalSession("task-connect", "tmux" as never);
    for (let index = 0; index < 6; index += 1) {
      await Promise.resolve();
    }

    // Then
    expect(mocks.installKanvibeHookFiles).toHaveBeenCalledWith(
      "/remote/repo__worktrees/feature-remote",
      "task-connect",
      "remote-host",
    );
    expect(mocks.createSessionWithoutWorktree).not.toHaveBeenCalled();
    expect(mocks.broadcastBoardUpdate).not.toHaveBeenCalled();
    expect(mocks.scheduleKanvibeHooksVerification).not.toHaveBeenCalled();

    resolveInstall();
    const result = await resultPromise;

    expect(result).toEqual(expect.objectContaining({
      id: "task-connect",
      sessionType: "tmux",
      sessionName: "repo-feature-remote",
      status: "progress",
    }));
    expect(mocks.createSessionWithoutWorktree).toHaveBeenCalledWith(
      "/remote/repo",
      "feature/remote",
      "tmux",
      "remote-host",
      "/remote/repo__worktrees/feature-remote",
    );
    expect(mocks.installKanvibeHookFiles.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.scheduleKanvibeHooksVerification.mock.invocationCallOrder[0],
    );
    expect(mocks.scheduleKanvibeHooksVerification).toHaveBeenCalledWith(
      "/remote/repo__worktrees/feature-remote",
      "task-connect",
      "remote-host",
      expect.objectContaining({
        onSuccess: expect.any(Function),
        onFailure: expect.any(Function),
      }),
    );
    expect(mocks.scheduleKanvibeHooksVerification.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.createSessionWithoutWorktree.mock.invocationCallOrder[0],
    );
    expect(mocks.scheduleKanvibeHooksInstall).not.toHaveBeenCalled();
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
  });

  it("기존 브랜치 task 터미널 연결은 hooks 설치가 지연되어도 세션 생성을 계속한다", async () => {
    vi.useFakeTimers();

    try {
      // Given
      const task = {
        id: "task-connect-slow-hooks",
        title: "느린 hooks 연결",
        projectId: "project-remote",
        branchName: "feature/slow-hooks",
        baseBranch: "main",
        worktreePath: "/remote/repo__worktrees/feature-slow-hooks",
        sshHost: "remote-host",
        sessionType: null,
        sessionName: null,
        status: "todo",
      };
      mocks.taskRepo.findOneBy.mockResolvedValue(task);
      mocks.projectRepo.findOneBy.mockResolvedValue({
        id: "project-remote",
        repoPath: "/remote/repo",
        defaultBranch: "main",
        sshHost: "remote-host",
      });
      let resolveInstall: () => void = () => {};
      mocks.installKanvibeHookFiles.mockImplementation(() => new Promise<void>((resolve) => {
        resolveInstall = resolve;
      }));
      mocks.createSessionWithoutWorktree.mockResolvedValue({
        sessionName: "repo-feature-slow-hooks",
      });
      mocks.taskRepo.save.mockImplementation(async (value) => value);

      const { connectTerminalSession } = await import("@/desktop/main/services/kanbanService");

      // When
      const resultPromise = connectTerminalSession("task-connect-slow-hooks", "tmux" as never);
      for (let index = 0; index < 6; index += 1) {
        await Promise.resolve();
      }

      // Then
      expect(mocks.installKanvibeHookFiles).toHaveBeenCalledWith(
        "/remote/repo__worktrees/feature-slow-hooks",
        "task-connect-slow-hooks",
        "remote-host",
      );
      expect(mocks.createSessionWithoutWorktree).not.toHaveBeenCalled();
      expect(mocks.scheduleKanvibeHooksVerification).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(1_500);
      const result = await resultPromise;

      expect(result).toEqual(expect.objectContaining({
        id: "task-connect-slow-hooks",
        sessionType: "tmux",
        sessionName: "repo-feature-slow-hooks",
        status: "progress",
      }));
      expect(mocks.createSessionWithoutWorktree).toHaveBeenCalledWith(
        "/remote/repo",
        "feature/slow-hooks",
        "tmux",
        "remote-host",
        "/remote/repo__worktrees/feature-slow-hooks",
      );
      expect(mocks.scheduleKanvibeHooksVerification).not.toHaveBeenCalled();
      expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);

      resolveInstall();
      await Promise.resolve();
      await Promise.resolve();

      expect(mocks.scheduleKanvibeHooksVerification).toHaveBeenCalledWith(
        "/remote/repo__worktrees/feature-slow-hooks",
        "task-connect-slow-hooks",
        "remote-host",
        expect.objectContaining({
          onSuccess: expect.any(Function),
          onFailure: expect.any(Function),
        }),
      );
    } finally {
      vi.clearAllTimers();
      vi.useRealTimers();
    }
  });

  it("기존 task에서 브랜치를 만들면 hooks 설치와 검증 예약 후 board update를 보낸다", async () => {
    // Given
    const task = {
      id: "task-branch",
      title: "원격 브랜치 생성",
      projectId: null,
      branchName: null,
      baseBranch: null,
      worktreePath: null,
      sshHost: null,
      sessionType: null,
      sessionName: null,
      status: "todo",
    };
    mocks.taskRepo.findOneBy.mockResolvedValue(task);
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-remote",
      repoPath: "/remote/repo",
      defaultBranch: "main",
      sshHost: "remote-host",
    });
    mocks.createWorktreeWithSession.mockResolvedValue({
      worktreePath: "/remote/repo__worktrees/feature-from-task",
      sessionName: "repo-feature-from-task",
    });
    let resolveInstall: () => void = () => {};
    mocks.installKanvibeHookFiles.mockImplementation(() => new Promise<void>((resolve) => {
      resolveInstall = resolve;
    }));
    mocks.taskRepo.save.mockImplementation(async (value) => value);

    const { branchFromTask } = await import("@/desktop/main/services/kanbanService");

    // When
    let resolved = false;
    const resultPromise = branchFromTask(
      "task-branch",
      "project-remote",
      "main",
      "feature/from-task",
      "zellij" as never,
    ).then((result) => {
      resolved = true;
      return result;
    });
    for (let index = 0; index < 6; index += 1) {
      await Promise.resolve();
    }

    // Then
    expect(mocks.installKanvibeHookFiles).toHaveBeenCalledWith(
      "/remote/repo__worktrees/feature-from-task",
      "task-branch",
      "remote-host",
    );
    expect(resolved).toBe(false);
    expect(mocks.broadcastBoardUpdate).not.toHaveBeenCalled();
    expect(mocks.scheduleKanvibeHooksVerification).not.toHaveBeenCalled();

    resolveInstall();
    await expect(resultPromise).resolves.toEqual(expect.objectContaining({
      id: "task-branch",
      projectId: "project-remote",
      branchName: "feature/from-task",
      sessionType: "zellij",
      sessionName: "repo-feature-from-task",
      status: "progress",
    }));
    expect(mocks.scheduleKanvibeHooksVerification).toHaveBeenCalledWith(
      "/remote/repo__worktrees/feature-from-task",
      "task-branch",
      "remote-host",
      expect.objectContaining({
        onSuccess: expect.any(Function),
        onFailure: expect.any(Function),
      }),
    );
    expect(mocks.installKanvibeHookFiles.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.scheduleKanvibeHooksVerification.mock.invocationCallOrder[0],
    );
    expect(mocks.scheduleKanvibeHooksVerification.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.broadcastBoardUpdate.mock.invocationCallOrder[0],
    );
    expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
  });

  it("DB worktreePath가 managed 예상 경로와 달라도 task 삭제는 프로젝트와 브랜치 기준으로 정리한 뒤 레코드를 삭제한다", async () => {
    // Given
    const consoleWarnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    const task = {
      id: "task-1",
      projectId: "project-1",
      branchName: "dev",
      worktreePath: "/Users/local/repo__worktrees/dev",
      sshHost: "remote-host",
      sessionType: null,
      sessionName: null,
    };
    mocks.taskRepo.findOneBy.mockResolvedValue(task);
    mocks.taskRepo.remove.mockResolvedValue(task);
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/remote/repo",
      sshHost: "remote-host",
    });

    const { deleteTask } = await import("@/desktop/main/services/kanbanService");

    // When
    const result = await deleteTask("task-1");

    // Then
    expect(result).toBe(true);
    expect(mocks.removeWorktreeAndBranch).toHaveBeenCalledWith(
      "/remote/repo",
      "dev",
      "remote-host",
      { throwOnError: true, worktreePath: "/Users/local/repo__worktrees/dev" },
    );
    expect(mocks.taskRepo.remove).toHaveBeenCalledWith(task);
    expect(consoleWarnSpy).not.toHaveBeenCalled();
  });

  it("DB worktreePath가 없어도 task 삭제는 프로젝트와 브랜치 기준으로 실제 worktree 탐색 정리를 위임한다", async () => {
    // Given
    const task = {
      id: "task-no-db-worktree-path",
      projectId: "project-1",
      branchName: "feature/actual-worktree",
      worktreePath: null,
      sshHost: null,
      sessionType: null,
      sessionName: null,
    };
    mocks.taskRepo.findOneBy.mockResolvedValue(task);
    mocks.taskRepo.remove.mockResolvedValue(task);
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
      sshHost: "remote-host",
    });

    const { deleteTask } = await import("@/desktop/main/services/kanbanService");

    // When
    const result = await deleteTask("task-no-db-worktree-path");

    // Then
    expect(result).toBe(true);
    expect(mocks.removeWorktreeAndBranch).toHaveBeenCalledWith(
      "/workspace/repo",
      "feature/actual-worktree",
      "remote-host",
      { throwOnError: true, worktreePath: null },
    );
    expect(mocks.taskRepo.remove).toHaveBeenCalledWith(task);
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
  });

  it("연결된 프로젝트를 찾을 수 없는 원격 stale task 삭제는 세션 정리 후 task 레코드를 삭제하지 않는다", async () => {
    // Given
    const consoleWarnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    const task = {
      id: "task-2",
      projectId: "missing-project",
      branchName: "main",
      worktreePath: "/home/rookedsysc/Downloads/prompt",
      sshHost: "roky-home",
      sessionType: "tmux" as never,
      sessionName: "prompt-main",
    };
    mocks.taskRepo.findOneBy.mockResolvedValue(task);
    mocks.projectRepo.findOneBy.mockResolvedValue(null);

    const { deleteTask } = await import("@/desktop/main/services/kanbanService");

    // When & Then
    await expect(deleteTask("task-2")).rejects.toThrow("연결된 프로젝트를 찾을 수 없어");
    expect(mocks.detachSession).toHaveBeenCalledWith("task-2", "cleanup-task-resources");
    expect(mocks.removeSessionOnly).toHaveBeenCalledWith(
      "tmux",
      "prompt-main",
      "roky-home",
      { throwOnError: true },
    );
    expect(mocks.removeWorktreeAndBranch).not.toHaveBeenCalled();
    expect(mocks.taskRepo.remove).not.toHaveBeenCalled();
    expect(consoleWarnSpy).not.toHaveBeenCalled();
  });

  it("프로젝트가 삭제되어 orphan 상태가 된 원격 stale task 삭제도 task 레코드를 삭제하지 않는다", async () => {
    // Given
    const consoleWarnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    const task = {
      id: "task-3",
      projectId: null,
      branchName: "dev",
      worktreePath: "/home/rookedsysc/Downloads/techtaurant-be",
      sshHost: "roky-home",
      sessionType: "tmux" as never,
      sessionName: "techtaurant-be-dev",
    };
    mocks.taskRepo.findOneBy.mockResolvedValue(task);

    const { deleteTask } = await import("@/desktop/main/services/kanbanService");

    // When & Then
    await expect(deleteTask("task-3")).rejects.toThrow("연결된 프로젝트를 찾을 수 없어");
    expect(mocks.projectRepo.findOneBy).not.toHaveBeenCalled();
    expect(mocks.detachSession).toHaveBeenCalledWith("task-3", "cleanup-task-resources");
    expect(mocks.removeSessionOnly).toHaveBeenCalledWith(
      "tmux",
      "techtaurant-be-dev",
      "roky-home",
      { throwOnError: true },
    );
    expect(mocks.removeWorktreeAndBranch).not.toHaveBeenCalled();
    expect(mocks.taskRepo.remove).not.toHaveBeenCalled();
    expect(consoleWarnSpy).not.toHaveBeenCalled();
  });

  it("세션만 연결된 원격 task 삭제는 프로젝트를 찾지 못해도 tmux 세션을 정리한다", async () => {
    // Given
    const task = {
      id: "task-remote",
      projectId: "missing-project",
      branchName: null,
      worktreePath: null,
      sshHost: "roky-home",
      sessionType: "tmux" as never,
      sessionName: "prompt-main",
    };
    mocks.taskRepo.findOneBy.mockResolvedValue(task);
    mocks.projectRepo.findOneBy.mockResolvedValue(null);
    mocks.taskRepo.remove.mockResolvedValue(task);

    const { deleteTask } = await import("@/desktop/main/services/kanbanService");

    // When
    const result = await deleteTask("task-remote");

    // Then
    expect(result).toBe(true);
    expect(mocks.detachSession).toHaveBeenCalledWith("task-remote", "cleanup-task-resources");
    expect(mocks.removeSessionOnly).toHaveBeenCalledWith(
      "tmux",
      "prompt-main",
      "roky-home",
      { throwOnError: true },
    );
    expect(mocks.removeWorktreeAndBranch).not.toHaveBeenCalled();
    expect(mocks.taskRepo.remove).toHaveBeenCalledWith(task);
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
  });

  it("원격 task 삭제 중 세션 정리가 실패하면 task 레코드를 삭제하지 않는다", async () => {
    // Given
    const task = {
      id: "task-remote-fail",
      projectId: "missing-project",
      branchName: "main",
      worktreePath: "/home/rookedsysc/Downloads/prompt",
      sshHost: "roky-home",
      sessionType: "tmux" as never,
      sessionName: "prompt-main",
    };
    mocks.taskRepo.findOneBy.mockResolvedValue(task);
    mocks.projectRepo.findOneBy.mockResolvedValue(null);
    mocks.removeSessionOnly.mockRejectedValueOnce(new Error("ssh failed"));

    const { deleteTask } = await import("@/desktop/main/services/kanbanService");

    // When & Then
    await expect(deleteTask("task-remote-fail")).rejects.toThrow("ssh failed");
    expect(mocks.detachSession).toHaveBeenCalledWith("task-remote-fail", "cleanup-task-resources");
    expect(mocks.removeSessionOnly).toHaveBeenCalledWith(
      "tmux",
      "prompt-main",
      "roky-home",
      { throwOnError: true },
    );
    expect(mocks.taskRepo.remove).not.toHaveBeenCalled();
    expect(mocks.broadcastBoardUpdate).not.toHaveBeenCalled();
  });

  it("tmux 세션 정리 전 앱이 붙잡고 있는 활성 터미널 PTY를 먼저 닫는다", async () => {
    // Given
    const callOrder: string[] = [];
    mocks.detachSession.mockImplementation(() => {
      callOrder.push("detach");
    });
    mocks.removeSessionOnly.mockImplementation(async () => {
      callOrder.push("remove-session");
    });

    mocks.taskRepo.findOneBy.mockResolvedValue({
      id: "task-open-terminal",
      projectId: null,
      branchName: null,
      worktreePath: null,
      sshHost: null,
      sessionType: "tmux" as never,
      sessionName: "repo-feat-open",
    });
    mocks.taskRepo.remove.mockResolvedValue({});

    const { deleteTask } = await import("@/desktop/main/services/kanbanService");

    // When
    await deleteTask("task-open-terminal");

    // Then
    expect(callOrder).toEqual(["detach", "remove-session"]);
    expect(mocks.detachSession).toHaveBeenCalledWith("task-open-terminal", "cleanup-task-resources");
    expect(mocks.removeSessionOnly).toHaveBeenCalledWith(
      "tmux",
      "repo-feat-open",
      null,
      { throwOnError: true },
    );
  });

  it("task 내용 수정 후 board update를 브로드캐스트한다", async () => {
    // Given
    const task = {
      id: "task-update",
      title: "Old title",
      description: null,
      priority: null,
    };
    mocks.taskRepo.findOneBy.mockResolvedValue(task);
    mocks.taskRepo.save.mockImplementation(async (value) => value);

    const { updateTask } = await import("@/desktop/main/services/kanbanService");

    // When
    const result = await updateTask("task-update", {
      description: "Fresh description",
    });

    // Then
    expect(result).toEqual(expect.objectContaining({
      id: "task-update",
      description: "Fresh description",
    }));
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
  });

  it("프로젝트 색상 수정 후 board update를 브로드캐스트한다", async () => {
    // Given
    const project = {
      id: "project-1",
      repoPath: "/workspace/repo",
      color: null,
    };
    mocks.projectRepo.findOneBy.mockResolvedValue(project);
    mocks.projectRepo.find.mockResolvedValue([]);
    mocks.projectRepo.save.mockImplementation(async (value) => value);

    const { updateProjectColor } = await import("@/desktop/main/services/kanbanService");

    // When
    await updateProjectColor("project-1", "#93C5FD");

    // Then
    expect(mocks.projectRepo.save).toHaveBeenCalledWith(expect.objectContaining({
      id: "project-1",
      color: "#93C5FD",
    }));
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
  });

  it("컬럼 내 task 순서 변경 후 board update를 브로드캐스트한다", async () => {
    // Given
    mocks.taskRepo.update.mockResolvedValue({ affected: 1 });

    const { reorderTasks } = await import("@/desktop/main/services/kanbanService");

    // When
    await reorderTasks("todo" as never, ["task-a", "task-b"]);

    // Then
    expect(mocks.taskRepo.update).toHaveBeenCalledTimes(2);
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
  });

  it("다른 컬럼으로 task 이동 후 board update를 브로드캐스트한다", async () => {
    // Given
    mocks.taskRepo.update.mockResolvedValue({ affected: 1 });

    const { moveTaskToColumn } = await import("@/desktop/main/services/kanbanService");

    // When
    await moveTaskToColumn("task-a", "review" as never, ["task-b", "task-a"]);

    // Then
    expect(mocks.taskRepo.update).toHaveBeenCalledWith("task-a", { status: "review" });
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
  });

  it("Done 상태 변경은 리소스 정리를 기다리지 않고 먼저 저장한다", async () => {
    // Given
    const task = {
      id: "task-done",
      title: "Remote cleanup",
      description: null,
      status: "review",
      branchName: null,
      projectId: null,
      sessionType: "tmux" as never,
      sessionName: "repo-fix-done",
      worktreePath: "/remote/repo__worktrees/fix-done",
      sshHost: "remote-host",
    };
    mocks.taskRepo.findOneBy.mockResolvedValue(task);
    mocks.taskRepo.save.mockImplementation(async (value) => value);
    mocks.removeSessionOnly.mockReturnValue(new Promise(() => {}));

    const { updateTaskStatus } = await import("@/desktop/main/services/kanbanService");

    // When
    const resultPromise = updateTaskStatus("task-done", "done" as never);
    const raceResult = await Promise.race([
      resultPromise.then(() => "resolved"),
      nextMacrotask("pending"),
    ]);

    // Then
    expect(raceResult).toBe("resolved");
    expect(mocks.taskRepo.save).toHaveBeenCalledWith(expect.objectContaining({
      id: "task-done",
      status: "done",
      sessionType: null,
      sessionName: null,
      worktreePath: null,
    }));
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
  });

  it("Done 컬럼 이동은 리소스 정리를 기다리지 않고 status와 순서를 먼저 저장한다", async () => {
    // Given
    const task = {
      id: "task-done",
      title: "Remote cleanup",
      description: null,
      status: "review",
      branchName: null,
      projectId: null,
      sessionType: "tmux" as never,
      sessionName: "repo-fix-done",
      worktreePath: "/remote/repo__worktrees/fix-done",
      sshHost: "remote-host",
    };
    mocks.taskRepo.findOneBy.mockResolvedValue(task);
    mocks.taskRepo.update.mockResolvedValue({ affected: 1 });
    mocks.removeSessionOnly.mockReturnValue(new Promise(() => {}));

    const { moveTaskToColumn } = await import("@/desktop/main/services/kanbanService");

    // When
    const resultPromise = moveTaskToColumn("task-done", "done" as never, ["task-a", "task-done"]);
    const raceResult = await Promise.race([
      resultPromise.then(() => "resolved"),
      nextMacrotask("pending"),
    ]);

    // Then
    expect(raceResult).toBe("resolved");
    expect(mocks.taskRepo.update).toHaveBeenCalledWith("task-done", {
      status: "done",
      sessionType: null,
      sessionName: null,
      worktreePath: null,
    });
    expect(mocks.taskRepo.update).toHaveBeenCalledWith("task-a", { displayOrder: 0 });
    expect(mocks.taskRepo.update).toHaveBeenCalledWith("task-done", { displayOrder: 1 });
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
  });

  it("Done 컬럼 이동 중 순서 저장이 실패하면 이전 상태와 리소스 필드로 롤백한다", async () => {
    // Given
    const task = {
      id: "task-done",
      title: "Remote cleanup",
      description: null,
      status: "review",
      branchName: null,
      projectId: null,
      sessionType: "tmux" as never,
      sessionName: "repo-fix-done",
      worktreePath: "/remote/repo__worktrees/fix-done",
      sshHost: "remote-host",
    };
    mocks.taskRepo.findOneBy
      .mockResolvedValueOnce(task)
      .mockResolvedValueOnce({
        ...task,
        status: "done",
        sessionType: null,
        sessionName: null,
        worktreePath: null,
      });
    mocks.taskRepo.update.mockImplementation(async (id, updates) => {
      if (id === "task-a" && "displayOrder" in updates) {
        throw new Error("display order failed");
      }

      return { affected: 1 };
    });

    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const { moveTaskToColumn } = await import("@/desktop/main/services/kanbanService");

    // When & Then
    await expect(moveTaskToColumn("task-done", "done" as never, ["task-a", "task-done"]))
      .rejects
      .toThrow("display order failed");

    expect(mocks.taskRepo.update).toHaveBeenCalledWith("task-done", {
      status: "review",
      sessionType: "tmux",
      sessionName: "repo-fix-done",
      worktreePath: "/remote/repo__worktrees/fix-done",
      sshHost: "remote-host",
    });
    expect(mocks.removeSessionOnly).not.toHaveBeenCalled();
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);

    consoleErrorSpy.mockRestore();
  });

  it("백그라운드 Done 정리가 실패하면 이전 상태와 리소스 필드로 롤백한다", async () => {
    vi.useFakeTimers();
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    try {
      // Given
      const task = {
        id: "task-done",
        title: "Remote cleanup",
        description: null,
        status: "review",
        branchName: null,
        projectId: null,
        sessionType: "tmux" as never,
        sessionName: "repo-fix-done",
        worktreePath: "/remote/repo__worktrees/fix-done",
        sshHost: "remote-host",
      };
      mocks.taskRepo.findOneBy
        .mockResolvedValueOnce(task)
        .mockResolvedValueOnce({
          ...task,
          status: "done",
          sessionType: null,
          sessionName: null,
          worktreePath: null,
        });
      mocks.taskRepo.save.mockImplementation(async (value) => value);
      mocks.taskRepo.update.mockResolvedValue({ affected: 1 });
      mocks.removeSessionOnly.mockRejectedValueOnce(new Error("ssh failed"));

      const { updateTaskStatus } = await import("@/desktop/main/services/kanbanService");

      // When
      await updateTaskStatus("task-done", "done" as never);
      await vi.runAllTimersAsync();

      // Then
      expect(mocks.taskRepo.update).toHaveBeenCalledWith("task-done", {
        status: "review",
        sessionType: "tmux",
        sessionName: "repo-fix-done",
        worktreePath: "/remote/repo__worktrees/fix-done",
        sshHost: "remote-host",
      });
      expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(2);
      expect(consoleErrorSpy).toHaveBeenCalled();
    } finally {
      mocks.taskRepo.findOneBy.mockReset();
      mocks.removeSessionOnly.mockReset();
      consoleErrorSpy.mockRestore();
      vi.clearAllTimers();
      vi.useRealTimers();
    }
  });


  it("Done 전환 백그라운드 정리도 DB worktreePath 없이 프로젝트와 브랜치 기준 공통 삭제 정책을 사용한다", async () => {
    vi.useFakeTimers();

    try {
      // Given
      const task = {
        id: "task-done-no-db-worktree-path",
        title: "Done cleanup",
        description: null,
        status: "review",
        branchName: "feature/actual-worktree",
        projectId: "project-1",
        sessionType: null,
        sessionName: null,
        worktreePath: null,
        sshHost: null,
      };
      mocks.taskRepo.findOneBy.mockResolvedValue(task);
      mocks.projectRepo.findOneBy.mockResolvedValue({
        id: "project-1",
        repoPath: "/workspace/repo",
        sshHost: "remote-host",
      });
      mocks.taskRepo.save.mockImplementation(async (value) => value);

      const { updateTaskStatus } = await import("@/desktop/main/services/kanbanService");

      // When
      await updateTaskStatus("task-done-no-db-worktree-path", "done" as never);
      await vi.runAllTimersAsync();

      // Then
      expect(mocks.removeWorktreeAndBranch).toHaveBeenCalledWith(
        "/workspace/repo",
        "feature/actual-worktree",
        "remote-host",
        { throwOnError: true, worktreePath: null },
      );
      expect(mocks.taskRepo.update).not.toHaveBeenCalledWith(
        "task-done-no-db-worktree-path",
        expect.objectContaining({ status: "review" }),
      );
    } finally {
      vi.clearAllTimers();
      vi.useRealTimers();
    }
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
      expect.objectContaining({ cwd: "/workspace/repo", timeout: 10_000 }),
      expect.any(Function),
    );
    expect(mocks.taskRepo.save).toHaveBeenCalledWith(expect.objectContaining({
      id: "task-4",
      prUrl: "https://github.com/kanvibe/kanvibe/pull/1",
    }));
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
    expect(result).toBe("https://github.com/kanvibe/kanvibe/pull/1");
  });

  it("gh CLI가 없으면 PR URL 조회를 조용히 건너뛴다", async () => {
    // Given
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.taskRepo.findOneBy.mockResolvedValue({
      id: "task-5",
      projectId: "project-1",
      branchName: "dev",
      prUrl: null,
    });
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
    });
    mocks.execFile.mockImplementation((file, args, options, callback) => {
      const error = Object.assign(new Error("spawn gh ENOENT"), {
        code: "ENOENT",
        errno: -2,
        syscall: "spawn gh",
        path: "gh",
      });
      callback(error, "", "");
      return {} as never;
    });

    const { fetchAndSavePrUrl } = await import("@/desktop/main/services/kanbanService");

    // When
    const result = await fetchAndSavePrUrl("task-5");

    // Then
    expect(result).toBeNull();
    expect(mocks.taskRepo.save).not.toHaveBeenCalled();
    expect(consoleErrorSpy).not.toHaveBeenCalled();
  });

  it("원격 프로젝트는 프로젝트 repo 경로와 SSH를 사용해 gh CLI로 PR URL을 조회한다", async () => {
    mocks.taskRepo.findOneBy.mockResolvedValue({
      id: "task-6",
      projectId: "project-remote",
      branchName: "feature/remote-pr",
      worktreePath: "/Users/local/repo__worktrees/feature-remote-pr",
      sshHost: "remote-host",
      prUrl: null,
    });
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-remote",
      repoPath: "/remote/repo",
      sshHost: "remote-host",
    });
    mocks.taskRepo.save.mockImplementation(async (value) => value);
    mocks.execGit.mockResolvedValue("https://github.com/kanvibe/kanvibe/pull/99");

    const { fetchAndSavePrUrl } = await import("@/desktop/main/services/kanbanService");

    const result = await fetchAndSavePrUrl("task-6");

    expect(mocks.execGit).toHaveBeenCalledWith(
      "cd '/remote/repo' && gh pr list --head 'feature/remote-pr' --json url -q '.[0].url'",
      "remote-host",
    );
    expect(mocks.execFile).not.toHaveBeenCalled();
    expect(mocks.taskRepo.save).toHaveBeenCalledWith(expect.objectContaining({
      id: "task-6",
      prUrl: "https://github.com/kanvibe/kanvibe/pull/99",
    }));
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
    expect(result).toBe("https://github.com/kanvibe/kanvibe/pull/99");
  });

  it("프로젝트를 찾을 수 없으면 task의 worktree 경로로 PR URL을 조회한다", async () => {
    mocks.taskRepo.findOneBy.mockResolvedValue({
      id: "task-7",
      projectId: "missing-project",
      branchName: "feature/fallback-path",
      worktreePath: "/remote/repo__worktrees/feature-fallback-path",
      sshHost: "remote-host",
      prUrl: null,
    });
    mocks.projectRepo.findOneBy.mockResolvedValue(null);
    mocks.taskRepo.save.mockImplementation(async (value) => value);
    mocks.execGit.mockResolvedValue("https://github.com/kanvibe/kanvibe/pull/101");

    const { fetchAndSavePrUrl } = await import("@/desktop/main/services/kanbanService");

    const result = await fetchAndSavePrUrl("task-7");

    expect(mocks.execGit).toHaveBeenCalledWith(
      "cd '/remote/repo__worktrees/feature-fallback-path' && gh pr list --head 'feature/fallback-path' --json url -q '.[0].url'",
      "remote-host",
    );
    expect(mocks.taskRepo.save).toHaveBeenCalledWith(expect.objectContaining({
      id: "task-7",
      prUrl: "https://github.com/kanvibe/kanvibe/pull/101",
    }));
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
    expect(result).toBe("https://github.com/kanvibe/kanvibe/pull/101");
  });

  it("원격에 gh CLI가 없으면 PR URL 조회를 조용히 건너뛴다", async () => {
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.taskRepo.findOneBy.mockResolvedValue({
      id: "task-8",
      projectId: "project-remote",
      branchName: "feature/no-gh",
      worktreePath: "/Users/local/repo__worktrees/feature-no-gh",
      sshHost: "remote-host",
      prUrl: null,
    });
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-remote",
      repoPath: "/remote/repo",
      sshHost: "remote-host",
    });
    mocks.execGit.mockRejectedValue(new Error("bash: gh: command not found"));

    const { fetchAndSavePrUrl } = await import("@/desktop/main/services/kanbanService");

    const result = await fetchAndSavePrUrl("task-8");

    expect(result).toBeNull();
    expect(mocks.execGit).toHaveBeenCalledWith(
      "cd '/remote/repo' && gh pr list --head 'feature/no-gh' --json url -q '.[0].url'",
      "remote-host",
    );
    expect(mocks.taskRepo.save).not.toHaveBeenCalled();
    expect(mocks.broadcastBoardUpdate).not.toHaveBeenCalled();
    expect(consoleErrorSpy).not.toHaveBeenCalled();
  });

  it("active task PR sync는 새 open PR URL을 task에 저장한다", async () => {
    // Given
    const prUrl = "https://github.com/kanvibe/kanvibe/pull/210";
    mocks.taskRepo.find.mockResolvedValue([
      {
        id: "task-9",
        title: "PR sync target",
        projectId: "project-1",
        branchName: "feature/pr-sync",
        worktreePath: "/workspace/repo__worktrees/feature-pr-sync",
        sshHost: null,
        prUrl: null,
        status: "todo",
      },
    ]);
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
      sshHost: null,
    });
    mocks.taskRepo.save.mockImplementation(async (value) => value);
    mocks.execFile.mockImplementation((file, args, options, callback) => {
      callback(null, JSON.stringify([{
        url: prUrl,
        state: "OPEN",
        mergedAt: null,
        updatedAt: "2026-04-30T01:00:00Z",
      }]), "");
      return {} as never;
    });

    const { syncActiveTaskPullRequests } = await import("@/desktop/main/services/kanbanService");

    // When
    const result = await syncActiveTaskPullRequests(new Set());

    // Then
    expect(result.updatedTaskIds).toEqual(["task-9"]);
    expect(mocks.taskRepo.save).toHaveBeenCalledWith(expect.objectContaining({
      id: "task-9",
      prUrl,
    }));
    expect(mocks.broadcastTaskPrMergedDetectedBatch).not.toHaveBeenCalled();
  });

  it("active task PR sync는 메인 브랜치 root task를 merge 검사 대상에서 제외한다", async () => {
    // Given
    mocks.taskRepo.find.mockResolvedValue([
      {
        id: "task-main",
        title: "Main branch task",
        projectId: "project-1",
        branchName: "main",
        worktreePath: "/workspace/repo",
        sshHost: null,
        prUrl: null,
        status: "review",
      },
    ]);
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
      defaultBranch: "main",
      sshHost: null,
    });

    const { syncActiveTaskPullRequests } = await import("@/desktop/main/services/kanbanService");

    // When
    const result = await syncActiveTaskPullRequests(new Set());

    // Then
    expect(result).toEqual({
      updatedTaskIds: [],
      mergeEventKeys: [],
      mergedPullRequests: [],
    });
    expect(mocks.execFile).not.toHaveBeenCalled();
    expect(mocks.taskRepo.save).not.toHaveBeenCalled();
    expect(mocks.broadcastTaskPrMergedDetectedBatch).not.toHaveBeenCalled();
  });

  it("active task PR sync 실패는 task 대상과 실패 이유를 결과에 포함한다", async () => {
    // Given
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.taskRepo.find.mockResolvedValue([
      {
        id: "task-11",
        title: "PR sync target",
        projectId: "project-1",
        branchName: "feature/pr-fail",
        worktreePath: "/workspace/repo__worktrees/feature-pr-fail",
        sshHost: null,
        prUrl: null,
        status: "review",
      },
    ]);
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
      defaultBranch: "main",
      sshHost: null,
    });
    mocks.execFile.mockImplementation((file, args, options, callback) => {
      callback(new Error("gh auth failed"), "", "");
      return {} as never;
    });

    try {
      const { syncActiveTaskPullRequests } = await import("@/desktop/main/services/kanbanService");

      // When
      const result = await syncActiveTaskPullRequests(new Set());

      // Then
      expect(result).toEqual({
        updatedTaskIds: [],
        mergeEventKeys: [],
        mergedPullRequests: [],
        failures: [
          {
            operation: "pull-request-sync",
            target: "PR sync target (feature/pr-fail)",
            reason: "gh auth failed",
            taskId: "task-11",
            branchName: "feature/pr-fail",
          },
        ],
      });
      expect(mocks.taskRepo.save).not.toHaveBeenCalled();
      expect(mocks.broadcastTaskPrMergedDetectedBatch).not.toHaveBeenCalled();
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it("active task PR sync는 merged PR을 감지하면 중복 없이 merge 이벤트를 브로드캐스트한다", async () => {
    // Given
    const prUrl = "https://github.com/kanvibe/kanvibe/pull/211";
    mocks.taskRepo.find.mockResolvedValue([
      {
        id: "task-10",
        title: "Merged PR task",
        projectId: "project-1",
        branchName: "feature/merged-pr",
        worktreePath: "/workspace/repo__worktrees/feature-merged-pr",
        sshHost: null,
        prUrl: null,
        status: "review",
      },
    ]);
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
      sshHost: null,
    });
    mocks.taskRepo.save.mockImplementation(async (value) => value);
    mocks.execFile.mockImplementation((file, args, options, callback) => {
      callback(null, JSON.stringify([{
        url: prUrl,
        state: "MERGED",
        mergedAt: "2026-04-30T02:00:00Z",
        updatedAt: "2026-04-30T02:00:00Z",
      }]), "");
      return {} as never;
    });

    const mergeEventKeys = new Set<string>();
    const { syncActiveTaskPullRequests } = await import("@/desktop/main/services/kanbanService");

    // When
    await syncActiveTaskPullRequests(mergeEventKeys);
    await syncActiveTaskPullRequests(mergeEventKeys);

    // Then
    expect(mocks.broadcastTaskPrMergedDetectedBatch).toHaveBeenCalledTimes(1);
    expect(mocks.broadcastTaskPrMergedDetectedBatch).toHaveBeenCalledWith({
      mergedPullRequests: [{
        taskId: "task-10",
        taskTitle: "Merged PR task",
        branchName: "feature/merged-pr",
        prUrl,
        mergedAt: "2026-04-30T02:00:00Z",
      }],
    });
  });

  it("active task PR sync는 task별 GitHub CLI 조회를 직렬로 실행한다", async () => {
    // Given
    mocks.taskRepo.find.mockResolvedValue([
      {
        id: "task-serial-a",
        title: "Serial PR A",
        projectId: "project-1",
        branchName: "feature/serial-a",
        worktreePath: "/workspace/repo__worktrees/serial-a",
        sshHost: null,
        prUrl: null,
        status: "review",
      },
      {
        id: "task-serial-b",
        title: "Serial PR B",
        projectId: "project-1",
        branchName: "feature/serial-b",
        worktreePath: "/workspace/repo__worktrees/serial-b",
        sshHost: null,
        prUrl: null,
        status: "review",
      },
    ]);
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
      defaultBranch: "main",
      sshHost: null,
    });
    const callbacks: Array<(error: Error | null, stdout: string, stderr: string) => void> = [];
    mocks.execFile.mockImplementation((file, args, options, callback) => {
      callbacks.push(callback);
      return {} as never;
    });

    const { syncActiveTaskPullRequests } = await import("@/desktop/main/services/kanbanService");

    // When
    const syncPromise = syncActiveTaskPullRequests(new Set());
    await new Promise((resolve) => setTimeout(resolve, 0));

    // Then
    expect(callbacks).toHaveLength(1);

    callbacks[0](null, JSON.stringify([{
      url: null,
      state: null,
      mergedAt: null,
      updatedAt: "2026-05-02T01:00:00Z",
    }]), "");
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(callbacks).toHaveLength(2);

    callbacks[1](null, JSON.stringify([{
      url: null,
      state: null,
      mergedAt: null,
      updatedAt: "2026-05-02T01:00:00Z",
    }]), "");
    await syncPromise;
  });

  it("active task PR sync는 local gh 조회 timeout 후 다음 task를 계속 처리한다", async () => {
    vi.useFakeTimers();
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      // Given
      mocks.taskRepo.find.mockResolvedValue([
        {
          id: "task-pr-stuck",
          title: "Stuck PR",
          projectId: "project-1",
          branchName: "feature/pr-stuck",
          worktreePath: "/workspace/repo__worktrees/pr-stuck",
          sshHost: null,
          prUrl: null,
          status: "review",
        },
        {
          id: "task-pr-next",
          title: "Next PR",
          projectId: "project-1",
          branchName: "feature/pr-next",
          worktreePath: "/workspace/repo__worktrees/pr-next",
          sshHost: null,
          prUrl: null,
          status: "review",
        },
      ]);
      mocks.projectRepo.findOneBy.mockResolvedValue({
        id: "project-1",
        repoPath: "/workspace/repo",
        defaultBranch: "main",
        sshHost: null,
      });
      const execFileOptions: Array<{ cwd?: string; timeout?: number }> = [];
      mocks.execFile.mockImplementation((file, args: string[], options, callback) => {
        execFileOptions.push(options);
        if (args.includes("feature/pr-stuck")) {
          if (typeof options.timeout === "number") {
            setTimeout(() => {
              callback(Object.assign(new Error("gh pr list timed out"), { killed: true, signal: "SIGTERM" }), "", "");
            }, options.timeout);
          }
          return {} as never;
        }

        callback(null, JSON.stringify([{
          url: null,
          state: null,
          mergedAt: null,
          updatedAt: "2026-05-02T01:00:00Z",
        }]), "");
        return {} as never;
      });

      const { syncActiveTaskPullRequests } = await import("@/desktop/main/services/kanbanService");

      // When
      const syncPromise = syncActiveTaskPullRequests(new Set());
      await vi.advanceTimersByTimeAsync(0);

      // Then
      expect(execFileOptions).toEqual([
        expect.objectContaining({ cwd: "/workspace/repo", timeout: 10_000 }),
      ]);

      await vi.advanceTimersByTimeAsync(10_000);
      await vi.advanceTimersByTimeAsync(0);

      expect(execFileOptions).toEqual([
        expect.objectContaining({ cwd: "/workspace/repo", timeout: 10_000 }),
        expect.objectContaining({ cwd: "/workspace/repo", timeout: 10_000 }),
      ]);
      await expect(syncPromise).resolves.toEqual({
        updatedTaskIds: [],
        mergeEventKeys: [],
        mergedPullRequests: [],
        failures: [
          expect.objectContaining({
            operation: "pull-request-sync",
            taskId: "task-pr-stuck",
            branchName: "feature/pr-stuck",
            reason: "gh pr list timed out",
          }),
        ],
      });
    } finally {
      consoleErrorSpy.mockRestore();
      vi.useRealTimers();
    }
  });

  it("active task pull sync는 default branch task를 제외하고 task별 pull을 직렬로 실행한다", async () => {
    // Given
    mocks.taskRepo.find.mockResolvedValue([
      {
        id: "task-main",
        title: "Main",
        projectId: "project-1",
        branchName: "main",
        worktreePath: "/workspace/repo",
        sshHost: null,
        status: "progress",
      },
      {
        id: "task-pull-a",
        title: "Pull A",
        projectId: "project-1",
        branchName: "feature/pull-a",
        worktreePath: "/workspace/repo__worktrees/pull-a",
        sshHost: null,
        status: "progress",
      },
      {
        id: "task-pull-b",
        title: "Pull B",
        projectId: "project-1",
        branchName: "feature/pull-b",
        worktreePath: "/workspace/repo__worktrees/pull-b",
        sshHost: null,
        status: "review",
      },
    ]);
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
      defaultBranch: "main",
      sshHost: null,
    });
    const resolvers: Array<(value: string) => void> = [];
    const rejecters: Array<(error: Error) => void> = [];
    mocks.pullCurrentBranch.mockImplementation(() => new Promise<string>((resolve, reject) => {
      resolvers.push(resolve);
      rejecters.push(reject);
    }));

    const { syncActiveTaskPulls } = await import("@/desktop/main/services/kanbanService");

    // When
    const syncPromise = syncActiveTaskPulls();
    await new Promise((resolve) => setTimeout(resolve, 0));

    // Then
    expect(mocks.pullCurrentBranch).toHaveBeenCalledTimes(1);
    expect(mocks.pullCurrentBranch).toHaveBeenNthCalledWith(
      1,
      "/workspace/repo__worktrees/pull-a",
      null,
      expect.objectContaining({ timeoutMs: 10_000 }),
    );

    resolvers[0]("Fast-forward\n src/file.ts | 1 +");
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(mocks.pullCurrentBranch).toHaveBeenCalledTimes(2);
    expect(mocks.pullCurrentBranch).toHaveBeenNthCalledWith(
      2,
      "/workspace/repo__worktrees/pull-b",
      null,
      expect.objectContaining({ timeoutMs: 10_000 }),
    );

    rejecters[1](new Error("Not possible to fast-forward"));

    await expect(syncPromise).resolves.toEqual({
      pulledTasks: [
        {
          taskId: "task-pull-a",
          taskTitle: "Pull A",
          branchName: "feature/pull-a",
          worktreePath: "/workspace/repo__worktrees/pull-a",
          sshHost: null,
          status: "updated",
          summary: "Fast-forward",
        },
        {
          taskId: "task-pull-b",
          taskTitle: "Pull B",
          branchName: "feature/pull-b",
          worktreePath: "/workspace/repo__worktrees/pull-b",
          sshHost: null,
          status: "failed",
          summary: "Not possible to fast-forward",
        },
      ],
    });
  });

  it("active task pull sync는 한 task의 remote branch 확인이 멈춰도 다음 task를 계속 처리한다", async () => {
    vi.useFakeTimers();
    try {
      // Given
      mocks.taskRepo.find.mockResolvedValue([
        {
          id: "task-main",
          title: "Main task",
          projectId: "project-1",
          branchName: "main",
          worktreePath: "/workspace/repo",
          sshHost: null,
          status: "progress",
        },
        {
          id: "task-stuck",
          title: "Stuck pull",
          projectId: "project-1",
          branchName: "feature/stuck",
          worktreePath: "/workspace/repo__worktrees/stuck",
          sshHost: null,
          status: "progress",
        },
        {
          id: "task-next",
          title: "Next pull",
          projectId: "project-1",
          branchName: "feature/next",
          worktreePath: "/workspace/repo__worktrees/next",
          sshHost: null,
          status: "review",
        },
      ]);
      mocks.projectRepo.findOneBy.mockResolvedValue({
        id: "project-1",
        repoPath: "/workspace/repo",
        defaultBranch: "main",
        sshHost: null,
      });
      mocks.remoteBranchExists.mockImplementation((worktreePath: string, branchName: string, sshHost: string | null, options?: { timeoutMs?: number }) => {
        if (worktreePath.includes("stuck")) {
          return new Promise<boolean>((resolve, reject) => {
            void resolve;
            setTimeout(() => {
              reject(new Error(`Remote branch check timed out after ${(options?.timeoutMs ?? 0) / 1000}s`));
            }, options?.timeoutMs ?? 0);
          });
        }

        return Promise.resolve(true);
      });
      mocks.pullCurrentBranch.mockResolvedValue("Already up to date.");

      const { syncActiveTaskPulls } = await import("@/desktop/main/services/kanbanService");

      // When
      const syncPromise = syncActiveTaskPulls();
      await vi.advanceTimersByTimeAsync(0);

      // Then
      expect(mocks.remoteBranchExists).toHaveBeenCalledTimes(1);

      await vi.advanceTimersByTimeAsync(10_000);
      await vi.advanceTimersByTimeAsync(0);

      expect(mocks.remoteBranchExists).toHaveBeenCalledTimes(2);
      expect(mocks.pullCurrentBranch).toHaveBeenCalledWith(
        "/workspace/repo__worktrees/next",
        null,
        expect.objectContaining({ timeoutMs: 10_000 }),
      );
      await expect(syncPromise).resolves.toEqual({
        pulledTasks: [
          expect.objectContaining({
            taskId: "task-stuck",
            branchName: "feature/stuck",
            status: "failed",
            summary: expect.stringContaining("timed out"),
          }),
        ],
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("remote task pull sync는 외부 timer로 원격 git 결과를 버리지 않는다", async () => {
    vi.useFakeTimers();
    try {
      // Given
      mocks.taskRepo.find.mockResolvedValue([
        {
          id: "task-remote",
          title: "Remote pull",
          projectId: "project-remote",
          branchName: "feature/remote-pull",
          worktreePath: "/remote/repo__worktrees/remote-pull",
          sshHost: "remote-host",
          status: "review",
        },
      ]);
      mocks.projectRepo.findOneBy.mockResolvedValue({
        id: "project-remote",
        repoPath: "/remote/repo",
        defaultBranch: "main",
        sshHost: "remote-host",
      });
      let resolveRemoteBranchExists: (value: boolean) => void = () => {};
      let resolveRemotePull: (value: string) => void = () => {};
      mocks.remoteBranchExists.mockImplementation(() => new Promise<boolean>((resolve) => {
        resolveRemoteBranchExists = resolve;
      }));
      mocks.pullCurrentBranch.mockImplementation(() => new Promise<string>((resolve) => {
        resolveRemotePull = resolve;
      }));

      const { syncActiveTaskPulls } = await import("@/desktop/main/services/kanbanService");

      // When
      let isSyncComplete = false;
      const syncPromise = syncActiveTaskPulls().then((result) => {
        isSyncComplete = true;
        return result;
      });
      await vi.advanceTimersByTimeAsync(0);

      // Then
      expect(mocks.remoteBranchExists).toHaveBeenCalledWith(
        "/remote/repo__worktrees/remote-pull",
        "feature/remote-pull",
        "remote-host",
      );

      await vi.advanceTimersByTimeAsync(10_000);
      await vi.advanceTimersByTimeAsync(0);

      expect(isSyncComplete).toBe(false);
      expect(mocks.pullCurrentBranch).not.toHaveBeenCalled();

      resolveRemoteBranchExists(true);
      await vi.advanceTimersByTimeAsync(0);

      expect(mocks.pullCurrentBranch).toHaveBeenCalledWith(
        "/remote/repo__worktrees/remote-pull",
        "remote-host",
      );

      await vi.advanceTimersByTimeAsync(10_000);
      await vi.advanceTimersByTimeAsync(0);

      expect(isSyncComplete).toBe(false);

      resolveRemotePull("Fast-forward\n src/file.ts | 1 +");

      await expect(syncPromise).resolves.toEqual({
        pulledTasks: [
          {
            taskId: "task-remote",
            taskTitle: "Remote pull",
            branchName: "feature/remote-pull",
            worktreePath: "/remote/repo__worktrees/remote-pull",
            sshHost: "remote-host",
            status: "updated",
            summary: "Fast-forward",
          },
        ],
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("active task pull sync는 done task를 pull하지 않는다", async () => {
    // Given
    mocks.taskRepo.find.mockResolvedValue([
      {
        id: "task-done",
        title: "Done task",
        projectId: "project-1",
        branchName: "feature/done",
        worktreePath: "/workspace/repo__worktrees/done",
        sshHost: null,
        status: "done",
      },
      {
        id: "task-progress",
        title: "Progress task",
        projectId: "project-1",
        branchName: "feature/progress",
        worktreePath: "/workspace/repo__worktrees/progress",
        sshHost: null,
        status: "progress",
      },
    ]);
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
      defaultBranch: "main",
      sshHost: null,
    });
    mocks.pullCurrentBranch.mockResolvedValue("Already up to date.");

    const { syncActiveTaskPulls } = await import("@/desktop/main/services/kanbanService");

    // When
    const result = await syncActiveTaskPulls();

    // Then
    expect(mocks.taskRepo.find).toHaveBeenCalledWith(expect.objectContaining({
      where: expect.objectContaining({
        status: expect.objectContaining({
          _type: "in",
          _value: ["todo", "progress", "pending", "review"],
        }),
      }),
    }));
    expect(mocks.pullCurrentBranch).toHaveBeenCalledTimes(1);
    expect(mocks.pullCurrentBranch).toHaveBeenCalledWith(
      "/workspace/repo__worktrees/progress",
      null,
      expect.objectContaining({ timeoutMs: 10_000 }),
    );
    expect(result).toEqual({ pulledTasks: [] });
  });

  it("active task pull sync는 remote branch가 없으면 pull하지 않는다", async () => {
    // Given
    mocks.taskRepo.find.mockResolvedValue([
      {
        id: "task-missing-remote",
        title: "Missing remote branch",
        projectId: "project-1",
        branchName: "feature/missing-remote",
        worktreePath: "/workspace/repo__worktrees/missing-remote",
        sshHost: null,
        status: "review",
      },
    ]);
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
      defaultBranch: "main",
      sshHost: null,
    });
    mocks.remoteBranchExists.mockResolvedValue(false);

    const { syncActiveTaskPulls } = await import("@/desktop/main/services/kanbanService");

    // When
    const result = await syncActiveTaskPulls();

    // Then
    expect(mocks.remoteBranchExists).toHaveBeenCalledWith(
      "/workspace/repo__worktrees/missing-remote",
      "feature/missing-remote",
      null,
      expect.objectContaining({ timeoutMs: 10_000 }),
    );
    expect(mocks.pullCurrentBranch).not.toHaveBeenCalled();
    expect(result).toEqual({ pulledTasks: [] });
  });

  it("active task pull sync는 remote branch 없음 pull 오류를 실패로 반환하지 않는다", async () => {
    // Given
    mocks.taskRepo.find.mockResolvedValue([
      {
        id: "task-stale-upstream",
        title: "Stale upstream",
        projectId: "project-1",
        branchName: "feature/stale-upstream",
        worktreePath: "/workspace/repo__worktrees/stale-upstream",
        sshHost: "remote-host",
        status: "review",
      },
    ]);
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
      defaultBranch: "main",
      sshHost: "remote-host",
    });
    mocks.remoteBranchExists.mockResolvedValue(true);
    mocks.pullCurrentBranch.mockRejectedValue(new Error(
      "remote-host 원격 명령 실패: Your configuration specifies to merge with the ref 'refs/heads/feature/stale-upstream' from the remote, but no such ref was fetched.",
    ));

    const { syncActiveTaskPulls } = await import("@/desktop/main/services/kanbanService");

    // When
    const result = await syncActiveTaskPulls();

    // Then
    expect(result).toEqual({ pulledTasks: [] });
  });

  it("active task pull sync는 같은 task pull 실패를 성공 전까지 한 번만 반환한다", async () => {
    // Given
    mocks.taskRepo.find.mockResolvedValue([
      {
        id: "task-pull-b",
        title: "Pull B",
        projectId: "project-1",
        branchName: "feature/pull-b",
        worktreePath: "/workspace/repo__worktrees/pull-b",
        sshHost: null,
        status: "review",
      },
    ]);
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
      defaultBranch: "main",
      sshHost: null,
    });
    mocks.pullCurrentBranch
      .mockRejectedValueOnce(new Error("Not possible to fast-forward"))
      .mockRejectedValueOnce(new Error("Not possible to fast-forward"))
      .mockResolvedValueOnce("Already up to date.")
      .mockRejectedValueOnce(new Error("Not possible to fast-forward"));

    const { syncActiveTaskPulls } = await import("@/desktop/main/services/kanbanService");

    // When
    const firstFailure = await syncActiveTaskPulls();
    const repeatedFailure = await syncActiveTaskPulls();
    const recovery = await syncActiveTaskPulls();
    const failureAfterRecovery = await syncActiveTaskPulls();

    // Then
    expect(firstFailure.pulledTasks).toEqual([
      expect.objectContaining({
        taskId: "task-pull-b",
        branchName: "feature/pull-b",
        status: "failed",
        summary: "Not possible to fast-forward",
      }),
    ]);
    expect(repeatedFailure.pulledTasks).toEqual([]);
    expect(recovery.pulledTasks).toEqual([]);
    expect(failureAfterRecovery.pulledTasks).toEqual([
      expect.objectContaining({
        taskId: "task-pull-b",
        branchName: "feature/pull-b",
        status: "failed",
        summary: "Not possible to fast-forward",
      }),
    ]);
  });
});

describe("kanbanService.getSearchableTasks", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("빠른 검색용 task 목록에서 done 상태를 조회하지 않는다", async () => {
    // Given
    const updatedAt = new Date("2026-05-02T00:00:00.000Z");
    mocks.taskRepo.find.mockResolvedValue([
      {
        id: "task-active",
        title: "Active task",
        branchName: "dev",
        projectId: "project-kanvibe",
        project: { name: "kanvibe", sshHost: null },
        sshHost: null,
        status: "progress",
        updatedAt,
      },
    ]);

    const { getSearchableTasks } = await import("@/desktop/main/services/kanbanService");

    // When
    const result = await getSearchableTasks();

    // Then
    expect(mocks.taskRepo.find).toHaveBeenCalledWith(expect.objectContaining({
      where: expect.objectContaining({
        status: expect.objectContaining({
          _type: "not",
          _value: "done",
        }),
      }),
      relations: ["project"],
      order: { updatedAt: "DESC", createdAt: "DESC" },
    }));
    expect(result).toEqual([
      {
        id: "task-active",
        title: "Active task",
        branchName: "dev",
        projectId: "project-kanvibe",
        projectName: "kanvibe",
        sshHost: null,
        status: "progress",
        updatedAt: "2026-05-02T00:00:00.000Z",
      },
    ]);
  });
});
