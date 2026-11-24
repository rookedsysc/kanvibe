import { beforeEach, describe, expect, it, vi } from "vitest";

const notificationInstances: Array<{ on: ReturnType<typeof vi.fn>; show: ReturnType<typeof vi.fn> }> = [];

const mocks = vi.hoisted(() => ({
  createNotification: vi.fn(),
  markNotificationRead: vi.fn(),
  getNotificationSettings: vi.fn(),
  isNotificationSupported: vi.fn(() => true),
}));

class MockNotification {
  static isSupported = mocks.isNotificationSupported;
  on = vi.fn();
  show = vi.fn();

  constructor(_options: unknown) {
    notificationInstances.push({ on: this.on, show: this.show });
  }
}

vi.mock("electron", () => ({
  Notification: MockNotification,
}));

vi.mock("@/desktop/main/notificationStore", () => ({
  createNotification: mocks.createNotification,
  markNotificationRead: mocks.markNotificationRead,
}));

vi.mock("@/desktop/main/services/appSettingsService", () => ({
  getNotificationSettings: mocks.getNotificationSettings,
}));

describe("desktopNotificationService", () => {
  it("stores sync results quietly in the inbox even when OS notifications are disabled", async () => {
    mocks.getNotificationSettings.mockResolvedValue({ isEnabled: false, enabledStatuses: [] });
    mocks.createNotification.mockResolvedValue({ created: true, notification: { id: "sync-result" } });
    const onNotificationsChanged = vi.fn();
    const { deliverBoardEventNotification } = await import("../desktopNotificationService");
    await expect(deliverBoardEventNotification({
      type: "background-sync-review-needed", mergedPullRequests: [], registeredWorktrees: [],
    }, "ko", { onNotificationsChanged })).resolves.toBe(true);
    expect(mocks.createNotification).toHaveBeenCalledOnce();
    expect(onNotificationsChanged).toHaveBeenCalledOnce();
    expect(notificationInstances).toHaveLength(0);
  });
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    notificationInstances.length = 0;
    mocks.isNotificationSupported.mockReturnValue(true);
  });

  it("SSH 연결 실패와 daemon 거부에 서로 다른 복구 방법을 표시한다", async () => {
    mocks.getNotificationSettings.mockResolvedValue({ isEnabled: true, enabledStatuses: [] });
    mocks.createNotification.mockImplementation(async (payload) => ({ created: true, notification: payload }));
    const { deliverBoardEventNotification } = await import("../desktopNotificationService");
    const options = { onNotificationsChanged: vi.fn() };

    await deliverBoardEventNotification({
      type: "hook-tunnel-failed", kind: "ssh", sshHost: "dev-vm", reason: "Operation timed out",
    }, "ko", options);
    await deliverBoardEventNotification({
      type: "hook-tunnel-failed", kind: "client-in-use", sshHost: "dev-vm",
      reason: "A desktop with this client ID is already connected",
    }, "ko", options);

    expect(mocks.createNotification).toHaveBeenNthCalledWith(1, expect.objectContaining({
      title: expect.stringContaining("SSH 연결 실패"),
      body: expect.stringMatching(/VPN.*\nOperation timed out/),
    }));
    expect(mocks.createNotification).toHaveBeenNthCalledWith(2, expect.objectContaining({
      title: expect.stringContaining("KanVibe 연결 거부"),
      body: expect.stringMatching(/기존 데스크톱.*\nA desktop with this client ID is already connected/),
    }));
    expect(notificationInstances).toHaveLength(2);
  });

  it("OS 알림이 꺼져 있어도 daemon 거부 사유를 앱 알림함에 보관한다", async () => {
    mocks.getNotificationSettings.mockResolvedValue({ isEnabled: false, enabledStatuses: [] });
    mocks.createNotification.mockResolvedValue({ created: true, notification: { id: "tunnel-failure" } });
    const { deliverBoardEventNotification } = await import("../desktopNotificationService");
    const onNotificationsChanged = vi.fn();

    await deliverBoardEventNotification({
      type: "hook-tunnel-failed", kind: "handshake", sshHost: "dev-vm", reason: "Host hello buildHash mismatch",
    }, "en", { onNotificationsChanged });

    expect(mocks.createNotification).toHaveBeenCalledWith(expect.objectContaining({
      title: "dev-vm — KanVibe handshake failed",
      body: expect.stringContaining("Host hello buildHash mismatch"),
    }));
    expect(onNotificationsChanged).toHaveBeenCalledOnce();
    expect(notificationInstances).toHaveLength(0);
  });

  it("응답 없는 연결을 교체하면 복구 사유를 앱 알림함에 남긴다", async () => {
    mocks.getNotificationSettings.mockResolvedValue({ isEnabled: true, enabledStatuses: [] });
    mocks.createNotification.mockResolvedValue({ created: true, notification: { id: "tunnel-recovered" } });
    const { deliverBoardEventNotification } = await import("../desktopNotificationService");
    const onNotificationsChanged = vi.fn();

    await deliverBoardEventNotification({
      type: "hook-tunnel-recovered", sshHost: "dev-vm",
    }, "ko", { onNotificationsChanged });

    expect(mocks.createNotification).toHaveBeenCalledWith(expect.objectContaining({
      title: "dev-vm — Hook 터널 복구",
      body: expect.stringContaining("응답이 없어"),
    }));
    expect(onNotificationsChanged).toHaveBeenCalledOnce();
    expect(notificationInstances).toHaveLength(0);
  });

  it("이미 생성된 알림은 처리 완료로 간주해 브라우저 fallback을 막는다", async () => {
    mocks.createNotification.mockResolvedValue({
      created: false,
      notification: {
        id: "notification-1",
        title: "title",
        body: "body",
        taskId: "task-1",
        relativePath: "/ko/task/task-1",
        locale: "ko",
        isRead: false,
        createdAt: "2026-04-19T00:00:00.000Z",
        dedupeKey: "task-status:task-1:review",
      },
    });

    const { deliverDesktopNotification } = await import("@/desktop/main/services/desktopNotificationService");

    await expect(deliverDesktopNotification({
      title: "title",
      body: "body",
      taskId: "task-1",
      locale: "ko",
      relativePath: "/ko/task/task-1",
      dedupeKey: "task-status:task-1:review",
    }, {
      iconPath: "/icon.png",
    })).resolves.toBe(true);

    expect(notificationInstances).toHaveLength(0);
  });

  it("새 알림은 Electron Notification으로 표시한다", async () => {
    mocks.createNotification.mockResolvedValue({
      created: true,
      notification: {
        id: "notification-1",
        title: "title",
        body: "body",
        taskId: "task-1",
        relativePath: "/ko/task/task-1",
        locale: "ko",
        isRead: false,
        createdAt: "2026-04-19T00:00:00.000Z",
        dedupeKey: "task-status:task-1:review",
      },
    });

    const onNotificationsChanged = vi.fn();
    const { deliverDesktopNotification } = await import("@/desktop/main/services/desktopNotificationService");

    await expect(deliverDesktopNotification({
      title: "title",
      body: "body",
      taskId: "task-1",
      locale: "ko",
      relativePath: "/ko/task/task-1",
      dedupeKey: "task-status:task-1:review",
    }, {
      iconPath: "/icon.png",
      onNotificationsChanged,
    })).resolves.toBe(true);

    expect(onNotificationsChanged).toHaveBeenCalledTimes(1);
    expect(notificationInstances).toHaveLength(1);
    expect(notificationInstances[0]?.show).toHaveBeenCalledTimes(1);
  });

  it("허용된 상태의 board event만 데스크톱 알림으로 전달한다", async () => {
    mocks.getNotificationSettings.mockResolvedValue({
      isEnabled: true,
      enabledStatuses: ["review"],
    });
    mocks.createNotification.mockResolvedValue({
      created: true,
      notification: {
        id: "notification-1",
        title: "kanvibe — feat/login",
        body: "로그인 구현: review로 변경",
        taskId: "task-1",
        relativePath: "/ko/task/task-1",
        locale: "ko",
        isRead: false,
        createdAt: "2026-04-19T00:00:00.000Z",
        dedupeKey: "task-status:task-1:review",
      },
    });

    const { deliverBoardEventNotification } = await import("@/desktop/main/services/desktopNotificationService");

    await expect(deliverBoardEventNotification({
      type: "task-status-changed",
      projectName: "kanvibe",
      branchName: "feat/login",
      taskTitle: "로그인 구현",
      description: null,
      newStatus: "review",
      taskId: "task-1",
    }, "ko", {
      iconPath: "/icon.png",
    })).resolves.toBe(true);

    expect(mocks.createNotification).toHaveBeenCalledWith(expect.objectContaining({
      title: "kanvibe — feat/login",
      dedupeKey: "task-status:task-1:review",
    }));
  });

  it("비활성화된 상태의 board event는 무시한다", async () => {
    mocks.getNotificationSettings.mockResolvedValue({
      isEnabled: true,
      enabledStatuses: ["progress"],
    });

    const { deliverBoardEventNotification } = await import("@/desktop/main/services/desktopNotificationService");

    await expect(deliverBoardEventNotification({
      type: "task-status-changed",
      projectName: "kanvibe",
      branchName: "feat/login",
      taskTitle: "로그인 구현",
      description: null,
      newStatus: "review",
      taskId: "task-1",
    }, "ko", {
      iconPath: "/icon.png",
    })).resolves.toBe(false);

    expect(mocks.createNotification).not.toHaveBeenCalled();
  });

  it("background sync review event는 review action metadata와 함께 알림을 생성한다", async () => {
    mocks.getNotificationSettings.mockResolvedValue({
      isEnabled: true,
      enabledStatuses: ["progress"],
    });
    mocks.createNotification.mockResolvedValue({
      created: true,
      notification: {
        id: "notification-review-1",
        title: "Background sync review",
        body: "1 merged PR\n1 registered worktree",
        taskId: null,
        relativePath: "/ko",
        locale: "ko",
        isRead: false,
        createdAt: "2026-05-01T00:00:00.000Z",
        dedupeKey: "background-sync-review:test",
        action: {
          type: "background-sync-review",
          payload: {
            mergedPullRequests: [],
            registeredWorktrees: [],
            pulledTasks: [],
          },
        },
      },
    });

    const { deliverBoardEventNotification } = await import("@/desktop/main/services/desktopNotificationService");

    await expect(deliverBoardEventNotification({
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
      pulledTasks: [],
    }, "ko", {
      iconPath: "/icon.png",
    })).resolves.toBe(true);

    expect(mocks.createNotification).toHaveBeenCalledWith(expect.objectContaining({
      locale: "ko",
      relativePath: "/ko",
      action: {
        type: "background-sync-review",
        payload: {
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
          pulledTasks: [],
        },
      },
    }));
  });
});
