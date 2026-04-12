import type { AppNotification, DesktopNotificationPayload } from "@/desktop/shared/notifications";

interface KanvibeDesktopApi {
  isDesktop: boolean;
  showNotification?: (payload: DesktopNotificationPayload) => Promise<boolean>;
  listNotifications?: () => Promise<AppNotification[]>;
  markNotificationRead?: (notificationId: string) => Promise<AppNotification | null>;
  markAllNotificationsRead?: () => Promise<void>;
  onNotificationsChanged?: (listener: () => void) => () => void;
  [key: string]: any;
}

interface Window {
  kanvibeDesktop?: KanvibeDesktopApi;
}
