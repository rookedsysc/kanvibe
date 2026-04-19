import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  execGit: vi.fn(),
  homedir: vi.fn(() => "/home/tester"),
  validateGitRepo: vi.fn(),
  getDefaultBranch: vi.fn(),
  scanGitRepos: vi.fn(),
  listWorktrees: vi.fn(),
  getProjectRepository: vi.fn(),
  getTaskRepository: vi.fn(),
  createSessionWithoutWorktree: vi.fn(),
  isSessionAlive: vi.fn(),
  formatSessionName: vi.fn(),
  computeProjectColor: vi.fn(() => "blue"),
  getDefaultSessionType: vi.fn(),
  setupClaudeHooks: vi.fn(),
  getClaudeHooksStatus: vi.fn(),
  setupGeminiHooks: vi.fn(),
  getGeminiHooksStatus: vi.fn(),
  setupCodexHooks: vi.fn(),
  getCodexHooksStatus: vi.fn(),
  setupOpenCodeHooks: vi.fn(),
  getOpenCodeHooksStatus: vi.fn(),
  getHookServerUrl: vi.fn(() => "http://localhost:9736"),
  getHookServerToken: vi.fn(() => "desktop-hook-token"),
  aggregateAiSessions: vi.fn(),
  getAiSessionDetail: vi.fn(),
  installKanvibeHooks: vi.fn(),
  readHookTaskIdFile: vi.fn(),
  broadcastBoardUpdate: vi.fn(),
}));

vi.mock("@/lib/database", () => ({
  getProjectRepository: mocks.getProjectRepository,
  getTaskRepository: mocks.getTaskRepository,
}));

vi.mock("@/entities/Project", () => ({
  Project: class Project {},
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

vi.mock("typeorm", () => ({
  IsNull: vi.fn(),
}));

vi.mock("@/lib/gitOperations", () => ({
  validateGitRepo: mocks.validateGitRepo,
  getDefaultBranch: mocks.getDefaultBranch,
  listBranches: vi.fn(),
  scanGitRepos: mocks.scanGitRepos,
  listWorktrees: mocks.listWorktrees,
  execGit: mocks.execGit,
}));

vi.mock("@/lib/worktree", () => ({
  isSessionAlive: mocks.isSessionAlive,
  formatSessionName: mocks.formatSessionName,
  createSessionWithoutWorktree: mocks.createSessionWithoutWorktree,
}));

vi.mock("@/lib/claudeHooksSetup", () => ({
  setupClaudeHooks: mocks.setupClaudeHooks,
  getClaudeHooksStatus: mocks.getClaudeHooksStatus,
}));

vi.mock("@/lib/geminiHooksSetup", () => ({
  setupGeminiHooks: mocks.setupGeminiHooks,
  getGeminiHooksStatus: mocks.getGeminiHooksStatus,
}));

vi.mock("@/lib/codexHooksSetup", () => ({
  setupCodexHooks: mocks.setupCodexHooks,
  getCodexHooksStatus: mocks.getCodexHooksStatus,
}));

vi.mock("@/lib/openCodeHooksSetup", () => ({
  setupOpenCodeHooks: mocks.setupOpenCodeHooks,
  getOpenCodeHooksStatus: mocks.getOpenCodeHooksStatus,
}));

vi.mock("@/lib/hookEndpoint", () => ({
  getHookServerUrl: mocks.getHookServerUrl,
  getHookServerToken: mocks.getHookServerToken,
}));

vi.mock("@/lib/aiSessions/aggregateAiSessions", () => ({
  aggregateAiSessions: mocks.aggregateAiSessions,
  getAiSessionDetail: mocks.getAiSessionDetail,
}));

vi.mock("os", () => ({
  default: {
    homedir: mocks.homedir,
  },
  homedir: mocks.homedir,
}));

vi.mock("@/lib/projectColor", () => ({
  computeProjectColor: mocks.computeProjectColor,
}));

vi.mock("@/lib/boardNotifier", () => ({
  broadcastBoardUpdate: mocks.broadcastBoardUpdate,
}));

vi.mock("@/lib/sshConfig", () => ({
  getAvailableHosts: vi.fn(),
}));

vi.mock("@/desktop/main/services/appSettingsService", () => ({
  getDefaultSessionType: mocks.getDefaultSessionType,
}));

vi.mock("@/lib/remoteSessionDependency", () => ({
  ensureRemoteSessionDependency: vi.fn(),
}));

vi.mock("@/lib/kanvibeHooksInstaller", () => ({
  installKanvibeHooks: mocks.installKanvibeHooks,
}));

vi.mock("@/lib/hookTaskBinding", () => ({
  readHookTaskIdFile: mocks.readHookTaskIdFile,
}));

describe("projectService.listSubdirectories", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.getDefaultSessionType.mockResolvedValue("tmux");
    mocks.readHookTaskIdFile.mockResolvedValue(null);
  });

  it("원격 호스트에서도 세션 도구 검증 없이 디렉토리를 스캔한다", async () => {
    // Given
    mocks.execGit.mockResolvedValue("/workspace/api\n/workspace/web\n/workspace/.hidden\n");
    const { listSubdirectories } = await import("@/desktop/main/services/projectService");

    // When
    const result = await listSubdirectories("/workspace", "remote-host");

    // Then
    expect(mocks.execGit).toHaveBeenCalledWith(
      'find "/workspace" -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort',
      "remote-host",
    );
    expect(result).toEqual(["api", "web"]);
  });

  it("틸드 경로는 홈 디렉토리로 치환해서 스캔한다", async () => {
    // Given
    mocks.execGit.mockResolvedValue("/home/tester/projects\n");
    const { listSubdirectories } = await import("@/desktop/main/services/projectService");

    // When
    const result = await listSubdirectories("~/", undefined);

    // Then
    expect(mocks.execGit).toHaveBeenCalledWith(
      'find "/home/tester/" -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort',
      null,
    );
    expect(result).toEqual(["projects"]);
  });

  it("원격 틸드 경로는 원격 HOME 기준으로 스캔한다", async () => {
    // Given
    mocks.execGit.mockResolvedValue("/home/remote/projects\n");
    const { listSubdirectories } = await import("@/desktop/main/services/projectService");

    // When
    const result = await listSubdirectories("~/projects", "remote-host");

    // Then
    expect(mocks.execGit).toHaveBeenCalledWith(
      'find "$HOME/projects" -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort',
      "remote-host",
    );
    expect(result).toEqual(["projects"]);
  });
});

