import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "@/desktop/renderer/App";
import type { AppNotification } from "@/desktop/shared/notifications";

const mocks = vi.hoisted(() => ({
  triggerDesktopRefresh: vi.fn(),
  updateTaskStatus: vi.fn(),
  deleteTasks: vi.fn().mockResolvedValue([]),
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  updateTaskStatus: mocks.updateTaskStatus,
  deleteTasks: mocks.deleteTasks,
}));

vi.mock("@/desktop/renderer/utils/refresh", () => ({
  triggerDesktopRefresh: (...args: unknown[]) => mocks.triggerDesktopRefresh(...args),
}));

vi.mock("@/desktop/renderer/actions/appSettings", () => ({
  getThemePreference: vi.fn().mockResolvedValue("system"),
  getTerminalOpacity: vi.fn().mockResolvedValue(1),
}));

vi.mock("@/desktop/renderer/routes/BoardRoute", () => ({
  default: () => <div>board route</div>,
}));

vi.mock("@/desktop/renderer/routes/DiffRoute", () => ({
  default: () => <div>diff route</div>,
}));

vi.mock("@/desktop/renderer/routes/PaneLayoutRoute", () => ({
  default: () => <div>pane layout route</div>,
}));

vi.mock("@/desktop/renderer/routes/SettingsRoute", () => ({
  default: () => <div>settings route</div>,
}));

vi.mock("@/desktop/renderer/routes/TaskDetailRoute", () => ({
  default: () => <div>task detail route</div>,
}));

vi.mock("@/desktop/renderer/routes/NotFoundRoute", () => ({
  default: () => <div>not found route</div>,
}));

vi.mock("@/desktop/renderer/components/TaskQuickSearchDialog", () => ({
  default: () => <div>quick search</div>,
}));

vi.mock("@/desktop/renderer/components/NotificationListener", () => ({
  default: () => <div>notification listener</div>,
}));

vi.mock("@/desktop/renderer/components/ReleaseUpdateDialog", () => ({
  default: () => <div>release update dialog</div>,
}));

vi.mock("@/desktop/renderer/components/BoardEventAlert", () => ({
  default: () => <div>board event alert</div>,
}));

