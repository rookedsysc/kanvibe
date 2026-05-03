import type { AppNotification, DesktopNotificationPayload } from "@/desktop/shared/notifications";

interface KanvibeDesktopApi {
  isDesktop: boolean;
  showNotification?: (payload: DesktopNotificationPayload) => Promise<boolean>;
  listNotifications?: () => Promise<AppNotification[]>;
  markNotificationRead?: (notificationId: string) => Promise<AppNotification | null>;
  markAllNotificationsRead?: () => Promise<void>;
  onNotificationsChanged?: (listener: () => void) => () => void;
  activateNotification?: (notificationId: string) => Promise<boolean>;
  consumePendingNotificationActivation?: () => Promise<AppNotification | null>;
  onNotificationActivated?: (listener: (notification: AppNotification) => void) => () => void;
  onNotificationShortcut?: (listener: () => void) => () => void;
  onCreateTaskShortcut?: (listener: () => void) => () => void;
  onRefreshShortcut?: (listener: () => void) => () => void;
  [key: string]: unknown;
}

interface Window {
  kanvibeDesktop?: KanvibeDesktopApi;
}
