import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// --- Mocks ---

const mockFetch = vi.fn().mockResolvedValue({ ok: true });
vi.stubGlobal("fetch", mockFetch);

describe("boardNotifier", () => {
  beforeEach(() => {
    vi.resetModules();
    mockFetch.mockClear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("should send board-updated message via internal broadcast endpoint", async () => {
    // Given
    const { broadcastBoardUpdate } = await import("@/lib/boardNotifier");

    // When
    broadcastBoardUpdate();

    // Then
    expect(mockFetch).toHaveBeenCalledWith(
      expect.stringContaining("/_internal/broadcast"),
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ type: "board-updated" }),
      })
    );
  });

  it("should send task-status-changed message with payload via internal broadcast endpoint", async () => {
    // Given
    const { broadcastTaskStatusChanged } = await import("@/lib/boardNotifier");
    const payload = {
      projectName: "kanvibe",
      branchName: "feat/test",
      taskTitle: "테스트 작업",
      description: "설명",
      newStatus: "review",
    };

    // When
    broadcastTaskStatusChanged(payload);

    // Then
    expect(mockFetch).toHaveBeenCalledWith(
      expect.stringContaining("/_internal/broadcast"),
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ type: "task-status-changed", ...payload }),
      })
    );
  });

  it("should include description as null in payload when not provided", async () => {
    // Given
    const { broadcastTaskStatusChanged } = await import("@/lib/boardNotifier");

    // When
    broadcastTaskStatusChanged({
      projectName: "test",
      branchName: "main",
      taskTitle: "작업",
      description: null,
      newStatus: "done",
    });

    // Then
    const body = JSON.parse(mockFetch.mock.calls[0][1].body);
    expect(body.description).toBeNull();
    expect(body.type).toBe("task-status-changed");
  });

  it("should silently catch fetch errors", async () => {
    // Given
    mockFetch.mockRejectedValueOnce(new Error("Connection refused"));
    const { broadcastBoardUpdate } = await import("@/lib/boardNotifier");

    // When & Then
    expect(() => broadcastBoardUpdate()).not.toThrow();
  });
});
