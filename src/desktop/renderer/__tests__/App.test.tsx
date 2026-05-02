import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "@/desktop/renderer/App";
import { INITIAL_DESKTOP_LOAD_TIMEOUT_MS } from "@/desktop/renderer/utils/loadingTimeout";

const mocks = vi.hoisted(() => ({
  getSessionState: vi.fn(),
  updateTaskStatus: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/auth", () => ({
  getSessionState: (...args: unknown[]) => mocks.getSessionState(...args),
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  updateTaskStatus: (...args: unknown[]) => mocks.updateTaskStatus(...args),
}));

vi.mock("@/components/LoginForm", () => ({
  default: () => <div>login form</div>,
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

vi.mock("@/desktop/renderer/components/BoardEventAlert", () => ({
  default: () => <div>board event alert</div>,
}));

describe("App", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.location.hash = "#/ko/login";
    window.kanvibeDesktop = {
      isDesktop: true,
      onBoardEvent: vi.fn(() => vi.fn()),
    } as unknown as NonNullable<typeof window.kanvibeDesktop>;
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("session lookup failure does not keep the app on the loading screen", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.getSessionState.mockRejectedValue(new Error("ipc failed"));

    try {
      render(<App />);

      await waitFor(() => {
        expect(screen.queryByText("Loading...")).toBeNull();
      });
      expect(screen.getByText("login form")).toBeTruthy();
    } finally {
      consoleError.mockRestore();
    }
  });

  it("should not stay on the loading screen when session lookup never settles", async () => {
    // Given
    vi.useFakeTimers();
    mocks.getSessionState.mockReturnValue(new Promise(() => {}));

    // When
    render(<App />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(INITIAL_DESKTOP_LOAD_TIMEOUT_MS);
    });

    // Then
    expect(screen.queryByText("Loading...")).toBeNull();
    expect(screen.getByText("login form")).toBeTruthy();
  });

  it("shows a background sync review dialog on the current detail route", async () => {
    window.location.hash = "#/en/task/task-1";
    mocks.getSessionState.mockResolvedValue({ isAuthenticated: true });
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
    expect(window.location.hash).toBe("#/en/task/task-1");
  });
});
