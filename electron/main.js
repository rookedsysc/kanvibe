const fs = require("node:fs");
const http = require("node:http");
const Module = require("node:module");
const path = require("node:path");
const process = require("node:process");
const { pathToFileURL } = require("node:url");
const { app, BrowserWindow, ipcMain, Notification, session, shell } = require("electron");

const DEFAULT_LOCALE = "ko";
const RENDERER_DEV_URL = process.env.KANVIBE_RENDERER_URL || null;
const HOOK_SERVER_HOST = "localhost";
const HOOK_SERVER_PORT = 9736;
const SHOULD_USE_SOURCE_MODULES = Boolean(RENDERER_DEV_URL);
const originalResolveFilename = Module._resolveFilename;

const isHeadlessLinuxRuntime = !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY;

if (SHOULD_USE_SOURCE_MODULES) {
  require("tsx/cjs");
}

if (process.platform === "linux") {
  app.disableHardwareAcceleration();
  app.commandLine.appendSwitch("disable-gpu");
  if (process.env.CI || isHeadlessLinuxRuntime) {
    app.commandLine.appendSwitch("no-sandbox");
  }
}

let mainWindow = null;
let hookServer = null;

function broadcastNotificationsChanged() {
  for (const window of BrowserWindow.getAllWindows()) {
    if (!window.isDestroyed()) {
      window.webContents.send("kanvibe:notifications-changed");
    }
  }
}

function getBuildMainRoot() {
  return path.join(app.getAppPath(), "build", "main", "src");
}

function getRuntimeModulePath(relativePath) {
  if (SHOULD_USE_SOURCE_MODULES) {
    return path.join(app.getAppPath(), relativePath);
  }

  return path.join(app.getAppPath(), "build", "main", relativePath.replace(/\.ts$/, ".js"));
}

function registerRuntimeAliases() {
  if (SHOULD_USE_SOURCE_MODULES || Module._resolveFilename !== originalResolveFilename) {
    return;
  }

  Module._resolveFilename = function resolveWithBuildAliases(request, parent, isMain, options) {
    if (request.startsWith("@/")) {
      const aliasedRequest = path.join(getBuildMainRoot(), request.slice(2));
      return originalResolveFilename.call(this, aliasedRequest, parent, isMain, options);
    }

    return originalResolveFilename.call(this, request, parent, isMain, options);
  };
}

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

function getDefaultRoute() {
  return `/${DEFAULT_LOCALE}/login`;
}

function getRendererNavigationUrl(target = getDefaultRoute()) {
  if (target.startsWith("http://") || target.startsWith("https://") || target.startsWith("file://")) {
    return target;
  }

  const normalizedTarget = target.startsWith("/") ? target : `/${target}`;

  if (RENDERER_DEV_URL) {
    const rendererUrl = new URL(RENDERER_DEV_URL);
    rendererUrl.hash = normalizedTarget;
    return rendererUrl.href;
  }

  return `${pathToFileURL(getRendererEntryPath()).href}#${normalizedTarget}`;
}

function getTitleBarOptions() {
  if (process.platform === "darwin") {
    return {
      titleBarStyle: "hiddenInset",
    };
  }

  return {
    titleBarStyle: "hidden",
    titleBarOverlay: {
      color: "#ffffff",
      symbolColor: "#111827",
      height: 40,
    },
  };
}

function createBrowserWindowOptions() {
  return {
    width: 1600,
    height: 1000,
    minWidth: 1200,
    minHeight: 800,
    backgroundColor: "#ffffff",
    autoHideMenuBar: true,
    ...getTitleBarOptions(),
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
    },
  };
}

function isKanvibeUrl(targetUrl) {
  if (targetUrl.startsWith("file://")) {
    return true;
  }

  if (!RENDERER_DEV_URL) {
    return false;
  }

  try {
    const parsedUrl = new URL(targetUrl);
    return parsedUrl.origin === new URL(RENDERER_DEV_URL).origin;
  } catch {
    return false;
  }
}

function getNotificationIconPath() {
  return path.join(app.getAppPath(), "public", "icons", "icon-192x192.png");
}

async function focusMainWindow(relativePath) {
  if (!mainWindow || mainWindow.isDestroyed()) {
    await createMainWindow();
  }

  if (!mainWindow) {
    return;
  }

  if (mainWindow.isMinimized()) {
    mainWindow.restore();
  }

  mainWindow.show();
  mainWindow.focus();

  if (!relativePath) {
    return;
  }

  const targetUrl = getRendererNavigationUrl(relativePath);
  if (mainWindow.webContents.getURL() !== targetUrl) {
    await mainWindow.loadURL(targetUrl);
  }
}

