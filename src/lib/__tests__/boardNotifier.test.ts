import { describe, it, expect, vi, beforeEach } from "vitest";

describe("boardNotifier", () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it("should deliver board-updated message to subscribers", async () => {
    const { broadcastBoardUpdate, subscribeToBoardEvents } = await import("@/lib/boardNotifier");
    const listener = vi.fn();
    const unsubscribe = subscribeToBoardEvents(listener);

    broadcastBoardUpdate();

    expect(listener).toHaveBeenCalledWith({ type: "board-updated" });
    unsubscribe();
  });

  it("should deliver task-status-changed message with payload to subscribers", async () => {
    const { broadcastTaskStatusChanged, subscribeToBoardEvents } = await import("@/lib/boardNotifier");
    const payload = {
      projectName: "kanvibe",
      branchName: "feat/test",
      taskTitle: "테스트 작업",
      description: "설명",
      newStatus: "review",
      taskId: "task-123",
    };
    const listener = vi.fn();
    const unsubscribe = subscribeToBoardEvents(listener);

    broadcastTaskStatusChanged(payload);

    expect(listener).toHaveBeenCalledWith({ type: "task-status-changed", ...payload });
    unsubscribe();
  });

  it("should include description as null in payload when not provided", async () => {
    const { broadcastTaskStatusChanged, subscribeToBoardEvents } = await import("@/lib/boardNotifier");
    const listener = vi.fn();
    const unsubscribe = subscribeToBoardEvents(listener);

    broadcastTaskStatusChanged({
      projectName: "test",
      branchName: "main",
      taskTitle: "작업",
      description: null,
      newStatus: "done",
      taskId: "task-456",
    });

    expect(listener).toHaveBeenCalledWith(expect.objectContaining({ description: null, type: "task-status-changed", taskId: "task-456" }));
    unsubscribe();
  });

  it("should deliver hook-status-target-missing message with payload to subscribers", async () => {
    const { broadcastHookStatusTargetMissing, subscribeToBoardEvents } = await import("@/lib/boardNotifier");
    const payload = {
      taskId: "task-404",
      requestedStatus: "review",
      reason: "task-not-found" as const,
    };
    const listener = vi.fn();
    const unsubscribe = subscribeToBoardEvents(listener);

    broadcastHookStatusTargetMissing(payload);

    expect(listener).toHaveBeenCalledWith({ type: "hook-status-target-missing", ...payload });
    unsubscribe();
  });

  it("should stop notifying after unsubscribe", async () => {
    const { broadcastBoardUpdate, subscribeToBoardEvents } = await import("@/lib/boardNotifier");
    const listener = vi.fn();
    const unsubscribe = subscribeToBoardEvents(listener);

    unsubscribe();

    broadcastBoardUpdate();

    expect(listener).not.toHaveBeenCalled();
  });
});
