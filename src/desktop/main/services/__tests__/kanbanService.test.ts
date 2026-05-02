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
  removeWorktreeAndBranch: vi.fn(),
  removeSessionOnly: vi.fn(),
  installKanvibeHooks: vi.fn(),
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
  createSessionWithoutWorktree: vi.fn(),
  removeSessionOnly: mocks.removeSessionOnly,
  buildManagedWorktreePath: vi.fn((projectPath: string, branchName: string) => `${projectPath}__worktrees/${branchName}`),
}));

vi.mock("@/lib/kanvibeHooksInstaller", () => ({
  installKanvibeHooks: mocks.installKanvibeHooks,
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

describe("kanbanService.createTask", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.taskRepo.create.mockImplementation((value) => value);
    mocks.installKanvibeHooks.mockResolvedValue(undefined);
    mocks.remoteBranchExists.mockResolvedValue(true);
    mocks.taskRepo.createQueryBuilder.mockReturnValue({
      select: vi.fn().mockReturnThis(),
      where: vi.fn().mockReturnThis(),
      getRawOne: vi.fn().mockResolvedValue({ max: 2 }),
    });
  });

  it("로컬 worktree 태스크를 만들면 hooks를 자동 설치한다", async () => {
    vi.useFakeTimers();

    try {
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
      await vi.runAllTimersAsync();

      // Then
      expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
        "/workspace/repo-worktrees/task-1",
        "task-1",
        null,
      );
    } finally {
      vi.clearAllTimers();
      vi.useRealTimers();
    }
  });

  it("worktree 태스크 생성은 hooks 설치 완료를 기다리지 않고 즉시 반환한다", async () => {
    vi.useFakeTimers();

    try {
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
      mocks.installKanvibeHooks.mockReturnValue(new Promise(() => {}));
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
      expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
      expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);

      await vi.runAllTimersAsync();
      expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
        "/workspace/repo-worktrees/task-1",
        "task-1",
        null,
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it("원격 worktree 태스크 생성은 hooks 설치 완료를 기다린다", async () => {
    vi.useFakeTimers();

    try {
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
      let resolveInstall: (() => void) | undefined;
      mocks.installKanvibeHooks.mockImplementation(() => new Promise<void>((resolve) => {
        resolveInstall = resolve;
      }));
      mocks.taskRepo.save.mockImplementation(async (value) => ({ id: "task-1", ...value }));

      const { createTask } = await import("@/desktop/main/services/kanbanService");

      // When
      let settled = false;
      const resultPromise = createTask({
        title: "원격 hooks 보장",
        branchName: "fix/remote-hooks",
        projectId: "project-1",
        sessionType: "tmux" as never,
      }).then((result) => {
        settled = true;
        return result;
      });
      for (let index = 0; index < 6; index += 1) {
        await Promise.resolve();
      }

      // Then
      expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
        "/remote/repo-worktrees/task-1",
        "task-1",
        "remote-host",
      );
      expect(settled).toBe(false);

      if (resolveInstall) {
        resolveInstall();
      }
      await expect(resultPromise).resolves.toEqual(expect.objectContaining({ id: "task-1" }));
    } finally {
      vi.useRealTimers();
    }
  });

  it("백그라운드 hooks 설치 실패는 실패 이벤트로 브로드캐스트한다", async () => {
    vi.useFakeTimers();

    try {
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
      mocks.installKanvibeHooks.mockRejectedValueOnce(new Error("codex config failed"));
      mocks.taskRepo.save.mockImplementation(async (value) => ({ id: "task-1", ...value }));

      const { createTask } = await import("@/desktop/main/services/kanbanService");

      // When
      await createTask({
        title: "알림 회귀 수정",
        branchName: "fix/notifications",
        projectId: "project-1",
        sessionType: "tmux" as never,
      });
      await vi.runAllTimersAsync();

      // Then
      expect(mocks.broadcastTaskHookInstallFailed).toHaveBeenCalledWith({
        taskId: "task-1",
        taskTitle: "알림 회귀 수정",
        error: "codex config failed",
      });
      consoleErrorSpy.mockRestore();
    } finally {
      vi.useRealTimers();
    }
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

  it("연결된 프로젝트를 찾을 수 없는 원격 stale task는 세션만 정리한다", async () => {
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
    expect(mocks.removeSessionOnly).toHaveBeenCalledWith(
      "tmux",
      "prompt-main",
      "roky-home",
    );
    expect(mocks.removeWorktreeAndBranch).not.toHaveBeenCalled();
    expect(consoleWarnSpy).not.toHaveBeenCalled();
  });

  it("프로젝트가 삭제되어 orphan 상태가 된 원격 stale task도 세션만 정리한다", async () => {
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
    expect(mocks.removeSessionOnly).toHaveBeenCalledWith(
      "tmux",
      "techtaurant-be-dev",
      "roky-home",
    );
    expect(mocks.removeWorktreeAndBranch).not.toHaveBeenCalled();
    expect(consoleWarnSpy).not.toHaveBeenCalled();
  });

  it("원격 task 삭제는 프로젝트를 찾지 못해도 연결된 tmux 세션을 정리한다", async () => {
    // Given
    const task = {
      id: "task-remote",
      projectId: "missing-project",
      branchName: "main",
      worktreePath: "/home/rookedsysc/Downloads/prompt",
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
    expect(mocks.removeSessionOnly).toHaveBeenCalledWith(
      "tmux",
      "prompt-main",
      "roky-home",
    );
    expect(mocks.removeWorktreeAndBranch).not.toHaveBeenCalled();
    expect(mocks.taskRepo.remove).toHaveBeenCalledWith(task);
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
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

  it("active task PR sync는 task별 GitHub CLI 조회를 병렬로 시작한다", async () => {
    // Given
    mocks.taskRepo.find.mockResolvedValue([
      {
        id: "task-parallel-a",
        title: "Parallel PR A",
        projectId: "project-1",
        branchName: "feature/parallel-a",
        worktreePath: "/workspace/repo__worktrees/parallel-a",
        sshHost: null,
        prUrl: null,
        status: "review",
      },
      {
        id: "task-parallel-b",
        title: "Parallel PR B",
        projectId: "project-1",
        branchName: "feature/parallel-b",
        worktreePath: "/workspace/repo__worktrees/parallel-b",
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
    expect(callbacks).toHaveLength(2);

    callbacks.forEach((callback) => {
      callback(null, JSON.stringify([{
        url: null,
        state: null,
        mergedAt: null,
        updatedAt: "2026-05-02T01:00:00Z",
      }]), "");
    });
    await syncPromise;
  });

  it("active task pull sync는 default branch task를 제외하고 task별 pull을 병렬로 실행한다", async () => {
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
    expect(mocks.pullCurrentBranch).toHaveBeenCalledTimes(2);
    expect(mocks.pullCurrentBranch).toHaveBeenNthCalledWith(1, "/workspace/repo__worktrees/pull-a", null);
    expect(mocks.pullCurrentBranch).toHaveBeenNthCalledWith(2, "/workspace/repo__worktrees/pull-b", null);

    resolvers[0]("Fast-forward\n src/file.ts | 1 +");
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
    expect(mocks.pullCurrentBranch).toHaveBeenCalledWith("/workspace/repo__worktrees/progress", null);
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
