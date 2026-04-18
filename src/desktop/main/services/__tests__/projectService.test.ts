import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  execGit: vi.fn(),
  homedir: vi.fn(() => "/home/tester"),
  validateGitRepo: vi.fn(),
  getDefaultBranch: vi.fn(),
  scanGitRepos: vi.fn(),
  getProjectRepository: vi.fn(),
  getTaskRepository: vi.fn(),
  createSessionWithoutWorktree: vi.fn(),
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
  listWorktrees: vi.fn(),
  execGit: mocks.execGit,
}));

vi.mock("@/lib/worktree", () => ({
  isSessionAlive: vi.fn(),
  formatSessionName: vi.fn(),
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
  aggregateAiSessions: vi.fn(),
  getAiSessionDetail: vi.fn(),
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
  broadcastBoardUpdate: vi.fn(),
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
  installKanvibeHooks: vi.fn(),
}));

describe("projectService.listSubdirectories", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.getDefaultSessionType.mockResolvedValue("tmux");
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
});
