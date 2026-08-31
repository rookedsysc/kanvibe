import type { DesktopServiceNamespace } from "@/desktop/main/serviceRegistry";
import type { BoardEventPayload } from "@/lib/boardNotifier";
import type { AppNotification, DesktopNotificationPayload } from "@/desktop/shared/notifications";
import type { TerminalTabShortcutCommand } from "@/desktop/shared/terminalTabs";
import type { AiUsageProvider } from "@/lib/aiUsage/types";

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
      focusExistingInternalRoute?: (route: string) => Promise<boolean>;
      onBoardEvent: (listener: (event: BoardEventPayload) => void) => () => void;
      /** tabId는 terminal 세션에서만 탭 식별자를 담고, tmux·zellij 세션에서는 null이다 */
      openTerminal: (taskId: string, tabId: string | null, cols: number, rows: number) => Promise<{ ok: boolean; error?: string }>;
      writeTerminal: (taskId: string, tabId: string | null, data: string) => void;
      resizeTerminal: (taskId: string, tabId: string | null, cols: number, rows: number) => void;
      focusTerminal: (taskId: string) => void;
      closeTerminal: (taskId: string, tabId: string | null) => void;
      pasteImageToRemoteTerminal: (
        taskId: string,
        imageDataUrl: string,
      ) => Promise<{ ok: boolean; remotePath?: string; error?: string }>;
      /** Ctrl+V 등 네이티브 paste 이벤트를 만들지 않는 트리거를 위한 동기 클립보드 이미지 조회 */
      readClipboardImage: () => string | null;
      onTerminalData: (listener: (event: { taskId: string; tabId: string | null; data: string }) => void) => () => void;
      onTerminalClose: (listener: (event: { taskId: string; tabId: string | null; reason: string | null }) => void) => () => void;
      /** AI 계정 로그인 세션은 태스크에 묶이지 않으므로 계정 루트가 식별자다 */
      openAiAccountLogin?: (
        provider: AiUsageProvider,
        accountRoot: string,
        cols: number,
        rows: number,
      ) => Promise<{ ok: boolean; error?: string }>;
      writeAiAccountLogin?: (accountRoot: string, data: string) => void;
      resizeAiAccountLogin?: (accountRoot: string, cols: number, rows: number) => void;
      closeAiAccountLogin?: (accountRoot: string) => void;
      onAiAccountLoginData?: (
        listener: (event: { accountRoot: string; data: string }) => void,
      ) => () => void;
      onAiAccountLoginExit?: (
        listener: (event: { accountRoot: string; exitCode: number }) => void,
      ) => () => void;
      closeCurrentWindow?: () => void;
      onTerminalTabShortcut?: (listener: (command: TerminalTabShortcutCommand) => void) => () => void;
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
    };
  }
}

export {};
