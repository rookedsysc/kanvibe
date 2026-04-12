import type { AppNotification } from "@/desktop/shared/notifications";

export function listNotifications(): Promise<AppNotification[]> {
  return window.kanvibeDesktop.listNotifications?.() ?? Promise.resolve([]);
}

export function markNotificationRead(notificationId: string): Promise<AppNotification | null> {
  return window.kanvibeDesktop.markNotificationRead?.(notificationId) ?? Promise.resolve(null);
}

export function markAllNotificationsRead(): Promise<void> {
  return window.kanvibeDesktop.markAllNotificationsRead?.() ?? Promise.resolve();
}
