import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  execGit: vi.fn(),
  isPathMissing: vi.fn(async (_targetPath: string, _sshHost?: string | null) => false),
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
  resolveProjectIconDataUrl: vi.fn<(repoPath: string, sshHost?: string | null) => Promise<string | null>>(async () => null),
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
  ensureKanvibeHooksCurrent: vi.fn(async () => false),
  getKanvibeHooksStatuses: vi.fn(),
  installKanvibeHooks: vi.fn(),
  installKanvibeHookProvider: vi.fn(),
  scheduleKanvibeHooksInstall: vi.fn(),
  broadcastBoardUpdate: vi.fn(),
  broadcastTaskSessionChanged: vi.fn(),
  detachSession: vi.fn(),
  readTextFile: vi.fn(),
  /** 공유 파일이 없는 저장소가 기본값이라 테스트마다 따로 준비하지 않아도 된다 */
  readTextFiles: vi.fn(async (targetPaths: string[]) => new Map(
    targetPaths.map((targetPath) => [targetPath, { exists: false, content: "" }]),
  )),
  writeTextFile: vi.fn(),
  writeTextFileIfAbsent: vi.fn(),
  quoteShellArgument: vi.fn((value: string) => `'${value}'`),
}));

vi.mock("@/lib/database", () => ({
  getProjectRepository: mocks.getProjectRepository,
  /**
   * task를 만드는 모든 경로가 display rank 조회(findOne)를 거치는데,
   * 테스트마다 필요한 메서드만 담아 repo mock을 짜므로 조회 기본값을 여기서 한 번 채워 준다.
   * 테스트가 직접 findOne을 넘기면 그 값이 이긴다.
   */
  getTaskRepository: async () => {
    const taskRepo = await mocks.getTaskRepository();
    return { find: vi.fn(async () => []), findOne: vi.fn(async () => null), ...taskRepo };
  },
}));

vi.mock("@/entities/Project", () => ({
  Project: class Project {},
}));

vi.mock("@/entities/KanbanTask", () => ({
  KanbanTask: class KanbanTask {},
  TaskStatus: {
    TODO: "todo",
    PROGRESS: "progress",
    PENDING: "pending",
    REVIEW: "review",
    DONE: "done",
  },
  SessionType: {
    TMUX: "tmux",
    HERDR: "herdr",
    ZELLIJ: "zellij",
    TERMINAL: "terminal",
  },
}));

vi.mock("@/entities/PaneLayoutConfig", () => ({
  PaneLayoutConfig: class PaneLayoutConfig {},
  PaneLayoutType: {},
}));

vi.mock("typeorm", () => ({
  IsNull: vi.fn(),
}));

vi.mock("@/lib/gitOperations", async (importOriginal) => ({
  resolvePathForShell: (await importOriginal<typeof import("@/lib/gitOperations")>()).resolvePathForShell,
  validateGitRepo: mocks.validateGitRepo,
  getDefaultBranch: mocks.getDefaultBranch,
  listBranches: mocks.listBranches,
  scanGitRepos: mocks.scanGitRepos,
  listWorktrees: mocks.listWorktrees,
  execGit: mocks.execGit,
}));