describe("projectService remote registration flow", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.getDefaultSessionType.mockResolvedValue("tmux");
  });

  it("원격 스캔은 세션 의존성 검증 없이 git 저장소 검색을 진행한다", async () => {
    mocks.scanGitRepos.mockResolvedValue(["/workspace/api"]);
    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([]),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "project-1", ...value })),
    });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue(null),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => value),
    });
    mocks.getDefaultBranch.mockResolvedValue("main");
    mocks.createSessionWithoutWorktree.mockResolvedValue({ sessionName: "api-main" });

    const { scanAndRegisterProjects } = await import("@/desktop/main/services/projectService");
    const { ensureRemoteSessionDependency } = await import("@/lib/remoteSessionDependency");

    const result = await scanAndRegisterProjects("~/workspace", "remote-host");

    expect(mocks.scanGitRepos).toHaveBeenCalledWith("~/workspace", "remote-host");
    expect(ensureRemoteSessionDependency).not.toHaveBeenCalled();
    expect(result.registered).toEqual(["api"]);
  });

  it("원격 세션 생성이 실패해도 프로젝트 등록은 유지한다", async () => {
    mocks.validateGitRepo.mockResolvedValue(true);
    mocks.getDefaultBranch.mockResolvedValue("main");
    mocks.createSessionWithoutWorktree.mockRejectedValue(new Error("tmux missing"));

    const remove = vi.fn();
    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([]),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "project-1", ...value })),
      remove,
    });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue(null),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "task-1", ...value })),
    });

    const { registerProject } = await import("@/desktop/main/services/projectService");
    const result = await registerProject("api", "/workspace/api", "remote-host");

    expect(result.success).toBe(true);
    expect(remove).not.toHaveBeenCalled();
    expect(result.project).toMatchObject({ name: "api", sshHost: "remote-host" });
  });

  it("같은 이름의 로컬 프로젝트가 있어도 원격 프로젝트는 구분된 이름으로 등록한다", async () => {
    mocks.validateGitRepo.mockResolvedValue(true);
    mocks.getDefaultBranch.mockResolvedValue("main");
    mocks.createSessionWithoutWorktree.mockResolvedValue({ sessionName: "kanvibe-main" });

    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([
        {
          id: "project-local",
          name: "kanvibe",
          repoPath: "/workspace/kanvibe",
          sshHost: null,
        },
      ]),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "project-remote", ...value })),
      remove: vi.fn(),
    });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue(null),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "task-1", ...value })),
    });

    const { registerProject } = await import("@/desktop/main/services/projectService");
    const result = await registerProject(
      "kanvibe",
      "/home/rookedsysc/Documents/kanvibe/kanvibe",
      "remote-host",
    );

    expect(result.success).toBe(true);
    expect(result.project).toMatchObject({
      name: "kanvibe/kanvibe",
      sshHost: "remote-host",
    });
  });

  it("같은 sshHost와 repoPath 조합은 이름이 달라도 중복 등록하지 않는다", async () => {
    mocks.validateGitRepo.mockResolvedValue(true);

    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([
        {
          id: "project-1",
          name: "kanvibe",
          repoPath: "/workspace/kanvibe",
          sshHost: "remote-host",
        },
      ]),
    });

    const { registerProject } = await import("@/desktop/main/services/projectService");
    const result = await registerProject("kanvibe-remote", "/workspace/kanvibe", "remote-host");

    expect(result).toEqual({
      success: false,
      error: "이미 등록된 프로젝트입니다.",
    });
  });
});

