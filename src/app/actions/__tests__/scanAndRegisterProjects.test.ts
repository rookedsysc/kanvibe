import { describe, it, expect, vi, beforeEach } from "vitest";
import { TaskStatus, SessionType } from "@/entities/KanbanTask";

// --- Mocks ---

const mockTaskFindOneBy = vi.fn();
const mockTaskCreate = vi.fn((data: Record<string, unknown>) => ({ ...data }));
const mockTaskSave = vi.fn((entity: Record<string, unknown>) => entity);
const mockProjectFind = vi.fn();
const mockProjectCreate = vi.fn((data: Record<string, unknown>) => ({ id: "proj-1", ...data }));
const mockProjectSave = vi.fn((entity: Record<string, unknown>) => entity);

vi.mock("@/lib/database", () => ({
  getTaskRepository: vi.fn().mockResolvedValue({
    findOneBy: mockTaskFindOneBy,
    create: mockTaskCreate,
    save: mockTaskSave,
  }),
  getProjectRepository: vi.fn().mockResolvedValue({
    find: mockProjectFind,
    create: mockProjectCreate,
    save: mockProjectSave,
  }),
}));

vi.mock("@/lib/gitOperations", () => ({
  scanGitRepos: vi.fn().mockResolvedValue(["/repo/path"]),
  getDefaultBranch: vi.fn().mockResolvedValue("main"),
  listWorktrees: vi.fn().mockResolvedValue([]),
  validateGitRepo: vi.fn(),
  listBranches: vi.fn(),
  execGit: vi.fn(),
}));

vi.mock("@/lib/worktree", () => ({
  isSessionAlive: vi.fn().mockResolvedValue(false),
  formatSessionName: vi.fn((projectName: string, branchName: string) => `${projectName}-${branchName}`.replace(/\//g, "-")),
  createSessionWithoutWorktree: vi.fn().mockResolvedValue({ sessionName: "test-session" }),
}));

vi.mock("@/lib/claudeHooksSetup", () => ({
  setupClaudeHooks: vi.fn().mockResolvedValue(undefined),
  getClaudeHooksStatus: vi.fn(),
}));

vi.mock("@/lib/geminiHooksSetup", () => ({
  setupGeminiHooks: vi.fn().mockResolvedValue(undefined),
  getGeminiHooksStatus: vi.fn(),
}));

vi.mock("@/lib/codexHooksSetup", () => ({
  setupCodexHooks: vi.fn().mockResolvedValue(undefined),
  getCodexHooksStatus: vi.fn(),
}));

vi.mock("next/cache", () => ({
  revalidatePath: vi.fn(),
}));

const { listWorktrees } = await import("@/lib/gitOperations");

describe("scanAndRegisterProjects — baseBranch", () => {
  beforeEach(() => {
    vi.clearAllMocks();

    mockProjectFind.mockResolvedValue([]);
    mockProjectSave.mockImplementation((entity: Record<string, unknown>) => ({
      id: "proj-1",
      name: "test-project",
      repoPath: "/repo/path",
      defaultBranch: "main",
      sshHost: null,
      ...entity,
    }));
  });

  it("should set baseBranch to project.defaultBranch when creating a new worktree task", async () => {
    // Given
    mockProjectFind
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
        { id: "proj-1", name: "test-project", repoPath: "/repo/path", defaultBranch: "main", sshHost: null },
      ]);

    vi.mocked(listWorktrees).mockResolvedValue([
      { path: "/repo/path/worktrees/feat-branch", branch: "feat-branch", isBare: false },
    ]);

    mockTaskFindOneBy.mockResolvedValue(null);

    const { scanAndRegisterProjects } = await import("@/app/actions/project");

    // When
    await scanAndRegisterProjects("/root");

    // Then
    expect(mockTaskCreate).toHaveBeenCalledWith(
      expect.objectContaining({
        branchName: "feat-branch",
        baseBranch: "main",
        projectId: "proj-1",
      })
    );
  });

  it("should set baseBranch on orphan task when linking to project", async () => {
    // Given
    const orphanTask = {
      branchName: "feat-orphan",
      projectId: null,
      worktreePath: null,
      baseBranch: null,
    };

    mockProjectFind
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
        { id: "proj-1", name: "test-project", repoPath: "/repo/path", defaultBranch: "main", sshHost: null },
      ]);

    vi.mocked(listWorktrees).mockResolvedValue([
      { path: "/repo/path/worktrees/feat-orphan", branch: "feat-orphan", isBare: false },
    ]);

    /** createDefaultBranchTask에서 2번, worktree 스캔에서 2번 findOneBy를 호출한다 */
    mockTaskFindOneBy
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(orphanTask);

    const { scanAndRegisterProjects } = await import("@/app/actions/project");

    // When
    await scanAndRegisterProjects("/root");

    // Then
    expect(orphanTask.baseBranch).toBe("main");
    expect(mockTaskSave).toHaveBeenCalledWith(
      expect.objectContaining({ baseBranch: "main" })
    );
  });

  it("should preserve existing baseBranch on orphan task when already set", async () => {
    // Given
    const orphanTask = {
      branchName: "feat-orphan",
      projectId: null,
      worktreePath: null,
      baseBranch: "develop",
    };

    mockProjectFind
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
        { id: "proj-1", name: "test-project", repoPath: "/repo/path", defaultBranch: "main", sshHost: null },
      ]);

    vi.mocked(listWorktrees).mockResolvedValue([
      { path: "/repo/path/worktrees/feat-orphan", branch: "feat-orphan", isBare: false },
    ]);

    /** createDefaultBranchTask에서 2번, worktree 스캔에서 2번 findOneBy를 호출한다 */
    mockTaskFindOneBy
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(orphanTask);

    const { scanAndRegisterProjects } = await import("@/app/actions/project");

    // When
    await scanAndRegisterProjects("/root");

    // Then
    expect(orphanTask.baseBranch).toBe("develop");
  });
});
