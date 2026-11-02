import { randomUUID } from "node:crypto";
import type { AppNotification, DesktopNotificationPayload } from "@/desktop/shared/notifications";
import { getAppSetting, setAppSetting } from "@/desktop/main/services/appSettingsService";

const APP_NOTIFICATIONS_KEY = "app_notifications";
const MAX_NOTIFICATIONS = 100;
const DEDUPE_WINDOW_MS = 4000;
const recentNotificationKeys = new Map<string, number>();
const notificationChangeListeners = new Set<() => void>();
let notificationWriteQueue: Promise<unknown> = Promise.resolve();

export function subscribeToNotificationChanges(listener: () => void): () => void {
  notificationChangeListeners.add(listener);
  return () => { notificationChangeListeners.delete(listener); };
}

function withNotifications<T>(mutate: (notifications: AppNotification[]) => Promise<T>): Promise<T> {
  // 알림 전체가 설정 한 칸을 공유하므로 생성과 모든 읽음 처리가 같은 읽기-쓰기 순서를 지켜야 한다.
  const next = notificationWriteQueue.then(async () => mutate(await readNotifications()));
  // 실패한 호출은 호출자에게 돌려주되 뒤따르는 변경까지 막지 않는다.
  notificationWriteQueue = next.catch(() => {});
  return next;
}

function getRelativePath(payload: DesktopNotificationPayload) {
  if (payload.taskId) {
    return `/${payload.locale}/task/${payload.taskId}`;
  }

  if (payload.relativePath) {
    return payload.relativePath;
  }

  return `/${payload.locale}`;
}

function pruneRecentKeys(now: number) {
  for (const [key, timestamp] of recentNotificationKeys.entries()) {
    if (now - timestamp > DEDUPE_WINDOW_MS) {
      recentNotificationKeys.delete(key);
    }
  }
}

async function readNotifications(): Promise<AppNotification[]> {
  const value = await getAppSetting(APP_NOTIFICATIONS_KEY);
  if (!value) {
    return [];
  }

  try {
    const parsed = JSON.parse(value);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

async function writeNotifications(notifications: AppNotification[]) {
  await setAppSetting(APP_NOTIFICATIONS_KEY, JSON.stringify(notifications.slice(0, MAX_NOTIFICATIONS)));
  for (const listener of notificationChangeListeners) {
    try {
      listener();
    } catch {
      // 저장은 완료됐으므로 한 화면의 전달 실패가 다른 구독자나 쓰기 결과를 막아서는 안 된다.
      console.error("[kanvibe] Notification change delivery failed");
    }
  }
}

export async function listNotifications(): Promise<AppNotification[]> {
  const notifications = await readNotifications();
  return notifications.sort((a, b) => b.createdAt.localeCompare(a.createdAt));
}

export async function getNotificationById(id: string): Promise<AppNotification | null> {
  const notifications = await readNotifications();
  return notifications.find((notification) => notification.id === id) ?? null;
}

export async function createNotification(payload: DesktopNotificationPayload): Promise<{ created: boolean; notification: AppNotification }> {
  return withNotifications(async (notifications) => {
    const now = Date.now();
    const dedupeKey = payload.dedupeKey || [payload.title, payload.body, payload.taskId || "", payload.locale].join("::");
    pruneRecentKeys(now);

    if (recentNotificationKeys.has(dedupeKey)) {
      const existing = notifications.find((notification) => notification.dedupeKey === dedupeKey);
      if (existing) {
        return { created: false, notification: existing };
      }
    }

    const notification: AppNotification = {
      id: randomUUID(),
      title: payload.title,
      body: payload.body,
      taskId: payload.taskId || null,
      relativePath: getRelativePath(payload),
      locale: payload.locale,
      isRead: false,
      createdAt: new Date(now).toISOString(),
      dedupeKey,
      action: payload.action ?? null,
    };

    await writeNotifications([notification, ...notifications]);
    recentNotificationKeys.set(dedupeKey, now);
    return { created: true, notification };
  });
}

export async function markNotificationRead(id: string): Promise<AppNotification | null> {
  return withNotifications(async (notifications) => {
    const notification = notifications.find((entry) => entry.id === id);
    if (!notification || notification.isRead) {
      return notification ?? null;
    }

    const updatedNotification = { ...notification, isRead: true };
    await writeNotifications(notifications.map((entry) => entry.id === id ? updatedNotification : entry));
    return updatedNotification;
  });
}

/** 특정 task에 연결된 미읽음 알림을 모두 읽음 처리하고, 실제로 바뀐 알림 수를 돌려준다 */
export async function markTaskNotificationsRead(taskId: string): Promise<number> {
  return withNotifications(async (notifications) => {
    let updatedCount = 0;
    const nextNotifications = notifications.map((notification) => {
      if (notification.taskId !== taskId || notification.isRead) {
        return notification;
      }

      updatedCount += 1;
      return { ...notification, isRead: true };
    });

    if (updatedCount === 0) {
      return 0;
    }

    await writeNotifications(nextNotifications);
    return updatedCount;
  });
}

export async function markAllNotificationsRead(): Promise<void> {
  await withNotifications(async (notifications) => {
    if (notifications.every((notification) => notification.isRead)) {
      return;
    }
    await writeNotifications(notifications.map((notification) => ({ ...notification, isRead: true })));
  });
}
