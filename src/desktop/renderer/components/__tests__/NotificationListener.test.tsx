import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, waitFor } from "@testing-library/react";
import NotificationListener from "@/desktop/renderer/components/NotificationListener";

const mocks = vi.hoisted(() => ({
  notifyTaskStatusChanged: vi.fn(),
  notifyHookStatusTargetMissing: vi.fn(),
  notifyBackgroundSyncReview: vi.fn(),
  getNotificationSettings: vi.fn(),
  useRefreshSignal: vi.fn(() => 0),
}));

let boardEventListener: ((event: any) => void) | null = null;

vi.mock("next-intl", () => ({
  useLocale: () => "ko",
}));

vi.mock("@/hooks/useTaskNotification", () => ({
  useTaskNotification: () => ({
    notifyTaskStatusChanged: mocks.notifyTaskStatusChanged,
    notifyHookStatusTargetMissing: mocks.notifyHookStatusTargetMissing,
    notifyBackgroundSyncReview: mocks.notifyBackgroundSyncReview,
  }),
}));

vi.mock("@/desktop/renderer/actions/appSettings", () => ({
  getNotificationSettings: mocks.getNotificationSettings,
}));

vi.mock("@/desktop/renderer/utils/refresh", () => ({
  useRefreshSignal: mocks.useRefreshSignal,
}));

describe("NotificationListener", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    boardEventListener = null;
    mocks.getNotificationSettings.mockResolvedValue({
      isEnabled: true,
      enabledStatuses: ["progress", "pending", "review"],
    });

    window.kanvibeDesktop = {
      onBoardEvent: vi.fn((listener) => {
        boardEventListener = listener;
        return () => {};
      }),
    } as any;
  });

  it("상태 변경 이벤트가 오면 데스크톱 알림 훅을 호출한다", async () => {
    // Given
    render(<NotificationListener />);
    await waitFor(() => {
      expect((window.kanvibeDesktop?.onBoardEvent as any).mock.calls.length).toBeGreaterThan(1);
    });
    mocks.notifyTaskStatusChanged.mockClear();

    // When
    await act(async () => {
      boardEventListener?.({
        type: "task-status-changed",
        projectName: "kanvibe",
        branchName: "feat/login",
        taskTitle: "로그인",
        description: null,
        newStatus: "review",
        taskId: "task-1",
      });
    });

    // Then
    expect(mocks.notifyTaskStatusChanged).toHaveBeenCalledWith({
      type: "task-status-changed",
      projectName: "kanvibe",
      branchName: "feat/login",
      taskTitle: "로그인",
      description: null,
      newStatus: "review",
      taskId: "task-1",
      locale: "ko",
    });
  });

  it("허용되지 않은 상태는 알림으로 전달하지 않는다", async () => {
    // Given
    mocks.getNotificationSettings.mockResolvedValue({
      isEnabled: true,
      enabledStatuses: ["progress"],
    });
    render(<NotificationListener />);
    await waitFor(() => {
      expect((window.kanvibeDesktop?.onBoardEvent as any).mock.calls.length).toBeGreaterThan(1);
    });
    mocks.notifyTaskStatusChanged.mockClear();

    // When
    await act(async () => {
      boardEventListener?.({
        type: "task-status-changed",
        projectName: "kanvibe",
        branchName: "feat/login",
        taskTitle: "로그인",
        description: null,
        newStatus: "review",
        taskId: "task-1",
      });
    });

    // Then
    expect(mocks.notifyTaskStatusChanged).not.toHaveBeenCalled();
  });

  it("background sync review 이벤트가 오면 review 알림 훅을 호출한다", async () => {
    render(<NotificationListener />);
    await waitFor(() => {
      expect((window.kanvibeDesktop?.onBoardEvent as any).mock.calls.length).toBeGreaterThan(1);
    });
    mocks.notifyBackgroundSyncReview.mockClear();

    await act(async () => {
      boardEventListener?.({
        type: "background-sync-review-needed",
        mergedPullRequests: [
          {
            taskId: "task-10",
            taskTitle: "Merged PR task",
            branchName: "feature/merged-pr",
            prUrl: "https://github.com/kanvibe/kanvibe/pull/211",
            mergedAt: "2026-04-30T02:00:00Z",
          },
        ],
        registeredWorktrees: [
          {
            taskId: "task-worktree",
            projectName: "api",
            branchName: "feature-sync",
            worktreePath: "/workspace/api__worktrees/feature-sync",
            sshHost: null,
          },
        ],
      });
    });

    expect(mocks.notifyBackgroundSyncReview).toHaveBeenCalledWith({
      type: "background-sync-review-needed",
      mergedPullRequests: [
        {
          taskId: "task-10",
          taskTitle: "Merged PR task",
          branchName: "feature/merged-pr",
          prUrl: "https://github.com/kanvibe/kanvibe/pull/211",
          mergedAt: "2026-04-30T02:00:00Z",
        },
      ],
      registeredWorktrees: [
        {
          taskId: "task-worktree",
          projectName: "api",
          branchName: "feature-sync",
          worktreePath: "/workspace/api__worktrees/feature-sync",
          sshHost: null,
        },
      ],
      locale: "ko",
    });
  });
});