function registerNotificationHandlers() {
  const { createNotification, listNotifications, markAllNotificationsRead, markNotificationRead } = require(getRuntimeModulePath(path.join("src", "desktop", "main", "notificationStore.ts")));

  ipcMain.handle("kanvibe:show-notification", async (_event, payload) => {
    const { created, notification: appNotification } = await createNotification(payload);

    if (created) {
      broadcastNotificationsChanged();
    }

    if (!created || !Notification.isSupported()) {
      return created;
    }

    const notification = new Notification({
      title: appNotification.title,
      body: appNotification.body,
      icon: getNotificationIconPath(),
    });

    notification.on("click", () => {
      void markNotificationRead(appNotification.id).then(() => {
        broadcastNotificationsChanged();
      });
      void focusMainWindow(appNotification.relativePath);
    });

    notification.show();
    return true;
  });

  ipcMain.handle("kanvibe:notifications-list", async () => {
    return listNotifications();
  });

  ipcMain.handle("kanvibe:notifications-mark-read", async (_event, notificationId) => {
    const notification = await markNotificationRead(notificationId);
    broadcastNotificationsChanged();
    return notification;
  });

  ipcMain.handle("kanvibe:notifications-mark-all-read", async () => {
    await markAllNotificationsRead();
    broadcastNotificationsChanged();
  });
}

function attachWindowHandlers(browserWindow) {
  browserWindow.webContents.setWindowOpenHandler(({ url }) => {
    if (isKanvibeUrl(url)) {
      return {
        action: "allow",
        overrideBrowserWindowOptions: createBrowserWindowOptions(),
      };
    }

    void shell.openExternal(url);
    return { action: "deny" };
  });

  browserWindow.webContents.on("did-create-window", (childWindow) => {
    attachWindowHandlers(childWindow);
  });

  browserWindow.webContents.on("before-input-event", (event, input) => {
    const isNewWindowShortcut =
      input.type === "keyDown" &&
      !input.isAutoRepeat &&
      !input.alt &&
      (input.control || input.meta) &&
      input.key.toLowerCase() === "n";

    if (!isNewWindowShortcut) {
      return;
    }

    event.preventDefault();

    const currentUrl = browserWindow.webContents.getURL() || getRendererNavigationUrl();
    void createAppWindow(currentUrl);
  });
}

async function waitForServer(url, retries = 80) {
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
  const { desktopServices } = require(getRuntimeModulePath(path.join("src", "desktop", "main", "serviceRegistry.ts")));
  const {
    openTerminal,
    writeTerminal,
    resizeTerminal,
    focusTerminal,
    closeTerminal,
    closeWindowTerminals,
  } = require(getRuntimeModulePath(path.join("src", "desktop", "main", "terminalBridge.ts")));

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
  const { subscribeToBoardEvents } = require(getRuntimeModulePath(path.join("src", "lib", "boardNotifier.ts")));

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

async function loadRenderer(window, targetUrl = getRendererNavigationUrl()) {
  if (RENDERER_DEV_URL) {
    await waitForServer(RENDERER_DEV_URL);
  } else {
    const rendererEntryPath = getRendererEntryPath();
    if (!fs.existsSync(rendererEntryPath)) {
      throw new Error(`Renderer build not found: ${rendererEntryPath}`);
    }
  }

  await window.loadURL(targetUrl);
}

async function createAppWindow(target = getRendererNavigationUrl()) {
  const browserWindow = new BrowserWindow(createBrowserWindowOptions());
  mainWindow = browserWindow;
  attachWindowHandlers(browserWindow);

  await loadRenderer(browserWindow, getRendererNavigationUrl(target));

  browserWindow.on("closed", () => {
    if (mainWindow === browserWindow) {
      mainWindow = null;
    }
  });

  return browserWindow;
}

async function createMainWindow() {
  return createAppWindow();
}

app.whenReady().then(async () => {
  ensureRuntimeEnvironment();
  registerRuntimeAliases();
  session.defaultSession.setPermissionRequestHandler((_webContents, permission, callback) => {
    callback(permission === "notifications");
  });

  registerDesktopHandlers();
  const unsubscribeBoardEvents = registerBoardEventForwarding();
  startHookServer();
  registerNotificationHandlers();

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
