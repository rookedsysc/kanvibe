#!/usr/bin/env node

const { execFileSync, spawn } = require("node:child_process");
const http = require("node:http");

const REQUIRED_NODE_MAJOR = 24;
const DEV_SERVER_URL = "http://127.0.0.1:5173";

function getNodeMajor() {
  return Number.parseInt(process.versions.node.split(".")[0] || "0", 10);
}

function hasBetterSqlite3Installed() {
  try {
    require.resolve("better-sqlite3");
    return true;
  } catch {
    return false;
  }
}

function ensureSupportedNodeVersion() {
  if (getNodeMajor() === REQUIRED_NODE_MAJOR) {
    return;
  }

  console.error(
    `[kanvibe] Unsupported Node.js runtime ${process.versions.node}. KanVibe desktop dev requires Node ${REQUIRED_NODE_MAJOR}.x.`,
  );
  console.error(`[kanvibe] Run \`nvm use ${REQUIRED_NODE_MAJOR}\` and then retry \`pnpm dev\`.`);
  process.exit(1);
}

function installElectronNativeDependencies() {
  console.warn("[kanvibe] Rebuilding native dependencies for the Electron runtime...");
  execFileSync("pnpm", ["exec", "electron-builder", "install-app-deps"], {
    stdio: "inherit",
    env: process.env,
  });
}

function installProjectDependencies() {
  console.warn("[kanvibe] Installing project dependencies because better-sqlite3 is missing...");
  execFileSync("pnpm", ["install"], {
    stdio: "inherit",
    env: process.env,
  });
}

function waitForUrl(url, retries = 80) {
  return new Promise((resolve, reject) => {
    let attempt = 0;

    const tryRequest = () => {
      const request = http.get(url, (response) => {
        response.resume();
        if (response.statusCode && response.statusCode < 500) {
          resolve();
          return;
        }

        retry();
      });

      request.on("error", retry);
      request.setTimeout(1000, () => {
        request.destroy();
        retry();
      });
    };

    const retry = () => {
      attempt += 1;
      if (attempt >= retries) {
        reject(new Error(`Dev server did not become ready on ${url}`));
        return;
      }

      setTimeout(tryRequest, 500);
    };

    tryRequest();
  });
}

function spawnViteServer() {
  return spawn("pnpm", ["exec", "vite", "--host", "127.0.0.1", "--port", "5173"], {
    stdio: "inherit",
    env: process.env,
  });
}

function spawnElectron() {
  return spawn("pnpm", ["exec", "electron", "."], {
    stdio: "inherit",
    env: {
      ...process.env,
      KANVIBE_RENDERER_URL: DEV_SERVER_URL,
    },
  });
}

async function main() {
  ensureSupportedNodeVersion();

  if (!hasBetterSqlite3Installed()) {
    installProjectDependencies();
  }

  installElectronNativeDependencies();

  const viteProcess = spawnViteServer();

  const stopChildren = () => {
    if (!viteProcess.killed) {
      viteProcess.kill("SIGTERM");
    }
  };

  process.on("SIGINT", stopChildren);
  process.on("SIGTERM", stopChildren);

  try {
    await waitForUrl(DEV_SERVER_URL);
  } catch (error) {
    stopChildren();
    throw error;
  }

  const electronProcess = spawnElectron();
  electronProcess.on("exit", (code) => {
    stopChildren();
    process.exit(code ?? 0);
  });
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
});
