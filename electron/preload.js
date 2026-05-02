const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("kanvibeDesktop", {
  isDesktop: true,
  invoke(namespace, method, args) {
    return ipcRenderer.invoke("kanvibe:invoke", namespace, method, args);
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
});
