import type { AppNotification, DesktopNotificationPayload } from "@/desktop/shared/notifications";

interface KanvibeDesktopApi {
  isDesktop: boolean;
  showNotification?: (payload: DesktopNotificationPayload) => Promise<boolean>;
  listNotifications?: () => Promise<AppNotification[]>;
  markNotificationRead?: (notificationId: string) => Promise<AppNotification | null>;
  markTaskNotificationsRead?: (taskId: string) => Promise<number>;
  markAllNotificationsRead?: () => Promise<void>;
  onNotificationsChanged?: (listener: () => void) => () => void;
  activateNotification?: (notificationId: string) => Promise<boolean>;
  consumePendingNotificationActivation?: () => Promise<AppNotification | null>;
  onNotificationActivated?: (listener: (notification: AppNotification) => void) => () => void;
  onNotificationShortcut?: (listener: () => void) => () => void;
  onCreateTaskShortcut?: (listener: () => void) => () => void;
  onCommandPaletteShortcut?: (listener: () => void) => () => void;
  onTaskDetailDockShortcut?: (listener: (shortcutIndex: number) => void) => () => void;
  onTaskDetailUsageShortcut?: (listener: () => void) => () => void;
  notifyShortcutBindingsChanged?: () => void;
  onShortcutBindingsChanged?: (listener: () => void) => () => void;
  notifyShortcutCaptureChanged?: (isCapturing: boolean) => void;
  focusExistingInternalRoute?: (route: string) => Promise<boolean>;
  [key: string]: unknown;
}

interface Window {
  kanvibeDesktop?: KanvibeDesktopApi;
}
