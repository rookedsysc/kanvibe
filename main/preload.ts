import { contextBridge, ipcRenderer, type IpcRendererEvent } from "electron";

/**
 * Electron main process와 renderer process 사이의 IPC 브릿지.
 * contextBridge로 안전하게 노출하여 renderer에서 window.ipc로 접근한다.
 */
const handler = {
  invoke(channel: string, ...args: unknown[]): Promise<unknown> {
    return ipcRenderer.invoke(channel, ...args);
  },
  send(channel: string, ...args: unknown[]): void {
    ipcRenderer.send(channel, ...args);
  },
  on(channel: string, callback: (...args: unknown[]) => void): () => void {
    const subscription = (_event: IpcRendererEvent, ...args: unknown[]) =>
      callback(...args);
    ipcRenderer.on(channel, subscription);

    return () => {
      ipcRenderer.removeListener(channel, subscription);
    };
  },
};

contextBridge.exposeInMainWorld("ipc", handler);
console.log("[preload] window.ipc exposed successfully");

export type IpcHandler = typeof handler;
