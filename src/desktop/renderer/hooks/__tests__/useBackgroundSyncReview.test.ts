import { describe, expect, it, vi } from "vitest";
import type { AppNotification } from "@/desktop/shared/notifications";
import { closeBackgroundSyncReview, deleteBackgroundSyncReviewTasks, getBackgroundSyncReviewCount, getBackgroundSyncReviewSnapshot, openBackgroundSyncReview } from "../useBackgroundSyncReview";

const mocks = vi.hoisted(() => ({ deleteTask: vi.fn() }));
vi.mock("@/desktop/renderer/actions/kanban", () => ({ deleteTask: mocks.deleteTask }));

function notification(id: string): AppNotification {
  return {
    id, title: "Review", body: "", taskId: null, relativePath: "/ko", locale: "ko", isRead: false, createdAt: "2026-11-14", dedupeKey: id,
    action: { type: "background-sync-review", payload: {
      mergedPullRequests: ["a", "b"].map((taskId) => ({ taskId, taskTitle: taskId, branchName: taskId, prUrl: "https://github.com/test/repo/pull/1", mergedAt: "2026-11-14" })),
      registeredWorktrees: [],
    } },
  };
}

describe("background review cleanup", () => {
  it("closes the dialog on start and preserves a running cleanup when reopened", async () => {
    let finish!: (value: boolean) => void;
    mocks.deleteTask.mockReturnValueOnce(new Promise((resolve) => { finish = resolve; })).mockResolvedValue(true);
    const review = notification("background-cleanup");
    openBackgroundSyncReview(review);
    const cleanup = deleteBackgroundSyncReviewTasks();
    expect(getBackgroundSyncReviewSnapshot()).toMatchObject({ isOpen: false, isCleaning: true });
    const repeated = deleteBackgroundSyncReviewTasks();
    expect(mocks.deleteTask).toHaveBeenCalledTimes(1);
    openBackgroundSyncReview(review);
    expect(getBackgroundSyncReviewSnapshot().isOpen).toBe(true);
    closeBackgroundSyncReview();
    finish(true);
    await cleanup;
    await repeated;
    expect(getBackgroundSyncReviewSnapshot()).toMatchObject({ isOpen: false, isCleaning: false });
    expect(Object.values(getBackgroundSyncReviewSnapshot().results).map((result) => result.status)).toEqual(["succeeded", "succeeded"]);
  });

  it("keeps failures for reopening and retries only failed tasks", async () => {
    mocks.deleteTask.mockClear();
    mocks.deleteTask.mockResolvedValueOnce(true).mockRejectedValueOnce(new Error("busy")).mockResolvedValueOnce(true);
    const review = notification("cleanup-retry");
    openBackgroundSyncReview(review);
    expect(getBackgroundSyncReviewCount(review)).toBe(2);
    await deleteBackgroundSyncReviewTasks();
    expect(getBackgroundSyncReviewCount(review)).toBe(1);
    openBackgroundSyncReview(review);
    expect(getBackgroundSyncReviewSnapshot().results.b).toEqual({ status: "failed", reason: "busy" });
    await deleteBackgroundSyncReviewTasks(true);
    expect(mocks.deleteTask.mock.calls.map(([taskId]) => taskId)).toEqual(["a", "b", "b"]);
    expect(getBackgroundSyncReviewCount(review)).toBe(0);
  });
});