vi.mock("@/lib/worktree", () => ({
  isSessionAlive: mocks.isSessionAlive,
  sanitizeHerdrSessionName: (sessionName: string) => `herdr-${sessionName}`,
  formatSessionName: mocks.formatSessionName,
  formatProjectBranchSessionName: mocks.formatSessionName,
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

vi.mock("@/lib/githubProjectIcon", () => ({
  resolveProjectIconDataUrl: mocks.resolveProjectIconDataUrl,
}));

vi.mock("@/lib/boardNotifier", () => ({
  broadcastBoardUpdate: mocks.broadcastBoardUpdate,
  broadcastTaskSessionChanged: mocks.broadcastTaskSessionChanged,
}));

vi.mock("@/lib/terminal", () => ({
  detachSession: mocks.detachSession,
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
  ensureKanvibeHooksCurrent: mocks.ensureKanvibeHooksCurrent,
  getKanvibeHooksStatuses: mocks.getKanvibeHooksStatuses,
  installKanvibeHooks: mocks.installKanvibeHooks,
  installKanvibeHookProvider: mocks.installKanvibeHookProvider,
  scheduleKanvibeHooksInstall: mocks.scheduleKanvibeHooksInstall,
}));

vi.mock("@/lib/hostFileAccess", () => ({
  isPathMissing: mocks.isPathMissing,
  readTextFile: mocks.readTextFile,
  readTextFiles: mocks.readTextFiles,
  writeTextFile: mocks.writeTextFile,
  writeTextFileIfAbsent: mocks.writeTextFileIfAbsent,
  quoteShellArgument: mocks.quoteShellArgument,
}));

describe("projectService sync deleted paths", () => {
  const project = { id: "cleanup-project", name: "api", repoPath: "/workspace/api", defaultBranch: "main", sshHost: null };
  const rootTask = { id: "cleanup-root", projectId: project.id, branchName: "main", baseBranch: "main", worktreePath: project.repoPath, sshHost: null, sessionType: "tmux" };
  const removedTask = { ...rootTask, id: "removed-task", branchName: "removed", worktreePath: "/workspace/api__worktrees/removed" };
  let tasks: typeof rootTask[];
  let projects: typeof project[];

  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.isPathMissing.mockReset().mockResolvedValue(false);
    mocks.execGit.mockResolvedValue("");
    mocks.listWorktrees.mockResolvedValue([{ path: project.repoPath, branch: "main", isBare: false }]);
    tasks = [{ ...rootTask }, { ...removedTask }];
    projects = [{ ...project }];
    const manager = {
      findOneBy: vi.fn(async () => projects[0]),
      delete: vi.fn(async () => { tasks = []; }),
      remove: vi.fn(async () => { projects = []; }),
    };
    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn(async () => projects),
      manager: { transaction: vi.fn(async (action) => action(manager)) },
    });
    mocks.getTaskRepository.mockResolvedValue({
      find: vi.fn(async () => [...tasks]),
      findOneBy: vi.fn(async (criteria: { branchName: string; projectId: string }) => tasks.find((task) => task.branchName === criteria.branchName && task.projectId === criteria.projectId) ?? null),
      save: vi.fn(async (task) => task),
      delete: vi.fn(async (criteria: { id: string }) => {
        const previousCount = tasks.length;
        tasks = tasks.filter((task) => task.id !== criteria.id);
        return { affected: previousCount - tasks.length };
      }),
    });
  });

  it("삭제된 프로젝트는 관련 태스크와 함께 정리하고 다시 만들지 않는다", async () => {
    mocks.isPathMissing.mockResolvedValue(true);
    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");
    const result = await syncRegisteredProjectWorktrees();
    expect(projects).toEqual([]);
    expect(tasks).toEqual([]);
    expect(result.changed).toBe(true);
    expect(mocks.writeTextFile).not.toHaveBeenCalled();
  });

  it("보드를 먼저 조회해도 삭제된 프로젝트 경로를 복구하지 않아 sync로 정리된다", async () => {
    vi.useFakeTimers();
    try {
      mocks.isPathMissing.mockResolvedValue(true);
      const { getAllProjects, syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");
      await getAllProjects();
      await vi.runOnlyPendingTimersAsync();
      expect(mocks.writeTextFile).not.toHaveBeenCalled();
      await syncRegisteredProjectWorktrees();
      expect(projects).toEqual([]);
      expect(tasks).toEqual([]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("느린 호스트의 경로 확인과 worktree 조회가 끝나지 않아도 다른 호스트와 로컬 프로젝트는 진행하고, 같은 호스트는 차례로 확인한다", async () => {
    const slowHostProjects = [
      { ...project, id: "slow-1", name: "slow-1", repoPath: "/slow/one", sshHost: "slow-host" },
      { ...project, id: "slow-2", name: "slow-2", repoPath: "/slow/two", sshHost: "slow-host" },
    ];
    const otherHostProject = { ...project, id: "other-1", name: "other-1", repoPath: "/other/one", sshHost: "other-host" };
    projects = [...slowHostProjects, otherHostProject, { ...project }] as never;
    tasks = [];
    let finishSlowPathCheck!: (isMissing: boolean) => void;
    const slowPathCheck = new Promise<boolean>((resolve) => { finishSlowPathCheck = resolve; });
    mocks.isPathMissing.mockImplementation((targetPath: string) => targetPath === "/slow/one"
      ? slowPathCheck
      : Promise.resolve(false));
    // 한 프로젝트의 동기화가 worktree 목록을 여러 번 읽으므로, 느린 호스트의 읽기는 모두 같은 대기에 묶는다.
    let finishSlowWorktreeList!: (worktrees: []) => void;
    const slowWorktreeList = new Promise<[]>((resolve) => { finishSlowWorktreeList = resolve; });
    mocks.listWorktrees.mockImplementation((repoPath: string) => repoPath === "/slow/one"
      ? slowWorktreeList
      : Promise.resolve([{ path: repoPath, branch: "main", isBare: false }]));
    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    const syncing = syncRegisteredProjectWorktrees();

    // 경로 확인: 다른 호스트와 로컬은 slow-host의 첫 확인을 기다리지 않는다. 같은 호스트의 둘째 프로젝트는 기다린다.
    const checkedPaths = () => mocks.isPathMissing.mock.calls.map(([targetPath]) => targetPath);
    await vi.waitFor(() => expect(checkedPaths()).toEqual(expect.arrayContaining(["/slow/one", "/other/one", project.repoPath])));
    expect(checkedPaths()).not.toContain("/slow/two");
    finishSlowPathCheck(false);

    // worktree 동기화: slow-host의 첫 프로젝트가 걸려 있어도 다른 호스트와 로컬 프로젝트의 조회는 시작한다.
    const listedRepos = () => mocks.listWorktrees.mock.calls.map(([repoPath]) => repoPath);
    await vi.waitFor(() => expect(listedRepos()).toEqual(expect.arrayContaining(["/slow/one", "/other/one", project.repoPath])));
    expect(listedRepos()).not.toContain("/slow/two");

    finishSlowWorktreeList([]);
    await syncing;
    expect(listedRepos()).toContain("/slow/two");
  });

  it("삭제된 worktree만 정리하며 Git 목록에 남아 있어도 다시 만들지 않는다", async () => {
    mocks.isPathMissing.mockImplementation(async (targetPath) => targetPath === removedTask.worktreePath);
    mocks.listWorktrees.mockResolvedValue([
      { path: project.repoPath, branch: "main", isBare: false },
      { path: removedTask.worktreePath, branch: "removed", isBare: false },
    ]);
    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");
    const result = await syncRegisteredProjectWorktrees();
    expect(tasks.map((task) => task.id)).toEqual([rootTask.id]);
    expect(projects).toHaveLength(1);
    expect(result.changed).toBe(true);
    expect(result.registeredWorktrees).toEqual([]);
    expect(mocks.detachSession).not.toHaveBeenCalled();
    expect(mocks.writeTextFile.mock.calls.map(([targetPath]) => targetPath))
      .not.toContain(`${removedTask.worktreePath}/.kanvibe/status.json`);
  });

  it("Git 목록 조회 실패와 경로 없는 태스크는 삭제 근거로 사용하지 않는다", async () => {
    tasks.push({ ...rootTask, id: "session-only", worktreePath: null as unknown as string });
    mocks.listWorktrees.mockResolvedValue([]);
    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");
    await syncRegisteredProjectWorktrees();
    expect(tasks.map((task) => task.id)).toEqual([rootTask.id, removedTask.id, "session-only"]);
    expect(projects).toHaveLength(1);
  });

  it("Git으로 이동한 worktree는 기존 태스크 ID와 메타데이터를 유지한다", async () => {
    const movedPath = "/workspace/api__worktrees/moved";
    Object.assign(tasks[1], { prUrl: "https://github.com/example/api/pull/1", agentType: "codex" });
    mocks.isPathMissing.mockImplementation(async (targetPath) => targetPath === removedTask.worktreePath);
    mocks.listWorktrees.mockResolvedValue([
      { path: project.repoPath, branch: "main", isBare: false },
      { path: movedPath, branch: removedTask.branchName, isBare: false },
    ]);
    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");
    await syncRegisteredProjectWorktrees();
    expect(tasks).toContainEqual(expect.objectContaining({
      id: removedTask.id, worktreePath: movedPath,
      prUrl: "https://github.com/example/api/pull/1", agentType: "codex",
    }));
  });

  it("삭제된 기본 브랜치 worktree를 다시 만들지 않고 프로젝트 기본 태스크를 유지한다", async () => {
    const missingRootPath = "/workspace/api__worktrees/main";
    tasks[0].worktreePath = missingRootPath;
    mocks.isPathMissing.mockImplementation(async (targetPath) => targetPath === missingRootPath);
    mocks.listWorktrees.mockResolvedValue([
      { path: project.repoPath, branch: "", isBare: true },
      { path: missingRootPath, branch: "main", isBare: false },
    ]);
    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");
    await syncRegisteredProjectWorktrees();
    expect(tasks.map((task) => task.id)).toContain(rootTask.id);
    expect(mocks.writeTextFile.mock.calls.map(([targetPath]) => targetPath))
      .not.toContain(`${missingRootPath}/.kanvibe/status.json`);
  });

  it("태스크의 SSH 정보가 비었으면 프로젝트 호스트에서 경로를 확인한다", async () => {
    projects[0] = { ...project, sshHost: "devbox" as unknown as null };
    mocks.isPathMissing.mockImplementation(async (targetPath, host) => targetPath === removedTask.worktreePath && !host);
    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");
    await syncRegisteredProjectWorktrees();
    expect(tasks.map((task) => task.id)).toContain(removedTask.id);
    expect(mocks.isPathMissing).toHaveBeenCalledWith(removedTask.worktreePath, "devbox");
  });

  it.each(["SSH connection refused", "EACCES"])("경로 확인 실패 시 레코드를 보존한다: %s", async (reason) => {
    mocks.isPathMissing.mockRejectedValue(new Error(reason));
    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");
    const result = await syncRegisteredProjectWorktrees();
    expect(tasks).toHaveLength(2);
    expect(projects).toHaveLength(1);
    expect(result.errors.join(" ")).toContain(reason);
    expect(mocks.writeTextFile).not.toHaveBeenCalled();
    mocks.isPathMissing.mockReset().mockResolvedValue(false);
  });
});

describe("projectService.deleteProject", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("프로젝트 삭제 시 KanVibe DB의 관련 task 레코드도 삭제한다", async () => {
    // Given
    const project = {
      id: "project-1",
      name: "api",
      repoPath: "/workspace/api",
      defaultBranch: "main",
      sshHost: null,
    };
    const manager = {
      findOneBy: vi.fn().mockResolvedValue(project),
      delete: vi.fn().mockResolvedValue({ affected: 2 }),
      remove: vi.fn().mockResolvedValue(project),
    };
    const projectRepo = {
      manager: {
        transaction: vi.fn(async (callback: (transactionManager: typeof manager) => Promise<void>) => callback(manager)),
      },
    };
    mocks.getProjectRepository.mockResolvedValue(projectRepo);

    const { deleteProject } = await import("@/desktop/main/services/projectService");

    // When
    const result = await deleteProject("project-1");

    // Then
    expect(result).toBe(true);
    expect(projectRepo.manager.transaction).toHaveBeenCalledTimes(1);
    expect(manager.findOneBy).toHaveBeenCalledWith(expect.any(Function), { id: "project-1" });
    expect(manager.delete).toHaveBeenCalledWith(expect.any(Function), { projectId: "project-1" });
    expect(manager.remove).toHaveBeenCalledWith(project);
    expect(mocks.getTaskRepository).not.toHaveBeenCalled();
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
  });

  it("프로젝트가 없으면 task 삭제와 보드 갱신을 수행하지 않는다", async () => {
    // Given
    const manager = {
      findOneBy: vi.fn().mockResolvedValue(null),
      delete: vi.fn(),
      remove: vi.fn(),
    };
    const projectRepo = {
      manager: {
        transaction: vi.fn(async (callback: (transactionManager: typeof manager) => Promise<void>) => callback(manager)),
      },
    };
    mocks.getProjectRepository.mockResolvedValue(projectRepo);

    const { deleteProject } = await import("@/desktop/main/services/projectService");

    // When
    const result = await deleteProject("missing-project");

    // Then
    expect(result).toBe(false);
    expect(mocks.getTaskRepository).not.toHaveBeenCalled();
    expect(manager.delete).not.toHaveBeenCalled();
    expect(manager.remove).not.toHaveBeenCalled();
    expect(mocks.broadcastBoardUpdate).not.toHaveBeenCalled();
  });

  it("삭제 중 요청한 explicit sync는 기본 브랜치 task를 되살리지 않는다", async () => {
    // Given
    vi.useFakeTimers();

    const project = {
      id: "project-1",
      name: "api",
      repoPath: "/workspace/api",
      defaultBranch: "main",
      sshHost: null,
    };
    let releaseTransaction!: () => void;
    const transactionStarted = new Promise<void>((resolve) => {
      const manager = {
        findOneBy: vi.fn().mockResolvedValue(project),
        delete: vi.fn().mockResolvedValue({ affected: 1 }),
        remove: vi.fn(async () => {
          resolve();
          await new Promise<void>((release) => {
            releaseTransaction = release;
          });
          return project;
        }),
      };

      mocks.getProjectRepository.mockResolvedValue({
        find: vi.fn().mockResolvedValue([project]),
        manager: {
          transaction: vi.fn(async (callback: (transactionManager: typeof manager) => Promise<void>) => callback(manager)),
        },
      });
    });
    const findOneBy = vi.fn().mockResolvedValue(null);
    const save = vi.fn(async (value) => ({ id: "task-main", ...value }));
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy,
      create: vi.fn((value) => value),
      save,
    });

    const { getAllProjects, deleteProject, syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    try {
      // When
      await getAllProjects();
      const deletePromise = deleteProject(project.id);
      await transactionStarted;
      await syncRegisteredProjectWorktrees([project] as never);
      await vi.runAllTimersAsync();
      releaseTransaction();
      const result = await deletePromise;

      // Then
      expect(result).toBe(true);
      expect(save).not.toHaveBeenCalled();
      expect(findOneBy).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
});

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
      "find '/workspace' -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort",
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
      "find '/home/tester/' -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort",
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
      "find \"$HOME\"'/projects' -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort",
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

  it("다른 PC에 같은 이름의 프로젝트가 있으면 상위 폴더를 붙이지 않고 그대로 등록한다", async () => {
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
      name: "kanvibe",
      sshHost: "remote-host",
    });
  });

  it("같은 PC에 같은 이름의 프로젝트가 있으면 상위 폴더를 붙여 구분한다", async () => {
    mocks.validateGitRepo.mockResolvedValue(true);
    mocks.getDefaultBranch.mockResolvedValue("main");

    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([
        {
          id: "project-existing",
          name: "kanvibe",
          repoPath: "/workspace/kanvibe",
          sshHost: "remote-host",
        },
      ]),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "project-new", ...value })),
      remove: vi.fn(),
    });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue(null),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "task-1", ...value })),
    });

    const { registerProject } = await import("@/desktop/main/services/projectService");
    const result = await registerProject("kanvibe", "/home/tester/Documents/kanvibe", "remote-host");

    expect(result.success).toBe(true);
    expect(result.project).toMatchObject({
      name: "Documents/kanvibe",
      sshHost: "remote-host",
    });
  });

  it("같은 PC에 이름과 상위 폴더가 모두 같은 프로젝트가 있으면 등록하지 않는다", async () => {
    mocks.validateGitRepo.mockResolvedValue(true);
    mocks.getDefaultBranch.mockResolvedValue("main");

    const save = vi.fn();
    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([
        {
          id: "project-existing",
          name: "kanvibe",
          repoPath: "/home/tester/a/work/kanvibe",
          sshHost: null,
        },
        {
          id: "project-existing-parent",
          name: "work/kanvibe",
          repoPath: "/home/tester/b/work/kanvibe",
          sshHost: null,
        },
      ]),
      create: vi.fn((value) => value),
      save,
      remove: vi.fn(),
    });

    const { registerProject } = await import("@/desktop/main/services/projectService");
    const result = await registerProject("kanvibe", "/home/tester/c/work/kanvibe");

    expect(result).toEqual({
      success: false,
      error: "같은 PC에 이름과 상위 폴더가 모두 같은 프로젝트가 이미 있습니다.",
    });
    expect(save).not.toHaveBeenCalled();
  });

  it("스캔 중 이름과 상위 폴더가 모두 겹치는 저장소는 숫자를 붙이지 않고 오류로 남긴다", async () => {
    mocks.scanGitRepos.mockResolvedValue(["/home/tester/c/work/kanvibe"]);
    mocks.getDefaultBranch.mockResolvedValue("main");

    const save = vi.fn(async (value) => ({ id: "project-new", ...value }));
    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([
        {
          id: "project-existing",
          name: "kanvibe",
          repoPath: "/home/tester/a/work/kanvibe",
          sshHost: null,
        },
        {
          id: "project-existing-parent",
          name: "work/kanvibe",
          repoPath: "/home/tester/b/work/kanvibe",
          sshHost: null,
        },
      ]),
      create: vi.fn((value) => value),
      save,
      remove: vi.fn(),
    });

    const { scanAndRegisterProjects } = await import("@/desktop/main/services/projectService");
    const result = await scanAndRegisterProjects("/home/tester");

    expect(result.registered).toEqual([]);
    expect(result.errors).toEqual([
      "/home/tester/c/work/kanvibe: 같은 PC에 이름과 상위 폴더가 모두 같은 프로젝트가 이미 있습니다.",
    ]);
    expect(save).not.toHaveBeenCalled();
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

  it("프로젝트 등록은 GitHub 아이콘 조회를 기다리지 않고 저장을 마친 뒤 백그라운드로 아이콘을 채운다", async () => {
    // Given
    mocks.validateGitRepo.mockResolvedValue(true);
    mocks.getDefaultBranch.mockResolvedValue("main");
    mocks.createSessionWithoutWorktree.mockResolvedValue({ sessionName: "api-main" });

    let releaseIconGate: (() => void) | undefined;
    const iconGate = new Promise<void>((resolve) => {
      releaseIconGate = resolve;
    });
    mocks.resolveProjectIconDataUrl.mockImplementation(async () => {
      await iconGate;
      return "data:image/png;base64,icon";
    });

    const projectSave = vi.fn(async (value) => ({ id: "project-1", ...value }));
    const projectUpdate = vi.fn(async () => ({ affected: 1 }));
    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([]),
      create: vi.fn((value) => value),
      save: projectSave,
      update: projectUpdate,
      remove: vi.fn(),
    });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue(null),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "task-1", ...value })),
    });

    try {
      const { registerProject } = await import("@/desktop/main/services/projectService");

      // When
      const result = await registerProject("api", "/workspace/api");

      // Then
      /** 아이콘 조회가 아직 끝나지 않아도 등록과 DB 저장은 완료된다 */
      expect(result.success).toBe(true);
      expect(projectSave).toHaveBeenCalledTimes(1);
      expect(projectSave.mock.calls[0][0].iconDataUrl).toBeUndefined();

      releaseIconGate?.();
      await new Promise((resolve) => setTimeout(resolve, 0));

      /** 아이콘은 이미 존재하는 행에만 반영해 조회 중 삭제된 프로젝트가 되살아나지 않게 한다 */
      expect(projectSave).toHaveBeenCalledTimes(1);
      expect(projectUpdate).toHaveBeenCalledWith(
        { id: "project-1" },
        { iconDataUrl: "data:image/png;base64,icon" },
      );
    } finally {
      mocks.resolveProjectIconDataUrl.mockImplementation(async () => null);
    }
  });

  it("아이콘 조회 도중 프로젝트가 삭제되면 아이콘을 다시 저장하지 않는다", async () => {
    // Given
    mocks.validateGitRepo.mockResolvedValue(true);
    mocks.getDefaultBranch.mockResolvedValue("main");
    mocks.createSessionWithoutWorktree.mockResolvedValue({ sessionName: "api-main" });
    mocks.resolveProjectIconDataUrl.mockImplementation(async () => "data:image/png;base64,icon");

    /** 삭제된 프로젝트라 조건부 update가 어떤 행도 건드리지 못한 상황 */
    const projectUpdate = vi.fn(async () => ({ affected: 0 }));
    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([]),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "project-1", ...value })),
      update: projectUpdate,
      remove: vi.fn(),
    });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue(null),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "task-1", ...value })),
    });

    try {
      const { registerProject } = await import("@/desktop/main/services/projectService");

      // When
      await registerProject("api", "/workspace/api");
      await new Promise((resolve) => setTimeout(resolve, 0));

      // Then
      expect(projectUpdate).toHaveBeenCalledTimes(1);
      /** 등록 직후 1회만 갱신되고, 반영되지 못한 아이콘으로 보드를 다시 갱신하지 않는다 */
      expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
    } finally {
      mocks.resolveProjectIconDataUrl.mockImplementation(async () => null);
    }
  });
});

