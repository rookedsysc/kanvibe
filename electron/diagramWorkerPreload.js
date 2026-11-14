const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("diagramWorker", {
  receive(callback) {
    ipcRenderer.once("kanvibe:diagram:render", (_event, input) => callback(input));
    ipcRenderer.send("kanvibe:diagram:ready");
  },
  complete(result) {
    ipcRenderer.send("kanvibe:diagram:complete", result);
  },
});