describe("projectService local hook installation", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.getDefaultSessionType.mockResolvedValue("tmux");
  });

  it("로컬 Claude hook 설치 시 서버 URL과 토큰을 함께 전달한다", async () => {
    const task = {
      id: "task-1",
      worktreePath: "/workspace/task-1",
      project: {
        id: "project-1",
        repoPath: "/workspace/repo",
        sshHost: null,
      },
    };

    mocks.getTaskRepository.mockResolvedValue({
      findOne: vi.fn().mockResolvedValue(task),
    });
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });

    const { installTaskHooks } = await import("@/desktop/main/services/projectService");

    await installTaskHooks(task.id);

    expect(mocks.getHookServerUrl).toHaveBeenCalledWith(null);
    expect(mocks.getHookServerToken).toHaveBeenCalled();
    expect(mocks.setupClaudeHooks).toHaveBeenCalledWith(
      "/workspace/task-1",
      "task-1",
      "http://localhost:9736",
      "desktop-hook-token",
    );
  });

  it("로컬 OpenCode hook 설치 시 서버 URL과 토큰을 함께 전달한다", async () => {
    const task = {
      id: "task-2",
      worktreePath: null,
      project: {
        id: "project-1",
        repoPath: "/workspace/repo",
        sshHost: null,
      },
    };

    mocks.getTaskRepository.mockResolvedValue({
      findOne: vi.fn().mockResolvedValue(task),
    });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });

    const { installTaskOpenCodeHooks } = await import("@/desktop/main/services/projectService");

    await installTaskOpenCodeHooks(task.id);

    expect(mocks.getHookServerUrl).toHaveBeenCalledWith(null);
    expect(mocks.getHookServerToken).toHaveBeenCalled();
    expect(mocks.setupOpenCodeHooks).toHaveBeenCalledWith(
      "/workspace/repo",
      "task-2",
      "http://localhost:9736",
      "desktop-hook-token",
    );
  });

  it("로컬 기본 브랜치 태스크가 TODO로 남아도 repo 경로에 hooks를 자동 설치한다", async () => {
    // Given
    mocks.validateGitRepo.mockResolvedValue(true);
    mocks.getDefaultBranch.mockResolvedValue("main");
    mocks.createSessionWithoutWorktree.mockRejectedValue(new Error("tmux missing"));

    const remove = vi.fn();
    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([]),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "project-1", ...value })),
      remove,
    });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue(null),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "task-1", ...value })),
    });

    const { registerProject } = await import("@/desktop/main/services/projectService");

    // When
    const result = await registerProject("api", "/workspace/api");

    // Then
    expect(result.success).toBe(true);
    expect(remove).not.toHaveBeenCalled();
    expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
      "/workspace/api",
      "task-1",
      null,
    );
  });

  it("이미 등록된 레거시 프로젝트 목록 조회는 즉시 반환하고 root hooks 복구는 백그라운드에서 진행한다", async () => {
    vi.useFakeTimers();

    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([
        {
          id: "project-1",
          name: "api",
          repoPath: "/workspace/api",
          defaultBranch: "main",
          sshHost: null,
        },
      ]),
    });
    const findOneBy = vi.fn()
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce({
        id: "task-main",
        branchName: "main",
        projectId: "project-1",
        baseBranch: "main",
        worktreePath: "/workspace/api",
        sshHost: null,
      });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy,
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "task-main", ...value })),
    });
    mocks.createSessionWithoutWorktree.mockRejectedValue(new Error("tmux missing"));

    const { getAllProjects } = await import("@/desktop/main/services/projectService");

    try {
      await expect(getAllProjects()).resolves.toEqual([
        {
          id: "project-1",
          name: "api",
          repoPath: "/workspace/api",
          defaultBranch: "main",
          sshHost: null,
        },
      ]);

      expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
      expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);

      await vi.runAllTimersAsync();

      expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
        "/workspace/api",
        "task-main",
        null,
      );
      expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("기존 root hooks 백그라운드 복구가 한 번 실패해도 다음 조회에서 다시 재시도한다", async () => {
    vi.useFakeTimers();

    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([
        {
          id: "project-1",
          name: "api",
          repoPath: "/workspace/api",
          defaultBranch: "main",
          sshHost: null,
        },
      ]),
    });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue({
        id: "task-main",
        branchName: "main",
        projectId: "project-1",
        baseBranch: "main",
        worktreePath: "/workspace/api",
        sshHost: null,
      }),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "task-main", ...value })),
    });
    mocks.readHookTaskIdFile.mockResolvedValue(null);
    mocks.installKanvibeHooks
      .mockRejectedValueOnce(new Error("hook server offline"))
      .mockResolvedValueOnce(undefined);

    const { getAllProjects } = await import("@/desktop/main/services/projectService");

    try {
      await getAllProjects();
      await vi.runAllTimersAsync();

      expect(mocks.installKanvibeHooks).toHaveBeenCalledTimes(1);
      expect(mocks.broadcastBoardUpdate).not.toHaveBeenCalled();

      await getAllProjects();
      await vi.runAllTimersAsync();

      expect(mocks.installKanvibeHooks).toHaveBeenCalledTimes(2);
      expect(mocks.installKanvibeHooks).toHaveBeenLastCalledWith(
        "/workspace/api",
        "task-main",
        null,
      );
      expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("레거시 프로젝트에서 기본 브랜치 task가 없어도 project hook 재설치를 계속 진행한다", async () => {
    const project = {
      id: "project-1",
      name: "api",
      repoPath: "/workspace/api",
      defaultBranch: "main",
      sshHost: null,
    };

    mocks.getProjectRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue(project),
    });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn()
        .mockResolvedValueOnce(null)
        .mockResolvedValueOnce(null)
        .mockResolvedValueOnce(null),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "task-main", ...value })),
    });
    mocks.createSessionWithoutWorktree.mockRejectedValue(new Error("tmux missing"));
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });

    const { installProjectHooks } = await import("@/desktop/main/services/projectService");

    await expect(installProjectHooks(project.id)).resolves.toEqual({
      success: true,
      status: { installed: true },
    });

    expect(mocks.setupClaudeHooks).toHaveBeenCalledWith(
      "/workspace/api",
      "task-main",
      "http://localhost:9736",
      "desktop-hook-token",
    );
  });

  it("스캔으로 발견한 로컬 TODO worktree 태스크에도 hooks를 자동 설치한다", async () => {
    // Given
    mocks.scanGitRepos.mockResolvedValue(["/workspace/api"]);
    mocks.getDefaultBranch.mockResolvedValue("main");
    mocks.createSessionWithoutWorktree.mockResolvedValue({ sessionName: "api-main" });
    mocks.readHookTaskIdFile.mockResolvedValue("task-main");
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: true });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: true });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });
    mocks.listWorktrees.mockResolvedValue([
      {
        path: "/workspace/api-worktrees/feature-login",
        branch: "feature-login",
        isBare: false,
      },
    ]);
    mocks.formatSessionName.mockReturnValue("api-feature-login");
    mocks.isSessionAlive.mockResolvedValue(false);

    const projectSave = vi.fn(async (value) => ({ id: "project-1", ...value }));
    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn()
        .mockResolvedValueOnce([])
        .mockResolvedValueOnce([
          {
            id: "project-1",
            name: "api",
            repoPath: "/workspace/api",
            defaultBranch: "main",
            sshHost: null,
          },
        ]),
      create: vi.fn((value) => value),
      save: projectSave,
      remove: vi.fn(),
    });

    const findOneBy = vi.fn()
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce({
        id: "task-main",
        branchName: "main",
        projectId: "project-1",
        baseBranch: "main",
        worktreePath: "/workspace/api",
        sshHost: null,
      })
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(null);
    const taskSave = vi.fn()
      .mockImplementationOnce(async (value) => ({ id: "task-main", ...value }))
      .mockImplementationOnce(async (value) => ({ id: "task-worktree", ...value }));
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy,
      create: vi.fn((value) => value),
      save: taskSave,
    });

    const { scanAndRegisterProjects } = await import("@/desktop/main/services/projectService");

    // When
    const result = await scanAndRegisterProjects("/workspace");

    // Then
    expect(result.worktreeTasks).toContain("feature-login");
    expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
      "/workspace/api-worktrees/feature-login",
      "task-worktree",
      null,
    );
  });
});

