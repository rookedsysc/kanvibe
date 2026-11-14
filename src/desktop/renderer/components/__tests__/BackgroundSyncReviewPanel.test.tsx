import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import BackgroundSyncReviewPanel from "@/desktop/renderer/components/BackgroundSyncReviewPanel";
import { deleteTask } from "@/desktop/renderer/actions/kanban";
import type { AppNotification } from "@/desktop/shared/notifications";

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  deleteTask: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) => (
    values ? `${key}:${JSON.stringify(values)}` : key
  ),
}));

function createMergedPullRequest(taskId: string) {
  return {
    taskId,
    taskTitle: `Title ${taskId}`,
    branchName: `feature/${taskId}`,
    prUrl: `https://github.com/kanvibe/kanvibe/pull/${taskId}`,
    mergedAt: "2026-05-01T01:00:00Z",
  };
}

function createReviewNotification(taskIds: string[]): AppNotification {
  return {
    id: "n-review",
    title: "Background sync review",
    body: "Review pending items",
    taskId: null,
    relativePath: "/en",
    locale: "en",
    isRead: false,
    createdAt: "2026-05-01T00:00:00.000Z",
    dedupeKey: "background-review-1",
    action: {
      type: "background-sync-review",
      payload: {
        mergedPullRequests: taskIds.map(createMergedPullRequest),
        registeredWorktrees: [],
      },
    },
  };
}

function openReviewPanel(taskIds: string[]) {
  const notification = createReviewNotification(taskIds);
  const onClose = vi.fn();
  const view = render(<BackgroundSyncReviewPanel review={notification.action!.payload} onClose={() => { onClose(); view.unmount(); }} />);
}

describe("BackgroundSyncReviewPanel - PR merge 일괄 삭제", () => {
  beforeEach(() => {
    vi.mocked(deleteTask).mockReset();
  });

  it("삭제하는 동안 상세를 유지하고 처리 수를 알린다", async () => {
    let resolveFirstDelete: (value: boolean) => void = () => {};
    vi.mocked(deleteTask)
      .mockReturnValueOnce(new Promise((resolve) => {
        resolveFirstDelete = resolve;
      }))
      .mockResolvedValueOnce(true);
    openReviewPanel(["task-a", "task-b"]);

    fireEvent.click(screen.getByRole("button", { name: "delete" }));

    expect(screen.getByRole("status").textContent).toBe('deleteProgress:{"processed":0,"total":2}');
    expect(screen.getByText("title")).toBeTruthy();

    resolveFirstDelete(true);

    await waitFor(() => {
      expect(screen.queryByText("title")).toBeNull();
    });
    expect(deleteTask).toHaveBeenNthCalledWith(1, "task-a");
    expect(deleteTask).toHaveBeenNthCalledWith(2, "task-b");
  });

  it("일부가 실패하면 대상별 결과와 요약을 남기고 실패한 항목만 다시 삭제한다", async () => {
    vi.mocked(deleteTask)
      .mockResolvedValueOnce(true)
      .mockRejectedValueOnce(new Error("worktree busy"))
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(true);
    openReviewPanel(["task-a", "task-b", "task-c"]);

    fireEvent.click(screen.getByRole("button", { name: "delete" }));

    await waitFor(() => {
      expect(screen.getByRole("alert").textContent).toBe('deleteSummary:{"total":3,"succeeded":2,"failed":1}');
    });
    expect(screen.getByText("worktree busy")).toBeTruthy();
    expect(screen.getAllByText("deleteSucceeded")).toHaveLength(2);
    expect(screen.getByText("deleteFailed")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "retryFailedDeletes" }));

    await waitFor(() => {
      expect(screen.queryByText("title")).toBeNull();
    });
    expect(deleteTask).toHaveBeenCalledTimes(4);
    expect(deleteTask).toHaveBeenLastCalledWith("task-b");
  });
});
