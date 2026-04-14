import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  execGit: vi.fn(),
  homedir: vi.fn(() => "/home/tester"),
}));

vi.mock("@/lib/database", () => ({
  getProjectRepository: vi.fn(),
  getTaskRepository: vi.fn(),
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
  validateGitRepo: vi.fn(),
  getDefaultBranch: vi.fn(),
  listBranches: vi.fn(),
  scanGitRepos: vi.fn(),
  listWorktrees: vi.fn(),
  execGit: mocks.execGit,
}));

vi.mock("@/lib/worktree", () => ({
  isSessionAlive: vi.fn(),
  formatSessionName: vi.fn(),
  createSessionWithoutWorktree: vi.fn(),
}));

vi.mock("@/lib/claudeHooksSetup", () => ({
  setupClaudeHooks: vi.fn(),
  getClaudeHooksStatus: vi.fn(),
}));

vi.mock("@/lib/geminiHooksSetup", () => ({
  setupGeminiHooks: vi.fn(),
  getGeminiHooksStatus: vi.fn(),
}));

vi.mock("@/lib/codexHooksSetup", () => ({
  setupCodexHooks: vi.fn(),
  getCodexHooksStatus: vi.fn(),
}));

vi.mock("@/lib/openCodeHooksSetup", () => ({
  setupOpenCodeHooks: vi.fn(),
  getOpenCodeHooksStatus: vi.fn(),
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
  computeProjectColor: vi.fn(),
}));

vi.mock("@/lib/boardNotifier", () => ({
  broadcastBoardUpdate: vi.fn(),
}));

vi.mock("@/lib/sshConfig", () => ({
  getAvailableHosts: vi.fn(),
}));

vi.mock("@/desktop/main/services/appSettingsService", () => ({
  getDefaultSessionType: vi.fn(),
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