describe("projectService remote hook and AI session support", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.getDefaultSessionType.mockResolvedValue("tmux");
  });

  it("원격 태스크의 Claude hook 상태도 sshHost 기준으로 조회한다", async () => {
    // Given
    const task = {
      id: "task-remote",
      worktreePath: "/remote/worktree",
      project: {
        id: "project-1",
        repoPath: "/remote/repo",
        sshHost: "remote-host",
      },
    };

    mocks.getTaskRepository.mockResolvedValue({
      findOne: vi.fn().mockResolvedValue(task),
    });
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });

    const { getTaskHooksStatus } = await import("@/desktop/main/services/projectService");

    // When
    await getTaskHooksStatus(task.id);

    // Then
    expect(mocks.getClaudeHooksStatus).toHaveBeenCalledWith(
      "/remote/worktree",
      "task-remote",
      "remote-host",
    );
  });

  it("원격 태스크에서 hook 재설치를 누르면 공통 installer를 다시 실행한다", async () => {
    // Given
    const task = {
      id: "task-remote",
      worktreePath: "/remote/worktree",
      project: {
        id: "project-1",
        repoPath: "/remote/repo",
        sshHost: "remote-host",
      },
    };

    mocks.getTaskRepository.mockResolvedValue({
      findOne: vi.fn().mockResolvedValue(task),
    });
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });

    const { installTaskHooks } = await import("@/desktop/main/services/projectService");

    // When
    await installTaskHooks(task.id);

    // Then
    expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
      "/remote/worktree",
      "task-remote",
      "remote-host",
    );
  });

  it("원격 AI 세션 집계에도 sshHost를 전달한다", async () => {
    // Given
    const task = {
      id: "task-remote",
      worktreePath: "/remote/worktree",
      project: {
        id: "project-1",
        repoPath: "/remote/repo",
        sshHost: "remote-host",
      },
    };

    mocks.getTaskRepository.mockResolvedValue({
      findOne: vi.fn().mockResolvedValue(task),
    });
    mocks.aggregateAiSessions.mockResolvedValue({
      isRemote: true,
      targetPath: "/remote/worktree",
      repoPath: "/remote/repo",
      sessions: [],
      sources: [],
    });

    const { getTaskAiSessions } = await import("@/desktop/main/services/projectService");

    // When
    await getTaskAiSessions(task.id, true, "claude");

    // Then
    expect(mocks.aggregateAiSessions).toHaveBeenCalledWith({
      worktreePath: "/remote/worktree",
      repoPath: "/remote/repo",
      includeRepoSessions: true,
      query: "claude",
      sshHost: "remote-host",
    });
  });
});
