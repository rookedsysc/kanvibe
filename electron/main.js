const fs = require("node:fs");
const http = require("node:http");
const Module = require("node:module");
const path = require("node:path");
const process = require("node:process");
const { pathToFileURL } = require("node:url");
const { app, BrowserWindow, ipcMain, session, shell } = require("electron");

const DEFAULT_LOCALE = "ko";
const RENDERER_DEV_URL = process.env.KANVIBE_RENDERER_URL || null;
const HOOK_SERVER_HOST = "0.0.0.0";
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

app.commandLine.appendSwitch("log-level", "3");

let mainWindow = null;
let hookServer = null;
let windowOpenHelpers = null;
let stopBackgroundTaskSync = null;
let pendingNotificationActivation = null;

function broadcastNotificationsChanged() {
  for (const window of BrowserWindow.getAllWindows()) {
    if (!window.isDestroyed()) {
      window.webContents.send("kanvibe:notifications-changed");
    }
  }
}

function broadcastNotificationActivated(appNotification) {
  for (const window of BrowserWindow.getAllWindows()) {
    if (!window.isDestroyed()) {
      window.webContents.send("kanvibe:notification-activated", appNotification);
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

function getWindowOpenHelpers() {
  if (!windowOpenHelpers) {
    windowOpenHelpers = require(getRuntimeModulePath(path.join("src", "desktop", "main", "windowOpen.ts")));
  }

  return windowOpenHelpers;
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
      disableBlinkFeatures: "ServiceWorker",
    },
  };
}

function normalizeNotificationLocale(locale) {
  if (typeof locale !== "string") {
    return DEFAULT_LOCALE;
  }

  if (locale.startsWith("en")) {
    return "en";
  }

  if (locale.startsWith("zh")) {
    return "zh";
  }

  return DEFAULT_LOCALE;
}

function getDesktopNotificationLocale() {
  const activeWindow = mainWindow && !mainWindow.isDestroyed()
    ? mainWindow
    : BrowserWindow.getAllWindows().find((window) => !window.isDestroyed()) || null;

  const currentUrl = activeWindow?.webContents.getURL() || "";
  const matchedLocale = currentUrl.match(/#\/([^/?#]+)/)?.[1];
  if (matchedLocale) {
    return normalizeNotificationLocale(matchedLocale);
  }

  return normalizeNotificationLocale(app.getLocale() || DEFAULT_LOCALE);
}

function createDesktopNotificationOptions() {
  return {
    onNotificationsChanged: broadcastNotificationsChanged,
    onNotificationClick: async (appNotification) => {
      await activateAppNotification(appNotification, { markAsRead: false });
    },
  };
}

function getNotificationTargetPath(appNotification) {
  return appNotification.taskId
    ? `/${appNotification.locale}/task/${appNotification.taskId}`
    : appNotification.relativePath;
}

function getAvailableWindows() {
  return BrowserWindow.getAllWindows().filter((window) => !window.isDestroyed());
}

function focusWindow(window) {
  if (!window || window.isDestroyed()) {
    return;
  }

  if (window.isMinimized()) {
    window.restore();
  }

  window.show();
  window.focus();
}

function registerAppWindow(browserWindow) {
  mainWindow = browserWindow;
  attachWindowHandlers(browserWindow);

  browserWindow.on("focus", () => {
    if (!browserWindow.isDestroyed()) {
      mainWindow = browserWindow;
    }
  });

  browserWindow.on("closed", () => {
    if (mainWindow === browserWindow) {
      mainWindow = getAvailableWindows()[0] || null;
    }
  });
}

async function focusMainWindow(relativePath) {
  const targetUrl = relativePath ? getRendererNavigationUrl(relativePath) : null;
  const availableWindows = getAvailableWindows();
  const fallbackWindow = mainWindow && !mainWindow.isDestroyed()
    ? mainWindow
    : availableWindows[0] || null;

  if (targetUrl) {
    const { resolveNavigationTargetWindow } = getWindowOpenHelpers();
    mainWindow = resolveNavigationTargetWindow({
      preferredWindow: fallbackWindow,
      targetUrl,
      rendererDevUrl: RENDERER_DEV_URL,
      openWindows: availableWindows,
      getWindowUrl: (window) => window.webContents.getURL(),
    });
  } else {
    mainWindow = fallbackWindow;
  }

  if (!mainWindow || mainWindow.isDestroyed()) {
    await createMainWindow(relativePath);
  }

  if (!mainWindow) {
    return;
  }

  focusWindow(mainWindow);

  if (!relativePath) {
    return;
  }

  if (mainWindow.webContents.getURL() !== targetUrl) {
    await mainWindow.loadURL(targetUrl);
  }
}

async function focusTaskNotificationWindow(relativePath) {
  const targetUrl = getRendererNavigationUrl(relativePath);
  const { resolveExistingNavigationTargetWindow } = getWindowOpenHelpers();
  const existingWindow = resolveExistingNavigationTargetWindow({
    targetUrl,
    rendererDevUrl: RENDERER_DEV_URL,
    openWindows: getAvailableWindows(),
    getWindowUrl: (window) => window.webContents.getURL(),
  });

  if (existingWindow) {
    mainWindow = existingWindow;
  } else {
    mainWindow = await createAppWindow(relativePath);
  }

  if (!mainWindow || mainWindow.isDestroyed()) {
    return;
  }

  focusWindow(mainWindow);

  if (mainWindow.webContents.getURL() !== targetUrl) {
    await mainWindow.loadURL(targetUrl);
  }
}

function getNotificationStore() {
  return require(getRuntimeModulePath(path.join("src", "desktop", "main", "notificationStore.ts")));
}

async function activateAppNotification(appNotification, options = {}) {
  const notificationStore = getNotificationStore();
  const markAsRead = options.markAsRead !== false;
  let activatedNotification = appNotification;

  if (markAsRead && !appNotification.isRead) {
    const updatedNotification = await notificationStore.markNotificationRead(appNotification.id);
    if (updatedNotification) {
      activatedNotification = updatedNotification;
    }
    broadcastNotificationsChanged();
  }

  pendingNotificationActivation = activatedNotification;
  const targetPath = getNotificationTargetPath(activatedNotification);
  if (activatedNotification.taskId) {
    await focusTaskNotificationWindow(targetPath);
  } else {
    await focusMainWindow(targetPath);
  }
  broadcastNotificationActivated(activatedNotification);
  return true;
}

function registerNotificationHandlers() {
  const { deliverDesktopNotification } = require(getRuntimeModulePath(path.join("src", "desktop", "main", "services", "desktopNotificationService.ts")));
  const { listNotifications, markAllNotificationsRead, markNotificationRead, getNotificationById } = getNotificationStore();

  ipcMain.handle("kanvibe:show-notification", async (_event, payload) => {
    return deliverDesktopNotification(payload, createDesktopNotificationOptions());
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

  ipcMain.handle("kanvibe:notifications-activate", async (_event, notificationId) => {
    const notification = await getNotificationById(notificationId);
    if (!notification) {
      return false;
    }

    return activateAppNotification(notification);
  });

  ipcMain.handle("kanvibe:notifications-consume-activation", async () => {
    const nextActivation = pendingNotificationActivation;
    pendingNotificationActivation = null;
    return nextActivation;
  });
}

function attachWindowHandlers(browserWindow) {
  browserWindow.webContents.setWindowOpenHandler(({ url }) => {
    const { resolveWindowOpenAction } = getWindowOpenHelpers();
    const windowOpenAction = resolveWindowOpenAction({
      targetUrl: url,
      rendererDevUrl: RENDERER_DEV_URL,
      openWindows: getAvailableWindows(),
      getWindowUrl: (window) => window.webContents.getURL(),
      excludeWindow: browserWindow,
    });

    if (windowOpenAction.type === "focus-existing") {
      mainWindow = windowOpenAction.existingWindow;
      focusWindow(windowOpenAction.existingWindow);
      return { action: "deny" };
    }

    if (windowOpenAction.type === "open-internal") {
      return {
        action: "allow",
        outlivesOpener: true,
        overrideBrowserWindowOptions: createBrowserWindowOptions(),
      };
    }

    void shell.openExternal(url);
    return { action: "deny" };
  });

  browserWindow.webContents.on("did-create-window", (childWindow) => {
    registerAppWindow(childWindow);
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

    browserWindow.webContents.send("kanvibe:create-task-shortcut");
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
  const { deliverBoardEventNotification } = require(getRuntimeModulePath(path.join("src", "desktop", "main", "services", "desktopNotificationService.ts")));

  return subscribeToBoardEvents((payload) => {
    void (async () => {
      try {
        await deliverBoardEventNotification(payload, getDesktopNotificationLocale(), createDesktopNotificationOptions());
      } catch (error) {
        console.error("[kanvibe] board event notification failed:", error);
      }

      for (const window of BrowserWindow.getAllWindows()) {
        if (!window.isDestroyed()) {
          window.webContents.send("kanvibe:board-event", payload);
        }
      }
    })();
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
  registerAppWindow(browserWindow);

  await loadRenderer(browserWindow, getRendererNavigationUrl(target));

  return browserWindow;
}

async function createMainWindow(target) {
  return createAppWindow(target);
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
  const { startBackgroundTaskSync } = require(getRuntimeModulePath(path.join("src", "desktop", "main", "services", "backgroundTaskSyncService.ts")));
  stopBackgroundTaskSync = startBackgroundTaskSync();

  app.on("activate", async () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      await createMainWindow();
    }
  });

  app.on("before-quit", () => {
    stopBackgroundTaskSync?.();
    stopBackgroundTaskSync = null;
    unsubscribeBoardEvents();
    hookServer?.close();
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    app.quit();
  }
});
