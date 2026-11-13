/* eslint-disable @typescript-eslint/no-require-imports */
const net = require("node:net");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { _electron: electron } = require("@playwright/test");
const electronExecutable = require("electron");

/** 데스크탑은 hook 포트에서 이만큼 떨어진 곳에 모바일 TLS 서버를 연다. src/lib/hookEndpoint.ts와 같은 값이다 */
const MOBILE_TLS_PORT_OFFSET = 10;

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
        else if (!port) reject(new Error("Failed to allocate a QA port"));
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

function canListenOnPort(port) {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.unref();
    server.on("error", () => resolve(false));
    server.listen(port, "127.0.0.1", () => server.close(() => resolve(true)));
  });
}

/** 설치된 앱(9736)이나 pnpm dev(19736)와 겹치지 않고, 병렬 QA끼리도 부딪히지 않게 실행마다 빈 포트를 고른다 */
async function findAvailableHookServerPort() {
  for (let attempt = 0; attempt < 8; attempt += 1) {
    const port = await listenOnEphemeralPort();
    const mobileTlsPort = port + MOBILE_TLS_PORT_OFFSET;
    if (mobileTlsPort <= 65535 && await canListenOnPort(mobileTlsPort)) return port;
  }
  throw new Error("Failed to allocate a QA hook server port with a free mobile TLS port");
}

async function resolveHookServerPort(options = {}) {
  return options.hookPort ?? findAvailableHookServerPort();
}

async function resolveCdpPort(options = {}) {
  const env = options.env || process.env;
  const configured = parseConfiguredPort(options.cdpPort ?? env.KANVIBE_QA_CDP_PORT);
  if (configured) return configured;
  return findAvailablePort();
}

async function launchKanVibeElectron(options = {}) {
  const rootDir = options.rootDir || process.cwd();
  const cdpPort = await resolveCdpPort(options);
  const hookPort = await resolveHookServerPort(options);
  const appDataDir = options.appDataDir || fs.mkdtempSync(path.join(os.tmpdir(), "kanvibe-electron-qa-"));
  let app;

  try {
    app = await electron.launch({
      executablePath: electronExecutable,
      args: [
        `--remote-debugging-port=${cdpPort}`,
        `--kanvibe-hook-port=${hookPort}`,
        "--no-sandbox",
        rootDir,
      ],
      cwd: rootDir,
      recordVideo: options.recordVideo,
      env: {
        ...process.env,
        CI: process.env.CI || "1",
        KANVIBE_QA_MODE: "1",
        KANVIBE_QA_OUTPUT_DIR: options.outputDir || "",
        KANVIBE_APP_DATA_DIR: appDataDir,
        KANVIBE_DB_PATH: path.join(appDataDir, "kanvibe.db"),
        ...(options.rendererUrl ? { KANVIBE_RENDERER_URL: options.rendererUrl } : {}),
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

    return { app, page, cdpPort, hookPort };
  } catch (error) {
    if (app) await app.close().catch(() => {});
    throw error;
  }
}

module.exports = { launchKanVibeElectron, resolveCdpPort, resolveHookServerPort };
