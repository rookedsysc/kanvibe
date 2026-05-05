import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  execGit: vi.fn(),
  homedir: vi.fn(() => "/home/tester"),
  validateGitRepo: vi.fn(),
  getDefaultBranch: vi.fn(),
  listBranches: vi.fn(),
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
  aggregateAiSessions: vi.fn(),
  getAiSessionDetail: vi.fn(),
  installKanvibeHooks: vi.fn(),
  scheduleKanvibeHooksInstall: vi.fn(),
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
  listBranches: mocks.listBranches,
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
  scheduleKanvibeHooksInstall: mocks.scheduleKanvibeHooksInstall,
}));

describe("projectService.listSubdirectories", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.execGit.mockResolvedValue("");
    mocks.listBranches.mockResolvedValue(["main", "develop"]);
    mocks.getDefaultSessionType.mockResolvedValue("tmux");
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: false });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: false });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: false });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: false });
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

  it("원격 프로젝트 브랜치 목록은 blocking fetch 없이 조회한다", async () => {
    // Given
    const findOneBy = vi.fn().mockResolvedValue({
      id: "project-remote",
      repoPath: "/remote/repo",
      sshHost: "remote-host",
    });
    mocks.getProjectRepository.mockResolvedValue({ findOneBy });
    const { getProjectBranches } = await import("@/desktop/main/services/projectService");

    // When
    const result = await getProjectBranches("project-remote");

    // Then
    expect(result).toEqual(["main", "develop"]);
    expect(mocks.listBranches).toHaveBeenCalledWith(
      "/remote/repo",
      "remote-host",
      { refresh: false },
    );
  });
});

