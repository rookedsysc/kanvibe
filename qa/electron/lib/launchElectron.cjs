/* eslint-disable @typescript-eslint/no-require-imports */
const net = require("node:net");
const { _electron: electron } = require("@playwright/test");

function collectProcessOutput(child) {
  const output = [];
  child.stdout?.on("data", (chunk) => output.push(chunk.toString("utf8")));
  child.stderr?.on("data", (chunk) => output.push(chunk.toString("utf8")));
  return () => output.join("");
}

function parseConfiguredPort(value) {
  if (value === undefined || value === null || value === "") return null;
  const port = Number.parseInt(String(value), 10);
  if (!Number.isInteger(port) || port <= 0 || port > 65535) {
    throw new Error(`Invalid KANVIBE_QA_CDP_PORT: ${value}`);
  }
  return port;
}

function listenOnEphemeralPort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : null;
      server.close((error) => {
        if (error) reject(error);
        else if (!port) reject(new Error("Failed to allocate a QA CDP port"));
        else resolve(port);
      });
    });
  });
}

async function findAvailablePort() {
  for (let attempt = 0; attempt < 8; attempt += 1) {
    const port = await listenOnEphemeralPort();
    if (port !== 19337) return port;
  }
  return listenOnEphemeralPort();
}

async function resolveCdpPort(options = {}) {
  const env = options.env || process.env;
  const configured = parseConfiguredPort(options.cdpPort ?? env.KANVIBE_QA_CDP_PORT);
  if (configured) return configured;
  return findAvailablePort();
}

function resolveElectronExecutable(options = {}) {
  if (options.executablePath) return options.executablePath;
  if (process.env.KANVIBE_QA_ELECTRON_EXECUTABLE) return process.env.KANVIBE_QA_ELECTRON_EXECUTABLE;
  return require("electron");
}

function buildElectronArgs(rootDir, cdpPort, options = {}) {
  if (options.args) return options.args;

  const args = [
    `--remote-debugging-port=${cdpPort}`,
    "--no-sandbox",
  ];

  if (options.passRootDir !== false) {
    args.push(rootDir);
  }

  return args;
}

async function launchKanVibeElectron(options = {}) {
  const rootDir = options.rootDir || process.cwd();
  const cdpPort = await resolveCdpPort(options);
  let app;

  try {
    app = await electron.launch({
      executablePath: resolveElectronExecutable(options),
      args: buildElectronArgs(rootDir, cdpPort, options),
      cwd: rootDir,
      env: {
        ...process.env,
        CI: process.env.CI || "1",
        KANVIBE_QA_MODE: "1",
        KANVIBE_QA_OUTPUT_DIR: options.outputDir || "",
        KANVIBE_APP_DATA_DIR: options.appDataDir || process.env.KANVIBE_APP_DATA_DIR || "",
      },
      timeout: options.timeoutMs || 45000,
    });

    const child = app.process();
    const output = collectProcessOutput(child);
    const page = await app.firstWindow({ timeout: options.timeoutMs || 45000 });

    page.setDefaultTimeout(options.actionTimeoutMs || 10000);
    page.setDefaultNavigationTimeout(options.navigationTimeoutMs || 30000);
    try {
      await page.setViewportSize(options.viewport || { width: 1440, height: 960 });
    } catch {
      // Electron windows can refuse viewport changes before first paint; non-fatal.
    }

    const originalClose = app.close.bind(app);
    app.close = async () => {
      await originalClose().catch(() => {});
    };
    app.output = output;

    return { app, page, cdpPort };
  } catch (error) {
    if (app) await app.close().catch(() => {});
    throw error;
  }
}

module.exports = { launchKanVibeElectron, resolveCdpPort };
