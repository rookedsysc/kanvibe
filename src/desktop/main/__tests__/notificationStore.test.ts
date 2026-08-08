import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppNotification } from "@/desktop/shared/notifications";

const mocks = vi.hoisted(() => ({
  getAppSetting: vi.fn(),
  setAppSetting: vi.fn(),
}));

vi.mock("@/desktop/main/services/appSettingsService", () => ({
  getAppSetting: (...args: unknown[]) => mocks.getAppSetting(...args),
  setAppSetting: (...args: unknown[]) => mocks.setAppSetting(...args),
}));

const { markTaskNotificationsRead } = await import("@/desktop/main/notificationStore");

function createNotification(overrides: Partial<AppNotification> = {}): AppNotification {
  return {
    id: "n1",
    title: "title",
    body: "body",
    taskId: "task-1",
    relativePath: "/ko/task/task-1",
    locale: "ko",
    isRead: false,
    createdAt: "2026-08-08T00:00:00.000Z",
    dedupeKey: "k1",
    action: null,
    ...overrides,
  };
}

function readWrittenNotifications(): AppNotification[] {
  const [, serialized] = mocks.setAppSetting.mock.calls.at(-1) as [string, string];
  return JSON.parse(serialized);
}

describe("markTaskNotificationsRead", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.setAppSetting.mockResolvedValue(undefined);
  });

  it("같은 task의 미읽음 알림만 읽음 처리하고 다른 task 알림은 그대로 둔다", async () => {
    mocks.getAppSetting.mockResolvedValue(JSON.stringify([
      createNotification({ id: "n1", taskId: "task-1", isRead: false }),
      createNotification({ id: "n2", taskId: "task-1", isRead: false }),
      createNotification({ id: "n3", taskId: "task-2", isRead: false }),
    ]));

    const updatedCount = await markTaskNotificationsRead("task-1");

    expect(updatedCount).toBe(2);
    expect(readWrittenNotifications().map((notification) => ({
      id: notification.id,
      isRead: notification.isRead,
    }))).toEqual([
      { id: "n1", isRead: true },
      { id: "n2", isRead: true },
      { id: "n3", isRead: false },
    ]);
  });

  it("이미 모두 읽은 task는 저장소에 다시 쓰지 않는다", async () => {
    mocks.getAppSetting.mockResolvedValue(JSON.stringify([
      createNotification({ id: "n1", taskId: "task-1", isRead: true }),
      createNotification({ id: "n2", taskId: "task-2", isRead: false }),
    ]));

    const updatedCount = await markTaskNotificationsRead("task-1");

    expect(updatedCount).toBe(0);
    expect(mocks.setAppSetting).not.toHaveBeenCalled();
  });

  it("taskId가 없는 알림은 읽음 처리 대상에서 제외한다", async () => {
    mocks.getAppSetting.mockResolvedValue(JSON.stringify([
      createNotification({ id: "n1", taskId: null, isRead: false }),
    ]));

    const updatedCount = await markTaskNotificationsRead("task-1");

    expect(updatedCount).toBe(0);
    expect(mocks.setAppSetting).not.toHaveBeenCalled();
  });
});