describe("projectService local hook installation", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    /** 파일 읽기 구현은 테스트마다 다르므로 이전 테스트의 구현이 남지 않도록 되돌린다 */
    mocks.readTextFile.mockReset();
    mocks.execGit.mockResolvedValue("");
    mocks.getDefaultSessionType.mockResolvedValue("tmux");
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: false });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: false });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: false });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: false });
  });

  it("로컬 Claude hook 설치 시 Claude provider만 다시 설치한다", async () => {
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

    expect(mocks.installKanvibeHookProvider).toHaveBeenCalledWith(
      "/workspace/task-1",
      "task-1",
      "claude",
      null,
    );
    expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
    expect(mocks.setupClaudeHooks).not.toHaveBeenCalled();
  });

  it("로컬 OpenCode hook 설치 시 OpenCode provider만 다시 설치한다", async () => {
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

    expect(mocks.installKanvibeHookProvider).toHaveBeenCalledWith(
      "/workspace/repo",
      "task-main",
      "openCode",
      null,
    );
    expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
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

  it("프로젝트 루트 경로 task에서 Claude hook 재설치를 누르면 Claude provider만 현재 root task에 다시 바인딩한다", async () => {
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

    expect(mocks.installKanvibeHookProvider).toHaveBeenCalledWith(
      "/workspace/repo",
      "task-main",
      "claude",
      null,
    );
    expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
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
    expect(mocks.ensureKanvibeHooksCurrent).toHaveBeenCalledWith(
      "/workspace/api",
      "task-1",
      null,
    );
  });

  it("레거시 프로젝트는 목록 조회에서 복구하지 않고 explicit sync에서 복구한다", async () => {
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

    const { getAllProjects, syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

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

      expect(mocks.broadcastBoardUpdate).not.toHaveBeenCalled();
      expect(findOneBy).not.toHaveBeenCalled();
      mocks.listWorktrees.mockResolvedValue([{ path: "/workspace/api", branch: "main", isBare: false }]);
      const result = await syncRegisteredProjectWorktrees();
      expect(result.changed).toBe(true);
      expect(result.errors).toEqual([]);
      expect(mocks.installKanvibeHooks).not.toHaveBeenCalledWith(
        "/workspace/api",
        "task-main",
        null,
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it("root task explicit sync가 한 번 실패해도 다음 sync에서 다시 재시도한다", async () => {
    vi.useFakeTimers();

    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const taskSave = vi.fn()
      .mockRejectedValueOnce(new Error("database offline"))
      .mockImplementation(async (value) => ({ id: "task-main", ...value }));

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
      findOneBy: vi.fn().mockResolvedValue(null),
      create: vi.fn((value) => value),
      save: taskSave,
    });
    mocks.listWorktrees.mockResolvedValue([
      {
        path: "/workspace/api",
        branch: "main",
        isBare: false,
      },
    ]);
    mocks.createSessionWithoutWorktree.mockResolvedValue({ sessionName: "api-main" });

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    try {
      const failed = await syncRegisteredProjectWorktrees();
      expect(failed.errors.join(" ")).toContain("database offline");

      expect(taskSave).toHaveBeenCalledTimes(1);
      expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
      expect(mocks.broadcastBoardUpdate).not.toHaveBeenCalled();

      const retried = await syncRegisteredProjectWorktrees();
      expect(retried.changed).toBe(true);
      expect(retried.errors).toEqual([]);

      expect(taskSave).toHaveBeenCalledTimes(2);
      expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
      expect(mocks.broadcastBoardUpdate).not.toHaveBeenCalled();
    } finally {
      consoleErrorSpy.mockRestore();
      vi.useRealTimers();
    }
  });

  it("원격 root hook 상태 조회는 getAllProjects 백그라운드에서 발생하지 않는다", async () => {
    vi.useFakeTimers();

    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});

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
    const { getAllProjects } = await import("@/desktop/main/services/projectService");

    try {
      await getAllProjects();
      await vi.runAllTimersAsync();

      expect(mocks.getClaudeHooksStatus).not.toHaveBeenCalled();
      expect(mocks.getGeminiHooksStatus).not.toHaveBeenCalled();
      expect(mocks.getCodexHooksStatus).not.toHaveBeenCalled();
      expect(mocks.getOpenCodeHooksStatus).not.toHaveBeenCalled();
      expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
      expect(consoleErrorSpy).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("원격 root hook 설치 오류는 getAllProjects 백그라운드에서 발생하지 않는다", async () => {
    vi.useFakeTimers();

    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});

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

    expect(mocks.installKanvibeHookProvider).toHaveBeenCalledWith(
      "/workspace/api",
      "task-main",
      "claude",
      null,
    );
    expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
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

  it("프로젝트 스캔은 git submodule을 별도 프로젝트로 등록하지 않는다", async () => {
    // Given
    mocks.scanGitRepos.mockResolvedValue([
      "/workspace/app",
      "/workspace/app/packages/ui",
    ]);
    mocks.execGit.mockImplementation(async (command: string) => {
      if (command.includes("rev-parse --show-superproject-working-tree")) {
        return command.includes('"/workspace/app/packages/ui"') ? "/workspace/app" : "";
      }

      if (command.includes("rev-parse --path-format=absolute --git-common-dir")) {
        return command.includes('"/workspace/app/packages/ui"')
          ? "/workspace/app/.git/modules/packages/ui"
          : "/workspace/app/.git";
      }

      return "";
    });
    mocks.getDefaultBranch.mockResolvedValue("main");
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: true });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: true });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });
    mocks.listWorktrees.mockResolvedValue([]);

    const projectSave = vi.fn(async (value) => ({ id: "project-1", ...value }));
    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn()
        .mockResolvedValueOnce([])
        .mockResolvedValueOnce([
          {
            id: "project-1",
            name: "app",
            repoPath: "/workspace/app",
            defaultBranch: "main",
            sshHost: null,
          },
        ]),
      create: vi.fn((value) => value),
      save: projectSave,
      remove: vi.fn(),
    });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue(null),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => ({ id: "task-main", ...value })),
    });

    const { scanAndRegisterProjects } = await import("@/desktop/main/services/projectService");

    // When
    const result = await scanAndRegisterProjects("/workspace");

    // Then
    expect(result.registered).toEqual(["app"]);
    expect(projectSave).toHaveBeenCalledTimes(1);
    expect(projectSave).toHaveBeenCalledWith(expect.objectContaining({
      name: "app",
      repoPath: "/workspace/app",
    }));
    expect(mocks.getDefaultBranch).toHaveBeenCalledWith("/workspace/app", null);
    expect(mocks.getDefaultBranch).not.toHaveBeenCalledWith("/workspace/app/packages/ui", null);
  });

  it.each([false, true])("worktree 경로만 스캔돼도 원격 호스트를 보존한다 (tmux=%s)", async (hasSession) => {
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
    mocks.isSessionAlive.mockResolvedValue(hasSession);

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
    const [savedWorktreeTask] = taskRepoSave.mock.calls[0];
    expect(savedWorktreeTask.branchName).toBe("feature-login");
    expect(savedWorktreeTask.sshHost).toBe("remote-host");
    expect(savedWorktreeTask.sessionType).toBe(hasSession ? "tmux" : undefined);
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

  it("tmux 세션이 없고 Herdr 세션이 살아 있으면 Herdr 세션으로 연결한다", async () => {
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
    mocks.isSessionAlive.mockImplementation(async (sessionType: string, name: string) => sessionType === "herdr" && !name.startsWith("kanvibe:"));

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
    const [savedWorktreeTask] = taskRepoSave.mock.calls[0];
    expect(savedWorktreeTask.branchName).toBe("feature-login");
    expect(savedWorktreeTask.sshHost).toBe("remote-host");
    expect(savedWorktreeTask.sessionType).toBe("herdr");
    expect(savedWorktreeTask.sessionName).toBe("herdr-repo-feature-login");
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

  it("등록된 프로젝트 background sync는 기존 root hooks를 자동 복구하지 않는다", async () => {
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: false });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: false });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: false });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: false });
    mocks.listWorktrees.mockResolvedValue([
      {
        path: "/workspace/api",
        branch: "main",
        isBare: false,
      },
    ]);

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
      findOneBy: vi.fn(async (criteria: { branchName?: string; projectId?: string | null }) => {
        if (criteria.projectId === "project-1" && criteria.branchName === "main") {
          return {
            id: "task-main",
            branchName: "main",
            projectId: "project-1",
            baseBranch: "main",
            worktreePath: "/workspace/api",
            sshHost: null,
          };
        }

        return null;
      }),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => value),
    });

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    const result = await syncRegisteredProjectWorktrees();

    expect(result.changed).toBe(false);
    expect(mocks.getClaudeHooksStatus).not.toHaveBeenCalled();
    expect(mocks.getGeminiHooksStatus).not.toHaveBeenCalled();
    expect(mocks.getCodexHooksStatus).not.toHaveBeenCalled();
    expect(mocks.getOpenCodeHooksStatus).not.toHaveBeenCalled();
    expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
  });

  it("등록된 프로젝트 경로가 git 저장소가 아니면 상태 파일을 쓰지 않고 동기화를 건너뛴다", async () => {
    // Given
    const consoleWarnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    mocks.execGit.mockImplementation(async (command: string) => (
      command.includes("rev-parse --is-inside-work-tree") ? "__KANVIBE_NOT_GIT_REPOSITORY__" : ""
    ));
    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([
        {
          id: "project-removed",
          name: "removed-workspace",
          repoPath: "/remote/removed-workspace",
          defaultBranch: "main",
          sshHost: "remote-mac",
        },
      ]),
    });
    const saveTask = vi.fn(async (value) => value);
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn(async () => null),
      create: vi.fn((value) => value),
      save: saveTask,
    });

    try {
      const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

      // When
      const result = await syncRegisteredProjectWorktrees();

      // Then
      expect(result.errors).toEqual([]);
      expect(result.changed).toBe(false);
      expect(mocks.listWorktrees).not.toHaveBeenCalled();
      expect(saveTask).not.toHaveBeenCalled();
      expect(mocks.writeTextFile).not.toHaveBeenCalled();
      expect(mocks.writeTextFileIfAbsent).not.toHaveBeenCalled();
      expect(consoleWarnSpy).toHaveBeenCalledWith(
        "removed-workspace 경로가 git 저장소가 아니어서 동기화를 건너뜁니다:",
        { repoPath: "/remote/removed-workspace", sshHost: "remote-mac" },
      );
    } finally {
      consoleWarnSpy.mockRestore();
    }
  });

  it("git 저장소 판정이 SSH 연결 실패로 끝나면 저장소가 아니라고 보지 않고 기존 동기화를 진행한다", async () => {
    // Given
    mocks.execGit.mockImplementation(async (command: string) => {
      if (command.includes("rev-parse --is-inside-work-tree")) {
        throw new Error("remote-mac 원격 명령 실패: Connection reset by 100.99.109.53 port 22");
      }
      return "";
    });
    mocks.listWorktrees.mockResolvedValue([]);
    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([
        {
          id: "project-1",
          name: "api",
          repoPath: "/remote/api",
          defaultBranch: "main",
          sshHost: "remote-mac",
        },
      ]),
    });
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn(async () => ({
        id: "task-main",
        branchName: "main",
        projectId: "project-1",
        baseBranch: "main",
        worktreePath: "/remote/api",
        sshHost: "remote-mac",
      })),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => value),
    });

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    // When
    await syncRegisteredProjectWorktrees();

    // Then
    expect(mocks.listWorktrees).toHaveBeenCalledWith("/remote/api", "remote-mac");
  });

  /**
   * 공유 색상 파일을 읽는 동안 사용자가 색을 바꿀 수 있으므로, sync가 읽어둔 색상이 아직 DB에
   * 남아 있을 때만 덮어써야 방금 고른 색이 되돌아가지 않는다.
   */
  function mockProjectColorSyncRepositories(
    project: { id: string; color: string },
    update: ReturnType<typeof vi.fn>,
  ): void {
    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([project]),
      findOneBy: vi.fn().mockResolvedValue(project),
      update,
    });

    mocks.getTaskRepository.mockResolvedValue({
      findBy: vi.fn().mockResolvedValue([]),
      findOneBy: vi.fn(async (criteria: { branchName?: string; projectId?: string | null }) => (
        criteria.projectId === project.id && criteria.branchName === "main"
          ? {
            id: "task-main",
            branchName: "main",
            projectId: project.id,
            baseBranch: "main",
            worktreePath: "/workspace/api",
            sshHost: null,
          }
          : null
      )),
      create: vi.fn((value) => value),
      save: vi.fn(async (value) => value),
    });
  }

  function mockSharedProjectColorFile(sharedColor: string): void {
    mocks.listWorktrees.mockResolvedValue([
      { path: "/workspace/api", branch: "main", isBare: false },
    ]);
    mocks.readTextFile.mockImplementation(async (filePath: string) => (
      filePath === "/workspace/api/.kanvibe/project.json"
        ? JSON.stringify({ schemaVersion: 1, projectColor: sharedColor })
        : ""
    ));
  }

  it("공유 색상을 DB에 반영하는 사이 사용자가 색을 바꾸면 그 값을 덮지 않는다", async () => {
    // Given
    mockSharedProjectColorFile("#0064FF");
    const project = {
      id: "project-1",
      name: "api",
      repoPath: "/workspace/api",
      defaultBranch: "main",
      sshHost: null,
      color: "#65D08A",
    };
    /** affected 0은 sync가 읽어둔 색상이 더 이상 DB에 없다는 뜻, 즉 사용자 편집이 먼저 반영된 상태다 */
    const update = vi.fn().mockResolvedValue({ affected: 0 });
    mockProjectColorSyncRepositories(project, update);

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    // When
    const result = await syncRegisteredProjectWorktrees();

    // Then
    expect(update).toHaveBeenCalledWith({ id: "project-1", color: "#65D08A" }, { color: "#0064FF" });
    expect(result.changed).toBe(false);
    expect(project.color).toBe("#65D08A");
  });

  it("공유 색상이 그대로 남아 있으면 DB와 보드에 반영한다", async () => {
    // Given
    mockSharedProjectColorFile("#0064FF");
    const project = {
      id: "project-1",
      name: "api",
      repoPath: "/workspace/api",
      defaultBranch: "main",
      sshHost: null,
      color: "#65D08A",
    };
    const update = vi.fn().mockResolvedValue({ affected: 1 });
    mockProjectColorSyncRepositories(project, update);

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    // When
    const result = await syncRegisteredProjectWorktrees();

    // Then
    expect(result.changed).toBe(true);
    expect(project.color).toBe("#0064FF");
  });

  it("등록된 원격 프로젝트 background sync는 기존 root hook 복구를 예약하지 않는다", async () => {
    vi.useFakeTimers();

    try {
      mocks.getClaudeHooksStatus.mockResolvedValue({ installed: false });
      mocks.getGeminiHooksStatus.mockResolvedValue({ installed: false });
      mocks.getCodexHooksStatus.mockResolvedValue({ installed: false });
      mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: false });
      mocks.listWorktrees.mockResolvedValue([
        {
          path: "/remote/api",
          branch: "main",
          isBare: false,
        },
      ]);

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
        save: vi.fn(async (value) => value),
      });

      const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

      const result = await syncRegisteredProjectWorktrees();
      await vi.runAllTimersAsync();

      expect(result.changed).toBe(false);
      expect(mocks.getClaudeHooksStatus).not.toHaveBeenCalled();
      expect(mocks.getGeminiHooksStatus).not.toHaveBeenCalled();
      expect(mocks.getCodexHooksStatus).not.toHaveBeenCalled();
      expect(mocks.getOpenCodeHooksStatus).not.toHaveBeenCalled();
      expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
    } finally {
      vi.clearAllTimers();
      vi.useRealTimers();
    }
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

  it("등록된 프로젝트 background sync는 활성 세션이 있어도 .kanvibe 상태가 없으면 새 worktree를 TODO task로 등록한다", async () => {
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: true });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: true });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });
    mocks.listWorktrees.mockResolvedValue([
      {
        path: "/workspace/api__worktrees/feature-active",
        branch: "feature-active",
        isBare: false,
      },
    ]);
    mocks.formatSessionName.mockReturnValue("api-feature-active");
    mocks.isSessionAlive.mockResolvedValue(true);
    mocks.readTextFile.mockResolvedValue("");

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
            worktreePath: "/workspace/api",
            sshHost: null,
          };
        }

        return null;
      }),
      create: vi.fn((value) => value),
      save: taskSave,
    });

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    await syncRegisteredProjectWorktrees();

    expect(taskSave).toHaveBeenCalledWith(expect.objectContaining({
      branchName: "feature-active",
      status: "todo",
      sessionName: "api-feature-active",
    }));
  });

  /** 지정한 저장소 경로의 status.json에만 상태가 기록된 공유 파일 배치 읽기를 준비한다 */
  function mockSharedTaskStatusFiles(statusByRepoPath: Record<string, string>): void {
    mocks.readTextFiles.mockImplementation(async (targetPaths: string[]) => new Map(
      targetPaths.map((targetPath) => {
        const status = statusByRepoPath[targetPath.replace("/.kanvibe/status.json", "")];
        return [
          targetPath,
          targetPath.endsWith("/.kanvibe/status.json") && status
            ? { exists: true, content: JSON.stringify({ schemaVersion: 1, status }) }
            : { exists: false, content: "" },
        ];
      }),
    ));
  }

  it.each([
    { baseBranch: "feature/parent", persistedStatus: null, expectedBase: "feature/parent" },
    { baseBranch: "feature/parent", persistedStatus: "review", expectedBase: "feature/parent" },
    { baseBranch: null, persistedStatus: null, expectedBase: "main" },
    { baseBranch: "", persistedStatus: null, expectedBase: "main" },
  ])("worktree sync는 기존 기준 브랜치를 보존하고 누락된 값만 보충한다: $baseBranch / $persistedStatus", async ({ baseBranch, persistedStatus, expectedBase }) => {
    const worktreePath = "/workspace/api__worktrees/feature-child";
    const rootTask = {
      id: "task-main", branchName: "main", projectId: "project-1", baseBranch: "main",
      worktreePath: "/workspace/api", sshHost: null, status: "todo", sessionType: "tmux", sessionName: "api-main",
    };
    const childTask = {
      id: "task-child", branchName: "feature-child", projectId: "project-1", baseBranch,
      worktreePath, sshHost: null, status: "todo",
    };
    mocks.listWorktrees.mockResolvedValue([
      { path: "/workspace/api", branch: "main", isBare: false },
      { path: worktreePath, branch: "feature-child", isBare: false },
    ]);
    mockSharedTaskStatusFiles(persistedStatus ? { [worktreePath]: persistedStatus } : {});
    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn().mockResolvedValue([{
        id: "project-1", name: "api", repoPath: "/workspace/api", defaultBranch: "main",
        sshHost: null, iconDataUrl: "data:existing",
      }]),
    });
    const taskSave = vi.fn(async (value) => ({ ...value }));
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn(async (criteria: { branchName?: string }) => (
        criteria.branchName === "main" ? rootTask : childTask
      )),
      save: taskSave,
    });
    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    const result = await syncRegisteredProjectWorktrees();

    expect(result.errors).toEqual([]);
    expect(childTask.baseBranch).toBe(expectedBase);
    const childSaves = taskSave.mock.calls.map(([task]) => task).filter((task) => task.id === "task-child");
    if (persistedStatus || !baseBranch) {
      expect(childSaves).toEqual([expect.objectContaining({
        baseBranch: expectedBase, status: persistedStatus ?? "todo",
      })]);
    } else {
      expect(childSaves).toEqual([]);
    }
  });

  it("등록된 프로젝트 background sync는 .kanvibe task 상태가 있으면 그 상태로 새 worktree를 등록한다", async () => {
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: true });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: true });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });
    mocks.listWorktrees.mockResolvedValue([
      {
        path: "/workspace/api__worktrees/feature-review",
        branch: "feature-review",
        isBare: false,
      },
    ]);
    mocks.formatSessionName.mockReturnValue("api-feature-review");
    mocks.isSessionAlive.mockResolvedValue(false);
    mockSharedTaskStatusFiles({ "/workspace/api__worktrees/feature-review": "review" });

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
            worktreePath: "/workspace/api",
            sshHost: null,
          };
        }

        return null;
      }),
      create: vi.fn((value) => value),
      save: taskSave,
    });

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    await syncRegisteredProjectWorktrees();

    expect(mocks.readTextFiles).toHaveBeenCalledWith(
      [
        "/workspace/api__worktrees/feature-review/.kanvibe/status.json",
        "/workspace/api__worktrees/feature-review/.kanvibe/task.json",
        "/workspace/api/.kanvibe/project.json",
      ],
      null,
    );
    expect(taskSave).toHaveBeenCalledWith(expect.objectContaining({
      branchName: "feature-review",
      status: "review",
    }));
  });

  it("등록된 프로젝트 background sync는 기존 worktree task도 .kanvibe task 상태대로 갱신한다", async () => {
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: true });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: true });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });
    mocks.listWorktrees.mockResolvedValue([
      {
        path: "/workspace/api__worktrees/feature-pending",
        branch: "feature-pending",
        isBare: false,
      },
    ]);
    mockSharedTaskStatusFiles({ "/workspace/api__worktrees/feature-pending": "pending" });

    const existingTask = {
      id: "task-worktree",
      branchName: "feature-pending",
      projectId: "project-1",
      baseBranch: "main",
      worktreePath: "/workspace/api__worktrees/feature-pending",
      sshHost: null,
      status: "todo",
    };

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

    const taskSave = vi.fn(async (value) => value);
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn(async (criteria: { branchName?: string; projectId?: string | null }) => {
        if (criteria.projectId === "project-1" && criteria.branchName === "main") {
          return {
            id: "task-main",
            branchName: "main",
            projectId: "project-1",
            baseBranch: "main",
            worktreePath: "/workspace/api",
            sshHost: null,
          };
        }

        if (criteria.projectId === "project-1" && criteria.branchName === "feature-pending") {
          return existingTask;
        }

        return null;
      }),
      create: vi.fn((value) => value),
      save: taskSave,
    });

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    await syncRegisteredProjectWorktrees();

    expect(taskSave).toHaveBeenCalledWith(expect.objectContaining({
      id: "task-worktree",
      status: "pending",
    }));
  });

  it("프로젝트 등록은 기본 브랜치 worktree의 .kanvibe 상태를 tmux 연결 후에도 유지한다", async () => {
    mocks.validateGitRepo.mockResolvedValue(true);
    mocks.getDefaultBranch.mockResolvedValue("main");
    mocks.listWorktrees.mockResolvedValue([
      {
        path: "/workspace/api",
        branch: "main",
        isBare: false,
      },
    ]);
    mocks.readTextFile.mockResolvedValue(JSON.stringify({ schemaVersion: 1, status: "review", updatedAt: "2026-06-03T00:00:00.000Z" }));
    mocks.createSessionWithoutWorktree.mockResolvedValue({ sessionName: "api-main" });
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
    const taskSave = vi.fn(async (value) => ({ id: "task-main", ...value }));
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn().mockResolvedValue(null),
      create: vi.fn((value) => value),
      save: taskSave,
    });

    const { registerProject } = await import("@/desktop/main/services/projectService");

    await expect(registerProject("api", "/workspace/api")).resolves.toMatchObject({ success: true });

    expect(mocks.createSessionWithoutWorktree).toHaveBeenCalledWith(
      "/workspace/api",
      "main",
      "tmux",
      null,
      "/workspace/api",
    );
    expect(taskSave).toHaveBeenCalledWith(expect.objectContaining({
      branchName: "main",
      sessionName: "api-main",
      status: "review",
    }));
  });

  it("등록된 프로젝트 background sync는 orphan worktree task도 .kanvibe 상태대로 연결한다", async () => {
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: true });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: true });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });
    mocks.listWorktrees.mockResolvedValue([
      {
        path: "/workspace/api__worktrees/feature-orphan",
        branch: "feature-orphan",
        isBare: false,
      },
    ]);
    mockSharedTaskStatusFiles({ "/workspace/api__worktrees/feature-orphan": "done" });
    mocks.formatSessionName.mockReturnValue("api-feature-orphan");
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

    const orphanTask = {
      id: "orphan-task",
      branchName: "feature-orphan",
      projectId: null,
      worktreePath: "/workspace/api__worktrees/feature-orphan",
      sshHost: null,
      status: "todo",
    };
    const taskSave = vi.fn(async (value) => value);
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn(async (criteria: { branchName?: string; projectId?: string | null }) => {
        if (criteria.projectId === "project-1" && criteria.branchName === "main") {
          return {
            id: "task-main",
            branchName: "main",
            projectId: "project-1",
            baseBranch: "main",
            worktreePath: "/workspace/api",
            sshHost: null,
          };
        }

        if (criteria.projectId === "project-1" && criteria.branchName === "feature-orphan") {
          return null;
        }

        if ((criteria.projectId === null || criteria.projectId === undefined) && criteria.branchName === "feature-orphan") {
          return orphanTask;
        }

        return null;
      }),
      create: vi.fn((value) => value),
      save: taskSave,
    });

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    const result = await syncRegisteredProjectWorktrees();

    expect(result.worktreeTasks).toContain("feature-orphan");
    expect(taskSave).toHaveBeenCalledWith(expect.objectContaining({
      id: "orphan-task",
      projectId: "project-1",
      baseBranch: "main",
      status: "done",
    }));
  });

  it("등록된 프로젝트 background sync는 메인 브랜치 세션 연결 시 .kanvibe 상태를 보존한다", async () => {
    mocks.listWorktrees.mockResolvedValue([]);
    mocks.readTextFile.mockResolvedValue(JSON.stringify({ schemaVersion: 1, status: "progress" }));
    mocks.formatSessionName.mockReturnValue("api-main");
    mocks.isSessionAlive.mockResolvedValue(true);

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

    const mainBranchTask = {
      id: "task-main",
      branchName: "main",
      projectId: "project-1",
      baseBranch: "main",
      worktreePath: null,
      sshHost: null,
      status: "todo",
    };
    const taskSave = vi.fn(async (value) => value);
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn(async (criteria: { branchName?: string; projectId?: string | null }) => {
        if (criteria.projectId === "project-1" && criteria.branchName === "main") {
          return mainBranchTask;
        }

        return null;
      }),
      create: vi.fn((value) => value),
      save: taskSave,
    });

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    await syncRegisteredProjectWorktrees();

    expect(taskSave).toHaveBeenCalledWith(expect.objectContaining({
      id: "task-main",
      sessionType: "tmux",
      sessionName: "api-main",
      status: "progress",
    }));
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

  it("등록된 프로젝트 background sync는 아이콘 backfill을 순차 worktree 스캔과 분리해 병렬로 실행한다", async () => {
    // Given
    mocks.isSessionAlive.mockResolvedValue(false);
    mocks.listWorktrees.mockResolvedValue([]);

    const iconLookupPaths: string[] = [];
    let releaseIconGate: (() => void) | undefined;
    const iconGate = new Promise<void>((resolve) => {
      releaseIconGate = resolve;
    });
    mocks.resolveProjectIconDataUrl.mockImplementation(async (repoPath: string) => {
      iconLookupPaths.push(repoPath);
      await iconGate;
      return `data:image/png;base64,${repoPath}`;
    });

    const projectUpdate = vi.fn(async () => ({ affected: 1 }));
    mocks.getProjectRepository.mockResolvedValue({ save: vi.fn(async (value) => value), update: projectUpdate });
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
      { id: "project-a", name: "api-a", repoPath: "/workspace/api-a", defaultBranch: "main", sshHost: null, iconDataUrl: null },
      { id: "project-b", name: "api-b", repoPath: "/workspace/api-b", defaultBranch: "main", sshHost: null, iconDataUrl: null },
      { id: "project-c", name: "api-c", repoPath: "/workspace/api-c", defaultBranch: "main", sshHost: null, iconDataUrl: "data:image/png;base64,kept" },
    ];

    try {
      const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

      // When
      const syncPromise = syncRegisteredProjectWorktrees(projects as never);
      await new Promise((resolve) => setTimeout(resolve, 0));

      // Then
      /** 아이콘이 없는 두 프로젝트의 조회가 서로를 기다리지 않고 동시에 진행된다 */
      expect(iconLookupPaths).toEqual(["/workspace/api-a", "/workspace/api-b"]);
      expect(mocks.listWorktrees).not.toHaveBeenCalled();

      releaseIconGate?.();
      const result = await syncPromise;

      expect(result.changed).toBe(true);
      expect(projects[2].iconDataUrl).toBe("data:image/png;base64,kept");
      /** 아이콘은 이미 존재하는 행에만 조건부로 반영한다 */
      expect(projectUpdate.mock.calls).toEqual([
        [{ id: "project-a" }, { iconDataUrl: "data:image/png;base64,/workspace/api-a" }],
        [{ id: "project-b" }, { iconDataUrl: "data:image/png;base64,/workspace/api-b" }],
      ]);
    } finally {
      mocks.resolveProjectIconDataUrl.mockImplementation(async () => null);
    }
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

  it("상세 훅 조회 실패는 성공 상태로 감추지 않는다", async () => {
    const task = { id: "task-remote", worktreePath: "/remote/worktree", project: { repoPath: "/remote/repo", sshHost: "remote-host" } };
    mocks.getTaskRepository.mockResolvedValue({ findOne: vi.fn(async () => task) });
    mocks.getClaudeHooksStatus.mockRejectedValueOnce(new Error("inspection unavailable"));
    const { getTaskHooksStatus } = await import("@/desktop/main/services/projectService");
    await expect(getTaskHooksStatus(task.id)).rejects.toThrow("inspection unavailable");
    expect(mocks.getClaudeHooksStatus).toHaveBeenCalledTimes(1);
    expect(mocks.ensureKanvibeHooksCurrent).not.toHaveBeenCalled();
  });

  it.each(["getProjectHooksStatus", "getProjectGeminiHooksStatus", "getProjectCodexHooksStatus"] as const)("%s는 기존 root task 조회만 하고 복구하지 않는다", async (method) => {
    const project = { id: "project-root", repoPath: "/remote/repo", defaultBranch: "main", sshHost: "remote-host" };
    const task = { id: "task-root", projectId: project.id, branchName: "main", worktreePath: project.repoPath, sshHost: project.sshHost };
    const save = vi.fn(async (value) => value);
    mocks.getProjectRepository.mockResolvedValue({ findOneBy: vi.fn(async () => project) });
    mocks.getTaskRepository.mockResolvedValue({ findOneBy: vi.fn(async () => task), save });
    const service = await import("@/desktop/main/services/projectService");

    await expect(service[method](project.id)).resolves.toEqual({ installed: false });
    expect(save).not.toHaveBeenCalled();
    expect(mocks.writeTextFile).not.toHaveBeenCalled();
    expect(mocks.ensureKanvibeHooksCurrent).not.toHaveBeenCalled();
  });

  it("상태 조회는 네 provider가 미설치여도 hooks를 복구하지 않는다", async () => {
    const task = { id: "task-read-only", worktreePath: "/remote/worktree", project: { repoPath: "/remote/repo", sshHost: "remote-host" } };
    mocks.getTaskRepository.mockResolvedValue({ findOne: vi.fn(async () => task) });
    const service = await import("@/desktop/main/services/projectService");

    const statuses = await Promise.all([
      service.getTaskHooksStatus(task.id), service.getTaskGeminiHooksStatus(task.id),
      service.getTaskCodexHooksStatus(task.id), service.getTaskOpenCodeHooksStatus(task.id),
    ]);

    expect(statuses).toEqual(Array.from({ length: 4 }, () => ({ installed: false })));
    expect(mocks.ensureKanvibeHooksCurrent).not.toHaveBeenCalled();
    expect(mocks.installKanvibeHookProvider).not.toHaveBeenCalled();
  });

  it("일괄 hook 상태 조회는 task target을 한 번만 조회한다", async () => {
    const task = { id: "task-batch", worktreePath: "/remote/worktree", project: { repoPath: "/remote/repo", sshHost: "remote-host" } };
    const findOne = vi.fn(async () => task);
    mocks.getTaskRepository.mockResolvedValue({ findOne });
    const expected = { claude: { installed: false }, gemini: { installed: true }, codex: { installed: false }, openCode: { installed: true } };
    mocks.getKanvibeHooksStatuses.mockResolvedValue(expected);
    const { getTaskHooksStatuses } = await import("@/desktop/main/services/projectService");

    await expect(getTaskHooksStatuses(task.id)).resolves.toBe(expected);
    expect(findOne).toHaveBeenCalledTimes(1);
    expect(mocks.getKanvibeHooksStatuses).toHaveBeenCalledWith("/remote/worktree", "task-batch", "remote-host");
    expect(mocks.ensureKanvibeHooksCurrent).not.toHaveBeenCalled();
  });

  it("기본 브랜치 상태 조회는 기존 root task를 읽고 상태 파일을 쓰지 않는다", async () => {
    const task = { id: "task-root", branchName: "main", worktreePath: "/remote/repo", project: { id: "project-root", repoPath: "/remote/repo", defaultBranch: "main", sshHost: "remote-host" } };
    const save = vi.fn(async (value) => value);
    mocks.getTaskRepository.mockResolvedValue({ findOne: vi.fn(async () => task), findOneBy: vi.fn(async () => task), save });
    const { getTaskHooksStatus } = await import("@/desktop/main/services/projectService");

    await expect(getTaskHooksStatus(task.id)).resolves.toEqual({ installed: false });
    expect(save).not.toHaveBeenCalled();
    expect(mocks.writeTextFile).not.toHaveBeenCalled();
    expect(mocks.ensureKanvibeHooksCurrent).not.toHaveBeenCalled();
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

  it("원격 repo root에서 실행 중인 비기본 브랜치 task는 현재 taskId로 hook 상태를 조회한다", async () => {
    const task = {
      id: "task-remote",
      branchName: "feature/login",
      baseBranch: "main",
      worktreePath: "/remote/repo",
      project: {
        id: "project-1",
        repoPath: "/remote/repo",
        defaultBranch: "main",
        sshHost: "remote-host",
      },
    };
    const findOneBy = vi.fn();

    mocks.getTaskRepository.mockResolvedValue({
      findOne: vi.fn().mockResolvedValue(task),
      findOneBy,
    });
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: true });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: true });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });

    const {
      getTaskHooksStatus,
      getTaskGeminiHooksStatus,
      getTaskCodexHooksStatus,
      getTaskOpenCodeHooksStatus,
    } = await import("@/desktop/main/services/projectService");

    await getTaskHooksStatus(task.id);
    await getTaskGeminiHooksStatus(task.id);
    await getTaskCodexHooksStatus(task.id);
    await getTaskOpenCodeHooksStatus(task.id);

    expect(mocks.ensureKanvibeHooksCurrent).not.toHaveBeenCalled();
    expect(mocks.getClaudeHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-remote", "remote-host");
    expect(mocks.getGeminiHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-remote", "remote-host");
    expect(mocks.getCodexHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-remote", "remote-host");
    expect(mocks.getOpenCodeHooksStatus).toHaveBeenCalledWith("/remote/repo", "task-remote", "remote-host");
    expect(findOneBy).not.toHaveBeenCalled();
  });

  it("원격 태스크에서 hook 재설치를 누르면 해당 provider만 다시 실행한다", async () => {
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
    expect(mocks.installKanvibeHookProvider).toHaveBeenCalledWith(
      "/remote/worktree",
      "task-remote",
      "claude",
      "remote-host",
    );
    expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
  });

  it("원격 repo root에서 실행 중인 비기본 브랜치 task는 현재 taskId로 hook을 재설치한다", async () => {
    const task = {
      id: "task-remote",
      branchName: "feature/login",
      baseBranch: "main",
      worktreePath: "/remote/repo",
      project: {
        id: "project-1",
        repoPath: "/remote/repo",
        defaultBranch: "main",
        sshHost: "remote-host",
      },
    };

    mocks.getTaskRepository.mockResolvedValue({
      findOne: vi.fn().mockResolvedValue(task),
      findOneBy: vi.fn(),
    });
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: true });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: true });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });

    const {
      installTaskHooks,
      installTaskGeminiHooks,
      installTaskCodexHooks,
      installTaskOpenCodeHooks,
    } = await import("@/desktop/main/services/projectService");

    await installTaskHooks(task.id);
    await installTaskGeminiHooks(task.id);
    await installTaskCodexHooks(task.id);
    await installTaskOpenCodeHooks(task.id);

    expect(mocks.installKanvibeHookProvider).toHaveBeenCalledTimes(4);
    expect(mocks.installKanvibeHookProvider).toHaveBeenNthCalledWith(1, "/remote/repo", "task-remote", "claude", "remote-host");
    expect(mocks.installKanvibeHookProvider).toHaveBeenNthCalledWith(2, "/remote/repo", "task-remote", "gemini", "remote-host");
    expect(mocks.installKanvibeHookProvider).toHaveBeenNthCalledWith(3, "/remote/repo", "task-remote", "codex", "remote-host");
    expect(mocks.installKanvibeHookProvider).toHaveBeenNthCalledWith(4, "/remote/repo", "task-remote", "openCode", "remote-host");
    expect(mocks.installKanvibeHooks).not.toHaveBeenCalled();
  });

  it("원격 AI 세션 집계는 task worktree scope와 sshHost를 전달한다", async () => {
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
    await getTaskAiSessions(task.id, "claude");

    // Then
    expect(mocks.aggregateAiSessions).toHaveBeenCalledWith({
      worktreePath: "/remote/worktree",
      repoPath: "/remote/repo",
      query: "claude",
      sshHost: "remote-host",
    });
  });
});