describe("projectService remote registration flow", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.execGit.mockResolvedValue("");
    mocks.getDefaultSessionType.mockResolvedValue("tmux");
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: false });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: false });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: false });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: false });
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

  it("원격 프로젝트 기본 브랜치 task는 자동 tmux 세션을 만들지 않는다", async () => {
    mocks.validateGitRepo.mockResolvedValue(true);
    mocks.getDefaultBranch.mockResolvedValue("main");

    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([]),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "project-1", ...value })),
      remove: vi.fn(),
    });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue(null),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "task-main", ...value })),
    });

    const { registerProject } = await import("@/desktop/main/services/projectService");

    const result = await registerProject("api", "/remote/api", "remote-host");

    expect(result.success).toBe(true);
    expect(mocks.createSessionWithoutWorktree).not.toHaveBeenCalled();
  });

  it("bare common repo 형태의 기본 브랜치 task는 worktreePath를 null로 저장한다", async () => {
    mocks.validateGitRepo.mockResolvedValue(true);
    mocks.getDefaultBranch.mockResolvedValue("main");
    mocks.execGit.mockResolvedValue("/remote/repo/.git");
    mocks.listWorktrees.mockResolvedValue([
      {
        path: "/remote/repo__worktrees/feature-login",
        branch: "feature-login",
        isBare: false,
      },
    ]);
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: true });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: true });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });

    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([]),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "project-1", ...value })),
      remove: vi.fn(),
    });

    const save = vi.fn(async (value) => ({ id: "task-main", ...value }));
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue(null),
      create: vi.fn((value) => value),
      save,
    });

    const { registerProject } = await import("@/desktop/main/services/projectService");

    const result = await registerProject("repo", "/remote/repo", "remote-host");

    expect(result.success).toBe(true);
    expect(save).toHaveBeenCalledWith(expect.objectContaining({
      branchName: "main",
      worktreePath: null,
      sshHost: "remote-host",
    }));
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
    mocks.execGit.mockResolvedValue("");
    mocks.getDefaultSessionType.mockResolvedValue("tmux");
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: false });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: false });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: false });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: false });
  });

  it("로컬 Claude hook 설치 시 공통 installer로 모든 hooks를 다시 설치한다", async () => {
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

    expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
      "/workspace/task-1",
      "task-1",
      null,
    );
    expect(mocks.setupClaudeHooks).not.toHaveBeenCalled();
  });

  it("로컬 OpenCode hook 설치 시 공통 installer로 모든 hooks를 다시 설치한다", async () => {
    const task = {
      id: "task-2",
      worktreePath: null,
      project: {
        id: "project-1",
        repoPath: "/workspace/repo",
        defaultBranch: "main",
        sshHost: null,
      },
    };

    mocks.getTaskRepository.mockResolvedValue({
      findOne: vi.fn().mockResolvedValue(task),
      findOneBy: vi.fn().mockResolvedValue({
        id: "task-main",
        branchName: "main",
        projectId: "project-1",
        worktreePath: "/workspace/repo",
        sshHost: null,
      }),
      save: vi.fn(async (value) => value),
    });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });

    const { installTaskOpenCodeHooks } = await import("@/desktop/main/services/projectService");

    await installTaskOpenCodeHooks(task.id);

    expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
      "/workspace/repo",
      "task-main",
      null,
    );
    expect(mocks.setupOpenCodeHooks).not.toHaveBeenCalled();
  });

  it("프로젝트 루트 경로 task의 Claude hook 상태는 현재 root task 기준으로 조회한다", async () => {
    const task = {
      id: "stale-task",
      worktreePath: "/workspace/repo",
      project: {
        id: "project-1",
        repoPath: "/workspace/repo",
        defaultBranch: "main",
        sshHost: null,
      },
    };

    mocks.getTaskRepository.mockResolvedValue({
      findOne: vi.fn().mockResolvedValue(task),
      findOneBy: vi.fn().mockResolvedValue({
        id: "task-main",
        branchName: "main",
        projectId: "project-1",
        worktreePath: "/workspace/repo",
        sshHost: null,
      }),
      save: vi.fn(async (value) => value),
    });
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });

    const { getTaskHooksStatus } = await import("@/desktop/main/services/projectService");

    await getTaskHooksStatus(task.id);

    expect(mocks.getClaudeHooksStatus).toHaveBeenCalledWith(
      "/workspace/repo",
      "task-main",
      null,
    );
  });

  it("프로젝트 루트 경로 task에서 Claude hook 재설치를 누르면 공통 installer로 현재 root task에 다시 바인딩한다", async () => {
    const task = {
      id: "stale-task",
      worktreePath: "/workspace/repo",
      project: {
        id: "project-1",
        repoPath: "/workspace/repo",
        defaultBranch: "main",
        sshHost: null,
      },
    };

    mocks.getTaskRepository.mockResolvedValue({
      findOne: vi.fn().mockResolvedValue(task),
      findOneBy: vi.fn().mockResolvedValue({
        id: "task-main",
        branchName: "main",
        projectId: "project-1",
        worktreePath: "/workspace/repo",
        sshHost: null,
      }),
      save: vi.fn(async (value) => value),
    });
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });

    const { installTaskHooks } = await import("@/desktop/main/services/projectService");

    await installTaskHooks(task.id);

    expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
      "/workspace/repo",
      "task-main",
      null,
    );
    expect(mocks.setupClaudeHooks).not.toHaveBeenCalled();
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

      expect(mocks.listWorktrees).not.toHaveBeenCalled();
      expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
      expect(mocks.broadcastBoardUpdate).not.toHaveBeenCalled();

      await vi.runAllTimersAsync();

      expect(mocks.broadcastBoardUpdate).toHaveBeenCalled();
      expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
        "/workspace/api",
        "task-main",
        null,
      );
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
    mocks.listWorktrees.mockResolvedValue([
      {
        path: "/workspace/api",
        branch: "main",
        isBare: false,
      },
    ]);
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

  it("원격 hook 상태 조회가 SSH 연결 오류로 실패해도 백그라운드 복구 로그를 남기지 않는다", async () => {
    vi.useFakeTimers();

    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const remoteConnectionError = new Error("remote-host 원격 명령 실패: Connection reset by 100.73.171.123 port 22");

    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([
        {
          id: "project-1",
          name: "prompt",
          repoPath: "/remote/prompt",
          defaultBranch: "main",
          sshHost: "remote-host",
        },
      ]),
    });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue({
        id: "task-main",
        branchName: "main",
        projectId: "project-1",
        baseBranch: "main",
        worktreePath: "/remote/prompt",
        sshHost: "remote-host",
      }),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "task-main", ...value })),
    });
    mocks.getClaudeHooksStatus.mockRejectedValueOnce(remoteConnectionError);

    const { getAllProjects } = await import("@/desktop/main/services/projectService");

    try {
      await getAllProjects();
      await vi.runAllTimersAsync();

      expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
      expect(consoleErrorSpy).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("원격 hook 설치가 SSH 연결 오류로 실패해도 백그라운드 복구 로그를 남기지 않는다", async () => {
    vi.useFakeTimers();

    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const remoteConnectionError = new Error("remote-host 원격 명령 실패: Connection closed by 100.73.171.123 port 22");

    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([
        {
          id: "project-1",
          name: "techtaurant-be",
          repoPath: "/remote/techtaurant-be",
          defaultBranch: "dev",
          sshHost: "remote-host",
        },
      ]),
    });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue({
        id: "task-main",
        branchName: "dev",
        projectId: "project-1",
        baseBranch: "dev",
        worktreePath: "/remote/techtaurant-be",
        sshHost: "remote-host",
      }),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "task-main", ...value })),
    });
    mocks.installKanvibeHooks.mockRejectedValueOnce(remoteConnectionError);

    const { getAllProjects } = await import("@/desktop/main/services/projectService");

    try {
      await getAllProjects();
      await vi.runAllTimersAsync();

      expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
        "/remote/techtaurant-be",
        "task-main",
        "remote-host",
      );
      expect(consoleErrorSpy).not.toHaveBeenCalled();
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

    expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
      "/workspace/api",
      "task-main",
      null,
    );
    expect(mocks.setupClaudeHooks).not.toHaveBeenCalled();
  });

  it("스캔으로 발견한 로컬 TODO worktree 태스크에도 hooks를 자동 설치한다", async () => {
    // Given
    mocks.scanGitRepos.mockResolvedValue(["/workspace/api"]);
    mocks.getDefaultBranch.mockResolvedValue("main");
    mocks.createSessionWithoutWorktree.mockResolvedValue({ sessionName: "api-main" });
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

    let rootTask: Record<string, unknown> | null = null;
    const findOneBy = vi.fn(async (criteria: { branchName?: string; projectId?: string | null }) => {
      if (criteria.projectId === "project-1" && criteria.branchName === "main") {
        return rootTask;
      }

      if (criteria.projectId === "project-1" && criteria.branchName === "feature-login") {
        return null;
      }

      if ((criteria.projectId === null || criteria.projectId === undefined) && criteria.branchName === "feature-login") {
        return null;
      }

      return null;
    });
    const taskSave = vi.fn(async (value) => {
      if (value.branchName === "main") {
        rootTask = { id: "task-main", ...value };
        return rootTask;
      }

      return { id: "task-worktree", ...value };
    });
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

  it("worktree 경로만 스캔돼도 common repo 기준 기존 원격 프로젝트에 TODO 태스크를 등록한다", async () => {
    mocks.scanGitRepos.mockResolvedValue(["/remote/repo__worktrees/feature-login"]);
    mocks.execGit.mockImplementation(async (command: string) => {
      if (command.includes("rev-parse --path-format=absolute --git-common-dir")) {
        return "/remote/repo/.git";
      }
      return "";
    });
    mocks.getDefaultBranch.mockResolvedValue("main");
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: true });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: true });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });
    mocks.listWorktrees.mockResolvedValue([
      {
        path: "/remote/repo__worktrees/feature-login",
        branch: "feature-login",
        isBare: false,
      },
    ]);
    mocks.formatSessionName.mockReturnValue("repo-feature-login");
    mocks.isSessionAlive.mockResolvedValue(false);

    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([
        {
          id: "project-1",
          name: "repo",
          repoPath: "/remote/repo",
          defaultBranch: "main",
          sshHost: "remote-host",
        },
      ]),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "project-1", ...value })),
      remove: vi.fn(),
    });

    const taskRepoSave = vi.fn(async (value) => ({ id: "task-worktree", ...value }));
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn(async (criteria: { branchName?: string; projectId?: string | null }) => {
        if (criteria.projectId === "project-1" && criteria.branchName === "main") {
          return {
            id: "task-main",
            branchName: "main",
            projectId: "project-1",
            baseBranch: "main",
            worktreePath: null,
            sshHost: "remote-host",
          };
        }

        return null;
      }),
      create: vi.fn((value) => value),
      save: taskRepoSave,
    });

    const { scanAndRegisterProjects } = await import("@/desktop/main/services/projectService");

    const result = await scanAndRegisterProjects("/remote/workspace", "remote-host");

    expect(result.worktreeTasks).toContain("feature-login");
    expect(mocks.listWorktrees).toHaveBeenCalledWith("/remote/repo", "remote-host");
    expect(mocks.scheduleKanvibeHooksInstall).toHaveBeenCalledWith(
      "/remote/repo__worktrees/feature-login",
      "task-worktree",
      "remote-host",
      expect.objectContaining({
        onSuccess: expect.any(Function),
        onFailure: expect.any(Function),
      }),
    );
    expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
  });

  it("원격 스캔은 root hook 상태 조회 실패와 무관하게 worktree 등록을 계속 진행한다", async () => {
    vi.useFakeTimers();

    try {
      const remoteConnectionError = new Error("remote-host 원격 명령 실패: Connection reset by 100.73.171.123 port 22");

      mocks.scanGitRepos.mockResolvedValue(["/remote/repo"]);
      mocks.getDefaultBranch.mockResolvedValue("main");
      mocks.getClaudeHooksStatus.mockRejectedValue(remoteConnectionError);
      mocks.getGeminiHooksStatus.mockRejectedValue(remoteConnectionError);
      mocks.getCodexHooksStatus.mockRejectedValue(remoteConnectionError);
      mocks.getOpenCodeHooksStatus.mockRejectedValue(remoteConnectionError);
      mocks.listWorktrees.mockResolvedValue([
        {
          path: "/remote/repo__worktrees/feature-login",
          branch: "feature-login",
          isBare: false,
        },
      ]);
      mocks.formatSessionName.mockReturnValue("repo-feature-login");
      mocks.isSessionAlive.mockResolvedValue(false);

      mocks.getProjectRepository.mockResolvedValue({
        find: vi.fn()
          .mockResolvedValueOnce([])
          .mockResolvedValueOnce([
            {
              id: "project-1",
              name: "repo",
              repoPath: "/remote/repo",
              defaultBranch: "main",
              sshHost: "remote-host",
            },
          ]),
        create: vi.fn((value) => value),
        save: vi.fn(async (value) => ({ id: "project-1", ...value })),
        remove: vi.fn(),
      });

      let rootTask: Record<string, unknown> | null = null;
      const taskSave = vi.fn(async (value) => {
        if (value.branchName === "main") {
          rootTask = { id: "task-main", ...value };
          return rootTask;
        }

        return { id: "task-worktree", ...value };
      });

      mocks.getTaskRepository.mockResolvedValue({
        findOneBy: vi.fn(async (criteria: { branchName?: string; projectId?: string | null }) => {
          if (criteria.projectId === "project-1" && criteria.branchName === "main") {
            return rootTask;
          }

          return null;
        }),
        create: vi.fn((value) => value),
        save: taskSave,
      });

      const { scanAndRegisterProjects } = await import("@/desktop/main/services/projectService");

      const result = await scanAndRegisterProjects("/remote/workspace", "remote-host");

      expect(result.worktreeTasks).toContain("feature-login");
      expect(mocks.getClaudeHooksStatus).not.toHaveBeenCalled();
      expect(mocks.getGeminiHooksStatus).not.toHaveBeenCalled();
      expect(mocks.getCodexHooksStatus).not.toHaveBeenCalled();
      expect(mocks.getOpenCodeHooksStatus).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("프로젝트 스캔 후 worktree 후속 처리는 현재 스캔에서 찾은 저장소만 대상으로 한다", async () => {
    mocks.scanGitRepos.mockResolvedValue(["/workspace/api"]);
    mocks.getDefaultBranch.mockResolvedValue("main");
    mocks.createSessionWithoutWorktree.mockResolvedValue({ sessionName: "api-main" });
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: true });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: true });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });
    mocks.listWorktrees.mockResolvedValue([]);

    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn()
        .mockResolvedValueOnce([
          {
            id: "project-2",
            name: "web",
            repoPath: "/workspace/web",
            defaultBranch: "main",
            sshHost: null,
          },
        ])
        .mockResolvedValueOnce([
          {
            id: "project-1",
            name: "api",
            repoPath: "/workspace/api",
            defaultBranch: "main",
            sshHost: null,
          },
          {
            id: "project-2",
            name: "web",
            repoPath: "/workspace/web",
            defaultBranch: "main",
            sshHost: null,
          },
        ]),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "project-1", ...value })),
      remove: vi.fn(),
    });

    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue(null),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({
        id: value.branchName === "main" ? "task-main" : "task-worktree",
        ...value,
      })),
    });

    const { scanAndRegisterProjects } = await import("@/desktop/main/services/projectService");

    await scanAndRegisterProjects("/workspace");

    expect(mocks.listWorktrees).toHaveBeenCalledWith("/workspace/api", null);
    expect(mocks.listWorktrees).not.toHaveBeenCalledWith("/workspace/web", null);
  });

  it("브랜치명만 같은 다른 orphan task는 현재 프로젝트 worktree에 재사용하지 않는다", async () => {
    mocks.scanGitRepos.mockResolvedValue(["/workspace/api"]);
    mocks.getDefaultBranch.mockResolvedValue("main");
    mocks.createSessionWithoutWorktree.mockResolvedValue({ sessionName: "api-main" });
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
      save: vi.fn(async (value) => ({ id: "project-1", ...value })),
      remove: vi.fn(),
    });

    let rootTask: Record<string, unknown> | null = null;
    const findOneBy = vi.fn(async (criteria: { branchName?: string; projectId?: string | null }) => {
      if (criteria.projectId === "project-1" && criteria.branchName === "main") {
        return rootTask;
      }

      if (criteria.projectId === "project-1" && criteria.branchName === "feature-login") {
        return null;
      }

      if ((criteria.projectId === null || criteria.projectId === undefined) && criteria.branchName === "feature-login") {
        return {
          id: "orphan-1",
          branchName: "feature-login",
          projectId: null,
          worktreePath: "/other/repo__worktrees/feature-login",
          sshHost: null,
        };
      }

      return null;
    });
    const create = vi.fn((value) => value);
    const save = vi.fn(async (value) => {
      if (value.branchName === "main") {
        rootTask = { id: "task-main", ...value };
        return rootTask;
      }

      return { id: "task-worktree", ...value };
    });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy,
      create,
      save,
    });

    const { scanAndRegisterProjects } = await import("@/desktop/main/services/projectService");

    await scanAndRegisterProjects("/workspace");

    expect(create).toHaveBeenCalledWith(expect.objectContaining({
      branchName: "feature-login",
      worktreePath: "/workspace/api-worktrees/feature-login",
      projectId: "project-1",
    }));
    expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
      "/workspace/api-worktrees/feature-login",
      "task-worktree",
      null,
    );
  });

  it("등록된 프로젝트 background sync는 새 worktree를 TODO task로 등록한다", async () => {
    // Given
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: true });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: true });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });
    mocks.listWorktrees.mockResolvedValue([
      {
        path: "/workspace/api__worktrees/feature-sync",
        branch: "feature-sync",
        isBare: false,
      },
    ]);
    mocks.formatSessionName.mockReturnValue("api-feature-sync");
    mocks.isSessionAlive.mockResolvedValue(false);

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

    const taskSave = vi.fn(async (value) => ({ id: value.branchName === "main" ? "task-main" : "task-worktree", ...value }));
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn(async (criteria: { branchName?: string; projectId?: string | null }) => {
        if (criteria.projectId === "project-1" && criteria.branchName === "main") {
          return {
            id: "task-main",
            branchName: "main",
            projectId: "project-1",
            baseBranch: "main",
            worktreePath: null,
            sshHost: null,
          };
        }

        return null;
      }),
      create: vi.fn((value) => value),
      save: taskSave,
    });

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    // When
    const result = await syncRegisteredProjectWorktrees();

    // Then
    expect(result.worktreeTasks).toContain("feature-sync");
    expect(result.registeredWorktrees).toEqual([
      {
        taskId: "task-worktree",
        projectName: "api",
        branchName: "feature-sync",
        worktreePath: "/workspace/api__worktrees/feature-sync",
        sshHost: null,
      },
    ]);
    expect(taskSave).toHaveBeenCalledWith(expect.objectContaining({
      branchName: "feature-sync",
      projectId: "project-1",
      status: "todo",
    }));
    expect(mocks.installKanvibeHooks).toHaveBeenCalledWith(
      "/workspace/api__worktrees/feature-sync",
      "task-worktree",
      null,
    );
  });

  it("원격 등록 프로젝트의 새 worktree hooks 설치는 백그라운드로 예약한다", async () => {
    vi.useFakeTimers();

    try {
      // Given
      mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });
      mocks.getGeminiHooksStatus.mockResolvedValue({ installed: true });
      mocks.getCodexHooksStatus.mockResolvedValue({ installed: true });
      mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });
      mocks.listWorktrees.mockResolvedValue([
        {
          path: "/remote/api__worktrees/feature-sync",
          branch: "feature-sync",
          isBare: false,
        },
      ]);
      mocks.formatSessionName.mockReturnValue("api-feature-sync");
      mocks.isSessionAlive.mockResolvedValue(false);

      mocks.getProjectRepository.mockResolvedValue({
        find: vi.fn().mockResolvedValue([
          {
            id: "project-1",
            name: "api",
            repoPath: "/remote/api",
            defaultBranch: "main",
            sshHost: "remote-host",
          },
        ]),
      });

      const taskSave = vi.fn(async (value) => ({ id: value.branchName === "main" ? "task-main" : "task-worktree", ...value }));
      mocks.getTaskRepository.mockResolvedValue({
        findOneBy: vi.fn(async (criteria: { branchName?: string; projectId?: string | null }) => {
          if (criteria.projectId === "project-1" && criteria.branchName === "main") {
            return {
              id: "task-main",
              branchName: "main",
              projectId: "project-1",
              baseBranch: "main",
              worktreePath: "/remote/api",
              sshHost: "remote-host",
            };
          }

          return null;
        }),
        create: vi.fn((value) => value),
        save: taskSave,
      });

      const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

      // When
      const result = await syncRegisteredProjectWorktrees();

      // Then
      expect(result.worktreeTasks).toContain("feature-sync");
      expect(mocks.scheduleKanvibeHooksInstall).toHaveBeenCalledWith(
        "/remote/api__worktrees/feature-sync",
        "task-worktree",
        "remote-host",
        expect.objectContaining({
          onSuccess: expect.any(Function),
          onFailure: expect.any(Function),
        }),
      );
      expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
    } finally {
      vi.clearAllTimers();
      vi.useRealTimers();
    }
  });

  it("등록된 프로젝트 background sync는 프로젝트별 worktree 조회를 직렬로 실행한다", async () => {
    // Given
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: true });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: true });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });
    mocks.isSessionAlive.mockResolvedValue(false);

    const seenRepoPaths: string[] = [];
    const firstCalls = new Set<string>();
    let releaseGate: (() => void) | undefined;
    const gate = new Promise<void>((resolve) => {
      releaseGate = resolve;
    });
    mocks.listWorktrees.mockImplementation(async (repoPath: string) => {
      seenRepoPaths.push(repoPath);
      if (!firstCalls.has(repoPath)) {
        firstCalls.add(repoPath);
        await gate;
      }
      return [];
    });

    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn(async (criteria: { branchName?: string; projectId?: string | null }) => ({
        id: `task-${criteria.projectId}`,
        branchName: criteria.branchName,
        projectId: criteria.projectId,
        baseBranch: criteria.branchName,
        worktreePath: null,
        sshHost: null,
      })),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => value),
    });

    const projects = [
      {
        id: "project-a",
        name: "api-a",
        repoPath: "/workspace/api-a",
        defaultBranch: "main",
        sshHost: null,
      },
      {
        id: "project-b",
        name: "api-b",
        repoPath: "/workspace/api-b",
        defaultBranch: "main",
        sshHost: null,
      },
    ];

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    // When
    const syncPromise = syncRegisteredProjectWorktrees(projects as never);
    await new Promise((resolve) => setTimeout(resolve, 0));

    // Then
    expect(Array.from(firstCalls).filter((repoPath) => repoPath.startsWith("/workspace/api-"))).toEqual(["/workspace/api-a"]);

    releaseGate?.();
    await syncPromise;

    expect(Array.from(firstCalls).filter((repoPath) => repoPath.startsWith("/workspace/api-"))).toEqual(["/workspace/api-a", "/workspace/api-b"]);
  });
});

describe("projectService remote hook and AI session support", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.execGit.mockResolvedValue("");
    mocks.getDefaultSessionType.mockResolvedValue("tmux");
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: false });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: false });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: false });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: false });
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
