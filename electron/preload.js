const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("kanvibeDesktop", {
  isDesktop: true,
  showNotification(payload) {
    return ipcRenderer.invoke("kanvibe:show-notification", payload);
  },
});
