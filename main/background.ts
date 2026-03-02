import path from "path";
import { app, ipcMain } from "electron";
import serve from "electron-serve";
import { createWindow } from "./helpers/create-window";
import { setupDatabase } from "./database";
import { registerKanbanHandlers } from "./ipc/kanban";
import { registerProjectHandlers } from "./ipc/project";
import { registerAppSettingsHandlers } from "./ipc/appSettings";
import { registerPaneLayoutHandlers } from "./ipc/paneLayout";
import { registerDiffHandlers } from "./ipc/diff";
import { startTerminalServer } from "./ipc/terminal";
import { startHooksServer } from "./ipc/hooks";
import { setupAutoUpdater } from "./updater";

const isProd = app.isPackaged;

if (isProd) {
  serve({ directory: "out" });
} else {
  app.setPath("userData", `${app.getPath("userData")} (development)`);
}

(async () => {
  await app.whenReady();

  /** DB 초기화를 윈도우 생성 전에 수행한다 */
  await setupDatabase();

  /** IPC 핸들러를 등록한다 */
  registerKanbanHandlers();
  registerProjectHandlers();
  registerAppSettingsHandlers();
  registerPaneLayoutHandlers();
  registerDiffHandlers();

  /** 터미널 WebSocket 서버를 시작한다 */
  const wsPort = parseInt(process.env.PORT || "4885", 10) - 1;
  startTerminalServer(wsPort);

  /** 외부 AI 에이전트용 hooks HTTP 서버를 시작한다 */
  const hooksPort = parseInt(process.env.PORT || "4885", 10);
  startHooksServer(hooksPort);

  /** 메인 윈도우를 생성한다 */
  const preloadPath = path.join(__dirname, "preload.js");

  const mainWindow = createWindow("main", {
    width: 1400,
    height: 900,
    titleBarStyle: "hiddenInset",
    titleBarOverlay: {
      color: "#F8F9FA",
      symbolColor: "#1F2937",
      height: 36,
    },
    webPreferences: {
      preload: preloadPath,
      sandbox: false,
    },
  });

  if (isProd) {
    await mainWindow.loadURL("app://./home");
  } else {
    const port = process.argv[2] || "8888";
    await mainWindow.loadURL(`http://localhost:${port}/`);
    mainWindow.webContents.openDevTools();
  }

  /** 프로덕션 빌드에서만 자동 업데이트를 활성화한다 */
  if (isProd) {
    setupAutoUpdater(mainWindow);
  }
})();

app.on("window-all-closed", () => {
  app.quit();
});

/** 렌더러에서 터미널 WebSocket 포트를 조회할 수 있도록 한다 */
ipcMain.handle("app:getWsPort", () => {
  const port = parseInt(process.env.PORT || "4885", 10);
  return port - 1;
});

/** 렌더러에서 hooks HTTP 서버 포트를 조회할 수 있도록 한다 */
ipcMain.handle("app:getHooksPort", () => {
  return parseInt(process.env.PORT || "4885", 10);
});

/** SSH 설정에서 사용 가능한 호스트 목록을 반환한다 */
ipcMain.handle("app:getAvailableHosts", async () => {
  const { getAvailableHosts } = await import("@/lib/sshConfig");
  return getAvailableHosts();
});