describe("projectService task description sync", () => {
  const WORKTREE_PATH = "/workspace/api-worktrees/feature-login";

  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.readTextFile.mockReset();
    mocks.execGit.mockResolvedValue("");
    mocks.getDefaultSessionType.mockResolvedValue("tmux");
    mocks.getClaudeHooksStatus.mockResolvedValue({ installed: true });
    mocks.getGeminiHooksStatus.mockResolvedValue({ installed: true });
    mocks.getCodexHooksStatus.mockResolvedValue({ installed: true });
    mocks.getOpenCodeHooksStatus.mockResolvedValue({ installed: true });
    mocks.listWorktrees.mockResolvedValue([
      { path: "/workspace/api", branch: "main", isBare: false },
      { path: WORKTREE_PATH, branch: "feature/login", isBare: false },
    ]);
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
  });

  function mockWorktreeTaskRepository(save: ReturnType<typeof vi.fn>): void {
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn(async (criteria: { branchName?: string; projectId?: string | null }) => {
        if (criteria.projectId !== "project-1") {
          return null;
        }

        if (criteria.branchName === "main") {
          return {
            id: "task-main",
            branchName: "main",
            projectId: "project-1",
            baseBranch: "main",
            worktreePath: "/workspace/api",
            sshHost: null,
            description: null,
          };
        }

        if (criteria.branchName === "feature/login") {
          return {
            id: "task-login",
            branchName: "feature/login",
            projectId: "project-1",
            baseBranch: "main",
            worktreePath: WORKTREE_PATH,
            sshHost: null,
            description: "옛 설명",
          };
        }

        return null;
      }),
      create: vi.fn((value) => value),
      save,
    });
  }

  /** 지정한 경로에만 내용이 있고 나머지 공유 파일은 없는 저장소를 흉내낸다 */
  function mockSharedTaskFiles(contentByPath: Record<string, string>): void {
    mocks.readTextFiles.mockImplementation(async (targetPaths: string[]) => new Map(
      targetPaths.map((targetPath) => [
        targetPath,
        { exists: targetPath in contentByPath, content: contentByPath[targetPath] ?? "" },
      ]),
    ));
  }

  it("다른 기기가 task.json에 남긴 설명을 DB로 가져온다", async () => {
    // Given
    const save = vi.fn(async (value) => value);
    mockWorktreeTaskRepository(save);
    mockSharedTaskFiles({
      [`${WORKTREE_PATH}/.kanvibe/task.json`]: JSON.stringify({
        schemaVersion: 1,
        description: "결제 실패 로그 원인 추적",
      }),
    });

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    // When
    const result = await syncRegisteredProjectWorktrees();

    // Then
    expect(result.changed).toBe(true);
    expect(save).toHaveBeenCalledWith(expect.objectContaining({
      id: "task-login",
      description: "결제 실패 로그 원인 추적",
    }));
  });

  it("다른 KanVibe가 task.json에 남긴 세션이 다르면 그 세션을 따르고 이전 터미널 연결을 끊는다", async () => {
    // Given
    const save = vi.fn(async (value) => value);
    mockWorktreeTaskRepository(save);
    mockSharedTaskFiles({
      [`${WORKTREE_PATH}/.kanvibe/task.json`]: JSON.stringify({
        schemaVersion: 1,
        description: "옛 설명",
        session: { sessionType: "zellij", sessionName: "api-feature-login" },
      }),
    });

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    // When
    const result = await syncRegisteredProjectWorktrees();

    // Then
    expect(result.changed).toBe(true);
    expect(save).toHaveBeenCalledWith(expect.objectContaining({
      id: "task-login",
      sessionType: "zellij",
      sessionName: "api-feature-login",
    }));
    expect(mocks.detachSession).toHaveBeenCalledWith("task-login", "follow-shared-task-session");
    expect(mocks.broadcastTaskSessionChanged).toHaveBeenCalledWith({ taskId: "task-login" });
    expect(save.mock.invocationCallOrder[0]).toBeLessThan(mocks.broadcastTaskSessionChanged.mock.invocationCallOrder[0]);
  });

  it("task.json의 세션이 이미 같으면 저장하지도 연결을 끊지도 않는다", async () => {
    // Given
    const save = vi.fn(async (value) => value);
    mocks.getTaskRepository.mockResolvedValue({
      findOneBy: vi.fn(async (criteria: { branchName?: string; projectId?: string | null }) => (
        criteria.branchName === "feature/login"
          ? {
              id: "task-login",
              branchName: "feature/login",
              projectId: "project-1",
              baseBranch: "main",
              worktreePath: WORKTREE_PATH,
              sshHost: null,
              description: null,
              sessionType: "zellij",
              sessionName: "api-feature-login",
            }
          : null
      )),
      create: vi.fn((value) => value),
      save,
    });
    mockSharedTaskFiles({
      [`${WORKTREE_PATH}/.kanvibe/task.json`]: JSON.stringify({
        session: { sessionType: "zellij", sessionName: "api-feature-login" },
      }),
    });

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    // When
    await syncRegisteredProjectWorktrees();

    // Then
    expect(save).not.toHaveBeenCalledWith(expect.objectContaining({ id: "task-login" }));
    expect(mocks.detachSession).not.toHaveBeenCalled();
    expect(mocks.broadcastTaskSessionChanged).not.toHaveBeenCalled();
  });

  it("task.json이 없으면 DB에 있던 설명을 지우지 않는다", async () => {
    // Given
    const save = vi.fn(async (value) => value);
    mockWorktreeTaskRepository(save);
    mockSharedTaskFiles({});

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");

    // When
    const result = await syncRegisteredProjectWorktrees();

    // Then
    expect(result.changed).toBe(false);
    expect(save).not.toHaveBeenCalled();
  });

  it("task priority가 있으면 project priority보다 우선해 DB로 가져온다", async () => {
    const save = vi.fn(async (value) => value);
    mockWorktreeTaskRepository(save);
    mockSharedTaskFiles({
      [`${WORKTREE_PATH}/.kanvibe/task.json`]: JSON.stringify({ priority: "low" }),
      "/workspace/api/.kanvibe/project.json": JSON.stringify({ priority: "high" }),
    });

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");
    await syncRegisteredProjectWorktrees();

    expect(save).toHaveBeenCalledWith(expect.objectContaining({
      id: "task-login",
      priority: "low",
    }));
  });

  it("task priority가 없으면 project priority를 DB로 가져온다", async () => {
    const save = vi.fn(async (value) => value);
    mockWorktreeTaskRepository(save);
    mockSharedTaskFiles({
      "/workspace/api/.kanvibe/project.json": JSON.stringify({ priority: "high" }),
    });

    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");
    await syncRegisteredProjectWorktrees();

    expect(save).toHaveBeenCalledWith(expect.objectContaining({
      id: "task-main",
      priority: "high",
    }));
    expect(save).not.toHaveBeenCalledWith(expect.objectContaining({
      id: "task-login",
      priority: "high",
    }));
  });
});

