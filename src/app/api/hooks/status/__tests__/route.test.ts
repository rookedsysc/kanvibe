import { beforeEach, describe, expect, it, vi } from "vitest";

const mockProjectFindOneBy = vi.fn();
const mockTaskFindOneBy = vi.fn();
const mockTaskSave = vi.fn();
const mockBroadcastHookStatusTargetMissing = vi.fn();
const mockBroadcastBoardUpdate = vi.fn();
const mockBroadcastTaskStatusChanged = vi.fn();

vi.mock("@/lib/database", () => ({
  getProjectRepository: vi.fn().mockResolvedValue({
    findOneBy: mockProjectFindOneBy,
  }),
  getTaskRepository: vi.fn().mockResolvedValue({
    findOneBy: mockTaskFindOneBy,
    save: mockTaskSave,
  }),
}));

vi.mock("@/app/actions/kanban", () => ({
  cleanupTaskResources: vi.fn(),
}));

vi.mock("next/cache", () => ({
  revalidatePath: vi.fn(),
}));

vi.mock("@/lib/boardNotifier", () => ({
  broadcastBoardUpdate: mockBroadcastBoardUpdate,
  broadcastHookStatusTargetMissing: mockBroadcastHookStatusTargetMissing,
  broadcastTaskStatusChanged: mockBroadcastTaskStatusChanged,
}));

describe("POST /api/hooks/status", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should broadcast missing project notification and keep 404 response when project is not found", async () => {
    // Given
    mockProjectFindOneBy.mockResolvedValue(null);

    const request = new Request("http://localhost:4885/api/hooks/status", {
      method: "POST",
      body: JSON.stringify({
        branchName: "feat/missing-project",
        projectName: "case-study",
        status: "review",
      }),
    });

    const { POST } = await import("@/app/api/hooks/status/route");

    // When
    const response = await POST(request as never);
    const body = await response.json();

    // Then
    expect(response.status).toBe(404);
    expect(body).toEqual({
      success: false,
      error: "프로젝트를 찾을 수 없습니다: case-study",
    });
    expect(mockBroadcastHookStatusTargetMissing).toHaveBeenCalledWith({
      projectName: "case-study",
      branchName: "feat/missing-project",
      requestedStatus: "review",
      reason: "project-not-found",
    });
    expect(mockTaskSave).not.toHaveBeenCalled();
    expect(mockBroadcastBoardUpdate).not.toHaveBeenCalled();
    expect(mockBroadcastTaskStatusChanged).not.toHaveBeenCalled();
  });

  it("should broadcast missing task notification and keep 404 response when task is not found", async () => {
    // Given
    mockProjectFindOneBy.mockResolvedValue({
      id: "project-1",
      name: "case-study",
    });
    mockTaskFindOneBy.mockResolvedValue(null);

    const request = new Request("http://localhost:4885/api/hooks/status", {
      method: "POST",
      body: JSON.stringify({
        branchName: "feat/missing-task",
        projectName: "case-study",
        status: "review",
      }),
    });

    const { POST } = await import("@/app/api/hooks/status/route");

    // When
    const response = await POST(request as never);
    const body = await response.json();

    // Then
    expect(response.status).toBe(404);
    expect(body).toEqual({
      success: false,
      error: "작업을 찾을 수 없습니다: case-study/feat/missing-task",
    });
    expect(mockBroadcastHookStatusTargetMissing).toHaveBeenCalledWith({
      projectName: "case-study",
      branchName: "feat/missing-task",
      requestedStatus: "review",
      reason: "task-not-found",
    });
    expect(mockTaskSave).not.toHaveBeenCalled();
    expect(mockBroadcastBoardUpdate).not.toHaveBeenCalled();
    expect(mockBroadcastTaskStatusChanged).not.toHaveBeenCalled();
  });
});
