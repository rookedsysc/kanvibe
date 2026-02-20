// @vitest-environment node
import { describe, it, expect, vi, beforeEach } from "vitest";
import { TaskStatus, SessionType } from "@/entities/KanbanTask";

// --- Mocks ---

const mockTaskCreate = vi.fn((data: Record<string, unknown>) => ({ ...data }));
const mockTaskSave = vi.fn((entity: Record<string, unknown>) => ({
  id: "task-1",
  ...entity,
}));
const mockCreateQueryBuilder = vi.fn().mockReturnValue({
  select: vi.fn().mockReturnThis(),
  where: vi.fn().mockReturnThis(),
  getRawOne: vi.fn().mockResolvedValue({ max: 0 }),
});

const mockProjectFindOneBy = vi.fn();

vi.mock("@/lib/database", () => ({
  getTaskRepository: vi.fn().mockResolvedValue({
    create: mockTaskCreate,
    save: mockTaskSave,
    createQueryBuilder: mockCreateQueryBuilder,
  }),
  getProjectRepository: vi.fn().mockResolvedValue({
    findOneBy: mockProjectFindOneBy,
  }),
}));

const mockCreateWorktreeWithSession = vi.fn();

vi.mock("@/lib/worktree", () => ({
  createWorktreeWithSession: mockCreateWorktreeWithSession,
  removeWorktreeAndSession: vi.fn(),
  removeWorktreeAndBranch: vi.fn(),
  createSessionWithoutWorktree: vi.fn(),
  removeSessionOnly: vi.fn(),
}));

vi.mock("@/lib/claudeHooksSetup", () => ({
  setupClaudeHooks: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("next/cache", () => ({
  revalidatePath: vi.fn(),
}));

describe("createTask", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should create task with TODO status when no worktree session is needed", async () => {
    // Given
    const { createTask } = await import("@/app/actions/kanban");

    // When
    const result = await createTask({ title: "simple task" });

    // Then
    expect(mockTaskCreate).toHaveBeenCalledWith(
      expect.objectContaining({ status: TaskStatus.TODO })
    );
    expect(result.status).toBe(TaskStatus.TODO);
  });

  it("should keep TODO status even when worktree session is created successfully", async () => {
    // Given
    const project = {
      id: "proj-1",
      name: "test-project",
      repoPath: "/repo/path",
      defaultBranch: "main",
      sshHost: null,
    };
    mockProjectFindOneBy.mockResolvedValue(project);
    mockCreateWorktreeWithSession.mockResolvedValue({
      worktreePath: "/repo/path/worktrees/feat-branch",
      sessionName: "feat-branch-session",
    });

    const { createTask } = await import("@/app/actions/kanban");

    // When
    const result = await createTask({
      title: "worktree task",
      branchName: "feat-branch",
      sessionType: SessionType.TMUX,
      projectId: "proj-1",
    });

    // Then
    expect(result.status).toBe(TaskStatus.TODO);
    expect(result.worktreePath).toBe("/repo/path/worktrees/feat-branch");
    expect(result.sessionName).toBe("feat-branch-session");
  });
});
