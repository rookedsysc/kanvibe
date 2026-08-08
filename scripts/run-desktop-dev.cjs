#!/usr/bin/env node

const { execFileSync, spawn } = require("node:child_process");
const http = require("node:http");
const net = require("node:net");

const REQUIRED_NODE_MAJOR = 24;
const DEV_SERVER_HOST = "127.0.0.1";
const PREFERRED_DEV_SERVER_PORT = 5173;
const VITE_DEV_SERVER_ENV = {
  CHOKIDAR_USEPOLLING: process.env.CHOKIDAR_USEPOLLING ?? "true",
};

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

function canListenOnPort(port) {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.unref();
    server.on("error", () => resolve(false));
    server.listen(port, DEV_SERVER_HOST, () => {
      server.close(() => resolve(true));
    });
  });
}

function listenOnEphemeralPort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.on("error", reject);
    server.listen(0, DEV_SERVER_HOST, () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : null;
      server.close((error) => {
        if (error) reject(error);
        else if (!port) reject(new Error("Failed to allocate a dev server port"));
        else resolve(port);
      });
    });
  });
}

/**
 * vite는 요청한 포트가 막혀 있으면 조용히 다른 포트로 넘어간다.
 * 그러면 Electron은 그 사실을 모른 채 원래 포트에 있는 남의 페이지를 열어 흰 화면이 된다.
 * 포트를 먼저 확정해 vite와 Electron에 같은 값을 넘긴다.
 */
async function resolveDevServerPort(preferredPort = PREFERRED_DEV_SERVER_PORT) {
  if (await canListenOnPort(preferredPort)) {
    return preferredPort;
  }

  const fallbackPort = await listenOnEphemeralPort();
  console.warn(
    `[kanvibe] Port ${preferredPort} is already in use. Starting the dev server on ${fallbackPort} instead.`,
  );
  return fallbackPort;
}

function spawnViteServer(port) {
  /** --strictPort가 없으면 위에서 잡은 포트를 vite가 다시 갈아치울 수 있다 */
  return spawn("pnpm", ["exec", "vite", "--host", DEV_SERVER_HOST, "--port", String(port), "--strictPort"], {
    stdio: "inherit",
    env: {
      ...process.env,
      ...VITE_DEV_SERVER_ENV,
    },
  });
}

function spawnElectron(devServerUrl) {
  return spawn("pnpm", ["exec", "electron", "--no-sandbox", "."], {
    stdio: "inherit",
    env: {
      ...process.env,
      KANVIBE_RENDERER_URL: devServerUrl,
    },
  });
}

/** 포트를 뺏겨 vite가 즉시 죽으면 준비 대기를 끝까지 돌리지 않고 이유를 그대로 알린다 */
function rejectWhenProcessExits(child, label) {
  return new Promise((_resolve, reject) => {
    child.on("exit", (code) => {
      reject(new Error(`${label} exited with code ${code} before the renderer was ready`));
    });
  });
}

async function main() {
  ensureSupportedNodeVersion();

  if (!hasBetterSqlite3Installed()) {
    installProjectDependencies();
  }

  const devServerPort = await resolveDevServerPort();
  const devServerUrl = `http://${DEV_SERVER_HOST}:${devServerPort}`;
  const viteProcess = spawnViteServer(devServerPort);

  const stopChildren = () => {
    if (!viteProcess.killed) {
      viteProcess.kill("SIGTERM");
    }
  };

  process.on("SIGINT", stopChildren);
  process.on("SIGTERM", stopChildren);

  const viteExited = rejectWhenProcessExits(viteProcess, "Vite dev server");
  viteExited.catch(() => {});

  try {
    await Promise.race([waitForUrl(devServerUrl), viteExited]);
  } catch (error) {
    stopChildren();
    throw error;
  }

  const electronProcess = spawnElectron(devServerUrl);
  electronProcess.on("exit", (code) => {
    stopChildren();
    process.exit(code ?? 0);
  });
}

if (require.main === module) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  });
}

module.exports = { resolveDevServerPort };
