import { findShortcutCommandForEvent, getTaskDetailDockIndexForCommand, resolveTerminalTabCommand } from "@/desktop/shared/shortcutBindings";
import { getCurrentShortcutPlatform, isBlockedShortcutEvent, isTaskDetailRouteUrl } from "@/desktop/shared/keyboardShortcut";
import { isShortcutCaptureActive, isShortcutCaptureTarget, readShortcutBindings } from "./utils/shortcutBindings";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const desktopEvents = {
  onBoardEvent: "board-event",
  onTerminalData: "terminal-data",
  onTerminalClose: "terminal-close",
  onAiAccountLoginData: "ai-login-data",
  onAiAccountLoginExit: "ai-login-exit",
  onTerminalTabShortcut: "terminal-tab-shortcut",
  onNotificationsChanged: "notifications-changed",
  onNotificationActivated: "notification-activated",
  onNotificationShortcut: "notification-shortcut",
  onCreateTaskShortcut: "create-task-shortcut",
  onCommandPaletteShortcut: "command-palette-shortcut",
  onTaskDetailDockShortcut: "task-detail-dock-shortcut",
  onTaskDetailUsageShortcut: "task-detail-usage-shortcut",
  onShortcutBindingsChanged: "shortcut-bindings-changed",
} as const;

const nativeMethods = [
  "focusExistingInternalRoute", "openInternalRouteInNewWindow", "openTerminal", "writeTerminal",
  "resizeTerminal", "focusTerminal", "closeTerminal", "pasteImageToRemoteTerminal",
  "writeSystemClipboard", "readClipboardImage", "openAiAccountLogin", "writeAiAccountLogin",
  "resizeAiAccountLogin", "closeAiAccountLogin", "closeCurrentWindow", "showNotification",
  "listNotifications", "markNotificationRead", "markTaskNotificationsRead", "markAllNotificationsRead",
  "activateNotification", "consumePendingNotificationActivation", "notifyShortcutBindingsChanged",
  "notifyShortcutCaptureChanged",
] as const;

const fireAndForget = new Set<string>([
  "writeTerminal", "resizeTerminal", "focusTerminal", "closeTerminal", "writeAiAccountLogin",
  "resizeAiAccountLogin", "closeAiAccountLogin", "closeCurrentWindow",
  "notifyShortcutBindingsChanged", "notifyShortcutCaptureChanged",
]);

export async function initializeDesktopBridge() {
  if (!isTauri()) return;

  const bridge: Record<string, unknown> = {
    isDesktop: true,
    invoke: (namespace: string, method: string, args: unknown[]) => invoke("desktop_invoke", { namespace, method, args }),
    logRendererError: (event: string, payload: unknown) => {
      void invoke("desktop_native", { method: "logRendererError", args: [event, payload] }).catch(console.error);
    },
  };
  for (const method of nativeMethods) {
    bridge[method] = (...args: unknown[]) => {
      const result = invoke("desktop_native", { method, args });
      if (fireAndForget.has(method)) {
        void result.catch(console.error);
        return;
      }
      return result;
    };
  }

  // 화면을 그리기 전에 구독을 완료해 PTY 초기 출력을 받는다.
  const unlisten: (() => void)[] = [];
  const dispatchers = new Map<string, (payload?: unknown) => void>();
  await Promise.all(Object.entries(desktopEvents).map(async ([method, event]) => {
    const subscribers = new Set<(payload: unknown) => void>();
    dispatchers.set(method, payload => subscribers.forEach(subscriber => subscriber(payload)));
    unlisten.push(await listen(`kanvibe:${event}`, ({ payload }) => {
      for (const subscriber of subscribers) subscriber(payload);
    }));
    bridge[method] = (subscriber: (payload: unknown) => void) => {
      subscribers.add(subscriber);
      return () => subscribers.delete(subscriber);
    };
  }));
  window.addEventListener("keydown", event => {
    const platform = getCurrentShortcutPlatform();
    if (isBlockedShortcutEvent(event, platform)) { event.preventDefault(); return; }
    if (isShortcutCaptureActive() || isShortcutCaptureTarget(event.target) || event.isComposing) return;
    const command = findShortcutCommandForEvent(readShortcutBindings(), event, platform);
    if (!command) return;
    const isTask = isTaskDetailRouteUrl(window.location.href);
    const dock = isTask ? getTaskDetailDockIndexForCommand(command) : null;
    const tab = resolveTerminalTabCommand(command, isTask);
    const method = command === "boardNotification" ? "onNotificationShortcut"
      : command === "createTask" ? "onCreateTaskShortcut"
      : command === "commandPalette" ? "onCommandPaletteShortcut"
      : dock !== null ? "onTaskDetailDockShortcut"
      : command === "taskDetailUsage" && isTask ? "onTaskDetailUsageShortcut"
      : tab ? "onTerminalTabShortcut" : null;
    if (command === "newWindow") {
      event.preventDefault(); event.stopImmediatePropagation();
      void invoke("desktop_native", { method: "createNewWindow", args: [window.location.hash.slice(1) || "/"] }).catch(console.error);
    } else if (method) {
      event.preventDefault(); event.stopImmediatePropagation();
      dispatchers.get(method)?.(dock ?? tab ?? undefined);
    }
  }, { capture: true });
  window.kanvibeDesktop = bridge as unknown as Window["kanvibeDesktop"];
  window.addEventListener("pagehide", () => unlisten.forEach(dispose => dispose()), { once: true });

  window.open = ((url?: string | URL) => {
    if (!url) return null;
    const target = new URL(String(url), window.location.href);
    const internal = target.origin === window.location.origin && target.pathname === window.location.pathname;
    void invoke("desktop_native", {
      method: internal ? "openInternalRouteInNewWindow" : "openExternal",
      args: [internal ? target.hash.slice(1) : target.href],
    }).catch(console.error);
    return null;
  }) as typeof window.open;

  document.addEventListener("mousedown", event => {
    if (event.button !== 0 || !(event.target instanceof Element)) return;
    let target: Element | null = event.target;
    while (target) {
      if (target.classList.contains("[-webkit-app-region:no-drag]")) return;
      if (target.classList.contains("[-webkit-app-region:drag]")) {
        event.preventDefault();
        void invoke("desktop_native", {
          method: event.detail === 2 ? "toggleWindowMaximize" : "startWindowDragging", args: [],
        }).catch(console.error);
        return;
      }
      target = target.parentElement;
    }
  });

  document.addEventListener("click", event => {
    const target = event.target instanceof Element ? event.target.closest<HTMLAnchorElement>("a[href]") : null;
    if (target && /^https?:/.test(target.href)) {
      event.preventDefault();
      void invoke("desktop_native", { method: "openExternal", args: [target.href] }).catch(console.error);
    }
  });
}
