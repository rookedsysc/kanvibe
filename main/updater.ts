import { autoUpdater } from "electron-updater";
import type { BrowserWindow } from "electron";
import { ipcMain } from "electron";

/**
 * electron-updater 자동 업데이트를 설정한다.
 * 업데이트 상태를 IPC 이벤트로 렌더러에 전달한다.
 */
export function setupAutoUpdater(mainWindow: BrowserWindow): void {
  autoUpdater.autoDownload = true;
  autoUpdater.autoInstallOnAppQuit = true;

  autoUpdater.on("checking-for-update", () => {
    mainWindow.webContents.send("updater:checking");
  });

  autoUpdater.on("update-available", (info) => {
    mainWindow.webContents.send("updater:available", info);
  });

  autoUpdater.on("update-not-available", () => {
    mainWindow.webContents.send("updater:not-available");
  });

  autoUpdater.on("download-progress", (progress) => {
    mainWindow.webContents.send("updater:progress", progress);
  });

  autoUpdater.on("update-downloaded", (info) => {
    mainWindow.webContents.send("updater:downloaded", info);
  });

  autoUpdater.on("error", (error) => {
    mainWindow.webContents.send("updater:error", error.message);
  });

  /** 앱 시작 후 5초 뒤에 업데이트를 확인한다 */
  setTimeout(() => {
    autoUpdater.checkForUpdatesAndNotify().catch(console.error);
  }, 5000);
}

/** 수동 업데이트 확인 */
ipcMain.handle("updater:checkForUpdates", async () => {
  const result = await autoUpdater.checkForUpdatesAndNotify();
  return result?.updateInfo ?? null;
});

/** 다운로드된 업데이트를 설치하고 앱을 재시작한다 */
ipcMain.handle("updater:quitAndInstall", () => {
  autoUpdater.quitAndInstall();
});
