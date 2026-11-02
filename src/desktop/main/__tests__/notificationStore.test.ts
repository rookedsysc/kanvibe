import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AppNotification } from "@/desktop/shared/notifications";

const mocks = vi.hoisted(() => ({
  getAppSetting: vi.fn(),
  setAppSetting: vi.fn(),
}));

vi.mock("@/desktop/main/services/appSettingsService", () => ({
  getAppSetting: (...args: unknown[]) => mocks.getAppSetting(...args),
  setAppSetting: (...args: unknown[]) => mocks.setAppSetting(...args),
}));

const notificationStore = await import("@/desktop/main/notificationStore");
const { createNotification: createStoredNotification, listNotifications, markNotificationRead, markTaskNotificationsRead, markAllNotificationsRead } = notificationStore;

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

describe("알림 저장소 동시 변경과 구독", () => {
  let storedNotifications: string | null;

  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-11-01T00:00:00.000Z"));
    storedNotifications = JSON.stringify([createNotification()]);
    mocks.getAppSetting.mockImplementation(async () => storedNotifications);
    mocks.setAppSetting.mockImplementation(async (_key: string, value: string) => {
      storedNotifications = value;
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it.each([
    ["하나 읽음", () => markNotificationRead("n1"), false],
    ["태스크 읽음", () => markTaskNotificationsRead("task-1"), false],
    ["모두 읽음", () => markAllNotificationsRead(), true],
  ] as const)("새 알림 생성과 %s 처리가 겹쳐도 두 변경을 모두 보존한다", async (label, markRead, isNewRead) => {
    const [created] = await Promise.all([
      createStoredNotification({ title: label, body: "새 이벤트", taskId: "task-2", locale: "ko" }),
      markRead(),
    ]);

    expect(await listNotifications()).toEqual([
      { ...created.notification, isRead: isNewRead },
      createNotification({ isRead: true }),
    ]);
  });

  it("동일한 알림이 동시에 들어오면 하나만 저장한다", async () => {
    const payload = { title: "동시 중복 알림", body: "같은 이벤트", locale: "ko", dedupeKey: "concurrent-event" };
    const [first, duplicate] = await Promise.all([
      createStoredNotification(payload),
      createStoredNotification(payload),
    ]);

    expect(first.created).toBe(true);
    expect(duplicate).toEqual({ created: false, notification: first.notification });
    expect(await listNotifications()).toEqual([first.notification, createNotification()]);
  });

  it("새 알림을 넣으면 최신 알림을 먼저 반환하고 기존 100개 보관 범위를 지킨다", async () => {
    storedNotifications = JSON.stringify(Array.from({ length: 100 }, (_value, index) => createNotification({
      id: `retained-${index}`, createdAt: new Date(Date.UTC(2026, 0, 1) - index).toISOString(),
    })));

    const { notification } = await createStoredNotification({ title: "보관 상한", body: "새 이벤트", locale: "ko" });
    const saved = await listNotifications();

    expect(saved).toHaveLength(100);
    expect(saved[0]).toEqual(notification);
    expect(saved.at(-1)?.id).toBe("retained-98");
    expect(saved.some((entry) => entry.id === "retained-99")).toBe(false);
  });

  it("성공적으로 바뀐 저장 상태만 알리고 중복·없는 대상·반복 읽음은 알리지 않는다", async () => {
    const changes: AppNotification[][] = [];
    const stop = notificationStore.subscribeToNotificationChanges(() => {
      changes.push(JSON.parse(storedNotifications ?? "[]"));
    });
    try {
      const payload = { title: "변경 이벤트", body: "본문", locale: "ko", dedupeKey: "change-event" };
      const { notification } = await createStoredNotification(payload);
      await createStoredNotification(payload);
      expect(await markNotificationRead("missing")).toBeNull();
      await markNotificationRead(notification.id);
      await markNotificationRead(notification.id);
      await markAllNotificationsRead();
      await markAllNotificationsRead();

      expect(changes.map((entries) => entries.map(({ id, isRead }) => ({ id, isRead })))).toEqual([
        [{ id: notification.id, isRead: false }, { id: "n1", isRead: false }],
        [{ id: notification.id, isRead: true }, { id: "n1", isRead: false }],
        [{ id: notification.id, isRead: true }, { id: "n1", isRead: true }],
      ]);
      stop();
      await createStoredNotification({ title: "구독 해제 후", body: "본문", locale: "ko" });
      expect(changes).toHaveLength(3);
    } finally {
      stop();
    }
  });

  it("태스크 읽음 변경도 저장이 끝난 뒤 구독자에게 알린다", async () => {
    const observed: AppNotification[][] = [];
    const stop = notificationStore.subscribeToNotificationChanges(() => {
      observed.push(JSON.parse(storedNotifications ?? "[]"));
    });
    try {
      await markTaskNotificationsRead("task-1");
      await markTaskNotificationsRead("task-1");

      expect(observed).toEqual([[createNotification({ isRead: true })]]);
    } finally {
      stop();
    }
  });

  it("저장이 실패하면 알리지 않고 다음 쓰기는 계속 처리한다", async () => {
    const changed = vi.fn();
    const stop = notificationStore.subscribeToNotificationChanges(changed);
    mocks.setAppSetting.mockRejectedValueOnce(new Error("settings unavailable"));
    try {
      await expect(markNotificationRead("n1")).rejects.toThrow("settings unavailable");
      expect(changed).not.toHaveBeenCalled();
      expect(await listNotifications()).toEqual([createNotification()]);

      await markNotificationRead("n1");

      expect(await listNotifications()).toEqual([createNotification({ isRead: true })]);
      expect(changed).toHaveBeenCalledTimes(1);
    } finally {
      stop();
    }
  });

  it("한 구독자가 실패해도 저장 결과와 다른 구독자 전달은 유지한다", async () => {
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
    const stopFailing = notificationStore.subscribeToNotificationChanges(() => { throw new Error("renderer gone"); });
    const changed = vi.fn();
    const stop = notificationStore.subscribeToNotificationChanges(changed);
    try {
      await expect(markNotificationRead("n1")).resolves.toEqual(createNotification({ isRead: true }));
      expect(changed).toHaveBeenCalledTimes(1);
      expect(await listNotifications()).toEqual([createNotification({ isRead: true })]);
    } finally {
      stopFailing();
      stop();
      errorLog.mockRestore();
    }
  });
});
