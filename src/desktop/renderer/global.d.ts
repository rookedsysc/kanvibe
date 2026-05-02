import type { DesktopServiceNamespace } from "@/desktop/main/serviceRegistry";
import type { BoardEventPayload } from "@/lib/boardNotifier";
import type { AppNotification, DesktopNotificationPayload } from "@/desktop/shared/notifications";

declare global {
  interface Window {
    find?: (
      query: string,
      caseSensitive?: boolean,
      backwards?: boolean,
      wrapAround?: boolean,
      wholeWord?: boolean,
      searchInFrames?: boolean,
      showDialog?: boolean,
    ) => boolean;
    kanvibeDesktop: {
      isDesktop: boolean;
      logRendererError?: (event: string, payload?: Record<string, unknown>) => void;
      invoke: (namespace: DesktopServiceNamespace, method: string, args: unknown[]) => Promise<unknown>;
      onBoardEvent: (listener: (event: BoardEventPayload) => void) => () => void;
      openTerminal: (taskId: string, cols: number, rows: number) => Promise<{ ok: boolean; error?: string }>;
      writeTerminal: (taskId: string, data: string) => void;
      resizeTerminal: (taskId: string, cols: number, rows: number) => void;
      focusTerminal: (taskId: string) => void;
      closeTerminal: (taskId: string) => void;
      onTerminalData: (listener: (event: { taskId: string; data: string }) => void) => () => void;
      onTerminalClose: (listener: (event: { taskId: string; reason: string | null }) => void) => () => void;
      showNotification?: (payload: DesktopNotificationPayload) => Promise<boolean>;
      listNotifications?: () => Promise<AppNotification[]>;
      markNotificationRead?: (notificationId: string) => Promise<AppNotification | null>;
      markAllNotificationsRead?: () => Promise<void>;
      onNotificationsChanged?: (listener: () => void) => () => void;
      activateNotification?: (notificationId: string) => Promise<boolean>;
      consumePendingNotificationActivation?: () => Promise<AppNotification | null>;
      onNotificationActivated?: (listener: (notification: AppNotification) => void) => () => void;
      onNotificationShortcut?: (listener: () => void) => () => void;
      onRefreshShortcut?: (listener: () => void) => () => void;
    };
  }
}

export {};
