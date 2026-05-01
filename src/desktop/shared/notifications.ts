import type { BackgroundSyncReviewPayload } from "@/lib/boardNotifier";

export interface BackgroundSyncReviewNotificationAction {
  type: "background-sync-review";
  payload: BackgroundSyncReviewPayload;
}

export type AppNotificationAction = BackgroundSyncReviewNotificationAction;

export interface AppNotification {
  id: string;
  title: string;
  body: string;
  taskId: string | null;
  relativePath: string;
  locale: string;
  isRead: boolean;
  createdAt: string;
  dedupeKey: string;
  action?: AppNotificationAction | null;
}

export interface DesktopNotificationPayload {
  title: string;
  body: string;
  taskId?: string;
  locale: string;
  relativePath?: string;
  dedupeKey?: string;
  action?: AppNotificationAction;
}