describe("projectService read and recovery isolation", () => {
  const project = { id: "latency-project", name: "api", repoPath: "/latency/api", defaultBranch: "main", sshHost: "latency-host", iconDataUrl: "data:icon", color: "blue" };
  const root = { id: "latency-root", projectId: project.id, title: "main", branchName: "main", baseBranch: "main", worktreePath: project.repoPath, sshHost: project.sshHost, status: "todo", sessionType: "herdr", sessionName: "kanvibe:api" };
  let tasks: typeof root[];
  let taskSave: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.isPathMissing.mockReset().mockResolvedValue(false);
    mocks.execGit.mockReset().mockResolvedValue("");
    mocks.listWorktrees.mockReset().mockResolvedValue([{ path: project.repoPath, branch: "main", isBare: false }]);
    mocks.readTextFile.mockReset().mockResolvedValue('{"projectColor":"blue"}');
    mocks.readTextFiles.mockReset().mockImplementation(async (paths: string[]) => new Map(paths.map((targetPath) => [targetPath, {
      exists: true,
      content: targetPath.endsWith("status.json") ? '{"status":"todo"}' : targetPath.endsWith("project.json") ? '{"projectColor":"blue"}' : "",
    }])));
    mocks.resolveProjectIconDataUrl.mockReset().mockResolvedValue(null);
    tasks = [{ ...root }];
    taskSave = vi.fn(async (task) => {
      const saved = { id: root.id, ...task };
      tasks = [...tasks.filter((existing) => existing.id !== saved.id), saved];
      return saved;
    });
    mocks.getProjectRepository.mockResolvedValue({ find: vi.fn(async () => [project]), findOneBy: vi.fn(async () => project), update: vi.fn(async () => ({ affected: 1 })) });
    mocks.getTaskRepository.mockResolvedValue({
      find: vi.fn(async () => tasks),
      findOneBy: vi.fn(async (criteria) => tasks.find((task) => task.projectId === criteria.projectId && task.branchName === criteria.branchName) ?? null),
      create: vi.fn((task) => task), save: taskSave,
    });
  });

  it("repeated snapshot reads dispatch no remote work even after timers drain", async () => {
    vi.useFakeTimers();
    try {
      const { getAllProjects } = await import("@/desktop/main/services/projectService");
      expect(await getAllProjects()).toEqual([project]);
      expect(await getAllProjects()).toEqual([project]);
      await vi.runAllTimersAsync();
      expect(mocks.isPathMissing).not.toHaveBeenCalled();
      expect(mocks.listWorktrees).not.toHaveBeenCalled();
      expect(mocks.execGit).not.toHaveBeenCalled();
      expect(mocks.writeTextFile).not.toHaveBeenCalled();
      expect(taskSave).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("unchanged root state avoids status and exclude writes during explicit recovery", async () => {
    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");
    const result = await syncRegisteredProjectWorktrees([project] as never);
    expect(result.errors).toEqual([]);
    expect(tasks).toEqual([root]);
    expect(mocks.writeTextFile).not.toHaveBeenCalled();
    expect(mocks.execGit.mock.calls.filter(([command]) => command.includes("--git-common-dir"))).toEqual([]);
  });

  it("missing shared status is persisted during explicit recovery", async () => {
    mocks.readTextFiles.mockImplementation(async (paths: string[]) => new Map(paths.map((targetPath) => [targetPath, { exists: false, content: "" }])));
    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");
    const result = await syncRegisteredProjectWorktrees([project] as never);
    expect(result.errors).toEqual([]);
    expect(mocks.writeTextFile).toHaveBeenCalledWith(
      `${project.repoPath}/.kanvibe/status.json`, expect.stringContaining('"status": "todo"'), project.sshHost,
    );
  });

  it("concurrent recovery of the same project shares its scan", async () => {
    let release!: (value: unknown[]) => void;
    mocks.listWorktrees.mockImplementationOnce(() => new Promise((resolve) => { release = resolve; }));
    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");
    const first = syncRegisteredProjectWorktrees([project] as never);
    const second = syncRegisteredProjectWorktrees([project] as never);
    await vi.waitFor(() => expect(mocks.listWorktrees).toHaveBeenCalled());
    const scansWhileBlocked = mocks.listWorktrees.mock.calls.length;
    release([{ path: project.repoPath, branch: "main", isBare: false }]);
    const [left, right] = await Promise.all([first, second]);
    expect(scansWhileBlocked).toBe(1);
    expect(left).toEqual(right);
    expect(left.errors).toEqual([]);
  });

  it("recovery serializes one host while a different host progresses", async () => {
    let release!: (value: unknown[]) => void;
    mocks.listWorktrees.mockImplementationOnce(() => new Promise((resolve) => { release = resolve; })).mockResolvedValue([]);
    const otherProject = { ...project, id: "latency-other", repoPath: "/latency/other" };
    const otherHost = { ...project, id: "latency-independent", repoPath: "/latency/independent", sshHost: "another-host" };
    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");
    const first = syncRegisteredProjectWorktrees([project] as never);
    await vi.waitFor(() => expect(mocks.listWorktrees).toHaveBeenCalled());
    const second = syncRegisteredProjectWorktrees([otherProject] as never);
    const independent = syncRegisteredProjectWorktrees([otherHost] as never);
    await vi.waitFor(() => expect(mocks.listWorktrees).toHaveBeenCalledWith(otherHost.repoPath, otherHost.sshHost));
    const scansWhileBlocked = mocks.listWorktrees.mock.calls.map(([repoPath]) => repoPath);
    release([]);
    await Promise.all([first, second, independent]);
    expect(scansWhileBlocked).toContain(project.repoPath);
    expect(scansWhileBlocked).toContain(otherHost.repoPath);
    expect(scansWhileBlocked).not.toContain(otherProject.repoPath);
  });

  it("a project deleted while queued on its host is not repaired", async () => {
    let release!: (value: unknown[]) => void;
    mocks.listWorktrees.mockImplementationOnce(() => new Promise((resolve) => { release = resolve; })).mockResolvedValue([]);
    const deletingProject = { ...project, id: "latency-deleted", repoPath: "/latency/deleted" };
    const manager = { findOneBy: vi.fn(async () => deletingProject), delete: vi.fn(), remove: vi.fn() };
    mocks.getProjectRepository.mockResolvedValue({
      find: vi.fn(async () => [project, deletingProject]), update: vi.fn(async () => ({ affected: 1 })),
      manager: { transaction: vi.fn(async (action) => action(manager)) },
    });
    const { syncRegisteredProjectWorktrees, deleteProject } = await import("@/desktop/main/services/projectService");
    const first = syncRegisteredProjectWorktrees([project] as never);
    await vi.waitFor(() => expect(mocks.listWorktrees).toHaveBeenCalled());
    const queued = syncRegisteredProjectWorktrees([deletingProject] as never);
    expect(await deleteProject(deletingProject.id)).toBe(true);
    release([]);
    await Promise.all([first, queued]);
    expect(mocks.listWorktrees.mock.calls.map(([repoPath]) => repoPath)).not.toContain(deletingProject.repoPath);
    expect(tasks.every((task) => task.projectId !== deletingProject.id)).toBe(true);
  });

  it("a failed explicit recovery is retried on the next sync and restores the missing root", async () => {
    tasks = [];
    taskSave.mockRejectedValueOnce(new Error("database offline"));
    const { syncRegisteredProjectWorktrees } = await import("@/desktop/main/services/projectService");
    const failed = await syncRegisteredProjectWorktrees([project] as never);
    expect(failed.errors.join(" ")).toContain("database offline");
    expect(tasks).toEqual([]);
    const retried = await syncRegisteredProjectWorktrees([project] as never);
    expect(retried.errors).toEqual([]);
    expect(tasks).toContainEqual(expect.objectContaining({ projectId: project.id, branchName: "main", status: "todo" }));
    expect(retried.changed).toBe(true);
  });
});
