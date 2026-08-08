import { randomUUID } from "node:crypto";
import type { AppNotification, DesktopNotificationPayload } from "@/desktop/shared/notifications";
import { getAppSetting, setAppSetting } from "@/desktop/main/services/appSettingsService";

const APP_NOTIFICATIONS_KEY = "app_notifications";
const MAX_NOTIFICATIONS = 100;
const DEDUPE_WINDOW_MS = 4000;
const recentNotificationKeys = new Map<string, number>();

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
  const now = Date.now();
  const dedupeKey = payload.dedupeKey || [payload.title, payload.body, payload.taskId || "", payload.locale].join("::");
  pruneRecentKeys(now);

  if (recentNotificationKeys.has(dedupeKey)) {
    const notifications = await readNotifications();
    const existing = notifications.find((notification) => notification.dedupeKey === dedupeKey);
    if (existing) {
      return { created: false, notification: existing };
    }
  }

  recentNotificationKeys.set(dedupeKey, now);
  const notifications = await readNotifications();
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
  return { created: true, notification };
}

export async function markNotificationRead(id: string): Promise<AppNotification | null> {
  const notifications = await readNotifications();
  let updatedNotification: AppNotification | null = null;
  const nextNotifications = notifications.map((notification) => {
    if (notification.id !== id) {
      return notification;
    }

    updatedNotification = { ...notification, isRead: true };
    return updatedNotification;
  });

  await writeNotifications(nextNotifications);
  return updatedNotification;
}

/** 특정 task에 연결된 미읽음 알림을 모두 읽음 처리하고, 실제로 바뀐 알림 수를 돌려준다 */
export async function markTaskNotificationsRead(taskId: string): Promise<number> {
  const notifications = await readNotifications();
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
}

export async function markAllNotificationsRead(): Promise<void> {
  const notifications = await readNotifications();
  await writeNotifications(notifications.map((notification) => ({ ...notification, isRead: true })));
}