describe("App", () => {
  let boardEventListener: ((event: { type: string }) => void) | null = null;
  let terminalTabShortcutListener: ((command: { type: string }) => void) | null = null;
  const closeCurrentWindow = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    boardEventListener = null;
    terminalTabShortcutListener = null;
    window.kanvibeDesktop = {
      isDesktop: true,
      onBoardEvent: vi.fn((listener: (event: { type: string }) => void) => {
        boardEventListener = listener;
        return vi.fn();
      }),
      onTerminalTabShortcut: vi.fn((listener: (command: { type: string }) => void) => {
        terminalTabShortcutListener = listener;
        return vi.fn();
      }),
      closeCurrentWindow,
    } as unknown as NonNullable<typeof window.kanvibeDesktop>;
  });

  /** 창 닫기 리스너가 태스크 상세에만 있으면 보드에서 Mod+W를 눌러도 창이 닫히지 않는다 */
  it("보드 화면에서도 창 닫기 명령을 받아 창을 닫는다", async () => {
    window.location.hash = "#/ko";

    render(<App />);

    await waitFor(() => {
      expect(screen.getByText("board route")).toBeTruthy();
    });

    act(() => {
      terminalTabShortcutListener?.({ type: "close-window" });
    });

    expect(closeCurrentWindow).toHaveBeenCalledTimes(1);
  });

  it("보드에서 Mod+W 키 입력만으로도 창을 닫는다", async () => {
    window.location.hash = "#/ko";

    render(<App />);

    await waitFor(() => {
      expect(screen.getByText("board route")).toBeTruthy();
    });

    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "w", ctrlKey: true, bubbles: true }));
    });

    expect(closeCurrentWindow).toHaveBeenCalledTimes(1);
  });

  /** 태스크 상세의 Mod+W는 탭만 닫아야 하므로 App이 가로채 창을 닫으면 안 된다 */
  it("태스크 상세에서 Mod+W를 눌러도 App은 창을 닫지 않는다", async () => {
    window.location.hash = "#/ko/task/task-1";

    render(<App />);

    await waitFor(() => {
      expect(screen.getByText("task detail route")).toBeTruthy();
    });

    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "w", ctrlKey: true, bubbles: true }));
      terminalTabShortcutListener?.({ type: "close-tab" });
    });

    expect(closeCurrentWindow).not.toHaveBeenCalled();
  });

  it("창 닫기가 아닌 탭 명령은 App이 처리하지 않는다", async () => {
    window.location.hash = "#/ko";

    render(<App />);

    await waitFor(() => {
      expect(screen.getByText("board route")).toBeTruthy();
    });

    act(() => {
      terminalTabShortcutListener?.({ type: "new-tab" });
      terminalTabShortcutListener?.({ type: "close-tab" });
    });

    expect(closeCurrentWindow).not.toHaveBeenCalled();
  });

  it("shows board route on index", async () => {
    window.location.hash = "#/ko";

    render(<App />);

    await waitFor(() => {
      expect(screen.getByText("board route")).toBeTruthy();
    });
  });

  it("shows settings route", async () => {
    window.location.hash = "#/ko/settings";

    render(<App />);

    await waitFor(() => {
      expect(screen.getByText("settings route")).toBeTruthy();
    });
  });

  it("debounces board update events and refreshes all visible data", async () => {
    window.location.hash = "#/ko/task/task-1";

    render(<App />);

    await waitFor(() => {
      expect(screen.getByText("task detail route")).toBeTruthy();
    });

    vi.useFakeTimers();
    try {
      boardEventListener?.({ type: "board-updated" });
      boardEventListener?.({ type: "board-updated" });

      expect(mocks.triggerDesktopRefresh).not.toHaveBeenCalled();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(250);
      });

      expect(mocks.triggerDesktopRefresh).toHaveBeenCalledTimes(1);
      expect(mocks.triggerDesktopRefresh).toHaveBeenCalledWith("all");
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not show a background sync review dialog from pending activation on route entry", async () => {
    window.location.hash = "#/en/task/task-1";
    window.kanvibeDesktop = {
      isDesktop: true,
      onBoardEvent: vi.fn(() => vi.fn()),
      consumePendingNotificationActivation: vi.fn().mockResolvedValue({
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
            mergedPullRequests: [
              {
                taskId: "task-1",
                taskTitle: "Detail task",
                branchName: "feature/current-route",
                prUrl: "https://github.com/kanvibe/kanvibe/pull/410",
                mergedAt: "2026-05-01T01:00:00Z",
              },
            ],
            registeredWorktrees: [
              {
                taskId: "task-worktree",
                projectName: "kanvibe",
                branchName: "feature/new-worktree",
                worktreePath: "/repo/kanvibe__worktrees/feature-new-worktree",
                sshHost: null,
              },
            ],
            pulledTasks: [
              {
                taskId: "task-pull",
                taskTitle: "Pull target",
                branchName: "feature/pull-fail",
                worktreePath: "/repo/kanvibe__worktrees/feature-pull-fail",
                sshHost: null,
                status: "failed",
                summary: "Not possible to fast-forward",
              },
            ],
            failures: [
              {
                operation: "pull-request-sync",
                target: "PR sync target (feature/pr-fail)",
                reason: "gh auth failed",
                taskId: "task-pr-fail",
                branchName: "feature/pr-fail",
              },
            ],
          },
        },
      }),
      onNotificationActivated: vi.fn(() => vi.fn()),
    } as unknown as NonNullable<typeof window.kanvibeDesktop>;

    render(<App />);

    await waitFor(() => {
      expect(screen.getByText("task detail route")).toBeTruthy();
    });
    expect(screen.queryByText("Background Sync Review")).toBeNull();
    expect(window.kanvibeDesktop.consumePendingNotificationActivation).not.toHaveBeenCalled();
    expect(window.location.hash).toBe("#/en/task/task-1");
  });

  it("shows a background sync review dialog when a notification is activated", async () => {
    window.location.hash = "#/en/task/task-1";
    let activateNotificationListener: ((notification: AppNotification) => void) | null = null;
    const notification: AppNotification = {
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
        type: "background-sync-review" as const,
        payload: {
          mergedPullRequests: [
            {
              taskId: "task-1",
              taskTitle: "Detail task",
              branchName: "feature/current-route",
              prUrl: "https://github.com/kanvibe/kanvibe/pull/410",
              mergedAt: "2026-05-01T01:00:00Z",
            },
          ],
          registeredWorktrees: [
            {
              taskId: "task-worktree",
              projectName: "kanvibe",
              branchName: "feature/new-worktree",
              worktreePath: "/repo/kanvibe__worktrees/feature-new-worktree",
              sshHost: null,
            },
          ],
          pulledTasks: [
            {
              taskId: "task-pull",
              taskTitle: "Pull target",
              branchName: "feature/pull-fail",
              worktreePath: "/repo/kanvibe__worktrees/feature-pull-fail",
              sshHost: null,
              status: "failed" as const,
              summary: "Not possible to fast-forward",
            },
          ],
          failures: [
            {
              operation: "pull-request-sync" as const,
              target: "PR sync target (feature/pr-fail)",
              reason: "gh auth failed",
              taskId: "task-pr-fail",
              branchName: "feature/pr-fail",
            },
          ],
        },
      },
    };

    window.kanvibeDesktop = {
      isDesktop: true,
      onBoardEvent: vi.fn(() => vi.fn()),
      consumePendingNotificationActivation: vi.fn(),
      onNotificationActivated: vi.fn((listener) => {
        activateNotificationListener = listener;
        return vi.fn();
      }),
    } as unknown as NonNullable<typeof window.kanvibeDesktop>;

    render(<App />);

    await waitFor(() => {
      expect(screen.getByText("task detail route")).toBeTruthy();
    });

    act(() => {
      activateNotificationListener?.(notification);
    });

    await waitFor(() => {
      expect(screen.getByText("Background Sync Review")).toBeTruthy();
    });
    expect(screen.getByText("https://github.com/kanvibe/kanvibe/pull/410")).toBeTruthy();
    expect(screen.getByText("/repo/kanvibe__worktrees/feature-new-worktree")).toBeTruthy();
    expect(screen.getByText("Pull results")).toBeTruthy();
    expect(screen.getByText("Pull target")).toBeTruthy();
    expect(screen.getByText("Not possible to fast-forward")).toBeTruthy();
    expect(screen.getByText("Sync failures")).toBeTruthy();
    expect(screen.getByText("PR sync target (feature/pr-fail)")).toBeTruthy();
    expect(screen.getByText("gh auth failed")).toBeTruthy();
    expect(window.kanvibeDesktop.consumePendingNotificationActivation).not.toHaveBeenCalled();
    expect(window.location.hash).toBe("#/en/task/task-1");

    fireEvent.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() => {
      expect(mocks.deleteTasks).toHaveBeenCalledWith(["task-1"]);
    });
    expect(mocks.updateTaskStatus).not.toHaveBeenCalled();
  });
});
