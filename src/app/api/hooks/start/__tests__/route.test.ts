import { describe, it, expect, vi, beforeEach } from "vitest";
import { TaskStatus, SessionType } from "@/entities/KanbanTask";

// --- Mocks ---

const mockTaskCreate = vi.fn((data: Record<string, unknown>) => ({ ...data }));
const mockTaskSave = vi.fn((entity: Record<string, unknown>) => ({
  id: "task-1",
  status: TaskStatus.PROGRESS,
  sessionName: null,
  ...entity,
}));
const mockProjectFindOneBy = vi.fn();

vi.mock("@/lib/database", () => ({
  getTaskRepository: vi.fn().mockResolvedValue({
    create: mockTaskCreate,
    save: mockTaskSave,
  }),
  getProjectRepository: vi.fn().mockResolvedValue({
    findOneBy: mockProjectFindOneBy,
  }),
}));

vi.mock("@/lib/worktree", () => ({
  createWorktreeWithSession: vi.fn().mockResolvedValue({
    worktreePath: "/worktree/path",
    sessionName: "session-1",
  }),
}));

vi.mock("next/cache", () => ({
  revalidatePath: vi.fn(),
}));

vi.mock("@/lib/boardNotifier", () => ({
  broadcastBoardUpdate: vi.fn(),
}));

describe("POST /api/hooks/start — baseBranch", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should set baseBranch to project.defaultBranch when baseBranch is not provided in request", async () => {
    // Given
    mockProjectFindOneBy.mockResolvedValue({
      id: "proj-1",
      repoPath: "/repo/path",
      defaultBranch: "main",
      sshHost: null,
    });

    const request = new Request("http://localhost:4885/api/hooks/start", {
      method: "POST",
      body: JSON.stringify({
        title: "New Task",
        branchName: "feat-new",
        sessionType: "tmux",
        projectId: "proj-1",
      }),
    });

    const { POST } = await import("@/app/api/hooks/start/route");

    // When
    const response = await POST(request as never);
    const body = await response.json();

    // Then
    expect(body.success).toBe(true);
    expect(mockTaskSave).toHaveBeenCalledWith(
      expect.objectContaining({ baseBranch: "main" })
    );
  });

  it("should use provided baseBranch from request when available", async () => {
    // Given
    mockProjectFindOneBy.mockResolvedValue({
      id: "proj-1",
      repoPath: "/repo/path",
      defaultBranch: "main",
      sshHost: null,
    });

    const request = new Request("http://localhost:4885/api/hooks/start", {
      method: "POST",
      body: JSON.stringify({
        title: "New Task",
        branchName: "feat-new",
        baseBranch: "develop",
        sessionType: "tmux",
        projectId: "proj-1",
      }),
    });

    const { POST } = await import("@/app/api/hooks/start/route");

    // When
    const response = await POST(request as never);
    const body = await response.json();

    // Then
    expect(body.success).toBe(true);
    expect(mockTaskSave).toHaveBeenCalledWith(
      expect.objectContaining({ baseBranch: "develop" })
    );
  });

  it("should keep baseBranch as null when no project context exists", async () => {
    // Given
    const request = new Request("http://localhost:4885/api/hooks/start", {
      method: "POST",
      body: JSON.stringify({
        title: "Simple Task",
      }),
    });

    const { POST } = await import("@/app/api/hooks/start/route");

    // When
    const response = await POST(request as never);
    const body = await response.json();

    // Then
    expect(body.success).toBe(true);
    expect(mockTaskCreate).toHaveBeenCalledWith(
      expect.objectContaining({ baseBranch: null })
    );
  });
});
