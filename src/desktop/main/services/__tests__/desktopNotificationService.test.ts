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
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    notificationInstances.length = 0;
    mocks.isNotificationSupported.mockReturnValue(true);
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
});
