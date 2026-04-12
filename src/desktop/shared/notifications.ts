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
}

export interface DesktopNotificationPayload {
  title: string;
  body: string;
  taskId?: string;
  locale: string;
  relativePath?: string;
  dedupeKey?: string;
}
