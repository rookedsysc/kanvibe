const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const process = require("node:process");
const { app, BrowserWindow, ipcMain, session } = require("electron");

require("tsx/cjs");

const RENDERER_DEV_URL = process.env.KANVIBE_RENDERER_URL || null;
const HOOK_SERVER_HOST = process.env.KANVIBE_HOOK_HOST || "127.0.0.1";
const HOOK_SERVER_PORT = Number.parseInt(process.env.PORT || "4885", 10);

const isHeadlessLinuxRuntime = !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY;

if (process.platform === "linux") {
  app.disableHardwareAcceleration();
  app.commandLine.appendSwitch("disable-gpu");
  if (process.env.CI || isHeadlessLinuxRuntime) {
    app.commandLine.appendSwitch("no-sandbox");
  }
}

let mainWindow = null;
let hookServer = null;

function ensureRuntimeEnvironment() {
  const appRoot = app.getAppPath();
  process.chdir(appRoot);
  process.env.KANVIBE_DESKTOP = "true";
  process.env.KANVIBE_HOST = HOOK_SERVER_HOST;
  process.env.PORT = String(HOOK_SERVER_PORT);
  process.env.KANVIBE_APP_DATA_DIR = app.getPath("userData");
  process.env.KANVIBE_SEED_DB_PATH = app.isPackaged
    ? path.join(process.resourcesPath, "database", "app.seed.db")
    : path.join(appRoot, "resources", "database", "app.seed.db");

  if (!process.env.NODE_ENV) {
    process.env.NODE_ENV = app.isPackaged ? "production" : "development";
  }
}

function getRendererEntryPath() {
  const appRoot = app.getAppPath();
  return path.join(appRoot, "build", "renderer", "index.html");
}

async function waitForUrl(url, retries = 80) {
  for (let attempt = 0; attempt < retries; attempt += 1) {
    const isReady = await new Promise((resolve) => {
      const request = http.get(url, (response) => {
        response.resume();
        resolve(response.statusCode && response.statusCode < 500);
      });

      request.on("error", () => resolve(false));
      request.setTimeout(1000, () => {
        request.destroy();
        resolve(false);
      });
    });

    if (isReady) {
      return;
    }

    await new Promise((resolve) => setTimeout(resolve, 500));
  }

  throw new Error(`Renderer did not become ready on ${url}`);
}

function registerDesktopHandlers() {
  const { desktopServices } = require(path.join(app.getAppPath(), "src", "desktop", "main", "serviceRegistry.ts"));
  const {
    openTerminal,
    writeTerminal,
    resizeTerminal,
    focusTerminal,
    closeTerminal,
    closeWindowTerminals,
  } = require(path.join(app.getAppPath(), "src", "desktop", "main", "terminalBridge.ts"));

  ipcMain.handle("kanvibe:invoke", async (_event, namespace, method, args) => {
    const service = desktopServices[namespace];
    if (!service) {
      throw new Error(`Unknown desktop service namespace: ${namespace}`);
    }

    const targetMethod = service[method];
    if (typeof targetMethod !== "function") {
      throw new Error(`Unknown desktop service method: ${namespace}.${method}`);
    }

    return targetMethod(...(Array.isArray(args) ? args : []));
  });

  ipcMain.handle("kanvibe:terminal-open", async (event, taskId, cols, rows) => {
    return openTerminal(event.sender, taskId, cols, rows);
  });

  ipcMain.on("kanvibe:terminal-write", (event, taskId, data) => {
    writeTerminal(event.sender.id, taskId, data);
  });

  ipcMain.on("kanvibe:terminal-resize", (event, taskId, cols, rows) => {
    resizeTerminal(event.sender.id, taskId, cols, rows);
  });

  ipcMain.on("kanvibe:terminal-focus", (_event, taskId) => {
    focusTerminal(taskId);
  });

  ipcMain.on("kanvibe:terminal-close", (event, taskId) => {
    closeTerminal(event.sender.id, taskId);
  });

  app.on("web-contents-created", (_createdEvent, webContents) => {
    webContents.once("destroyed", () => {
      closeWindowTerminals(webContents.id);
    });
  });
}

function registerBoardEventForwarding() {
  const { subscribeToBoardEvents } = require(path.join(app.getAppPath(), "src", "lib", "boardNotifier.ts"));

  return subscribeToBoardEvents((payload) => {
    for (const window of BrowserWindow.getAllWindows()) {
      if (!window.isDestroyed()) {
        window.webContents.send("kanvibe:board-event", payload);
      }
    }
  });
}

function startHookServer() {
  const { createHookServer } = require(path.join(app.getAppPath(), "electron", "hookServer.js"));
  hookServer = createHookServer({ host: HOOK_SERVER_HOST, port: HOOK_SERVER_PORT });
}

async function loadRenderer(window) {
  if (RENDERER_DEV_URL) {
    await waitForUrl(RENDERER_DEV_URL);
    await window.loadURL(RENDERER_DEV_URL);
    return;
  }

  const rendererEntryPath = getRendererEntryPath();
  if (!fs.existsSync(rendererEntryPath)) {
    throw new Error(`Renderer build not found: ${rendererEntryPath}`);
  }

  await window.loadFile(rendererEntryPath);
}

async function createMainWindow() {
  mainWindow = new BrowserWindow({
    width: 1600,
    height: 1000,
    minWidth: 1200,
    minHeight: 800,
    backgroundColor: "#ffffff",
    autoHideMenuBar: true,
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  await loadRenderer(mainWindow);

  mainWindow.on("closed", () => {
    mainWindow = null;
  });
}

app.whenReady().then(async () => {
  ensureRuntimeEnvironment();
  session.defaultSession.setPermissionRequestHandler((_webContents, permission, callback) => {
    callback(permission === "notifications");
  });

  registerDesktopHandlers();
  const unsubscribeBoardEvents = registerBoardEventForwarding();
  startHookServer();

  await createMainWindow();

  app.on("activate", async () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      await createMainWindow();
    }
  });

  app.on("before-quit", () => {
    unsubscribeBoardEvents();
    hookServer?.close();
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    app.quit();
  }
});
