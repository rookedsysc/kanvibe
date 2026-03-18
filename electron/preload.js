const { contextBridge } = require("electron");

contextBridge.exposeInMainWorld("kanvibeDesktop", {
  isDesktop: true,
});
