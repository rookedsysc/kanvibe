const http = require("node:http");
const path = require("node:path");
const process = require("node:process");
const { app, BrowserWindow, session, shell } = require("electron");

const PORT = process.env.PORT || "4885";
const DEFAULT_LOCALE = process.env.KANVIBE_LOCALE || "ko";
const isHeadlessLinuxRuntime = !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY;

if (process.platform === "linux") {
  app.disableHardwareAcceleration();
  app.commandLine.appendSwitch("disable-gpu");
  if (process.env.CI || isHeadlessLinuxRuntime) {
    app.commandLine.appendSwitch("no-sandbox");
  }
}

let mainWindow = null;
let serverBootstrapped = false;

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
  try {
    const parsedUrl = new URL(targetUrl);
    return parsedUrl.origin === `http://127.0.0.1:${PORT}`;
  } catch {
    return false;
  }
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

  throw new Error(`KanVibe server did not become ready on ${url}`);
}

function bootstrapInternalServer() {
  if (serverBootstrapped) {
    return;
  }

  const appRoot = app.getAppPath();
  process.chdir(appRoot);
  process.env.PORT = PORT;
  process.env.KANVIBE_DESKTOP = "true";
  process.env.KANVIBE_HOST = "127.0.0.1";
  process.env.KANVIBE_APP_DATA_DIR = app.getPath("userData");
  process.env.KANVIBE_SEED_DB_PATH = app.isPackaged
    ? path.join(process.resourcesPath, "database", "app.seed.db")
    : path.join(appRoot, "resources", "database", "app.seed.db");

  if (!process.env.NODE_ENV) {
    process.env.NODE_ENV = app.isPackaged ? "production" : "development";
  }

  require(path.join(appRoot, "boot.js"));
  serverBootstrapped = true;
}

async function createMainWindow() {
  bootstrapInternalServer();

  const startUrl = `http://127.0.0.1:${PORT}/${DEFAULT_LOCALE}/login`;
  await waitForServer(startUrl);

  mainWindow = new BrowserWindow(createBrowserWindowOptions());
  attachWindowHandlers(mainWindow);

  await mainWindow.loadURL(startUrl);

  mainWindow.on("closed", () => {
    mainWindow = null;
  });
}

app.whenReady().then(async () => {
  session.defaultSession.setPermissionRequestHandler((_webContents, permission, callback) => {
    callback(permission === "notifications");
  });

  await createMainWindow();

  app.on("activate", async () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      await createMainWindow();
    }
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    app.quit();
  }
});
