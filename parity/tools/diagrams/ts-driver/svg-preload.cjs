/* eslint-disable @typescript-eslint/no-require-imports */
/**
 * 기준 SVG를 받기 위한 보조 preload다. 앱의 diagramWorkerPreload.js와 같은 창구를 열되,
 * 완료 신호가 오면 그 순간의 SVG 원문을 main으로 보낸다. TS 앱을 지울 때 함께 지운다.
 */
const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("diagramWorker", {
  receive(callback) {
    ipcRenderer.once("kanvibe:diagram:render", (_event, input) => callback(input));
    ipcRenderer.send("kanvibe:diagram:ready");
  },
  complete() {
    const svg = document.querySelector("#diagram svg");
    ipcRenderer.send("kanvibe:diagram:svg", svg ? svg.outerHTML : null);
  },
});
