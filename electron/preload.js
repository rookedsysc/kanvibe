/* eslint-disable @typescript-eslint/no-require-imports */

const { contextBridge, ipcRenderer } = require("electron");

function serializeRendererError(error) {
  if (error instanceof Error) {
    return error.stack || `${error.name}: ${error.message}`;
  }

  if (typeof error === "string") {
    return error;
  }

  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

function sendRendererLog(event, payload = {}) {
  try {
    ipcRenderer.send("kanvibe:renderer-log", { event, payload });
  } catch {
    // Logging must never break the renderer bridge.
  }
}

window.addEventListener("error", (event) => {
  sendRendererLog("preload:window-error", {
    message: event.message,
    filename: event.filename,
    lineno: event.lineno,
    colno: event.colno,
    error: serializeRendererError(event.error),
  });
});

window.addEventListener("unhandledrejection", (event) => {
  sendRendererLog("preload:unhandled-rejection", {
    reason: serializeRendererError(event.reason),
  });
});

contextBridge.exposeInMainWorld("kanvibeDesktop", {
  isDesktop: true,
  logRendererError(event, payload) {
    sendRendererLog(event, payload);
  },
  invoke(namespace, method, args) {
    return ipcRenderer.invoke("kanvibe:invoke", namespace, method, args);
  },
  focusExistingInternalRoute(route) {
    return ipcRenderer.invoke("kanvibe:focus-existing-internal-route", route);
  },
  onBoardEvent(listener) {
    const handler = (_event, payload) => listener(payload);
    ipcRenderer.on("kanvibe:board-event", handler);
    return () => {
      ipcRenderer.removeListener("kanvibe:board-event", handler);
    };
  },
  openTerminal(taskId, cols, rows) {
    return ipcRenderer.invoke("kanvibe:terminal-open", taskId, cols, rows);
  },
  writeTerminal(taskId, data) {
    ipcRenderer.send("kanvibe:terminal-write", taskId, data);
  },
  resizeTerminal(taskId, cols, rows) {
    ipcRenderer.send("kanvibe:terminal-resize", taskId, cols, rows);
  },
  focusTerminal(taskId) {
    ipcRenderer.send("kanvibe:terminal-focus", taskId);
  },
  closeTerminal(taskId) {
    ipcRenderer.send("kanvibe:terminal-close", taskId);
  },
  onTerminalData(listener) {
    const handler = (_event, payload) => listener(payload);
    ipcRenderer.on("kanvibe:terminal-data", handler);
    return () => {
      ipcRenderer.removeListener("kanvibe:terminal-data", handler);
    };
  },
  onTerminalClose(listener) {
    const handler = (_event, payload) => listener(payload);
    ipcRenderer.on("kanvibe:terminal-close", handler);
    return () => {
      ipcRenderer.removeListener("kanvibe:terminal-close", handler);
    };
  },
  showNotification(payload) {
    return ipcRenderer.invoke("kanvibe:show-notification", payload);
  },
  listNotifications() {
    return ipcRenderer.invoke("kanvibe:notifications-list");
  },
  markNotificationRead(notificationId) {
    return ipcRenderer.invoke("kanvibe:notifications-mark-read", notificationId);
  },
  markAllNotificationsRead() {
    return ipcRenderer.invoke("kanvibe:notifications-mark-all-read");
  },
  activateNotification(notificationId) {
    return ipcRenderer.invoke("kanvibe:notifications-activate", notificationId);
  },
  consumePendingNotificationActivation() {
    return ipcRenderer.invoke("kanvibe:notifications-consume-activation");
  },
  onNotificationsChanged(listener) {
    const handler = () => listener();
    ipcRenderer.on("kanvibe:notifications-changed", handler);
    return () => {
      ipcRenderer.removeListener("kanvibe:notifications-changed", handler);
    };
  },
  onNotificationActivated(listener) {
    const handler = (_event, payload) => listener(payload);
    ipcRenderer.on("kanvibe:notification-activated", handler);
    return () => {
      ipcRenderer.removeListener("kanvibe:notification-activated", handler);
    };
  },
  onNotificationShortcut(listener) {
    const handler = () => listener();
    ipcRenderer.on("kanvibe:notification-shortcut", handler);
    return () => {
      ipcRenderer.removeListener("kanvibe:notification-shortcut", handler);
    };
  },
  onCreateTaskShortcut(listener) {
    const handler = () => listener();
    ipcRenderer.on("kanvibe:create-task-shortcut", handler);
    return () => {
      ipcRenderer.removeListener("kanvibe:create-task-shortcut", handler);
    };
  },
  onTaskDetailDockShortcut(listener) {
    const handler = (_event, shortcutIndex) => listener(shortcutIndex);
    ipcRenderer.on("kanvibe:task-detail-dock-shortcut", handler);
    return () => {
      ipcRenderer.removeListener("kanvibe:task-detail-dock-shortcut", handler);
    };
  },
});
